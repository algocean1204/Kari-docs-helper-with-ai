#!/bin/bash
# Kari 베타 배포용 .dmg를 만들고 확인한다(Apple Silicon, macOS 13 이상).
#
#   app/scripts/package_dmg.sh                    # 지금 커밋(HEAD)의 사본을 임시 작업 트리로 만들어 빌드부터
#   app/scripts/package_dmg.sh --src <폴더>        # 이미 있는 깨끗한 사본에서 빌드부터
#   app/scripts/package_dmg.sh --app <Kari.app>   # 이미 빌드된 앱 묶음으로(복사본에 서명한다. 원본은 건드리지 않는다)
#   --out <폴더>                               # 결과를 둘 곳. 기본 .cache/dist
#
# 하는 일: 릴리스 빌드 → 다시 서명(디버그용 get-task-allow 빼기) → 서명·권한·아키텍처·최소 macOS 확인
#          → .dmg(Kari.app, Applications 바로가기, 처음 여는 법.txt, 안내, 라이선스) → 마운트해 꺼낸 앱을 다시 확인
#          → 격리 표시(com.apple.quarantine)를 붙였을 때 Gatekeeper(spctl)의 판단을 출력.
#
# 서명 신원
# - 기본은 임시 서명(-). 인증서가 없어도 되지만 Apple의 공증을 받지 못해 받는 사람이 처음 한 번 "그래도 열기"를 해야 한다.
# - KARI_SIGN_IDENTITY="Developer ID Application: 이름 (팀ID)"를 주면 그 인증서로 서명한다. 공증에 필요한
#   강화된 런타임(--options runtime)과 시각 도장(--timestamp)을 붙이고 dmg에도 서명한다.
# - 거기에 KARI_NOTARY_PROFILE=<이름>(`xcrun notarytool store-credentials <이름>`으로 만든 키체인 프로필)을 주면
#   dmg를 Apple에 공증 받고(notarytool submit --wait) 결과를 dmg에 붙인다(stapler staple).
#
# 빌드 번호
# - KARI_BUILD_NUMBER=<수>를 주면 `flutter build --build-number`로 넘긴다. 앱 업데이트(Sparkle)는 이 번호(CFBundleVersion)로
#   새 판인지 가린다. 주지 않으면 Flutter의 기본값(pubspec의 판)이다.
set -euo pipefail

ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
SCRIPTS_REL="$(cd "$(dirname "$0")" && pwd)"
SCRIPTS_REL="${SCRIPTS_REL#"$ROOT"/}"   # app/scripts
OUT="$ROOT/.cache/dist"
APP_IN=""
SRC=""
IDENTITY="${KARI_SIGN_IDENTITY:-}"
NOTARY="${KARI_NOTARY_PROFILE:-}"

usage() { sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; }
die() { echo "실패: $*" >&2; exit 1; }
step() { printf '\n== %s\n' "$*"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --app) APP_IN="$2"; shift 2 ;;
    --src) SRC="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[ -n "$NOTARY" ] && [ -z "$IDENTITY" ] && die "공증(KARI_NOTARY_PROFILE)에는 KARI_SIGN_IDENTITY가 필요합니다"

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
WORK="$(mktemp -d "$OUT/work.XXXXXX")"
WORKTREE=""
MNT="$WORK/mnt"
cleanup() {
  if [ -d "$MNT" ] && mount | grep -q " on $MNT "; then hdiutil detach -quiet "$MNT" || true; fi
  if [ -n "$WORKTREE" ]; then git -C "$ROOT" worktree remove --force "$WORKTREE" || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

# ── 1. 빌드(D1) ──────────────────────────────────────────────
if [ -z "$APP_IN" ]; then
  if [ -z "$SRC" ]; then
    SRC="$WORK/src"
    step "HEAD($(git -C "$ROOT" rev-parse --short HEAD))의 사본을 만든다: $SRC"
    git -C "$ROOT" worktree add --detach "$SRC" HEAD
    WORKTREE="$SRC"
  fi
  SRC="$(cd "$SRC" && pwd)"
  [ -z "$(git -C "$SRC" status --porcelain)" ] || die "$SRC에 커밋하지 않은 변경이 있습니다(깨끗한 사본에서만 빌드합니다)"
  step "릴리스 빌드: $SRC/app"
  BUILD_ARGS=()
  [ -n "${KARI_BUILD_NUMBER:-}" ] && BUILD_ARGS=(--build-number "$KARI_BUILD_NUMBER")
  (cd "$SRC/app" && flutter pub get && flutter build macos --release ${BUILD_ARGS[@]+"${BUILD_ARGS[@]}"})
  APP_IN="$SRC/app/build/macos/Build/Products/Release/Kari.app"
  DOCS="$SRC"
else
  [ -d "$APP_IN/Contents" ] || die "앱 묶음이 아닙니다: $APP_IN"
  DOCS="$ROOT"
fi

VERSION="$(sed -n 's/^version: *\([0-9][0-9.]*\).*/\1/p' "$DOCS/app/pubspec.yaml")"
[ -n "$VERSION" ] || die "app/pubspec.yaml에서 판을 읽지 못했습니다"
APP_VERSION="$(plutil -extract CFBundleShortVersionString raw "$APP_IN/Contents/Info.plist")"
[ "$APP_VERSION" = "$VERSION" ] || die "앱의 판($APP_VERSION)과 pubspec의 판($VERSION)이 다릅니다"
ENT="$DOCS/app/macos/Runner/Release.entitlements"
DMG="$OUT/Kari-$VERSION-beta-arm64.dmg"
# 받는 사람용 안내. KARI_GUIDE로 다른 글 파일을 줄 수 있다.
GUIDE="${KARI_GUIDE:-$DOCS/Docs/guide/install.md}"
NOTICE="$DOCS/Docs/THIRD_PARTY_LICENSES.md"
for f in "$GUIDE" "$DOCS/LICENSE" "$NOTICE"; do [ -f "$f" ] || die "$f가 없습니다"; done
python3 "$DOCS/$SCRIPTS_REL/third_party.py" --check || die "THIRD_PARTY_LICENSES.md가 의존성과 맞지 않습니다"

# ── 2. 다시 서명(D2) ─────────────────────────────────────────
APP="$WORK/Kari.app"
step "앱 묶음을 복사해 서명한다(원본: $APP_IN)"
ditto "$APP_IN" "$APP"
xattr -cr "$APP"  # Finder 정보 같은 확장 속성이 있으면 서명이 거절된다
# 실행 파일의 디버그 기호에는 빌드한 폴더의 경로가 들어 있다. 걷어 낸다(서명은 바로 아래에서 다시 한다).
strip -S -x "$APP/Contents/MacOS/Kari"
SIGN=(codesign --force --sign "${IDENTITY:--}")
[ -n "$IDENTITY" ] && SIGN+=(--options runtime --timestamp)
# 안쪽부터 바깥으로: 프레임워크 안의 dylib → 프레임워크 → 앱. 앱에는 Release.entitlements의 권한만 준다.
while IFS= read -r -d '' lib; do "${SIGN[@]}" "$lib"; done \
  < <(find "$APP/Contents/Frameworks" -type f -name '*.dylib' -print0)
# Sparkle(앱 업데이트) 안의 도구와 XPC 서비스. Sparkle 문서(Sandboxing)의 차례대로 강화된 런타임을 붙여 따로 서명한다.
# Downloader.xpc는 제 권한을 지킨다. --deep은 쓰지 않는다(Downloader의 권한이 다른 것에 번진다).
# 인증서로 서명할 때는 SIGN에 이미 --options runtime이 있다.
SPK="$APP/Contents/Frameworks/Sparkle.framework/Versions/B"
SPK_SIGN=("${SIGN[@]}")
[ -z "$IDENTITY" ] && SPK_SIGN+=(--options runtime)
if [ -d "$SPK" ]; then
  "${SPK_SIGN[@]}" "$SPK/XPCServices/Installer.xpc"
  "${SPK_SIGN[@]}" --preserve-metadata=entitlements "$SPK/XPCServices/Downloader.xpc"
  "${SPK_SIGN[@]}" "$SPK/Autoupdate"
  "${SPK_SIGN[@]}" "$SPK/Updater.app"
fi
for fw in "$APP"/Contents/Frameworks/*.framework; do "${SIGN[@]}" "$fw"; done
"${SIGN[@]}" --entitlements "$ENT" "$APP"

# 서명, 권한, 아키텍처, 최소 macOS를 확인한다. 하나라도 어긋나면 멈춘다.
check_app() {
  local app="$1" bin minos
  codesign -v --deep --strict --verbose=2 "$app"
  codesign -dv "$app" 2>&1 | grep -E '^(Identifier|Format|Signature|TeamIdentifier|Authority)=' || true
  codesign -d --entitlements - --xml "$app" 2>/dev/null > "$WORK/ent.plist"
  python3 - "$WORK/ent.plist" "$ENT" <<'PY'
import plistlib, sys
got, want = (plistlib.load(open(p, "rb")) for p in sys.argv[1:])
print("권한:", ", ".join(f"{k}={v}" for k, v in sorted(got.items())))
if got != want:
    sys.exit(f"권한이 Release.entitlements와 다릅니다. 더 있는 것: {sorted(set(got) - set(want))}, 빠진 것: {sorted(set(want) - set(got))}")
PY
  echo "LSMinimumSystemVersion: $(plutil -extract LSMinimumSystemVersion raw "$app/Contents/Info.plist")"
  for bin in "$app/Contents/MacOS/Kari" "$app"/Contents/Frameworks/*.framework/Versions/A/*; do
    [ -f "$bin" ] && [ -x "$bin" ] || continue
    minos="$(otool -l "$bin" | awk '/LC_BUILD_VERSION/{f=1} f&&$1=="minos"{print $2; exit}')"
    printf '  %-22s %-6s minos %-6s %s\n' "$(basename "$bin")" "$(lipo -archs "$bin")" "$minos" "$(du -h "$bin" | cut -f1)"
    [ "$(lipo -archs "$bin")" = "arm64" ] || die "$(basename "$bin")가 arm64 전용이 아닙니다"
    awk -v m="$minos" 'BEGIN { split(m, a, "."); exit !(a[1] < 13 || (a[1] == 13 && a[2] + 0 == 0)) }' \
      || die "$(basename "$bin")의 최소 macOS($minos)가 13.0보다 높습니다"
  done
  echo "앱 크기: $(du -sh "$app" | cut -f1)"
}
step "서명 확인"
check_app "$APP"

# 빌드한 기기의 사용자 폴더 경로가 앱 안에 남아 있으면 멈춘다(받는 사람에게 사용자 이름이 보인다).
if LC_ALL=C grep -r -a -q -F "$HOME" "$APP/Contents"; then
  die "앱 안에 빌드한 기기의 경로($HOME)가 남아 있습니다"
fi
echo "빌드한 기기의 경로: 없음"

# ── 3. dmg(D3, D5) ───────────────────────────────────────────
step "dmg를 만든다: $DMG"
STAGE="$WORK/stage"
mkdir -p "$STAGE/라이선스"
ditto "$APP" "$STAGE/Kari.app"
ln -s /Applications "$STAGE/Applications"
cp "$GUIDE" "$STAGE/처음 여는 법.txt"
# 안내가 가리키는 다른 글(쓰는 법, 면접, 개인정보)을 함께 넣는다.
mkdir -p "$STAGE/안내"
for f in writing interview privacy; do cp "$DOCS/Docs/guide/$f.md" "$STAGE/안내/$f.md"; done
cp "$DOCS/LICENSE" "$STAGE/라이선스/LICENSE.txt"
cp "$NOTICE" "$STAGE/라이선스/THIRD_PARTY_LICENSES.txt"
rm -f "$DMG"
hdiutil create -quiet -volname "Kari $VERSION beta" -srcfolder "$STAGE" -fs HFS+ -format UDZO "$DMG"
if [ -n "$IDENTITY" ]; then
  codesign --force --sign "$IDENTITY" --timestamp "$DMG"
  if [ -n "$NOTARY" ]; then
    xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY" --wait
    xcrun stapler staple "$DMG"
  fi
fi

# ── 4. 만든 dmg 확인(D3, D4) ─────────────────────────────────
step "dmg 확인"
hdiutil verify -quiet "$DMG" && echo "hdiutil verify: 통과"
mkdir -p "$MNT"
hdiutil attach -quiet -nobrowse -readonly -noautoopen -mountpoint "$MNT" "$DMG"
ls -la "$MNT" "$MNT/라이선스"
[ -d "$MNT/Kari.app" ] || die "dmg에 Kari.app이 없습니다"
[ "$(readlink "$MNT/Applications")" = "/Applications" ] || die "dmg에 Applications 바로가기가 없습니다"
[ -s "$MNT/처음 여는 법.txt" ] && [ -s "$MNT/안내/writing.md" ] || die "dmg에 안내 글이 없습니다"
[ -s "$MNT/라이선스/THIRD_PARTY_LICENSES.txt" ] && [ -s "$MNT/라이선스/LICENSE.txt" ] || die "dmg에 라이선스 고지가 없습니다"
EXTRACT="$WORK/extract/Kari.app"
mkdir -p "$WORK/extract"
ditto "$MNT/Kari.app" "$EXTRACT"
hdiutil detach -quiet "$MNT"

step "꺼낸 앱의 서명 확인"
check_app "$EXTRACT"

step "격리 표시를 붙였을 때(다른 기기에서 받은 것처럼)"
QUARANTINE="0083;$(printf '%x' "$(date +%s)");Safari;"
xattr -w com.apple.quarantine "$QUARANTINE" "$EXTRACT"
echo "\$ spctl -a -vv -t execute Kari.app"
spctl -a -vv -t execute "$EXTRACT" 2>&1 || true
cp "$DMG" "$WORK/q.dmg"
xattr -w com.apple.quarantine "$QUARANTINE" "$WORK/q.dmg"
echo "\$ spctl -a -vv -t open --context context:primary-signature $(basename "$DMG")"
spctl -a -vv -t open --context context:primary-signature "$WORK/q.dmg" 2>&1 || true
if command -v syspolicy_check >/dev/null; then
  echo "\$ syspolicy_check distribution Kari.app"
  syspolicy_check distribution "$EXTRACT" 2>&1 | head -20 || true
fi

step "끝"
echo "dmg: $DMG ($(du -h "$DMG" | cut -f1))"
shasum -a 256 "$DMG"
