#!/usr/bin/env bash
# 다음 판 번호를 계산한다. 지난 v* 태그(없으면 v0.0.0으로 보고 기록 전체)부터 <끝>까지의 커밋 제목을 본다.
#   app/scripts/next_version.sh [<끝>]                 다음 판을 출력한다(기본 끝은 HEAD)
#   app/scripts/next_version.sh --check <판> [<끝>]    그 판이 규칙에 맞는지 종료 코드로 알린다
# 규칙(.github/CONTRIBUTING.md의 '판 번호'): 지난 판이 0.x이면 feat나 !가 하나라도 있을 때 MINOR를 올리고 PATCH는 0,
# 아니면 PATCH를 올린다. 1.0.0은 유지 관리자가 정하는 것이라 지난 판이 0.x일 때만 --check가 받는다.
# 끝이 바로 태그가 가리키는 커밋이면 그 태그는 건너뛰고 그 앞의 태그를 지난 판으로 본다(릴리스 때 태그를 단 커밋을 볼 수 있게).
# 시험판 꼴의 태그(v0.2.0-beta 등)는 지난 판으로 보지 않는다. bash 3.2(macOS)와 git만 쓴다.
set -euo pipefail

die() { echo "next_version: $*" >&2; exit 1; }

check= end=HEAD
while [ $# -gt 0 ]; do
  case "$1" in
    --check) check=${2:?--check에 판이 필요합니다}; shift 2 ;;
    -h | --help) sed -n '2,8p' "$0"; exit 0 ;;
    *) end=$1; shift ;;
  esac
done
end_sha=$(git rev-parse --verify -q "$end^{commit}") || die "끝 '$end'를 찾을 수 없습니다"

ver_re='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
last_tag() { git describe --tags --abbrev=0 --match 'v[0-9]*.[0-9]*.[0-9]*' --exclude 'v*-*' "$1" 2>/dev/null || true; }
tag=$(last_tag "$end_sha")
if [ -n "$tag" ] && [ "$(git rev-parse "$tag^{commit}")" = "$end_sha" ]; then tag=$(last_tag "$end_sha^"); fi
base=${tag#v}
[ -n "$tag" ] || base=0.0.0
[[ $base =~ $ver_re ]] || die "지난 태그 '$tag'가 MAJOR.MINOR.PATCH 꼴이 아닙니다"
maj=${BASH_REMATCH[1]} min=${BASH_REMATCH[2]} pat=${BASH_REMATCH[3]}

feat= bang= count=0
re='^([a-z]+)(\([a-z0-9-]+\))?(!)?: '
subjects=$(git log --no-merges --format=%s "${tag:+$tag..}$end_sha")
while IFS= read -r subject; do
  [ -n "$subject" ] || continue
  count=$((count + 1))
  if [[ $subject =~ $re ]]; then
    if [ "${BASH_REMATCH[1]}" = feat ]; then feat=1; fi
    if [ -n "${BASH_REMATCH[3]}" ]; then bang=1; fi
  fi
done <<< "$subjects"
[ "$count" -gt 0 ] || die "지난 판($base) 뒤에 새 커밋이 없습니다"

if [ "$maj" = 0 ]; then
  if [ -n "$feat$bang" ]; then next="0.$((min + 1)).0"; else next="0.$min.$((pat + 1))"; fi
elif [ -n "$bang" ]; then next="$((maj + 1)).0.0"
elif [ -n "$feat" ]; then next="$maj.$((min + 1)).0"
else next="$maj.$min.$((pat + 1))"
fi

if [ -z "$check" ]; then echo "$next"; exit 0; fi

[[ $check =~ $ver_re ]] || die "판 '$check'가 MAJOR.MINOR.PATCH 꼴이 아닙니다(앞에 v를 붙이지 않습니다)"
c1=${BASH_REMATCH[1]} c2=${BASH_REMATCH[2]} c3=${BASH_REMATCH[3]}
if [ "$c1" -lt "$maj" ] ||
  { [ "$c1" -eq "$maj" ] && [ "$c2" -lt "$min" ]; } ||
  { [ "$c1" -eq "$maj" ] && [ "$c2" -eq "$min" ] && [ "$c3" -le "$pat" ]; }; then
  die "판 ${check}는 지난 판 ${base}보다 커야 합니다"
fi
if [ "$check" = "$next" ] || { [ "$maj" = 0 ] && [ "$check" = 1.0.0 ]; }; then
  echo "판 $check: 규칙에 맞습니다(지난 판 $base, 규칙의 다음 판 $next)"
else
  die "판 ${check}는 규칙에 맞지 않습니다. 지난 판 $base 뒤의 커밋 제목으로는 ${next}입니다. 1.0.0은 유지 관리자가 정할 때만 씁니다"
fi
