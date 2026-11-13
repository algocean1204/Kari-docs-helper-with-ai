// 시선 기록의 처리 시간 측정: 앱과 같은 GazeMonitor.measure를 창 없이, 카메라를 켜지 않고 돌린다.
// 그림 경로를 주면 그 그림을 640×480에 그려 쓰고(얼굴이 있는 그림으로 재려면), 없으면 회색 그림이다.
//
//   cd app/macos && FW=~/flutter/bin/cache/artifacts/engine/darwin-x64/FlutterMacOS.xcframework/macos-arm64_x86_64
//   xcrun swiftc -O -target arm64-apple-macos13.0 -F $FW -framework FlutterMacOS -Xlinker -rpath -Xlinker $FW \
//     Runner/GazeMonitor.swift GazeMeasure/main.swift -o ../build/gaze-measure && ../build/gaze-measure [그림]
import Foundation
import ImageIO

var image: CGImage?
if let path = CommandLine.arguments.dropFirst().first,
  let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil)
{
  image = CGImageSourceCreateImageAtIndex(source, 0, nil)
}
print(GazeMonitor.measure(frames: 200, image: image))
