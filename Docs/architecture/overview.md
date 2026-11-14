# 전체 구조

Kari는 Flutter 앱(화면)과 Rust 엔진(문서 규칙, 배치, 저장, 그리기, 내보내기, AI 호출, 면접)으로 나뉜다. 엔진은 동적 라이브러리로 앱 프로세스 안에 실린다.

```mermaid
flowchart LR
  subgraph App["Flutter 앱 (app/)"]
    UI["화면<br/>lib/screens"]
    ED["편집 세션<br/>lib/editing"]
    EG["엔진 호출<br/>lib/engine"]
  end
  SW["macOS 접착<br/>macos/Runner/*.swift"]
  subgraph Engine["Rust 엔진 (app/engine/, kari.framework)"]
    FFI["kari-ffi<br/>C 경계"]
  end
  DB[("자료 폴더<br/>library.db, assets/, backups/")]
  API["AI 제공사 API<br/>Anthropic, OpenAI"]

  UI --> ED --> EG
  EG -- "kari_call(JSON)" --> FFI
  ED -- "kari_edit_render(화소 버퍼)" --> FFI
  SW -- "텍스처, 파일 대화 상자, 인쇄·공유, 카메라" --- UI
  FFI --> DB
  FFI --> API
```

## 엔진 크레이트

| 크레이트 | 폴더 | 하는 일 | 의존 |
|---|---|---|---|
| `kari-doc` | `app/engine/doc` | 문서 상태, op 검증·적용·역연산, 상태 해시, 스냅샷, 순서 키 | — |
| `kari-store` | `app/engine/store` | SQLite 라이브러리: 묶음 기록, 스냅샷, 버전, 되돌리기, 에셋, 설정, 백업 | doc |
| `kari-layout` | `app/engine/layout` | 줄 나눔, 쪽 나눔, 표와 변의 선, 문단 테두리, 표시 목록, 위치 질의 | doc |
| `kari-edit` | `app/engine/edit` | 선택과 편집 명령(스타일, 표 모양, 자동 서식 포함) → op 묶음 | doc, layout |
| `kari-render` | `app/engine/render` | 표시 목록을 Skia로 화소 버퍼에 그림, 그림 풀기, 보기 배율과 좌표 | layout |
| `kari-pdf` | `app/engine/pdf` | 표시 목록을 krilla로 PDF 바이트로 | doc, layout |
| `kari-docx` | `app/engine/docx` | 문서 모델을 Word 문서(DOCX)와 텍스트 바이트로 | doc, layout |
| `kari-ai` | `app/engine/ai` | 키 보관, 제공사(Anthropic, OpenAI) 어댑터 | — |
| `kari-interview` | `app/engine/interview` | 지식 팩, 면접 진행, 채점, 리포트, 홈 대시보드, 시선 기록의 구간 | ai |
| `kari-ffi` | `app/engine/ffi` | 앱과의 경계. 동적 라이브러리 `kari` | 위 전부 |

```mermaid
flowchart BT
  doc[kari-doc]
  store[kari-store] --> doc
  layout[kari-layout] --> doc
  edit[kari-edit] --> layout
  edit --> doc
  render[kari-render] --> layout
  pdf[kari-pdf] --> layout
  docx[kari-docx] --> layout
  ai[kari-ai]
  interview[kari-interview] --> ai
  ffi[kari-ffi] --> store & edit & render & pdf & docx & interview
```

`kari-doc`, `kari-layout`, `kari-edit`, `kari-render`, `kari-pdf`, `kari-docx`는 파일, DB, 스레드를 모른다. 저장은 `kari-store`, 잠금과 스레드는 `kari-ffi`만 다룬다.

## 앱과 엔진의 경계

### C 함수

| 함수 | 부르는 곳 | 하는 일 | 명령 잠금 |
|---|---|---|---|
| `kari_call(request)` | 엔진 아이솔레이트 | JSON 요청 하나를 처리하고 JSON 응답을 돌려준다 | 잡음 (`ai.*`, `iv.*`, `doc.exportPdf`·`doc.exportDocx`·`doc.exportTxt`는 필요한 동안만) |
| `kari_free(response)` | 엔진 아이솔레이트 | `kari_call`의 응답 문자열을 푼다 | — |
| `kari_font_add(data, len)` | 앱 | 글꼴 파일 하나를 넘긴다. 엔진은 넘긴 글꼴로만 배치하고 그린다 | 안 잡음 |
| `kari_edit_gen(doc)` | 화면 스레드 | 문서의 바뀜 번호. 그릴 것이 달라지면 오른다 | 안 잡음 |
| `kari_edit_render(doc, pixels, …)` | 화면 스레드 | 문서를 화소 버퍼(BGRA, 미리 곱한 알파)에 그린다 | 안 잡음 |
| `kari_edit_render_at(doc, pixels, …, scroll_x, scroll_y)` | 화면 스레드 | 위와 같고 가로 스크롤을 더한다(배율을 키워 쪽이 영역보다 넓을 때) | 안 잡음 |

- **명령 잠금**: 엔진 상태(연 라이브러리와 메모리의 문서)를 지키는 잠금 하나. `kari_call` 하나를 처음부터 끝까지 감싼다.
- 엔진 안의 패닉은 경계를 넘지 않고 `{"err": {"code": "panic"}}` 응답이 된다.

### 요청과 응답

```json
요청  {"m": "edit.cmd", "a": {"id": 3, "cmd": {"k": "insert", "text": "안녕"}, "now": 1700000000000}}
응답  {"ok": …}  또는  {"err": {"code": "…", "message": "…", "detail": …}}
```

| 이름 앞머리 | 처리하는 파일 | 내용 |
|---|---|---|
| `lib.*`, `docs.*`, `doc.*`, `version.*`, `settings.*` | `app/engine/ffi/src/library.rs` | 라이브러리 열기·닫기·백업, 문서 목록, 커밋, 되돌리기, 기록, 버전, 설정 |
| `edit.*` | `app/engine/ffi/src/edit.rs` | 편집 세션, 편집 명령, 입력기, 찾기, 글자 수, 개요, 그림, 복사, 보기 배율, 표 경계, 눈금선, 스타일의 자세한 값(`styles.rs`) |
| `canvas.*` | `app/engine/ffi/src/canvas.rs` | 캔버스 문서의 편집 세션(`edit.*`과 같은 모양): `canvas.open`, `canvas.close`, `canvas.state`, `canvas.view`(보기 이동·확대·맞추기), `canvas.pointer`(누름·끌기·뗌·두 번 누름·지나감·취소·오른쪽 누름), `canvas.cmd`(이름과 인자로 부르는 만들기·지우기·복제·방향키·속성·뒤집기·맞춤·붙여넣기·글 상자 편집 시작과 끝), `canvas.text`와 `canvas.ime`(글 상자 안 입력과 한글 조합), `canvas.copy`(고른 글이나 고른 노드) → [layout-and-editing.md](layout-and-editing.md#캔버스-고르기와-끌기) |
| `doc.exportPdf` | `app/engine/ffi/src/pdf.rs` | PDF 내보내기. 흐름 문서는 쪽마다, 캔버스 문서는 대지마다 쪽 하나 |
| `doc.exportDocx`, `doc.exportTxt` | `app/engine/ffi/src/docx.rs` | Word 문서·텍스트 내보내기(옮기지 못한 것의 목록을 돌려준다) |
| `ai.*` | `app/engine/ffi/src/ai.rs` | 제공사, 키, 모델 목록 → [ai-connection.md](ai-connection.md) |
| `ai.tasks`, `ai.setTask`, `ai.resetTasks` | `app/engine/ffi/src/task.rs` | 일마다의 모델 |
| `iv.*` | `app/engine/ffi/src/iv.rs` | 면접 → [interview-engine.md](interview-engine.md) |
| `iv.dashboard` | `app/engine/ffi/src/dashboard.rs` | 홈 대시보드 |

Dart 쪽 함수는 `app/lib/engine/engine.dart`(`EngineApi`)에 이름마다 하나씩 있다. 시험은 같은 인터페이스의 가짜 엔진(`app/lib/engine/fake*.dart`)으로 돈다.

### 호출의 두 줄

`kari_call`은 디스크나 네트워크를 기다릴 수 있어 화면 스레드에서 부르지 않는다. `app/lib/engine/native.dart`의 `NativeEngine`이 요청을 두 줄로 나눈다. 줄마다 전용 아이솔레이트(Dart의 격리된 실행 단위) 하나가 받은 순서대로 실행한다.

| 줄 | 아이솔레이트 | 요청 |
|---|---|---|
| 느린 줄 | `kari-slow` | `ai.*`, `doc.exportPdf`, `doc.exportDocx`, `doc.exportTxt`, 아래 기본 줄의 것을 뺀 `iv.*` |
| 기본 줄 | `kari-engine` | 나머지 전부(편집 명령 포함)와 `iv.cancel`, `iv.pack`, `iv.posting`, `iv.postings`, `iv.session`, `iv.sessions`, `iv.report`, `iv.mastery`, `iv.preview`, `iv.gaze`, `iv.dashboard` |

느린 줄의 요청은 오래 걸리는 동안 명령 잠금을 쥐고 있지 않는다. 그래서 키 확인, PDF 내보내기, 면접 질문을 기다리는 동안에도 편집이 이어진다.

```mermaid
sequenceDiagram
  participant UI as 화면 스레드
  participant M as kari-engine 줄
  participant S as kari-slow 줄
  participant E as 엔진
  UI->>M: edit.cmd
  M->>E: kari_call (명령 잠금)
  UI->>S: iv.answer
  S->>E: kari_call (모델을 기다리는 동안 잠금 없음)
  E-->>M: 응답
  E-->>S: 응답
```

## 화면 그리기

편집 화면의 종이는 Flutter 위젯이 아니라 엔진이 그린 텍스처다.

1. Swift(`app/macos/Runner/MainFlutterWindow.swift`의 `TextureBridge`)가 `CVPixelBuffer` 세 장을 만들어 돌려 쓴다: Flutter가 가져간 것, 가장 최근에 다 그린 것, 지금 그리는 것.
2. 화면 스레드가 프레임마다 `kari_edit_gen`을 읽는다. 바뀜 번호나 세로·가로 스크롤이 달라졌을 때만 그린다(`app/lib/screens/paper.dart`).
3. 비어 있는 버퍼의 메모리에 `kari_edit_render_at`이 바로 그린다. 복사가 한 번 줄어든다.
4. 커서는 엔진이 그리지 않고 앱이 텍스처 위에 올린다.

그리기는 명령 잠금을 잡지 않고, 편집 세션의 앞 사본 잠금만 짧게 잡는다 → [layout-and-editing.md](layout-and-editing.md).

캔버스 문서도 같은 C 함수와 텍스처로 그린다. `kari_edit_gen`과 `kari_edit_render`는 문서 번호로 흐름 세션을 먼저, 없으면 캔버스 세션을 찾는다. 캔버스 세션은 보기(배율과 스크롤)를 스스로 가지므로 `scroll_x`, `scroll_y`는 쓰지 않고, `scale`은 레티나 배율(논리 화소당 화소)이다. 장면 위에 고른 상자와 손잡이도 엔진이 같은 버퍼에 덧그린다. 글 상자를 편집 중이면 선택 영역, 조합 밑줄, 커서도 덧그린다.

### 캔버스 화면(앱)

캔버스 문서는 `Shell`이 연 문서의 종류를 보고 `app/lib/screens/canvas.dart`로 보낸다(흐름 문서는 `doc.dart`). 흐름 문서의 `EditSession`과 따로 `app/lib/editing/canvas_session.dart`의 `CanvasSession`이 `canvas.*`를 부른다.

- **한 줄**: 엔진 호출은 한 줄로 순서대로 보낸다. 끌기 중의 `move`, 수치 끌기의 `set`(미리 보기), 확대는 줄 끝에서 아직 나가지 않은 같은 종류와 값만 바꿔 합치고, 스크롤은 델타를 더한다. 다른 입력과는 합치지 않아 순서가 바뀌지 않는다.
- **판**(`canvas_surface.dart`): 흐름 문서의 종이와 같은 텍스처 다리(`editing/surface.dart`)로 엔진이 그린 화소를 올린다. 포인터는 화면 좌표 그대로 `canvas.pointer`(같은 자리의 짧은 두 번째 누름은 `double`)로, 트랙패드와 휠의 스크롤·핀치는 `canvas.view`로 보낸다. 고른 상자, 손잡이, 글 커서는 엔진이 같은 버퍼에 그리므로 앱은 덧그리지 않는다. 끄는 동안 ⇧나 ⌥를 누르거나 떼면 마지막 자리로 `move`를 다시 보낸다(엔진이 수정 키를 입력마다 읽는다). 오른쪽 누름은 `context`를 보낸 뒤 메뉴(복사, 붙여넣기, 복제, 지우기)를 띄운다.
- **복사와 붙여넣기**(`canvas_session.dart`): 복사는 `canvas.copy`가 준 글을 시스템 클립보드에 쓴다(글이 없어도 빈 글을 써서 앞서 있던 그림을 비운다). 붙여넣기는 클립보드의 글과, 붙여넣기 판에 그림이 있으면 채널 `kari/files`의 `pasteImage`가 준 경로를 `canvas.cmd paste`에 넘기고, 엔진이 답하면 채널이 임시 파일이라고 알린 것(`temp`)만 지운다. 복사한 사용자의 파일은 이름과 상관없이 지우지 않는다. 무엇을 붙일지는 엔진이 고른다. 그림을 받지 못해 알린 까닭은 엔진의 거절이 덮지 않는다. 복사·잘라내기·붙여넣기의 메뉴 항목은 속성 패널의 칸 같은 글 칸에 초점이 있으면 노드를 건드리지 않는다. 잘라내기는 복사가 실패하면 지우지 않는다. 글 상자를 편집하는 중에는 같은 키가 글 상자 안의 글을 다룬다.
- **키**(`editing/canvas_keys.dart`): 키 → 할 일 표가 순수 함수다. 글 상자를 편집하는 중에는 흐름 문서의 키 표(`keys.dart`) 가운데 `canvas.text`가 받는 것만 통과시킨다.
- **한글 입력**: 글 상자 편집이 시작되면(`state.text`가 생기면) 입력 채널을 붙이고 끝나면 뗀다. 입력 칸 값은 상태의 `text.ime`이고, 입력기의 변경은 흐름 문서와 같은 견주기(`editing/ime.dart`)를 거쳐 `canvas.ime`로 간다.
- **속성 패널**(`canvas_props.dart`): 맨 위에 맞춤과 간격 단추(`align`, `distribute`), 위치와 크기에 각도 칸(`set`의 `rotation`)과 뒤집기 단추(`flip`)가 있다. 값은 상태의 `selection[]`(`rotation`)과 `selection[].props`가 정한다. 입력은 `canvas.cmd set`을 한 번 보낸다. 수치 끌기는 `preview: true`로 앞 사본에만 적용하고, 놓을 때 같은 값을 `preview` 없이 보낸다.
- **메뉴 막대**: 캔버스 문서가 열려 있으면 `appMenus`가 캔버스 메뉴를 낸다(`app_menu.dart`의 `CanvasMenu`). 키는 ⌘가 든 것만 건다. 한 글자 키와 ⌥ 키(⇧H, ⌥A 등)는 글 상자에서 그 글자를 칠 때 메뉴가 가로채지 않게 판의 키 표가 받는다.
- **버전 패널**은 `VersionHost`(`editing/version_host.dart`) 인터페이스로 두 세션이 함께 쓴다. 복원은 `doc.restore`, 되돌리기는 `doc.undo`다. 둘 다 엔진이 캔버스 세션의 앞 사본을 따라가게 한다.

## 플랫폼 채널

| 채널 | 방법 | 하는 일 |
|---|---|---|
| `kari/texture` | `create`, `dispose` | 텍스처 만들기와 풀기 |
| `kari/files` | `savePath` | 저장 대화 상자(PDF, Word 문서, 텍스트. 고를 수 있는 형식은 기본 이름의 확장자를 따른다) |
| `kari/files` | `openPath` | 열기 대화 상자(PNG, JPEG) |
| `kari/files` | `pasteImage` | 붙여넣기 판의 그림을 `{path, temp}`로 준다(없으면 null). `temp`는 채널이 쓴 임시 파일인가이다. Finder에서 복사한 파일(`public.file-url`)은 파일 하나가 그림 파일일 때 그 경로이고 아니면 null이다(판에 함께 실리는 아이콘은 쓰지 않는다). 그 밖에는 앱 임시 폴더에 `kari-paste-`로 시작하는 파일로 쓴다: PNG와 JPEG는 바이트 그대로, TIFF는 PNG로 바꿔 쓴다. 20MB와 화소 2,500만을 넘거나 바꾸지 못하면 오류로 답한다. 캔버스 문서의 붙여넣기가 부르고, 엔진에 넘긴 뒤 `temp`인 파일만 지운다 |
| `kari/files` | `openUrl` | 기본 브라우저로 주소 열기(`http`, `https`, `mailto`, `tel`만) |
| `kari/files` | `print {path, name}` | 앱이 만든 PDF를 인쇄 창(문서 창에 붙는 시트)에 넘긴다. 실제 크기, 자동 회전 없음, 용지는 첫 쪽의 크기와 방향. 창이 닫히면 답한다(`app/macos/Runner/Printing.swift`) |
| `kari/files` | `share {path, name, x, y, w, h}` | 앱이 만든 PDF를 공유 메뉴에 넘긴다. 메뉴는 주어진 사각형(`⋯` 단추) 자리에 뜬다. 고른 서비스가 끝나거나 고르지 않고 닫히면 답한다 |
| `kari/gaze` | `status`, `start`, `stop`, `openSettings`, `measure` | 시선 기록의 카메라: 권한 상태, 카메라 열기(처음이면 권한을 묻는다), 닫기, 시스템 설정의 카메라 권한 화면 열기, 얼굴 찾기 한 장의 처리 시간 재기(카메라를 켜지 않는다)(`app/macos/Runner/GazeMonitor.swift`) |
| `kari/gaze` | `sample {face, yaw, pitch}` | Swift → Dart. 한 장의 얼굴 유무와 각도 → [interview-engine.md](interview-engine.md#시선-기록) |

인쇄와 공유에 넘길 PDF는 `app/lib/screens/print_share.dart`의 `handOff`가 앱 임시 폴더의 `kari-share/` 아래에 `doc.exportPdf`로 만들고, 채널이 답하면 지운다.

## 권한

`app/macos/Runner/Release.entitlements`: `com.apple.security.app-sandbox`, `network.client`, `files.user-selected.read-write`, `device.camera`, `print`. 카메라를 묻는 글은 `Info.plist`의 `NSCameraUsageDescription`이다. 디버그 빌드(`DebugProfile.entitlements`)는 Flutter 개발 도구를 위해 `cs.allow-jit`, `network.server`를 더 가진다. 권한마다의 쓰임은 [../guide/privacy.md](../guide/privacy.md#앱의-권한)에 있다.

## 엔진을 앱에 싣기

`app/hook/build.dart`는 Dart 빌드 훅이다. `flutter run`, `flutter build`, `flutter test`가 모두 이 훅을 거친다.

1. `cargo build --profile dist -p kari-ffi --target aarch64-apple-darwin`을 부른다. `dist` 프로파일은 `release`에 전체 링크 최적화와 기호 지우기를 더한 것이다(`app/engine/Cargo.toml`).
2. 만든 `libkari.dylib`을 코드 에셋으로 넘긴다. Flutter가 이것을 `Kari.app/Contents/Frameworks/kari.framework`로 싣는다.
3. 엔진의 `Cargo.toml`, `Cargo.lock`, 각 크레이트가 바뀌면 훅이 다시 돈다.

앱은 arm64 전용이고 최소 macOS는 13이다.

## 자료 폴더

| 경로 | 내용 |
|---|---|
| `library.db` | SQLite 라이브러리 DB 하나. 문서, 기록, 스냅샷, 버전, 에셋 정보, 설정, 면접 기록 |
| `assets/<해시 앞 2자>/<해시>` | 그림 파일. 이름은 내용의 SHA-256 |
| `backups/library-<ms>.db` | 백업. 라이브러리를 열 때, 편집이 쉬거나 문서를 닫을 때 마지막 백업에서 하루가 지났으면 만든다. 최근 10개를 남긴다 |

- 위치: `~/Library/Application Support/dev.clonamic.kari`. 앱은 샌드박스 안에서 돌아 실제로는 `~/Library/Containers/dev.clonamic.kari/` 아래에 놓인다.
- 환경 변수 `KARI_HOME`을 주면 그 폴더를 쓴다.
- 표와 규칙 → [document-model.md](document-model.md).

## 더 읽을 것

- [document-model.md](document-model.md): 노드, op, 되돌리기, 저장, 버전
- [layout-and-editing.md](layout-and-editing.md): 배치, 편집 명령, 한글 입력, 그리기, PDF
- [interview-engine.md](interview-engine.md), [knowledge-pack.md](knowledge-pack.md): 면접
- [ai-connection.md](ai-connection.md): 키, 제공사, 모델 호출
- [../contributing/code-map.md](../contributing/code-map.md): 파일 지도
