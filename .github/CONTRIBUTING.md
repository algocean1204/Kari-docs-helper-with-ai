# Kari에 기여하기

보안 문제는 이슈 대신 [SECURITY.md](SECURITY.md)의 비공개 신고로 알려 주세요. 이 저장소의 모든 공간에는 [행동 강령](CODE_OF_CONDUCT.md)이 적용됩니다.

Kari는 유지 관리자 한 사람이 검토하고 병합합니다.

## 기여의 순서

1. **이슈 먼저.** 버그는 버그 신고 양식으로, 새 기능이나 큰 변경은 기능 제안 양식으로 이슈를 엽니다. 질문과 아이디어는 [디스커션](https://github.com/algocean1204/Kari-docs-helper-with-ai/discussions)에 남깁니다.
2. 유지 관리자가 이슈에 방향을 답하면 작업합니다. 오타나 문서의 작은 수정은 이 단계를 건너뛰어도 됩니다.
3. PR을 엽니다. 본문의 `Fixes #번호`로 이슈를 연결합니다. **연결된 이슈가 없는 외부 PR은 검토하지 않습니다.**

## 받는 것

| 받음 | 받지 않음 |
|---|---|
| 이슈로 재현된 버그 수정 | 이슈 없이 온 큰 변경 |
| 이슈에서 합의한 기능 | 판 번호, 태그, 릴리스의 변경(유지 관리자가 한다) |
| 문서 수정 | 회귀를 잡지 않는, 커버리지만 늘리는 시험 |
| 지식 팩의 개념 추가와 고침([작성법](../Docs/contributing/knowledge-pack-authoring.md)) | 실제 이력서, 연락처, 실제 회사 이름이 든 견본 자료 |

## 개발 환경

필요한 도구와 명령은 [Docs/contributing/build-and-test.md](../Docs/contributing/build-and-test.md), 고칠 곳은 [Docs/contributing/code-map.md](../Docs/contributing/code-map.md), 구조는 [Docs/architecture/](../Docs/architecture/)에 있습니다.

## 가지 이름

뜻이 드러나게 `fix/…`, `feat/…`, `docs/…`, `chore/…`로 짓습니다. 예: `fix/table-header-repeat`.

## 코드 지침

- 한 PR에는 한 가지 주제만 담습니다.
- 지금 구조를 따릅니다. 새 의존성이나 새 최상위 폴더는 이슈에서 먼저 합의합니다. 저장소 최상위에는 `README.md`, `LICENSE`, `.gitignore`, `.github/`, `app/`, `Docs/`만 둡니다(CI가 검사합니다).
- 동작을 바꾸면 그 동작을 잡는 시험을 함께 넣습니다.
- 화면의 색과 글자 값은 직접 적지 않고 `app/lib/design/tokens.dart`의 역할을 씁니다. 새 색을 넣으면 `app/test/design_test.dart`의 대비 목록에 더합니다.
- 의존성을 바꾸면 `python3 app/scripts/third_party.py`로 [Docs/THIRD_PARTY_LICENSES.md](../Docs/THIRD_PARTY_LICENSES.md)를 다시 만듭니다.
- 기기 밖으로 나가는 자료를 바꾸면 [Docs/guide/privacy.md](../Docs/guide/privacy.md)를 함께 고칩니다.
- API 키를 코드, 시험, 이슈, PR에 적지 않습니다. 시험에는 `sk-ant-test-…` 같은 가짜 값을 씁니다.
- 실제 서버 시험의 키는 `app/.env`에 둡니다(`app/.env.example`을 복사합니다. `.env`는 git이 무시합니다).
- 시험 자료의 전자우편 주소는 `example.com` 같은 예약된 도메인을 쓰고, 기기의 사용자 폴더 경로를 적지 않습니다. CI의 `public_check.py`가 봅니다.

## PR을 열기 전에

CI와 같은 검사입니다.

```bash
cd app/engine
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd ..
flutter analyze
flutter test
cd ..
python3 app/scripts/pack_check.py --doc Docs/architecture/knowledge-pack.md
python3 app/scripts/check_links.py
python3 app/scripts/public_check.py
```

## PR의 검토와 병합

- PR 양식을 모두 채웁니다. 화면이 바뀌면 전후 그림을 붙입니다.
- `main`에는 PR로만 들어갑니다. CI의 집계 검사(`ci-passed`)가 통과해야 병합할 수 있습니다.
- 병합은 스쿼시 병합 하나입니다. PR 하나가 커밋 하나가 되고, PR 제목이 커밋 제목이 됩니다. 제목은 `type(scope): 설명` 모양으로 씁니다. 예: `fix(layout): 머리 행이 다음 쪽에서 빠지는 문제`.
- 유지 관리자가 PR 가지에 커밋을 얹어 마무리하거나, 같은 내용을 새 PR로 다시 만들어 병합할 수 있습니다. 다시 만들 때는 원 작성자를 `Co-authored-by`로 남깁니다.
- 받지 않는 PR은 까닭을 적고 닫습니다.
- CLA나 DCO 서명은 요구하지 않습니다.

## 라이선스

기여한 것은 [MIT 라이선스](../LICENSE)로 배포됩니다. 번들 글꼴 Pretendard는 [SIL OFL 1.1](../app/assets/fonts/Pretendard-LICENSE.txt)을 따릅니다.
