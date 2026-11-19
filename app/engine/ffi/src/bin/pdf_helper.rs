//! 시험용 PDF 도우미: 앱이 `--kari-pdf-helper`로 다시 띄운 자기 실행 파일과 같은 몸통(`kari_pdf_helper`)을 부른다.
//! `KARI_TEST_PAGE_HANG`이 쪽 번호(0부터)면 그 쪽의 그림 일(`page`)에서 입력을 읽지 않고 멈춰 있다(멈추기 시험).

// 앱 빌드(`app` 기능)에서는 라이브러리가 같은 할당자를 둔다.
#[cfg(not(feature = "app"))]
#[global_allocator]
static A: kari_import::pdf::Capped = kari_import::pdf::Capped;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Ok(hang) = std::env::var("KARI_TEST_PAGE_HANG") {
        if args.windows(2).any(|w| w[0] == "page" && w[1] == hang) {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        }
    }
    // 쪽 그림은 시험용 화가가 그린다(앱에서는 macOS의 PDF 그리기와 글자 인식).
    std::process::exit(kari::kari_pdf_helper_with(Some(
        kari_import::pdf::fake_paint,
    )));
}
