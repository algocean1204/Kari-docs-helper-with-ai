//! 시험용 PDF 도우미: 앱이 `--kari-pdf-helper`로 다시 띄운 자기 실행 파일과 같은 몸통(`kari_pdf_helper`)을 부른다.

fn main() {
    std::process::exit(kari::kari_pdf_helper());
}
