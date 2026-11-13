import AVFoundation
import FlutterMacOS
import Vision

/// 시선 기록의 카메라 쪽. 카메라를 열고 초당 5번 Vision으로 얼굴을 찾아
/// "얼굴 있음/없음, 좌우 각도, 상하 각도"만 채널 `kari/gaze`로 보낸다. 판정과 구간 만들기는 Dart(app/lib/gaze.dart)가 한다.
/// 그림(프레임)은 이 파일 밖으로 내보내지 않고, 저장하지 않고, 로그에도 남기지 않는다.
///
/// Vision의 각도(SDK 헤더 Vision/VNObservation.h, developer.apple.com의 VNFaceObservation 문서에서 확인):
/// - 단위는 라디안이다. 여기서 도(°)로 바꿔 보낸다.
/// - `yaw`(좌우): 양수가 반시계 방향, 범위 [-π/2, π/2]. macOS 10.14부터.
/// - `pitch`(상하): 양수가 고개를 아래로 끄덕이는 쪽, 범위 [-π/2, π/2]. macOS 12부터이고 요청 판 3
///   (`VNDetectFaceRectanglesRequestRevision3`, 이 판부터 세 각도가 연속값)이 채운다. 앱의 최소 판은 macOS 13이라 늘 쓸 수 있다.
/// - 계산하지 못하면 nil이다. 그때는 그 각도를 보내지 않는다(Dart는 0으로 본다).
///
/// 메서드: `status`(권한을 묻지 않고 지금 상태), `start`(처음이면 macOS가 권한을 묻는다), `stop`, `openSettings`,
/// `measure`(같은 크기의 그림에 얼굴 요청을 되풀이해 한 장의 처리 시간을 잰다. 카메라를 켜지 않는다).
/// Swift가 Dart로 부르는 것: `sample { face, yaw?, pitch? }`.
final class GazeMonitor: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
  /// 한 장을 보는 간격(초당 5번).
  static let interval = 0.2

  private let channel: FlutterMethodChannel
  private let queue = DispatchQueue(label: "kari.gaze")
  /// 아래 셋은 `queue`에서만 만진다.
  private var session: AVCaptureSession?
  private var last = 0.0
  private let request = GazeMonitor.makeRequest()

  init(messenger: FlutterBinaryMessenger) {
    channel = FlutterMethodChannel(name: "kari/gaze", binaryMessenger: messenger)
    super.init()
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self else {
        result(nil)
        return
      }
      switch call.method {
      case "status":
        result(Self.status())
      case "start":
        self.start(result)
      case "stop":
        self.stop()
        result(nil)
      case "openSettings":
        let url = URL(
          string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera")!
        result(NSWorkspace.shared.open(url))
      case "measure":
        let frames = (call.arguments as? [String: Any])?["frames"] as? Int ?? 100
        DispatchQueue.global(qos: .userInitiated).async {
          let r = Self.measure(frames: frames, image: nil)
          DispatchQueue.main.async { result(r) }
        }
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }

  /// 카메라 권한: authorized, notDetermined, denied. 카메라가 없으면 none. 권한을 묻지 않는다.
  static func status() -> String {
    if AVCaptureDevice.default(for: .video) == nil { return "none" }
    switch AVCaptureDevice.authorizationStatus(for: .video) {
    case .authorized: return "authorized"
    case .notDetermined: return "notDetermined"
    default: return "denied"
    }
  }

  /// 연다. 돌려주는 것: on, denied, none.
  private func start(_ result: @escaping FlutterResult) {
    switch Self.status() {
    case "none":
      result("none")
    case "denied":
      result("denied")
    case "notDetermined":
      AVCaptureDevice.requestAccess(for: .video) { granted in
        DispatchQueue.main.async {
          if granted { self.open(result) } else { result("denied") }
        }
      }
    default:
      open(result)
    }
  }

  private func open(_ result: @escaping FlutterResult) {
    queue.async {
      if self.session != nil {
        DispatchQueue.main.async { result("on") }
        return
      }
      let s = AVCaptureSession()
      s.sessionPreset = s.canSetSessionPreset(.vga640x480) ? .vga640x480 : .low
      let output = AVCaptureVideoDataOutput()
      output.alwaysDiscardsLateVideoFrames = true
      output.setSampleBufferDelegate(self, queue: self.queue)
      guard let device = AVCaptureDevice.default(for: .video),
        let input = try? AVCaptureDeviceInput(device: device),
        s.canAddInput(input), s.canAddOutput(output)
      else {
        DispatchQueue.main.async { result("none") }
        return
      }
      s.addInput(input)
      s.addOutput(output)
      s.startRunning()
      self.session = s
      self.last = 0
      DispatchQueue.main.async { result("on") }
    }
  }

  /// 닫는다. 세션을 멈추고 놓아 카메라 표시등이 꺼진다. 이미 닫혀 있으면 아무것도 하지 않는다.
  func stop() {
    queue.async {
      guard let s = self.session else { return }
      s.stopRunning()
      for o in s.outputs { s.removeOutput(o) }
      for i in s.inputs { s.removeInput(i) }
      self.session = nil
    }
  }

  func captureOutput(
    _ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer,
    from connection: AVCaptureConnection
  ) {
    let now = CACurrentMediaTime()
    guard session != nil, now - last >= Self.interval,
      let pixels = CMSampleBufferGetImageBuffer(sampleBuffer)
    else { return }
    last = now
    let values = Self.detect(VNImageRequestHandler(cvPixelBuffer: pixels, options: [:]), request)
    DispatchQueue.main.async { self.channel.invokeMethod("sample", arguments: values) }
  }

  static func makeRequest() -> VNDetectFaceRectanglesRequest {
    let r = VNDetectFaceRectanglesRequest()
    r.revision = VNDetectFaceRectanglesRequestRevision3
    return r
  }

  /// 그림 한 장에서 가장 큰 얼굴의 각도(도). 얼굴이 없거나 요청이 실패하면 `face: false`.
  static func detect(_ handler: VNImageRequestHandler, _ request: VNDetectFaceRectanglesRequest)
    -> [String: Any]
  {
    guard (try? handler.perform([request])) != nil,
      let f = request.results?.max(by: {
        $0.boundingBox.width * $0.boundingBox.height < $1.boundingBox.width * $1.boundingBox.height
      })
    else { return ["face": false] }
    var v: [String: Any] = ["face": true]
    if let yaw = f.yaw { v["yaw"] = yaw.doubleValue * 180 / .pi }
    if let pitch = f.pitch { v["pitch"] = pitch.doubleValue * 180 / .pi }
    return v
  }

  /// 640×480 그림 한 장(주면 그 그림, 없으면 회색)에 얼굴 요청을 [frames]번 되풀이한다.
  /// 첫 번은 모델을 올리는 시간이 들어 따로 적는다. 돌려주는 것: { frames, firstMs, avgMs, maxMs, face }.
  static func measure(frames: Int, image: CGImage?) -> [String: Any] {
    var buffer: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, 640, 480, kCVPixelFormatType_32BGRA, nil, &buffer)
    guard let pixels = buffer else { return [:] }
    CVPixelBufferLockBaseAddress(pixels, [])
    if let base = CVPixelBufferGetBaseAddress(pixels) {
      memset(base, 0x80, CVPixelBufferGetDataSize(pixels))
      if let image,
        let ctx = CGContext(
          data: base, width: 640, height: 480, bitsPerComponent: 8,
          bytesPerRow: CVPixelBufferGetBytesPerRow(pixels),
          space: CGColorSpaceCreateDeviceRGB(),
          bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue
            | CGBitmapInfo.byteOrder32Little.rawValue)
      {
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: 640, height: 480))
      }
    }
    CVPixelBufferUnlockBaseAddress(pixels, [])
    let request = makeRequest()
    var times: [Double] = []
    var face = false
    for _ in 0..<max(frames, 2) {
      let t = DispatchTime.now().uptimeNanoseconds
      let v = detect(VNImageRequestHandler(cvPixelBuffer: pixels, options: [:]), request)
      times.append(Double(DispatchTime.now().uptimeNanoseconds - t) / 1e6)
      face = v["face"] as? Bool ?? false
    }
    let rest = times.dropFirst()
    return [
      "frames": rest.count, "firstMs": times[0], "avgMs": rest.reduce(0, +) / Double(rest.count),
      "maxMs": rest.max() ?? 0, "face": face,
    ]
  }
}
