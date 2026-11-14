// 앱 업데이트 꾸러미와 appcast의 EdDSA(ed25519) 서명. 릴리스 워크플로가 macOS의 CryptoKit으로만 서명한다.
//
//   xcrun swift app/scripts/update_sign.swift pub                         비밀 키의 공개 키(base64 44자)
//   xcrun swift app/scripts/update_sign.swift sign <파일>                 파일 바이트의 서명(base64)
//   xcrun swift app/scripts/update_sign.swift feed <appcast>              Sparkle의 서명된 피드 꼴로 끝에 서명 묶음을 붙인다
//   xcrun swift app/scripts/update_sign.swift verify <공개 키> <파일> <서명>
//   xcrun swift app/scripts/update_sign.swift verify-feed <공개 키> <appcast>
//
// 비밀 키(base64 32바이트 씨앗)는 환경 변수 KARI_UPDATE_PRIVATE_KEY로만 받는다(명령 인자, 파일에 두지 않는다).
// 확인(verify, verify-feed)은 공개 키만 쓴다. 맞으면 0, 아니면 1로 끝난다.
//
// 피드 서명 묶음은 Sparkle 2.9.6의 꼴과 같다(common_cli/Signing.swift `signAppcast`,
// Sparkle/SPUExtractSignedFeed.m `SPUExtractAppcastContent`): 서명할 내용 바로 뒤에
// "<!-- sparkle-signatures:\nedSignature: <서명>\nlength: <내용 바이트 수>\n-->\n"을 붙이고, 앱은 맨 뒤의 묶음 앞까지를 내용으로 본다.

import CryptoKit
import Foundation

func fail(_ message: String) -> Never {
  FileHandle.standardError.write(Data((message + "\n").utf8))
  exit(1)
}

func privateKey() -> Curve25519.Signing.PrivateKey {
  let seed = ProcessInfo.processInfo.environment["KARI_UPDATE_PRIVATE_KEY"] ?? ""
  guard let raw = Data(base64Encoded: seed.trimmingCharacters(in: .whitespacesAndNewlines)),
    let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: raw)
  else { fail("KARI_UPDATE_PRIVATE_KEY가 EdDSA 비밀 키(base64 32바이트) 꼴이 아닙니다") }
  return key
}

func publicKey(_ base64: String) -> Curve25519.Signing.PublicKey {
  guard let raw = Data(base64Encoded: base64),
    let key = try? Curve25519.Signing.PublicKey(rawRepresentation: raw)
  else { fail("공개 키가 base64 32바이트 꼴이 아닙니다") }
  return key
}

func read(_ path: String) -> Data {
  guard let data = FileManager.default.contents(atPath: path) else { fail("읽지 못함: \(path)") }
  return data
}

func sign(_ data: Data) -> String {
  guard let sig = try? privateKey().signature(for: data) else { fail("서명하지 못함") }
  return sig.base64EncodedString()
}

let feedPrefix = Data("<!-- sparkle-signatures:\n".utf8)

/// 서명된 피드를 내용, 서명, 적힌 길이로 나눈다. 묶음이 없으면 nil.
func splitFeed(_ data: Data) -> (content: Data, signature: String, length: Int)? {
  guard let start = data.range(of: feedPrefix, options: .backwards),
    let end = data.range(of: Data("-->".utf8), in: start.upperBound..<data.endIndex),
    let block = String(data: data[start.upperBound..<end.lowerBound], encoding: .utf8)
  else { return nil }
  var signature = ""
  var length = -1
  for line in block.split(separator: "\n") {
    if line.hasPrefix("edSignature:") {
      signature = line.dropFirst("edSignature:".count).trimmingCharacters(in: .whitespaces)
    } else if line.hasPrefix("length:") {
      length = Int(line.dropFirst("length:".count).trimmingCharacters(in: .whitespaces)) ?? -1
    }
  }
  return (data[data.startIndex..<start.lowerBound], signature, length)
}

let args = Array(CommandLine.arguments.dropFirst())
switch (args.first, args.count) {
case ("pub", 1):
  print(privateKey().publicKey.rawRepresentation.base64EncodedString())
case ("sign", 2):
  print(sign(read(args[1])))
case ("feed", 2):
  let content = read(args[1])
  if content.range(of: feedPrefix) != nil { fail("이미 서명 묶음이 있습니다: \(args[1])") }
  let block = "<!-- sparkle-signatures:\nedSignature: \(sign(content))\nlength: \(content.count)\n-->\n"
  guard (try? (content + Data(block.utf8)).write(to: URL(fileURLWithPath: args[1]))) != nil else {
    fail("쓰지 못함: \(args[1])")
  }
case ("verify", 4):
  guard let sig = Data(base64Encoded: args[3]),
    publicKey(args[1]).isValidSignature(sig, for: read(args[2]))
  else { fail("서명이 맞지 않습니다: \(args[2])") }
case ("verify-feed", 3):
  guard let feed = splitFeed(read(args[2])), feed.length == feed.content.count,
    let sig = Data(base64Encoded: feed.signature),
    publicKey(args[1]).isValidSignature(sig, for: feed.content)
  else { fail("피드의 서명이 맞지 않습니다: \(args[2])") }
default:
  fail("쓰는 법은 파일 맨 위의 주석을 보세요")
}
