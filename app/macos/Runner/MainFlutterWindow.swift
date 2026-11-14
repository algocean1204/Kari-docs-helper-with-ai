import Cocoa
import CoreVideo
import FlutterMacOS
import ImageIO
import UniformTypeIdentifiers

/// 엔진이 그린 화면을 Flutter Texture로 올리는 접착 코드. 로직은 두지 않는다.
///
/// 버퍼 세 장을 돌려 쓴다: Flutter가 가져간 것, 가장 최근에 다 그린 것, 지금 그리는 것.
/// 엔진은 버퍼의 메모리에 직접 그리므로 프레임마다 화면 전체를 한 번 더 복사하지 않는다.
final class TextureBridge: NSObject, FlutterTexture {
  /// 지금 쓰는 텍스처와 Dart가 그리고 있는 텍스처.
  static var shared: TextureBridge?
  static var drawing: TextureBridge?
  /// Dart가 그리는 동안(시작부터 끝까지) 잡는다. 그동안 텍스처를 바꾸거나 풀지 않게 한다.
  static let frameLock = NSLock()

  private let registry: FlutterTextureRegistry
  private var buffers: [CVPixelBuffer] = []
  private let lock = NSLock()
  private var front = 0
  private var shown = -1
  private var back = -1
  let width: Int
  let height: Int
  private(set) var textureId: Int64 = 0

  init?(registry: FlutterTextureRegistry, width: Int, height: Int) {
    self.registry = registry
    self.width = width
    self.height = height
    super.init()
    let attrs: [CFString: Any] = [
      kCVPixelBufferIOSurfacePropertiesKey: [:] as CFDictionary,
      kCVPixelBufferMetalCompatibilityKey: true,
    ]
    for _ in 0..<3 {
      var buffer: CVPixelBuffer?
      CVPixelBufferCreate(
        kCFAllocatorDefault, width, height, kCVPixelFormatType_32BGRA, attrs as CFDictionary,
        &buffer)
      guard let buffer else { return nil }
      buffers.append(buffer)
    }
    textureId = registry.register(self)
  }

  var stride: Int { CVPixelBufferGetBytesPerRow(buffers[0]) }

  func copyPixelBuffer() -> Unmanaged<CVPixelBuffer>? {
    lock.lock()
    shown = front
    let buffer = buffers[front]
    lock.unlock()
    return Unmanaged.passRetained(buffer)
  }

  func beginFrame() -> UnsafeMutableRawPointer? {
    lock.lock()
    back = (0..<3).first { $0 != front && $0 != shown } ?? 0
    lock.unlock()
    CVPixelBufferLockBaseAddress(buffers[back], [])
    return CVPixelBufferGetBaseAddress(buffers[back])
  }

  func endFrame() {
    guard back >= 0 else { return }
    CVPixelBufferUnlockBaseAddress(buffers[back], [])
    lock.lock()
    front = back
    back = -1
    lock.unlock()
    registry.textureFrameAvailable(textureId)
  }

  /// 지금 텍스처를 풀고 [next]로 바꾼다(크기가 바뀌면 새로 만든 것, 화면을 떠나면 nil).
  static func replace(with next: TextureBridge?) {
    frameLock.lock()
    if let old = shared {
      old.registry.unregisterTexture(old.textureId)
    }
    shared = next
    frameLock.unlock()
  }
}

// Dart가 프레임마다 FFI로 직접 부르는 C 함수. 주소는 채널로 한 번만 넘긴다.
// 시작은 Dart가 아는 크기를 받아, 지금 텍스처와 크기가 다르면(바꾸는 중) 그리지 않게 nil을 돌려준다.
// 시작이 nil이어도 Dart는 끝을 부른다.
private let beginFrameC: @convention(c) (Int, Int) -> UnsafeMutableRawPointer? = {
  width, height in
  TextureBridge.frameLock.lock()
  guard let bridge = TextureBridge.shared, bridge.width == width, bridge.height == height
  else { return nil }
  TextureBridge.drawing = bridge
  return bridge.beginFrame()
}
private let endFrameC: @convention(c) () -> Void = {
  TextureBridge.drawing?.endFrame()
  TextureBridge.drawing = nil
  TextureBridge.frameLock.unlock()
}

/// 메뉴 막대(Flutter의 PlatformMenuBar)에 macOS만 할 수 있는 것을 맞춘다. Flutter는 메뉴가 바뀔 때마다 주 메뉴를
/// 통째로 새로 만들므로 그때마다 다시 한다: 이름 앞의 표시(U+200B, app_menu.dart의 checkMark)를 떼어 체크로 바꾸고,
/// `도움말`은 메뉴 검색 칸이 붙는 도움말 메뉴로, `창`은 창 목록이 붙는 창 메뉴로, `서비스`는 서비스 메뉴로 알린다.
enum MenuBarFix {
  static let checkMark = "\u{200B}"

  static func apply(_ menu: NSMenu?) {
    guard let menu else { return }
    for item in menu.items {
      if item.title == "도움말" {
        // 항목이 없으면 Flutter가 하위 메뉴를 만들지 않는다. 빈 메뉴에도 macOS가 검색 칸을 둔다.
        if item.submenu == nil { item.submenu = NSMenu(title: item.title) }
        item.isEnabled = true
        NSApp.helpMenu = item.submenu
      }
      if item.title == "창" { NSApp.windowsMenu = item.submenu }
      if let sub = item.submenu { mark(sub) }
    }
  }

  private static func mark(_ menu: NSMenu) {
    for item in menu.items {
      if item.title.hasPrefix(checkMark) {
        item.title.removeFirst()
        item.state = .on
      }
      if item.title == "서비스", let sub = item.submenu { NSApp.servicesMenu = sub }
      if let sub = item.submenu { mark(sub) }
    }
  }
}

/// 실제 앱 시험(`integration_test/menu_test.dart`)만 쓰는 채널 `kari/menu-test`. `KARI_MENU_TEST=1`일 때만 연다.
/// 키는 앱 안에서 NSEvent를 만들어 앱의 이벤트 줄(`NSApp.postEvent`)에 넣는다(사용자의 자판과 다른 앱에는 가지 않는다).
/// 그래서 사람이 친 키처럼 다음 차례에 macOS의 key equivalent 순서(창의 보기 → 주 메뉴)를 지난다.
/// 메뉴 고르기와 조합 글도 채널의 응답을 보낸 뒤 다음 차례에 한다(사람의 입력은 채널 처리 도중에 오지 않는다).
enum MenuTest {
  /// 키가 지난 길: 앱이 키를 꺼낸 때의 상태(앞에 있는지, 키 창, 첫 응답자)와 키로 불린 메뉴 항목. `route`가 비운다.
  private static var route: [String] = []
  private static var watchers: [Any] = []

  static func register(_ messenger: FlutterBinaryMessenger) {
    guard ProcessInfo.processInfo.environment["KARI_MENU_TEST"] == "1" else { return }
    recordExit()
    // 창이 가려져 있거나 화면이 잠겨 있으면 macOS가 앱의 타이머를 늦춘다(App Nap). 시험 중에는 늦추지 않게 한다.
    watchers.append(
      ProcessInfo.processInfo.beginActivity(
        options: [.userInitiatedAllowingIdleSystemSleep, .latencyCritical], reason: "menu test"))
    if let m = NSEvent.addLocalMonitorForEvents(matching: .keyDown, handler: { e in
      let responder = e.window?.firstResponder.map { String(describing: type(of: $0)) } ?? "-"
      route.append(
        "key active=\(NSApp.isActive) keyWindow=\(NSApp.keyWindow != nil) responder=\(responder)")
      return e
    }) {
      watchers.append(m)
    }
    watchers.append(
      NotificationCenter.default.addObserver(
        forName: NSMenu.didSendActionNotification, object: nil, queue: nil
      ) { n in
        route.append("menu \((n.userInfo?["MenuItem"] as? NSMenuItem)?.title ?? "?")")
      })
    let channel = FlutterMethodChannel(name: "kari/menu-test", binaryMessenger: messenger)
    channel.setMethodCallHandler { call, result in
      let args = call.arguments as? [String: Any] ?? [:]
      switch call.method {
      case "menus":
        result((NSApp.mainMenu?.items ?? []).map(describe))
      case "route":
        result(route)
        route = []
      case "key":
        result(key(args))
      case "select":
        guard let (menu, idx) = find(args["path"] as? [String] ?? []), menu.items[idx].isEnabled
        else {
          result(false)
          return
        }
        result(true)
        DispatchQueue.main.async { menu.performActionForItem(at: idx) }
      case "compose":
        // 입력기의 조합 중인 글(marked text)을 Flutter의 입력 채널에 넣는다.
        activate()
        let window = NSApp.keyWindow ?? NSApp.windows.first
        guard let client = window?.firstResponder as? NSTextInputClient else {
          result(false)
          return
        }
        let text = args["text"] as? String ?? ""
        result(true)
        DispatchQueue.main.async {
          client.setMarkedText(
            text, selectedRange: NSRange(location: (text as NSString).length, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0))
        }
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }

  /// 시험 도중 앱이 끝난 까닭을 앱 컨테이너의 `tmp/kari-menu-test-exit.log`에 남긴다: 끝내기(terminate)와 문서 창
  /// 닫기는 그때의 호출 스택, 그 밖의 exit는 atexit에서의 스택, flutter 도구가 보낸 SIGTERM은 그 표시.
  /// 충돌(abort, 잘못된 메모리 접근)은 macOS 충돌 보고서에 남으므로 여기서 잡지 않는다.
  private static var exitLog: Int32 = -1

  private static func note(_ s: String) {
    _ = "\(Date()) \(s)\n".withCString { write(exitLog, $0, strlen($0)) }
  }

  private static func recordExit() {
    exitLog = open(
      NSTemporaryDirectory() + "kari-menu-test-exit.log", O_WRONLY | O_CREAT | O_TRUNC | O_APPEND,
      0o644)
    let stack = { Thread.callStackSymbols.joined(separator: "\n") }
    let center = NotificationCenter.default
    watchers.append(
      center.addObserver(forName: NSApplication.willTerminateNotification, object: nil, queue: nil)
      { _ in note("terminate\n\(stack())") })
    watchers.append(
      center.addObserver(forName: NSWindow.willCloseNotification, object: nil, queue: nil) { n in
        if n.object is MainFlutterWindow { note("window close\n\(stack())") }
      })
    atexit { MenuTest.note("exit\n" + Thread.callStackSymbols.joined(separator: "\n")) }
    signal(SIGTERM) { _ in
      MenuTest.note("SIGTERM")
      signal(SIGTERM, SIG_DFL)
      raise(SIGTERM)
    }
  }

  private static func describe(_ item: NSMenuItem) -> [String: Any] {
    var mods: [String] = []
    let m = item.keyEquivalentModifierMask
    if m.contains(.control) { mods.append("ctrl") }
    if m.contains(.option) { mods.append("alt") }
    if m.contains(.shift) { mods.append("shift") }
    if m.contains(.command) { mods.append("cmd") }
    return [
      "title": item.isSeparatorItem ? "-" : item.title,
      "enabled": item.isEnabled,
      "on": item.state == .on,
      "key": item.keyEquivalent,
      "mods": mods,
      "help": item.submenu != nil && item.submenu === NSApp.helpMenu,
      "children": (item.submenu?.items ?? []).map(describe),
    ]
  }

  /// 앱을 앞으로 가져온다(키와 입력기는 앞 창에만 간다). 배경에서 도는 시험이면 가져오지 않는다.
  private static func activate() {
    if !NSApp.isActive && !MainFlutterWindow.background { NSApp.activate(ignoringOtherApps: true) }
  }

  /// 수정 키: 이름, 왼쪽 키의 keyCode(kVK_*), 그 키가 눌렸을 때의 플래그. 플래그에는 왼쪽 키를 가리키는 장치 비트
  /// (NX_DEVICEL*KEYMASK)도 넣는다. 실제 자판이 넣는 값이고, Flutter는 이 비트로 어느 수정 키가 눌렸는지 본다.
  private static let modifierKeys: [(String, UInt16, NSEvent.ModifierFlags)] = [
    ("shift", 56, [.shift, NSEvent.ModifierFlags(rawValue: 0x02)]),
    ("ctrl", 59, [.control, NSEvent.ModifierFlags(rawValue: 0x01)]),
    ("alt", 58, [.option, NSEvent.ModifierFlags(rawValue: 0x20)]),
    ("cmd", 55, [.command, NSEvent.ModifierFlags(rawValue: 0x08)]),
  ]

  /// 실제 자판처럼 키 하나를 보낸다: 수정 키를 하나씩 누르고(flagsChanged), 키를 누르고 떼고(keyDown, keyUp),
  /// 수정 키를 누른 반대 차례로 하나씩 놓는다.
  private static func key(_ a: [String: Any]) -> Bool {
    activate()
    guard let window = NSApp.keyWindow ?? NSApp.windows.first else { return false }
    window.makeKey()
    let mods = Set(a["mods"] as? [String] ?? [])
    let held = modifierKeys.filter { mods.contains($0.0) }
    let code = UInt16(a["code"] as? Int ?? 0)
    let chars = a["chars"] as? String ?? ""
    let plain = a["plain"] as? String ?? chars
    func event(_ type: NSEvent.EventType, _ f: NSEvent.ModifierFlags, _ keyCode: UInt16)
      -> NSEvent?
    {
      NSEvent.keyEvent(
        with: type, location: .zero, modifierFlags: f,
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, characters: type == .flagsChanged ? "" : chars,
        charactersIgnoringModifiers: type == .flagsChanged ? "" : plain, isARepeat: false,
        keyCode: keyCode)
    }
    var events: [NSEvent?] = []
    var flags: NSEvent.ModifierFlags = []
    for (_, keyCode, f) in held {
      flags.formUnion(f)
      events.append(event(.flagsChanged, flags, keyCode))
    }
    events.append(event(.keyDown, flags, code))
    events.append(event(.keyUp, flags, code))
    for (_, keyCode, f) in held.reversed() {
      flags.subtract(f)
      events.append(event(.flagsChanged, flags, keyCode))
    }
    let list = events.compactMap { $0 }
    guard list.count == events.count else { return false }
    if a["path"] as? String == "window" {
      // 앱이 앞에 있을 때 macOS가 키를 보내는 차례를 그대로 따른다: ⌘가 눌린 키는 키 창의 보기, 주 메뉴 차례로
      // 묻고, 아무도 받지 않으면 창의 첫 응답자에게 간다. 화면이 잠겼거나 다른 앱이 앞에 있어 앱이 키 창을
      // 가질 수 없을 때에도(이때 macOS는 창을 건너뛰고 주 메뉴부터 묻는다) 창이 먼저 받는 길을 본다.
      DispatchQueue.main.async {
        for e in list {
          if e.type == .keyDown && e.modifierFlags.contains(.command) {
            if window.performKeyEquivalent(with: e) {
              route.append("window")
              continue
            }
            if NSApp.mainMenu?.performKeyEquivalent(with: e) == true { continue }
          }
          window.sendEvent(e)
        }
      }
      return true
    }
    for e in list { NSApp.postEvent(e, atStart: false) }
    return true
  }

  /// 메뉴 항목의 자리(그 메뉴와 차례). 사람이 메뉴를 눌렀을 때와 같이 `performActionForItem`으로 고른다.
  private static func find(_ path: [String]) -> (NSMenu, Int)? {
    var menu = NSApp.mainMenu
    for (i, title) in path.enumerated() {
      guard let m = menu, let idx = m.items.firstIndex(where: { $0.title == title }) else {
        return nil
      }
      if i == path.count - 1 { return (m, idx) }
      menu = m.items[idx].submenu
    }
    return nil
  }
}

class MainFlutterWindow: NSWindow {
  /// 시선 기록의 카메라(채널 `kari/gaze`, GazeMonitor.swift). 카메라는 Dart가 `start`를 부를 때만 연다.
  private var gaze: GazeMonitor?

  /// 앱 업데이트(채널 `kari/update`, Updater.swift). Sparkle은 Dart가 `start`를 부를 때 시작한다.
  private var updater: AppUpdater?

  /// 주 메뉴가 바뀌는 것을 본다(MenuBarFix).
  private var menuWatch: NSKeyValueObservation?

  /// 실제 앱 시험을 사용자의 작업을 가리지 않고 돌린다(`KARI_TEST_BACKGROUND=1`): Dock에 뜨지 않고, 스스로
  /// 앞으로 나오지 않고, 창은 다른 창들 뒤에 둔다. 없으면 보통 앱과 같다.
  static let background = ProcessInfo.processInfo.environment["KARI_TEST_BACKGROUND"] == "1"

  /// 창 닫기를 앱 끄기로 바꾸는 delegate(아래 QuitOnClose). 창은 delegate를 약하게 잡으므로 여기서 붙든다.
  private let quitOnClose = QuitOnClose()

  override func makeKeyAndOrderFront(_ sender: Any?) {
    if Self.background { orderBack(sender) } else { super.makeKeyAndOrderFront(sender) }
  }

  override func orderFront(_ sender: Any?) {
    if Self.background { orderBack(sender) } else { super.orderFront(sender) }
  }

  override func awakeFromNib() {
    if Self.background {
      // 앱이 다 뜨기 전(주 nib를 읽는 중)에 정해야 Dock에 뜨지 않는다. 창이 뜬 뒤에는 활성화를 아예 막는다:
      // flutter 도구는 앱에 붙은 뒤 `open`으로 앱을 앞으로 가져오고, accessory 앱은 밖에서 활성화될 수 있다.
      // prohibited로 시작하면 창이 뜨지 않으므로 창이 뜬 다음 차례에 바꾼다(바꿔도 떠 있는 창은 그대로다).
      // 한계: 이 코드보다 먼저 macOS가 앱을 보통 앱으로 등록하므로, 뜨는 순간 Dock에 잠깐 보일 수 있다.
      NSApp.setActivationPolicy(.accessory)
      DispatchQueue.main.async { NSApp.setActivationPolicy(.prohibited) }
    }
    let flutterViewController = FlutterViewController()
    self.contentViewController = flutterViewController
    if self.delegate == nil { self.delegate = quitOnClose }
    // 화면 배치가 지키는 가장 작은 창 크기(app/test/design_test.dart의 1024×700).
    self.setContentSize(NSSize(width: 1280, height: 800))
    self.contentMinSize = NSSize(width: 1024, height: 700)
    self.center()

    RegisterGeneratedPlugins(registry: flutterViewController)

    let registrar = flutterViewController.registrar(forPlugin: "TextureBridge")
    let channel = FlutterMethodChannel(name: "kari/texture", binaryMessenger: registrar.messenger)
    channel.setMethodCallHandler { call, result in
      switch call.method {
      case "create":
        guard let args = call.arguments as? [String: Any],
          let width = args["width"] as? Int,
          let height = args["height"] as? Int,
          let bridge = TextureBridge(registry: registrar.textures, width: width, height: height)
        else {
          result(FlutterError(code: "texture", message: "텍스처를 만들지 못했습니다", details: nil))
          return
        }
        TextureBridge.replace(with: bridge)
        result([
          "id": bridge.textureId,
          "stride": bridge.stride,
          "begin": unsafeBitCast(beginFrameC, to: Int.self),
          "end": unsafeBitCast(endFrameC, to: Int.self),
        ])
      case "dispose":
        TextureBridge.replace(with: nil)
        result(nil)
      default:
        result(FlutterMethodNotImplemented)
      }
    }

    // 주소와 파일을 운영체제에 넘기는 채널. `openUrl`은 링크를 기본 브라우저로 연다.
    // 엔진이 이미 검사한 주소지만 여기서도 스킴을 다시 본다. `openFolder`는 폴더를 Finder로 연다. `savePath`는 저장 대화 상자로
    // 고른 경로를 돌려주고, 취소하면 nil. `openPath`는 열기 대화 상자로 넣을 그림(PNG, JPEG)을 고른다.
    // `pasteImage`는 붙여넣기 판의 그림을 파일 경로로 준다(캔버스 문서의 그림 붙여넣기).
    // `print`와 `share`는 앱이 만든 PDF를 인쇄 창과 공유 창에 넘기고(Printing.swift), 창이 닫히거나 공유가 끝나면 답한다.
    let files = FlutterMethodChannel(
      name: "kari/files",
      binaryMessenger: flutterViewController.registrar(forPlugin: "Files").messenger)
    files.setMethodCallHandler { [weak self] call, result in
      switch call.method {
      case "openUrl":
        guard let args = call.arguments as? [String: Any],
          let text = args["url"] as? String,
          let url = URL(string: text),
          let scheme = url.scheme?.lowercased(),
          ["http", "https", "mailto", "tel"].contains(scheme)
        else {
          result(FlutterError(code: "url", message: "열 수 없는 주소입니다", details: nil))
          return
        }
        result(NSWorkspace.shared.open(url))
      case "openFolder":
        // 자료 폴더를 Finder로 연다(자료를 열지 못했을 때 사본을 떠 둘 수 있게).
        // 있는 폴더의 절대 경로만 받고, 꾸러미 폴더(.app 같은)를 실행하지 않도록 Finder 창으로만 보인다.
        var isDir: ObjCBool = false
        guard let path = (call.arguments as? [String: Any])?["path"] as? String,
          path.hasPrefix("/"),
          FileManager.default.fileExists(atPath: path, isDirectory: &isDir),
          isDir.boolValue
        else {
          result(false)
          return
        }
        result(NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: path))
      case "savePath":
        guard let self else {
          result(nil)
          return
        }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = (call.arguments as? [String: Any])?["name"] as? String ?? ""
        // 고를 수 있는 형식은 기본 이름의 확장자(pdf, docx, txt)를 따른다.
        let ext = (panel.nameFieldStringValue as NSString).pathExtension
        panel.allowedContentTypes = [UTType(filenameExtension: ext) ?? .pdf]
        panel.canCreateDirectories = true
        panel.beginSheetModal(for: self) { response in
          result(response == .OK ? panel.url?.path : nil)
        }
      case "openPath":
        // 넣을 그림 고르기. PNG와 JPEG만 고를 수 있다. 취소하면 nil.
        guard let self else {
          result(nil)
          return
        }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.png, .jpeg]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.beginSheetModal(for: self) { response in
          result(response == .OK ? panel.url?.path : nil)
        }
      case "pasteImage":
        result(pasteboardImage())
      case "print":
        // 앱이 만든 PDF를 인쇄 창(문서 창에 붙는 시트)에 넘긴다. 닫히면 끝까지 인쇄했는지(참·거짓).
        guard let self, let args = call.arguments as? [String: Any],
          let path = args["path"] as? String
        else {
          result(FlutterError(code: "print", message: "인쇄 창을 열지 못했습니다", details: nil))
          return
        }
        Printing.print(path: path, name: args["name"] as? String ?? "", window: self) { done in
          if let done {
            result(done)
          } else {
            result(FlutterError(code: "print", message: "PDF를 인쇄 창에 넘기지 못했습니다", details: nil))
          }
        }
      case "share":
        // 앱이 만든 PDF를 공유 창에 넘긴다. 자리는 Flutter의 논리 화소(창의 왼쪽 위 기준)다.
        guard let view = self?.contentViewController?.view, let args = call.arguments as? [String: Any],
          let path = args["path"] as? String
        else {
          result(FlutterError(code: "share", message: "공유 창을 열지 못했습니다", details: nil))
          return
        }
        let n = { (k: String) in (args[k] as? NSNumber)?.doubleValue ?? 0 }
        let y = view.isFlipped ? n("y") : view.bounds.height - n("y") - n("h")
        let rect = NSRect(x: n("x"), y: y, width: n("w"), height: n("h"))
        Printing.share(path: path, rect: rect, view: view) { done in
          switch done {
          case .success(let shared): result(shared)
          case .failure(let e):
            result(FlutterError(code: "share", message: e.localizedDescription, details: nil))
          }
        }
      default:
        result(FlutterMethodNotImplemented)
      }
    }

    gaze = GazeMonitor(messenger: flutterViewController.registrar(forPlugin: "Gaze").messenger)
    updater = AppUpdater(messenger: flutterViewController.registrar(forPlugin: "Update").messenger)

    menuWatch = NSApp.observe(\.mainMenu, options: [.initial, .new]) { app, _ in
      MenuBarFix.apply(app.mainMenu)
    }
    MenuTest.register(flutterViewController.registrar(forPlugin: "MenuTest").messenger)

    super.awakeFromNib()
  }
}

/// 붙여넣기 판의 그림. 그림이 없으면 nil, 있으면 `{path, temp}`(temp는 이 함수가 쓴 임시 파일인가), 받을 수 없으면
/// FlutterError. 앱은 temp인 파일만 지운다(복사한 사용자의 파일은 이름과 상관없이 지우지 않는다).
/// Finder에서 파일을 복사했으면(`public.file-url`) 판에 함께 실리는 파일 아이콘이 아니라 그 파일을 본다: 파일 하나가
/// 그림 파일이면 그 경로, 아니면 그림 없음. 그 밖에는 PNG와 JPEG는 바이트 그대로, TIFF는 PNG로 바꿔 임시 파일
/// (`kari-paste-`로 시작)에 쓴다(엔진은 PNG와 JPEG만 받는다). 바이트와 화소 상한을 넘으면 바꾸지 않고 오류로 답한다.
// 한계: 판에서 바이트를 꺼내는 것 자체는 크기를 미리 알 수 없어 한 번 읽는다
func pasteboardImage() -> Any? {
  let board = NSPasteboard.general
  // 엔진이 받는 파일 크기 상한(20MB)과 같다. 화소는 PNG로 바꿀 때 메모리를 지키는 상한이다.
  let maxBytes = 20 << 20
  let maxPixels = 25_000_000
  func fail(_ message: String) -> FlutterError {
    FlutterError(code: "image", message: message, details: nil)
  }
  if board.types?.contains(.fileURL) == true {
    let urls = board.readObjects(
      forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
    guard let urls, urls.count == 1,
      (try? urls[0].resourceValues(forKeys: [.contentTypeKey]))?.contentType?.conforms(to: .image)
        == true
    else { return nil }
    return ["path": urls[0].path, "temp": false] as [String: Any]
  }
  let jpeg = NSPasteboard.PasteboardType(UTType.jpeg.identifier)
  var found: (Data, String)?
  if let data = board.data(forType: .png) {
    found = (data, "png")
  } else if let data = board.data(forType: jpeg) {
    found = (data, "jpg")
  } else if let tiff = board.data(forType: .tiff) {
    guard tiff.count <= maxBytes else { return fail("클립보드의 그림이 20MB가 넘습니다") }
    guard let source = CGImageSourceCreateWithData(tiff as CFData, nil),
      let info = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
      let w = info[kCGImagePropertyPixelWidth] as? Int,
      let h = info[kCGImagePropertyPixelHeight] as? Int
    else { return fail("클립보드의 TIFF 그림을 받지 못했습니다") }
    guard w > 0, h > 0, w * h <= maxPixels else {
      return fail("클립보드의 그림이 너무 큽니다")
    }
    guard let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:])
    else { return fail("클립보드의 TIFF 그림을 받지 못했습니다") }
    found = (png, "png")
  }
  guard let (data, ext) = found else { return nil }
  guard data.count <= maxBytes else { return fail("클립보드의 그림이 20MB가 넘습니다") }
  let url = FileManager.default.temporaryDirectory
    .appendingPathComponent("kari-paste-\(UUID().uuidString).\(ext)")
  do {
    try data.write(to: url)
    return ["path": url.path, "temp": true] as [String: Any]
  } catch {
    return fail("클립보드의 그림을 임시 파일로 쓰지 못했습니다")
  }
}

/// 창은 하나뿐이라 닫으면 앱이 끝난다. 창을 먼저 닫으면 저장하지 못한 것이 있을 때 묻는 창을 띄울 자리가 없으므로,
/// 닫기(닫기 단추가 묻는 windowShouldClose)를 창이 살아 있는 동안의 끄기 요청으로 바꾼다. 끄기를 취소하면 창이 남는다.
final class QuitOnClose: NSObject, NSWindowDelegate {
  func windowShouldClose(_ sender: NSWindow) -> Bool {
    NSApp.terminate(sender)
    return false
  }
}
