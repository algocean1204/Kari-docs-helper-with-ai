//! 한글 문서(HWPX) 가져오기 시험. 견본 꾸러미는 시험 안에서 만든다(사람과 회사는 지어낸 것).

use kari_doc::serde_json::json;

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
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.extend_from_slice(&[0; 600]);
    assert_eq!(run(&ole).err(), Some(HWP_LEGACY));
    assert_eq!(run(b"HWP Document File V3.00 \x1a").err(), Some(HWP_LEGACY));
    assert!(HWP_LEGACY.contains("HWPX나 PDF로 저장"));
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
