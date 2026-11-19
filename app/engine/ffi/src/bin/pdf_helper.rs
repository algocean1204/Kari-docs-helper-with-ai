//! 시험용 PDF 도우미: 앱이 `--kari-pdf-helper`로 다시 띄운 자기 실행 파일과 같은 몸통(`kari_pdf_helper`)을 부른다.

// 앱 빌드(`app` 기능)에서는 라이브러리가 같은 할당자를 둔다.
#[cfg(not(feature = "app"))]
#[global_allocator]
static A: kari_import::pdf::Capped = kari_import::pdf::Capped;

fn main() {
    std::process::exit(kari::kari_pdf_helper());
}
