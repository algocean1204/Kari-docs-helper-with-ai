# 지식 팩 작성

면접 지식 팩에 개념을 더하거나 고치는 법이다. 팩의 구조와 필드는 [지식 팩(온톨로지)](../architecture/knowledge-pack.md)에 있다.

## 고칠 파일

| 하는 일 | 파일 |
|---|---|
| 개념 더하기·고치기 | `app/engine/interview/pack/backend/<영역>.json`의 `concepts` |
| 개념 목록과 핵심 여부 | `app/engine/interview/src/pack.rs`의 `EXPECT`(검사기는 이 목록 밖의 개념을 거절한다) |
| 개념 수 | `app/engine/interview/tests/pack.rs`의 `builtin_pack_passes`(개념 수를 확인한다) |
| 영역 더하기 | `pack.json`의 `areas`, `pack.rs`의 `FILES`, `tests/pack.rs`의 영역 목록 |
| 문서의 개념 표 | `Docs/architecture/knowledge-pack.md`(아래 "검사"의 명령으로 다시 만든다) |

## 작성 규칙

| 대상 | 규칙 |
|---|---|
| 글 | 한국어. 기술 용어는 처음 나올 때 영문을 괄호로 붙인다 |
| 글의 출처 | 출처의 문장을 옮겨 적지 않고 새로 쓴다 |
| 체크포인트 `text` | 답에 들어 있어야 할 내용 하나를 "…를 말한다", "…를 설명한다"로 적는다. 체크포인트 하나에 생각 하나 |
| 체크포인트의 표현 | 기능으로 적는다. 제품 이름, 도구 이름, 숫자는 "예:"로만 둔다(다른 제품의 같은 수단으로 답해도 충족되게) |
| 필수(`must`) | 그 면을 묻는 열린 질문에 제대로 답했다면 빠질 수 없는 내용. 꼭 있는 면과 `principle`에 2~3개. 나머지는 더 깊은 내용으로 둔다. 공통 면(`estimate`, `verify`)에는 두지 않는다 |
| 결정형·운영형의 `principle` | "어떻게 동작하는가"는 `rationale`이나 `observe`에 섞지 않고 `principle`에 둔다 |
| 오개념 `claim` | 어떤 제품과 조건에서도 틀린 말만 적는다. 조건에 따라 맞는 말이면 조건을 `claim`과 `fix`에 적는다 |
| 오개념 `probe` | 그 생각이 틀렸음이 드러나는 상황을 묻는 질문 한 문장 |
| 시나리오 | 구체적인 상황 하나와 그것이 건드리는 면. `faces`는 그 개념에 있는 면만 |
| 트레이드오프 | 한쪽 편을 들지 않고 두 쪽(`a`, `b`)을 다 적는다 |
| 사실 | 출처에서 확인되는 것만. 의견, 유행, 특정 회사의 관행을 정답으로 적지 않는다. 판에 따라 다른 값은 판이나 조건을 함께 적는다 |
| 추론 | 출처의 문장에서 한 걸음 끌어낸 것까지. 틀릴 수 있는 조건이 있으면 체크포인트에 적는다 |
| 별칭 `aliases` | 이력서와 공고에 실제로 쓰이는 말(한글, 영문, 줄임말) 2~8개 |

## 출처 규칙

모든 체크포인트와 오개념에 `source`(`title`, `url`, `where`)를 붙인다. `url`은 `https://`로 시작한다. `where`에는 그 내용이 있는 절이나 단락 이름을 적는다.

| 고르는 순서 | 예 |
|---|---|
| 1. 공식 문서 | PostgreSQL 문서, Kubernetes 문서 |
| 2. 표준 | RFC |
| 3. 원 논문 | |
| 4. 널리 쓰는 교재와 저자가 직접 쓴 글 | 교재의 저자와 장, 저자의 사이트 |
| 5. 만든 곳의 기술 문서와 튜토리얼 | |

- 받는 것: 원 논문을 인용해 정리한 널리 쓰는 참고 자료.
- 받지 않는 것: 블로그 요약, 위키 미러, 질의응답 사이트, 남이 쓴 것을 다시 요약한 글.
- 주소는 직접 열어 그 절이 체크포인트를 뒷받침하는지 확인한 것만 적는다.
- 출처는 리포트에 제목과 주소로 보인다.

## 검사

```bash
python3 app/scripts/pack_check.py                 # 팩 전체. 엔진의 검사기와 같은 규칙
python3 app/scripts/pack_check.py app/engine/interview/pack/backend --partial   # 작성 중: 없는 영역 파일과 목록 밖의 개념을 문제 삼지 않는다
python3 app/scripts/pack_check.py --table         # 문서의 영역·개념 표를 출력한다
python3 app/scripts/pack_check.py --doc Docs/architecture/knowledge-pack.md # 문서의 표가 팩과 같은지 본다
(cd app/engine && cargo test -p kari-interview)   # 엔진이 팩을 읽고 검사하는 시험
```

- 개념을 더하거나 이름·유형·핵심 여부를 바꾸면 `--table`의 출력으로 `Docs/architecture/knowledge-pack.md`의 `<!-- pack-table:start -->`와 `<!-- pack-table:end -->` 사이를 바꾼다.
- 팩은 엔진 라이브러리에 글로 들어가므로, 팩을 고친 뒤에는 엔진과 앱을 다시 빌드해야 반영된다.
