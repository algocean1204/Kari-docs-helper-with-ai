import FlutterMacOS
import Sparkle

/// 앱 업데이트(채널 `kari/update`). 새 판 확인, 내려받기, 서명 확인, 설치, 다시 켜기는 Sparkle이 하고
/// 화면은 Dart(app/lib/update.dart)가 그린다. 이 파일은 Sparkle의 사용자 화면 자리(SPUUserDriver)를 채널로 잇기만 한다.
///
/// Dart → Swift: `info` → `{id, name}`(번들 ID와 이름), `start` → `{available, auto, version, build}`, `setAuto(bool)`, `check`, `answer(bool)`(새 판 알림에서 고른 것),
/// `retry`(설치하려고 앱을 끄는 요청을 다시 보낸다).
/// Swift → Dart: `found {version, build, notes}`, `status {kind, code?, message?}`.
/// `kind`: `latest`(새 판 없음), `downloading`, `installing`, `waiting`(설치를 기다리는데 앱이 아직 꺼지지 않음),
/// `error`, `idle`(일이 끝나거나 그만둠).
///
/// 서명이 맞는 꾸러미만 설치한다: 공개 키(Info.plist의 SUPublicEDKey)가 없으면 Sparkle을 시작하지 않는다.
/// 키가 없을 때 Sparkle은 코드 서명만으로 받는 옛 길을 열어 두는데, 이 앱은 임시 서명이라 그 길을 쓰면 안 된다.
/// 키가 있으면 Sparkle이 압축을 풀기 전에 EdDSA 서명을 보고(SUVerifyUpdateBeforeExtraction), 맞지 않으면
/// 설치하지 않고 `showUpdaterError`로 알린다.
@MainActor
final class AppUpdater: NSObject, SPUUserDriver {
  private let channel: FlutterMethodChannel
  private var updater: SPUUpdater?
  /// 새 판 알림에 답할 곳. 알림이 떠 있는 동안만 있다.
  private var pending: ((SPUUserUpdateChoice) -> Void)?
  /// 이번 업데이트를 사용자가 "지금 업데이트"로 고른 뒤인가. 고르지 않은 업데이트는 설치하지 않는다.
  private var chosen = false
  /// 설치 도구가 앱을 끄려다 앱이 아직 꺼지지 않았을 때 끄는 요청을 다시 보내는 곳.
  private var retryQuit: (() -> Void)?

  init(messenger: FlutterBinaryMessenger) {
    channel = FlutterMethodChannel(name: "kari/update", binaryMessenger: messenger)
    super.init()
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self else { return result(nil) }
      switch call.method {
      case "start":
        result(self.start())
      case "info":
        // 앱의 번들 ID와 이름. Dart가 키 보관의 서비스 이름과 화면의 앱 이름(Kari, Kari Dev)으로 쓴다.
        result([
          "id": Bundle.main.bundleIdentifier ?? "",
          "name": Bundle.main.object(forInfoDictionaryKey: "CFBundleName") as? String ?? "",
        ])
      case "setAuto":
        self.updater?.automaticallyChecksForUpdates = call.arguments as? Bool ?? true
        result(self.updater?.automaticallyChecksForUpdates ?? false)
      case "check":
        self.updater?.checkForUpdates()
        result(self.updater != nil)
      case "retry":
        self.retryQuit?()
        result(nil)
      case "answer":
        let install = call.arguments as? Bool ?? false
        self.chosen = install
        self.pending?(install ? .install : .dismiss)
        self.pending = nil
        result(nil)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }

  /// Info.plist의 공개 키가 EdDSA(ed25519) 공개 키 꼴(base64 32바이트)인가.
  static var hasKey: Bool {
    guard let s = Bundle.main.object(forInfoDictionaryKey: "SUPublicEDKey") as? String,
      let d = Data(base64Encoded: s.trimmingCharacters(in: .whitespacesAndNewlines))
    else { return false }
    return d.count == 32
  }

  /// 빌드 번호(CFBundleVersion)가 숫자인가. 소스에서 만든 개발 빌드는 판 글("0.2.2")이 빌드 번호가 되는데,
  /// Sparkle이 그것을 피드의 빌드 번호와 견주면 앞뒤가 뒤바뀔 수 있다(새 빌드를 옛 판으로 덮는다).
  static var numericBuild: Bool {
    guard let b = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String,
      !b.isEmpty
    else { return false }
    return b.unicodeScalars.allSatisfy { CharacterSet.decimalDigits.contains($0) && $0.isASCII }
  }

  /// 소스에서 만든 개발 앱(Debug·Profile 빌드, 번들 ID `dev.clonamic.kari.dev`)인가. AppInfo.xcconfig가 정한다.
  static var devApp: Bool { Bundle.main.bundleIdentifier?.hasSuffix(".dev") == true }

  /// Sparkle을 시작한다. 자동 확인이 켜져 있으면 Sparkle이 마지막 확인에서 24시간이 지날 때마다 확인한다.
  /// 개발 앱이거나 빌드 번호가 숫자가 아니면 시작하지 않는다(설정의 업데이트 줄이 그 까닭을 알린다).
  private func start() -> [String: Any] {
    if updater == nil, !Self.devApp, Self.hasKey, Self.numericBuild {
      let u = SPUUpdater(
        hostBundle: .main, applicationBundle: .main, userDriver: self, delegate: nil)
      do {
        try u.start()
        updater = u
      } catch {
        NSLog("Kari 업데이트를 시작하지 못함: %@", error.localizedDescription)
      }
    }
    let info = Bundle.main.infoDictionary
    return [
      "available": updater != nil, "auto": updater?.automaticallyChecksForUpdates ?? false,
      "version": info?["CFBundleShortVersionString"] as? String ?? "",
      "build": info?["CFBundleVersion"] as? String ?? "",
    ]
  }

  private func status(_ kind: String, _ error: Error? = nil) {
    var args: [String: Any] = ["kind": kind]
    if let e = error as NSError? {
      args["code"] = e.code
      args["message"] = e.localizedDescription
    }
    channel.invokeMethod("status", arguments: args)
  }

  // MARK: SPUUserDriver

  func show(
    _ request: SPUUpdatePermissionRequest,
    reply: @escaping (SUUpdatePermissionResponse) -> Void
  ) {
    // Info.plist의 SUEnableAutomaticChecks가 있어 Sparkle은 묻지 않는다. 물으면 지금 설정을 그대로 답한다.
    reply(
      SUUpdatePermissionResponse(
        automaticUpdateChecks: updater?.automaticallyChecksForUpdates ?? true,
        sendSystemProfile: false))
  }

  func showUserInitiatedUpdateCheck(cancellation: @escaping () -> Void) {}

  func showUpdateFound(
    with appcastItem: SUAppcastItem, state: SPUUserUpdateState,
    reply: @escaping (SPUUserUpdateChoice) -> Void
  ) {
    pending?(.dismiss)
    pending = reply
    chosen = false
    channel.invokeMethod(
      "found",
      arguments: [
        "version": appcastItem.displayVersionString,
        "build": appcastItem.versionString,
        "notes": appcastItem.itemDescription ?? "",
      ])
  }

  /// 새 판 알림이 떠 있는데 "업데이트 확인…"을 다시 누르면 Sparkle이 부른다. 알림은 Kari 창 안의 대화 상자라,
  /// 창이 다른 앱 뒤에 있거나 내려가 있으면 보이지 않는다. 앱을 앞으로 가져오고 창을 펼쳐 알림이 보이게 한다.
  func showUpdateInFocus() {
    guard !MainFlutterWindow.background else { return }
    if !NSApp.isActive { NSApp.activate(ignoringOtherApps: true) }
    if let w = NSApp.windows.first(where: { $0 is MainFlutterWindow }) {
      if w.isMiniaturized { w.deminiaturize(nil) }
      w.makeKeyAndOrderFront(nil)
    }
  }

  func showUpdateReleaseNotes(with downloadData: SPUDownloadData) {}

  func showUpdateReleaseNotesFailedToDownloadWithError(_ error: Error) {}

  func showUpdateNotFoundWithError(_ error: Error, acknowledgement: @escaping () -> Void) {
    status("latest")
    acknowledgement()
  }

  func showUpdaterError(_ error: Error, acknowledgement: @escaping () -> Void) {
    pending = nil
    chosen = false
    status("error", error)
    acknowledgement()
  }

  func showDownloadInitiated(cancellation: @escaping () -> Void) {
    status("downloading")
  }

  func showDownloadDidReceiveExpectedContentLength(_ expectedContentLength: UInt64) {}

  func showDownloadDidReceiveData(ofLength length: UInt64) {}

  func showDownloadDidStartExtractingUpdate() {}

  func showExtractionReceivedProgress(_ progress: Double) {}

  func showReady(toInstallAndRelaunch reply: @escaping (SPUUserUpdateChoice) -> Void) {
    // 사용자가 이번 판을 "지금 업데이트"로 골랐을 때만 설치한다. 설치 도구는 앱에 종료 요청(Apple 종료 이벤트)을 보낸다.
    // 고르지 않았으면 이번 설치를 그만둔다(.dismiss는 앱이 꺼질 때 설치할 수 있어 쓰지 않는다).
    if chosen {
      status("installing")
      reply(.install)
    } else {
      reply(.skip)
    }
  }

  func showInstallingUpdate(
    withApplicationTerminated applicationTerminated: Bool,
    retryTerminatingApplication: @escaping () -> Void
  ) {
    // 꺼지지 않았으면 끄는 요청이 막혔거나 아직 처리 중이다. Dart가 "다시 시도"를 보인다.
    retryQuit = applicationTerminated ? nil : retryTerminatingApplication
    status(applicationTerminated ? "installing" : "waiting")
  }

  func showUpdateInstalledAndRelaunched(_ relaunched: Bool, acknowledgement: @escaping () -> Void) {
    acknowledgement()
  }

  func dismissUpdateInstallation() {
    pending = nil
    chosen = false
    retryQuit = nil
    status("idle")
  }
}
