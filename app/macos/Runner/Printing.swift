import AppKit
import PDFKit

/// 인쇄와 공유(채널 `kari/files`의 `print`, `share`). 앱이 만든 PDF 파일을 macOS 창에 넘기기만 한다.
/// 파일을 지우는 일은 Dart가 결과를 받은 뒤 한다.
enum Printing {
  /// PDF 한 장의 인쇄 작업. 실제 크기(배율 없음), 자동 회전 끔. 용지는 문서의 첫 쪽 크기로 먼저 맞춰 두고,
  /// 프린터에 그 용지가 없거나 사용자가 바꾸면 인쇄 창의 설정을 따른다. 읽을 수 없는 파일이면 nil.
  static func operation(path: String, name: String) -> NSPrintOperation? {
    guard let pdf = PDFDocument(url: URL(fileURLWithPath: path)), let first = pdf.page(at: 0)
    else { return nil }
    let size = first.bounds(for: .mediaBox).size
    let info = (NSPrintInfo.shared.copy() as? NSPrintInfo) ?? NSPrintInfo()
    info.orientation = size.width > size.height ? .landscape : .portrait
    info.paperSize = size
    guard let op = pdf.printOperation(for: info, scalingMode: .pageScaleNone, autoRotate: false)
    else { return nil }
    op.jobTitle = name
    op.showsPrintPanel = true
    op.showsProgressPanel = true
    return op
  }

  /// 끝날 때까지 붙잡아 둔 위임 객체. 인쇄 작업과 공유 창은 위임을 붙잡지 않는다.
  private static var alive: [NSObject] = []

  /// 인쇄 창을 [window]에 붙는 시트로 연다. 닫히면 [done]: 끝까지 인쇄했으면 참, 취소나 실패면 거짓.
  static func print(path: String, name: String, window: NSWindow, done: @escaping (Bool?) -> Void) {
    guard let op = operation(path: path, name: name) else {
      done(nil)
      return
    }
    let d = PrintDone(done: done)
    alive.append(d)
    op.runModal(
      for: window, delegate: d,
      didRun: #selector(PrintDone.printOperationDidRun(_:success:contextInfo:)), contextInfo: nil)
  }

  /// 공유 창을 [rect](뷰 좌표)에 띄운다. 끝나면 [done]: 공유했으면 참, 고르지 않고 닫았으면 거짓,
  /// 고른 서비스가 실패했으면 그 오류.
  static func share(
    path: String, rect: NSRect, view: NSView, done: @escaping (Result<Bool, Error>) -> Void
  ) {
    let picker = NSSharingServicePicker(items: [URL(fileURLWithPath: path)])
    let d = ShareDone(done: done)
    alive.append(d)
    picker.delegate = d
    picker.show(relativeTo: rect, of: view, preferredEdge: .minY)
  }

  fileprivate static func release(_ d: NSObject) {
    alive.removeAll { $0 === d }
  }
}

private final class PrintDone: NSObject {
  let done: (Bool?) -> Void
  init(done: @escaping (Bool?) -> Void) { self.done = done }

  @objc func printOperationDidRun(
    _ op: NSPrintOperation, success: Bool, contextInfo: UnsafeMutableRawPointer?
  ) {
    done(success)
    Printing.release(self)
  }
}

private final class ShareDone: NSObject, NSSharingServicePickerDelegate, NSSharingServiceDelegate {
  private var done: ((Result<Bool, Error>) -> Void)?
  init(done: @escaping (Result<Bool, Error>) -> Void) { self.done = done }

  private func finish(_ how: Result<Bool, Error>) {
    done?(how)
    done = nil
    Printing.release(self)
  }

  func sharingServicePicker(
    _ sharingServicePicker: NSSharingServicePicker, didChoose service: NSSharingService?
  ) {
    // 서비스를 골랐으면 그 서비스가 끝났다고 알릴 때까지 파일을 둔다.
    if service == nil { finish(.success(false)) }
  }

  func sharingServicePicker(
    _ sharingServicePicker: NSSharingServicePicker, delegateFor sharingService: NSSharingService
  ) -> NSSharingServiceDelegate? { self }

  func sharingService(_ sharingService: NSSharingService, didShareItems items: [Any]) {
    finish(.success(true))
  }

  func sharingService(
    _ sharingService: NSSharingService, didFailToShareItems items: [Any], error: Error
  ) {
    finish(.failure(error))
  }
}
