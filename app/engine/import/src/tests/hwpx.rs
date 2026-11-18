//! 한글 문서(HWPX) 가져오기 시험. 견본 꾸러미는 시험 안에서 만든다(사람과 회사는 지어낸 것).

use kari_doc::serde_json::{json, Map, Value};

use super::*;

const HS: &str = r#"xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core""#;

const HEAD: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core"><hh:refList>
<hh:fontfaces><hh:fontface lang="HANGUL"><hh:font id="0" face="함초롬바탕"/><hh:font id="1" face="견본체"/></hh:fontface><hh:fontface lang="LATIN"><hh:font id="0" face="Times"/></hh:fontface></hh:fontfaces>
<hh:borderFills><hh:borderFill id="1"><hc:fillBrush><hc:winBrush faceColor="none" alpha="0"/></hc:fillBrush></hh:borderFill><hh:borderFill id="2"><hc:fillBrush><hc:winBrush faceColor="#E6E6E6" alpha="0"/></hc:fillBrush></hh:borderFill></hh:borderFills>
<hh:charProperties>
<hh:charPr id="0" height="1000" textColor="#000000"><hh:fontRef hangul="0" latin="0"/><hh:underline type="NONE"/><hh:strikeout shape="NONE"/></hh:charPr>
<hh:charPr id="1" height="1500" textColor="#FF0000"><hh:fontRef hangul="1" latin="0"/><hh:bold/><hh:italic/><hh:underline type="BOTTOM"/><hh:strikeout shape="CONTINUOUS"/></hh:charPr>
<hh:charPr id="2" height="800" textColor="#000000"><hh:fontRef hangul="0" latin="0"/><hh:supscript/></hh:charPr>
</hh:charProperties>
<hh:numberings><hh:numbering id="1"><hh:paraHead level="1" start="3" numFormat="DIGIT">^1.</hh:paraHead></hh:numbering></hh:numberings>
<hh:paraProperties>
<hh:paraPr id="0"><hh:align horizontal="JUSTIFY"/><hh:heading type="NONE" idRef="0" level="0"/></hh:paraPr>
<hh:paraPr id="1"><hh:align horizontal="CENTER"/><hh:heading type="NONE" idRef="0" level="0"/></hh:paraPr>
<hh:paraPr id="2"><hh:align horizontal="LEFT"/><hh:heading type="OUTLINE" idRef="0" level="0"/></hh:paraPr>
<hh:paraPr id="3"><hh:align horizontal="LEFT"/><hh:heading type="OUTLINE" idRef="0" level="1"/></hh:paraPr>
<hh:paraPr id="4"><hh:align horizontal="LEFT"/><hh:heading type="NUMBER" idRef="1" level="0"/></hh:paraPr>
<hh:paraPr id="5"><hh:align horizontal="RIGHT"/><hh:heading type="BULLET" idRef="1" level="1"/></hh:paraPr>
</hh:paraProperties></hh:refList></hh:head>"##;

const MANIFEST: &str = r#"<?xml version="1.0"?><opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest><opf:item id="header" href="Contents/header.xml" media-type="application/xml"/><opf:item id="image1" href="BinData/image1.png" media-type="image/png" isEmbeded="1"/><opf:item id="image2" href="BinData/image2.bmp" media-type="image/bmp" isEmbeded="1"/><opf:item id="gone" href="BinData/gone.png" media-type="image/png" isEmbeded="1"/></opf:manifest></opf:package>"#;

const CONTAINER: &str = r#"<?xml version="1.0"?><ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>"#;

const MIME: &[u8] = b"application/hwp+zip";

/// 구역 본문(`hs:sec` 안)으로 HWPX 꾸러미를 만든다. `more`는 같은 이름이면 바꿔 넣을 파일.
fn hwpx(body: &str, more: &[(&str, &[u8])]) -> Vec<u8> {
    let sec = format!(r#"<?xml version="1.0" encoding="UTF-8"?><hs:sec {HS}>{body}</hs:sec>"#);
    let mut files: Vec<(&str, Vec<u8>)> = vec![
        ("mimetype", MIME.to_vec()),
        ("META-INF/container.xml", CONTAINER.as_bytes().to_vec()),
        ("Contents/content.hpf", MANIFEST.as_bytes().to_vec()),
        ("Contents/header.xml", HEAD.as_bytes().to_vec()),
        ("Contents/section0.xml", sec.into_bytes()),
        ("BinData/image1.png", PNG.to_vec()),
        ("BinData/image2.bmp", b"BM-bytes".to_vec()),
    ];
    for (n, b) in more {
        match files.iter_mut().find(|(m, _)| m == n) {
            Some(f) => f.1 = b.to_vec(),
            None => files.push((n, b.to_vec())),
        }
    }
    let list: Vec<(&str, &[u8], bool)> = files
        .iter()
        .map(|(n, b)| (*n, &b[..], *n != "mimetype"))
        .collect();
    zip(&list)
}

/// 문단 하나: 문단 모양 번호와 (글자 모양 번호, 글) 들.
fn hp(para: u32, runs: &[(u32, &str)]) -> String {
    let rs: String = runs
        .iter()
        .map(|(c, t)| format!(r#"<hp:run charPrIDRef="{c}"><hp:t>{t}</hp:t></hp:run>"#))
        .collect();
    format!(r#"<hp:p id="0" paraPrIDRef="{para}" styleIDRef="0">{rs}<hp:linesegarray/></hp:p>"#)
}

/// 칸 안에 넣을 내용과 자리: (행, 열, 가로 합침, 세로 합침, 칸 바탕 번호, 제목 칸인가).
struct Cell(u32, u32, u32, u32, u32, u32);

fn tc(c: Cell, inner: &str) -> String {
    let Cell(r, col, cs, rs, fill, head) = c;
    format!(
        r#"<hp:tc name="" header="{head}" borderFillIDRef="{fill}"><hp:subList>{inner}</hp:subList><hp:cellAddr colAddr="{col}" rowAddr="{r}"/><hp:cellSpan colSpan="{cs}" rowSpan="{rs}"/><hp:cellSz width="{}" height="1000"/></hp:tc>"#,
        3600 * cs
    )
}

/// 글만 든 칸.
fn tt(r: u32, c: u32, cs: u32, rs: u32, fill: u32, head: u32, text: &str) -> String {
    tc(Cell(r, c, cs, rs, fill, head), &hp(0, &[(0, text)]))
}

/// 표 하나를 담은 문단(한글이 내는 꼴: 표가 `hp:run` 안에 있고 빈 `hp:t`가 뒤따른다).
fn tbl(rows: &[String]) -> String {
    let trs: String = rows.iter().map(|r| format!("<hp:tr>{r}</hp:tr>")).collect();
    format!(
        r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:tbl rowCnt="{}" colCnt="3">{trs}</hp:tbl><hp:t/></hp:run></hp:p>"#,
        rows.len()
    )
}

#[test]
fn 글자_모양과_문단_맞춤을_header의_번호로_찾아_옮긴다() {
    let body = [
        hp(0, &[(0, "견본상사 "), (1, "굵고 기울고"), (2, "2")]),
        hp(1, &[(0, "가운데")]),
    ]
    .concat();
    let got = run(&hwpx(&body, &[])).unwrap();
    let f = &got.fragment;
    let runs = &f.paras[0].runs;
    assert_eq!(runs[0], json!({"t": "견본상사 ", "s": {"size": 10.0}}));
    assert_eq!(
        runs[1],
        json!({"t": "굵고 기울고", "s": {"weight": 700, "italic": true, "underline": true,
            "strike": true, "color": "#FF0000FF", "size": 15.0}})
    );
    assert_eq!(
        runs[2],
        json!({"t": "2", "s": {"size": 8.0, "vertical": "SUPER"}})
    );
    assert_eq!(f.paras[0].props["pf"], json!({"align": "JUSTIFIED"}));
    assert_eq!(f.paras[1].props["pf"], json!({"align": "CENTER"}));
    assert_eq!(got.lost, ["글꼴 견본체, 함초롬바탕(Pretendard로 바꿈)"]);
}

#[test]
fn 개요_수준은_제목으로_번호와_글머리표는_목록으로_옮긴다() {
    let body = [
        hp(2, &[(0, "홍길동")]),
        hp(3, &[(0, "경력")]),
        hp(4, &[(0, "첫째")]),
        hp(4, &[(0, "둘째")]),
        hp(5, &[(0, "글머리")]),
        hp(0, &[(0, "본문")]),
    ]
    .concat();
    let f = run(&hwpx(&body, &[])).unwrap().fragment;
    assert_eq!(
        styles_of(&f),
        [Some("h1"), Some("h2"), None, None, None, None]
    );
    // 제목 안의 글자 크기는 문단 스타일에 맡긴다.
    assert_eq!(f.paras[0].runs, [json!({"t": "홍길동"})]);
    // 번호 목록: 시작 번호는 처음 한 번만 적는다.
    assert_eq!(
        f.paras[2].props["pf"]["list"],
        json!({"kind": "ORDERED", "level": 0, "start": 3})
    );
    assert_eq!(
        f.paras[3].props["pf"]["list"],
        json!({"kind": "ORDERED", "level": 0})
    );
    assert_eq!(
        f.paras[4].props["pf"],
        json!({"list": {"kind": "BULLET", "level": 1}, "align": "RIGHT"})
    );
    assert_eq!(
        summary(&f).headings,
        [(1, "홍길동".to_string()), (2, "경력".to_string())]
    );
}

#[test]
fn 탭_줄바꿈_공백과_여러_구역을_구역_번호_순서로_옮긴다() {
    let sec0 = r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>가<hp:tab width="4000"/>나<hp:lineBreak/>다<hp:nbSpace/>라&amp;&lt;</hp:t></hp:run></hp:p>"#;
    let sec = |t: &str| {
        format!(
            r#"<?xml version="1.0"?><hs:sec {HS}>{}</hs:sec>"#,
            hp(0, &[(0, t)])
        )
    };
    let (s1, s10) = (sec("둘째 구역"), sec("열한째 구역"));
    let z = hwpx(
        sec0,
        &[
            ("Contents/section10.xml", s10.as_bytes()),
            ("Contents/section1.xml", s1.as_bytes()),
        ],
    );
    let f = run(&z).unwrap().fragment;
    let all: Vec<String> = f.paras.iter().map(text_of).collect();
    assert_eq!(all, ["가\t나\n다 라&<", "둘째 구역", "열한째 구역"]);
}

#[test]
fn 합친_칸_제목_행_칸_바탕_열_폭이_있는_표를_옮긴다() {
    // 3×3: 첫 행은 제목 칸, (1,0)은 세로 2칸, (1,1)은 가로 2칸.
    let rows = [
        [
            tt(0, 0, 1, 1, 1, 1, "이름"),
            tt(0, 1, 1, 1, 1, 1, "회사"),
            tt(0, 2, 1, 1, 1, 1, "기간"),
        ]
        .concat(),
        [
            tt(1, 0, 1, 2, 2, 0, "홍길동"),
            tt(1, 1, 2, 1, 1, 0, "견본상사"),
        ]
        .concat(),
        [tt(2, 1, 1, 1, 1, 0, "백엔드"), tt(2, 2, 1, 1, 1, 0, "2024")].concat(),
    ];
    let z = hwpx(
        &[hp(0, &[(0, "앞")]), tbl(&rows), hp(0, &[(0, "뒤")])].concat(),
        &[],
    );
    let f = run(&z).unwrap().fragment;
    // 표만 있던 문단은 빈 문단을 남기지 않는다.
    assert_eq!(f.paras.len(), 3);
    let t = f.paras[1].table.as_ref().unwrap();
    assert_eq!(t.props["cols"], json!([36.0, 36.0, 36.0]));
    assert_eq!(t.props["headerRows"], json!(1));
    let cell = |r: usize, c: usize| t.rows[r].1[c].as_ref();
    assert_eq!(cell(1, 0).unwrap().0["span"], json!({"rows": 2, "cols": 1}));
    assert_eq!(cell(1, 0).unwrap().0["fill"], json!("#E6E6E6FF"));
    assert_eq!(text_of(&cell(1, 0).unwrap().1[0]), "홍길동");
    assert_eq!(cell(1, 1).unwrap().0["span"], json!({"rows": 1, "cols": 2}));
    // 합쳐져 덮인 칸은 `None`.
    assert!(cell(1, 2).is_none() && cell(2, 0).is_none());
    // 바탕이 없는("none") 칸은 `fill`이 없다.
    assert!(cell(0, 0).unwrap().0.get("fill").is_none());
    assert_eq!(text_of(&cell(2, 2).unwrap().1[0]), "2024");
    assert_eq!(summary(&f).tables, 1);
}

#[test]
fn 표_안의_표와_큰_표는_글로_풀어_쓴다() {
    let inner = tbl(&[tt(0, 0, 1, 1, 1, 0, "안1") + &tt(0, 1, 1, 1, 1, 0, "안2")]);
    let outer = tbl(&[tc(Cell(0, 0, 1, 1, 1, 0), &inner)]);
    let rows: Vec<String> = (0..51).map(|i| tt(i, 0, 1, 1, 1, 0, "줄")).collect();
    let got = run(&hwpx(&[outer, tbl(&rows)].concat(), &[])).unwrap();
    assert_eq!(
        got.lost,
        [
            "표 안의 표 1개(글로 풀어 씀)",
            "큰 표 1개(50행이나 20열을 넘어 글로 풀어 씀)"
        ]
    );
    let f = &got.fragment;
    // 안쪽 표는 칸 안에서 행마다 문단 하나(칸은 탭으로)가 되고, 큰 표는 행마다 문단 하나가 된다.
    let outer_cell = f.paras[0].table.as_ref().unwrap().rows[0].1[0]
        .as_ref()
        .unwrap();
    assert_eq!(text_of(&outer_cell.1[0]), "안1\t안2");
    assert_eq!(f.paras.iter().filter(|p| text_of(p) == "줄").count(), 51);
}

#[test]
fn 그림은_넣고_못_읽는_그림은_목록에_남긴다() {
    let pic = |id: &str| {
        format!(
            r#"<hp:run charPrIDRef="0"><hp:pic id="1"><hp:curSz width="7200" height="3600"/><hc:img binaryItemIDRef="{id}" bright="0"/><hp:sz width="1" height="1"/></hp:pic><hp:t/></hp:run>"#
        )
    };
    let body = format!(
        r#"<hp:p paraPrIDRef="0">{}{}{}{}</hp:p>"#,
        pic("image1"),
        pic("image2"),
        pic("gone"),
        pic("nothing")
    );
    let got = run(&hwpx(&body, &[])).unwrap();
    // 7200 HWPUNIT = 72pt, 3600 = 36pt.
    assert_eq!(
        got.fragment.paras[0].runs[0],
        json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": format!("h{}", PNG.len()), "w": 72.0, "h": 36.0}})
    );
    assert_eq!(summary(&got.fragment).images, 1);
    // BMP, 꾸러미에 없는 파일, 목록에 없는 id.
    assert_eq!(got.lost, ["그림 3개(읽지 못했거나 PNG·JPEG가 아니라 뺌)"]);
}

#[test]
fn 옮기지_못한_것은_센다() {
    let body = r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>본문</hp:t></hp:run>
<hp:run><hp:rect><hp:drawText><hp:subList><hp:p><hp:run><hp:t>상자</hp:t></hp:run></hp:p></hp:subList></hp:drawText></hp:rect></hp:run>
<hp:run><hp:ellipse/></hp:run><hp:run><hp:chart/></hp:run>
<hp:run><hp:equation><hp:script>x</hp:script></hp:equation></hp:run>
<hp:run><hp:ctrl><hp:footNote><hp:subList><hp:p><hp:run><hp:t>각주</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl></hp:run>
<hp:run><hp:ctrl><hp:header><hp:subList><hp:p><hp:run><hp:t>홍길동 · 010-0000-0000</hp:t></hp:run></hp:p></hp:subList></hp:header></hp:ctrl></hp:run>
<hp:run><hp:ctrl><hp:colPr/></hp:ctrl></hp:run></hp:p>"#;
    let got = run(&hwpx(body, &[])).unwrap();
    assert_eq!(text_of(&got.fragment.paras[0]), "본문");
    assert_eq!(
        got.lost,
        [
            "글꼴 함초롬바탕(Pretendard로 바꿈)",
            "글 상자 1개(뺌)",
            "도형·차트 2개(뺌)",
            "각주·미주 1개(뺌)",
            "수식 1개(뺌)",
            "머리글·바닥글(뺌)",
        ]
    );
}

#[test]
fn 옛_한글_이진_파일은_읽지_않고_안내한다() {
    // 한글 5.0의 머리 흐름 이름만 있고 뒤가 잘린 OLE 파일도 깨짐 오류가 아니라 옛 한글 안내다.
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.extend_from_slice(&[0; 600]);
    ole.extend("FileHeader".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(run(&ole).err(), Some(HWP_LEGACY));
    assert_eq!(run(b"HWP Document File V3.00 \x1a").err(), Some(HWP_LEGACY));
    assert!(HWP_LEGACY.contains("HWPX나 PDF로 저장"));
}

#[test]
fn 흐름이_없는_ole_파일은_한글이나_word로_안내하지_않는다() {
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.extend_from_slice(&[0; 600]);
    assert_eq!(run(&ole).err(), Some(UNREADABLE));
    assert_eq!(run(&ole[..8]).err(), Some(UNREADABLE));
}

#[test]
fn 깨진_꾸러미_밖을_가리키는_경로_압축_폭탄은_거절한다() {
    assert!(run(b"PK\x03\x04hwpx broken").is_err());
    // 꾸러미 밖을 가리키는 항목이 하나라도 있으면 통째로 거절한다.
    let z = zip(&[
        ("mimetype", MIME, false),
        ("Contents/section0.xml", b"<hs:sec/>", true),
        ("../evil.xml", b"x", false),
    ]);
    assert_eq!(run(&z).err(), Some(zip::UNSAFE_PATH));
    // 그림 목록이 꾸러미 밖을 가리켜도 읽지 않고 빠진 그림으로 센다.
    let manifest = r#"<opf:package xmlns:opf="x"><opf:manifest><opf:item id="s" href="../../secret.png"/></opf:manifest></opf:package>"#;
    let pic = r#"<hp:p><hp:run><hp:pic><hc:img binaryItemIDRef="s"/></hp:pic></hp:run></hp:p>"#;
    let z = hwpx(pic, &[("Contents/content.hpf", manifest.as_bytes())]);
    assert_eq!(run(&z).err(), Some(EMPTY));
    // 풀면 상한을 넘는 구역 파일.
    let bomb = vec![b' '; zip::MAX_ENTRY + 10];
    let z = zip(&[
        ("mimetype", MIME, false),
        ("Contents/section0.xml", &bomb, true),
    ]);
    assert_eq!(run(&z).err(), Some(zip::TOO_BIG));
    // 구역 파일이 없는 HWPX, 깨진 XML.
    let z = zip(&[("mimetype", MIME, false)]);
    assert_eq!(run(&z).err(), Some("한글 문서(HWPX)를 읽지 못했습니다."));
    let z = zip(&[
        ("mimetype", MIME, false),
        ("Contents/section0.xml", b"<hs:sec><hp:p></hs:sec>", true),
    ]);
    assert!(run(&z).is_err());
}

#[test]
fn 밖의_엔티티를_펼치지_않는다() {
    let sec = format!(
        r#"<?xml version="1.0"?><!DOCTYPE hs:sec [<!ENTITY x SYSTEM "file:///etc/hosts">]><hs:sec {HS}>{}</hs:sec>"#,
        hp(0, &[(0, "&x;홍길동")])
    );
    let z = zip(&[
        ("mimetype", MIME, false),
        ("Contents/section0.xml", sec.as_bytes(), true),
    ]);
    assert_eq!(text_of(&run(&z).unwrap().fragment.paras[0]), "&x;홍길동");
}

#[test]
fn 문단이_상한을_넘으면_거절한다() {
    let body: String = (0..MAX_PARAS + 1).map(|_| hp(0, &[(0, "줄")])).collect();
    assert_eq!(run(&hwpx(&body, &[])).err(), Some(TOO_MANY_PARAS));
}

#[test]
fn 겹치는_칸이나_상한을_넘는_열은_글로_풀어_쓴다() {
    // 같은 자리를 두 칸이 차지한다.
    let rows = [tt(0, 0, 2, 1, 1, 0, "가") + &tt(0, 1, 1, 1, 1, 0, "나")];
    let got = run(&hwpx(&tbl(&rows), &[])).unwrap();
    assert_eq!(text_of(&got.fragment.paras[0]), "가\t나");
    // 열 주소가 20을 넘는다.
    let rows = [tt(0, 25, 1, 1, 1, 0, "멀리")];
    let got = run(&hwpx(&tbl(&rows), &[])).unwrap();
    assert_eq!(text_of(&got.fragment.paras[0]), "멀리");
    assert_eq!(got.lost, ["큰 표 1개(50행이나 20열을 넘어 글로 풀어 씀)"]);
}

#[test]
fn 하이퍼링크는_허용한_주소만_링크로_옮기고_나머지는_글로_두고_센다() {
    let field = |id: u32, cmd: &str| {
        format!(
            r#"<hp:run><hp:ctrl><hp:fieldBegin id="{id}" type="HYPERLINK"><hp:parameters cnt="2"><hp:integerParam name="Prop">0</hp:integerParam><hp:stringParam name="Command">{cmd}</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl></hp:run>"#
        )
    };
    let end = |id: u32| {
        format!(r#"<hp:run><hp:ctrl><hp:fieldEnd beginIDRef="{id}"/></hp:ctrl></hp:run>"#)
    };
    let run_of = |t: &str| format!(r#"<hp:run charPrIDRef="0"><hp:t>{t}</hp:t></hp:run>"#);
    let body = format!(
        r#"<hp:p paraPrIDRef="0">{}{}{}{}{}{}{}{}{}{}{}</hp:p>"#,
        run_of("앞 "),
        field(1, r"https\://example.com/hong\;a;1;0;0;"),
        run_of("포트폴리오"),
        end(1),
        run_of(" 뒤 "),
        field(2, "javascript\\:alert(1);1;0;0;"),
        run_of("나쁜 링크"),
        end(2),
        field(3, "#책갈피;1;0;0;"),
        run_of("안쪽 링크"),
        end(3),
    );
    let got = run(&hwpx(&body, &[])).unwrap();
    let links: Vec<_> = got.fragment.paras[0]
        .runs
        .iter()
        .map(|r| (r["t"].as_str().unwrap(), r.get("link").cloned()))
        .collect();
    // 글자 모양이 같은 이어진 글은 한 run으로 합쳐진다.
    assert_eq!(
        links,
        [
            ("앞 ", None),
            ("포트폴리오", Some(json!("https://example.com/hong;a"))),
            (" 뒤 나쁜 링크안쪽 링크", None),
        ]
    );
    assert!(got
        .lost
        .contains(&"링크 2개(주소를 쓸 수 없어 글만 둠)".to_string()));
}

/// 그림 `image1`을 담은 문단.
fn pic_p() -> String {
    r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:pic><hp:curSz width="7200" height="3600"/><hc:img binaryItemIDRef="image1"/></hp:pic><hp:t/></hp:run></hp:p>"#.to_string()
}

#[test]
fn 버리는_표의_칸_안_그림은_저장하지_않고_글꼴도_세지_않는다() {
    // 첫 칸에 그림과 글이 있고, 둘째 칸이 열 상한을 넘는다 / 둘째 칸이 첫 칸과 겹친다.
    let cases = [
        [
            tc(Cell(0, 0, 1, 1, 1, 0), &(pic_p() + &hp(0, &[(0, "글")]))),
            tt(0, 25, 1, 1, 1, 0, "멀리"),
        ],
        [
            tc(Cell(0, 0, 2, 1, 1, 0), &(pic_p() + &hp(0, &[(0, "글")]))),
            tt(0, 1, 1, 1, 1, 0, "겹침"),
        ],
    ];
    for rows in cases {
        let z = hwpx(&tbl(&[rows.concat()]), &[]);
        let mut calls = 0;
        let got = import(&z, BODY, &mut |b| {
            calls += 1;
            store(b)
        })
        .unwrap();
        assert_eq!(calls, 0, "버린 표의 그림을 저장했다");
        assert_eq!(got.lost, ["큰 표 1개(50행이나 20열을 넘어 글로 풀어 씀)"]);
        assert_eq!(summary(&got.fragment).images, 0);
    }
}

#[test]
fn 깊이_겹친_표의_글은_한_번만_쌓고_상한에서_멈춘다() {
    let mut inner = hp(0, &[(0, "L40")]);
    for k in (1..40).rev() {
        inner = hp(0, &[(0, &format!("L{k}"))]) + &tbl(&[tc(Cell(0, 0, 1, 1, 1, 0), &inner)]);
    }
    let z = hwpx(&tbl(&[tc(Cell(0, 0, 1, 1, 1, 0), &inner)]), &[]);
    let f = run(&z).unwrap().fragment;
    let cell = f.paras[0].table.as_ref().unwrap().rows[0].1[0]
        .as_ref()
        .unwrap();
    let text: Vec<String> = cell.1.iter().map(text_of).collect();
    let want: Vec<String> = (2..=40).map(|k| format!("L{k}")).collect();
    assert_eq!(text, ["L1".to_string(), want.join(" ")]);
    // 글이 상한(9만 자)보다 길면 모으는 동안 멈춘다.
    let long = hp(0, &[(0, &"가".repeat(200_000))]);
    let inner = tbl(&[tc(Cell(0, 0, 1, 1, 1, 0), &long)]);
    let z = hwpx(&tbl(&[tc(Cell(0, 0, 1, 1, 1, 0), &inner)]), &[]);
    let f = run(&z).unwrap().fragment;
    let cell = f.paras[0].table.as_ref().unwrap().rows[0].1[0]
        .as_ref()
        .unwrap();
    assert_eq!(text_of(&cell.1[0]).chars().count(), 90_000);
}

#[test]
fn ole_복합_문서는_안의_이름으로_까닭을_가른다() {
    let ole = |name: &str| {
        let mut v = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        v.extend_from_slice(&[0; 300]);
        v.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
        v.extend_from_slice(&[0; 300]);
        v
    };
    assert_eq!(run(&ole("EncryptedPackage")).err(), Some(ENCRYPTED));
    assert_eq!(run(&ole("WordDocument")).err(), Some(OLD_WORD));
    assert_eq!(run(&ole("FileHeader")).err(), Some(HWP_LEGACY));
    // 한글 파일의 머리 흐름이 있으면, 미리보기 글이나 안에 넣은 개체에 Word 이름이 있어도 한글로 본다.
    let mut hwp = ole("WordDocument");
    hwp.extend_from_slice(b"HWP Document File");
    assert_eq!(run(&hwp).err(), Some(HWP_LEGACY));
    assert!(ENCRYPTED.contains("암호"));
    assert!(OLD_WORD.contains(".docx로 저장"));
}

/// 칸 하나(주소와 합침은 글 그대로 넣어 0, 음수, 큰 수를 쓸 수 있다).
fn tc_raw(r: &str, c: &str, cs: &str, rs: &str, text: &str) -> String {
    format!(
        r#"<hp:tc borderFillIDRef="1"><hp:subList>{}</hp:subList><hp:cellAddr colAddr="{c}" rowAddr="{r}"/><hp:cellSpan colSpan="{cs}" rowSpan="{rs}"/><hp:cellSz width="3600" height="1000"/></hp:tc>"#,
        hp(0, &[(0, text)])
    )
}

/// 문서에 넣을 수 있는 조각인가: 글 run에 제어 문자(탭·줄바꿈 밖)와 U+FFFC가 없고, 개체 run의 글은 U+FFFC 하나다.
/// 표는 50행 안이고, 행마다 칸 수가 열 수와 같고, 합친 칸이 표 안에서 겹치지 않고 모든 자리를 한 번씩 덮는다.
fn check_frag(paras: &[FragPara]) {
    for p in paras {
        for r in &p.runs {
            let t = r["t"].as_str().unwrap();
            if r.get("o").is_some() {
                assert_eq!(t, "\u{FFFC}");
            } else {
                assert!(
                    !t.chars()
                        .any(|c| c == '\u{FFFC}' || (c.is_control() && c != '\t' && c != '\n')),
                    "{t:?}"
                );
            }
        }
        let Some(t) = &p.table else { continue };
        let ncols = t.props["cols"].as_array().unwrap().len();
        let nrows = t.rows.len();
        assert!(nrows <= 50 && ncols <= 20);
        let mut seen = vec![vec![false; ncols]; nrows];
        for (r, (_, cells)) in t.rows.iter().enumerate() {
            assert_eq!(cells.len(), ncols, "행 {r}의 칸 수");
            for (c, cell) in cells.iter().enumerate() {
                let Some((props, inner)) = cell else { continue };
                check_frag(inner);
                let span = |k: &str| {
                    props
                        .get("span")
                        .map_or(1, |s| s[k].as_u64().unwrap() as usize)
                };
                for row in seen.iter_mut().skip(r).take(span("rows")) {
                    for slot in row.iter_mut().skip(c).take(span("cols")) {
                        assert!(!*slot, "겹친 칸");
                        *slot = true;
                    }
                }
                assert!(r + span("rows") <= nrows && c + span("cols") <= ncols);
            }
        }
        assert!(seen.iter().flatten().all(|s| *s), "덮이지 않은 자리");
    }
}

fn fp(runs: Value, props: Value) -> FragPara {
    FragPara {
        runs: runs.as_array().unwrap().clone(),
        props: props.as_object().unwrap().clone(),
        table: None,
    }
}

#[test]
fn 견본_명세의_모든_항목이_문서_모델과_그대로_같다() {
    let link = r#"<hp:run><hp:ctrl><hp:fieldBegin id="1" type="HYPERLINK"><hp:parameters><hp:stringParam name="Command">https\://example.com/hong;1;0;0;</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl></hp:run><hp:run charPrIDRef="0"><hp:t>견본</hp:t></hp:run><hp:run><hp:ctrl><hp:fieldEnd beginIDRef="1"/></hp:ctrl></hp:run>"#;
    let table = tbl(&[
        tt(0, 0, 1, 1, 1, 1, "이름") + &tt(0, 1, 1, 1, 1, 1, "회사"),
        tt(1, 0, 2, 1, 2, 0, "홍길동"),
    ]);
    let body = [
        hp(2, &[(1, "홍길동")]),
        r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>견본상사<hp:tab/>백엔드<hp:lineBreak/>2024</hp:t></hp:run><hp:run charPrIDRef="1"><hp:t>굵게</hp:t></hp:run><hp:run charPrIDRef="2"><hp:t>2</hp:t></hp:run></hp:p>"#.to_string(),
        hp(4, &[(0, "첫째")]),
        hp(5, &[(0, "글머리")]),
        format!(r#"<hp:p paraPrIDRef="1"><hp:run charPrIDRef="0"><hp:t>누리집 </hp:t></hp:run>{link}<hp:run charPrIDRef="0"><hp:pic><hp:curSz width="7200" height="3600"/><hc:img binaryItemIDRef="image1"/></hp:pic></hp:run></hp:p>"#),
        table,
    ]
    .concat();
    let got = run(&hwpx(&body, &[])).unwrap();
    let size10 = json!({"size": 10.0});
    let cell_p = |t: &str| {
        fp(
            json!([{"t": t, "s": size10}]),
            json!({"pf": {"align": "JUSTIFIED"}}),
        )
    };
    let mut fill = Map::new();
    fill.insert("fill".into(), json!("#E6E6E6FF"));
    fill.insert("span".into(), json!({"rows": 1, "cols": 2}));
    let want = vec![
        // 제목 안의 굵기와 크기는 문단 스타일에 맡긴다.
        fp(
            json!([{"t": "홍길동", "s": {"italic": true, "underline": true, "strike": true, "color": "#FF0000FF"}}]),
            json!({"style": "h1"}),
        ),
        fp(
            json!([
                {"t": "견본상사\t백엔드\n2024", "s": size10},
                {"t": "굵게", "s": {"weight": 700, "italic": true, "underline": true, "strike": true, "color": "#FF0000FF", "size": 15.0}},
                {"t": "2", "s": {"size": 8.0, "vertical": "SUPER"}},
            ]),
            json!({"pf": {"align": "JUSTIFIED"}}),
        ),
        fp(
            json!([{"t": "첫째", "s": size10}]),
            json!({"pf": {"list": {"kind": "ORDERED", "level": 0, "start": 3}}}),
        ),
        fp(
            json!([{"t": "글머리", "s": size10}]),
            json!({"pf": {"list": {"kind": "BULLET", "level": 1}, "align": "RIGHT"}}),
        ),
        fp(
            json!([
                {"t": "누리집 ", "s": size10},
                {"t": "견본", "s": size10, "link": "https://example.com/hong"},
                {"t": "\u{FFFC}", "o": {"type": "image", "asset": format!("h{}", PNG.len()), "w": 72.0, "h": 36.0}},
            ]),
            json!({"pf": {"align": "CENTER"}}),
        ),
        FragPara {
            runs: Vec::new(),
            props: Map::new(),
            table: Some(kari_edit::FragTable {
                whole: true,
                props: json!({"cols": [36.0, 36.0], "headerRows": 1})
                    .as_object()
                    .unwrap()
                    .clone(),
                rows: vec![
                    (
                        Map::new(),
                        vec![
                            Some((Map::new(), vec![cell_p("이름")])),
                            Some((Map::new(), vec![cell_p("회사")])),
                        ],
                    ),
                    (Map::new(), vec![Some((fill, vec![cell_p("홍길동")])), None]),
                ],
            }),
        },
    ];
    assert_eq!(got.fragment.paras, want);
    assert_eq!(got.lost, ["글꼴 견본체, 함초롬바탕(Pretendard로 바꿈)"]);
    check_frag(&got.fragment.paras);
}

#[test]
fn mimetype이_줄었거나_뒤에_있거나_틀려도_구역_파일로_알아본다() {
    let sec = format!(r#"<hs:sec {HS}>{}</hs:sec>"#, hp(0, &[(0, "본문")]));
    let (sec, head) = (sec.as_bytes(), HEAD.as_bytes());
    let cases: [Vec<(&str, &[u8], bool)>; 4] = [
        vec![
            ("mimetype", MIME, true),
            ("Contents/header.xml", head, true),
            ("Contents/section0.xml", sec, true),
        ],
        vec![
            ("Contents/section0.xml", sec, true),
            ("Contents/header.xml", head, true),
            ("mimetype", MIME, false),
        ],
        vec![
            ("mimetype", b"application/zip", false),
            ("Contents/section0.xml", sec, true),
        ],
        vec![("Contents/section0.xml", sec, true)],
    ];
    for files in cases {
        let got = run(&zip(&files)).unwrap();
        assert_eq!(text_of(&got.fragment.paras[0]), "본문");
    }
    // 표지도 구역 파일도 없으면 웹 페이지 꾸러미로 보고 거절한다(멈추지 않는다).
    let z = zip(&[
        ("mimetype", b"application/x", false),
        ("Contents/other.xml", sec, true),
    ]);
    assert!(run(&z).is_err());
}

#[test]
fn word_표지와_한글_표지가_함께_있으면_word로_읽고_웹_꾸러미는_한글로_보지_않는다() {
    let sec = format!(r#"<hs:sec {HS}>{}</hs:sec>"#, hp(0, &[(0, "한글 쪽")]));
    let z = docx(
        &p("", &r("Word 쪽")),
        &[
            ("mimetype", MIME),
            ("Contents/section0.xml", sec.as_bytes()),
        ],
    );
    assert_eq!(text_of(&run(&z).unwrap().fragment.paras[0]), "Word 쪽");
    let z = zip(&[
        ("index.html", "<p>웹 쪽</p>".as_bytes(), true),
        ("Contents/header.xml", HEAD.as_bytes(), true),
    ]);
    assert_eq!(text_of(&run(&z).unwrap().fragment.paras[0]), "웹 쪽");
}

#[test]
fn 엔티티_폭탄과_깊은_겹침은_펼치지_않거나_거절한다() {
    let mut dtd = String::from(r#"<!ENTITY a0 "가가가가가가가가가가">"#);
    for i in 1..10 {
        let refs = format!("&a{};", i - 1).repeat(10);
        dtd += &format!(r#"<!ENTITY a{i} "{refs}">"#);
    }
    let sec = format!(
        r#"<?xml version="1.0"?><!DOCTYPE hs:sec [{dtd}]><hs:sec {HS}>{}</hs:sec>"#,
        hp(0, &[(0, "&a9;")])
    );
    let z = hwpx("", &[("Contents/section0.xml", sec.as_bytes())]);
    assert_eq!(text_of(&run(&z).unwrap().fragment.paras[0]), "&a9;");
    let deep = "<hp:x>".repeat(300) + &"</hp:x>".repeat(300);
    assert_eq!(
        run(&hwpx(&deep, &[])).err(),
        Some("문서가 너무 크거나 깊어 가져오지 않았습니다.")
    );
}

#[test]
fn 없는_모양을_가리키면_기본_모양으로_두고_센다() {
    let body = [
        r#"<hp:p paraPrIDRef="77"><hp:run charPrIDRef="99"><hp:t>가</hp:t></hp:run><hp:run charPrIDRef="-1"><hp:t>나</hp:t></hp:run></hp:p>"#,
        r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="184467440737095516150"><hp:t>다</hp:t></hp:run></hp:p>"#,
    ]
    .concat();
    let got = run(&hwpx(&body, &[])).unwrap();
    assert_eq!(got.fragment.paras[0].runs, [json!({"t": "가나"})]);
    assert!(got.fragment.paras[0].props.is_empty());
    assert_eq!(got.fragment.paras[1].runs, [json!({"t": "다"})]);
    assert_eq!(
        got.lost,
        ["글자·문단 모양 4곳(가리킨 모양이 없어 기본 모양으로 둠)"]
    );
}

#[test]
fn 이상한_합침과_거짓_행_수는_고치거나_글로_풀고_표는_늘_곧다() {
    // 0과 음수의 합침은 1로 본다.
    let rows = [tc_raw("0", "0", "0", "-1", "가") + &tc_raw("0", "1", "1", "1", "나")];
    let f = run(&hwpx(&tbl(&rows), &[])).unwrap().fragment;
    let t = f.paras[0].table.as_ref().unwrap();
    assert_eq!(t.props["cols"], json!([36.0, 36.0]));
    assert!(t.rows[0]
        .1
        .iter()
        .all(|c| c.as_ref().unwrap().0.get("span").is_none()));
    check_frag(&f.paras);
    // 표 밖으로 넘는 세로 합침: 실제 칸으로 행을 늘리고 빈 칸을 채운다.
    let rows = [tc_raw("0", "0", "1", "3", "가") + &tc_raw("0", "1", "1", "1", "나")];
    let f = run(&hwpx(&tbl(&rows), &[])).unwrap().fragment;
    assert_eq!(f.paras[0].table.as_ref().unwrap().rows.len(), 3);
    check_frag(&f.paras);
    // 상한을 넘는 합침과 서로 겹치는 합침은 글로 풀고 알린다.
    for rows in [
        [tc_raw("0", "0", "30", "1", "가")],
        [tc_raw("0", "0", "2", "2", "가") + &tc_raw("1", "1", "1", "1", "나")],
    ] {
        let got = run(&hwpx(&tbl(&rows), &[])).unwrap();
        assert!(got.fragment.paras.iter().all(|p| p.table.is_none()));
        assert_eq!(got.lost, ["큰 표 1개(50행이나 20열을 넘어 글로 풀어 씀)"]);
    }
    // 적힌 행 수(rowCnt)는 믿지 않는다: 실제 행이 100개라 글로 푼다.
    let trs: String = (0..100)
        .map(|i| {
            format!(
                "<hp:tr>{}</hp:tr>",
                tc_raw(&i.to_string(), "0", "1", "1", "줄")
            )
        })
        .collect();
    let body = format!(
        r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:tbl rowCnt="2" colCnt="1">{trs}</hp:tbl></hp:run></hp:p>"#
    );
    let got = run(&hwpx(&body, &[])).unwrap();
    assert_eq!(got.fragment.paras.len(), 100);
    assert_eq!(got.lost, ["큰 표 1개(50행이나 20열을 넘어 글로 풀어 씀)"]);
}

#[test]
fn 그림_이름이_꾸러미_밖이거나_겹치면_거절하고_같은_그림은_한_번만_푼다() {
    let pic = |id: &str| {
        format!(
            r#"<hp:run charPrIDRef="0"><hp:pic><hc:img binaryItemIDRef="{id}"/></hp:pic></hp:run>"#
        )
    };
    let sec = format!(
        r#"<hs:sec {HS}><hp:p paraPrIDRef="0">{}{}</hp:p></hs:sec>"#,
        pic("image1"),
        pic("image1")
    );
    let z = zip(&[
        ("mimetype", MIME, false),
        ("Contents/section0.xml", sec.as_bytes(), true),
        ("BinData/../x.png", PNG, false),
    ]);
    assert_eq!(run(&z).err(), Some(zip::UNSAFE_PATH));
    let z = zip(&[
        ("mimetype", MIME, false),
        ("Contents/section0.xml", sec.as_bytes(), true),
        ("BinData/image1.png", PNG, false),
        ("BinData/image1.png", b"other", false),
    ]);
    assert_eq!(run(&z).err(), Some(zip::OVERLAP));
    let mut calls = 0;
    let got = import(
        &hwpx(&sec[sec.find('>').unwrap() + 1..sec.len() - 9], &[]),
        BODY,
        &mut |b| {
            calls += 1;
            store(b)
        },
    )
    .unwrap();
    assert_eq!((calls, summary(&got.fragment).images), (1, 2));
    // PNG라는 이름에 다른 바이트가 들어 있으면 받는 쪽이 내용으로 거절하고 센다.
    let got = run(&hwpx(
        &pic_p(),
        &[("BinData/image1.png", b"\xFF\xD8\xFF broken")],
    ))
    .unwrap();
    assert_eq!(summary(&got.fragment).images, 0);
    assert_eq!(got.lost, ["그림 1개(읽지 못했거나 PNG·JPEG가 아니라 뺌)"]);
}

#[test]
fn 쓸_수_없는_주소의_링크는_글만_두고_센다() {
    let field = |cmd: &str| {
        format!(
            r#"<hp:run><hp:ctrl><hp:fieldBegin type="HYPERLINK"><hp:parameters><hp:stringParam name="Command">{cmd}</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl></hp:run><hp:run charPrIDRef="0"><hp:t>글</hp:t></hp:run><hp:run><hp:ctrl><hp:fieldEnd/></hp:ctrl></hp:run>"#
        )
    };
    let long = format!("https\\://example.com/{};1;0;0;", "a".repeat(40_000));
    let body = format!(
        r#"<hp:p paraPrIDRef="0">{}{}{}</hp:p>"#,
        field("javascript\\:alert(1);1;0;0;"),
        field("file\\:///etc/hosts;1;0;0;"),
        field(&long)
    );
    let got = run(&hwpx(&body, &[])).unwrap();
    assert_eq!(text_of(&got.fragment.paras[0]), "글글글");
    assert!(got.fragment.paras[0]
        .runs
        .iter()
        .all(|r| r.get("link").is_none()));
    assert!(got
        .lost
        .contains(&"링크 3개(주소를 쓸 수 없어 글만 둠)".to_string()));
}

#[test]
fn 제어_문자와_개체_자리_글자와_서로게이트_참조는_지우고_센다() {
    let body = hp(0, &[(0, "가\u{0}\u{1}\u{1F}\r나\u{FFFC}&#xD800;&#1;다")]);
    let got = run(&hwpx(&body, &[])).unwrap();
    assert_eq!(text_of(&got.fragment.paras[0]), "가나다");
    assert!(
        got.lost
            .contains(&"넣을 수 없는 글자 7자(제어 문자 등이라 뺌)".to_string()),
        "{:?}",
        got.lost
    );
    check_frag(&got.fragment.paras);
    // 표를 글로 풀 때도 넣을 수 없는 글자는 뺀다.
    let rows = [tc_raw("0", "0", "30", "1", "가\u{FFFC}\u{1}나")];
    let got = run(&hwpx(&tbl(&rows), &[])).unwrap();
    assert_eq!(text_of(&got.fragment.paras[0]), "가나");
    check_frag(&got.fragment.paras);
}

#[test]
fn utf_16_구역은_읽고_utf_8이나_16이_아니면_거절한다() {
    let sec = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?><hs:sec {HS}>{}</hs:sec>"#,
        hp(0, &[(0, "한글")])
    );
    let le: Vec<u8> = sec.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let be: Vec<u8> = sec.encode_utf16().flat_map(u16::to_be_bytes).collect();
    for bytes in [
        [&[0xFF, 0xFE][..], &le].concat(),
        le.clone(),
        [&[0xFE, 0xFF][..], &be].concat(),
        be,
    ] {
        let got = run(&hwpx("", &[("Contents/section0.xml", &bytes)])).unwrap();
        assert_eq!(text_of(&got.fragment.paras[0]), "한글");
    }
    // 선언은 EUC-KR이지만 실제로 UTF-8이면 읽고, EUC-KR 바이트면 거절한다.
    let sec = format!(
        r#"<?xml version="1.0" encoding="EUC-KR"?><hs:sec {HS}>{}</hs:sec>"#,
        hp(0, &[(0, "한글")])
    );
    assert!(run(&hwpx("", &[("Contents/section0.xml", sec.as_bytes())])).is_ok());
    let euc = sec
        .replace("한글", "\u{0}")
        .replace('\u{0}', "")
        .into_bytes();
    let euc = [
        &euc[..euc.len() - 40],
        &[0xC7, 0xD1, 0xB1, 0xDB],
        &euc[euc.len() - 40..],
    ]
    .concat();
    assert_eq!(
        run(&hwpx("", &[("Contents/section0.xml", &euc)])).err(),
        Some(xml::BAD_XML)
    );
}

/// 꾸러미의 가운데 목록에서 `name` 항목에 암호 표시(깃발 0번 비트)를 켠다.
fn set_encrypted(z: &mut [u8], name: &str) {
    let at = (0..z.len() - 46)
        .find(|&i| z[i..].starts_with(b"PK\x01\x02") && z[i + 46..].starts_with(name.as_bytes()))
        .unwrap();
    z[at + 8] |= 1;
}

#[test]
fn 암호가_걸린_한글_문서는_암호_안내를_한다() {
    let body = hp(0, &[(0, "본문")]);
    let mut z = hwpx(&body, &[]);
    set_encrypted(&mut z, "Contents/section0.xml");
    assert_eq!(run(&z).err(), Some(ENCRYPTED));
    let manifest = r#"<odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><odf:file-entry odf:full-path="Contents/section0.xml"><odf:encryption-data odf:checksum-type="SHA1"/></odf:file-entry></odf:manifest>"#;
    let z = hwpx(&body, &[("META-INF/manifest.xml", manifest.as_bytes())]);
    assert_eq!(run(&z).err(), Some(ENCRYPTED));
    let z = hwpx(
        &body,
        &[("Contents/section0.xml", &[0x8A, 0x13, 0x5C, 0x00, 0x91])],
    );
    assert_eq!(run(&z).err(), Some(ENCRYPTED));
}

#[test]
fn 구역이_천_개를_넘으면_거절한다() {
    let sec = format!(r#"<hs:sec {HS}>{}</hs:sec>"#, hp(0, &[(0, "쪽")]));
    let names: Vec<String> = (1..=1000)
        .map(|i| format!("Contents/section{i}.xml"))
        .collect();
    let more: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), sec.as_bytes())).collect();
    assert_eq!(
        run(&hwpx(&hp(0, &[(0, "쪽")]), &more)).err(),
        Some("한글 문서의 구역이 1,000개를 넘어 가져오지 않았습니다.")
    );
    let got = run(&hwpx(&hp(0, &[(0, "쪽")]), &more[..999])).unwrap();
    assert_eq!(got.fragment.paras.len(), 1000);
}

#[test]
fn 아주_긴_문단은_9만_자에서_나누고_알린다() {
    let got = run(&hwpx(&hp(0, &[(0, &"가".repeat(100_000))]), &[])).unwrap();
    let lens: Vec<usize> = got
        .fragment
        .paras
        .iter()
        .map(|p| text_of(p).chars().count())
        .collect();
    assert_eq!(lens, [90_000, 10_000]);
    assert!(got
        .lost
        .contains(&"긴 문단 1개(한 문단의 글자 수 상한을 넘어 여러 문단으로 나눔)".to_string()));
}

#[test]
fn 옮기지_못한_메모_ole_개체_양식은_종류대로_센다() {
    let body = r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>본문</hp:t></hp:run><hp:run><hp:ctrl><hp:fieldBegin type="MEMO"/></hp:ctrl></hp:run><hp:run><hp:ctrl><hp:fieldEnd/></hp:ctrl></hp:run><hp:run><hp:ole/><hp:video/><hp:checkBtn/><hp:textart/></hp:run></hp:p>"#;
    let got = run(&hwpx(body, &[])).unwrap();
    assert_eq!(
        got.lost,
        [
            "글꼴 함초롬바탕(Pretendard로 바꿈)",
            "그림이 아닌 개체 3개(SVG, 동영상, 입력 칸 등이라 뺌)",
            "도형·차트 1개(뺌)",
            "메모 1개(뺌)",
        ]
    );
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
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

/// 무작위 문단·표·그림·링크·도형과 이상한 값(없는 모양 번호, 0·음수·큰 합침, 제어 문자, 엔티티)으로 구역 본문을 만든다.
fn random_body(g: &mut Rng, depth: u32) -> String {
    const TEXT: [&str; 12] = [
        "가",
        "a",
        " ",
        "&amp;",
        "&#xD800;",
        "&#1;",
        "\u{FFFC}",
        "\u{1}",
        "<hp:tab/>",
        "<hp:lineBreak/>",
        "&#x41;",
        "견본",
    ];
    const ID: [&str; 8] = ["0", "1", "2", "3", "4", "5", "99", "-1"];
    const SPAN: [&str; 6] = ["1", "1", "2", "0", "-1", "30"];
    let mut out = String::new();
    for _ in 0..1 + g.below(6) {
        let mut runs = String::new();
        for _ in 0..1 + g.below(4) {
            runs += &match g.below(8) {
                0 if depth < 3 => {
                    let mut trs = String::new();
                    let rows = 1 + g.below(4);
                    for r in 0..rows {
                        trs += "<hp:tr>";
                        for c in 0..1 + g.below(4) {
                            let addr = |g: &mut Rng, v: usize| {
                                if g.below(4) == 0 {
                                    g.below(6).to_string()
                                } else {
                                    v.to_string()
                                }
                            };
                            let (ca, ra) = (addr(g, c), addr(g, r));
                            trs += &format!(
                                r#"<hp:tc header="{}" borderFillIDRef="{}"><hp:subList>{}</hp:subList><hp:cellAddr colAddr="{ca}" rowAddr="{ra}"/><hp:cellSpan colSpan="{}" rowSpan="{}"/><hp:cellSz width="{}" height="1"/></hp:tc>"#,
                                g.below(2),
                                g.pick(&["1", "2", "9"]),
                                random_body(g, depth + 1),
                                g.pick(&SPAN),
                                g.pick(&SPAN),
                                g.pick(&["3600", "0", "-5", "99999999"]),
                            );
                        }
                        trs += "</hp:tr>";
                    }
                    format!(
                        r#"<hp:run charPrIDRef="0"><hp:tbl rowCnt="{}" colCnt="2">{trs}</hp:tbl></hp:run>"#,
                        g.below(9)
                    )
                }
                1 => format!(
                    r#"<hp:run charPrIDRef="0"><hp:pic><hp:curSz width="{}" height="7200"/><hc:img binaryItemIDRef="{}"/></hp:pic></hp:run>"#,
                    g.pick(&["7200", "0", "-1", "999999999"]),
                    g.pick(&["image1", "image2", "gone", "none"])
                ),
                2 => format!(
                    r#"<hp:run><hp:ctrl><hp:fieldBegin type="HYPERLINK"><hp:parameters><hp:stringParam name="Command">{}</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl></hp:run>"#,
                    g.pick(&[
                        "https\\://example.com/a;1;",
                        "javascript\\:x;",
                        "mailto\\:hong@example.com",
                        "\\",
                        ""
                    ])
                ),
                3 => r#"<hp:run><hp:ctrl><hp:fieldEnd/></hp:ctrl></hp:run>"#.into(),
                4 => format!(
                    "<hp:run><hp:{}/></hp:run>",
                    g.pick(&["rect", "ole", "equation", "chart", "secPr"])
                ),
                _ => {
                    let t: String = (0..g.below(6)).map(|_| g.pick(&TEXT)).collect();
                    format!(
                        r#"<hp:run charPrIDRef="{}"><hp:t>{t}</hp:t></hp:run>"#,
                        g.pick(&ID)
                    )
                }
            };
        }
        out += &format!(r#"<hp:p paraPrIDRef="{}">{runs}</hp:p>"#, g.pick(&ID));
    }
    out
}

/// 무작위 견본 64개(씨앗 고정). 구역 XML의 바이트를 가끔 바꾸거나 자른다. 멈추지 않고, 받으면 문서에 넣을 수 있는 조각이다.
#[test]
fn 무작위_한글_문서_64개는_멈추지_않고_곧은_조각이_된다() {
    let mut ok = 0;
    for seed in 0..64u64 {
        let mut g = Rng(0x4857_5058 ^ (seed * 0x9E37_79B9 + 1));
        let mut sec = format!(r#"<hs:sec {HS}>{}</hs:sec>"#, random_body(&mut g, 0)).into_bytes();
        match g.below(4) {
            0 => {
                for _ in 0..1 + g.below(4) {
                    let at = g.below(sec.len());
                    sec[at] = [b'<', b'>', b'"', b'/', b'&', 0, 0xFF][g.below(7)];
                }
            }
            1 => sec.truncate(g.below(sec.len())),
            _ => {}
        }
        let z = hwpx("", &[("Contents/section0.xml", &sec)]);
        if let Ok(got) = run(&z) {
            check_frag(&got.fragment.paras);
            assert!(got.fragment.paras.len() <= MAX_PARAS);
            ok += 1;
        }
    }
    // 바꾸지 않은 견본(절반쯤)은 받아야 한다.
    assert!(ok >= 24, "받은 견본 {ok}개");
}
