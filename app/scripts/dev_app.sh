#!/bin/bash
# 소스에서 개발 앱(Kari Dev)을 만들어 /Applications/Kari Dev.app에 바꿔 깐다. 배포 앱(Kari)과 자료·키를 따로 쓴다.
#
#   app/scripts/dev_app.sh                    # 지금 커밋(HEAD)
#   app/scripts/dev_app.sh <커밋이나 가지>     # 그 커밋
#   --no-install                              # 빌드와 확인까지만 한다(깔지 않는다)
#
# 커밋한 것만 들어간다(임시 사본에서 빌드하고 끝나면 지운다). 빌드 번호는 dev.<짧은 커밋>이다.
set -euo pipefail

ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
REV=HEAD
INSTALL=1
DEST="/Applications/Kari Dev.app"
ID=dev.clonamic.kari.dev

usage() { sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; }
die() { echo "실패: $*" >&2; exit 1; }
step() { printf '\n== %s\n' "$*"; }
# 켜 둔 앱을 덮으면 그 앱이 망가진다. 끄는 것은 사용자에게 맡긴다.
running() { pgrep -xq "Kari Dev"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --no-install) INSTALL=0; shift ;;
    -h|--help) usage; exit 0 ;;
    -*) usage >&2; exit 2 ;;
    *) REV="$1"; shift ;;
  esac
done

SHA="$(git -C "$ROOT" rev-parse --verify --quiet "$REV^{commit}")" || die "커밋을 찾지 못했습니다: $REV"
SHORT="$(git -C "$ROOT" rev-parse --short "$SHA")"
BUILD="dev.$SHORT"
if [ "$INSTALL" = 1 ] && running; then die "Kari Dev가 켜져 있습니다. 끈 뒤 다시 실행해 주세요"; fi

mkdir -p "$ROOT/.cache"
WORK="$(mktemp -d "$ROOT/.cache/dev-app.XXXXXX")"
SRC="$WORK/src"
WORKTREE=""
cleanup() {
  if [ -n "$WORKTREE" ]; then git -C "$ROOT" worktree remove --force "$WORKTREE" || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

step "$REV($SHORT)의 사본을 만든다: $SRC"
git -C "$ROOT" worktree add --detach "$SRC" "$SHA"
WORKTREE="$SRC"

step "Profile 빌드"
(cd "$SRC/app" && flutter pub get && flutter build macos --profile)
APP="$SRC/app/build/macos/Build/Products/Profile/Kari Dev.app"
PLIST="$APP/Contents/Info.plist"
[ -d "$APP" ] || die "빌드한 앱이 없습니다: $APP"

# Flutter는 빌드 번호의 숫자가 아닌 글자를 지운다. 숫자가 아닌 번호를 바로 적고 같은 권한으로 다시 서명한다.
step "빌드 번호 $BUILD"
ENT="$WORK/entitlements.plist"
codesign -d --entitlements - --xml "$APP" > "$ENT"
plutil -replace CFBundleVersion -string "$BUILD" "$PLIST"
codesign --force --sign - --entitlements "$ENT" "$APP"
codesign --verify --deep --strict "$APP"

step "확인"
for kv in "CFBundleIdentifier=$ID" "CFBundleName=Kari Dev" "CFBundleVersion=$BUILD"; do
  got="$(plutil -extract "${kv%%=*}" raw "$PLIST")"
  [ "$got" = "${kv#*=}" ] || die "${kv%%=*}가 ${kv#*=}가 아니라 ${got}입니다"
  echo "${kv%%=*} = $got"
done

if [ "$INSTALL" = 0 ]; then
  step "깔지 않고 마친다(--no-install)"
  exit 0
fi
if running; then die "Kari Dev가 켜져 있습니다. 끈 뒤 다시 실행해 주세요"; fi
step "$DEST에 바꿔 깐다"
NEW="$(dirname "$DEST")/.Kari Dev.app.new"
rm -rf "$NEW"
ditto "$APP" "$NEW"
rm -rf "$DEST"
mv "$NEW" "$DEST"
echo "$DEST ($BUILD)"
