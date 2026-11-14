import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kari/design/theme.dart';
import 'package:kari/design/tokens.dart';
import 'package:kari/screens/settings.dart';
import 'package:kari/update.dart';

import 'design_test.dart' show loadFonts;

const _channel = MethodChannel('kari/update');

/// macOS 쪽(Updater.swift)을 흉내 낸다. 받은 호출을 모으고 `start`에는 [start]를 돌려준다.
List<MethodCall> _native(
  WidgetTester tester, {
  Map<String, Object?> start = const {'available': true, 'auto': true},
}) {
  final calls = <MethodCall>[];
  var auto = start['auto'];
  tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(_channel, (
    c,
  ) async {
    calls.add(c);
    return switch (c.method) {
      'start' => {...start, 'auto': auto},
      'setAuto' => auto = c.arguments,
      'check' => start['available'],
      _ => null,
    };
  });
  addTearDown(
    () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      _channel,
      null,
    ),
  );
  return calls;
}

/// macOS 쪽이 Dart를 부른 것처럼 한다.
Future<void> _fromNative(
  WidgetTester tester,
  String method, [
  Object? args,
]) async {
  await tester.binding.defaultBinaryMessenger.handlePlatformMessage(
    _channel.name,
    _channel.codec.encodeMethodCall(MethodCall(method, args)),
    (_) {},
  );
  await tester.pumpAndSettle();
}

Future<Updates> _host(WidgetTester tester) async {
  final u = Updates();
  await tester.pumpWidget(
    MaterialApp(
      theme: buildTheme(Brightness.light),
      home: UpdateHost(
        updates: u,
        child: const Scaffold(body: SizedBox()),
      ),
    ),
  );
  await tester.pumpAndSettle();
  return u;
}

/// plist(XML)에서 키 하나의 값: `<string>`의 글, `<integer>`의 수, `<true/>`·`<false/>`.
Object? _plist(String xml, String key) {
  final m = RegExp(
    '<key>${RegExp.escape(key)}</key>\\s*<(\\w+)(/?)>([^<]*)',
  ).firstMatch(xml);
  if (m == null) return null;
  return switch (m[1]) {
    'true' => true,
    'false' => false,
    'integer' => int.parse(m[3]!),
    _ => m[3],
  };
}

void main() {
  setUpAll(loadFonts);

  group('업데이트', () {
    testWidgets('새 판이 오면 판, 빌드, 바뀐 점과 두 단추를 보이고 "지금 업데이트"를 macOS 쪽에 넘긴다', (
      tester,
    ) async {
      final calls = _native(tester);
      final u = await _host(tester);
      expect(calls.single.method, 'start');
      expect(u.available, isTrue);

      await _fromNative(tester, 'found', {
        'version': '0.1.0',
        'build': '42',
        'notes': '표 붙여넣기를 고침',
      });
      expect(find.text('새 판이 있습니다'), findsOneWidget);
      expect(
        find.textContaining(keepAll('Kari 0.1.0 (빌드 42)')),
        findsOneWidget,
      );
      expect(find.text('표 붙여넣기를 고침'), findsOneWidget);
      expect(find.text('나중에'), findsOneWidget);

      await tester.tap(find.text('지금 업데이트'));
      await tester.pumpAndSettle();
      expect(find.text('새 판이 있습니다'), findsNothing);
      expect(calls.last.method, 'answer');
      expect(calls.last.arguments, isTrue);
      expect(find.textContaining('새 판을 받는 중입니다'), findsOneWidget);
    });

    testWidgets('"나중에"와 바깥 누르기는 설치하지 않는다', (tester) async {
      final calls = _native(tester);
      await _host(tester);
      final offer = {'version': '0.1.0', 'build': '42', 'notes': ''};

      await _fromNative(tester, 'found', offer);
      expect(find.text('바뀐 점'), findsNothing); // 바뀐 점이 비면 그 묶음을 두지 않는다
      await tester.tap(find.text('나중에'));
      await tester.pumpAndSettle();
      expect(calls.last.method, 'answer');
      expect(calls.last.arguments, isFalse);

      await _fromNative(tester, 'found', offer);
      await tester.tapAt(const Offset(4, 4));
      await tester.pumpAndSettle();
      expect(find.text('새 판이 있습니다'), findsNothing);
      expect(calls.last.method, 'answer');
      expect(calls.last.arguments, isFalse);
      expect(calls.where((c) => c.arguments == true), isEmpty);
    });

    testWidgets('받기나 설치에 실패하면 Sparkle의 까닭과 함께 실패 띠로 알리고 다시 해 볼 길을 말한다', (
      tester,
    ) async {
      _native(tester);
      await _host(tester);
      await _fromNative(tester, 'status', {
        'kind': 'error',
        'code': 3001,
        'message': 'signature',
      });
      expect(find.textContaining('업데이트하지 못했습니다'), findsOneWidget);
      expect(find.textContaining('(signature)'), findsOneWidget);
      expect(find.byIcon(Icons.error), findsOneWidget);

      await _fromNative(tester, 'status', {'kind': 'latest'});
      expect(find.text('지금 쓰는 판이 최신입니다'), findsOneWidget);
    });

    testWidgets('설치하려고 끄는 요청을 앱이 취소하면 "Kari를 끄면 업데이트가 설치됩니다"와 "다시 시도"를 보인다', (
      tester,
    ) async {
      final calls = _native(tester);
      await _host(tester);
      await _fromNative(tester, 'status', {'kind': 'waiting'});
      expect(find.text('Kari를 끄면 업데이트가 설치됩니다'), findsOneWidget);
      // 보통 알림처럼 몇 초 뒤에 사라지지 않는다.
      await tester.pump(const Duration(minutes: 1));
      expect(find.text('Kari를 끄면 업데이트가 설치됩니다'), findsOneWidget);

      await tester.tap(find.text('다시 시도'));
      await tester.pumpAndSettle();
      expect(calls.last.method, 'retry');

      // 이미 꺼진 경우(installing)에는 다시 시도를 두지 않는다.
      await _fromNative(tester, 'status', {'kind': 'installing'});
      expect(find.text('새 판을 설치하려고 Kari를 닫습니다'), findsOneWidget);
      expect(find.text('다시 시도'), findsNothing);
    });

    testWidgets('공개 키가 없는 판은 확인을 macOS 쪽에 보내지 않고 받을 곳을 알린다', (tester) async {
      final calls = _native(tester, start: {'available': false, 'auto': false});
      final u = await _host(tester);
      expect(u.available, isFalse);
      await u.check();
      await tester.pumpAndSettle();
      expect(calls.map((c) => c.method), ['start']);
      expect(find.textContaining('이 판은 앱 안에서 업데이트할 수 없습니다'), findsOneWidget);
    });

    testWidgets('"업데이트 확인"은 확인을 macOS 쪽에 부탁한다', (tester) async {
      final calls = _native(tester);
      final u = await _host(tester);
      await u.check();
      await tester.pumpAndSettle();
      expect(calls.last.method, 'check');
      expect(find.text('새 판이 있는지 확인하는 중입니다'), findsOneWidget);
    });

    testWidgets('설정의 "자동으로 업데이트 확인"은 기본 켬이고 끄면 macOS 쪽에 저장한다', (tester) async {
      final calls = _native(tester);
      await updates.start();
      tester.view.physicalSize = const Size(1200, 2400);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        MaterialApp(
          theme: buildTheme(Brightness.light),
          home: const Scaffold(body: SettingsScreen()),
        ),
      );
      await tester.pumpAndSettle();
      final row = find.ancestor(
        of: find.text('자동으로 업데이트 확인'),
        matching: find.byType(Row),
      );
      final toggle = find.descendant(of: row, matching: find.byType(Switch));
      expect(tester.widget<Switch>(toggle).value, isTrue);

      await tester.ensureVisible(toggle);
      await tester.tap(toggle);
      await tester.pumpAndSettle();
      expect(calls.last.method, 'setAuto');
      expect(calls.last.arguments, isFalse);
      expect(updates.auto, isFalse);
      expect(tester.widget<Switch>(toggle).value, isFalse);
    });

    test('확인 주기와 서명 설정: 24시간, 묻지 않고 확인, 몰래 설치하지 않음, 풀기 전과 피드의 서명 확인', () {
      final info = File('macos/Runner/Info.plist').readAsStringSync();
      expect(_plist(info, 'SUScheduledCheckInterval'), 86400);
      expect(_plist(info, 'SUEnableAutomaticChecks'), isTrue);
      expect(_plist(info, 'SUAllowsAutomaticUpdates'), isFalse);
      expect(_plist(info, 'SUVerifyUpdateBeforeExtraction'), isTrue);
      // 서명된 피드만 받고, 서명이 틀린 피드를 일정 기간 뒤에 받아 주는 길(키 바꾸기용)은 닫는다.
      expect(_plist(info, 'SURequireSignedFeed'), isTrue);
      expect(_plist(info, 'SUSignedFeedFailureExpirationInterval'), 0);
      expect(_plist(info, 'SUEnableInstallerLauncherService'), isTrue);
      expect(_plist(info, 'SUPublicEDKey'), r'$(KARI_UPDATE_PUBLIC_KEY)');
      expect(
        _plist(info, 'SUFeedURL'),
        'https://github.com/algocean1204/Kari-docs-helper-with-ai/releases/latest/download/appcast.xml',
      );

      // 샌드박스의 설치 도구 연결 이름은 번들 ID에서 나온다. 권한 파일은 package_dmg.sh가 그대로 쓰므로 글자로 적혀 있다.
      final id = RegExp(r'PRODUCT_BUNDLE_IDENTIFIER = (\S+)').firstMatch(
        File('macos/Runner/Configs/AppInfo.xcconfig').readAsStringSync(),
      )![1]!;
      for (final f in ['Release', 'DebugProfile']) {
        final ent = File('macos/Runner/$f.entitlements').readAsStringSync();
        expect(ent, contains('<string>$id-spks</string>'), reason: f);
        expect(ent, contains('<string>$id-spki</string>'), reason: f);
        expect(_plist(ent, 'com.apple.security.app-sandbox'), isTrue);
      }
    });
  });
}
