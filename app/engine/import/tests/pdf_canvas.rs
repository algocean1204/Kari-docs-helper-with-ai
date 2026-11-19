//! PDF → 캔버스 시험. 견본은 Kari의 캔버스를 PDF로 내보낸 것(글 상자의 자리·크기·굵기를 아는 왕복 정답)과 손으로 쓴
//! PDF 문법(돌린 쪽, 잘라 보이는 쪽, 그림만 있는 쪽, 보이지 않는 글)이다. 쪽 그림은 시험용 도우미의 견본 화가가 그린다.

use std::time::{Duration, Instant};

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_import::pdf;
use kari_import::pdfcanvas::{self as pc, Mode, Ocr, Page};
use kari_layout::{Fonts, Layouter};

fn fonts() -> Fonts {
    let mut f = Fonts::new();
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        f.add(std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
            .unwrap();
    }
    f
}

const META: kari_pdf::Meta = kari_pdf::Meta {
    title: "견본",
    lang: "ko",
};

fn canvas_pdf(d: &Document) -> Vec<u8> {
    let mut l = Layouter::new(fonts());
    let s = l.scene(d, &|_| None);
    kari_pdf::export_canvas(d, &s, l.fonts(), &META, &|_| None).unwrap()
}

fn layout(bytes: &[u8]) -> Result<pc::Layout, &'static str> {
    pdf::set_helper(env!("CARGO_BIN_EXE_kari-import-pdf-helper"));
    pdf::canvas_layout(bytes)
}

/// (x, y, 크기, 굵게, 글).
type Want = (f64, f64, f64, bool, &'static str);

/// A4 캔버스 한 장에 글 상자들을 놓는다.
fn sheet(items: &[Want]) -> Document {
    let mut d = Document::new(Kind::Canvas, "견본").unwrap();
    let mut ops = vec![json!({"op": "set", "id": "n1", "props": {"w": 595, "h": 842}})];
    for (i, (x, y, size, bold, t)) in items.iter().enumerate() {
        ops.push(json!({"op": "add", "node": {"id": format!("t{i}"), "type": "text", "parent": "n1", "props": {
            "x": x, "y": y, "runs": [{"t": t}], "autoResize": "WIDTH_AND_HEIGHT",
            "style": {"font": "Pretendard", "weight": if *bold { 700 } else { 400 }, "size": size}}}}));
    }
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    d
}

const ITEMS: [Want; 8] = [
    (56.0, 48.0, 18.0, true, "포트폴리오 견본"),
    (
        56.0,
        90.0,
        11.0,
        false,
        "견본상사 디자인 팀에서 일했습니다.",
    ),
    (
        56.0,
        110.5,
        11.0,
        false,
        "Brand system and editorial layout",
    ),
    (320.0, 160.0, 9.0, false, "작은 글 9pt"),
    (40.0, 400.0, 24.0, true, "큰 제목 24"),
    (300.0, 700.0, 14.0, false, "오른쪽 아래 14pt"),
    (100.0, 520.25, 10.5, true, "굵은 본문 10.5"),
    (420.0, 800.0, 12.0, false, "쪽 끝 2026"),
];

#[test]
fn 내보낸_캔버스를_다시_읽으면_글_상자의_자리는_2pt_크기는_0_5pt_안이고_굵기가_남는다() {
    let pdf = canvas_pdf(&sheet(&ITEMS));
    let l = layout(&pdf).unwrap();
    assert_eq!(l.pages.len(), 1);
    let p = &l.pages[0];
    assert_eq!((p.w, p.h, p.mode), (595.0, 842.0, Mode::Text));
    assert_eq!(p.boxes.len(), ITEMS.len(), "{:#?}", p.boxes);
    let (mut worst_pos, mut worst_size) = (0.0f64, 0.0f64);
    for (x, y, size, bold, t) in ITEMS {
        let b = p
            .boxes
            .iter()
            .find(|b| b.text() == t)
            .unwrap_or_else(|| panic!("{t}: {:#?}", p.boxes));
        worst_pos = worst_pos.max((b.x - x).abs()).max((b.y - y).abs());
        worst_size = worst_size.max((b.size - size).abs());
        assert!(b.runs.iter().all(|r| r.bold == bold), "{t}: {b:?}");
    }
    eprintln!("자리 오차 최대 {worst_pos:.3}pt, 크기 오차 최대 {worst_size:.3}pt");
    assert!(worst_pos <= 2.0, "{worst_pos}");
    assert!(worst_size <= 0.5, "{worst_size}");
    assert!(l.lost.is_empty(), "{:?}", l.lost);
}

#[test]
fn 한_줄_안의_굵은_낱말과_작은_글자는_조각으로_남는다() {
    let mut d = Document::new(Kind::Canvas, "견본").unwrap();
    d.apply(
        &json!([{"op": "set", "id": "n1", "props": {"w": 595, "h": 842}},
            {"op": "add", "node": {"id": "t", "type": "text", "parent": "n1", "props": {
            "x": 50, "y": 50, "autoResize": "WIDTH_AND_HEIGHT",
            "runs": [{"t": "기간 "}, {"t": "2024–2026", "s": {"weight": 700}}, {"t": " 주석", "s": {"size": 8}}],
            "style": {"font": "Pretendard", "weight": 400, "size": 12}}}}]),
        Guard::default(),
    )
    .unwrap();
    let l = layout(&canvas_pdf(&d)).unwrap();
    let b = &l.pages[0].boxes;
    assert_eq!(b.len(), 1, "{b:#?}");
    assert_eq!(b[0].text(), "기간 2024–2026 주석");
    assert!((b[0].size - 12.0).abs() <= 0.5);
    let bold: String = b[0]
        .runs
        .iter()
        .filter(|r| r.bold)
        .map(|r| r.t.as_str())
        .collect();
    assert_eq!(bold.trim(), "2024–2026");
    let small = b[0].runs.iter().find(|r| r.t.contains("주석")).unwrap();
    assert!((small.size - 8.0).abs() <= 0.5, "{small:?}");
}

/// 객체들(1번부터)로 PDF를 만든다. 1번이 카탈로그, 2번이 쪽 나무다.
fn build(objs: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        at.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for a in at {
        out.extend(format!("{a:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    out
}

fn stream(s: &str) -> String {
    format!("<< /Length {} >>\nstream\n{s}\nendstream", s.len())
}

/// 쪽들(쪽 사전의 덧붙일 항목, 내용). 글꼴 F1은 Helvetica(한 바이트 표준 인코딩).
fn pages(ps: &[(&str, &str)]) -> Vec<u8> {
    let n = ps.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    for (i, (extra, content)) in ps.iter().enumerate() {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] {extra} /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + 2 * i
        ));
        objs.push(stream(content));
    }
    build(&objs)
}

/// 1×1 회색 인라인 그림을 쪽 전체에 늘린 스캔 쪽의 내용.
const SCAN: &str = "q 600 0 0 800 0 0 cm BI /W 1 /H 1 /BPC 8 /CS /G ID A EI Q";

#[test]
fn 돌린_쪽과_잘라_보이는_쪽은_보이는_모양대로_크기와_자리가_정해진다() {
    let text = "BT /F1 20 Tf 100 700 Td (Turn) Tj ET";
    let pdf = pages(&[
        ("", text),
        ("/Rotate 90", text),
        ("/CropBox [50 100 450 750]", text),
        ("/Rotate 270 /CropBox [50 100 450 750]", text),
        // 가로로 놓인 쪽(내용을 반대로 돌려 그려 돌린 뒤 바로 보인다).
        (
            "/Rotate 90",
            "q 0 1 -1 0 600 0 cm BT /F1 20 Tf 100 300 Td (Upright) Tj ET Q",
        ),
    ]);
    let l = layout(&pdf).unwrap();
    let p: Vec<&Page> = l.pages.iter().collect();
    assert_eq!((p[0].w, p[0].h), (600.0, 800.0));
    assert_eq!((p[1].w, p[1].h), (800.0, 600.0));
    assert_eq!((p[2].w, p[2].h), (400.0, 650.0));
    assert_eq!((p[3].w, p[3].h), (650.0, 400.0));
    let base = |b: &pc::TextBox| b.y + pc::BASELINE * b.size;
    // 돌리지 않은 쪽: 기준선은 위에서 800-700=100.
    let b0 = &p[0].boxes[0];
    assert!(
        (b0.x - 100.0).abs() < 0.01 && (base(b0) - 100.0).abs() < 0.01,
        "{b0:?}"
    );
    // 90° 돌림(시계 방향으로 보임): 원래 (x, y) → 보이는 (y, x). 글의 시작점 (100, 700)은 (700, 100).
    let b1 = &p[1].boxes[0];
    assert!(
        (b1.x - 700.0).abs() < 25.0 && (b1.y - 100.0).abs() < 25.0,
        "{b1:?}"
    );
    // 잘라 보이는 쪽: CropBox 왼쪽 위가 0.
    let b2 = &p[2].boxes[0];
    assert!(
        (b2.x - 50.0).abs() < 0.01 && (base(b2) - 50.0).abs() < 0.01,
        "{b2:?}"
    );
    let b4 = &p[4].boxes;
    assert_eq!(b4.len(), 1, "{b4:?}");
    assert_eq!(b4[0].text(), "Upright");
    assert!((b4[0].size - 20.0).abs() < 0.01);
    assert!(p.iter().all(|p| p.mode == Mode::Text));
}

#[test]
fn 스캔_쪽은_글자_인식으로_보이지_않는_글이_있는_쪽은_그대로_둔다() {
    let pdf = pages(&[
        ("", SCAN),
        // 인식 글을 얹은 스캔 PDF: 그림 위에 보이지 않는 글(Tr 3).
        (
            "",
            &format!("{SCAN} BT 3 Tr /F1 12 Tf 72 700 Td (Recognized) Tj ET"),
        ),
        ("", "BT /F1 12 Tf 72 700 Td (Plain) Tj ET"),
        // 아무것도 없는 쪽도 페이지가 된다(글자 인식은 읽은 글 없음으로 끝난다).
        ("", ""),
    ]);
    let l = layout(&pdf).unwrap();
    let modes: Vec<Mode> = l.pages.iter().map(|p| p.mode).collect();
    assert_eq!(modes, [Mode::Ocr, Mode::Kept, Mode::Text, Mode::Ocr]);
    assert_eq!(l.pages[1].boxes[0].text(), "Recognized");
}

#[test]
fn 글_쪽의_바탕은_글을_보이지_않게_바꾼_pdf로_그린다() {
    let pdf = canvas_pdf(&sheet(&ITEMS));
    let stripped = pc::strip(&pdf, 0).expect("글을 뺀 PDF");
    // 다시 읽으면 글은 그대로 있지만 모두 보이지 않게 그려져(그대로 쪽) 바탕에 두 번 보이지 않는다.
    let l = pc::layout(&stripped).unwrap();
    assert_eq!(l.pages[0].mode, Mode::Kept);
    assert_eq!(l.pages[0].boxes.len(), ITEMS.len());
    // 깨진 바이트는 바꾸지 않는다(바탕을 그대로 그린다).
    assert!(pc::strip(b"%PDF-1.7 broken", 0).is_none());
    assert!(pc::strip(&pdf, 5).is_none());

    pdf::set_helper(env!("CARGO_BIN_EXE_kari-import-pdf-helper"));
    let s = pdf::canvas_shot(&pdf, 0, true, false, pc::SHARP, 1 << 20).unwrap();
    assert!(
        s.stripped && s.ocr == Ocr::Off && s.lines.is_empty(),
        "{s:?}"
    );
    let s = pdf::canvas_shot(&pdf, 0, false, true, pc::OCR_SCALE, 1 << 20).unwrap();
    assert!(
        !s.stripped && s.ocr == Ocr::Done && s.lines.len() == 1,
        "{s:?}"
    );
    assert_eq!(pdf::ocr_korean(), Some(true));
}

#[test]
fn 쪽_크기의_상한과_하한_쪽마다_다른_크기() {
    let pdf = pages(&[
        ("/MediaBox [0 0 14400 14400]", SCAN),
        ("/MediaBox [0 0 1 1]", SCAN),
        ("/MediaBox [0 0 300 200]", SCAN),
        ("/MediaBox [0 0 200 300]", SCAN),
    ]);
    let l = layout(&pdf).unwrap();
    let sizes: Vec<(f64, f64)> = l.pages.iter().map(|p| (p.w, p.h)).collect();
    assert_eq!(
        sizes,
        [
            (14400.0, 14400.0),
            (1.0, 1.0),
            (300.0, 200.0),
            (200.0, 300.0)
        ]
    );
    // 큰 쪽은 화소 상한(4,000) 안의 배율로 그린다(144dpi보다 낮다).
    let s = pc::scale_for(14400.0, 14400.0, pc::SHARP);
    assert!(s < pc::SHARP && (14400.0 * s - pc::MAX_PX).abs() < 1e-6);
}

#[test]
fn 이상한_pdf는_정해진_문장으로_거절한다() {
    for (bad, want) in [
        (b"%PDF-1.7\n broken".to_vec(), pdf::BROKEN),
        (Vec::new(), pdf::BROKEN),
        (vec![b'%'; 10], pdf::BROKEN),
    ] {
        assert_eq!(layout(&bad).unwrap_err(), want);
    }
    // 쪽이 301개면 쪽 상한.
    let many: Vec<(&str, &str)> = vec![("", ""); 301];
    assert_eq!(layout(&pages(&many)).unwrap_err(), pdf::TOO_MANY_PAGES);
}

/// 씨앗을 고정한 xorshift.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[test]
fn 무작위로_바꾼_pdf_64개도_2초_안에_끝나고_쪽_그림까지_멈추지_않는다() {
    let base = canvas_pdf(&sheet(&ITEMS));
    let mut ok = 0;
    for seed in 0..64u64 {
        let mut g = Rng(0x4341_4E56 ^ (seed * 0x9E37_79B9 + 1));
        let mut b = base.clone();
        for _ in 0..1 + g.below(3) {
            match g.below(3) {
                0 => b.truncate(g.below(b.len())),
                1 => {
                    for _ in 0..1 + g.below(8) {
                        let at = g.below(b.len());
                        b[at] ^= 1 << g.below(8);
                    }
                }
                _ => b.extend((0..1 + g.below(64)).map(|_| g.next() as u8)),
            }
        }
        let t = Instant::now();
        if let Ok(l) = layout(&b) {
            ok += 1;
            let _ = pdf::canvas_shot(&b, 0, l.pages[0].mode == Mode::Text, false, 1.0, 1 << 20);
        }
        let took = t.elapsed();
        assert!(
            cfg!(debug_assertions) || took < Duration::from_secs(2),
            "씨앗 {seed}: {took:?}"
        );
    }
    eprintln!("받은 견본 {ok}개 / 64");
}
