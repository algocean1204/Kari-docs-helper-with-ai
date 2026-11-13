import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';

import '../engine/engine.dart';
import '../library.dart';
import 'paper.dart' show filesChannel;

/// 인쇄·공유로 넘길 PDF의 파일 이름. 쓸 수 없는 글자(`/`, `:`)는 `-`로 바꾸고, 비면 `문서`.
/// 파일 이름의 한계(255바이트)를 넘지 않게 뒤를 자른다.
String pdfFileName(String name) {
  var base = name.replaceAll(RegExp('[/:]'), '-').trim();
  if (base.isEmpty) base = '문서';
  final runes = base.runes.toList();
  while (utf8.encode('${String.fromCharCodes(runes)}.pdf').length > 255) {
    runes.removeLast();
  }
  return '${String.fromCharCodes(runes)}.pdf';
}

/// 문서를 앱의 임시 폴더에 PDF로 만들어 macOS 창(채널 `kari/files`의 [method]: `print` 또는 `share`)에 넘기고,
/// 창이 닫히거나 공유가 끝나면(성공·취소·실패) 그 파일을 지운다. [made]는 PDF를 만든 뒤 창을 열기 전에 부른다.
/// 실패하면 화면에 보일 이유를 돌려준다.
///
/// 넘길 때마다 하위 폴더를 따로 만들어, 공유가 끝나기 전에 또 공유해도 서로의 파일을 건드리지 않는다.
// 한계: 끝을 알리지 않는 공유 서비스의 파일은 앱을 켤 때에야 지운다([clearOut]). 켜 둔 채 쌓이는 일이 잦으면
// 하루 지난 하위 폴더를 넘길 때마다 지운다.
Future<String?> handOff(
  Library lib,
  DocInfo doc,
  String method, {
  Map<String, Object?> args = const {},
  void Function()? made,
}) async {
  final dir = Directory(
    '${lib.outDir}/${DateTime.now().microsecondsSinceEpoch}',
  );
  final path = '${dir.path}/${pdfFileName(doc.name)}';
  final verb = method == 'print' ? '인쇄' : '공유';
  try {
    dir.createSync(recursive: true);
    await lib.engine.docExportPdf(doc.id, path);
    made?.call();
    await filesChannel.invokeMethod<bool>(method, {
      'path': path,
      'name': doc.name,
      ...args,
    });
    return null;
  } on EngineError catch (e) {
    return '$verb할 PDF를 만들지 못했습니다 · ${describe(e)}';
  } on FileSystemException catch (e) {
    return '$verb할 PDF를 만들지 못했습니다 · ${e.message}';
  } on PlatformException catch (e) {
    return '$verb하지 못했습니다 · ${e.message ?? e.code}';
  } on MissingPluginException {
    return '$verb 창을 열 수 없습니다';
  } finally {
    try {
      dir.deleteSync(recursive: true);
    } on FileSystemException {
      // 이미 없다.
    }
  }
}
