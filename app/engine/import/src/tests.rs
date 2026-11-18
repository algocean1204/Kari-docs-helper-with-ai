//! 가져오기 시험. 견본은 Word·Google Docs가 내는 꼴을 줄여 시험 안에서 만든다(사람과 회사는 지어낸 것).

use kari_doc::serde_json::{json, Value};

use crate::zip::tests::zip;
use crate::*;

const BODY: (f64, f64) = (481.9, 728.5);
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n-fake-";

mod docx_table;
mod hwpx;

/// 시험용 저장: PNG 머리면 받아 "h<길이>"와 20×10px로, 아니면 거절.
fn store(b: &[u8]) -> Option<(String, (u32, u32))> {
    b.starts_with(b"\x89PNG")
        .then(|| (format!("h{}", b.len()), (20, 10)))
}

fn run(bytes: &[u8]) -> Result<Imported, &'static str> {
    import(bytes, BODY, &mut store)
}

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

const STYLES: &str = r#"<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/></w:style>
<w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/><w:rPr><w:b/><w:sz w:val="40"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="2"><w:name w:val="heading 2"/></w:style>
<w:style w:type="paragraph" w:styleId="Mine"><w:name w:val="My Section"/><w:basedOn w:val="2"/></w:style>
<w:style w:type="paragraph" w:styleId="Out3"><w:name w:val="Custom"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Loud"><w:name w:val="Loud"/><w:rPr><w:b/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ListBullet"><w:name w:val="List Bullet"/><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style>"#;

const NUMBERING: &str = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="3"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>"#;

/// 본문 XML(`w:body` 안)과 더 넣을 파일로 DOCX를 만든다.
fn docx(body: &str, more: &[(&str, &[u8])]) -> Vec<u8> {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>{body}<w:sectPr/></w:body></w:document>"#
    );
    let styles = format!(r#"<?xml version="1.0"?><w:styles {W}>{STYLES}</w:styles>"#);
    let num = format!(r#"<?xml version="1.0"?><w:numbering {W}>{NUMBERING}</w:numbering>"#);
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="image" Target="media/image1.png"/><Relationship Id="rId8" Type="image" Target="media/pic.emf"/><Relationship Id="rId5" Type="hyperlink" Target="https://example.com/hong" TargetMode="External"/><Relationship Id="rId6" Type="hyperlink" Target="javascript:alert(1)" TargetMode="External"/></Relationships>"#;
    let fonts = format!(
        r#"<?xml version="1.0"?><w:fonts {W}><w:font w:name="맑은 고딕"/><w:font w:name="Calibri"/></w:fonts>"#
    );
    let mut files: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", b"<Types/>".to_vec()),
        ("word/document.xml", doc.into_bytes()),
        ("word/styles.xml", styles.into_bytes()),
        ("word/numbering.xml", num.into_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes().to_vec()),
        ("word/fontTable.xml", fonts.into_bytes()),
        ("word/media/image1.png", PNG.to_vec()),
        ("word/media/pic.emf", b"EMF-bytes".to_vec()),
    ];
    for (n, b) in more {
        files.push((n, b.to_vec()));
    }
    let list: Vec<(&str, &[u8], bool)> = files.iter().map(|(n, b)| (*n, &b[..], true)).collect();
    zip(&list)
}

fn p(style: &str, inner: &str) -> String {
    let ppr = if style.is_empty() {
        String::new()
    } else {
        format!(r#"<w:pPr><w:pStyle w:val="{style}"/></w:pPr>"#)
    };
    format!("<w:p>{ppr}{inner}</w:p>")
}

fn r(text: &str) -> String {
    format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
}

fn list_p(num: u32, lvl: u32, text: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="{num}"/></w:numPr></w:pPr>{}</w:p>"#,
        r(text)
    )
}

fn drawing(rid: &str) -> String {
    format!(
        r#"<w:r><w:drawing><wp:inline><wp:extent cx="1270000" cy="635000"/><a:graphic><a:graphicData><a:blip r:embed="{rid}"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
    )
}

fn styles_of(f: &Fragment) -> Vec<Option<&str>> {
    f.paras
        .iter()
        .map(|p| p.props.get("style").and_then(Value::as_str))
        .collect()
}

fn text_of(p: &FragPara) -> String {
    p.runs
        .iter()
        .filter_map(|r| r.get("t").and_then(Value::as_str))
        .collect()
}

#[test]
fn docx_제목_1에서_3과_제목을_스타일로_옮긴다() {
    let body = [
        p("Title", &r("홍길동 이력서")),
        p("1", &r("경력")),
        p("2", &r("견본상사")),
        p("Mine", &r("바탕이 제목 2")),
        p("Out3", &r("개요 수준 3")),
        p("", &r("본문")),
    ]
    .concat();
    let got = run(&docx(&body, &[])).unwrap();
    let f = &got.fragment;
    assert_eq!(
        styles_of(f),
        [
            Some("title"),
            Some("h1"),
            Some("h2"),
            Some("h2"),
            Some("h3"),
            None
        ]
    );
    // 제목 안의 굵기와 크기는 문단 스타일에 맡긴다.
    assert_eq!(f.paras[1].runs, [json!({"t": "경력"})]);
    let s = summary(f);
    assert_eq!(s.headings[0], (0, "홍길동 이력서".to_string()));
    assert_eq!(s.headings[1], (1, "경력".to_string()));
    assert_eq!(s.headings[4], (3, "개요 수준 3".to_string()));
    assert_eq!((s.paras, s.tables, s.images), (6, 0, 0));
}

#[test]
fn docx_겹친_목록의_종류_단계_시작_번호를_옮긴다() {
    let body = [
        list_p(1, 0, "글머리표"),
        list_p(1, 1, "안의 번호"),
        list_p(1, 1, "둘째 번호"),
        list_p(2, 0, "번호 하나"),
        p("ListBullet", &r("스타일의 목록")),
        list_p(0, 0, "목록 아님"),
    ]
    .concat();
    let f = run(&docx(&body, &[])).unwrap().fragment;
    let lists: Vec<Value> = f
        .paras
        .iter()
        .map(|p| {
            p.props
                .get("pf")
                .map_or(Value::Null, |pf| pf["list"].clone())
        })
        .collect();
    assert_eq!(
        lists,
        [
            json!({"kind": "BULLET", "level": 0}),
            json!({"kind": "ORDERED", "level": 1, "start": 3}),
            json!({"kind": "ORDERED", "level": 1}),
            json!({"kind": "ORDERED", "level": 0}),
            json!({"kind": "BULLET", "level": 0}),
            Value::Null,
        ]
    );
}

#[test]
fn docx_굵게_기울임_색_크기_첨자_링크를_옮기고_위험한_주소는_글로만_둔다() {
    let body = p(
        "",
        &[
            r#"<w:r><w:rPr><w:b/><w:i/><w:color w:val="C00000"/><w:sz w:val="28"/></w:rPr><w:t>강조</w:t></w:r>"#,
            r#"<w:r><w:rPr><w:b w:val="0"/><w:color w:val="000000"/><w:u w:val="single"/><w:strike/></w:rPr><w:t>밑줄</w:t></w:r>"#,
            r#"<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t>2</w:t></w:r>"#,
            r#"<w:hyperlink r:id="rId5"><w:r><w:rPr><w:color w:val="0563C1"/></w:rPr><w:t>hong@example.com</w:t></w:r></w:hyperlink>"#,
            r#"<w:hyperlink r:id="rId6"><w:r><w:t>나쁜 링크</w:t></w:r></w:hyperlink>"#,
            r#"<w:del><w:r><w:delText>지운 글</w:delText></w:r></w:del><w:ins><w:r><w:t>넣은 글</w:t></w:r></w:ins>"#,
        ]
        .concat(),
    );
    let f = run(&docx(&body, &[])).unwrap().fragment;
    assert_eq!(
        f.paras[0].runs,
        [
            json!({"t": "강조", "s": {"weight": 700, "italic": true, "color": "#C00000FF", "size": 14.0}}),
            json!({"t": "밑줄", "s": {"underline": true, "strike": true}}),
            json!({"t": "2", "s": {"vertical": "SUPER"}}),
            json!({"t": "hong@example.com", "link": "https://example.com/hong"}),
            json!({"t": "나쁜 링크넣은 글"}),
        ]
    );
}

#[test]
fn docx_문단_스타일의_굵기와_맞춤을_옮긴다() {
    let body = r#"<w:p><w:pPr><w:pStyle w:val="Loud"/><w:jc w:val="center"/></w:pPr><w:r><w:t>가운데</w:t></w:r></w:p>"#;
    let f = run(&docx(body, &[])).unwrap().fragment;
    assert_eq!(f.paras[0].props["pf"], json!({"align": "CENTER"}));
    assert_eq!(
        f.paras[0].runs,
        [json!({"t": "가운데", "s": {"weight": 700}})]
    );
}

#[test]
fn docx_합친_칸과_머리_행과_바탕이_있는_표를_옮긴다() {
    let tc = |props: &str, text: &str| {
        format!("<w:tc><w:tcPr>{props}</w:tcPr>{}</w:tc>", p("", &r(text)))
    };
    let body = format!(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/><w:gridCol w:w="4000"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr>{}{}</w:tr>
<w:tr>{}{}{}</w:tr>
<w:tr>{}{}{}</w:tr></w:tbl>"#,
        tc(
            r#"<w:gridSpan w:val="2"/><w:shd w:val="clear" w:fill="D9E2F3"/>"#,
            "견본상사"
        ),
        tc("", "기간"),
        tc(r#"<w:vMerge w:val="restart"/>"#, "백엔드"),
        tc("", "2024"),
        tc("", "API"),
        tc("<w:vMerge/>", ""),
        tc("", "2025"),
        tc("", "결제"),
    );
    let f = run(&docx(&body, &[])).unwrap().fragment;
    let t = f.paras[0].table.as_ref().unwrap();
    assert_eq!(t.props["cols"], json!([100.0, 150.0, 200.0]));
    assert_eq!(t.props["headerRows"], json!(1));
    assert_eq!(t.rows.len(), 3);
    let cell = |r: usize, c: usize| t.rows[r].1[c].as_ref();
    let (p00, ps) = cell(0, 0).unwrap();
    assert_eq!(p00["span"], json!({"rows": 1, "cols": 2}));
    assert_eq!(p00["fill"], json!("#D9E2F3FF"));
    assert_eq!(text_of(&ps[0]), "견본상사");
    assert!(cell(0, 1).is_none(), "가로로 합쳐 가려진 칸");
    assert_eq!(cell(1, 0).unwrap().0["span"], json!({"rows": 2, "cols": 1}));
    assert!(cell(2, 0).is_none(), "세로로 합쳐 가려진 칸");
    assert_eq!(text_of(&cell(2, 2).unwrap().1[0]), "결제");
    assert_eq!(summary(&f).tables, 1);
}

#[test]
fn docx_표_안의_표와_큰_표는_글로_풀어_쓴다() {
    let inner = format!(
        "<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}</w:tc></w:tr></w:tbl>",
        p("", &r("안1")),
        p("", &r("안2"))
    );
    let outer = format!(
        "<w:tbl><w:tr><w:tc>{}{inner}</w:tc></w:tr></w:tbl>",
        p("", &r("밖"))
    );
    let rows: String = (0..51)
        .map(|i| format!("<w:tr><w:tc>{}</w:tc></w:tr>", p("", &r(&i.to_string()))))
        .collect();
    let big = format!("<w:tbl>{rows}</w:tbl>");
    let got = run(&docx(&(outer + &big), &[])).unwrap();
    let f = &got.fragment;
    let t = f.paras[0].table.as_ref().unwrap();
    let ps = &t.rows[0].1[0].as_ref().unwrap().1;
    assert_eq!(
        ps.iter().map(text_of).collect::<Vec<_>>(),
        ["밖", "안1\t안2"]
    );
    assert_eq!(f.paras.len(), 1 + 51);
    assert!(
        got.lost
            .contains(&"표 안의 표 1개(글로 풀어 씀)".to_string()),
        "{:?}",
        got.lost
    );
    assert!(
        got.lost.iter().any(|l| l.starts_with("큰 표 1개")),
        "{:?}",
        got.lost
    );
}

#[test]
fn docx_그림은_넣고_못_읽는_그림과_개체는_목록에_남긴다() {
    let body = [
        p("", &(r("사진: ") + &drawing("rId9"))),
        p("", &drawing("rId8")),
        p("", r#"<w:r><w:drawing><wp:anchor><a:graphic><a:graphicData><c:chart r:id="rIdX"/></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#),
        p("", r#"<w:r><w:drawing><wp:anchor><a:graphic><wps:txbx><w:txbxContent><w:p><w:r><w:t>상자</w:t></w:r></w:p></w:txbxContent></wps:txbx></a:graphic></wp:anchor></w:drawing></w:r>"#),
        p("", r#"<w:r><w:t>각주</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:commentReference w:id="0"/></w:r>"#),
        p("", r#"<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>"#),
    ]
    .concat();
    let header =
        format!(r#"<w:hdr {W}><w:p><w:r><w:t>홍길동 · 010-0000-0000</w:t></w:r></w:p></w:hdr>"#);
    let got = run(&docx(&body, &[("word/header1.xml", header.as_bytes())])).unwrap();
    let f = &got.fragment;
    // 1,270,000 EMU = 100pt, 635,000 EMU = 50pt.
    assert_eq!(
        f.paras[0].runs[1],
        json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": format!("h{}", PNG.len()), "w": 100.0, "h": 50.0}})
    );
    assert_eq!(summary(f).images, 1);
    assert_eq!(
        got.lost,
        [
            "글꼴 Calibri, 맑은 고딕(Pretendard로 바꿈)",
            "그림 1개(읽지 못했거나 PNG·JPEG가 아니라 뺌)",
            "글 상자 1개(뺌)",
            "도형·차트 1개(뺌)",
            "각주·미주 1개(뺌)",
            "메모 1개(뺌)",
            "수식 1개(뺌)",
            "머리글·바닥글(뺌)",
        ]
    );
}

#[test]
fn docx_밖의_엔티티를_펼치지_않는다() {
    let doc = format!(
        r#"<?xml version="1.0"?><!DOCTYPE w:document [<!ENTITY x SYSTEM "file:///etc/hosts">]><w:document {W}><w:body><w:p><w:r><w:t>&x;홍길동</w:t></w:r></w:p></w:body></w:document>"#
    );
    let z = zip(&[("word/document.xml", doc.as_bytes(), true)]);
    let f = run(&z).unwrap().fragment;
    assert_eq!(text_of(&f.paras[0]), "&x;홍길동");
}

#[test]
fn docx_그림이_꾸러미_밖을_가리키면_빼고_꾸러미는_거절한다() {
    let rels =
        r#"<Relationships><Relationship Id="rId9" Target="../../secret.png"/></Relationships>"#;
    let doc = format!(
        r#"<w:document {W}><w:body>{}</w:body></w:document>"#,
        p("", &drawing("rId9"))
    );
    let z = zip(&[
        ("word/document.xml", doc.as_bytes(), true),
        ("word/_rels/document.xml.rels", rels.as_bytes(), true),
    ]);
    let got = run(&z);
    // `word/../../secret.png`는 꾸러미의 뿌리 위라 읽지 않는다. 그림 하나가 빠져 글이 없다.
    assert_eq!(got.err(), Some(EMPTY));
    let z = zip(&[
        ("word/document.xml", doc.as_bytes(), true),
        ("../evil.xml", b"x", false),
    ]);
    assert_eq!(run(&z).err(), Some(zip::UNSAFE_PATH));
}

#[test]
fn 압축_폭탄과_큰_파일은_거절한다() {
    let bomb = vec![b' '; zip::MAX_ENTRY + 10];
    let z = zip(&[("word/document.xml", &bomb, true)]);
    assert_eq!(run(&z).err(), Some(zip::TOO_BIG));
    let big = vec![b'a'; MAX_FILE + 1];
    assert_eq!(run(&big).err(), Some(TOO_LARGE));
    assert_eq!(run(b"").err(), Some(EMPTY));
    assert!(run(b"PK\x03\x04broken").is_err());
}

/// Google Docs의 "웹 페이지(.html, 압축)" 꼴: 클래스 스타일, 겹치지 않은 목록과 단계 클래스, 넘김 링크, 그림 폴더.
const GDOCS: &str = r#"<html><head><meta content="text/html; charset=UTF-8" http-equiv="content-type"><style type="text/css">ul.lst-kix_a-0{list-style-type:none}.lst-kix_a-0>li:before{content:"\0025cf   "}.c1{color:#000000;font-weight:700;font-size:11pt;font-family:"Arial"}.c2{color:#1155cc;text-decoration:underline}.c3{font-family:"Nanum Gothic";font-style:italic}.title{font-size:26pt}</style></head><body class="c5 doc-content"><p class="c4 title" id="h.x"><span class="c1">홍길동</span></p><h1 class="c6" id="h.y"><span>경력</span></h1><ul class="c7 lst-kix_a-0 start"><li class="c8 li-bullet-0"><span class="c3">견본상사</span></li></ul><ul class="c7 lst-kix_a-1 start"><li class="c8 li-bullet-0"><span>백엔드 개발</span></li></ul><p><span class="c2"><a href="https://www.google.com/url?q=https://example.com/portfolio%3Fa%3D1&amp;sa=D&amp;source=editors">포트폴리오</a></span></p><p><span style="overflow: hidden; display: inline-block; width: 151.50px; height: 75.75px;"><img alt="증명사진" src="images/image1.png" style="width: 151.50px; height: 75.75px;" title=""></span><img src="images/missing.png"><img src="https://example.com/x.png"></p><svg><text>버림</text></svg></body></html>"#;

#[test]
fn google_docs_웹_페이지_꾸러미를_그림과_함께_읽는다() {
    let z = zip(&[
        ("Hong.html", GDOCS.as_bytes(), true),
        ("images/image1.png", PNG, false),
    ]);
    let got = run(&z).unwrap();
    let f = &got.fragment;
    assert_eq!(styles_of(f)[1], Some("h1"));
    assert_eq!(
        f.paras[0].runs,
        [json!({"t": "홍길동", "s": {"weight": 700, "size": 11.0}})]
    );
    let list = |i: usize| f.paras[i].props["pf"]["list"].clone();
    assert_eq!(list(2), json!({"kind": "BULLET", "level": 0}));
    assert_eq!(list(3), json!({"kind": "BULLET", "level": 1}));
    assert_eq!(
        f.paras[4].runs[0]["link"],
        json!("https://example.com/portfolio?a=1")
    );
    // 151.5px × 0.75 = 113.63pt
    assert_eq!(
        f.paras[5].runs,
        [
            json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": format!("h{}", PNG.len()), "w": 113.63, "h": 56.81, "alt": "증명사진"}})
        ]
    );
    assert_eq!(
        got.lost,
        [
            "글꼴 Arial, Nanum Gothic(Pretendard로 바꿈)",
            "그림 1개(읽지 못했거나 PNG·JPEG가 아니라 뺌)",
            "웹 주소의 그림 1개(내려받지 않아 뺌)",
            "그림이 아닌 개체 1개(SVG, 동영상, 입력 칸 등이라 뺌)",
        ]
    );
}

#[test]
fn html_파일은_data_주소의_그림만_받고_곁의_파일은_알린다() {
    // "aGVsbG8=" = "hello"(PNG가 아니라 받지 않음), PNG 머리를 base64로.
    let png64 = "iVBORw0KGgotZmFrZS0=";
    let html = format!(
        r#"<h2>자기소개</h2><p>홍길동 <img src="data:image/png;base64,{png64}" width="40"><img src="data:image/png;base64,aGVsbG8="><img src="photo.jpg"></p><table><tr><td colspan="2">견본상사</td></tr><tr><td>2024</td><td>백엔드</td></tr></table>"#
    );
    let got = run(html.as_bytes()).unwrap();
    let f = &got.fragment;
    assert_eq!(styles_of(f)[0], Some("h2"));
    // 너비만 주면 그림의 비율(20×10)을 지킨다: 40px = 30pt, 높이 15pt.
    assert_eq!(f.paras[1].runs[1]["o"]["w"], json!(30.0));
    assert_eq!(f.paras[1].runs[1]["o"]["h"], json!(15.0));
    let t = f.paras[2].table.as_ref().unwrap();
    assert_eq!(
        t.rows[0].1[0].as_ref().unwrap().0["span"],
        json!({"rows": 1, "cols": 2})
    );
    assert_eq!(
        got.lost,
        [
            "그림 1개(읽지 못했거나 PNG·JPEG가 아니라 뺌)",
            "HTML 밖의 그림 파일 1개(함께 고른 파일이 아니라 뺌)",
        ]
    );
}

#[test]
fn html의_그림은_개수_상한까지만_넣는다() {
    // 서로 다른 그림 302개. 같은 그림을 여러 번 가리키는 것은 한 번으로 친다(아래).
    let n = MAX_IMAGES + 2;
    let names: Vec<String> = (0..n).map(|i| format!("images/{i}.png")).collect();
    let html: String = names.iter().map(|s| format!("<img src=\"{s}\">")).collect();
    let html = format!("<p>글{html}{}</p>", "<img src=\"images/0.png\">".repeat(5));
    let bytes: Vec<Vec<u8>> = (0..n)
        .map(|i| [PNG, i.to_string().as_bytes()].concat())
        .collect();
    let mut files: Vec<(&str, &[u8], bool)> = vec![("index.html", html.as_bytes(), true)];
    files.extend(
        names
            .iter()
            .zip(&bytes)
            .map(|(s, b)| (s.as_str(), &b[..], false)),
    );
    let got = run(&zip(&files)).unwrap();
    assert_eq!(summary(&got.fragment).images, MAX_IMAGES + 5);
    assert_eq!(got.lost, ["그림 2개(개수나 크기의 상한을 넘어 뺌)"]);
}

#[test]
fn 꾸러미에_html이_없거나_글이_없으면_거절한다() {
    let z = zip(&[("images/a.png", PNG, false)]);
    assert!(run(&z).is_err());
    assert_eq!(
        run(b"<html><body><script>x()</script></body></html>").err(),
        Some(EMPTY)
    );
    assert!(run(b"\xFF\xFE<p>\x00").is_err());
}

#[test]
fn 압축_폭탄_그림을_여러_번_가리켜도_한_번만_푼다() {
    let bomb = vec![0u8; zip::MAX_ENTRY + 1];
    // DOCX: 같은 큰 항목을 2,000번 가리킨다.
    let refs: String = (0..2000).map(|_| drawing("rIdB")).collect();
    let doc = format!(
        r#"<w:document {W}><w:body>{}</w:body></w:document>"#,
        p("", &(r("글") + &refs))
    );
    let rels =
        r#"<Relationships><Relationship Id="rIdB" Target="media/bomb.png"/></Relationships>"#;
    let z = zip(&[
        ("word/document.xml", doc.as_bytes(), true),
        ("word/_rels/document.xml.rels", rels.as_bytes(), true),
        ("word/media/bomb.png", &bomb, true),
    ]);
    let t = std::time::Instant::now();
    let got = run(&z).unwrap();
    assert!(t.elapsed().as_secs_f64() < 1.0, "{:?}", t.elapsed());
    assert_eq!(
        got.lost,
        ["그림 2000개(읽지 못했거나 PNG·JPEG가 아니라 뺌)"]
    );
    // 웹 페이지 꾸러미: 같은 큰 그림을 2,000번.
    let html = format!("<p>글{}</p>", "<img src=\"images/bomb.png\">".repeat(2000));
    let z = zip(&[
        ("index.html", html.as_bytes(), true),
        ("images/bomb.png", &bomb, true),
    ]);
    let t = std::time::Instant::now();
    let got = run(&z).unwrap();
    assert!(t.elapsed().as_secs_f64() < 1.0, "{:?}", t.elapsed());
    assert_eq!(
        got.lost,
        ["그림 2000개(읽지 못했거나 PNG·JPEG가 아니라 뺌)"]
    );
}

#[test]
fn 상한에_걸린_항목도_푼_합계에_든다() {
    let bomb = vec![0u8; zip::MAX_ENTRY + 1];
    // 네 번 상한까지 푼 뒤에는 합계(256MB)가 차서 작은 항목도 풀지 않는다.
    let z = zip(&[
        ("b0", &bomb, true),
        ("b1", &bomb, true),
        ("b2", &bomb, true),
        ("b3", &bomb, true),
        ("small", b"x", true),
    ]);
    let r = zip::Zip::open(&z).unwrap();
    for n in ["b0", "b1", "b2", "b3", "b0"] {
        assert_eq!(r.read(n).err(), Some(zip::TOO_BIG), "{n}");
    }
    assert_eq!(r.read("small").err(), Some(zip::TOO_BIG));
}

#[test]
fn docx_목록이_바꿔_쓴_시작_번호를_따른다() {
    let num = format!(
        r#"<w:numbering {W}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/></w:lvlOverride></w:num><w:num w:numId="3"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride></w:num></w:numbering>"#
    );
    let body = [
        list_p(1, 0, "가"),
        list_p(1, 0, "나"),
        list_p(2, 0, "다시 1"),
        list_p(3, 0, "5부터"),
    ]
    .concat();
    let doc = format!(r#"<w:document {W}><w:body>{body}</w:body></w:document>"#);
    let z = zip(&[
        ("word/document.xml", doc.as_bytes(), true),
        ("word/numbering.xml", num.as_bytes(), true),
    ]);
    let f = run(&z).unwrap().fragment;
    let starts: Vec<Value> = f
        .paras
        .iter()
        .map(|p| p.props["pf"]["list"]["start"].clone())
        .collect();
    assert_eq!(starts, [Value::Null, Value::Null, json!(1), json!(5)]);
}

#[test]
fn docx_아주_긴_글은_문단_상한에서_나누고_알린다() {
    let long = "가".repeat(300_000);
    let got = run(&docx(&p("", &r(&long)), &[])).unwrap();
    let f = &got.fragment;
    assert_eq!(f.paras.len(), 4);
    for para in &f.paras {
        let n = text_of(para).chars().count();
        assert!(n <= 90_000, "{n}");
    }
    let total: usize = f.paras.iter().map(|p| text_of(p).chars().count()).sum();
    assert_eq!(total, 300_000);
    assert_eq!(
        got.lost,
        [
            "글꼴 Calibri, 맑은 고딕(Pretendard로 바꿈)",
            "긴 문단 1개(한 문단의 글자 수 상한을 넘어 여러 문단으로 나눔)"
        ]
    );
}

#[test]
fn 앞부분에_pdf_표시가_든_꾸러미도_꾸러미로_읽는다() {
    let z = zip(&[("a.html", b"<p>%PDF-1.7 Kari</p>".as_slice(), false)]);
    let got = run(&z).unwrap();
    assert_eq!(got.pages, None);
    assert_eq!(text_of(&got.fragment.paras[0]), "%PDF-1.7 Kari");
}

#[test]
fn 너무_크거나_깊은_html은_그_까닭을_알린다() {
    let deep = "<div>".repeat(600) + "글";
    let e = run(deep.as_bytes()).err().unwrap();
    assert!(e.contains("너무 크거나 복잡"), "{e}");
    let big = format!("<p>{}</p>", "가".repeat(7 << 20));
    let e = run(big.as_bytes()).err().unwrap();
    assert!(e.contains("너무 크거나 복잡"), "{e}");
}

#[test]
fn 이름만_다른_별칭_항목_2000개가_한_내용을_가리키면_곧바로_거절한다() {
    use crate::zip::tests::{raw, truncated};
    let refs: String = (0..2000).map(|i| drawing(&format!("rId{i}"))).collect();
    let doc = format!(
        r#"<w:document {W}><w:body>{}</w:body></w:document>"#,
        p("", &(r("글") + &refs))
    );
    let rels: String = (0..2000)
        .map(|i| format!(r#"<Relationship Id="rId{i}" Target="media/i{i}.png"/>"#))
        .collect();
    let rels = format!("<Relationships>{rels}</Relationships>");
    let deflate = |b: &[u8]| miniz_oxide::deflate::compress_to_vec(b, 6);
    let locals = [
        ("word/media/i0.png", truncated(60 << 20), 60 << 20),
        ("word/document.xml", deflate(doc.as_bytes()), doc.len()),
        (
            "word/_rels/document.xml.rels",
            deflate(rels.as_bytes()),
            rels.len(),
        ),
    ];
    let mut entries: Vec<(String, usize)> = (0..2000)
        .map(|i| (format!("word/media/i{i}.png"), 0))
        .collect();
    entries.push(("word/document.xml".into(), 1));
    entries.push(("word/_rels/document.xml.rels".into(), 2));
    let z = raw(&locals, &entries);
    let t = std::time::Instant::now();
    assert_eq!(run(&z).err(), Some(zip::OVERLAP));
    assert!(t.elapsed().as_secs_f64() < 1.0, "{:?}", t.elapsed());
    // 별칭이 없으면(같은 깨진 그림 하나를 2,000번 가리키면) 한 번만 풀고 끝난다.
    let mut entries = vec![("word/media/i0.png".to_string(), 0)];
    entries.push(("word/document.xml".into(), 1));
    entries.push(("word/_rels/document.xml.rels".into(), 2));
    let rels: String = (0..2000)
        .map(|i| format!(r#"<Relationship Id="rId{i}" Target="media/i0.png"/>"#))
        .collect();
    let rels = format!("<Relationships>{rels}</Relationships>");
    let locals = [
        locals[0].clone(),
        locals[1].clone(),
        (
            "word/_rels/document.xml.rels",
            deflate(rels.as_bytes()),
            rels.len(),
        ),
    ];
    let z = raw(&locals, &entries);
    let t = std::time::Instant::now();
    let got = run(&z).unwrap();
    assert!(t.elapsed().as_secs_f64() < 1.0, "{:?}", t.elapsed());
    assert_eq!(
        got.lost,
        ["그림 2000개(읽지 못했거나 PNG·JPEG가 아니라 뺌)"]
    );
}
