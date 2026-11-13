# 코드 지도

무엇을 고치려면 어디를 보는지 정리한 표입니다. 구조의 설명은 [architecture/overview.md](../architecture/overview.md)에 있습니다.

## 저장소 구성

| 자리 | 내용 |
|---|---|
| `README.md`, `LICENSE` | 소개와 라이선스 |
| `.github/` | CI, 이슈·PR 틀, 기여 안내, 보안 정책, 행동 강령, `main` 보호 규칙 |
| `Docs/` | 공개 문서 |
| `app/` | 제품 코드 전부 |

| `app/` 안 | 내용 |
|---|---|
| `lib/` | Flutter 화면과 엔진 호출 |
| `macos/` | macOS 접착 코드(텍스처, 파일 대화 상자, 인쇄·공유, 카메라), 권한, Xcode 설정 |
| `hook/build.dart` | 엔진 라이브러리를 빌드해 앱에 싣는 빌드 훅 |
| `test/`, `integration_test/` | 앱 시험 |
| `engine/` | Rust 엔진 작업 공간. 크레이트 10개. Rust 판은 `engine/rust-toolchain.toml` |
| `assets/fonts/` | 앱과 엔진이 함께 쓰는 글꼴 Pretendard(SIL OFL 1.1) |
| `scripts/` | 배포와 검사 스크립트 |

## 엔진 크레이트

| 크레이트 | 폴더 | 맡은 일 |
|---|---|---|
| `kari-doc` | `app/engine/doc` | 메모리 안의 문서: 노드, op 적용과 역연산, 상태 해시, 형제 순서 키, 스키마 |
| `kari-store` | `app/engine/store` | SQLite 라이브러리 DB: 문서, 묶음 기록, 스냅샷, 버전, 되돌리기, 에셋, 설정, 백업, 범용 기록 |
| `kari-layout` | `app/engine/layout` | 줄 나눔, 쪽 나눔, 표와 변의 선, 문단 테두리, 표시 목록, 위치 질의 |
| `kari-edit` | `app/engine/edit` | 편집 명령 → 묶음(op 배열)과 그 뒤의 선택 |
| `kari-render` | `app/engine/render` | 표시 목록을 Skia로 화소 버퍼에 그리기, 그림 풀기 |
| `kari-pdf` | `app/engine/pdf` | 표시 목록을 krilla로 PDF로 |
| `kari-docx` | `app/engine/docx` | 문서 모델을 Word 문서(DOCX)와 텍스트로 |
| `kari-ai` | `app/engine/ai` | 키 보관, 제공사 어댑터(Anthropic, OpenAI) |
| `kari-interview` | `app/engine/interview` | 면접 엔진과 지식 팩(`pack/backend/`) |
| `kari-ffi` | `app/engine/ffi` | 앱과의 경계. 동적 라이브러리 `kari` |

## 무엇을 고치려면

| 고칠 것 | 볼 곳 |
|---|---|
| 노드의 종류와 속성 | `app/engine/doc/schema/flow.json`, `app/engine/doc/src/named_flow.rs`, `app/engine/doc/src/schema.rs` |
| op 적용과 역연산 | `app/engine/doc/src/apply.rs`, `app/engine/doc/src/ops.rs`, 글 수정은 `app/engine/doc/src/runs.rs` |
| 저장, 버전, 복원 | `app/engine/store/src/commit.rs`, `history.rs`, `undo.rs` |
| 문서 목록(만들기, 복제, 휴지통) | `app/engine/store/src/docs.rs`, 화면은 `app/lib/screens/home.dart`, 상태는 `app/lib/library.dart` |
| 줄 나눔 | `app/engine/layout/src/para.rs`, `lines.rs` |
| 쪽 나눔, 외톨이 줄, 머리글·바닥글 | `app/engine/layout/src/page.rs`, `layouter.rs` |
| 표의 배치와 변의 선 | `app/engine/layout/src/table.rs`, 눈금선 그리기는 `app/engine/render/src/lib.rs` |
| 커서, 선택, 누른 자리 | `app/engine/layout/src/query.rs`, `nav.rs`, `app/engine/edit/src/motion.rs` |
| 편집 명령 | `app/engine/edit/src/command.rs`에서 갈래를 보고 `text.rs`, `format.rs`, `style.rs`, `list.rs`, `link.rs`, `table.rs`, `table_look.rs`, `image.rs`, `page.rs`, `find.rs`, `clip.rs`, `case.rs`, `motion.rs` |
| 표 모양(선, 바탕, 표 속성) | `app/engine/edit/src/table_look.rs`, 화면은 `app/lib/screens/table_props.dart`(표 도구 줄, `선`·`바탕색` 메뉴, 표 속성 창, 경계 끌기) |
| 스타일 | `app/engine/edit/src/style.rs`, 상태와 자세한 값은 `app/engine/ffi/src/styles.rs`, 화면은 `app/lib/screens/style_dialog.dart`, 새 문서의 기본은 `app/lib/library.dart` |
| 문단 테두리와 바탕 | 배치는 `app/engine/layout/src/frame.rs`, 이웃 문단 잇기는 `layouter.rs`, 창의 묶음은 `style_dialog.dart`의 `BorderGroup` |
| 자동 서식 | `app/engine/edit/src/auto.rs`, 따로 된 묶음으로 잇는 것은 `app/engine/ffi/src/edit.rs`의 `run_auto`, 설정은 `app/lib/screens/settings.dart` |
| 개요, 쪽으로 가기 | `app/engine/edit/src/outline.rs`, `motion.rs`의 `go_to`, 화면은 `app/lib/screens/navigate.dart` |
| 메뉴 막대 | `app/lib/screens/app_menu.dart`(캔버스 문서의 메뉴는 `canvasMenus`) |
| Word 문서, 텍스트 내보내기 | `app/engine/docx/src/`(`body.rs` 본문·표·목록·머리글, `props.rs` 글자·문단 모양과 스타일, `txt.rs` 텍스트, `zip.rs` 꾸러미), 호출은 `app/engine/ffi/src/docx.rs` |
| 글자·문단 모양, 서식 지우기 | `app/engine/edit/src/format.rs`, 쪽 나누기는 `text.rs`, 번호 시작 값은 `list.rs`, 사용자 지정 용지는 `page.rs` |
| 첨자, 자간, 형광펜, 줄 간격의 배치 | `app/engine/layout/src/style.rs`, `para.rs`, `lines.rs` |
| 문단을 나누지 않기, 문단 앞에서 쪽 나누기 | `app/engine/layout/src/page.rs` |
| 편집 상태 보고(글자·문단 값) | `app/engine/ffi/src/state.rs` |
| 앱의 명령 JSON → 엔진 명령 | `app/engine/ffi/src/cmd.rs` |
| 키와 단축키 | `app/lib/editing/keys.dart`. 인쇄와 배율 키는 `app/lib/screens/doc.dart`, 메뉴 막대의 키는 `app/lib/screens/app_menu.dart` |
| 도구 줄, 글자·문단 모양 창 | `app/lib/screens/toolbar.dart`, `format_dialogs.dart`, 문서 색 견본은 `app/lib/editing/doc_colors.dart` |
| 보기 배율 | `app/engine/render/src/geo.rs`, `edit.zoom`은 `app/engine/ffi/src/edit.rs`, 벌리기·가로 스크롤·커서 따라가기는 `app/lib/screens/paper.dart`, 상태 줄의 배율 단추는 `find_bar.dart` |
| 인쇄, 공유 | `app/lib/screens/print_share.dart`, `app/macos/Runner/Printing.swift`, 채널은 `MainFlutterWindow.swift` |
| 한글 입력 | `app/lib/editing/ime.dart`, `app/lib/editing/session.dart`(캔버스는 `canvas_session.dart`), `app/engine/edit/src/utf16.rs` |
| 캔버스 화면 | 화면과 도구 줄은 `app/lib/screens/canvas.dart`, 판(포인터·스크롤·키)은 `canvas_surface.dart`, 속성 패널은 `canvas_props.dart`, 세션은 `app/lib/editing/canvas_session.dart`, 키 표는 `app/lib/editing/canvas_keys.dart`, 상태 모양은 `app/lib/engine/canvas_state.dart`. 쓰는 법은 [guide/canvas.md](../guide/canvas.md) |
| 화면 그리기 | `app/engine/render/src/lib.rs`, 텍스처는 `app/lib/editing/surface.dart`, `app/macos/Runner/MainFlutterWindow.swift` |
| 그림 넣기와 정리 | `app/engine/render/src/image.rs`, `app/engine/edit/src/image.rs`, `app/engine/store/src/assets.rs` |
| PDF | `app/engine/pdf/src/lib.rs`, 호출은 `app/engine/ffi/src/pdf.rs` |
| 엔진 호출 이름 추가 | `app/engine/ffi/src/lib.rs`가 이름의 앞머리(`ai.`, `iv.`, `edit.`, 그 밖)로 모듈을 고르고, 각 모듈(`library.rs`, `edit.rs`, `ai.rs`, `iv.rs`)의 `dispatch`가 이름별로 나눈다. `ai.tasks`, `ai.setTask`, `ai.resetTasks`는 `task.rs`, `doc.exportPdf`는 `pdf.rs`, `doc.exportDocx`, `doc.exportTxt`는 `docx.rs`로 바로 가고, `iv.dashboard`는 `iv.rs`가 `dashboard.rs`로 넘긴다. 앱 쪽은 `app/lib/engine/engine.dart`와 시험용 흉내 `app/lib/engine/fake*.dart` |
| AI 키와 모델 호출 | 키는 `app/engine/ai/src/secrets.rs`, 공통 틀은 `lib.rs`(`Provider`), `types.rs`, `http.rs`, Anthropic은 `anthropic.rs`, `body.rs`, `stream.rs`, `reply.rs`, OpenAI는 `openai.rs`. 제공사 목록과 키 넣기는 `app/engine/ffi/src/ai.rs`, 화면은 `app/lib/screens/settings.dart`, `ai.dart` |
| 일마다의 모델 | `app/engine/ffi/src/task.rs`(일, 기본값, 판 고정), 계열·가장 새 판·가격은 `app/engine/ai/src/anthropic.rs`, `openai.rs`, 화면은 `app/lib/screens/ai_tasks.dart` |
| 면접의 진행 | `app/engine/interview/src/graph.rs`, `plan.rs`, `pick.rs` |
| 면접의 프롬프트 | `app/engine/interview/src/prompt.rs`(모든 프롬프트가 여기에 있다) |
| 채점과 점수 | `app/engine/interview/src/grade.rs`, `score.rs` |
| 리포트 | `app/engine/interview/src/report.rs`, 화면은 `app/lib/screens/report.dart` |
| 연락처 가리기 | `app/engine/interview/src/mask.rs` |
| 홈 대시보드 | `app/engine/interview/src/dashboard.rs`, 호출은 `app/engine/ffi/src/dashboard.rs`, 화면은 `app/lib/screens/home.dart` |
| 시선 기록 | 카메라는 `app/macos/Runner/GazeMonitor.swift`, 판정과 구간은 `app/lib/gaze.dart`, 켜고 끄는 화면은 `app/lib/screens/gaze.dart`, 저장과 리포트 줄은 `app/engine/interview/src/gaze.rs`와 `app/engine/ffi/src/iv.rs`의 `iv.gaze` |
| 지식 팩 | `app/engine/interview/pack/backend/*.json`, 검사는 `app/engine/interview/src/pack.rs`와 `app/scripts/pack_check.py`. 작성법은 [knowledge-pack-authoring.md](knowledge-pack-authoring.md) |
| 색과 글자 값 | `app/lib/design/tokens.dart`(대비 검사는 `app/test/design_test.dart`) |
| 공용 화면 부품 | `app/lib/design/components.dart` |
| 화면 | `app/lib/screens/`: `home.dart`(홈), `doc.dart`(문서 화면과 위쪽 줄), `name_field.dart`(문서 이름 칸), `paper.dart`(종이), `toolbar.dart`, `style_dialog.dart`(스타일 메뉴와 창), `table_props.dart`, `format_dialogs.dart`, `edit_dialogs.dart`(링크, 탭 정지, 쪽 설정, 그림 크기, 표 넣기 창), `navigate.dart`(개요 패널, 쪽으로 가기 창), `app_menu.dart`(메뉴 막대), `find_bar.dart`(찾기 막대, 상태 줄), `editor.dart`(견본 문서 화면), `versions.dart`, `canvas.dart`(캔버스 문서 화면), `canvas_surface.dart`, `canvas_props.dart`, `print_share.dart`, `interview.dart`, `gaze.dart`, `report.dart`, `settings.dart`, `ai.dart`(키 넣기 창), `ai_tasks.dart`, `dialogs.dart`(공고 추가, 보내는 내용, 이름 바꾸기, 휴지통 창) |
| 엔진 라이브러리를 앱에 싣는 것 | `app/hook/build.dart` |
| 앱의 권한 | `app/macos/Runner/Release.entitlements`, `DebugProfile.entitlements` |

## 시험의 자리

| 대상 | 폴더 |
|---|---|
| 엔진 크레이트 | `app/engine/<크레이트>/tests/` |
| 앱과 엔진의 경계 | `app/engine/ffi/tests/` |
| 앱(엔진 흉내로) | `app/test/` |
| 앱과 진짜 엔진 | `app/test/native_test.dart`, `app/test/edit_native_test.dart` |
| 창을 띄우는 실제 앱 시험 | `app/integration_test/app_test.dart`, `menu_test.dart`(메뉴 막대) |
| 실제 서버 시험(키가 있을 때만, 요금이 듦) | `app/engine/ai/tests/live.rs`, `app/engine/interview/tests/live.rs`, `quality.rs`, `sessions.rs`, `app/integration_test/live_test.dart` |
| 측정 | `app/engine/<크레이트>/examples/*_bench.rs` |
