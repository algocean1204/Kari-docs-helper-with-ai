# Kari에 기여하기

보안 문제는 이슈 대신 [SECURITY.md](SECURITY.md)의 비공개 신고로 알려 주세요. 이 저장소의 모든 공간에는 [행동 강령](CODE_OF_CONDUCT.md)이 적용됩니다.

Kari는 유지 관리자 한 사람이 검토하고 병합합니다. 모든 이슈와 PR은 유지 관리자의 검토를 거칩니다.

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

## 가지

- 오래 두는 가지는 둘입니다. `develop`은 다음 판을 모으는 가지이고(저장소의 기본 가지), `main`은 릴리스한 판과 릴리스할 판만 두는 가지입니다. 둘 다 언제나 빌드되고 시험을 통과하는 상태입니다.
- 흐름은 언제나 작업 가지 → `develop` → `main`입니다. 작업은 `develop`에서 딴 짧은 가지에서 하고, PR의 대상(base)은 `develop`입니다. 이슈 하나에 가지 하나, PR 하나입니다. 병합되면 가지는 자동으로 지워집니다.
- 이름은 `<type>/<짧은-주제>`입니다. type은 아래 [커밋과 PR 제목](#커밋과-pr-제목)의 type과 같고, 주제는 소문자, 숫자, 하이픈(`.`, `_`도 됨)으로 쓰며 첫 글자는 소문자나 숫자입니다. 예: `fix/table-header-repeat`, `feat/table-sort`, `docs/install-guide`, `test/docx-export`, `chore/ci-cache`. 규칙은 `^(feat|fix|docs|test|refactor|perf|build|ci|chore|revert)/[a-z0-9][a-z0-9._-]*$`이고 CI가 봅니다(의존성 도구의 `dependabot/` 가지는 그대로 통과합니다).
- `develop`과 `main`에는 PR로만 들어갑니다. 유지 관리자도 같은 순서(이슈 → 가지 → `develop`으로 PR)로 일합니다.
- `main`으로 가는 PR은 유지 관리자가 `develop`에서 엽니다. 다른 가지에서 `main`으로 연 PR은 CI가 거절합니다.
- 릴리스 태그(`v*`)는 유지 관리자만 만듭니다. 릴리스용 가지는 두지 않습니다.

## 커밋과 PR 제목

[Conventional Commits 1.0.0](https://www.conventionalcommits.org/ko/v1.0.0/)을 따릅니다. `develop`으로의 병합은 스쿼시라 PR 제목이 곧 커밋 제목이므로, 검사는 PR 제목에 합니다. 가지 안의 커밋 제목은 자유롭지만 같은 꼴로 쓰면 좋습니다([로컬 검사](#로컬에서-커밋-제목-검사하기)).

```
type(scope): 설명
```

| type | 쓰는 때 | 릴리스 글의 묶음 |
|---|---|---|
| `feat` | 사용자가 보는 새 기능 | 기능 |
| `fix` | 버그 수정 | 고침 |
| `perf` | 동작은 같고 빨라짐 | 성능 |
| `docs` | 문서만 | 문서 |
| `test` | 시험만 | 그 밖 |
| `refactor` | 동작이 같은 구조 정리 | 그 밖 |
| `build` | 빌드, 의존성, 패키징 | 그 밖 |
| `ci` | CI와 저장소 자동화 | 그 밖 |
| `chore` | 위에 들지 않는 정리 | 그 밖 |
| `revert` | 앞의 변경을 되돌림 | 그 밖 |

- **scope**는 선택입니다. 소문자, 숫자, 하이픈으로 씁니다. 어디를 고쳤는지 짧게 알리는 이름이면 됩니다. 예: `engine`(`app/engine` 전반), `layout`, `pdf`, `docx`, `render`, `store`, `ffi`, `ai`, `interview`, `app`(앱 화면), `canvas`, `editor`, `update`, `design`, `docs`, `ci`, `release`, `deps`.
- **설명**은 한국어로, 이 저장소의 지금 꼴대로 씁니다. 명령조가 아니라 일을 마친 꼴(`…을 맞춤`, `…을 더함`, `…를 고침`)이고, 마침표를 찍지 않으며, 1자 이상 72자 이하입니다(글자 수로 셉니다).
- **호환이 깨지는 변경**(저장된 문서를 못 읽게 되는 것, 설정이나 명령의 이름이 바뀌는 것)은 콜론 앞에 `!`를 붙입니다. 릴리스 글의 '호환이 깨지는 변경' 묶음에는 `!`가 붙은 제목이 오르므로 `!`를 꼭 붙이고, PR 본문에 `BREAKING CHANGE: 무엇이 어떻게 달라졌는지`를 한 줄 더합니다.
- 예:
  - `feat(canvas): 안내선과 같은 간격 맞춤을 더함`
  - `fix(layout): 머리 행이 다음 쪽에서 빠지는 문제를 고침`
  - `docs: 설치 안내에 처음 여는 법을 더함`
  - `refactor(store)!: 저장 꼴을 바꿈`

PR 제목, 가지 이름, 연결 이슈는 PR마다 CI의 `pr` 검사가 봅니다. 실패하면 무엇이 틀렸는지 알려 주고, 제목은 PR 화면에서 고치면 검사가 다시 돕니다.

### 로컬에서 커밋 제목 검사하기

강제는 아닙니다. 켜면 `git commit`이 제목을 같은 규칙으로 보고, 틀리면 거절합니다. `Merge`, `Revert "`, `fixup!`, `squash!`로 시작하는 제목은 통과합니다.

```bash
git config core.hooksPath app/scripts/hooks      # 켜기
git config --unset core.hooksPath                # 끄기
git commit --no-verify                           # 한 번만 건너뛰기
```

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
- `develop`과 `main`에는 PR로만 들어갑니다. CI의 집계 검사(`ci-passed`)가 통과해야 병합할 수 있습니다. 집계에는 PR 제목, 가지 이름, 연결 이슈 검사(`pr`)도 들어 있습니다.
- `develop`으로의 병합은 스쿼시 병합 하나입니다. PR 하나가 커밋 하나가 되고, PR 제목이 커밋 제목이 됩니다. 제목은 [위의 규칙](#커밋과-pr-제목)을 따릅니다.
- `develop`에서 `main`으로의 병합은 병합 커밋으로 합니다. `develop`의 커밋이 그대로 `main`에 남아 릴리스 글이 그 제목으로 묶입니다.
- 유지 관리자가 PR 가지에 커밋을 얹어 마무리하거나, 같은 내용을 새 PR로 다시 만들어 병합할 수 있습니다. 다시 만들 때는 원 작성자를 `Co-authored-by`로 남깁니다.
- 받지 않는 PR은 까닭을 적고 닫습니다.
- CLA나 DCO 서명은 요구하지 않습니다.

## 라이선스

기여한 것은 [MIT 라이선스](../LICENSE)로 배포됩니다. 번들 글꼴 Pretendard는 [SIL OFL 1.1](../app/assets/fonts/Pretendard-LICENSE.txt)을 따릅니다.
