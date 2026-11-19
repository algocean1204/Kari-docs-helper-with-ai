//! 글자 인식 견본: Pretendard로 쓴 캔버스(제목 18pt 굵게, 본문 11pt, 지어낸 사람과 회사)를 PDF로 내보내고, 엔진이
//! 읽은 줄 상자를 정답으로 적는다. `app/macos/PdfPageCheck`가 앱과 같은 길(`PdfPage.paint`)로 300dpi 그림을 그려
//! 글자를 읽고 이 정답과 견준다. 글을 보이지 않게 바꾼 PDF(`stripped.pdf`)도 함께 쓴다.
//!
//!   cargo run -p kari-import --example canvas_sample -- <폴더>
//!   → text.pdf, stripped.pdf, truth.json([{쪽, 글, x, y, w, h}], pt, 왼쪽 위 기준)

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_import::pdfcanvas;
use kari_layout::{Fonts, Layouter};

const BODY: [&str; 12] = [
    "견본상사 브랜드 디자인 팀에서 일했습니다.",
    "Designed the editorial system for 24 issues.",
    "2024년 3월부터 2026년 2월까지 리뉴얼을 이끌었습니다.",
    "예시페이 결제 화면의 아이콘 120개를 다시 그렸습니다.",
    "Typography, layout grids and motion guidelines.",
    "사용자 조사 결과를 바탕으로 정보 구조를 바꿨습니다.",
    "전시 포스터와 리플릿을 한 벌로 맞췄습니다.",
    "Worked with three engineers on the design tokens.",
    "인쇄 감리와 색 교정을 맡아 불량률을 줄였습니다.",
    "포트폴리오에 담은 작업은 모두 공개된 것입니다.",
    "Contact details are available on request.",
    "읽기 쉬운 글과 단단한 구조를 좋아합니다.",
];

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("폴더"));
    std::fs::create_dir_all(&dir).unwrap();
    let pages = 20;
    let mut d = Document::new(Kind::Canvas, "견본").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    let mut n = 0;
    for p in 0..pages {
        let pid = format!("p{p}");
        ops.push(json!({"op": "add", "node": {"id": pid, "type": "page", "parent": null, "props": {
            "w": 595, "h": 842, "cy": p as f64 * 900.0, "preset": "A4", "orientation": "portrait"}}}));
        let mut text = |y: f64, size: f64, weight: u32, t: String| {
            n += 1;
            ops.push(json!({"op": "add", "node": {"id": format!("t{n}"), "type": "text", "parent": pid, "props": {
                "x": 56, "y": y, "runs": [{"t": t}], "autoResize": "WIDTH_AND_HEIGHT",
                "style": {"font": "Pretendard", "weight": weight, "size": size}}}}));
        };
        text(56.0, 18.0, 700, format!("포트폴리오 {}쪽 · 홍길동", p + 1));
        let mut y = 100.0;
        for i in 0..36 {
            let (size, weight, t) = if i % 9 == 0 {
                (18.0, 700, format!("작업 {}", i / 9 + 1))
            } else {
                (11.0, 400, BODY[(i + p) % BODY.len()].to_string())
            };
            text(y, size, weight, t);
            y += 1.6 * size;
        }
    }
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    let mut fonts = Fonts::new();
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        fonts.add(std::fs::read(path).unwrap()).unwrap();
    }
    let mut l = Layouter::new(fonts);
    let s = l.scene(&d, &|_| None);
    let meta = kari_pdf::Meta {
        title: "견본",
        lang: "ko",
    };
    let pdf = kari_pdf::export_canvas(&d, &s, l.fonts(), &meta, &|_| None).unwrap();
    std::fs::write(dir.join("text.pdf"), &pdf).unwrap();
    std::fs::write(
        dir.join("stripped.pdf"),
        pdfcanvas::strip(&pdf, 0).expect("글을 뺀 PDF"),
    )
    .unwrap();
    let got = pdfcanvas::layout(&pdf).unwrap();
    let truth: Vec<Value> = got
        .pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.boxes.iter().map(move |b| {
                json!({"page": i + 1, "t": b.text(), "x": b.x, "y": b.y, "w": b.w, "h": 1.2 * b.size, "size": b.size})
            })
        })
        .collect();
    std::fs::write(
        dir.join("truth.json"),
        kari_doc::serde_json::to_vec(&json!({"w": 595, "h": 842, "lines": truth})).unwrap(),
    )
    .unwrap();
    println!("{}쪽, 줄 {}개", got.pages.len(), truth.len());
}
