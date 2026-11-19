import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import Vision

/// PDF를 캔버스로 가져올 때 쪽 하나를 그림으로 그리고, 스캔 쪽이면 그 그림의 글자를 읽는다.
/// PDF 도우미 프로세스(`--kari-pdf-helper page …`) 안에서만 돈다: 엔진이 그 프로세스의 메모리와 시간을 지켜보다 넘으면
/// 끝내므로, 나쁜 PDF가 그리기를 끝없이 늘여도 앱은 멈추지 않는다. 글자 인식은 Vision으로 기기 안에서 하고 네트워크를
/// 쓰지 않는다.
enum PdfPage {
  /// 그림의 긴 변 상한(화소). 엔진의 그림 넣기 상한과 같다.
  static let maxSide = 4000
  /// 쪽 그림을 다 그렸을 때 부르는 엔진의 함수(`kari_pdf_drawn`). 도우미의 감시가 그 뒤로는 그리기 대신 글자 인식의
  /// 시간 상한을 본다. 도우미를 띄운 앱이 정한다(창 없는 시험은 비워 둔다).
  nonisolated(unsafe) static var drawn: (@convention(c) () -> Void)?
  /// 인식한 한 줄: 글과 그림 안의 상자(0~1, 왼쪽 위 기준).
  struct Line {
    let text: String
    let x, y, w, h: Double
  }

  /// 엔진에 넘기는 화가(`kari_pdf_helper_with`). (PDF, 길이, 쪽 번호(1부터), 배율, 그림 바이트 상한, 표시, 결과 길이)
  /// → malloc한 [머리 JSON 길이 u32 LE][머리 JSON][그림]. 표시: 1 글자 인식, 2 JPEG, 4 한국어 인식 여부만 묻기.
  static let paint:
    @convention(c) (
      UnsafePointer<UInt8>?, Int, UInt32, Double, Int, UInt32, UnsafeMutablePointer<Int>?
    ) -> UnsafeMutablePointer<UInt8>? = { pdf, len, page, scale, max, flags, outLen in
      let data = pdf.map { Data(bytes: $0, count: len) } ?? Data()
      guard let out = answer(data, page: Int(page), scale: scale, max: max, flags: flags),
        let p = malloc(Swift.max(out.count, 1))?.assumingMemoryBound(to: UInt8.self)
      else { return nil }
      out.copyBytes(to: p, count: out.count)
      outLen?.pointee = out.count
      return p
    }

  /// 화가의 몸통. 그리지 못하면 nil. 쪽 그림과 인식이 쓴 메모리는 돌아오기 전에 놓는다.
  static func answer(_ pdf: Data, page: Int, scale: Double, max: Int, flags: UInt32) -> Data? {
    autoreleasepool { draw(pdf, page: page, scale: scale, max: max, flags: flags) }
  }

  private static func draw(_ pdf: Data, page: Int, scale: Double, max: Int, flags: UInt32) -> Data? {
    if flags & 4 != 0 {
      return frame(["ko": koreanSupported()], image: Data())
    }
    guard let image = render(pdf, page: page, scale: scale) else { return nil }
    drawn?()
    var ocr = "off"
    var lines: [Line] = []
    if flags & 1 != 0 {
      if !koreanSupported() {
        ocr = "nokorean"
      } else if let got = recognize(image) {
        (ocr, lines) = ("ok", got)
      } else {
        ocr = "failed"
      }
    }
    guard let (bytes, final) = encode(image, jpeg: flags & 2 != 0, max: max) else { return nil }
    let head: [String: Any] = [
      "w": final.width, "h": final.height,
      "scale": scale * Double(final.width) / Double(image.width),
      "ocr": ocr,
      "lines": lines.map { ["t": $0.text, "x": $0.x, "y": $0.y, "w": $0.w, "h": $0.h] },
    ]
    return frame(head, image: bytes)
  }

  private static func frame(_ head: [String: Any], image: Data) -> Data? {
    guard let h = try? JSONSerialization.data(withJSONObject: head) else { return nil }
    var out = Data()
    var n = UInt32(h.count).littleEndian
    withUnsafeBytes(of: &n) { out.append(contentsOf: $0) }
    out.append(h)
    out.append(image)
    return out
  }

  /// 쪽 `page`(1부터)를 흰 바탕에 배율 `scale`(1pt당 화소)로 그린다. 크기는 CropBox(없으면 MediaBox)를 `/Rotate`대로
  /// 돌린 모양이고, 긴 변은 `maxSide`를 넘지 않는다.
  static func render(_ pdf: Data, page: Int, scale: Double) -> CGImage? {
    guard let provider = CGDataProvider(data: pdf as CFData),
      let doc = CGPDFDocument(provider)
    else { return nil }
    if doc.isEncrypted && !doc.isUnlocked && !doc.unlockWithPassword("") { return nil }
    guard let pg = doc.page(at: page) else { return nil }
    let box = pg.getBoxRect(.cropBox)
    guard box.width >= 1, box.height >= 1, scale.isFinite, scale > 0 else { return nil }
    let turn = ((Int(pg.rotationAngle) % 360) + 360) % 360
    let (w, h) = turn == 90 || turn == 270 ? (box.height, box.width) : (box.width, box.height)
    let k = min(scale, Double(maxSide) / Double(max(w, h)))
    let pw = Swift.max(1, Int((w * k).rounded()))
    let ph = Swift.max(1, Int((h * k).rounded()))
    guard
      let ctx = CGContext(
        data: nil, width: pw, height: ph, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
    else { return nil }
    ctx.setFillColor(CGColor(gray: 1, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: pw, height: ph))
    ctx.interpolationQuality = .high
    ctx.scaleBy(x: CGFloat(pw) / w, y: CGFloat(ph) / h)
    // 쪽 좌표 → 보이는 쪽(왼쪽 아래 0, 시계 방향으로 돌림). 엔진이 글 상자를 놓는 규칙과 같다.
    let (x0, y0, x1, y1) = (box.minX, box.minY, box.maxX, box.maxY)
    let t: CGAffineTransform
    switch turn {
    case 90: t = CGAffineTransform(a: 0, b: -1, c: 1, d: 0, tx: -y0, ty: x1)
    case 180: t = CGAffineTransform(a: -1, b: 0, c: 0, d: -1, tx: x1, ty: y1)
    case 270: t = CGAffineTransform(a: 0, b: 1, c: -1, d: 0, tx: y1, ty: -x0)
    default: t = CGAffineTransform(a: 1, b: 0, c: 0, d: 1, tx: -x0, ty: -y0)
    }
    ctx.concatenate(t)
    ctx.clip(to: box)
    ctx.drawPDFPage(pg)
    return ctx.makeImage()
  }

  /// 이 macOS의 정확 모드 글자 인식이 한국어를 읽는가.
  static func koreanSupported() -> Bool {
    let r = request()
    return koreanIn((try? r.supportedRecognitionLanguages()) ?? [])
  }

  static func koreanIn(_ langs: [String]) -> Bool {
    langs.contains { $0 == "ko-KR" || $0 == "ko" }
  }

  private static func request() -> VNRecognizeTextRequest {
    let r = VNRecognizeTextRequest()
    r.revision = VNRecognizeTextRequestRevision3
    r.recognitionLevel = .accurate
    r.recognitionLanguages = ["ko-KR", "en-US"]
    r.usesLanguageCorrection = true
    return r
  }

  /// 그림의 글자를 줄마다 읽는다(기기 안). 실패하면 nil.
  static func recognize(_ image: CGImage) -> [Line]? {
    let r = request()
    do {
      try VNImageRequestHandler(cgImage: image, options: [:]).perform([r])
    } catch {
      return nil
    }
    return (r.results ?? []).compactMap { o in
      guard let c = o.topCandidates(1).first else { return nil }
      let b = o.boundingBox
      return Line(
        text: c.string, x: Double(b.minX), y: Double(1 - b.maxY), w: Double(b.width),
        h: Double(b.height))
    }
  }

  /// PNG(글을 뺀 바탕)나 JPEG(스캔)로 담는다. `max`바이트를 넘으면 줄여 다시 담는다. (바이트, 담은 그림).
  static func encode(_ image: CGImage, jpeg: Bool, max: Int) -> (Data, CGImage)? {
    var img = image
    for _ in 0..<8 {
      let out = NSMutableData()
      let type = (jpeg ? UTType.jpeg : UTType.png).identifier as CFString
      guard let dest = CGImageDestinationCreateWithData(out, type, 1, nil) else { return nil }
      CGImageDestinationAddImage(
        dest, img, jpeg ? [kCGImageDestinationLossyCompressionQuality: 0.85] as CFDictionary : nil)
      guard CGImageDestinationFinalize(dest) else { return nil }
      if out.length <= max { return (out as Data, img) }
      let k = Swift.min(0.9, (Double(max) / Double(out.length)).squareRoot() * 0.95)
      let (w, h) = (Swift.max(1, Int(Double(img.width) * k)), Swift.max(1, Int(Double(img.height) * k)))
      guard
        let ctx = CGContext(
          data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
          space: CGColorSpace(name: CGColorSpace.sRGB)!,
          bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
      else { return nil }
      ctx.interpolationQuality = .high
      ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
      guard let smaller = ctx.makeImage() else { return nil }
      img = smaller
    }
    return nil
  }
}
