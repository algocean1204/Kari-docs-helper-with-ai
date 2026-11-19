import Cocoa
import FlutterMacOS

@main
class AppDelegate: FlutterAppDelegate {
  /// 엔진이 PDF를 읽으려고 이 실행 파일을 `--kari-pdf-helper`로 다시 띄우면, NSApplication을 만들기 전에 엔진의 도우미만
  /// 돌고 끝난다(창, Dock 아이콘, Flutter 없음). 같은 서명·같은 샌드박스라 따로 서명할 실행 파일이 없다.
  static func main() {
    if CommandLine.arguments.count > 1 && CommandLine.arguments[1] == "--kari-pdf-helper" {
      exit(pdfHelper())
    }
    _ = NSApplicationMain(CommandLine.argc, CommandLine.unsafeArgv)
  }

  /// 앱 묶음의 엔진 라이브러리를 열어 `kari_pdf_helper_with`를 부른다. 캔버스로 가져올 때 쪽 그림과 글자 인식은
  /// 엔진이 이 프로세스 안에서 `PdfPage.paint`를 불러 한다. 열지 못하면 2.
  private static func pdfHelper() -> Int32 {
    guard let dir = Bundle.main.privateFrameworksPath,
      let lib = dlopen(dir + "/kari.framework/kari", RTLD_NOW),
      let sym = dlsym(lib, "kari_pdf_helper_with")
    else { return 2 }
    typealias Helper = @convention(c) (@convention(c) (
      UnsafePointer<UInt8>?, Int, UInt32, Double, Int, UInt32, UnsafeMutablePointer<Int>?
    ) -> UnsafeMutablePointer<UInt8>?) -> Int32
    return unsafeBitCast(sym, to: Helper.self)(PdfPage.paint)
  }

  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return true
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
  }
}
