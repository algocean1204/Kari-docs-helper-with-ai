#!/usr/bin/env bash
# 릴리스 글의 바뀐 점을 만든다. 지난 태그부터 <끝>까지의 커밋 제목을 type으로 묶어 표준 출력에 쓴다.
#   app/scripts/release_notes.sh <끝 태그나 커밋> [<앞 태그나 커밋>]
# 앞을 생략하면 <끝> 바로 앞의 v* 태그를 쓰고, 없으면 기록 전체를 쓴다. 저장소 주소는 GITHUB_REPOSITORY에서 읽는다.
# 제목 규칙은 .github/CONTRIBUTING.md의 Conventional Commits. 꼴이 맞지 않는 제목은 '그 밖'에 그대로 둔다.
set -euo pipefail

end=${1:?끝 태그나 커밋이 필요합니다}
base=${2:-$(git describe --tags --abbrev=0 --match 'v*' "$end^" 2>/dev/null || true)}
range=${base:+$base..}$end

re='^([a-z]+)(\(([a-z0-9-]+)\))?(!)?: (.+)$'
breaking= feat= fix= perf= docs= other=
while IFS= read -r subject; do
  case "$subject" in '' | 'Merge '*) continue ;; esac
  type= scope= bang= desc=$subject
  if [[ $subject =~ $re ]]; then
    type=${BASH_REMATCH[1]} scope=${BASH_REMATCH[3]} bang=${BASH_REMATCH[4]} desc=${BASH_REMATCH[5]}
  fi
  line="- ${scope:+**$scope**: }$desc"$'\n'
  [ -z "$bang" ] || breaking+=$line
  case "$type" in
    feat) feat+=$line ;;
    fix) fix+=$line ;;
    perf) perf+=$line ;;
    docs) docs+=$line ;;
    *) other+=$line ;;
  esac
done < <(git log --no-merges --format=%s "$range")

section() { [ -z "$2" ] || printf '## %s\n\n%s\n' "$1" "$2"; }
section "호환이 깨지는 변경" "$breaking"
section "기능" "$feat"
section "고침" "$fix"
section "성능" "$perf"
section "문서" "$docs"
section "그 밖" "$other"
if [ -n "$base" ] && [ -n "${GITHUB_REPOSITORY:-}" ]; then
  echo "**전체 변경**: https://github.com/$GITHUB_REPOSITORY/compare/$base...$end"
fi
