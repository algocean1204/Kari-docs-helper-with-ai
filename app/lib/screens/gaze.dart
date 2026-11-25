// ignore: deprecated_member_use
import 'package:flutter/material.dart';

import '../design/components.dart';
import '../design/tokens.dart';
import '../library.dart';

/// 시선 기록의 ⓘ 설명. 세는 것과 세지 않는 때는 app/lib/gaze.dart의 [GazeTrack], 영상을 다루는 법은
/// macos/Runner/GazeMonitor.swift에 있다.
const gazeInfo =
    '얼굴이 10초 넘게 화면을 벗어난 구간만 셉니다. 질문 뒤 15초와 입력 중엔 세지 않고, 영상은 저장·전송하지 않습니다. 기본값 끔.';

/// 면접 준비의 시선 기록 켬/끔. 설정 화면의 "시선 관찰"과 같은 값이다.
/// 권한이 거절돼 있거나 카메라가 없으면 시선 기록 없이 진행한다고 알린다. 권한은 처음 켤 때 macOS가 묻는다.
class GazeChoice extends StatelessWidget {
  const GazeChoice({super.key, required this.library});

  final Library library;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final on = library.get<bool>(Setting.gaze);
    final blocked = on
        ? switch (library.gaze.permission) {
            'denied' => '카메라 권한이 없어 시선 기록 없이 진행합니다',
            'none' => '카메라가 없어 시선 기록 없이 진행합니다',
            _ => null,
          }
        : null;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        FocusRing(
          radius: 16,
          child: Semantics(
            label: '시선 기록',
            child: Switch(
              value: on,
              onChanged: (v) async {
                await library.set(Setting.gaze, v);
                if (v) await library.gaze.check();
              },
            ),
          ),
        ),
        const SizedBox(height: Space.x1),
        KeepText(
          on
              ? '카메라로 화면을 보고 있는지만 봅니다. 영상은 저장하지 않고 기기 밖으로 나가지 않습니다.'
              : '켜면 카메라로 화면을 보고 있는지만 봅니다. 처음 켤 때 macOS가 카메라 권한을 묻습니다.',
          style: AppText.caption.copyWith(color: c.textMuted),
        ),
        if (blocked != null) ...[
          const SizedBox(height: Space.x2),
          Wrap(
            spacing: Space.x2,
            runSpacing: Space.x1,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Icon(Icons.videocam_off, size: 16, color: c.textMuted),
              Text(blocked, style: AppText.caption.copyWith(color: c.text)),
              if (library.gaze.permission == 'denied')
                AppButton(
                  '시스템 설정 열기',
                  kind: ButtonKind.quiet,
                  onPressed: library.gaze.openSettings,
                ),
            ],
          ),
        ],
      ],
    );
  }
}

/// 진행 화면 머리의 작은 표시: "시선 기록 중"과 끄는 단추. 수치는 보이지 않는다(점수로 읽힌다).
class GazeBadge extends StatelessWidget {
  const GazeBadge({super.key, required this.onOff});

  final VoidCallback onOff;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Padding(
      padding: const EdgeInsets.only(right: Space.x4),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.videocam, size: 16, color: c.textMuted),
          const SizedBox(width: Space.x1),
          Text('시선 기록 중', style: AppText.caption.copyWith(color: c.textMuted)),
          const SizedBox(width: Space.x2),
          AppButton('시선 기록 끄기', kind: ButtonKind.quiet, onPressed: onOff),
        ],
      ),
    );
  }
}
