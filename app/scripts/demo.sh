#!/usr/bin/env bash
# 앱 사용법 시연(README의 움직이는 그림과 전체 시연 영상)을 다시 만든다.
#
#   app/scripts/demo.sh            # 프레임을 찍고 묶는다
#   app/scripts/demo.sh --encode   # 찍어 둔 프레임만 다시 묶는다
#
# 창을 띄우지 않는다. 위젯 시험(app/test/demo/demo_test.dart)이 앱을 그려 프레임을 PNG로 남기고,
# 이 스크립트가 ffmpeg와 img2webp(brew install ffmpeg webp)로 묶는다.
#
# 만드는 것
# - Docs/assets/readme/features/<클립>.webp, <클립>.poster.jpg: README의 기능 표
# - Docs/assets/readme/hero.jpg: README 맨 위의 그림
# - .cache/demo/kari-demo.mp4: 처음부터 끝까지 이은 시연(저장소에 넣지 않는다)
set -euo pipefail

app="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$app/.." && pwd)"
work="$root/.cache/demo"
dest="$root/Docs/assets/readme"
clips=(home writing format tables find page versions export canvas layers ai interview report)
hero=tables

# 멈춘 그림으로 쓸 프레임(클립의 n장 가운데). 기본은 마지막 장면이다.
poster_at() {
  case "$1" in
    home) echo $(($2 * 25 / 100)) ;;      # 휴지통 창
    find) echo $(($2 - 45)) ;;            # 바꾼 뒤, 찾기 막대를 닫기 전
    versions) echo $(($2 - 30)) ;;        # 되돌린 뒤, 버전 패널을 닫기 전
    interview) echo $(($2 * 62 / 100)) ;; # 꼬리질문에 답을 치는 중
    report) echo 40 ;;                    # 리포트 첫 화면
    *) echo $(($2 - 1)) ;;
  esac
}

if [[ "${1:-}" != "--encode" ]]; then
  rm -rf "$work/frames"
  (cd "$app" && KARI_DEMO_DIR="$work" flutter test test/demo/demo_test.dart)
fi

mkdir -p "$dest/features"
tmp="$work/tmp"
all="$work/all"
rm -rf "$all"
mkdir -p "$all"
n=0
for c in "${clips[@]}"; do
  src="$work/frames/$c"
  [[ -d "$src" ]] || { echo "프레임이 없다: $c" >&2; exit 1; }
  count="$(ls "$src" | wc -l | tr -d ' ')"
  last="$(printf '%05d.png' "$((count - 1))")"
  poster="$(printf '%05d.png' "$(poster_at "$c" "$count")")"

  # README 그림: 가로 960, 초당 30장.
  rm -rf "$tmp"
  mkdir -p "$tmp"
  ffmpeg -loglevel error -framerate 30 -i "$src/%05d.png" \
    -vf "fps=30,scale=960:-2:flags=lanczos" "$tmp/%05d.png"
  img2webp -loop 0 -lossy -q 85 -m 6 -d 33 "$tmp"/*.png -o "$dest/features/$c.webp" >/dev/null 2>&1
  ffmpeg -loglevel error -y -i "$src/$poster" -vf "scale=960:-2:flags=lanczos" -q:v 3 \
    "$dest/features/$c.poster.jpg"
  if [[ "$c" == "$hero" ]]; then
    ffmpeg -loglevel error -y -i "$src/$last" -vf "scale=1600:-2:flags=lanczos" -q:v 3 "$dest/hero.jpg"
  fi

  # 전체 영상에 이어 붙일 프레임.
  for f in "$src"/*.png; do
    ln -s "$f" "$all/$(printf '%06d' "$n").png"
    n=$((n + 1))
  done
  printf '%-10s %5.1f초 %6s\n' "$c" "$(echo "$count / 30" | bc -l)" \
    "$(du -h "$dest/features/$c.webp" | cut -f1)"
done
rm -rf "$tmp"

ffmpeg -loglevel error -y -framerate 30 -i "$all/%06d.png" \
  -vf "scale=1280:-2:flags=lanczos" -c:v libx264 -pix_fmt yuv420p -crf 20 -movflags +faststart \
  "$work/kari-demo.mp4"
rm -rf "$all"
echo "README 그림: $(du -ch "$dest/hero.jpg" "$dest/features"/* | tail -1 | cut -f1)"
echo "전체 영상: $work/kari-demo.mp4"
