<!-- 외부 기여는 연결된 이슈가 있어야 검토합니다. 먼저 이슈에서 방향을 맞춰 주세요. -->

## 연결 이슈

Fixes #

## 무엇을 바꿨나요

<!-- 한두 문장으로. -->

## 왜 이 방법인가요

<!-- 고려한 다른 방법과 고르지 않은 까닭. -->

## 화면 전후

<!-- 화면이 바뀌면 바뀌기 전과 뒤의 그림을 붙여 주세요. 화면이 바뀌지 않으면 N/A와 까닭. 견본 글만 보이게 해 주세요. -->

## 시험한 것

<!-- 돌린 명령, 실행해 본 macOS 판과 칩. -->

## 위험과 되돌리기

- 수준: 낮음 / 중간 / 높음
- 저장된 문서, 버전, 설정에 영향이 있나요:
- 되돌리는 법:

## 확인

- [ ] 한 가지 주제만 다룬다
- [ ] `app/engine`을 고쳤다면 `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`가 통과한다
- [ ] `app`을 고쳤다면 `flutter analyze`, `flutter test`가 통과한다
- [ ] 동작을 바꿨다면 그 동작을 잡는 시험을 넣었다
- [ ] 의존성을 바꿨다면 `python3 app/scripts/third_party.py`로 `Docs/THIRD_PARTY_LICENSES.md`를 다시 만들었다
- [ ] 기기 밖으로 나가는 자료가 바뀌었다면 `Docs/guide/privacy.md`를 고쳤다
- [ ] 판 번호(`app/pubspec.yaml`의 `version`)를 건드리지 않았다
- [ ] 유지 관리자가 이 가지에 커밋을 얹어도 된다(Allow edits by maintainers)
