import 'dart:ffi';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// 엔진이 그린 화소를 Flutter 텍스처로 올리는 macOS 접착(macos/Runner/MainFlutterWindow.swift).
///
/// Swift가 버퍼 세 장을 돌려 쓰고, 그리기 시작과 끝을 C 함수 주소로 한 번 넘긴다. Dart는 프레임마다 그 함수를
/// FFI로 바로 부르고, 받은 버퍼에 엔진이 그린다(`kari_edit_render`).
class Surface {
  Surface._(
    this.textureId,
    this.width,
    this.height,
    this.stride,
    this._begin,
    this._end,
  );

  static const _channel = MethodChannel('kari/texture');

  /// 화소 크기의 텍스처를 만든다. 있던 텍스처는 Swift가 풀고 바꾼다.
  static Future<Surface> create(int width, int height) async {
    final r = (await _channel.invokeMapMethod<String, Object?>('create', {
      'width': width,
      'height': height,
    }))!;
    return Surface._(
      r['id']! as int,
      width,
      height,
      r['stride']! as int,
      Pointer<
            NativeFunction<Pointer<Uint8> Function(IntPtr, IntPtr)>
          >.fromAddress(r['begin']! as int)
          .asFunction(),
      Pointer<NativeFunction<Void Function()>>.fromAddress(
        r['end']! as int,
      ).asFunction(),
    );
  }

  /// 텍스처를 보이는 위젯. 위젯 시험에는 텍스처가 없어, 화면을 그림으로 남기는 시험이 그린 화소를 보일 위젯으로 바꾼다.
  static Widget Function(int textureId) view = (id) =>
      Texture(textureId: id, filterQuality: FilterQuality.none);

  /// 텍스처를 푼다. 편집 화면을 떠날 때.
  static Future<void> dispose() => _channel.invokeMethod<void>('dispose');

  final int textureId;
  final int width;
  final int height;

  /// 버퍼 한 줄의 바이트 수.
  final int stride;
  final Pointer<Uint8> Function(int, int) _begin;
  final void Function() _end;

  /// 다음 버퍼를 받아 [draw]로 그리고 Flutter에 넘긴다. 텍스처가 바뀌는 중이라 크기가 맞는 버퍼가 없으면
  /// 그리지 않고 거짓을 돌려준다. [draw]가 거짓이면(엔진이 그리지 못함) 그것을 돌려준다.
  bool frame(bool Function(Pointer<Uint8> pixels) draw) {
    final p = _begin(width, height);
    try {
      return p != nullptr && draw(p);
    } finally {
      _end();
    }
  }
}
