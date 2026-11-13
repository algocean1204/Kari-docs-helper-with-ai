//! 정해진 문서를 버퍼에 그려 `app/engine/target/kari-shots/`에 PNG로 쓰고(눈으로 확인한다) 화소를 검사한다.
//! 종이 안은 흰색, 글자 자리에 어두운 화소, 선택 영역의 색, 종이 밖은 바탕색, 스크롤하면 내용이 그만큼 옮겨짐.

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Item, Layouter, Overlay, Pos, Rect};
use kari_render::{draw, Colors, Faces, Frame, Geo, Scene};
use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo, Pixmap};

const SCALE: f32 = 2.0;
/// 밝은 테마의 바탕, 선택(accentSoft), 조합 밑줄(accent).
const COLORS: Colors = Colors {
    bg: [0xF5, 0xF5, 0xF5, 0xFF],
    selection: [0xE5, 0xF4, 0xFF, 0xFF],
    preedit: [0x0A, 0x6D, 0xC2, 0xFF],
};
const LINES: [&str; 6] = [
    "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.",
    "Redis 캐시를 도입하면서 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.",
    "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.",
    "장애 발생 시 서킷 브레이커와 재시도 정책이 어떻게 동작하는지 문서로 정리했습니다.",
    "레거시 코드의 테스트 커버리지를 31%에서 74%로 끌어올렸습니다.",
    "동시성 문제를 재현하기 위해 낙관적 락과 비관적 락을 각각 적용해 비교했습니다.",
];

fn font(name: &str) -> Vec<u8> {
    let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// 그림이 없는 문서용 기억(읽기는 늘 실패).
fn none() -> kari_render::Images {
    kari_render::Images::new(Box::new(|_| None), kari_render::IMAGE_BUDGET)
}

struct Kit {
    lay: Layouter,
    faces: Faces,
}

fn kit() -> Kit {
    let files = [font("Pretendard-Regular.ttf"), font("Pretendard-Bold.ttf")];
    let mut fonts = Fonts::new();
    for f in &files {
        fonts.add(f.clone()).unwrap();
    }
    Kit {
        lay: Layouter::new(fonts),
        faces: Faces::new(files.iter().map(Vec::as_slice)),
    }
}

/// 문단 `p0`, `p1`, …. `root`는 루트 속성(스타일, 쪽).
fn doc(root: Value, paras: &[Value]) -> Document {
    let mut d = Document::new(Kind::Flow, "그림").unwrap();
    let mut ops = vec![
        json!({"op": "del", "id": "n1"}),
        json!({"op": "set", "id": "root", "props": root}),
    ];
    for (i, p) in paras.iter().enumerate() {
        ops.push(json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root", "props": p}}));
    }
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    d
}

fn plain(texts: &[&str]) -> Vec<Value> {
    texts.iter().map(|t| json!({"runs": [{"t": t}]})).collect()
}

/// 그린 화소. BGRA, 줄 간격은 너비 × 4.
struct Shot {
    px: Vec<u8>,
    w: i32,
    h: i32,
    geo: Geo,
}

impl Shot {
    /// 논리 화소(보이는 영역 기준)의 화소 [r, g, b, a].
    fn at(&self, x: f32, y: f32) -> [u8; 4] {
        let (x, y) = ((x * SCALE) as usize, (y * SCALE) as usize);
        let i = y * self.w as usize * 4 + x * 4;
        [self.px[i + 2], self.px[i + 1], self.px[i], self.px[i + 3]]
    }

    /// 논리 화소 사각형 안에서 조건을 만족하는 화소의 비율.
    fn share(&self, x: f32, y: f32, w: f32, h: f32, f: impl Fn([u8; 4]) -> bool) -> f32 {
        let (mut hit, mut all) = (0, 0);
        let mut yy = y;
        while yy < y + h {
            let mut xx = x;
            while xx < x + w {
                all += 1;
                hit += f(self.at(xx, yy)) as u32;
                xx += 0.5;
            }
            yy += 0.5;
        }
        hit as f32 / all.max(1) as f32
    }

    fn save(&mut self, name: &str) {
        let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let info = ImageInfo::new(
            (self.w, self.h),
            ColorType::BGRA8888,
            AlphaType::Premul,
            None,
        );
        let stride = self.w as usize * 4;
        let png = Pixmap::new(&info, &mut self.px, stride)
            .and_then(|p| p.encode(EncodedImageFormat::PNG, None))
            .expect("PNG 인코딩");
        std::fs::write(format!("{dir}/{name}.png"), png).unwrap();
    }
}

/// 보이는 영역 `view`(논리 화소)에 그린다.
fn shoot(k: &Kit, view: (f32, f32), scroll: f32, sel: &[Rect], pre: &[Rect]) -> Shot {
    shoot_with(k, view, scroll, sel, pre, &mut none(), 1.0)
}

fn shoot_with(
    k: &Kit,
    view: (f32, f32),
    scroll: f32,
    sel: &[Rect],
    pre: &[Rect],
    images: &mut kari_render::Images,
    zoom: f32,
) -> Shot {
    let out = k.lay.output();
    let geo = Geo::new(view.0, out.page_size, out.pages.len(), zoom);
    let (w, h) = ((view.0 * SCALE) as i32, (view.1 * SCALE) as i32);
    let mut px = vec![0u8; w as usize * h as usize * 4];
    let frame = Frame {
        pixels: &mut px,
        width: w,
        height: h,
        stride: w as usize * 4,
        scale: SCALE,
        scroll_x: 0.0,
        scroll_y: scroll,
    };
    let scene = Scene {
        layout: out,
        geo,
        colors: COLORS,
        selection: sel,
        preedit: pre,
        guides: false,
    };
    draw(frame, &k.faces, &scene, images).unwrap();
    Shot { px, w, h, geo }
}

fn dark(p: [u8; 4]) -> bool {
    (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300
}

fn white(p: [u8; 4]) -> bool {
    p == [255, 255, 255, 255]
}

/// 첫 글리프 묶음의 (쪽, 글자 상자 x, 위, 너비, 높이) pt.
fn first_glyphs(k: &Kit) -> (f32, f32, f32, f32) {
    let page = &k.lay.output().pages[0];
    let Some(Item::Glyphs { size, glyphs, .. }) = page
        .items
        .iter()
        .find(|i| matches!(i, Item::Glyphs { glyphs, .. } if glyphs.len() > 4))
    else {
        panic!("글리프가 없다");
    };
    let (x0, y) = (glyphs[0].x, glyphs[0].y);
    let x1 = glyphs[4].x;
    (x0, y - size * 0.8, x1 - x0, size * 0.8)
}

/// 기본 검사: 종이 밖은 바탕색, 종이 여백은 흰색, 첫 글자 자리에 어두운 화소.
fn basic(k: &Kit, s: &Shot, scroll: f32) {
    assert_eq!(s.at(2.0, 2.0), [0xF5, 0xF5, 0xF5, 0xFF], "종이 밖은 바탕색");
    let (px, py) = s.geo.to_px(0, 8.0, 8.0);
    assert!(white(s.at(px, py - scroll)), "종이 여백은 흰색");
    let (x, y, w, h) = first_glyphs(k);
    let (gx, gy) = s.geo.to_px(0, x, y);
    let ink = s.share(gx, gy - scroll, w * s.geo.k, h * s.geo.k, dark);
    assert!(ink > 0.05, "글자 자리에 어두운 화소: {ink}");
}

#[test]
fn korean_paragraphs() {
    let mut k = kit();
    let d = doc(json!({}), &plain(&LINES));
    k.lay.layout(&d, None);
    let mut s = shoot(&k, (1000.0, 700.0), 0.0, &[], &[]);
    basic(&k, &s, 0.0);
    s.save("01-paragraphs");
}

#[test]
fn heading_bold_underline() {
    let mut k = kit();
    let styles = json!({"styles": {
        "title": {"cs": {"size": 22, "weight": 700}, "pf": {"space": {"after": 10}}},
        "h1": {"cs": {"size": 14, "weight": 700}, "pf": {"space": {"before": 12, "after": 4}}},
    }});
    let paras = vec![
        json!({"style": "title", "runs": [{"t": "김카리"}]}),
        json!({"style": "h1", "runs": [{"t": "경력"}]}),
        json!({"runs": [
            {"t": "예시페이 "},
            {"t": "백엔드 엔지니어", "s": {"weight": 700}},
            {"t": " · 2023.01 – 2024.05 · "},
            {"t": "결제 API 설계", "s": {"underline": true}},
            {"t": " · "},
            {"t": "기울임 글", "s": {"italic": true}},
            {"t": " · "},
            {"t": "취소선", "s": {"strike": true}},
        ]}),
        json!({"runs": [{"t": LINES[2]}]}),
    ];
    let d = doc(styles, &paras);
    k.lay.layout(&d, None);
    let mut s = shoot(&k, (1000.0, 500.0), 0.0, &[], &[]);
    basic(&k, &s, 0.0);
    // 밑줄: 밑줄 사각형 자리가 어둡다.
    let page = &k.lay.output().pages[0];
    let line = page
        .items
        .iter()
        .find_map(|i| match i {
            Item::Rect { rect, .. } if rect[3] < 2.0 => Some(*rect),
            _ => None,
        })
        .expect("밑줄 사각형");
    let (x, y) = s.geo.to_px(0, line[0], line[1]);
    let mid = y + line[3] * s.geo.k / 2.0;
    let ink = s.share(x + 1.0, mid, line[2] * s.geo.k - 2.0, 0.5, dark);
    assert!(ink > 0.9, "밑줄: {ink}");
    s.save("02-styles");
}

#[test]
fn selection_color() {
    let mut k = kit();
    let d = doc(json!({}), &plain(&LINES));
    k.lay.layout(&d, None);
    let sel = k.lay.selection(&Pos::new("p0", 10), &Pos::new("p2", 20));
    assert!(sel.len() >= 3);
    let mut s = shoot(&k, (1000.0, 700.0), 0.0, &sel, &[]);
    basic(&k, &s, 0.0);
    // 가운데 줄의 선택 영역: 글자가 없는 곳은 선택 색.
    let r = sel[1];
    let (x, y) = s.geo.to_px(0, r.x, r.y);
    let share = s.share(x, y, r.w * s.geo.k, r.h * s.geo.k, |p| {
        p == [0xE5, 0xF4, 0xFF, 0xFF]
    });
    assert!(share > 0.4, "선택 색: {share}");
    s.save("03-selection");
}

#[test]
fn composing_underline() {
    let mut k = kit();
    let d = doc(json!({}), &plain(&LINES[..2]));
    let ov = Overlay {
        para: "p0".into(),
        at: "Kafka ".len(),
        text: "조합중".into(),
    };
    k.lay.layout(&d, Some(&ov));
    let pre = k.lay.preedit_rects();
    assert_eq!(pre.len(), 1);
    let mut s = shoot(&k, (1000.0, 500.0), 0.0, &[], &pre);
    basic(&k, &s, 0.0);
    let r = pre[0];
    let (x, y) = s.geo.to_px(0, r.x, r.y + r.h);
    let share = s.share(x + 1.0, y - 0.75, r.w * s.geo.k - 2.0, 0.5, |p| {
        p == [0x0A, 0x6D, 0xC2, 0xFF]
    });
    assert!(share > 0.9, "조합 밑줄: {share}");
    s.save("04-composing");
}

fn two_pages() -> Document {
    let texts: Vec<&str> = (0..60).map(|i| LINES[i % LINES.len()]).collect();
    doc(json!({}), &plain(&texts))
}

#[test]
fn scroll_moves_content() {
    let mut k = kit();
    let d = two_pages();
    k.lay.layout(&d, None);
    assert_eq!(k.lay.output().pages.len(), 2);
    let view = (1000.0, 700.0);
    let top = shoot(&k, view, 0.0, &[], &[]);
    basic(&k, &top, 0.0);
    // 첫 쪽의 끝과 둘째 쪽의 처음이 함께 보이는 자리.
    let scroll = (top.geo.top(1) - 400.0).round();
    let mut a = shoot(&k, view, scroll, &[], &[]);
    let b = shoot(&k, view, scroll + 120.0, &[], &[]);
    // 쪽 사이의 틈은 바탕색, 둘째 쪽의 여백은 흰색.
    let gap = a.geo.top(1) - 12.0 - scroll;
    assert_eq!(a.at(500.0, gap), [0xF5, 0xF5, 0xF5, 0xFF]);
    assert!(white(a.at(500.0, a.geo.top(1) - scroll + 10.0)));
    // 겹치는 줄은 스크롤한 만큼 옮겨진 같은 화소다.
    let shift = (120.0 * SCALE) as usize;
    let row = a.w as usize * 4;
    let rows = a.h as usize - shift;
    let same = (0..rows)
        .filter(|&r| a.px[(r + shift) * row..(r + shift + 1) * row] == b.px[r * row..(r + 1) * row])
        .count();
    assert!(same as f32 / rows as f32 > 0.99, "같은 줄 {same}/{rows}");
    a.save("05-scrolled");
}

#[test]
fn narrow_view_fits_page() {
    let mut k = kit();
    let d = doc(json!({}), &plain(&LINES));
    k.lay.layout(&d, None);
    // 쪽 너비에 맞춤(배율 0). 50%보다 작아지지 않는 폭으로 본다.
    let mut s = shoot_with(&k, (460.0, 600.0), 0.0, &[], &[], &mut none(), 0.0);
    assert!(s.geo.k < 4.0 / 3.0);
    assert!((s.geo.left - 24.0).abs() < 0.01);
    basic(&k, &s, 0.0);
    // 쪽의 오른쪽 끝 바로 안은 흰색, 그 밖 여백은 바탕색 쪽이다.
    assert!(white(s.at(460.0 - 25.0, 100.0)));
    assert!(!white(s.at(460.0 - 4.0, 100.0)));
    s.save("06-narrow");
}

#[test]
fn bad_buffers_are_refused() {
    let mut k = kit();
    let d = doc(json!({}), &plain(&LINES[..1]));
    k.lay.layout(&d, None);
    let out = k.lay.output();
    let scene = Scene {
        layout: out,
        geo: Geo::new(100.0, out.page_size, 1, 1.0),
        colors: COLORS,
        selection: &[],
        preedit: &[],
        guides: false,
    };
    let mut px = vec![0u8; 100];
    let mut run = |width, height, stride, scale| {
        let frame = Frame {
            pixels: &mut px,
            width,
            height,
            stride,
            scale,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
        draw(frame, &k.faces, &scene, &mut none()).is_ok()
    };
    // 줄 간격이 너비보다 작다, 버퍼가 작다, 배율이 틀렸다, 크기가 0이다.
    assert!(!run(10, 2, 39, 1.0));
    assert!(!run(10, 3, 40, 1.0));
    assert!(!run(10, 2, 40, f32::NAN));
    assert!(!run(0, 2, 40, 1.0));
    assert!(run(10, 2, 40, 1.0));
}

/// 생성된 글(목록 표지, 채움선) 자리에 어두운 화소가 있는가.
fn generated_ink(k: &Kit, s: &Shot) -> usize {
    let mut n = 0;
    for it in &k.lay.output().pages[0].items {
        if let Item::Glyphs {
            text: Some(_),
            glyphs,
            size,
            ..
        } = it
        {
            let (x, y) = s.geo.to_px(0, glyphs[0].x, glyphs[0].y - size * 0.8);
            let w = (glyphs[glyphs.len() - 1].x - glyphs[0].x + size * 0.6) * s.geo.k;
            if s.share(x, y, w, size * 0.9 * s.geo.k, dark) > 0.01 {
                n += 1;
            }
        }
    }
    n
}

/// 목록 9수준(글머리표와 번호).
#[test]
fn lists_nine_levels() {
    let mut k = kit();
    let mut paras = Vec::new();
    for kind in ["BULLET", "ORDERED"] {
        for lv in 0..9 {
            let t = format!("{kind} 수준 {lv}: {}", LINES[lv % LINES.len()]);
            paras.push(json!({"runs": [{"t": t}], "pf": {"list": {"kind": kind, "level": lv}}}));
            if kind == "ORDERED" {
                // 같은 수준의 둘째 항목(번호가 이어진다).
                paras.push(json!({"runs": [{"t": "둘째 항목"}], "pf": {"list": {"kind": kind, "level": lv}}}));
            }
        }
        paras.push(json!({"runs": [{"t": ""}]}));
    }
    paras.push(json!({"runs": [{"t": "열넷을 넘는 한글 번호"}], "pf": {"list": {"kind": "ORDERED", "level": 1, "start": 14}}}));
    paras
        .push(json!({"runs": [{"t": "거 다음"}], "pf": {"list": {"kind": "ORDERED", "level": 1}}}));
    let d = doc(json!({}), &paras);
    k.lay.layout(&d, None);
    let mut s = shoot(&k, (900.0, 2500.0), 0.0, &[], &[]);
    basic(&k, &s, 0.0);
    assert!(generated_ink(&k, &s) >= 18, "표지 자리에 잉크");
    s.save("07-lists");
}

/// 탭 네 맞춤과 채움선(이력서의 날짜 정렬).
#[test]
fn tabs_four_aligns_and_leaders() {
    let mut k = kit();
    let tab =
        |align: &str, leader: &str, pos: f64| json!({"pos": pos, "align": align, "leader": leader});
    let body_w = 595.28 - 2.0 * 56.69;
    let para = |t: &str, tabs: Value| json!({"runs": [{"t": t}], "pf": {"tabs": tabs}});
    let paras = vec![
        para("왼쪽\t2023.01", json!([tab("LEFT", "NONE", 120.0)])),
        para(
            "가운데\t가운데 맞춤 글",
            json!([tab("CENTER", "NONE", body_w / 2.0)]),
        ),
        para(
            "오른쪽\t2023.01 – 2024.05",
            json!([tab("RIGHT", "NONE", body_w)]),
        ),
        para("소수점\t1234.56", json!([tab("DECIMAL", "NONE", 300.0)])),
        para("소수점\t7.5", json!([tab("DECIMAL", "NONE", 300.0)])),
        para("점 채움\t12쪽", json!([tab("RIGHT", "DOT", body_w)])),
        para("줄표 채움\t34쪽", json!([tab("RIGHT", "DASH", body_w)])),
        para("밑줄 채움\t56쪽", json!([tab("RIGHT", "LINE", body_w)])),
        para(
            "견본상사 · 서버 개발 인턴\t2025.07 – 2026.02",
            json!([tab("RIGHT", "NONE", body_w)]),
        ),
        para(
            "넓은 조각\t이 조각은 정지보다 넓어서 탭의 너비가 0이 됩니다",
            json!([tab("RIGHT", "NONE", 80.0)]),
        ),
    ];
    let d = doc(json!({}), &paras);
    k.lay.layout(&d, None);
    let mut s = shoot(&k, (900.0, 600.0), 0.0, &[], &[]);
    basic(&k, &s, 0.0);
    assert!(generated_ink(&k, &s) >= 3, "채움선 자리에 잉크");
    s.save("08-tabs");
}

/// 링크가 든 쪽(밑줄과 링크 색, 줄을 넘는 링크).
#[test]
fn links_page() {
    let mut k = kit();
    let paras = vec![
        json!({"runs": [{"t": "포트폴리오: "}, {"t": "github.com/example", "link": "https://github.com/example"}]}),
        json!({"runs": [{"t": "메일: "}, {"t": "seoyeon@example.com", "link": "mailto:seoyeon@example.com"}, {"t": " · 전화: "}, {"t": "010-1234-5678", "link": "tel:010-1234-5678"}]}),
        json!({"runs": [{"t": "색을 정한 링크", "link": "https://a.example", "s": {"color": "#C62828FF"}}]}),
        json!({"runs": [{"t": "줄을 넘는 링크: "}, {"t": format!("{} {}", LINES[0], LINES[1]), "link": "https://example.com/long"}, {"t": " 뒤의 글"}]}),
    ];
    let d = doc(json!({}), &paras);
    k.lay.layout(&d, None);
    let mut s = shoot(&k, (900.0, 500.0), 0.0, &[], &[]);
    basic(&k, &s, 0.0);
    let links = k.lay.output().pages[0]
        .items
        .iter()
        .filter(|i| matches!(i, Item::Link { .. }))
        .count();
    assert!(links >= 6, "링크 영역: {links}");
    // 링크 글은 파란색으로 그려진다.
    let (x, y) = s.geo.to_px(0, 56.69 + 60.0, 56.69);
    let blue = s.share(x, y, 80.0, 20.0, |p| p[2] > 150 && p[0] < 120);
    assert!(blue > 0.01, "링크 색: {blue}");
    s.save("09-links");
}

/// 머리글·바닥글과 쪽 번호가 든 두 쪽 문서(1쪽은 머리글을 뺀다).
#[test]
fn header_footer_two_pages() {
    let mut k = kit();
    let mut paras = Vec::new();
    for i in 0..44 {
        paras.push(json!({"runs": [{"t": LINES[i % LINES.len()]}]}));
    }
    let root = json!({
        "header": {"l": "김카리", "c": "백엔드 엔지니어", "r": "이력서"},
        "footer": {"l": "kari@example.com", "c": "{쪽} / {전체}", "r": "2026.11"},
        "hfSkipFirst": true,
    });
    let d = doc(root, &paras);
    k.lay.layout(&d, None);
    assert_eq!(k.lay.output().pages.len(), 2);
    let mut s = shoot(&k, (900.0, 2600.0), 0.0, &[], &[]);
    basic(&k, &s, 0.0);
    // 2쪽의 머리글 자리(위 여백 띠)에 어두운 화소, 1쪽의 같은 자리는 흰색.
    let (x1, y1) = s.geo.to_px(0, 56.69, 22.0);
    let (x2, y2) = s.geo.to_px(1, 56.69, 22.0);
    let gray = |p: [u8; 4]| p[0] < 200;
    assert!(
        s.share(x1, y1, 40.0, 16.0, gray) < 0.001,
        "1쪽에는 머리글이 없다"
    );
    assert!(s.share(x2, y2, 40.0, 16.0, gray) > 0.01, "2쪽 머리글");
    s.save("12-header-footer");
}

/// 가로 방향 문서(A4 가로, 쪽 번호 바닥글).
#[test]
fn landscape_page() {
    let mut k = kit();
    let root = json!({
        "page": {"preset": "A4", "w": 841.89, "h": 595.28, "orientation": "landscape",
                 "margin": {"t": 42.52, "r": 42.52, "b": 42.52, "l": 42.52}},
        "footer": {"r": "{쪽} / {전체}"},
    });
    let d = doc(root, &plain(&LINES));
    k.lay.layout(&d, None);
    assert_eq!(k.lay.output().page_size, (841.89, 595.28));
    let mut s = shoot(&k, (1200.0, 900.0), 0.0, &[], &[]);
    basic(&k, &s, 0.0);
    s.save("13-landscape");
}

// ---- 그림 ----

use kari_render::{prepare_image, Images, IMAGE_BUDGET, MAX_BYTES};
use skia_safe::{surfaces, Codec, Color, Data, EncodedOrigin, Paint};

fn data(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// 증명사진처럼 보이는 그림(바탕, 어깨, 얼굴). `format`으로 담는다.
fn photo(w: i32, h: i32, format: EncodedImageFormat) -> Vec<u8> {
    let mut s = surfaces::raster_n32_premul((w, h)).unwrap();
    let c = s.canvas();
    c.clear(Color::from_rgb(214, 226, 238));
    let mut p = Paint::default();
    p.set_anti_alias(true)
        .set_color(Color::from_rgb(52, 64, 84));
    let (fw, fh) = (w as f32, h as f32);
    c.draw_oval(
        skia_safe::Rect::from_xywh(fw * 0.1, fh * 0.62, fw * 0.8, fh * 0.6),
        &p,
    );
    p.set_color(Color::from_rgb(236, 200, 170));
    c.draw_circle((fw * 0.5, fh * 0.42), fw * 0.22, &p);
    let img = s.image_snapshot();
    img.peek_pixels().unwrap().encode(format, Some(90)).unwrap()
}

/// 넣을 그림의 화소 크기와 그 좌표의 색(돌려 담긴 것을 본다).
fn decoded(bytes: &[u8]) -> (skia_safe::Image, EncodedOrigin) {
    let mut c = Codec::from_data(Data::new_copy(bytes)).unwrap();
    let o = c.origin();
    (c.get_image(None, None).unwrap(), o)
}

/// 넣기 전의 검사와 정리.
#[test]
fn 그림_넣기_검사와_정리() {
    // PNG, JPEG: 원래 바이트 그대로
    for name in ["photo.png", "photo.jpg"] {
        let b = data(name);
        let p = prepare_image(&b).unwrap();
        assert_eq!((p.width, p.height, p.changed), (40, 30, false), "{name}");
        assert_eq!(p.bytes, b);
    }
    // 방향 정보(RightTop, 90° 돌림)가 든 16×8 JPEG → 8×16으로 돌려 담고, 다시 담은 것에는 방향 정보가 없다.
    let p = prepare_image(&data("exif-right-top.jpg")).unwrap();
    assert_eq!((p.width, p.height, p.changed), (8, 16, true));
    let (img, origin) = decoded(&p.bytes);
    assert_eq!(origin, EncodedOrigin::TopLeft);
    assert_eq!((img.width(), img.height()), (8, 16));
    // 원래 왼쪽 반(빨강)이 시계 방향으로 돌아 위쪽 반이 된다.
    let px = |x: i32, y: i32| {
        let mut b = [0u8; 4];
        let info = skia_safe::ImageInfo::new(
            (1, 1),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        assert!(img.read_pixels(
            &info,
            &mut b,
            4,
            (x, y),
            skia_safe::image::CachingHint::Allow
        ));
        b
    };
    assert!(
        px(4, 2)[0] > 150 && px(4, 2)[2] < 120,
        "위는 빨강: {:?}",
        px(4, 2)
    );
    assert!(px(4, 13)[2] > 150, "아래는 파랑: {:?}", px(4, 13));
    // 긴 변이 4,000px를 넘으면 줄여 같은 형식으로 담는다.
    let p = prepare_image(&photo(4001, 10, EncodedImageFormat::PNG)).unwrap();
    assert_eq!((p.width, p.height, p.changed), (4000, 10, true));
    assert!(p.bytes.starts_with(b"\x89PNG"));
    let p = prepare_image(&photo(3000, 5000, EncodedImageFormat::JPEG)).unwrap();
    assert_eq!((p.width, p.height), (2400, 4000));
    assert!(p.bytes.starts_with(&[0xFF, 0xD8]));
    // 거절: 깨진 파일, 다른 형식, 20MB 초과
    let png = data("photo.png");
    assert!(prepare_image(&png[..40]).is_err());
    assert!(prepare_image(b"GIF89a\x01\x00\x01\x00").is_err());
    assert!(prepare_image(b"not an image").is_err());
    let mut big = png.clone();
    big.resize(MAX_BYTES + 1, 0);
    assert_eq!(
        prepare_image(&big).unwrap_err(),
        "20MB가 넘는 그림은 넣을 수 없습니다."
    );
}

/// 기억해 둔 푼 그림의 합계가 상한을 넘지 않는다. 다시 풀지 않는다.
#[test]
fn 푼_그림의_기억은_상한을_넘지_않는다() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let loads = Arc::new(AtomicUsize::new(0));
    let n = loads.clone();
    let pic = photo(200, 100, EncodedImageFormat::PNG); // 풀면 80,000바이트
    let mut images = Images::new(
        Box::new(move |h| {
            n.fetch_add(1, Ordering::Relaxed);
            (h != "없음").then(|| pic.clone())
        }),
        250_000,
    );
    for i in 0..10 {
        assert!(images.get(&format!("그림{i}")).is_some());
        assert!(images.bytes() <= 250_000, "{}", images.bytes());
    }
    assert_eq!(images.bytes(), 240_000);
    // 최근 것은 다시 풀지 않는다.
    let before = loads.load(Ordering::Relaxed);
    images.get("그림9");
    assert_eq!(loads.load(Ordering::Relaxed), before);
    assert!(images.get("없음").is_none());
    // 가장 큰 그림(4,000×4,000) 하나는 들어간다.
    const _: () = assert!(IMAGE_BUDGET >= 4000 * 4000 * 4);
}

/// 그림: 사진이 든 이력서 한 쪽(오른쪽 위 증명사진), 줄을 넘기는 그림, 쪽을 넘기는 그림.
#[test]
fn images_page() {
    let mut k = kit();
    let face = photo(300, 400, EncodedImageFormat::JPEG);
    let wide = photo(800, 300, EncodedImageFormat::PNG);
    let tall = photo(400, 600, EncodedImageFormat::PNG);
    let h = |n: char| n.to_string().repeat(64);
    let img = |n: char, w: f64, hh: f64| json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": h(n), "w": w, "h": hh, "alt": "사진"}});
    let mut paras = vec![
        json!({"runs": [img('a', 90.0, 120.0)], "pf": {"align": "RIGHT", "space": {"after": 0}}}),
        json!({"style": "title", "runs": [{"t": "김카리"}]}),
        json!({"runs": [{"t": "백엔드 엔지니어 · kari@example.com"}]}),
        json!({"style": "h1", "runs": [{"t": "경력"}]}),
    ];
    for line in &LINES[..6] {
        paras.push(json!({"runs": [{"t": line}]}));
    }
    // 남은 폭보다 넓어 다음 줄로 가는 그림
    paras
        .push(json!({"runs": [{"t": "구성도: "}, img('b', 470.0, 176.0), {"t": " 그림 뒤의 글"}]}));
    for i in 0..10 {
        paras.push(json!({"runs": [{"t": LINES[i % LINES.len()]}]}));
    }
    // 쪽에 남은 높이보다 높아 다음 쪽으로 가는 그림
    paras.push(json!({"runs": [img('c', 300.0, 450.0)]}));
    let d = doc(json!({}), &paras);
    k.lay.layout(&d, None);
    let pages: Vec<usize> = k
        .lay
        .output()
        .pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            p.items
                .iter()
                .filter(|it| matches!(it, Item::Image { .. }))
                .map(move |_| i)
        })
        .collect();
    assert_eq!(pages, [0, 0, 1], "셋째 그림은 다음 쪽");
    // 둘째 그림은 "구성도: " 뒤에 들어가지 않아 다음 줄의 처음에 놓인다.
    let second = k.lay.output().pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Image { rect, .. } => Some(*rect),
            _ => None,
        })
        .nth(1)
        .unwrap();
    assert!(
        (second[0] - 56.69).abs() < 0.01,
        "줄을 넘긴 그림: {second:?}"
    );
    let (a, b, c) = (face, wide, tall);
    let mut images = Images::new(
        Box::new(move |hh| match hh.chars().next() {
            Some('a') => Some(a.clone()),
            Some('b') => Some(b.clone()),
            Some('c') => Some(c.clone()),
            _ => None,
        }),
        IMAGE_BUDGET,
    );
    let mut s = shoot_with(&k, (900.0, 2600.0), 0.0, &[], &[], &mut images, 1.0);
    basic(&k, &s, 0.0);
    // 증명사진 자리에 그림의 얼굴색이 있다(회색 상자가 아니다).
    let r = k.lay.output().pages[0]
        .items
        .iter()
        .find_map(|i| match i {
            Item::Image { rect, .. } => Some(*rect),
            _ => None,
        })
        .unwrap();
    let (x, y) = s.geo.to_px(0, r[0] + r[2] * 0.5, r[1] + r[3] * 0.42);
    let skin = s.at(x, y);
    assert!(skin[0] > 200 && skin[2] < 200, "얼굴색: {skin:?}");
    assert!(images.bytes() <= IMAGE_BUDGET);
    s.save("14-images");
}

// ---- 표가 든 이력서 견본 ----

/// 표 하나의 op들. `cells[r][c]`는 (글, 셀 속성). 글이 `None`이면 가려진 셀(빈 문단).
fn table_ops(
    id: &str,
    cols: &[f64],
    header: usize,
    cells: &[Vec<(Option<Value>, Value)>],
) -> Vec<Value> {
    let mut ops = vec![
        json!({"op": "add", "node": {"id": id, "type": "table", "parent": "root",
        "props": {"cols": cols, "headerRows": header}}}),
    ];
    for (r, row) in cells.iter().enumerate() {
        let rid = format!("{id}r{r}");
        ops.push(json!({"op": "add", "node": {"id": rid, "type": "row", "parent": id}}));
        for (c, (runs, props)) in row.iter().enumerate() {
            let cid = format!("{id}c{r}_{c}");
            let mut props = props.clone();
            if runs.is_none() {
                props["covered"] = json!(true);
            }
            ops.push(json!({"op": "add", "node": {"id": cid, "type": "cell", "parent": rid, "props": props}}));
            let runs = runs.clone().unwrap_or(json!([{"t": ""}]));
            ops.push(json!({"op": "add", "node": {"id": format!("{id}p{r}_{c}"), "type": "para", "parent": cid,
                "props": {"runs": runs, "pf": {"space": {"before": 0, "after": 0}}}}}));
        }
    }
    ops
}

fn txt(t: &str) -> Option<Value> {
    Some(json!([{"t": t}]))
}

fn bold(t: &str) -> Option<Value> {
    Some(json!([{"t": t, "s": {"weight": 700}}]))
}

/// 이력서 모양의 표 문서: 사진(세 행을 합친 셀) + 인적 사항, 학력 표(머리 행 바탕), 두 쪽에 걸친 경력 표(머리 행 반복).
/// 사진 에셋은 'a' × 64.
fn table_resume() -> Document {
    let mut d = Document::new(Kind::Flow, "표 이력서").unwrap();
    let photo = json!([{"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 84, "h": 108}}]);
    let head = json!({"fill": "#EEF2F7FF", "alignV": "CENTER"});
    let k = json!({"fill": "#F7F7F7FF"});
    let none = json!({});
    let mut ops = vec![
        json!({"op": "text", "id": "n1", "at": 0, "ins": [{"t": "이력서"}]}),
        json!({"op": "set", "id": "n1", "props": {"style": "title"}}),
    ];
    ops.extend(table_ops(
        "id",
        &[100.0, 80.0, 301.9],
        0,
        &[
            vec![
                (
                    Some(photo),
                    json!({"span": {"rows": 3, "cols": 1}, "alignV": "CENTER"}),
                ),
                (txt("성명"), k.clone()),
                (bold("홍길동"), none.clone()),
            ],
            vec![
                (None, none.clone()),
                (txt("연락처"), k.clone()),
                (txt("010-1234-5678 · hello@example.com"), none.clone()),
            ],
            vec![
                (None, none.clone()),
                (txt("주소"), k.clone()),
                (txt("서울특별시 마포구 월드컵북로 00길 00"), none.clone()),
            ],
        ],
    ));
    let h1 = |id: &str, t: &str| json!({"op": "add", "node": {"id": id, "type": "para", "parent": "root", "props": {"style": "h1", "runs": [{"t": t}]}}});
    ops.push(h1("h_edu", "학력"));
    ops.extend(table_ops(
        "edu",
        &[110.0, 150.0, 130.0, 91.9],
        1,
        &[
            ["기간", "학교", "전공", "구분"]
                .iter()
                .map(|t| (bold(t), head.clone()))
                .collect(),
            ["2016.03 – 2020.02", "한국대학교", "컴퓨터공학", "졸업"]
                .iter()
                .map(|t| (txt(t), none.clone()))
                .collect(),
            ["2013.03 – 2016.02", "서울고등학교", "이과", "졸업"]
                .iter()
                .map(|t| (txt(t), none.clone()))
                .collect(),
        ],
    ));
    ops.push(h1("h_job", "경력"));
    let mut rows: Vec<Vec<(Option<Value>, Value)>> = vec![["기간", "회사", "담당 업무"]
        .iter()
        .map(|t| (bold(t), head.clone()))
        .collect()];
    let jobs = [
        "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.",
        "Redis 캐시를 도입하면서 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.",
        "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.",
        "장애 발생 시 서킷 브레이커와 재시도 정책이 어떻게 동작하는지 문서로 정리했습니다.",
    ];
    for i in 0..22 {
        rows.push(vec![
            (
                txt(&format!("{}.03 – {}.12", 2024 - i, 2024 - i)),
                none.clone(),
            ),
            (
                txt(["클로나믹", "견본상사", "예시페이"][i % 3]),
                none.clone(),
            ),
            (txt(jobs[i % jobs.len()]), none.clone()),
        ]);
    }
    ops.extend(table_ops("job", &[110.0, 80.0, 291.9], 1, &rows));
    ops.push(json!({"op": "add", "node": {"id": "end", "type": "para", "parent": "root", "props": {"runs": [{"t": "위 내용은 사실과 다름없습니다."}]}}}));
    d.apply_trusted(&Value::Array(ops)).unwrap();
    d
}

/// 표가 든 이력서를 그린다(두 쪽). `app/engine/target/kari-shots/15-table-resume.png`.
#[test]
fn table_resume_shot() {
    let mut k = kit();
    let d = table_resume();
    k.lay.layout(&d, None);
    assert_eq!(k.lay.output().pages.len(), 2, "경력 표가 두 쪽에 걸친다");
    let face = photo(300, 400, EncodedImageFormat::JPEG);
    let mut images = Images::new(Box::new(move |_| Some(face.clone())), IMAGE_BUDGET);
    let mut s = shoot_with(&k, (900.0, 2600.0), 0.0, &[], &[], &mut images, 1.0);
    basic(&k, &s, 0.0);
    // 머리 행 바탕색이 칸에 깔렸다.
    let page = &k.lay.output().pages[0];
    let bg = page
        .items
        .iter()
        .find_map(|i| match i {
            Item::Rect { rect, color } if *color == [0xEE, 0xF2, 0xF7, 0xFF] => Some(*rect),
            _ => None,
        })
        .unwrap();
    let (x, y) = s.geo.to_px(0, bg[0] + 2.0, bg[1] + 2.0);
    assert_eq!(s.at(x, y)[..3], [0xEE, 0xF2, 0xF7]);
    s.save("15-table-resume");
}

/// 글꼴에 없는 글자(한자, 이모지)는 빈 네모로 보이고, 있는 글자(가나)는 그대로 그린다. 크게(24pt) 그려 남긴다.
#[test]
fn missing_glyph_shot() {
    let mut k = kit();
    let big = |t: &str| json!({"runs": [{"t": t, "s": {"size": 24}}]});
    let d = doc(
        json!({}),
        &[
            big("성명 홍길동 (洪吉童)"),
            big("일본어 가나 あア는 글꼴에 있다"),
            big("이모지 😀 는 네모"),
            json!({"runs": [{"t": "본문 크기: 漢字 😀 あ"}]}),
        ],
    );
    k.lay.layout(&d, None);
    assert_eq!(k.lay.missing(), 3 + 1 + 3);
    let mut s = shoot(&k, (900.0, 500.0), 0.0, &[], &[]);
    // 첫 네모(洪)의 왼쪽 선 위는 글자색, 네모 가운데는 비어 있다. 네모의 선은 위, 아래, 왼쪽, 오른쪽 차례.
    let left = k.lay.output().pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { rect, .. } => Some(*rect),
            _ => None,
        })
        .nth(2)
        .unwrap();
    let at = "성명 홍길동 (".len();
    let c = k.lay.caret(&Pos::new("p0", at), false).unwrap();
    assert!((left[0] - c.x).abs() < 0.01, "네모는 글자 자리에");
    let mid = left[1] + left[3] / 2.0;
    let (x, y) = s.geo.to_px(0, left[0] + 12.0, mid);
    assert!(white(s.at(x, y)), "네모 안은 빈칸");
    let (x, y) = s.geo.to_px(0, left[0] + 0.25, mid);
    assert!(!white(s.at(x, y)), "네모의 왼쪽 선");
    s.save("16-missing-glyph");
}

/// 같은 보이는 영역(1000×700)에 50%, 100%, 200%로 그린다. 50%는 쪽 둘이 보이고, 200%는 쪽이 영역보다 넓다.
#[test]
fn zoom_steps_fill_the_same_view() {
    let mut k = kit();
    let lines: Vec<&str> = LINES.iter().cycle().take(90).copied().collect();
    let d = doc(json!({}), &plain(&lines));
    k.lay.layout(&d, None);
    for (z, name) in [(0.5, "z10-50"), (1.0, "z10-100"), (2.0, "z10-200")] {
        let mut s = shoot_with(&k, (1000.0, 700.0), 0.0, &[], &[], &mut none(), z);
        assert!((s.geo.zoom() - z).abs() < 1e-6);
        assert_eq!(s.geo.width > 1000.0, z == 2.0, "200%만 가로로 넘친다");
        // 쪽 왼쪽 끝 바로 안은 흰 종이(200%는 내용의 왼쪽 여백 24 뒤).
        assert!(white(s.at(s.geo.left + 2.0, 100.0)));
        s.save(name);
    }
}

/// 200%는 100% 그림을 늘린 것이 아니라 다시 그린 것이다. 같은 자리에 맞춰 놓고 글자 둘레의 화소를 견준다.
/// 100%를 두 배로 늘린 그림은 2×2 칸마다 한 색이지만, 다시 그린 그림은 글자 가장자리에서 칸 안의 색이 갈린다.
#[test]
fn zoom_redraws_instead_of_stretching() {
    let mut k = kit();
    let d = doc(json!({}), &plain(&LINES));
    k.lay.layout(&d, None);
    let mut a = shoot_with(&k, (1000.0, 350.0), 0.0, &[], &[], &mut none(), 1.0);
    // 위 여백(24)은 배율과 함께 커지지 않으므로 두 배 그림에 맞게 24만큼 내려 그린다.
    let mut b = shoot_with(&k, (2000.0, 700.0), -24.0, &[], &[], &mut none(), 2.0);
    assert!((b.geo.zoom() - 2.0).abs() < 1e-6 && (b.geo.left - 2.0 * a.geo.left).abs() < 1e-3);
    // 늘린 그림: b의 화소 (x, y) = a의 화소 (x/2, y/2).
    let stretched: Vec<u8> = (0..b.h as usize)
        .flat_map(|y| (0..b.w as usize).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let i = (y / 2) * a.w as usize * 4 + (x / 2) * 4;
            a.px[i..i + 4].to_vec()
        })
        .collect();
    let gray = |px: &[u8], x: usize, y: usize| {
        let i = y * b.w as usize * 4 + x * 4;
        (px[i] as i32 + px[i + 1] as i32 + px[i + 2] as i32) / 3
    };
    // 본문 글줄이 있는 곳(a의 논리 화소): 쪽 왼쪽 + 80 ~ +620, 위 110 ~ 300. b의 화소로 바꾼다.
    let to_b = |v: f32| (v * SCALE * 2.0) as usize;
    let (x0, x1) = (to_b(a.geo.left + 80.0), to_b(a.geo.left + 620.0));
    let (y0, y1) = (to_b(110.0), to_b(300.0));
    // 같은 자리에 그렸는지는 6화소 옮겨 견준 것보다 차가 작은지로 본다.
    let (mut inked, mut split, mut diff, mut moved, mut n) = (0, 0, 0i64, 0i64, 0i64);
    for by in (y0..y1).step_by(2) {
        for bx in (x0..x1).step_by(2) {
            let at = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| (bx + dx, by + dy));
            let cell = at.map(|(x, y)| gray(&b.px, x, y));
            if cell.iter().all(|v| *v > 250) {
                continue;
            }
            inked += 1;
            let (lo, hi) = (cell.iter().min().unwrap(), cell.iter().max().unwrap());
            split += (hi - lo > 24) as i32;
            for (x, y) in at {
                diff += (gray(&b.px, x, y) - gray(&stretched, x, y)).abs() as i64;
                moved += (gray(&b.px, x, y) - gray(&stretched, x + 6, y)).abs() as i64;
                n += 1;
            }
        }
    }
    let share = split as f32 / inked.max(1) as f32;
    let (mean, off) = (
        diff as f32 / n.max(1) as f32,
        moved as f32 / n.max(1) as f32,
    );
    eprintln!("글자 칸 {inked}, 색이 갈린 칸 {split} ({share:.2}), 늘린 그림과의 평균 차 {mean:.1}, 6화소 옮겼을 때 {off:.1}");
    a.save("z6-100");
    b.save("z6-200");
    let geo = b.geo;
    let (w, h) = (b.w, b.h);
    Shot {
        px: stretched.clone(),
        w,
        h,
        geo,
    }
    .save("z6-100-stretched");
    assert!(inked > 1000, "글자가 그려져야 한다");
    assert!(
        share > 0.3,
        "다시 그렸으면 글자 가장자리의 칸 안에서 색이 갈린다"
    );
    assert!(
        mean > 3.0 && mean < off * 0.7,
        "같은 글을 같은 자리에 그렸고(차가 작다) 늘린 것과는 다르다(0이 아니다)"
    );
}
