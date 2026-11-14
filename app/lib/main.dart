import 'dart:async';
import 'dart:io';
import 'dart:ui' show AppExitResponse;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'design/components.dart';
import 'design/theme.dart';
import 'design/tokens.dart';
import 'engine/engine.dart';
import 'engine/fake.dart';
import 'engine/native.dart';
import 'gaze.dart';
import 'library.dart';
import 'screens/app_menu.dart';
import 'screens/canvas.dart';
import 'screens/doc.dart';
import 'screens/editor.dart';
import 'screens/home.dart';
import 'screens/interview.dart';
import 'screens/dialogs.dart';
import 'screens/paper.dart' show filesChannel;
import 'screens/report.dart';
import 'screens/settings.dart';

/// 앱의 화면. 옆 메뉴의 순서이기도 하다.
enum AppRoute {
  home('홈', Icons.home_outlined),
  editor('문서', Icons.description_outlined),
  interview('면접', Icons.forum_outlined),
  report('리포트', Icons.checklist),
  settings('설정', Icons.settings_outlined);

  const AppRoute(this.label, this.icon);
  final String label;
  final IconData icon;
}

void main() {
  // 개발과 QA용 시작 옵션: APP_ROUTE=home|editor|interview|report|settings, APP_THEME=light|dark,
  // APP_STATE=sample(견본 자료로 그린 화면. 면접과 리포트는 가짜 엔진의 흐름), APP_STATE=first(견본의 첫 실행 상태).
  // 자료 폴더는 KARI_HOME으로 바꾼다.
  final env = Platform.environment;
  final sample = env['APP_STATE'] == 'sample' || env['APP_STATE'] == 'first';
  final route = AppRoute.values.asNameMap()[env['APP_ROUTE']] ?? AppRoute.home;
  final mode = switch (env['APP_THEME']) {
    'light' => ThemeMode.light,
    'dark' => ThemeMode.dark,
    _ => ThemeMode.system,
  };
  runApp(
    App(
      initialRoute: route,
      themeMode: mode,
      firstRun: env['APP_STATE'] == 'first',
      library: sample
          ? null
          : (Library(NativeEngine(), libraryDir(env, Platform.operatingSystem))
              ..camera = ChannelGazeCamera()),
    ),
  );
}

class App extends StatelessWidget {
  const App({
    super.key,
    this.initialRoute = AppRoute.home,
    this.themeMode = ThemeMode.system,
    this.firstRun = false,
    this.library,
    this.menuBar,
  });

  /// macOS 메뉴 막대를 붙이는지. 정하지 않으면 macOS 앱에서만 붙인다(위젯 시험에는 메뉴 막대가 없다).
  final bool? menuBar;

  final AppRoute initialRoute;
  final ThemeMode themeMode;

  /// 견본에서 문서와 공고가 하나도 없는 첫 실행 상태로 띄운다.
  final bool firstRun;

  /// 엔진의 자료. 없으면 견본 자료로 그린다(디자인 시험, APP_STATE=sample).
  final Library? library;

  @override
  Widget build(BuildContext context) {
    final lib = library;
    final menus =
        menuBar ??
        (defaultTargetPlatform == TargetPlatform.macOS &&
            !Platform.environment.containsKey('FLUTTER_TEST'));
    return MaterialApp(
      title: 'Kari',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(Brightness.light),
      darkTheme: buildTheme(Brightness.dark),
      themeMode: themeMode,
      home: lib == null
          ? Shell(
              initialRoute: initialRoute,
              firstRun: firstRun,
              menuBar: menus,
            )
          : _Start(library: lib, initialRoute: initialRoute, menuBar: menus),
    );
  }
}

/// 시작. 라이브러리를 여는 동안 빈 바탕을 두고, 200ms가 넘으면 "불러오는 중입니다"를 보인다.
/// 열리면 화면으로, 실패하면 오류와 다시 시도 단추를 보인다.
class _Start extends StatefulWidget {
  const _Start({
    required this.library,
    required this.initialRoute,
    required this.menuBar,
  });

  final Library library;
  final AppRoute initialRoute;
  final bool menuBar;

  @override
  State<_Start> createState() => _StartState();
}

class _StartState extends State<_Start> {
  late final AppLifecycleListener _exit;

  @override
  void initState() {
    super.initState();
    _exit = AppLifecycleListener(onExitRequested: _onExit);
    widget.library.open();
  }

  /// 앱을 끌 때(⌘Q, 그리고 macOS 쪽이 끄기 요청으로 바꾸는 창 닫기). 열린 편집 화면의 조합 중인 글과 저장하지 못한 편집을 보내고 라이브러리를 닫는다(엔진이
  /// 열린 문서마다 "문서를 닫을 때" 버전을 남긴다). 그래도 저장하지 못한 것이 있으면 끄기를 멈추고 묻는다.
  Future<AppExitResponse> _onExit() async {
    final lib = widget.library;
    if (await lib.shutdown()) return AppExitResponse.exit;
    if (!mounted || !await confirmDiscard(context, '지금 끄면', '저장하지 않고 끄기')) {
      return AppExitResponse.cancel;
    }
    await lib.shutdown(force: true);
    return AppExitResponse.exit;
  }

  @override
  void dispose() {
    _exit.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final lib = widget.library;
    return ListenableBuilder(
      listenable: lib,
      builder: (context, _) => switch (lib.phase) {
        LibraryPhase.opening => const Scaffold(body: _Loading()),
        LibraryPhase.failed => Scaffold(
          body: _OpenFailed(
            error: lib.openError!,
            dir: lib.dir,
            onRetry: lib.open,
            backups: lib.backups,
            onRestore: lib.restoreBackup,
            now: lib.now(),
          ),
        ),
        LibraryPhase.ready => Shell(
          initialRoute: widget.initialRoute,
          library: lib,
          menuBar: widget.menuBar,
        ),
      },
    );
  }
}

class _Loading extends StatefulWidget {
  const _Loading();

  @override
  State<_Loading> createState() => _LoadingState();
}

class _LoadingState extends State<_Loading> {
  late final Timer _timer;
  bool _show = false;

  @override
  void initState() {
    super.initState();
    _timer = Timer(
      const Duration(milliseconds: 200),
      () => setState(() => _show = true),
    );
  }

  @override
  void dispose() {
    _timer.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!_show) return const SizedBox.expand();
    return Center(
      child: Text(
        '불러오는 중입니다',
        style: AppText.body.copyWith(color: AppColors.of(context).textMuted),
      ),
    );
  }
}

/// 자료 폴더를 열지 못했을 때 사람이 다음에 할 일.
String openAdvice(String code) => switch (code) {
  'too_new' => '이 자료는 더 새 판의 Kari가 만들었습니다. Kari를 새 판으로 바꾼 뒤 다시 열어 주세요.',
  'corrupt' ||
  'integrity' => '자료 폴더를 다른 곳에 복사해 두세요. 그 안의 backups 폴더에 지난 사본이 있을 수 있습니다.',
  _ => '다시 시도해도 같으면 자료 폴더를 다른 곳에 복사해 두세요. 그 안의 backups 폴더에 지난 사본이 있을 수 있습니다.',
};

/// 자료 폴더를 Finder로 연다(`kari/files` 채널의 `openFolder`). 열었으면 true.
Future<bool> openFolder(String path) async {
  try {
    return await filesChannel.invokeMethod<bool>('openFolder', {
          'path': path,
        }) ==
        true;
  } on PlatformException catch (e) {
    FlutterError.reportError(
      FlutterErrorDetails(exception: e, library: 'kari 시작'),
    );
  } on MissingPluginException {
    // 채널이 없는 곳(채널을 끼우지 않은 위젯 시험)에서는 열지 않는다.
  }
  return false;
}

/// 저장하지 못한 편집이 남았을 때 묻는다. [when](지금 끄면, 이 화면을 떠나면)에 그 편집이 사라진다고 알리고,
/// [confirm]을 고르면 참. 취소하면 편집 화면의 "저장하지 못했습니다"와 다시 시도가 그대로 남는다.
Future<bool> confirmDiscard(
  BuildContext context,
  String when,
  String confirm,
) async =>
    await showDialog<bool>(
      context: context,
      builder: (_) => ConfirmDialog(
        title: '저장하지 못한 편집이 있습니다',
        message: '$when 저장하지 못한 편집은 사라집니다. 취소한 뒤 위쪽 줄의 다시 시도로 저장할 수 있습니다.',
        confirm: confirm,
        danger: true,
      ),
    ) ==
    true;

/// 라이브러리를 열지 못했을 때. 무엇이 안 됐는지, 오류 코드, 자료 폴더, 다음에 할 일을 보여 준다.
/// 손상이고 백업이 있으면 가장 최근 백업에서 되살릴 수 있다(되살리기 전에 한 번 묻는다).
class _OpenFailed extends StatelessWidget {
  const _OpenFailed({
    required this.error,
    required this.dir,
    required this.onRetry,
    required this.backups,
    required this.onRestore,
    required this.now,
  });

  final EngineError error;
  final String dir;
  final VoidCallback onRetry;

  /// 되살릴 수 있는 백업을 만든 시각(최근 것부터).
  final List<int> backups;
  final Future<void> Function() onRestore;
  final DateTime now;

  Future<void> _restore(BuildContext context) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => ConfirmDialog(
        title: '백업에서 되살릴까요?',
        message:
            '가장 최근 백업(${formatWhen(backups.first, now)})으로 자료를 되살립니다. '
            '그 뒤에 고친 내용은 백업에 없습니다. 지금 파일은 자료 폴더에 그대로 남겨 둡니다.',
        confirm: '백업에서 되살리기',
      ),
    );
    if (ok == true) await onRestore();
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    // 폴더 열기가 실패했는가. 아래 StatefulBuilder만 다시 그리므로 이 값이 남는다.
    var folderFailed = false;
    Widget row(String name, String value) => Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 72,
          child: Text(name, style: AppText.body.copyWith(color: c.textMuted)),
        ),
        Expanded(
          child: SelectableText(
            value,
            style: AppText.body.copyWith(color: c.text),
          ),
        ),
      ],
    );
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(Space.x8),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Icon(Icons.error, size: 20, color: c.danger),
                  const SizedBox(width: Space.x2),
                  Expanded(
                    child: Text(
                      '자료 폴더를 열지 못했습니다',
                      style: AppText.title.copyWith(color: c.text),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: Space.x4),
              if (error.message.isNotEmpty) ...[
                row('설명', error.message),
                const SizedBox(height: Space.x2),
              ],
              row('오류 코드', error.code),
              const SizedBox(height: Space.x2),
              row('자료 폴더', dir),
              const SizedBox(height: Space.x2),
              row('할 일', openAdvice(error.code)),
              if (error.code == 'corrupt' && backups.isNotEmpty) ...[
                const SizedBox(height: Space.x2),
                row('최근 백업', formatWhen(backups.first, now)),
              ],
              const SizedBox(height: Space.x6),
              StatefulBuilder(
                builder: (context, setState) => Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Wrap(
                      spacing: Space.x2,
                      runSpacing: Space.x2,
                      children: [
                        AppButton(
                          '다시 시도',
                          kind: ButtonKind.primary,
                          onPressed: onRetry,
                        ),
                        if (error.code == 'corrupt' && backups.isNotEmpty)
                          AppButton(
                            '백업에서 되살리기',
                            onPressed: () => _restore(context),
                          ),
                        AppButton(
                          '자료 폴더 열기',
                          onPressed: () async {
                            final ok = await openFolder(dir);
                            if (context.mounted) {
                              setState(() => folderFailed = !ok);
                            }
                          },
                        ),
                      ],
                    ),
                    if (folderFailed) ...[
                      const SizedBox(height: Space.x2),
                      Text(
                        '폴더를 열지 못했습니다. 위의 경로를 Finder의 "폴더로 이동"(⇧⌘G)에 붙여 넣어 여세요.',
                        style: AppText.caption.copyWith(color: c.danger),
                      ),
                    ],
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 옆 메뉴와 화면 영역.
class Shell extends StatefulWidget {
  const Shell({
    super.key,
    this.initialRoute = AppRoute.home,
    this.firstRun = false,
    this.library,
    this.menuBar = false,
  });

  final AppRoute initialRoute;
  final bool firstRun;

  /// 엔진의 자료. 없으면 견본 화면을 그린다.
  final Library? library;

  /// macOS 메뉴 막대를 붙인다.
  final bool menuBar;

  @override
  State<Shell> createState() => _ShellState();
}

class _ShellState extends State<Shell> {
  late AppRoute _route = widget.initialRoute;

  /// 문서 화면에 연 문서.
  int? _doc;

  /// 리포트 탭에서 처음 보여 줄 세션(면접 탭의 "리포트 보기").
  String? _reportId;

  /// 면접 준비에서 골라 둘 문서와 공고(홈의 "면접 연습 시작"). [_ivAgain]은 리포트의 "이 주제로 다시 연습"이
  /// 넘긴 그 연습의 기록이다(공고, 연차, 시간, 압박을 그대로 골라 둔다).
  int? _ivDoc;
  String? _ivPosting;
  Json? _ivAgain;

  /// 문서를 연 뒤 골라 보여 줄 글(홈의 "이력서에서 보기").
  String? _reveal;

  /// 편집 화면이 메뉴 막대에 문서 명령을 내는 자리.
  final _menu = MenuHost();

  /// 견본 상태(엔진 없이 그리는 디자인 시험, APP_STATE=sample)는 가짜 엔진의 자료로 돈다. 첫 실행은 빈 가짜 엔진이다.
  late final Library? _sample = widget.library == null
      ? (Library(widget.firstRun ? FakeEngine() : sampleEngine(), '/sample')
          ..open())
      : null;

  Library get _lib => widget.library ?? _sample!;

  /// 편집 화면을 떠나도 되는지. 조합 중인 글과 저장하지 못한 편집을 먼저 보내고, 그래도 저장하지 못한 것이 있으면
  /// 묻는다(편집 화면이 닫히면서 그 편집을 버리기 때문이다).
  Future<bool> _leave() async {
    final s = widget.library?.editing;
    if (s != null &&
        !await s.flush() &&
        (!mounted ||
            !await confirmDiscard(context, '이 화면을 떠나면', '저장하지 않고 떠나기'))) {
      return false;
    }
    return mounted;
  }

  Future<void> _go(AppRoute r) async {
    if (r != _route && !await _leave()) return;
    setState(() => _route = r);
    // 홈의 대시보드(하던 면접의 질문 번호 같은 것)를 다시 읽는다.
    if (r == AppRoute.home) _lib.refreshInterview();
  }

  Future<void> _open(int id, {String? reveal}) async {
    if (id != _doc && !await _leave()) return;
    _show(id, reveal: reveal);
  }

  void _show(int id, {String? reveal}) => setState(() {
    _doc = id;
    _reveal = reveal;
    _route = AppRoute.editor;
  });

  /// 새 문서를 만들고 바로 연다. 실패하면 지금 화면에 오류 한 줄이 남는다.
  /// 견본 상태는 견본 문서 화면으로 간다.
  Future<void> _create() async {
    if (widget.library == null) {
      _go(AppRoute.editor);
      return;
    }
    if (!await _leave()) return;
    final id = await widget.library!.create();
    if (id != null && mounted) _show(id);
  }

  /// 캔버스 문서를 만들고 바로 연다. 견본 상태는 견본 문서 화면으로 간다.
  Future<void> _createCanvas() async {
    if (widget.library == null) {
      _go(AppRoute.editor);
      return;
    }
    if (!await _leave()) return;
    final id = await widget.library!.createCanvas();
    if (id != null && mounted) _show(id);
  }

  void _startInterview({int? doc, String? posting, Json? again}) {
    // 마친 연습이 열려 있으면 닫는다. 닫지 않으면 준비 화면 대신 "연습을 마쳤습니다" 카드가 다시 보인다.
    if (_lib.interview.done) _lib.interview.close();
    setState(() {
      _ivDoc = doc;
      _ivPosting = posting;
      _ivAgain = again;
      _route = AppRoute.interview;
    });
  }

  /// 끝나지 않은 면접을 연다. 이미 연 면접이면 그대로 간다.
  void _resume(String id) {
    final run = _lib.interview;
    if (run.id != id) run.open(id);
    _go(AppRoute.interview);
  }

  void _showReport(String id) => setState(() {
    _reportId = id;
    _route = AppRoute.report;
  });

  Widget _interview(Library lib) => InterviewTab(
    key: ValueKey((_ivDoc, _ivPosting, _ivAgain?['id'])),
    library: lib,
    doc: _ivDoc,
    posting: _ivPosting,
    again: _ivAgain,
    onOpenSettings: () => _go(AppRoute.settings),
    onCreateDoc: _create,
    onAddPosting: _addPosting,
    onShowReport: _showReport,
  );

  Widget _report(Library lib) => ReportTab(
    key: ValueKey(_reportId),
    library: lib,
    initial: _reportId,
    onPractice: (s) => _startInterview(doc: s['doc'] as int, again: s),
    onStart: _startInterview,
  );

  Widget _home(Library lib) => LibraryHome(
    library: lib,
    // 견본 상태는 견본 문서 화면으로 간다.
    onOpen: (d, {reveal}) => widget.library == null
        ? _go(AppRoute.editor)
        : _open(d.id, reveal: reveal),
    onCreate: _create,
    onCreateCanvas: _createCanvas,
    onOpenSettings: () => _go(AppRoute.settings),
    onAddPosting: _addPosting,
    onStartInterview: _startInterview,
    onResume: _resume,
    onShowReport: _showReport,
  );

  Widget _real(Library lib) => switch (_route) {
    AppRoute.home => _home(lib),
    AppRoute.editor =>
      lib.docs.any((d) => d.id == _doc && d.kind == 'canvas')
          ? CanvasScreen(
              key: ValueKey(_doc),
              library: lib,
              docId: _doc!,
              onOpen: _open,
              menu: _menu,
            )
          : DocScreen(
              library: lib,
              docId: _doc,
              reveal: _reveal,
              onCreate: _create,
              onOpen: _open,
              menu: _menu,
              onInterview: () => _startInterview(doc: _doc),
            ),
    AppRoute.interview => _interview(lib),
    AppRoute.report => _report(lib),
    AppRoute.settings => SettingsScreen(
      library: lib,
      onInterview: _startInterview,
      onAddPosting: _addPosting,
    ),
  };

  Future<void> _addPosting() => showDialog<bool>(
    context: context,
    builder: (_) => AddPostingDialog(library: _lib),
  );

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final collapsed = MediaQuery.sizeOf(context).width < Breakpoints.wideNav;
    final page = Scaffold(
      body: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            width: collapsed ? 56 : 208,
            color: c.surface,
            padding: const EdgeInsets.symmetric(
              horizontal: Space.x2,
              vertical: Space.x3,
            ),
            child: FocusTraversalGroup(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (final r in AppRoute.values) ...[
                    // 설정은 아래쪽에 따로 둔다.
                    if (r == AppRoute.settings) const Spacer(),
                    _NavItem(
                      route: r,
                      selected: r == _route,
                      collapsed: collapsed,
                      onTap: () => _go(r),
                    ),
                    const SizedBox(height: 2),
                  ],
                ],
              ),
            ),
          ),
          VerticalDivider(width: 1, color: c.divider),
          Expanded(
            child: FocusTraversalGroup(
              child: widget.library != null
                  ? _real(widget.library!)
                  : switch (_route) {
                      AppRoute.home => _home(_lib),
                      AppRoute.editor => const EditorScreen(),
                      AppRoute.interview => _interview(_lib),
                      AppRoute.report => _report(_lib),
                      AppRoute.settings => const SettingsScreen(),
                    },
            ),
          ),
        ],
      ),
    );
    if (!widget.menuBar) return page;
    return AppMenuBar(
      host: _menu,
      app: (
        newDoc: _create,
        home: () => _go(AppRoute.home),
        settings: () => _go(AppRoute.settings),
      ),
      child: page,
    );
  }

  @override
  void dispose() {
    _menu.dispose();
    super.dispose();
  }
}

/// 옆 메뉴 항목. 고른 항목은 바탕, 굵기, 아이콘 색으로 함께 표시한다.
class _NavItem extends StatelessWidget {
  const _NavItem({
    required this.route,
    required this.selected,
    required this.collapsed,
    required this.onTap,
  });

  final AppRoute route;
  final bool selected;
  final bool collapsed;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final fg = selected ? c.accentText : c.textMuted;
    final item = FocusRing(
      child: Semantics(
        button: true,
        selected: selected,
        label: route.label,
        child: ExcludeSemantics(
          child: Material(
            color: selected ? c.accentSoft : Colors.transparent,
            borderRadius: BorderRadius.circular(Radii.control),
            child: InkWell(
              onTap: onTap,
              borderRadius: BorderRadius.circular(Radii.control),
              hoverColor: c.text.withValues(alpha: 0.06),
              focusColor: Colors.transparent,
              child: SizedBox(
                height: 36,
                child: Row(
                  mainAxisAlignment: collapsed
                      ? MainAxisAlignment.center
                      : MainAxisAlignment.start,
                  children: [
                    if (!collapsed) const SizedBox(width: Space.x3),
                    Icon(route.icon, size: 18, color: fg),
                    if (!collapsed) ...[
                      const SizedBox(width: Space.x2),
                      Text(
                        route.label,
                        style: (selected ? AppText.label : AppText.body)
                            .copyWith(color: selected ? c.accentText : c.text),
                      ),
                    ],
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
    return collapsed ? Tooltip(message: route.label, child: item) : item;
  }
}
