//! DOCX·텍스트 내보내기의 확인 항목. 꾸러미는 `unzip`, XML은 python3의 파서로 읽는다.
//! macOS에서는 `textutil`로 되읽는다. 도구가 없으면 건너뛰지 않고 실패한다.

use std::collections::BTreeSet;
use std::process::Command;
use std::time::Instant;

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Kind};
use kari_docx::{export, text, Docx};

/// 문단 `p0`, `p1`, …을 가진 문서. `paras`는 문단마다의 props. `root`는 루트에 더할 props.
fn doc_with(root: Value, paras: &[Value]) -> Document {
    let mut d = Document::new(Kind::Flow, "홍길동 이력서").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    if root.as_object().is_some_and(|o| !o.is_empty()) {
        ops.push(json!({"op": "set", "id": "root", "props": root}));
    }
    for (i, p) in paras.iter().enumerate() {
        ops.push(json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root", "props": p}}));
    }
    d.apply_trusted(&Value::Array(ops)).unwrap();
    d
}

fn doc(paras: &[Value]) -> Document {
    doc_with(json!({}), paras)
}

fn apply(d: &mut Document, ops: Vec<Value>) {
    d.apply_trusted(&Value::Array(ops)).unwrap();
}

/// 표 하나의 op들. `cells[r][c]`는 (글, 셀 속성). 글이 `None`이면 가려진 셀(빈 문단).
fn table_ops(id: &str, props: Value, cells: &[Vec<(Option<&str>, Value)>]) -> Vec<Value> {
    let mut ops = vec![
        json!({"op": "add", "node": {"id": id, "type": "table", "parent": "root", "props": props}}),
    ];
    for (r, row) in cells.iter().enumerate() {
        let rid = format!("{id}r{r}");
        ops.push(json!({"op": "add", "node": {"id": rid, "type": "row", "parent": id}}));
        for (c, (t, props)) in row.iter().enumerate() {
            let cid = format!("{id}c{r}_{c}");
            let mut props = props.clone();
            if t.is_none() {
                props["covered"] = json!(true);
            }
            ops.push(json!({"op": "add", "node": {"id": cid, "type": "cell", "parent": rid, "props": props}}));
            ops.push(json!({"op": "add", "node": {"id": format!("{id}p{r}_{c}"), "type": "para", "parent": cid,
                "props": {"runs": [{"t": t.unwrap_or("")}]}}}));
        }
    }
    ops
}

/// 작은 PNG(색 한 가지). 압축은 내보내기와 같은 크레이트로 한다.
fn png(w: u32, h: u32) -> Vec<u8> {
    let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32fast::hash(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    // 행마다 거르개 0과 화소들.
    let row: Vec<u8> = std::iter::once(0)
        .chain([70, 110, 160].repeat(w as usize))
        .collect();
    let raw = row.repeat(h as usize);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

/// 사진처럼 보이는 그림(배경, 어깨, 얼굴). PDF 시험의 견본과 같은 모양이다.
fn photo(w: i32, h: i32, format: skia_safe::EncodedImageFormat) -> Vec<u8> {
    use skia_safe::{surfaces, Color, Paint};
    let mut s = surfaces::raster_n32_premul((w, h)).unwrap();
    let c = s.canvas();
    c.clear(Color::from_rgb(210, 222, 236));
    let mut p = Paint::default();
    p.set_anti_alias(true);
    let (fw, fh) = (w as f32, h as f32);
    p.set_color(Color::from_rgb(60, 72, 96));
    c.draw_oval(
        skia_safe::Rect::from_xywh(fw * 0.1, fh * 0.62, fw * 0.8, fh * 0.6),
        &p,
    );
    p.set_color(Color::from_rgb(236, 200, 170));
    c.draw_circle((fw * 0.5, fh * 0.42), fw * 0.22, &p);
    let img = s.image_snapshot();
    img.peek_pixels().unwrap().encode(format, Some(90)).unwrap()
}

fn no_assets(_: &str) -> Option<Vec<u8>> {
    None
}

/// 내보내 `CARGO_TARGET_TMPDIR/name`에 쓴다.
fn write(name: &str, d: &Document, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> (Docx, String) {
    let out = export(d, Some(1_794_550_245_000), read);
    let path = format!("{}/{name}", env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(&path, &out.bytes).unwrap();
    (out, path)
}

/// 도구를 돌려 표준 출력을 받는다. 없거나 실패하면 시험이 실패한다.
fn tool(cmd: &str, args: &[&str]) -> String {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{cmd}를 실행하지 못했습니다: {e}"));
    assert!(
        out.status.success(),
        "{cmd} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// 꾸러미 안 부분의 글.
fn part(path: &str, name: &str) -> String {
    // unzip은 이름의 대괄호를 패턴으로 읽는다.
    let name = name.replace('[', "\\[").replace(']', "\\]");
    tool("unzip", &["-p", path, &name])
}

/// 꾸러미의 부분 이름들.
fn names(path: &str) -> BTreeSet<String> {
    tool("unzip", &["-Z1", path])
        .lines()
        .map(str::to_string)
        .collect()
}

/// `attr="…"`의 값들.
fn attrs(xml: &str, attr: &str) -> Vec<String> {
    let key = format!("{attr}=\"");
    xml.match_indices(&key)
        .map(|(i, _)| {
            let rest = &xml[i + key.len()..];
            rest[..rest.find('"').unwrap()].to_string()
        })
        .collect()
}

/// X1: zip으로 열리고, 필요한 부분이 있고, 모든 XML이 잘 짜여 있고, 관계 id가 가리키는 대상이 모두 있다.
fn check_package(path: &str) {
    let t = tool("unzip", &["-t", path]);
    assert!(t.contains("No errors detected"), "{t}");
    let names = names(path);
    for need in [
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/core.xml",
        "word/document.xml",
        "word/_rels/document.xml.rels",
        "word/styles.xml",
        "word/numbering.xml",
        "word/settings.xml",
    ] {
        assert!(names.contains(need), "{need} 없음: {names:?}");
    }
    // 모든 XML 부분을 파서로 읽는다.
    let script = "import sys, zipfile, xml.dom.minidom as m\nz = zipfile.ZipFile(sys.argv[1])\nassert z.testzip() is None\nfor n in z.namelist():\n    if n.endswith('.xml') or n.endswith('.rels'):\n        m.parseString(z.read(n))\nprint('ok')";
    assert_eq!(tool("python3", &["-I", "-c", script, path]).trim(), "ok");
    // 관계: 안쪽 대상은 꾸러미에 있고, 본문·머리글·바닥글이 쓰는 r:id는 모두 관계에 있다.
    let rels = part(path, "word/_rels/document.xml.rels");
    let ids: BTreeSet<String> = attrs(&rels, "Id").into_iter().collect();
    for rel in rels.split("<Relationship ").skip(1) {
        if !rel.contains("TargetMode=\"External\"") {
            let target = &attrs(rel, "Target")[0];
            assert!(names.contains(&format!("word/{target}")), "{target} 없음");
        }
    }
    for n in names
        .iter()
        .filter(|n| n.starts_with("word/") && n.ends_with(".xml"))
    {
        let xml = part(path, n);
        for id in attrs(&xml, "r:id")
            .into_iter()
            .chain(attrs(&xml, "r:embed"))
        {
            assert!(ids.contains(&id), "{n}의 {id}가 관계에 없음");
        }
    }
    let top = part(path, "_rels/.rels");
    for target in attrs(&top, "Target") {
        assert!(names.contains(&target), "{target} 없음");
    }
    // 내용 종류: 머리글·바닥글·미디어가 모두 등록돼 있다.
    let types = part(path, "[Content_Types].xml");
    for n in &names {
        if n.contains("header") || n.contains("footer") {
            assert!(
                types.contains(&format!("PartName=\"/{n}\"")),
                "{n}의 종류 없음"
            );
        }
        if let Some(ext) = n
            .strip_prefix("word/media/")
            .and_then(|f| f.rsplit('.').next())
        {
            assert!(types.contains(&format!("Extension=\"{ext}\"")), "{ext}");
        }
    }
}

/// 2장의 표 줄마다 쓰는 문서: 글자·문단 모양, 스타일, 목록, 링크, 그림, 표, 쪽, 머리글·바닥글, 언어.
fn all_features() -> Document {
    let mut d = doc_with(
        json!({
            "page": {"preset": "A4", "w": 841.89, "h": 595.28, "orientation": "landscape",
                "margin": {"t": 50, "r": 40, "b": 60, "l": 30}},
            "header": {"l": "홍길동", "c": "", "r": "이력서"},
            "footer": {"c": "{쪽} / {전체}"},
            "hfSkipFirst": true,
            "lang": "ko",
            "styles": {
                "body": {"name": "본문", "cs": {"font": "Pretendard", "size": 10.5, "weight": 400, "color": "#1E1E1EFF"},
                    "pf": {"align": "LEFT", "line": {"unit": "PERCENT", "value": 160}, "space": {"before": 0, "after": 4}}},
                "title": {"name": "제목", "based": "body", "next": "body", "cs": {"size": 20, "weight": 700}},
                "h1": {"name": "제목 1", "based": "body", "next": "body", "cs": {"size": 13, "weight": 700}, "pf": {"keepNext": true}},
                "quote": {"name": "인용", "based": "body", "cs": {"italic": true}, "pf": {"tabs": [{"pos": 100}]}},
            }
        }),
        &[
            json!({"style": "title", "runs": [{"t": "제목 글"}]}),
            json!({"style": "h1", "runs": [{"t": "경력"}]}),
            json!({"runs": [{"t": "첫 줄\n둘째 줄\t탭 뒤"}]}),
            json!({"runs": [
                {"t": "굵게", "s": {"weight": 700}},
                {"t": "기울임", "s": {"italic": true}},
                {"t": "밑줄", "s": {"underline": true}},
                {"t": "취소선", "s": {"strike": true}},
                {"t": "크기", "s": {"size": 14}},
                {"t": "빨강", "s": {"color": "#FF0000FF"}},
                {"t": "형광펜", "s": {"bg": "#FFEB3BFF"}},
                {"t": "위", "s": {"vertical": "SUPER"}},
                {"t": "아래", "s": {"vertical": "SUB"}},
                {"t": "자간", "s": {"spacing": 1.5}},
            ]}),
            json!({"runs": [{"t": "가운데"}], "pf": {"align": "CENTER", "indent": {"left": 36, "right": 18, "first": 12},
                "space": {"before": 12, "after": 6}, "keepNext": true, "keepLines": true, "breakBefore": true}}),
            json!({"runs": [{"t": "오른쪽 내어쓰기"}], "pf": {"align": "RIGHT", "indent": {"first": -9}, "line": {"unit": "PT", "value": 18}}}),
            json!({"runs": [{"t": "양쪽"}], "pf": {"align": "JUSTIFIED"}}),
            json!({"runs": [{"t": "이름\t2024"}], "pf": {"tabs": [{"pos": 200, "align": "RIGHT", "leader": "DOT"},
                {"pos": 120, "align": "CENTER", "leader": "DASH"}, {"pos": 150, "align": "DECIMAL", "leader": "LINE"}]}}),
            json!({"style": "quote", "runs": [{"t": "인용 글"}], "pf": {"tabs": [{"pos": 50}]}}),
            json!({"runs": [{"t": "첫째"}], "pf": {"list": {"kind": "ORDERED", "level": 0}}}),
            json!({"runs": [{"t": "가 항목"}], "pf": {"list": {"kind": "ORDERED", "level": 1}}}),
            json!({"runs": [{"t": "점"}], "pf": {"list": {"kind": "BULLET", "level": 1}}}),
            json!({"runs": [{"t": "다섯째부터"}], "pf": {"list": {"kind": "ORDERED", "level": 0, "start": 5}}}),
            json!({"runs": [{"t": "동그라미"}], "pf": {"list": {"kind": "ORDERED", "level": 6}}}),
            json!({"runs": [
                {"t": "웹", "link": "https://example.com/a?b=1&c=2"},
                {"t": " · "},
                {"t": "메일", "link": "mailto:hello@example.com"},
                {"t": " · "},
                {"t": "전화", "link": "tel:+82-10-1234-5678"},
            ]}),
            json!({"runs": [{"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 120, "h": 90, "alt": "증명사진"}}]}),
        ],
    );
    let none = json!({});
    let mut ops = table_ops(
        "t",
        json!({"cols": [100, 150, 120], "headerRows": 1, "border": {"color": "#336699FF", "weight": 1}}),
        &[
            vec![
                (
                    Some("기간"),
                    json!({"fill": "#EEF2F7FF", "alignV": "CENTER"}),
                ),
                (
                    Some("회사"),
                    json!({"span": {"rows": 1, "cols": 2}, "pad": [2, 8, 2, 8]}),
                ),
                (None, none.clone()),
            ],
            vec![
                (
                    Some("2024"),
                    json!({"span": {"rows": 2, "cols": 1}, "alignV": "BOTTOM"}),
                ),
                (Some("가"), none.clone()),
                (Some("나"), none.clone()),
            ],
            vec![
                (None, none.clone()),
                (Some("다"), none.clone()),
                (Some("라"), none.clone()),
            ],
        ],
    );
    ops.push(json!({"op": "set", "id": "tr0", "props": {"h": 30}}));
    apply(&mut d, ops);
    d
}

fn read_photo(h: &str) -> Option<Vec<u8>> {
    (h == "a".repeat(64)).then(|| png(40, 30))
}

#[test]
fn x1_꾸러미가_열리고_xml이_잘_짜여_있고_관계가_맞다() {
    let (_, path) = write("x1.docx", &all_features(), &read_photo);
    check_package(&path);
    let names = names(&path);
    for need in [
        "word/header1.xml",
        "word/header2.xml",
        "word/footer1.xml",
        "word/footer2.xml",
        "word/media/image1.png",
    ] {
        assert!(names.contains(need), "{need}");
    }
}

#[test]
fn x2_표의_줄마다_값이_기대와_같다() {
    let (out, path) = write("x2.docx", &all_features(), &read_photo);
    assert!(out.lost.is_empty(), "{:?}", out.lost);
    let doc = part(&path, "word/document.xml");
    let has = |s: &str| assert!(doc.contains(s), "document.xml에 없음: {s}");
    // 문단과 글, 줄바꿈, 탭.
    has(
        r#"<w:t xml:space="preserve">첫 줄</w:t><w:br/><w:t xml:space="preserve">둘째 줄</w:t><w:tab/><w:t xml:space="preserve">탭 뒤</w:t>"#,
    );
    // 글자 모양(크기는 반 pt, 자간은 20분의 1 pt).
    for s in [
        r#"<w:rPr><w:b/></w:rPr><w:t xml:space="preserve">굵게"#,
        r#"<w:rPr><w:i/></w:rPr><w:t xml:space="preserve">기울임"#,
        r#"<w:rPr><w:u w:val="single"/></w:rPr><w:t xml:space="preserve">밑줄"#,
        r#"<w:rPr><w:strike/></w:rPr><w:t xml:space="preserve">취소선"#,
        r#"<w:rPr><w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr><w:t xml:space="preserve">크기"#,
        r#"<w:rPr><w:color w:val="FF0000"/></w:rPr><w:t xml:space="preserve">빨강"#,
        r#"<w:rPr><w:shd w:val="clear" w:color="auto" w:fill="FFEB3B"/></w:rPr><w:t xml:space="preserve">형광펜"#,
        r#"<w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t xml:space="preserve">위"#,
        r#"<w:rPr><w:vertAlign w:val="subscript"/></w:rPr><w:t xml:space="preserve">아래"#,
        r#"<w:rPr><w:spacing w:val="30"/></w:rPr><w:t xml:space="preserve">자간"#,
    ] {
        has(s);
    }
    // 문단 모양(길이는 20분의 1 pt). 줄 간격 18pt 고정 = 360, 앞 12pt = 240.
    has(
        r#"<w:pPr><w:keepNext/><w:keepLines/><w:pageBreakBefore/><w:spacing w:before="240" w:after="120"/><w:ind w:left="720" w:right="360" w:firstLine="240"/><w:jc w:val="center"/></w:pPr>"#,
    );
    has(
        r#"<w:pPr><w:spacing w:line="360" w:lineRule="exact"/><w:ind w:left="0" w:right="0" w:hanging="180"/><w:jc w:val="right"/></w:pPr>"#,
    );
    has(r#"<w:jc w:val="both"/>"#);
    has(
        r#"<w:tabs><w:tab w:val="center" w:leader="hyphen" w:pos="2400"/><w:tab w:val="decimal" w:leader="underscore" w:pos="3000"/><w:tab w:val="right" w:leader="dot" w:pos="4000"/></w:tabs>"#,
    );
    // 스타일의 탭 정지(100pt)를 문단이 갈음하면 지운다.
    has(
        r#"<w:pStyle w:val="K-quote"/><w:tabs><w:tab w:val="clear" w:pos="2000"/><w:tab w:val="left" w:leader="none" w:pos="1000"/></w:tabs>"#,
    );
    has(r#"<w:pStyle w:val="Title"/>"#);
    has(r#"<w:pStyle w:val="Heading1"/>"#);
    // 목록: 글머리표는 numId 1, 번호는 다시 시작할 때마다 새 numId. 글 왼쪽 끝 = 18pt × (수준 + 1).
    has(
        r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr><w:ind w:left="360" w:right="0" w:hanging="360"/>"#,
    );
    has(
        r#"<w:numPr><w:ilvl w:val="1"/><w:numId w:val="2"/></w:numPr><w:ind w:left="720" w:right="0" w:hanging="360"/>"#,
    );
    has(r#"<w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr>"#);
    has(r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr>"#);
    has(r#"<w:numPr><w:ilvl w:val="6"/><w:numId w:val="3"/></w:numPr><w:ind w:left="2520""#);
    let num = part(&path, "word/numbering.xml");
    for s in [
        r#"<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="ganada"/><w:lvlText w:val="%2."/>"#,
        r#"<w:lvl w:ilvl="5"><w:start w:val="1"/><w:numFmt w:val="ganada"/><w:lvlText w:val="(%6)"/>"#,
        r#"<w:lvl w:ilvl="6"><w:start w:val="1"/><w:numFmt w:val="decimalEnclosedCircle"/><w:lvlText w:val="%7"/>"#,
        r#"<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="◦"/>"#,
        r#"<w:num w:numId="3"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride></w:num>"#,
    ] {
        assert!(num.contains(s), "numbering.xml에 없음: {s}");
    }
    // 링크: 관계와 바깥 주소.
    let rels = part(&path, "word/_rels/document.xml.rels");
    for url in [
        "https://example.com/a?b=1&amp;c=2",
        "mailto:hello@example.com",
        "tel:+82-10-1234-5678",
    ] {
        assert!(
            rels.contains(&format!(
                r#"/hyperlink" Target="{url}" TargetMode="External"/>"#
            )),
            "{url}: {rels}"
        );
    }
    assert_eq!(doc.matches("<w:hyperlink r:id=").count(), 3);
    has(
        r#"<w:hyperlink r:id="rId4" w:history="1"><w:r><w:rPr><w:color w:val="0B57D0"/><w:u w:val="single"/></w:rPr><w:t xml:space="preserve">웹</w:t>"#,
    );
    // 그림: 120×90pt = 1524000×1143000 EMU, 대체 텍스트.
    has(
        r#"<wp:extent cx="1524000" cy="1143000"/><wp:docPr id="1" name="그림 1" descr="증명사진"/>"#,
    );
    assert!(rels.contains(r#"/image" Target="media/image1.png"/>"#));
    // 표: 열 폭, 합친 칸, 머리 행, 선(8분의 1 pt), 바탕, 세로 맞춤, 안쪽 여백, 행 높이.
    for s in [
        r#"<w:tblW w:w="7400" w:type="dxa"/><w:tblBorders><w:top w:val="single" w:sz="8" w:space="0" w:color="336699"/>"#,
        r#"<w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/><w:gridCol w:w="2400"/></w:tblGrid>"#,
        r#"<w:trPr><w:trHeight w:val="600" w:hRule="atLeast"/><w:cantSplit/><w:tblHeader/></w:trPr>"#,
        r#"<w:tcW w:w="2000" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="EEF2F7"/><w:vAlign w:val="center"/>"#,
        r#"<w:tcW w:w="5400" w:type="dxa"/><w:gridSpan w:val="2"/><w:tcMar><w:top w:w="40" w:type="dxa"/><w:left w:w="160" w:type="dxa"/><w:bottom w:w="40" w:type="dxa"/><w:right w:w="160" w:type="dxa"/></w:tcMar>"#,
        r#"<w:vMerge w:val="restart"/><w:vAlign w:val="bottom"/>"#,
        r#"<w:vMerge/><w:vAlign w:val="bottom"/></w:tcPr><w:p/></w:tc>"#,
        r#"<w:tblCellMar><w:top w:w="80" w:type="dxa"/><w:left w:w="120" w:type="dxa"/>"#,
    ] {
        has(s);
    }
    assert_eq!(doc.matches("<w:tblHeader/>").count(), 1);
    // 쪽: 가로 A4, 여백. 머리글 띠 50pt → (50 - 10.8) / 2 = 19.6pt = 392.
    has(
        r#"<w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/><w:pgMar w:top="1000" w:right="800" w:bottom="1200" w:left="600" w:header="392" w:footer="492" w:gutter="0"/><w:titlePg/>"#,
    );
    has(r#"<w:headerReference w:type="default" r:id="#);
    has(r#"<w:headerReference w:type="first" r:id="#);
    let hdr = part(&path, "word/header1.xml");
    assert!(
        hdr.contains(r#"<w:tab w:val="center" w:pos="7719"/><w:tab w:val="right" w:pos="15438"/>"#),
        "{hdr}"
    );
    assert!(hdr.contains("홍길동") && hdr.contains("이력서"));
    let ftr = part(&path, "word/footer1.xml");
    assert!(
        ftr.contains(r#"<w:fldSimple w:instr=" PAGE ">"#)
            && ftr.contains(r#"<w:fldSimple w:instr=" NUMPAGES ">"#),
        "{ftr}"
    );
    assert!(ftr.contains("<w:sz w:val=\"18\"/>") && ftr.contains("5A5A5A"));
    assert_eq!(part(&path, "word/header2.xml").matches("<w:t").count(), 0);
    // 스타일: 본문은 Normal(기본), 제목, 제목 1(개요 수준), 사용자 스타일 이름.
    let st = part(&path, "word/styles.xml");
    for s in [
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:before="0" w:after="80" w:line="320" w:lineRule="auto"/><w:jc w:val="left"/></w:pPr><w:rPr><w:rFonts w:ascii="Pretendard" w:hAnsi="Pretendard" w:eastAsia="Pretendard" w:cs="Pretendard"/><w:b w:val="0"/><w:color w:val="1E1E1E"/><w:sz w:val="21"/><w:szCs w:val="21"/></w:rPr></w:style>"#,
        r#"<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/>"#,
        r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/>"#,
        r#"<w:outlineLvl w:val="0"/></w:pPr>"#,
        r#"<w:style w:type="paragraph" w:styleId="K-quote"><w:name w:val="인용"/>"#,
        r#"<w:lang w:val="ko-KR" w:eastAsia="ko-KR"/>"#,
    ] {
        assert!(st.contains(s), "styles.xml에 없음: {s}");
    }
    let core = part(&path, "docProps/core.xml");
    assert!(
        core.contains("<dc:title>홍길동 이력서</dc:title>")
            && core.contains("2026-11-13T06:10:45Z"),
        "{core}"
    );
}

#[cfg(target_os = "macos")]
fn textutil(path: &str, to: &str) -> String {
    tool("textutil", &["-convert", to, "-stdout", path])
}

/// 공백을 모두 뺀 글.
#[cfg(target_os = "macos")]
fn squeeze(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// X3: macOS의 textutil로 되읽은 글이 문서의 글과 같다(문단 차례, 표 칸의 글, 목록 항목의 글).
/// HTML로 바꾸면 굵게와 표가 살아 있다.
#[cfg(target_os = "macos")]
#[test]
fn x3_되읽은_글이_문서의_글과_같다() {
    let d = all_features();
    let (_, path) = write("x3.docx", &d, &read_photo);
    let back = squeeze(&textutil(&path, "txt"));
    // 문서의 글(목록 표지를 뺀 문단 글, 표는 칸마다)이 차례대로 나온다.
    let mut at = 0;
    for line in text(&d).lines() {
        for piece in line.split('\t') {
            let piece = piece.trim_start();
            let body = match piece.split_once(' ') {
                Some((m, rest))
                    if ["•", "◦", "▪"].contains(&m)
                        || m.ends_with('.')
                        || m.ends_with(')')
                        || m.chars().all(|c| ('\u{2460}'..='\u{24FF}').contains(&c)) =>
                {
                    rest
                }
                _ => piece,
            };
            let want = squeeze(body);
            let i = back[at..]
                .find(&want)
                .unwrap_or_else(|| panic!("되읽은 글에 없거나 차례가 다름: {want}\n{back}"));
            at += i + want.len();
        }
    }
    let html = textutil(&path, "html");
    assert!(html.contains("<table"), "표");
    // 링크는 textutil이 읽지 않는다(textutil이 HTML에서 만든 DOCX에도 링크가 없다). 링크는 x2에서 본다.
    assert!(
        html.contains("font-weight: bold") || html.contains("<b>"),
        "굵게"
    );
}

/// X5: 같은 문서 → 같은 바이트, 다른 문서 → 다른 바이트.
#[test]
fn x5_같은_문서는_같은_바이트() {
    let d = all_features();
    let a = export(&d, Some(1_000), &read_photo);
    let b = export(&d.clone(), Some(1_000), &read_photo);
    assert_eq!(a.bytes, b.bytes);
    let mut e = d.clone();
    apply(
        &mut e,
        vec![json!({"op": "text", "id": "p2", "at": 0, "ins": "가"})],
    );
    assert_ne!(export(&e, Some(1_000), &read_photo).bytes, a.bytes);
    assert_ne!(
        export(&d, Some(2_000), &read_photo).bytes,
        a.bytes,
        "고친 때가 다르면 다르다"
    );
}

/// X6: 특수한 글과 문서.
#[test]
fn x6_특수한_글() {
    let long = "가나다라마바사 Lorem ipsum ".repeat(5_000);
    let special = "<태그> & \"따옴표\" '홑' 😀👩‍👩‍👧 漢字 \u{1112}\u{1161}\u{11AB}글";
    let mut d = doc(&[
        json!({"runs": [{"t": special}]}),
        json!({"runs": [{"t": long.clone()}]}),
        json!({"runs": [{"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 10, "h": 10, "alt": "대체\u{1}글 <&>"}}]}),
    ]);
    // 이름과 스타일 이름에 든 제어 문자는 걸러 낸다.
    apply(
        &mut d,
        vec![
            json!({"op": "doc", "props": {"name": "이름\u{7}<&>"}}),
            json!({"op": "set", "id": "root", "props": {"styles": {"body": {"name": "본문"}, "x": {"name": "스\u{2}타일 \"&\""}}}}),
        ],
    );
    let (_, path) = write("x6.docx", &d, &read_photo);
    check_package(&path);
    let xml = part(&path, "word/document.xml");
    assert!(xml.contains(
        "&lt;태그&gt; &amp; &quot;따옴표&quot; '홑' 😀👩‍👩‍👧 漢字 \u{1112}\u{1161}\u{11AB}글"
    ));
    assert!(xml.contains(&long));
    assert!(xml.contains("descr=\"대체글 &lt;&amp;&gt;\""));
    assert!(part(&path, "docProps/core.xml").contains("<dc:title>이름&lt;&amp;&gt;</dc:title>"));
    assert!(part(&path, "word/styles.xml").contains("<w:name w:val=\"스타일 &quot;&amp;&quot;\"/>"));
    #[cfg(target_os = "macos")]
    {
        let back = textutil(&path, "txt");
        assert!(back.contains(special), "{back}");
        assert!(back.contains(long.trim_end()));
    }

    // 빈 문서, 표만 있는 문서, 그림만 있는 문서.
    let empty = Document::new(Kind::Flow, "빈 문서").unwrap();
    let (_, p) = write("x6-empty.docx", &empty, &no_assets);
    check_package(&p);
    assert!(part(&p, "word/document.xml").contains("<w:body><w:p></w:p><w:sectPr>"));

    let mut t = doc(&[]);
    apply(
        &mut t,
        table_ops(
            "t",
            json!({"cols": [100, 100]}),
            &[vec![(Some("가"), json!({})), (Some("나"), json!({}))]],
        ),
    );
    let (_, p) = write("x6-table.docx", &t, &no_assets);
    check_package(&p);
    let xml = part(&p, "word/document.xml");
    assert!(
        xml.contains("</w:tbl><w:p/><w:sectPr>"),
        "표 뒤에는 문단이 있어야 한다: {xml}"
    );

    let i = doc(&[
        json!({"runs": [{"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 900, "h": 300}}]}),
    ]);
    let (_, p) = write("x6-image.docx", &i, &read_photo);
    check_package(&p);
    // 본문 폭(595.28 - 2 × 56.69 = 481.9pt)보다 넓으면 비율을 지켜 줄인다.
    let xml = part(&p, "word/document.xml");
    assert!(
        xml.contains("<wp:extent cx=\"6120130\" cy=\"2040043\"/>"),
        "{xml}"
    );
}

/// X7: 옮기지 못한 것의 목록.
#[test]
fn x7_옮기지_못한_것의_목록이_맞다() {
    let d = doc(&[json!({"runs": [
        {"t": "반투명", "s": {"color": "#0A6DC280"}},
        {"t": "중간 굵기", "s": {"weight": 500}},
        {"t": "스크립트", "link": "javascript:alert(1)"},
        {"t": "고침", "mark": {"kind": "INS", "by": "ai"}},
        {"t": "\u{FFFC}", "o": {"type": "image", "asset": "b".repeat(64), "w": 10, "h": 10}},
        {"t": "\u{FFFC}", "o": {"type": "image", "asset": "c".repeat(64), "w": 10, "h": 10}},
    ]})]);
    let read = |h: &str| (h == "c".repeat(64)).then(|| b"GIF89a....".to_vec());
    let out = export(&d, None, &read);
    assert_eq!(
        out.lost,
        [
            "그림 2개(읽지 못했거나 PNG·JPEG가 아니라 뺌)",
            "링크 1개(열 수 없는 주소라 글로만)",
            "반투명 색 1곳(흰 바탕에 섞은 색으로)",
            "굵기 1곳(굵게나 보통으로)",
            "변경 표시 1곳(글로만)",
        ]
    );
    // 반투명 색은 흰 바탕에 섞은 색(알파 128/255)이다.
    let path = format!("{}/x7.docx", env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(&path, &out.bytes).unwrap();
    check_package(&path);
    let xml = part(&path, "word/document.xml");
    assert!(xml.contains(r#"<w:color w:val="84B6E0"/>"#), "{xml}");
    assert!(!xml.contains("javascript"));
    assert!(xml.contains("스크립트"));
    // 없는 문서.
    assert!(export(&all_features(), None, &read_photo).lost.is_empty());
    assert!(
        export(&Document::new(Kind::Flow, "빈").unwrap(), None, &no_assets)
            .lost
            .is_empty()
    );
}

/// X8: 텍스트. 문단, 목록, 표, 빈 문서. UTF-8, 줄 끝 `\n`.
#[test]
fn x8_텍스트() {
    let mut d = doc(&[
        json!({"style": "title", "runs": [{"t": "이력서"}]}),
        json!({"runs": [{"t": "한 줄\t탭"}]}),
        json!({"runs": [{"t": "첫째"}], "pf": {"list": {"kind": "ORDERED", "level": 0}}}),
        json!({"runs": [{"t": "가 항목"}], "pf": {"list": {"kind": "ORDERED", "level": 1}}}),
        json!({"runs": [{"t": "둘째"}], "pf": {"list": {"kind": "ORDERED", "level": 0}}}),
        json!({"runs": [{"t": "점"}], "pf": {"list": {"kind": "BULLET", "level": 0}}}),
        json!({"runs": [{"t": "끊긴 뒤"}], "pf": {"list": {"kind": "ORDERED", "level": 0}}}),
        json!({"runs": [{"t": "그림"}, {"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 1, "h": 1}}, {"t": " 뒤"}]}),
    ]);
    apply(
        &mut d,
        table_ops(
            "t",
            json!({"cols": [100, 100, 100]}),
            &[
                vec![
                    (Some("기간"), json!({})),
                    (Some("회사"), json!({"span": {"rows": 1, "cols": 2}})),
                    (None, json!({})),
                ],
                vec![
                    (Some("2024"), json!({})),
                    (Some("가"), json!({})),
                    (Some("나"), json!({})),
                ],
            ],
        ),
    );
    let t = text(&d);
    assert_eq!(
        t,
        "이력서\n한 줄\t탭\n1. 첫째\n  가. 가 항목\n2. 둘째\n• 점\n1. 끊긴 뒤\n그림 뒤\n기간\t회사\t\n2024\t가\t나\n"
    );
    assert!(std::str::from_utf8(t.as_bytes()).is_ok() && !t.contains('\r'));
    assert_eq!(text(&Document::new(Kind::Flow, "빈").unwrap()), "\n");
}

/// X10: 300쪽 문서를 1초 안에 내보낸다.
#[test]
fn x10_300쪽_문서를_1초_안에() {
    const S: [&str; 4] = [
        "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.",
        "Redis 캐시를 도입하면서 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.",
        "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.",
        "CI/CD 파이프라인을 GitHub Actions로 옮기고 배포 시간을 25분에서 7분으로 단축했습니다.",
    ];
    let paras: Vec<Value> = (0..4_000)
        .map(|i| {
            let t: Vec<&str> = (0..3).map(|k| S[(i + k) % 4]).collect();
            json!({"runs": [{"t": t.join(" ")}, {"t": " 굵게", "s": {"weight": 700}}]})
        })
        .collect();
    let d = doc(&paras);
    let mut l = kari_layout::Layouter::new(fonts());
    let pages = l.layout(&d, None).pages.len();
    assert!(pages >= 300, "{pages}쪽");
    let t = Instant::now();
    let out = export(&d, None, &no_assets);
    let docx = t.elapsed();
    let t = Instant::now();
    let txt = text(&d);
    let plain = t.elapsed();
    println!(
        "{pages}쪽: DOCX {docx:?} ({} 바이트), 텍스트 {plain:?} ({} 바이트)",
        out.bytes.len(),
        txt.len()
    );
    // 기준은 앱에 싣는 최적화 빌드의 것이다(최적화하지 않은 시험 빌드에서는 재기만 한다).
    if !cfg!(debug_assertions) {
        assert!(docx.as_secs_f64() < 1.0 && plain.as_secs_f64() < 1.0);
    }
}

fn fonts() -> kari_layout::Fonts {
    let mut f = kari_layout::Fonts::new();
    for name in ["Pretendard-Regular.ttf", "Pretendard-Bold.ttf"] {
        let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
        f.add(std::fs::read(&path).unwrap()).unwrap();
    }
    f
}

/// X4: 눈으로 보는 견본 이력서(제목, 링크, 목록, 사진이 든 표, 머리글·쪽 번호). 같은 문서의 PDF와 함께
/// `app/engine/target/kari-shots/21-resume.{docx,pdf,txt}`로 남긴다.
#[test]
fn x4_견본_이력서를_남긴다() {
    let mut d = doc_with(
        json!({"header": {"l": "홍길동", "r": "이력서"}, "footer": {"c": "{쪽} / {전체}"}}),
        &[
            json!({"style": "title", "runs": [{"t": "홍길동 · 백엔드 엔지니어"}]}),
            json!({"runs": [
                {"t": "github.com/example", "link": "https://github.com/example"},
                {"t": " · "},
                {"t": "hello@example.com", "link": "mailto:hello@example.com"},
                {"t": " · 010-1234-5678"},
            ]}),
        ],
    );
    let photo_run = json!([{"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 84, "h": 108, "alt": "증명사진"}}]);
    let head = json!({"fill": "#EEF2F7FF", "alignV": "CENTER"});
    let k = json!({"fill": "#F7F7F7FF"});
    let none = json!({});
    let mut ops = table_ops(
        "id",
        json!({"cols": [100, 80, 301.9]}),
        &[
            vec![
                (
                    Some(""),
                    json!({"span": {"rows": 3, "cols": 1}, "alignV": "CENTER"}),
                ),
                (Some("성명"), k.clone()),
                (Some("홍길동"), none.clone()),
            ],
            vec![
                (None, none.clone()),
                (Some("연락처"), k.clone()),
                (Some("010-1234-5678 · hello@example.com"), none.clone()),
            ],
            vec![
                (None, none.clone()),
                (Some("주소"), k.clone()),
                (Some("서울특별시 마포구 월드컵북로 00길 00"), none.clone()),
            ],
        ],
    );
    ops.push(json!({"op": "set", "id": "idp0_0", "props": {"runs": photo_run}}));
    ops.push(json!({"op": "set", "id": "idp0_2", "props": {"runs": [{"t": "홍길동", "s": {"weight": 700}}]}}));
    let h1 = |id: &str, t: &str| json!({"op": "add", "node": {"id": id, "type": "para", "parent": "root", "props": {"style": "h1", "runs": [{"t": t}]}}});
    let item = |id: &str, kind: &str, lv: u8, t: &str| {
        json!({"op": "add", "node": {"id": id, "type": "para", "parent": "root",
        "props": {"runs": [{"t": t}], "pf": {"list": {"kind": kind, "level": lv}}}}})
    };
    ops.push(h1("h_sum", "요약"));
    ops.push(item(
        "l1",
        "BULLET",
        0,
        "결제·주문 도메인 백엔드 7년, 트래픽 피크 대응과 장애 복구를 주도했습니다.",
    ));
    ops.push(item(
        "l2",
        "BULLET",
        0,
        "Kafka, Redis, PostgreSQL, Kubernetes 운영 경험.",
    ));
    ops.push(item(
        "l3",
        "BULLET",
        1,
        "p95 지연 1.2초 → 300ms, 배포 시간 25분 → 7분.",
    ));
    ops.push(h1("h_edu", "학력"));
    ops.extend(table_ops(
        "edu",
        json!({"cols": [110, 150, 130, 91.9], "headerRows": 1}),
        &[
            ["기간", "학교", "전공", "구분"]
                .iter()
                .map(|t| (Some(*t), head.clone()))
                .collect(),
            ["2016.03 – 2020.02", "한국대학교", "컴퓨터공학", "졸업"]
                .iter()
                .map(|t| (Some(*t), none.clone()))
                .collect(),
        ],
    ));
    ops.push(h1("h_job", "경력"));
    ops.push(json!({"op": "add", "node": {"id": "j1", "type": "para", "parent": "root",
        "props": {"runs": [{"t": "클로나믹 · 백엔드 리드\t2021.03 – 현재", "s": {"weight": 700}}], "pf": {"tabs": [{"pos": 481.9, "align": "RIGHT"}]}}}}));
    for (i, t) in [
        "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.",
        "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.",
        "CI/CD 파이프라인을 GitHub Actions로 옮기고 배포 시간을 25분에서 7분으로 단축했습니다.",
    ]
    .iter()
    .enumerate()
    {
        ops.push(item(&format!("k{i}"), "ORDERED", 0, t));
    }
    ops.push(json!({"op": "add", "node": {"id": "end", "type": "para", "parent": "root", "props": {"runs": [{"t": "위 내용은 사실과 다름없습니다."}], "pf": {"align": "RIGHT", "space": {"before": 18}}}}}));
    apply(&mut d, ops);

    let face = photo(300, 400, skia_safe::EncodedImageFormat::JPEG);
    let read = move |_: &str| Some(face.clone());
    let out = export(&d, Some(1_794_550_245_000), &read);
    assert!(out.lost.is_empty(), "{:?}", out.lost);
    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let docx = format!("{dir}/21-resume.docx");
    std::fs::write(&docx, &out.bytes).unwrap();
    check_package(&docx);
    std::fs::write(format!("{dir}/21-resume.txt"), text(&d)).unwrap();
    // 같은 문서의 PDF(화면과 같은 배치).
    let mut l = kari_layout::Layouter::new(fonts());
    let lay = l.layout(&d, None).clone();
    let meta = kari_pdf::Meta {
        title: d.name(),
        lang: "ko",
    };
    let pdf = kari_pdf::export(&d, &lay, l.fonts(), &meta, &read).unwrap();
    std::fs::write(format!("{dir}/21-resume.pdf"), pdf).unwrap();
}

/// 칸 하나(`tc`)의 XML. `text`가 든 칸.
fn tc_of<'a>(xml: &'a str, text: &str) -> &'a str {
    let at = xml.find(&format!(">{text}<")).unwrap();
    let start = xml[..at].rfind("<w:tc>").unwrap();
    &xml[start..at]
}

/// 칸마다의 변(배치와 같은 규칙으로 정한 선), 표 맞춤, 행 최소 높이가 DOCX로 간다.
#[test]
fn x11_표의_변_맞춤_행_높이() {
    let red = json!({"weight": 2, "color": "#C00000FF"});
    let blue = json!({"weight": 1, "color": "#1155CCFF"});
    let none = json!({"weight": 0, "color": "#000000FF"});
    let mut d = doc(&[json!({"runs": [{"t": "앞"}]})]);
    let mut ops = table_ops(
        "t",
        json!({"cols": [100, 100, 100], "align": "CENTER"}),
        &[
            vec![
                (Some("가"), json!({"border": {"bottom": red.clone()}})),
                (Some("나"), json!({"border": {"right": none}})),
                (Some("다"), json!({"span": {"rows": 2, "cols": 1}})),
            ],
            vec![
                (Some("라"), json!({"border": {"top": blue}})),
                (Some("마"), json!({})),
                (None, json!({})),
            ],
        ],
    );
    ops.push(json!({"op": "set", "id": "tr0", "props": {"h": 30}}));
    apply(&mut d, ops);
    let (out, path) = write("x11.docx", &d, &no_assets);
    assert!(out.lost.is_empty(), "{:?}", out.lost);
    check_package(&path);
    let xml = part(&path, "word/document.xml");
    assert!(xml.contains(r#"<w:tblW w:w="6000" w:type="dxa"/><w:jc w:val="center"/>"#));
    assert!(xml.contains(r#"<w:trHeight w:val="600" w:hRule="atLeast"/>"#));
    let thick = r#"w:val="single" w:sz="16" w:space="0" w:color="C00000""#;
    let gray = r#"w:val="single" w:sz="4" w:space="0" w:color="8C8C8C""#;
    // 둘이 다르면 굵은 쪽: "가"의 아래와 "라"의 위가 같은 빨강 2pt.
    assert!(tc_of(&xml, "가").contains(&format!("<w:bottom {thick}/>")));
    assert!(tc_of(&xml, "라").contains(&format!("<w:top {thick}/>")));
    // 선 없음은 nil. 여러 행에 걸친 "다"의 왼쪽은 맞은편 구간(nil, 기본 선) 가운데 굵은 쪽이다.
    assert!(tc_of(&xml, "나").contains(r#"<w:right w:val="nil"/>"#));
    assert!(tc_of(&xml, "다").contains(&format!("<w:left {gray}/>")));
    assert!(tc_of(&xml, "마").contains(&format!("<w:top {gray}/>")));
    // 여러 행에 걸친 칸의 조각 사이에는 선이 없다.
    let first = tc_of(&xml, "다");
    assert!(
        first.contains(r#"<w:vMerge w:val="restart"/>"#)
            && first.contains(r#"<w:bottom w:val="nil"/>"#)
    );
    // 칸마다 변을 정하지 않은 표는 칸에 변을 적지 않는다(예전 출력 그대로).
    let plain = doc(&[json!({"runs": [{"t": "앞"}]})]);
    let mut plain = plain;
    apply(
        &mut plain,
        table_ops(
            "t",
            json!({"cols": [100]}),
            &[vec![(Some("가"), json!({}))]],
        ),
    );
    let (_, path) = write("x11-plain.docx", &plain, &no_assets);
    let xml = part(&path, "word/document.xml");
    assert!(!xml.contains("<w:tcBorders>") && !xml.contains("<w:jc w:val=\"center\"/></w:tblPr>"));
}

/// 문단의 테두리(변마다 굵기·색·글과의 거리)와 바탕이 직접 서식과 스타일 모두에서 DOCX로 간다.
/// 스타일의 선·바탕을 문단에서 끈 것, 가로줄, 기본 스타일(부제, 제목 3)의 Word 이름도 본다.
#[test]
fn x12_문단의_테두리와_바탕() {
    let side = |w: f64, c: &str, g: f64| json!({"weight": w, "color": c, "gap": g});
    let blue = "#336699FF";
    let d = doc_with(
        json!({"styles": {"boxed": {"name": "상자", "based": "body", "pf": {
            "border": {"top": side(1.0, blue, 2.0), "bottom": side(1.0, blue, 2.0),
                "left": side(0.5, blue, 4.0), "right": side(0.5, blue, 4.0)},
            "shade": "#EEF2F7FF"}}}}),
        &[
            json!({"style": "boxed", "runs": [{"t": "상자 글"}]}),
            // 스타일의 위 선과 바탕을 문단에서 끈다.
            json!({"style": "boxed", "runs": [{"t": "끈 글"}], "pf": {"border": {"top": {"weight": 0}}, "shade": "#00000000"}}),
            // 색이 없으면 본문 기본색, 굵기와 거리는 위쪽 끝, 반투명 바탕.
            json!({"runs": [{"t": "직접"}], "pf": {"border": {"left": {"weight": 6, "gap": 31}}, "shade": "#FF000080"}}),
            // 가로줄: 아래 선만 있는 빈 문단.
            json!({"runs": [{"t": ""}], "pf": {"border": {"bottom": side(0.5, "#1E1E1EFF", 1.0)}}}),
            json!({"style": "subtitle", "runs": [{"t": "부제 글"}]}),
            json!({"style": "h3", "runs": [{"t": "제목 3 글"}]}),
        ],
    );
    let (out, path) = write("x12.docx", &d, &no_assets);
    assert_eq!(out.lost, ["반투명 색 1곳(흰 바탕에 섞은 색으로)"]);
    check_package(&path);
    let line = |s: &str, sz: u32, space: u32, color: &str| {
        format!(r#"<w:{s} w:val="single" w:sz="{sz}" w:space="{space}" w:color="{color}"/>"#)
    };
    let st = part(&path, "word/styles.xml");
    let boxed = format!(
        r#"<w:pBdr>{}{}{}{}</w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="EEF2F7"/>"#,
        line("top", 8, 2, "336699"),
        line("left", 4, 4, "336699"),
        line("bottom", 8, 2, "336699"),
        line("right", 4, 4, "336699")
    );
    assert!(
        st.contains(&format!(r#"<w:style w:type="paragraph" w:styleId="K-boxed"><w:name w:val="상자"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr>{boxed}"#)),
        "{st}"
    );
    // 저장된 스타일 표에 없는 기본 스타일도 Word의 이름으로 나간다.
    for s in [
        r#"<w:style w:type="paragraph" w:styleId="Subtitle"><w:name w:val="Subtitle"/>"#,
        r#"<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/>"#,
        r#"<w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/>"#,
    ] {
        assert!(st.contains(s), "styles.xml에 없음: {s}");
    }
    let xml = part(&path, "word/document.xml");
    let has = |s: &str| assert!(xml.contains(s), "document.xml에 없음: {s}\n{xml}");
    has(r#"<w:p><w:pPr><w:pStyle w:val="K-boxed"/></w:pPr><w:r><w:t xml:space="preserve">상자 글"#);
    has(&format!(
        r#"<w:pStyle w:val="K-boxed"/><w:pBdr><w:top w:val="nil"/>{}{}{}</w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="auto"/></w:pPr>"#,
        line("left", 4, 4, "336699"),
        line("bottom", 8, 2, "336699"),
        line("right", 4, 4, "336699")
    ));
    has(&format!(
        r#"<w:pPr><w:pBdr><w:top w:val="nil"/>{}<w:bottom w:val="nil"/><w:right w:val="nil"/></w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="FF7F7F"/></w:pPr>"#,
        line("left", 48, 31, "1E1E1E")
    ));
    has(&format!(
        r#"<w:p><w:pPr><w:pBdr><w:top w:val="nil"/><w:left w:val="nil"/>{}<w:right w:val="nil"/></w:pBdr></w:pPr></w:p>"#,
        line("bottom", 4, 1, "1E1E1E")
    ));
    has(r#"<w:pStyle w:val="Subtitle"/>"#);
    has(r#"<w:pStyle w:val="Heading3"/>"#);
    // 글은 그대로다.
    #[cfg(target_os = "macos")]
    assert_eq!(
        squeeze(&textutil(&path, "txt")),
        squeeze("상자 글 끈 글 직접 부제 글 제목 3 글")
    );
}

/// 문서 스키마의 값 자리마다, 그 값만 다른 두 문서를 내보내 DOCX가 달라지거나 `lost`에 드는지 본다.
/// 스키마에 새 값 자리가 생기면 아래 표에 없어 실패한다. 내보내기가 그 값을 옮기거나 `lost`에 넣고 표에 더한다.
#[test]
fn x13_스키마의_모든_속성이_옮겨지거나_목록에_든다() {
    // 다른 값에서 정해지거나 배치에도 쓰이지 않아 DOCX에 따로 쓸 것이 없는 자리와 그 까닭.
    let same = [
        ("flow.page.preset", "용지 이름. 크기는 w, h로 옮긴다"),
        ("flow.page.orientation", "w, h에서 정해진다"),
        ("row.keep", "배치처럼 행을 늘 쪽에서 나누지 않는다"),
    ];
    let cs = [
        ("font", json!("Pretendard"), json!("Noto Serif KR")),
        ("size", json!(11), json!(14)),
        ("weight", json!(400), json!(700)),
        ("italic", json!(false), json!(true)),
        ("underline", json!(false), json!(true)),
        ("strike", json!(false), json!(true)),
        ("color", json!("#000000FF"), json!("#C00000FF")),
        ("bg", json!("#FFEB3BFF"), json!("#B3E5FCFF")),
        ("vertical", json!("NONE"), json!("SUPER")),
        ("spacing", json!(0), json!(1)),
    ];
    let mut pf: Vec<(String, Value, Value)> = [
        ("shade", json!("#EEEEEEFF"), json!("#DDDDDDFF")),
        ("align", json!("LEFT"), json!("CENTER")),
        ("indent.left", json!(0), json!(10)),
        ("indent.right", json!(0), json!(10)),
        ("indent.first", json!(0), json!(10)),
        ("space.before", json!(0), json!(10)),
        ("space.after", json!(0), json!(10)),
        ("line.unit", json!("PERCENT"), json!("PT")),
        ("line.value", json!(150), json!(200)),
        ("keepNext", json!(false), json!(true)),
        ("keepLines", json!(false), json!(true)),
        ("breakBefore", json!(false), json!(true)),
        ("tabs[].pos", json!(10), json!(20)),
        ("tabs[].align", json!("LEFT"), json!("RIGHT")),
        ("tabs[].leader", json!("NONE"), json!("DOT")),
        ("list.kind", json!("BULLET"), json!("ORDERED")),
        ("list.level", json!(0), json!(1)),
        ("list.start", json!(1), json!(5)),
    ]
    .into_iter()
    .map(|(k, a, b)| (k.to_string(), a, b))
    .collect();
    for s in ["top", "bottom", "left", "right"] {
        pf.push((format!("border.{s}.weight"), json!(1), json!(2)));
        pf.push((
            format!("border.{s}.color"),
            json!("#000000FF"),
            json!("#C00000FF"),
        ));
        pf.push((format!("border.{s}.gap"), json!(0), json!(3)));
    }
    let a = "a".repeat(64);
    let mut cases: Vec<(String, Value, Value)> = [
        ("flow.page.w", json!(595.28), json!(612)),
        ("flow.page.h", json!(841.89), json!(792)),
        ("flow.page.margin.t", json!(56.69), json!(40)),
        ("flow.page.margin.r", json!(56.69), json!(40)),
        ("flow.page.margin.b", json!(56.69), json!(40)),
        ("flow.page.margin.l", json!(56.69), json!(40)),
        ("flow.lang", json!("ko"), json!("en")),
        ("flow.header.l", json!(""), json!("왼쪽")),
        ("flow.header.c", json!("머리"), json!("가운데")),
        ("flow.header.r", json!(""), json!("오른쪽")),
        ("flow.footer.l", json!(""), json!("왼쪽")),
        ("flow.footer.c", json!(""), json!("{쪽}")),
        ("flow.footer.r", json!(""), json!("오른쪽")),
        ("flow.hfSkipFirst", json!(false), json!(true)),
        ("flow.styles.*.name", json!("사용자"), json!("다른 이름")),
        ("flow.styles.*.based", json!("body"), json!("title")),
        ("flow.styles.*.next", json!("body"), json!("title")),
        ("para.style", json!("body"), json!("h1")),
        ("para.runs[].t", json!("글"), json!("글자")),
        (
            "para.runs[].link",
            json!("https://example.com/a"),
            json!("https://example.com/b"),
        ),
        ("para.runs[].o.type", json!("image"), json!("field")),
        ("para.runs[].o.asset", json!(a), json!("b".repeat(64))),
        ("para.runs[].o.w", json!(10), json!(20)),
        ("para.runs[].o.h", json!(10), json!(20)),
        ("para.runs[].o.alt", json!("사진"), json!("증명사진")),
        ("para.runs[].o.name", json!("page"), json!("pages")),
        ("para.runs[].mark.kind", json!("INS"), json!("DEL")),
        ("para.runs[].mark.by", json!("user"), json!("ai")),
        ("para.runs[].mark.id", json!("m1"), json!("m2")),
        ("table.cols[]", json!(100), json!(200)),
        ("table.headerRows", json!(0), json!(1)),
        ("table.align", json!("LEFT"), json!("CENTER")),
        ("table.border.color", json!("#000000FF"), json!("#C00000FF")),
        ("table.border.weight", json!(1), json!(2)),
        ("row.h", json!(0), json!(30)),
        ("cell.span.rows", json!(1), json!(2)),
        ("cell.span.cols", json!(1), json!(2)),
        ("cell.covered", json!(false), json!(true)),
        ("cell.fill", json!("#FFFFFF00"), json!("#EEF2F7FF")),
        ("cell.pad[]", json!(4), json!(8)),
        (
            "cell.border",
            json!({}),
            json!({"bottom": {"weight": 2, "color": "#C00000FF"}}),
        ),
        ("cell.alignV", json!("TOP"), json!("CENTER")),
    ]
    .into_iter()
    .map(|(k, a, b)| (k.to_string(), a, b))
    .collect();
    for (k, a, b) in &cs {
        cases.push((format!("flow.styles.*.cs.{k}"), a.clone(), b.clone()));
        cases.push((format!("para.runs[].s.{k}"), a.clone(), b.clone()));
    }
    for (k, a, b) in &pf {
        cases.push((format!("flow.styles.*.pf.{k}"), a.clone(), b.clone()));
        cases.push((format!("para.pf.{k}"), a.clone(), b.clone()));
    }

    // 표에 있는 자리와 스키마의 자리가 같아야 한다.
    let known: BTreeSet<String> = cases
        .iter()
        .map(|c| c.0.clone())
        .chain(same.iter().map(|s| s.0.to_string()))
        .collect();
    let schema: BTreeSet<String> = kari_doc::prop_paths(Kind::Flow).into_iter().collect();
    let new: Vec<_> = schema.difference(&known).collect();
    let gone: Vec<_> = known.difference(&schema).collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "DOCX 내보내기가 아직 모르는 스키마 속성: {new:?}, 스키마에 없는 표의 속성: {gone:?}"
    );

    // 그림은 이름마다 다른 PNG.
    let read = |h: &str| (h.len() == 64).then(|| png(if h.starts_with('a') { 4 } else { 5 }, 4));
    let base = placed(None);
    assert!(export(&base, None, &read).lost.is_empty());
    let mut silent = Vec::new();
    for (path, a, b) in &cases {
        let (x, y) = (
            export(&placed(Some((path.as_str(), a))), None, &read),
            export(&placed(Some((path.as_str(), b))), None, &read),
        );
        if x.bytes == y.bytes && y.lost.is_empty() {
            silent.push(path.as_str());
        }
    }
    assert!(
        silent.is_empty(),
        "DOCX에 옮겨지지도 lost에 들지도 않는 속성: {silent:?}"
    );
}

/// [x13]의 문서: 스타일 `k`를 쓰는 문단 `p0`과 2×2 표 `t`. `leaf`가 있으면 그 자리(`para.pf.align` 꼴)에 값을 넣는다.
/// 값 자리에 꼭 있어야 하는 형제 값(변의 굵기, 목록의 종류 등)은 함께 채운다.
fn placed(leaf: Option<(&str, &Value)>) -> Document {
    let mut d = doc_with(
        json!({"header": {"c": "머리"}, "styles": {"k": {"name": "사용자", "based": "body"}}}),
        &[json!({"style": "k", "runs": [{"t": "글"}]})],
    );
    apply(
        &mut d,
        table_ops(
            "t",
            json!({"cols": [100, 100]}),
            &[
                vec![(Some("가"), json!({})), (Some("나"), json!({}))],
                vec![(Some("다"), json!({})), (Some("라"), json!({}))],
            ],
        ),
    );
    let Some((path, v)) = leaf else { return d };
    let (ty, rest) = path.split_once('.').unwrap();
    let segs: Vec<&str> = rest
        .split('.')
        .flat_map(|s| match s.strip_suffix("[]") {
            Some(n) => vec![n, "[]"],
            None => vec![s],
        })
        .collect();
    let mut prop = match (ty, segs[0]) {
        ("flow", "page") => {
            json!({"w": 595.28, "h": 841.89, "margin": {"t": 56.69, "r": 56.69, "b": 56.69, "l": 56.69}})
        }
        ("flow", "header") => json!({"c": "머리"}),
        ("flow", "styles") => json!({"k": {"name": "사용자", "based": "body"}}),
        ("para", "runs") => match segs.get(2) {
            Some(&"o") if segs[3] == "name" => {
                json!([{"t": "\u{FFFC}", "o": {"type": "field", "name": "page"}}])
            }
            Some(&"o") => {
                json!([{"t": "\u{FFFC}", "o": {"type": "image", "asset": "a".repeat(64), "w": 10, "h": 10, "name": "page"}}])
            }
            Some(&"mark") => json!([{"t": "글", "mark": {"kind": "INS", "by": "user"}}]),
            _ => json!([{"t": "글"}]),
        },
        ("table", "cols") => json!([100, 100]),
        ("table", "border") => json!({"weight": 1, "color": "#000000FF"}),
        ("cell", "pad") => json!([4, 6, 4, 6]),
        ("cell", "span") => json!({"rows": 1, "cols": 1}),
        _ => json!({}),
    };
    if segs.len() == 1 {
        prop = v.clone();
    } else {
        let mut at = &mut prop;
        let (last, inner) = segs[1..].split_last().unwrap();
        for s in inner {
            at = match *s {
                "[]" => {
                    if at.as_array().is_none_or(Vec::is_empty) {
                        *at = json!([{}]);
                    }
                    &mut at[0]
                }
                "*" => &mut at["k"],
                k => {
                    if at.get(k).is_none() {
                        at[k] = json!({});
                    }
                    &mut at[k]
                }
            };
        }
        let parent = segs[..segs.len() - 1]
            .iter()
            .rev()
            .find(|s| **s != "[]")
            .copied()
            .unwrap_or("");
        let need = match parent {
            "top" | "bottom" | "left" | "right" => json!({"weight": 1}),
            "list" => json!({"kind": "ORDERED"}),
            "line" => json!({"unit": "PERCENT", "value": 150}),
            "tabs" => json!({"pos": 10}),
            _ => json!({}),
        };
        for (k, n) in need.as_object().unwrap() {
            if at.get(k).is_none() {
                at[k] = n.clone();
            }
        }
        match *last {
            "[]" => at[0] = v.clone(),
            k => at[k] = v.clone(),
        }
    }
    let id = match ty {
        "flow" => "root",
        "para" => "p0",
        "table" => "t",
        "row" => "tr0",
        _ => "tc0_0",
    };
    apply(
        &mut d,
        vec![json!({"op": "set", "id": id, "props": {segs[0]: prop}})],
    );
    d
}
