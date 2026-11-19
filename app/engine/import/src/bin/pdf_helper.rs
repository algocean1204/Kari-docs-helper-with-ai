//! 시험용 PDF 도우미: 앱의 실행 파일이 `--kari-pdf-helper`로 하는 일과 같다(힙 상한을 세는 할당자까지).
//! `KARI_TEST_HELPER_HANG`이 있으면 입력을 읽지 않고 멈춰 있다(시간 상한과 멈추기 시험).

#[global_allocator]
static A: kari_import::pdf::Capped = kari_import::pdf::Capped;

fn main() {
    if std::env::var_os("KARI_TEST_HELPER_HANG").is_some() {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
    // 쪽 그림은 시험용 화가가 그린다(앱에서는 macOS의 PDF 그리기와 글자 인식).
    std::process::exit(kari_import::pdf::helper_main_with(Some(
        kari_import::pdf::fake_paint,
    )));
}
