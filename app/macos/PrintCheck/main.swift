// 인쇄 작업 확인: 앱과 같은 Printing.operation으로 PDF 한 장의 인쇄 작업을 만들기만 하고 실행하지 않는다(창이 뜨지 않는다).
// 용지가 문서의 쪽 크기와 방향을 따르는지, 이름과 인쇄 창 설정, 읽을 수 없는 파일이면 nil인지 본다.
//
//   cd app/macos && xcrun swiftc -O Runner/Printing.swift PrintCheck/main.swift -o ../build/print-check \
//     && ../build/print-check
import AppKit
import PDFKit

/// 크기(pt)가 [w]×[h]인 빈 쪽 [pages]장짜리 PDF를 임시 폴더에 쓴다.
func pdf(_ w: CGFloat, _ h: CGFloat, pages: Int) -> String {
  let path = NSTemporaryDirectory() + "kari-print-check-\(Int(w))x\(Int(h)).pdf"
  var box = CGRect(x: 0, y: 0, width: w, height: h)
  let ctx = CGContext(URL(fileURLWithPath: path) as CFURL, mediaBox: &box, nil)!
  for _ in 0..<pages {
    ctx.beginPDFPage(nil)
    ctx.endPDFPage()
  }
  ctx.closePDF()
  return path
}

var failed = 0
func check(_ ok: Bool, _ what: String) {
  print(ok ? "통과" : "실패", what)
  if !ok { failed += 1 }
}

let a4 = pdf(595.28, 841.89, pages: 2)
if let op = Printing.operation(path: a4, name: "이력서") {
  let info = op.printInfo
  check(abs(info.paperSize.width - 595.28) < 0.5 && abs(info.paperSize.height - 841.89) < 0.5, "A4 세로: 용지 \(info.paperSize)")
  check(info.orientation == .portrait, "A4 세로: 방향")
  check(op.jobTitle == "이력서", "작업 이름")
  check(op.showsPrintPanel && op.showsProgressPanel, "인쇄 창과 진행 창을 보인다")
} else {
  check(false, "A4 PDF로 인쇄 작업을 만든다")
}

let wide = pdf(792, 612, pages: 1)
if let op = Printing.operation(path: wide, name: "가로") {
  let s = op.printInfo.paperSize
  check(abs(s.width - 792) < 0.5 && abs(s.height - 612) < 0.5, "LETTER 가로: 용지 \(s)")
  check(op.printInfo.orientation == .landscape, "LETTER 가로: 방향")
} else {
  check(false, "가로 PDF로 인쇄 작업을 만든다")
}

check(Printing.operation(path: NSTemporaryDirectory() + "없는-파일.pdf", name: "") == nil, "없는 파일은 nil")
try? FileManager.default.removeItem(atPath: a4)
try? FileManager.default.removeItem(atPath: wide)
exit(failed == 0 ? 0 : 1)
