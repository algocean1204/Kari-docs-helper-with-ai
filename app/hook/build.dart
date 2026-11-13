// 엔진(engine/ffi, 크레이트 kari-ffi)을 cargo로 빌드해 앱에 싣는다.
// flutter run, flutter build, flutter test가 모두 이 훅을 거친다. 라이브러리는 lib/engine/native.dart가 부른다.
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:hooks/hooks.dart';

void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) return;
    final code = input.config.code;
    final os = code.targetOS;
    final engine = input.packageRoot.resolve('engine/');
    final target = _rustTarget(os, code.targetArchitecture);

    final env = <String, String>{};
    if (os == OS.macOS) {
      // 훅은 SDKROOT를 물려받지 않는다. SQLite(C 코드)를 빌드하려면 필요하다.
      final sdk = await _run('xcrun', ['--sdk', 'macosx', '--show-sdk-path']);
      env['SDKROOT'] = sdk.trim();
      env['MACOSX_DEPLOYMENT_TARGET'] = '${code.macOS.targetVersion}';
      // 한계: 이 기기의 Rust에 다른 아키텍처의 표준 라이브러리가 없으면 그 조각을 건너뛴다.
      // 앱은 arm64 전용이라 지금은 이 길을 지나지 않는다. Intel 맥을 지원할 때 대상을 설치하고 이 길을 없앤다.
      final libdir = await _run('rustc', [
        '--print',
        'target-libdir',
        '--target',
        target,
      ]);
      if (!Directory(libdir.trim()).existsSync()) {
        stderr.writeln('kari: Rust 대상 $target이 없어 이 아키텍처의 엔진을 건너뜁니다.');
        return;
      }
    }

    // 빌드한 기기의 경로(사용자 폴더 이름)가 라이브러리 안의 오류 문구에 남지 않게 바꿔 적는다.
    // 뒤에 적은 것이 먼저 맞는다: 홈 폴더 → cargo 폴더 → 엔진 폴더.
    final home =
        Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
    final cargoHome =
        Platform.environment['CARGO_HOME'] ??
        (home == null ? null : '$home/.cargo');
    env['CARGO_ENCODED_RUSTFLAGS'] = [
      ...?Platform.environment['RUSTFLAGS']?.split(RegExp(r'\s+')),
      if (home != null) '--remap-path-prefix=$home=/home',
      if (cargoHome != null) '--remap-path-prefix=$cargoHome=/cargo',
      '--remap-path-prefix=${engine.toFilePath()}=engine/',
    ].where((f) => f.isNotEmpty).join('\x1f');

    await _run(
      'cargo',
      ['build', '--profile', 'dist', '-p', 'kari-ffi', '--target', target],
      dir: engine,
      env: env,
    );

    output.assets.code.add(
      CodeAsset(
        package: input.packageName,
        name: 'engine/native.dart',
        linkMode: DynamicLoadingBundled(),
        file: engine.resolve('target/$target/dist/${os.dylibFileName('kari')}'),
      ),
    );
    // 엔진 코드가 바뀌면 훅을 다시 돌린다(바뀌지 않았으면 cargo는 바로 끝난다).
    output.dependencies.addAll([
      engine.resolve('Cargo.toml'),
      engine.resolve('Cargo.lock'),
      for (final crate in [
        'doc',
        'layout',
        'edit',
        'render',
        'pdf',
        'docx',
        'store',
        'ai',
        // 지식 팩(interview/pack)의 글도 엔진 라이브러리에 들어간다.
        'interview',
        'ffi',
      ])
        for (final f in Directory.fromUri(
          engine.resolve('$crate/'),
        ).listSync(recursive: true))
          if (f is File) f.uri,
    ]);
  });
}

String _rustTarget(OS os, Architecture arch) {
  final cpu = switch (arch) {
    Architecture.arm64 => 'aarch64',
    Architecture.x64 => 'x86_64',
    _ => throw UnsupportedError('엔진을 빌드하지 않는 아키텍처입니다: $arch'),
  };
  return switch (os) {
    OS.macOS => '$cpu-apple-darwin',
    OS.linux => '$cpu-unknown-linux-gnu',
    OS.windows => '$cpu-pc-windows-msvc',
    _ => throw UnsupportedError('엔진을 빌드하지 않는 운영체제입니다: $os'),
  };
}

Future<String> _run(
  String exe,
  List<String> args, {
  Uri? dir,
  Map<String, String>? env,
}) async {
  final r = await Process.run(
    exe,
    args,
    workingDirectory: dir?.toFilePath(),
    environment: env,
  );
  if (r.exitCode != 0) {
    throw Exception('$exe ${args.join(' ')} 실패(${r.exitCode}):\n${r.stderr}');
  }
  return r.stdout as String;
}
