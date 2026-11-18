//! 시험용 PDF 도우미: 앱의 실행 파일이 `--kari-pdf-helper`로 하는 일과 같다.
//! `KARI_TEST_HELPER_HANG`이 있으면 입력을 읽지 않고 멈춰 있다(시간 상한과 멈추기 시험).

fn main() {
    if std::env::var_os("KARI_TEST_HELPER_HANG").is_some() {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
    std::process::exit(kari_import::pdf::helper_main());
}
