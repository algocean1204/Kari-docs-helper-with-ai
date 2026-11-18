//! DOCX 표를 버릴 때 칸 안의 그림을 저장하지 않는 것, 겹친 상자 안의 글을 한 번만 쌓는 것.

use super::*;

#[test]
fn 버리는_표의_칸_안_그림은_저장하지_않는다() {
    // 첫 칸에 그림이 있고, 둘째 칸이 열 상한(20)을 넘게 합친다.
    let body = format!(
        r#"<w:tbl><w:tr><w:tc>{}</w:tc><w:tc><w:tcPr><w:gridSpan w:val="25"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>"#,
        p("", &drawing("rId9"))
    );
    let mut calls = 0;
    let got = import(&docx(&body, &[]), BODY, &mut |b| {
        calls += 1;
        store(b)
    })
    .unwrap();
    assert_eq!(calls, 0, "버린 표의 그림을 저장했다");
    assert!(
        got.lost.iter().any(|l| l.starts_with("큰 표 1개")),
        "{:?}",
        got.lost
    );
    assert!(
        !got.lost.iter().any(|l| l.contains("그림")),
        "{:?}",
        got.lost
    );
}

#[test]
fn 겹친_상자_안의_글은_한_번만_쌓고_상한에서_멈춘다() {
    let mut inner = p("", &r("L40"));
    for k in (0..40).rev() {
        inner = format!(
            "<w:p>{}<w:r><w:pict><w:txbxContent>{inner}</w:txbxContent></w:pict></w:r></w:p>",
            r(&format!("L{k}"))
        );
    }
    let cell = |c: &str| format!("<w:tbl><w:tr><w:tc>{c}</w:tc></w:tr></w:tbl>");
    // 바깥 표 안의 표는 글로 풀어 쓴다.
    let body = cell(&format!("{}{}", p("", &r("앞")), cell(&inner)));
    let f = run(&docx(&body, &[])).unwrap().fragment;
    let c = f.paras[0].table.as_ref().unwrap().rows[0].1[0]
        .as_ref()
        .unwrap();
    let text: Vec<String> = c.1.iter().map(text_of).collect();
    assert_eq!(text[0], "앞");
    let want: Vec<String> = (0..=40).map(|k| format!("L{k}")).collect();
    assert_eq!(text[1], want.join(" "));
    let long = p("", &r(&"가".repeat(200_000)));
    let body = cell(&format!("{}{}", p("", &r("앞")), cell(&long)));
    let f = run(&docx(&body, &[])).unwrap().fragment;
    let c = f.paras[0].table.as_ref().unwrap().rows[0].1[0]
        .as_ref()
        .unwrap();
    assert_eq!(text_of(&c.1[1]).chars().count(), 90_000);
}
