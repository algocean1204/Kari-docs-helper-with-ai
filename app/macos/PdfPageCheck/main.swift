// PDF를 캔버스로 가져올 때의 쪽 그림과 글자 인식 시험: 앱이 PDF 도우미 안에서 부르는 것과 같은 `PdfPage.paint`를 창 없이
// 부른다. 견본은 엔진이 Pretendard로 쓴 PDF와 엔진이 읽은 줄 상자(정답)다.
// 재는 것: 300dpi 글자 인식의 글자 오류율(CER ≤ 3%)과 줄 상자 IoU(평균 ≥ 0.7), 쪽당 시간(p95 ≤ 1.5초), 쪽 하나의
// 메모리 증가(≤ 400MB. 앱은 쪽마다 도우미 프로세스를 새로 띄우므로 첫 쪽까지의 증가가 그 프로세스의 몫이다. 한 프로세스에서
// 여러 쪽을 이어 그리면 CoreGraphics가 푼 그림을 쥐고 있어 늘어난다), 글을 보이지 않게 바꾼 PDF의 바탕에 글이 남지 않음,
// 한국어 지원 판단.
//
//   cd app/engine && cargo run -p kari-import --example canvas_sample -- ../../.cache/pdf-sample
//   cd app/macos && xcrun swiftc -O -target arm64-apple-macos13.0 Runner/PdfPage.swift PdfPageCheck/main.swift \
//     -o ../../.cache/pdf-page-check && ../../.cache/pdf-page-check ../../.cache/pdf-sample
// 네트워크 없이 도는지는 `sandbox-exec -p '(version 1)(allow default)(deny network*)' <실행 파일> <폴더>`로 본다.
import CoreGraphics
import Foundation

func fail(_ m: String) -> Never {
  print("실패: \(m)")
  exit(1)
}

func maxRSS() -> Int {
  var u = rusage()
  getrusage(RUSAGE_SELF, &u)
  return Int(u.ru_maxrss)
}

struct Box {
  let t: String
  let x, y, w, h: Double
}

func iou(_ a: Box, _ b: Box) -> Double {
  let ix = max(0, min(a.x + a.w, b.x + b.w) - max(a.x, b.x))
  let iy = max(0, min(a.y + a.h, b.y + b.h) - max(a.y, b.y))
  let i = ix * iy
  let u = a.w * a.h + b.w * b.h - i
  return u > 0 ? i / u : 0
}

func distance(_ a: [Character], _ b: [Character]) -> Int {
  if a.isEmpty { return b.count }
  var prev = Array(0...b.count)
  var cur = prev
  for i in 1...a.count {
    cur[0] = i
    for j in 1...max(b.count, 1) where j <= b.count {
      cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] == b[j - 1] ? 0 : 1))
    }
    swap(&prev, &cur)
  }
  return prev[b.count]
}

/// 화가를 앱과 같은 C 함수로 부른다. (머리 JSON, 그림 바이트).
func paint(_ pdf: Data, page: Int, scale: Double, flags: UInt32) -> ([String: Any], Data)? {
  var n = 0
  let out = pdf.withUnsafeBytes { raw in
    PdfPage.paint(
      raw.bindMemory(to: UInt8.self).baseAddress, pdf.count, UInt32(page), scale, 20 << 20, flags,
      &n)
  }
  guard let out else { return nil }
  defer { free(out) }
  let data = Data(bytes: out, count: n)
  let len = Int(data.prefix(4).withUnsafeBytes { $0.loadUnaligned(as: UInt32.self) })
  guard let head = try? JSONSerialization.jsonObject(with: data.subdata(in: 4..<4 + len)) as? [String: Any]
  else { return nil }
  return (head, data.subdata(in: 4 + len..<data.count))
}

guard let arg = CommandLine.arguments.dropFirst().first else { fail("견본 폴더") }
let dir = URL(fileURLWithPath: arg)
guard let text = try? Data(contentsOf: dir.appendingPathComponent("text.pdf")),
  let stripped = try? Data(contentsOf: dir.appendingPathComponent("stripped.pdf")),
  let tj = try? JSONSerialization.jsonObject(with: Data(contentsOf: dir.appendingPathComponent("truth.json")))
    as? [String: Any]
else { fail("견본을 읽지 못함") }
let (pw, ph) = (tj["w"] as! Double, tj["h"] as! Double)
var truth: [Int: [Box]] = [:]
for l in tj["lines"] as! [[String: Any]] {
  truth[l["page"] as! Int, default: []].append(
    Box(t: l["t"] as! String, x: l["x"] as! Double, y: l["y"] as! Double, w: l["w"] as! Double, h: l["h"] as! Double))
}

// 1. 한국어 지원 판단.
guard PdfPage.koreanIn(["ko-KR", "en-US"]), !PdfPage.koreanIn(["en-US", "fr-FR"]) else { fail("한국어 판단") }
let ko = PdfPage.koreanSupported()
print("이 macOS의 한국어 글자 인식: \(ko)")
if !ko {
  guard let (h, _) = paint(text, page: 1, scale: 300 / 72, flags: 3), h["ocr"] as? String == "nokorean" else {
    fail("한국어를 못 읽는 macOS의 안내")
  }
  print("한국어를 읽지 못해 바탕만 그림: 통과")
  exit(0)
}

// 2. 글을 보이지 않게 바꾼 바탕: 정답 줄 상자 안이 모두 흰색이고, 원래 PDF는 그 자리에 글이 있다.
func ink(_ pdf: Data, boxes: [Box]) -> Int {
  guard let img = PdfPage.render(pdf, page: 1, scale: 2),
    let ctx = CGContext(
      data: nil, width: img.width, height: img.height, bitsPerComponent: 8, bytesPerRow: img.width * 4,
      space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
  else { fail("그리기") }
  ctx.draw(img, in: CGRect(x: 0, y: 0, width: img.width, height: img.height))
  let px = ctx.data!.assumingMemoryBound(to: UInt8.self)
  let k = Double(img.width) / pw
  var dark = 0
  for b in boxes {
    for y in Int(b.y * k)..<Int((b.y + b.h) * k) {
      for x in Int(b.x * k)..<Int((b.x + b.w) * k) where x < img.width && y < img.height {
        if px[(y * img.width + x) * 4] < 128 { dark += 1 }
      }
    }
  }
  return dark
}
let before = ink(text, boxes: truth[1]!)
let after = ink(stripped, boxes: truth[1]!)
print("글 상자 안의 어두운 화소: 원래 \(before), 글을 뺀 바탕 \(after)")
guard before > 1000, after == 0 else { fail("글을 뺀 바탕에 글이 남음") }

// 3. 300dpi 글자 인식: 정확도, 쪽당 시간, 메모리.
let base = maxRSS()
var times: [Double] = []
var (errs, chars) = (0, 0)
var ious: [Double] = []
var ratios: [Double] = []
var first = 0
for page in 1...truth.count { autoreleasepool {
  let t0 = Date()
  guard let (head, image) = paint(text, page: page, scale: 300 / 72, flags: 3) else { fail("\(page)쪽 그리기") }
  times.append(Date().timeIntervalSince(t0))
  if page == 1 { first = (maxRSS() - base) >> 20 }
  guard head["ocr"] as? String == "ok", let ls = head["lines"] as? [[String: Any]] else { fail("\(page)쪽 인식") }
  if page == 1 {
    print("1쪽 그림 \(head["w"]!)×\(head["h"]!) 화소, JPEG \(image.count / 1024)KB")
    guard (head["w"] as! Int) >= Int(pw * 2) else { fail("144dpi보다 작은 그림") }
  }
  let got = ls.map {
    Box(
      t: $0["t"] as! String, x: ($0["x"] as! Double) * pw, y: ($0["y"] as! Double) * ph,
      w: ($0["w"] as! Double) * pw, h: ($0["h"] as! Double) * ph)
  }
  let want = truth[page]!
  let flat = { (bs: [Box]) in
    Array(bs.sorted { ($0.y, $0.x) < ($1.y, $1.x) }.map(\.t).joined().filter { !$0.isWhitespace })
  }
  let (a, b) = (flat(want), flat(got))
  errs += distance(a, b)
  chars += a.count
  for w in want {
    let best = got.max { iou(w, $0) < iou(w, $1) }
    ious.append(best.map { iou(w, $0) } ?? 0)
    if let best { ratios.append(best.h / (w.h / 1.2)) }
  }
} }
let grew = (maxRSS() - base) >> 20
let sorted = times.sorted()
let p95 = sorted[min(sorted.count - 1, Int((Double(sorted.count) * 0.95).rounded(.up)) - 1)]
let cer = Double(errs) / Double(max(chars, 1))
let meanIoU = ious.reduce(0, +) / Double(ious.count)
let ratio = ratios.sorted()[ratios.count / 2]
print(String(format: "글자 오류율 %.2f%% (%d/%d)", cer * 100, errs, chars))
print(String(format: "줄 상자 IoU 평균 %.3f, 가장 낮은 %.3f, 0.7 이상 %d/%d", meanIoU, ious.min()!, ious.filter { $0 >= 0.7 }.count, ious.count))
print(String(format: "쪽당 시간 p95 %.2f초, 가장 긴 %.2f초, %d쪽 합계 %.1f초", p95, sorted.last!, times.count, times.reduce(0, +)))
print(String(format: "인식 상자 높이 ÷ 글자 크기(가운데 값) %.3f", ratio))
print("최대 RSS 증가: 첫 쪽까지 \(first)MB, \(times.count)쪽을 한 프로세스에서 \(grew)MB")
guard cer <= 0.03 else { fail("글자 오류율") }
guard meanIoU >= 0.7 else { fail("줄 상자 IoU") }
guard p95 <= 1.5 else { fail("쪽당 시간") }
guard first <= 400 else { fail("메모리") }
print("통과")
