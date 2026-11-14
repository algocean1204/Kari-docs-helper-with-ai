//! 다른 앱(브라우저, Google Docs, Pages, Word)에서 복사한 HTML 붙여넣기. 견본은 각 앱이 내는 꼴을 직접 줄여 쓴 것이다.

mod common;

use common::*;
use kari_doc::serde_json::{json, Value};
use kari_edit::{html_fragment, Command, Pos, Refusal};
use std::time::{Duration, Instant};

fn paste(t: &mut T, html: &str) -> Step {
    t.run(Command::PasteHtml {
        html: html.into(),
        text: "평문".into(),
    })
}

/// 문단마다 (style, pf, runs).
fn shape(t: &T) -> Vec<(Option<Value>, Option<Value>, Value)> {
    t.lay
        .paras()
        .iter()
        .filter(|p| kari_layout::cell_of(&t.doc, p).is_none())
        .map(|p| {
            let n = t.doc.node(p).unwrap();
            (
                n.props.get("style").cloned(),
                n.props.get("pf").cloned(),
                t.runs(p),
            )
        })
        .collect()
}

fn texts(t: &T) -> Vec<String> {
    t.lay.paras().iter().map(|p| t.text(p)).collect()
}

/// 새 문서 하나에 붙인다.
fn fresh(html: &str) -> T {
    let mut t = T::new(&["|"]);
    let step = paste(&mut t, html);
    assert_eq!(step.refused, None, "{html}");
    t
}

#[test]
fn 브라우저_견본은_굵게_기울임_밑줄_취소선_링크와_제목을_살리고_기본색은_버린다() {
    let t = fresh(
        "<meta charset='utf-8'><html><body><!--StartFragment--><h2 style=\"font-size:24px;font-weight:700;color:rgb(0,0,0)\">소개</h2>\
         <p style=\"color: rgb(34, 34, 34); font-size: 16px;\">안녕 <b>굵게</b>와 <i>기울임</i>, <u>밑줄</u>, <s>취소선</s>, \
         <a href=\"https://example.com/a?b=1\" style=\"color:rgb(0,0,238)\">링크</a>.</p><!--EndFragment--></body></html>",
    );
    let s = shape(&t);
    assert_eq!(s.len(), 2, "{s:?}");
    // 제목: 스타일은 h2, 글자 크기와 굵기는 스타일에 맡긴다.
    assert_eq!(s[0].0, Some(json!("h2")));
    assert_eq!(s[0].2, json!([{"t": "소개"}]));
    // 본문: 16px = 12pt, 거의 검정은 기본색이라 버린다.
    assert_eq!(s[1].0, None);
    let z = json!({"size": 12});
    assert_eq!(
        s[1].2,
        json!([
            {"t": "안녕 ", "s": z},
            {"t": "굵게", "s": {"weight": 700, "size": 12}},
            {"t": "와 ", "s": z},
            {"t": "기울임", "s": {"italic": true, "size": 12}},
            {"t": ", ", "s": z},
            {"t": "밑줄", "s": {"underline": true, "size": 12}},
            {"t": ", ", "s": z},
            {"t": "취소선", "s": {"strike": true, "size": 12}},
            {"t": ", ", "s": z},
            // 링크는 링크 색으로 그리므로 원본 색을 싣지 않는다.
            {"t": "링크", "s": z, "link": "https://example.com/a?b=1"},
            {"t": ".", "s": z},
        ])
    );
}

#[test]
fn google_docs_견본은_굵기_없는_감싸개와_제목_안의_크기를_따르지_않고_목록_단계를_살린다() {
    let t = fresh(
        "<meta charset='utf-8'><b style=\"font-weight:normal;\" id=\"docs-internal-guid-0\">\
         <h1 dir=\"ltr\" style=\"line-height:1.38;\"><span style=\"font-size:20pt;font-family:Arial,sans-serif;color:#000000;font-weight:400;\">큰 제목</span></h1>\
         <p dir=\"ltr\"><span style=\"font-size:11pt;color:#000000;font-weight:700;\">굵은 글</span><span style=\"font-size:11pt;color:#ff0000;\"> 빨강</span></p>\
         <ul><li dir=\"ltr\"><p dir=\"ltr\"><span style=\"font-size:11pt;\">하나</span></p></li>\
         <li dir=\"ltr\"><p dir=\"ltr\"><span>둘</span></p><ul><li dir=\"ltr\"><p dir=\"ltr\"><span>안쪽</span></p></li></ul></li></ul>\
         <ol start=\"3\"><li><p>셋</p></li><li><p>넷</p></li></ol></b>",
    );
    let s = shape(&t);
    assert_eq!(s[0].0, Some(json!("h1")));
    assert_eq!(s[0].2, json!([{"t": "큰 제목"}]));
    assert_eq!(
        s[1].2,
        json!([
            {"t": "굵은 글", "s": {"weight": 700, "size": 11}},
            {"t": " 빨강", "s": {"size": 11, "color": "#FF0000FF"}},
        ])
    );
    let list = |i: usize| s[i].1.clone().unwrap()["list"].clone();
    assert_eq!(texts(&t)[2..], ["하나", "둘", "안쪽", "셋", "넷"]);
    assert_eq!(list(2), json!({"kind": "BULLET", "level": 0}));
    assert_eq!(list(3), json!({"kind": "BULLET", "level": 0}));
    assert_eq!(list(4), json!({"kind": "BULLET", "level": 1}));
    // 번호 목록: 시작 번호는 첫 항목에만.
    assert_eq!(list(5), json!({"kind": "ORDERED", "level": 0, "start": 3}));
    assert_eq!(list(6), json!({"kind": "ORDERED", "level": 0}));
}

#[test]
fn pages_견본은_시트의_클래스에서_굵기와_밑줄을_읽고_px를_pt로_본다() {
    let t = fresh(
        "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\"><html><head>\
         <meta http-equiv=\"Content-Type\" content=\"text/html; charset=UTF-8\"><title></title><meta name=\"Generator\" content=\"Cocoa HTML Writer\">\
         <style type=\"text/css\">\n\
         p.p1 {margin: 0.0px 0.0px 0.0px 0.0px; font: 12.0px Helvetica}\n\
         p.p2 {margin: 0.0px; font: 12.0px Helvetica; min-height: 14.0px}\n\
         span.s1 {font-kerning: none}\n\
         span.s2 {font: 12.0px Helvetica; font-weight: bold; font-kerning: none}\n\
         span.s3 {text-decoration: underline ; font-kerning: none}\n\
         ul.ul1 {list-style-type: disc}\n\
         </style></head><body><p class=\"p1\"><span class=\"s1\">일반 </span><span class=\"s2\">굵게</span><span class=\"s1\"> 그리고 </span><span class=\"s3\">밑줄</span></p>\
         <p class=\"p2\"><span class=\"s1\"></span><br></p><ul class=\"ul1\"><li class=\"li1\"><span class=\"s1\">첫째</span></li></ul></body></html>",
    );
    let s = shape(&t);
    assert_eq!(s.len(), 3, "{s:?}");
    assert_eq!(
        s[0].2,
        json!([
            {"t": "일반 ", "s": {"size": 12}},
            {"t": "굵게", "s": {"weight": 700, "size": 12}},
            {"t": " 그리고 ", "s": {"size": 12}},
            {"t": "밑줄", "s": {"underline": true, "size": 12}},
        ])
    );
    // 글 없는 줄은 빈 문단이다.
    assert_eq!(texts(&t)[1], "");
    assert_eq!(
        s[2].1,
        Some(json!({"list": {"kind": "BULLET", "level": 0}}))
    );
    assert_eq!(texts(&t)[2], "첫째");
}

#[test]
fn word_견본은_목록_표지를_버리고_mso_list의_종류와_단계를_읽고_표를_표로_넣는다() {
    let t = fresh(
        "<html xmlns:o=\"urn:schemas-microsoft-com:office:office\"><head><meta charset=utf-8><style><!-- /* Style Definitions */ \
         p.MsoNormal, li.MsoNormal {margin:0cm; font-size:12.0pt;} p.MsoListParagraph {margin-left:36.0pt;} \
         @page WordSection1 {size:595.3pt 841.9pt;} div.WordSection1 {page:WordSection1;} --></style></head>\
         <body lang=KO><!--StartFragment--><p class=MsoNormal><b><span lang=EN-US style='font-size:14.0pt'>굵은 제목<o:p></o:p></span></b></p>\
         <p class=MsoListParagraphCxSpFirst style='text-indent:-18.0pt;mso-list:l0 level1 lfo1'><![if !supportLists]>\
         <span style='font-family:Symbol'><span style='mso-list:Ignore'>·<span style='font:7.0pt \"Times New Roman\"'>&nbsp;&nbsp;&nbsp;&nbsp; </span></span></span><![endif]>항목 하나<o:p></o:p></p>\
         <p class=MsoListParagraphCxSpLast style='text-indent:-18.0pt;mso-list:l0 level2 lfo1'><![if !supportLists]>\
         <span style='mso-list:Ignore'>1.<span style='font:7.0pt \"Times New Roman\"'>&nbsp;&nbsp;</span></span><![endif]>하위 번호<o:p></o:p></p>\
         <table class=MsoTableGrid border=1 cellspacing=0 cellpadding=0><tr><td width=100 valign=top><p class=MsoNormal>가<o:p></o:p></p></td>\
         <td><p class=MsoNormal>나</p></td></tr></table><!--EndFragment--></body></html>",
    );
    let s = shape(&t);
    assert_eq!(
        s[0].2,
        json!([{"t": "굵은 제목", "s": {"weight": 700, "size": 14}}])
    );
    assert_eq!(texts(&t)[1], "항목 하나");
    assert_eq!(texts(&t)[2], "하위 번호");
    assert_eq!(
        s[1].1,
        Some(json!({"list": {"kind": "BULLET", "level": 0}}))
    );
    assert_eq!(
        s[2].1,
        Some(json!({"list": {"kind": "ORDERED", "level": 1}}))
    );
    invariants(&t.doc);
    let cells: Vec<String> = t
        .lay
        .paras()
        .iter()
        .filter(|p| kari_layout::cell_of(&t.doc, p).is_some())
        .map(|p| t.text(p))
        .collect();
    assert_eq!(cells, ["가", "나"]);
}

#[test]
fn rtf를_cocoa가_html로_바꾼_글은_클래스로_적은_밑줄과_취소선과_링크를_읽는다() {
    // macOS의 NSAttributedString이 RTF "plain {\\b bold} {\\i ital} {\\ul under} {\\strike strk} 링크"를 바꾼 것(실제 출력).
    let t = fresh(
        "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">\n<html>\n<head>\n\
         <meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">\n<meta http-equiv=\"Content-Style-Type\" content=\"text/css\">\n\
         <title></title>\n<meta name=\"Generator\" content=\"Cocoa HTML Writer\">\n<meta name=\"CocoaVersion\" content=\"2775.1\">\n\
         <style type=\"text/css\">\np.p1 {margin: 0.0px 0.0px 0.0px 0.0px; font: 12.0px Helvetica}\nspan.s1 {text-decoration: underline}\n\
         span.s2 {text-decoration: line-through}\n</style>\n</head>\n<body>\n\
         <p class=\"p1\">plain <b>bold</b> <i>ital</i> <span class=\"s1\">under</span> <span class=\"s2\">strk</span> <a href=\"https://example.com\">link</a></p>\n\
         <p class=\"p1\">second</p>\n</body>\n</html>\n",
    );
    let s = shape(&t);
    assert_eq!(texts(&t), ["plain bold ital under strk link", "second"]);
    // 12.0px은 12pt(Cocoa는 pt 값을 px로 적는다).
    assert_eq!(
        s[0].2,
        json!([
            {"t": "plain ", "s": {"size": 12}},
            {"t": "bold", "s": {"weight": 700, "size": 12}},
            {"t": " ", "s": {"size": 12}},
            {"t": "ital", "s": {"italic": true, "size": 12}},
            {"t": " ", "s": {"size": 12}},
            {"t": "under", "s": {"underline": true, "size": 12}},
            {"t": " ", "s": {"size": 12}},
            {"t": "strk", "s": {"strike": true, "size": 12}},
            {"t": " ", "s": {"size": 12}},
            {"t": "link", "s": {"size": 12}, "link": "https://example.com"},
        ])
    );
}

#[test]
fn 표는_합친_칸과_머리_행을_살리고_표_안에_붙이면_글로_푼다() {
    let t = fresh(
        "<table><thead><tr><th>이름<th>값</thead><tbody>\
         <tr><td rowspan=2>가<td>1<tr><td>2<tr><td colspan=2>합침</table><p>끝",
    );
    invariants(&t.doc);
    let root = t
        .doc
        .children(None)
        .into_iter()
        .find(|n| n.kind == "flow")
        .unwrap();
    let table = t
        .doc
        .children(Some(&root.id))
        .into_iter()
        .find(|n| n.kind == "table")
        .expect("표");
    let g = kari_layout::table_grid(&t.doc, &table.id);
    assert_eq!((g.rows.len(), g.cols), (4, 2));
    assert_eq!(t.doc.prop(&table.id, "headerRows"), Some(&json!(1)));
    let spans: Vec<(usize, usize)> = g.anchors.iter().map(|a| (a.rows, a.cols)).collect();
    assert!(
        spans.contains(&(2, 1)) && spans.contains(&(1, 2)),
        "{spans:?}"
    );
    assert!(texts(&t).contains(&"끝".to_string()));
    // 표 칸 안에 붙이면 표를 만들지 않고 칸 안의 글로 푼다.
    let mut t = T::from_doc(doc_of(&[json!({"runs": [{"t": "앞"}]})]));
    t.run(Command::InsertTable { rows: 1, cols: 1 });
    let cell = t
        .lay
        .paras()
        .iter()
        .find(|p| kari_layout::cell_of(&t.doc, p).is_some())
        .unwrap()
        .clone();
    t.select(&Pos::new(&cell, 0), &Pos::new(&cell, 0));
    let step = paste(&mut t, "<table><tr><td>a<td>b<tr><td>c<td>d</table>");
    assert_eq!(step.refused, None);
    invariants(&t.doc);
    let in_cell: Vec<String> = t
        .lay
        .paras()
        .iter()
        .filter(|p| kari_layout::cell_of(&t.doc, p).is_some())
        .map(|p| t.text(p))
        .collect();
    assert_eq!(in_cell, ["a\tb", "c\td"]);
}

#[test]
fn 줄바꿈과_공백은_브라우저처럼_줄이고_pre는_그대로_두며_글_앞뒤의_빈_줄은_버린다() {
    let t = fresh(
        "<div><br></div><div>  앞   뒤\n줄  <br>다음 줄<br></div><div><br></div><div>&nbsp;</div><pre>가  나\n다</pre><p></p><div><br></div>",
    );
    assert_eq!(texts(&t), ["앞 뒤 줄\n다음 줄", "", "", "가  나\n다"]);
}

#[test]
fn 스크립트_스타일_주석_그림_숨은_글은_버린다() {
    let t = fresh(
        "<p>보임<script>alert(1)</script><style>p{color:red}</style><!-- 주석 --><img src=\"data:image/png;base64,AAAA\" alt=\"그림\">\
         <span hidden>숨김1</span><span style=\"display:none\">숨김2</span><span style=\"visibility:hidden\">숨김3</span>\
         <input value=\"입력\"><button>단추</button><svg><text>벡터</text></svg><noscript>없음</noscript> 끝</p>",
    );
    assert_eq!(texts(&t), ["보임 끝"]);
    // 시트가 정한 숨김과 서식도 읽는다.
    let t = fresh("<style>.h{display:none} .b{font-weight:bold}</style><p><span class=h>숨김</span><span class=b>굵게</span>");
    assert_eq!(texts(&t), ["굵게"]);
    assert_eq!(shape(&t)[0].2, json!([{"t": "굵게", "s": {"weight": 700}}]));
}

#[test]
fn 링크는_허용_스킴만_읽고_색은_종이에서_보이는_것만_읽는다() {
    let t = fresh(
        "<p><a href=\"javascript:alert(1)\">스크립트</a> <a href=\"/상대\">상대</a> <a href=\"#a\">쪽지</a> \
         <a href=\"mailto:a@example.com\">메일</a> <a href=\"HTTP://EXAMPLE.COM/x\">대문자</a> \
         <span style=\"color:#fff\">흰색</span> <span style=\"color:#ff000080\">반투명</span> <span style=\"color:blue\">파랑</span> \
         <span style=\"color:rgb(250 250 250)\">거의 흰색</span> <span style=\"font-size:9999px\">큼</span> <span style=\"font-size:3em\">상대</span></p>",
    );
    let runs = shape(&t)[0].2.clone();
    let runs: Vec<&Value> = runs.as_array().unwrap().iter().collect();
    let link = |t: &str| {
        runs.iter()
            .find(|r| r["t"] == t)
            .and_then(|r| r.get("link").cloned())
    };
    assert_eq!(link("스크립트"), None);
    assert_eq!(link("상대"), None);
    assert_eq!(link("쪽지"), None);
    assert_eq!(link("메일"), Some(json!("mailto:a@example.com")));
    assert_eq!(link("대문자"), Some(json!("HTTP://EXAMPLE.COM/x")));
    let s = |t: &str| {
        runs.iter()
            .find(|r| r["t"].as_str().is_some_and(|x| x.contains(t)))
            .and_then(|r| r.get("s").cloned())
    };
    assert_eq!(s("흰색").filter(|s| s.get("color").is_some()), None);
    assert_eq!(s("거의 흰색").filter(|s| s.get("color").is_some()), None);
    assert_eq!(s("파랑"), Some(json!({"color": "#0000FFFF"})));
    // 크기는 문서가 받는 범위(1~1000pt) 안에서만, 상대 크기는 읽지 않는다.
    assert_eq!(s("큼"), None);
    assert_eq!(s("상대"), None);
}

#[test]
fn 빈_문단에_붙이면_첫_문단의_제목과_목록_모양을_받고_글_가운데에_붙이면_글로_들어간다() {
    let mut t = T::new(&["|"]);
    paste(&mut t, "<h3>제목</h3><p>본문</p>");
    assert_eq!(
        shape(&t).iter().map(|s| s.0.clone()).collect::<Vec<_>>(),
        [Some(json!("h3")), None]
    );
    // 가운데: 첫 문단은 커서 문단의 글에 이어 붙는다.
    let mut t = T::new(&["앞|뒤"]);
    paste(&mut t, "<h3>제목</h3><p>본문</p>");
    assert_eq!(texts(&t), ["앞제목", "본문뒤"]);
    assert_eq!(shape(&t)[0].0, None);
    // 선택이 있으면 지우고 붙인다.
    let mut t = T::new(&["가[나]다"]);
    paste(&mut t, "<b>굵게</b>");
    assert_eq!(texts(&t), ["가굵게다"]);
    assert_eq!(
        t.runs("p0"),
        json!([{"t": "가"}, {"t": "굵게", "s": {"weight": 700}}, {"t": "다"}])
    );
}

#[test]
fn 되돌리기는_한_번이고_이름은_붙여넣기다() {
    let mut t = T::new(&["앞|뒤"]);
    let h0 = t.doc.hash();
    let step = paste(
        &mut t,
        "<p>하나</p><ul><li>둘</ul><table><tr><td>셀</table>",
    );
    assert_eq!(step.label, "붙여넣기");
    assert_eq!(t.undo.len(), 1);
    let inv = t.undo.pop().unwrap();
    t.external(Value::Array(inv));
    assert_eq!(t.doc.hash(), h0);
}

#[test]
fn 읽을_글이_없으면_평문으로_붙이고_너무_큰_입력도_평문으로_물러난다() {
    for html in [
        "",
        "<html><head><title>제목</title></head><body></body></html>",
        "<script>x</script>",
        "<p></p><div><br></div>",
    ] {
        let mut t = T::new(&["|"]);
        let step = paste(&mut t, html);
        assert_eq!(texts(&t), ["평문"], "{html}");
        assert_eq!(step.refused, None);
    }
    // 6MB가 넘는 HTML은 읽지 않는다.
    let big = format!("<p>{}</p>", "가".repeat(3 << 20));
    assert!(html_fragment(&big, 480.0, true).unwrap().is_none());
    let mut t = T::new(&["|"]);
    paste(&mut t, &big);
    assert_eq!(texts(&t), ["평문"]);
}

fn within(what: &str, secs: u64, f: impl FnOnce()) {
    let t0 = Instant::now();
    f();
    assert!(
        t0.elapsed() < Duration::from_secs(secs),
        "{what}: {:?}",
        t0.elapsed()
    );
}

#[test]
fn 해로운_입력은_멈추지_않고_평문이나_빈_결과로_물러난다() {
    // 깊은 중첩: 파서가 제곱으로 느려지지 않게 어림으로 걸러 평문으로 붙인다.
    for n in [600, 100_000] {
        let deep = "<div>".repeat(n) + "깊이" + &"</div>".repeat(n);
        within("깊은 div", 5, || {
            let mut t = T::new(&["|"]);
            let step = paste(&mut t, &deep);
            assert_eq!(step.refused, None);
            assert_eq!(texts(&t), ["평문"]);
        });
    }
    // 허용 깊이 안의 중첩은 읽는다.
    let ok = "<div>".repeat(300) + "중첩" + &"</div>".repeat(300);
    assert_eq!(texts(&fresh(&ok)), ["중첩"]);
    // 닫히지 않은 태그, 잘린 태그, 거대한 속성, 깨진 엔티티.
    let giant = format!(
        "<p style=\"{}\" title=\"{}\">큰 속성</p>",
        "color:red;".repeat(50_000),
        "가".repeat(1 << 20)
    );
    for html in [
        "<p>잘린 <b",
        "<p>잘린 <a href=\"https://exa",
        "<b><i>겹침</b>이어서</i><u>밑줄",
        "<table><tr><td><table><tr><td>안",
        "&amp &lt &#xZZ; &#99999999; &unknown; 끝",
        "<p>\u{0}\u{1}널과 제어 문자\u{7f}</p>",
        giant.as_str(),
        "<<<<>>>><p<p<p>>",
        "<td>표 없는 칸<tr>행<th>머리",
    ] {
        within(&html.chars().take(30).collect::<String>(), 5, || {
            let mut t = T::new(&["|"]);
            let step = paste(&mut t, html);
            assert_eq!(step.refused, None, "{html:.40}");
            invariants(&t.doc);
        });
    }
    let t = fresh(&giant);
    assert_eq!(texts(&t), ["큰 속성"]);
    // 제어 문자는 문서에 들어가지 않는다(문서가 거절하는 글자).
    assert_eq!(texts(&fresh("<p>\u{0}\u{1}널\u{7f}")), ["널"]);
    // 태그가 아주 많은 입력도 걸리지 않는다.
    let many = "<span>가</span>".repeat(60_000);
    within("태그 많음", 20, || {
        let mut t = T::new(&["|"]);
        paste(&mut t, &many);
        assert_eq!(t.text("p0").chars().count(), 60_000);
    });
}

#[test]
fn 문단이_상한을_넘으면_거절하고_표_상한을_넘는_표는_글로_푼다() {
    let many = "<p>가</p>".repeat(50_001);
    let mut t = T::new(&["|"]);
    let before = t.doc.hash();
    let step = paste(&mut t, &many);
    assert_eq!(step.refused, Some(Refusal::TooManyLines));
    assert_eq!(t.doc.hash(), before);
    let ok = "<p>가</p>".repeat(50_000);
    assert!(html_fragment(&ok, 480.0, true).unwrap().is_some());
    // 행이 51개거나 열이 21개인 표는 표로 만들지 않고 행마다 한 문단, 칸은 탭으로 푼다.
    let rows = format!("<table>{}</table>", "<tr><td>a<td>b</tr>".repeat(51));
    let t = fresh(&rows);
    assert_eq!(texts(&t).len(), 51);
    assert_eq!(texts(&t)[0], "a\tb");
    let cols = format!("<table><tr>{}</tr></table>", "<td>x".repeat(21));
    let t = fresh(&cols);
    assert_eq!(texts(&t), [["x"; 21].join("\t")]);
    // 긴 문단은 문서의 한 문단 상한에 못 미치게 나눠 넣는다.
    let long = format!("<p>{}</p>", "가".repeat(250_000));
    let t = fresh(&long);
    assert!(texts(&t).len() >= 3);
    assert_eq!(
        texts(&t).iter().map(|s| s.chars().count()).sum::<usize>(),
        250_000
    );
}

#[test]
fn 상한_검사를_지나는_입력도_트리가_노드_수와_단_수의_상한을_넘으면_읽기를_그만두고_평문으로_물러난다(
) {
    // 서식 요소 500개를 연 채 문단을 이으면 파서가 문단마다 그 500개를 다시 세워 노드가 입력 크기에 비례하지 않는다
    // (입력 7만 바이트가 노드 800만 개). 노드 수 상한이 읽기를 그만두게 한다.
    let mut recon = String::from("<p>");
    for i in 0..500 {
        recon += &format!("<b class=c{i}>");
    }
    recon += "</p>";
    recon += &"<p>x".repeat(16_000);
    within("서식 요소 다시 세우기", 10, || {
        assert!(html_fragment(&recon, 480.0, true).unwrap().is_none());
        let mut t = T::new(&["|"]);
        paste(&mut t, &recon);
        assert_eq!(texts(&t), ["평문"]);
    });
    // 닫는 태그마다 겹침 어림이 0으로 되돌아가는 입력: 실제 겹침은 깊어져 파서가 제곱으로 느려진다.
    let aaa = "<b><div></b>x".repeat(66_000);
    within("겹침 어림 우회", 10, || {
        assert!(html_fragment(&aaa, 480.0, true).unwrap().is_none());
    });
    // 상한 안의 큰 입력은 읽는다: 문단 5만 개는 노드 10만 개 남짓이다.
    let ok = "<p>가</p>".repeat(50_000);
    assert!(html_fragment(&ok, 480.0, true).unwrap().is_some());
}
