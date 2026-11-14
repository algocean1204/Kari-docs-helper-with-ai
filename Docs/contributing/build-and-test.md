# 빌드와 시험

## 필요한 도구

| 도구 | 판 | 쓰는 곳 |
|---|---|---|
| Flutter | 3.38.2 (stable) | 앱 |
| Rust | 1.97.1 | 엔진(`app/engine`). 판과 구성 요소(`rustfmt`, `clippy`)는 `app/engine/rust-toolchain.toml`이 정하고, rustup이 그 판을 받아 쓴다. 앱 빌드 훅도 `app/engine`에서 `cargo`를 불러 같은 판을 쓴다. 판을 올릴 때는 이 파일의 `channel`만 바꾼다 |
| Xcode | 26 이상 | macOS 앱 빌드 |
| CocoaPods(`pod`) | 최신 | macOS 앱 빌드(`integration_test` 플러그인을 묶는다. 릴리스 앱에는 들어가지 않는다) |
| poppler | `pdftotext`, `pdffonts`, `pdfinfo` | `app/engine/pdf`의 시험이 만든 PDF를 검사한다 |
| qpdf | 11 이상 | `app/engine/pdf`의 시험이 PDF 구조를 읽는다 |
| Python | 3.12 이상 | `app/scripts/`. 표준 라이브러리만 쓴다 |

```bash
brew install poppler qpdf cocoapods
```

- 지원하는 개발 기기는 Apple Silicon 맥이다. 앱은 arm64 전용이고 최소 macOS는 13이다.
- Linux에서는 엔진 시험과 앱 시험(창 없이)이 돈다. `libssl-dev`, `pkg-config`, `libfreetype6-dev`, `libfontconfig1-dev`가 필요하다.

## 엔진

```bash
cd app/engine
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p kari-ffi --features test-hooks   # 가짜 AI 서버를 쓰는 경계 시험까지
cargo build --profile dist -p kari-ffi   # 앱에 싣는 라이브러리(target/dist/)
```

- `test-hooks`는 시험용 길(가짜 서버 주소, 메모리 키 보관)을 켠다. 앱에 싣는 빌드에는 없다.

- `dist` 프로파일은 `release`에 전체 링크 최적화(`lto = "fat"`)와 기호 지우기를 더한 것이다.
- 측정: `cargo run --release -p <크레이트> --example <이름>`. 예제는 `app/engine/<크레이트>/examples/`에 있다(`doc_bench`, `store_bench`, `layout_bench`, `edit_bench`, `ffi_bench`, `zoom_bench`, `outline_bench`). `zoom_bench`는 배율 100·200·300%에서 그리기와 명령 시간, 300%에서 큰 문서를 스크롤할 때의 메모리를 잰다. `outline_bench`는 제목이 많은 문서에서 글자 하나를 넣는 명령과 개요 읽기의 시간을 잰다.

| 변수 | 뜻 |
|---|---|
| `KARI_CRASH_RUNS` | `kari-store`의 강제 종료 시험 횟수(기본 20) |
| `KARI_SEEDS` | 무작위 시험(`kari-doc`, `kari-edit`, `kari-store`의 `tests/random.rs`)에 더 돌릴 씨앗 수 |
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `KARI_ENV`, `KARI_PROVIDER` | 실제 서버 시험의 키와 제공사(아래) |

편집 명령을 새로 만들면 `app/engine/edit/tests/random.rs`의 `command`에 그 명령을 내는 갈래를 더하고 `r.below(N)`의 N을 늘린다. 고정 씨앗 시험은 되돌리기 이름의 가짓수(`labels.len()`)를 확인하므로, 이름이 새로 생기면 그 수도 고친다.

## 앱

```bash
cd app
flutter pub get
flutter analyze
flutter test
flutter run -d macos
flutter build macos --release                            # build/macos/Build/Products/Release/Kari.app
flutter test integration_test/app_test.dart -d macos     # 실제 앱 시험(아래)
```

- `flutter run`, `build`, `test`는 빌드 훅(`app/hook/build.dart`)을 거쳐 `cargo build --profile dist -p kari-ffi --target <대상>`을 부른다. 라이브러리는 `Kari.app/Contents/Frameworks/kari.framework`에 들어간다.
- `flutter test`의 대부분은 엔진 흉내(`app/lib/engine/fake*.dart`)로 돌고, `app/test/native_test.dart`와 `edit_native_test.dart`는 진짜 라이브러리를 부른다.
- 빌드 폴더를 지울 때는 `rm -rf build .dart_tool/flutter_build`로 둘을 함께 지운다. `build`만 지우면 엔진을 싣는 단계가 건너뛰어진다.

| 변수 | 값 | 뜻 |
|---|---|---|
| `KARI_HOME` | 폴더 | 자료 폴더. 없으면 `~/Library/Application Support/dev.clonamic.kari`(샌드박스 안에서는 앱 컨테이너 안) |
| `APP_STATE` | `sample`, `first` | 엔진 대신 견본 자료로 그린다. 저장하지 않는다 |
| `APP_ROUTE` | `home`, `editor`, `interview`, `report`, `settings` | 처음 띄울 화면 |
| `APP_THEME` | `light`, `dark` | 테마. 없으면 OS 설정 |
| `SHOT_DIR` | 폴더 | `flutter test`가 화면 그림을 PNG로 쓴다 |

앱은 샌드박스 안에서 돌아 컨테이너 밖에 쓰지 못한다. 빌드한 앱에 `KARI_HOME`을 줄 때는 컨테이너 안의 폴더를 준다.

```bash
KARI_HOME="$HOME/Library/Containers/dev.clonamic.kari/Data/tmp/kari-dev" \
  build/macos/Build/Products/Release/Kari.app/Contents/MacOS/Kari
```

## 실제 앱 시험

`app/integration_test/app_test.dart`는 창을 띄워 진짜 엔진과 진짜 텍스처로 앱을 몬다. CI에서는 돌지 않는다.

- 자료 폴더는 `KARI_HOME`이 있으면 그 폴더(컨테이너 안의 경로), 없으면 새 임시 폴더이고 끝나면 지운다.
- 입력 지연 시험은 실제로 그려진 프레임을 재므로 창이 화면에 보여야 한다.
- 인쇄 시험은 인쇄 창이 문서 창에 붙어 뜨면 멈춰 기다린다. 사람이 인쇄 창의 `취소`를 눌러야 끝난다.
- 공유 시험은 `KARI_SHARE_CHECK=1`일 때만 돈다. 공유 메뉴가 `⋯` 단추 자리에 뜨면 사람이 Esc로 닫아야 끝난다.
- `KARI_TEST_BACKGROUND=1`이면 앱이 Dock에 뜨지 않고 앞으로 나오지 않으며 창은 다른 창들 뒤에 있다. 창이 보여야 하는 입력 지연, 인쇄, 공유 시험은 건너뛴다.

```bash
cd app
flutter test integration_test/app_test.dart -d macos
KARI_SHARE_CHECK=1 flutter test integration_test/app_test.dart -d macos   # 공유 시험까지
KARI_TEST_BACKGROUND=1 flutter test integration_test/app_test.dart -d macos   # 앱을 앞에 띄우지 않고
KARI_TEST_BACKGROUND=1 flutter test integration_test/canvas_test.dart -d macos   # 캔버스 문서: 사각형·글 상자(한글)·옮기기·크기·되돌리기·돌리기·맞춤·복사와 붙여넣기·PDF
```

## 시연 그림 다시 만들기

README의 대표 그림과 기능 표의 움직이는 그림(`Docs/assets/readme/`)은 `app/scripts/demo.sh`로 다시 만든다. 화면이나 기능이 바뀌면 다시 돌려 그림을 맞춘다. 창을 띄우지 않는다: 위젯 시험 `app/test/demo/demo_test.dart`가 문서와 캔버스는 진짜 엔진으로, 면접과 리포트는 흉내 엔진으로 앱을 그리고, 1/30초씩 시계를 보내며 프레임을 PNG로 남긴다. 마우스 커서와 누름 표시, 단축키 표시는 시험이 그 위에 그리고, 누르기와 글자 입력(한글은 자모 조합 과정까지)은 같은 자리에 진짜 이벤트로 보낸다. 스크립트는 프레임을 `ffmpeg`와 `img2webp`(`brew install ffmpeg webp`)로 기능마다의 `webp`와 멈춘 그림(`poster.jpg`, 클립에서 고른 한 장면)으로 묶고, 처음부터 끝까지 이은 영상을 `.cache/demo/kari-demo.mp4`(저장소 밖)에 쓴다. 보통의 `flutter test`에서는 이 시험이 건너뛰어진다(`KARI_DEMO_DIR`이 있을 때만 돈다). `--encode`를 주면 찍어 둔 프레임을 다시 묶기만 한다. 화면의 사람, 회사, 공고, 면접 질문과 리포트는 견본 자료다.

```bash
app/scripts/demo.sh            # 찍고 묶는다
app/scripts/demo.sh --encode   # 묶기만 다시
```

## 실제 서버 시험

실제 Anthropic, OpenAI 서버를 부르는 시험이다. 키가 있을 때만 돌고 API 요금이 든다. CI에서는 돌지 않는다.

| 시험 | 하는 일 | 돌리는 법 |
|---|---|---|
| `app/engine/ai/tests/live.rs` | 두 제공사의 모델 목록, 짧은 호출, 스트림과 구조화된 출력 | `cargo test -p kari-ai --test live -- --ignored --nocapture` |
| `app/engine/interview/tests/live.rs` | 견본 이력서로 세 턴 | `cargo test -p kari-interview --test live -- --ignored --nocapture` |
| `app/engine/interview/tests/quality.rs` | 채점과 질문 만들기의 품질을 평가 자료(`tests/eval/`)로 잰다 | `cargo test -p kari-interview --release --test quality -- --ignored --nocapture` |
| `app/engine/interview/tests/sessions.rs` | 세션을 끝까지, 끊고 이어 가기 | `cargo test -p kari-interview --release --test sessions -- --ignored --nocapture` |
| `app/integration_test/live_test.dart` | 제공사마다 실제 앱에서 키 넣기 → 면접 두 답 → 리포트 | `flutter test integration_test/live_test.dart -d macos`(`app`에서) |

- 엔진 시험은 `app/engine`에서 돌린다. `#[ignore]`라 `--ignored`를 줄 때만 돈다. 키는 환경 변수 `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, 없으면 `KARI_ENV`가 가리키는 `.env` 파일의 같은 이름 줄에서 읽는다. `app/.env.example`을 `app/.env`로 복사해 값을 채우고 `KARI_ENV`에 그 절대 경로를 준다(`.env`는 git이 무시한다). 키가 없는 제공사의 시험은 건너뛴다.
- `kari-interview`의 시험은 `KARI_PROVIDER`(`anthropic` 기본, `openai`)의 제공사로 돈다. `kari-ai`의 시험은 두 제공사를 각각 돈다.
- `kari-interview`의 시험은 호출마다 추정 비용을 `app/engine/target/live-ledger.jsonl`에 제공사와 함께 적고, 그 제공사의 합이 예산(`KARI_LIVE_BUDGET`, 달러)에 닿으면 더 부르지 않고 멈춘다. 기본 예산은 `tests/common/live.rs`에 있다. 모델, 강도, 횟수를 바꾸는 변수(`KARI_LIVE_*`, `KARI_QA_*`)는 각 파일의 머리에 있다.
- 앱 시험은 실행할 때의 환경 변수 `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`에서만 키를 읽고, 키가 없는 제공사의 시험은 건너뛴다. 키체인에 그 제공사의 Kari 항목이 이미 있으면(`ai.keyStored`) 덮어쓰지 않고 실패로 멈춘다. 시험은 키를 앱의 설정 화면으로 넣고, 끝날 때(실패해도) 그 항목을 지운다.

## 지식 팩 검사

```bash
python3 app/scripts/pack_check.py
```

자세한 것은 [knowledge-pack-authoring.md](knowledge-pack-authoring.md)에 있다.

## 라이선스 고지

의존성(`app/engine/Cargo.lock`, `app/pubspec.lock`)을 바꾸면 고지 파일을 다시 만든다.

```bash
(cd app && flutter pub get)
python3 app/scripts/third_party.py          # Docs/THIRD_PARTY_LICENSES.md를 다시 쓴다
python3 app/scripts/third_party.py --check  # 최신인지 본다(다르면 종료 코드 1)
```

## CI

`.github/workflows/ci.yml`이 `main`·`develop`으로의 푸시와 PR에서 돈다(PR의 제목·본문만 고친 것으로는 돌지 않는다). `macos` 밖의 작업은 Ubuntu에서 돈다. 비밀값을 쓰지 않고 권한은 읽기뿐이다. 같은 가지에 새 실행이 생기면 앞 실행은 취소한다. Cargo와 Flutter의 내려받은 것과 엔진의 빌드 산출물은 캐시해 다시 쓴다.

| 작업 | 하는 일 |
|---|---|
| `repo` | 저장소에 두면 안 되는 것이 없는지(`python3 app/scripts/public_check.py`: 최상위에 `README.md`, `LICENSE`, `.gitignore`, `.github`, `app`, `Docs` 밖의 것, `.env`와 키 파일, 글 안의 비밀 모양, 기기 경로), 공개 글의 상대 링크가 맞는지(`python3 app/scripts/check_links.py`) |
| `changes` | PR에서 대상 가지와 견줘 무거운 작업을 돌릴지 정한다. `engine`은 `app/engine/`나 지식 팩 문서가, `app`은 `app/`이, `macos`는 `app/`이나 제3자 고지가 바뀔 때 돈다. 워크플로가 바뀌거나 푸시일 때는 모두 돈다 |
| `engine` | `app/engine`에서 `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`. 지식 팩 검사와 [architecture/knowledge-pack.md](../architecture/knowledge-pack.md)의 개념 표 대조 |
| `app` | `flutter analyze`, `flutter test`(빌드 훅이 엔진을 빌드한다) |
| `macos` | macOS 러너에서 `flutter build macos --debug`(Swift, 권한 파일, 빌드 훅). 공개 저장소에서만 돈다 |
| `ci-passed` | 위 작업이 모두 성공했는지. 바뀐 경로에 닿지 않아 건너뛴 무거운 작업만 통과로 본다 |

`.github/workflows/pr-meta.yml`은 PR에서만, 제목·본문을 고칠 때도 돈다.

| 작업 | 하는 일 |
|---|---|
| `pr` | PR 제목이 `type(scope): 설명` 꼴인지, 가지 이름이 `<type>/<주제>` 꼴인지, `main`으로 가는 PR이 `develop`에서 오는지, 외부 기여자의 PR 본문에 `Fixes #번호` 같은 연결 이슈가 있는지([규칙](../../.github/CONTRIBUTING.md#커밋과-pr-제목)) |
| `version` | `main`으로 가는 PR의 판이 [판 번호 규칙](../../.github/CONTRIBUTING.md#판-번호)에 맞는지, `develop`으로 가는 PR이 판을 바꾸면 릴리스 PR인지 |
| `pr-meta` | 위 둘이 성공했는지 |

`develop`과 `main` 보호 규칙의 필수 검사는 `ci-passed`와 `pr-meta` 둘이다.

## 문제 해결

| 증상 | 해결 |
|---|---|
| 링크 단계에서 `libSystem.tbd`를 읽지 못함 | 명령 앞에 `SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"`를 붙인다. 앱의 빌드 훅은 이 값을 스스로 맞춘다 |
| 앱이 여는 데 실패하고 `unavailable`을 보임 | 엔진 라이브러리를 싣지 못한 것이다. `flutter build` 출력에서 빌드 훅(`cargo`)의 오류를 본다 |
