//! PDF 내보내기의 확인 항목과 오류. poppler(pdftotext, pdffonts, pdfinfo)와
//! qpdf로 검사한다. 도구가 없으면 건너뛰지 않고 실패한다.

use std::collections::HashMap;
use std::process::Command;

use kari_doc::serde_json::{self, json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Item, Layout, Layouter};
use kari_pdf::{export, Error, Meta};

const META: Meta = Meta {
    title: "홍길동 이력서",
    lang: "ko",
};

/// 줄이 넘어가는 문단과 여러 쪽을 만드는 말뭉치.
const SENTENCES: [&str; 6] = [
    "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.",
    "Redis 캐시를 도입하면서 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.",
    "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.",
    "트랜잭션 격리 수준을 READ COMMITTED에서 REPEATABLE READ로 바꾼 이유와 부작용을 검토했습니다.",
    "레거시 코드의 테스트 커버리지를 31%에서 74%로 끌어올렸습니다. 값·괄호(CDC)·따옴표 “인용”도 넣습니다.",
    "CI/CD 파이프라인을 GitHub Actions로 옮기고 배포 시간을 25분에서 7분으로 단축했습니다.",
];

fn font_bytes(name: &str) -> Vec<u8> {
    let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn fonts() -> Fonts {
    let mut f = Fonts::new();
    f.add(font_bytes("Pretendard-Regular.ttf")).unwrap();
    f.add(font_bytes("Pretendard-Bold.ttf")).unwrap();
    f
}

fn apply(d: &mut Document, ops: Value) {
    d.apply(&ops, Guard::default()).unwrap();
}

/// 문단 `p0`, `p1`, …을 가진 문서. `paras`는 문단마다의 props.
fn doc(paras: &[Value]) -> Document {
    let mut d = Document::new(Kind::Flow, "시험").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    for (i, p) in paras.iter().enumerate() {
        ops.push(json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root", "props": p}}));
    }
    apply(&mut d, Value::Array(ops));
    d
}

/// 한글, 영문, 숫자, 굵게, 기울임, 링크, 반투명 색, 밑줄과 바탕, 합자·문맥 대체, 풀어 쓴 한글, 탭,
/// 줄이 넘어가는 문단을 넣은 여러 쪽짜리 문서.
fn rich() -> Document {
    let mut paras = vec![
        json!({"style": "title", "runs": [{"t": "홍길동 · 백엔드 엔지니어"}]}),
        json!({"runs": [
            {"t": "github.com/example", "link": "https://github.com/example"},
            {"t": " · "},
            {"t": "hello@example.com", "link": "mailto:hello@example.com"},
        ]}),
        json!({"runs": [
            {"t": "굵게 쓴 글 Bold 2024 ", "s": {"weight": 700}},
            {"t": "기울인 글 Italic 31%", "s": {"italic": true}},
            {"t": " 반투명 색", "s": {"color": "#0A6DC280"}},
            {"t": " 밑줄과 바탕", "s": {"underline": true, "bg": "#FFEB3BFF"}},
            {"t": " 기울인 링크", "s": {"italic": true}, "link": "https://example.com/a"},
        ]}),
        json!({"runs": [{"t": "합자와 문맥 대체: a -> b => c, 풀어 쓴 한글 \u{1112}\u{1161}\u{11AB}글, 탭\t다음"}]}),
    ];
    for i in 0..40 {
        let t: Vec<&str> = (0..3)
            .map(|k| SENTENCES[(i + k) % SENTENCES.len()])
            .collect();
        paras.push(json!({"runs": [{"t": t.join(" ")}]}));
    }
    doc(&paras)
}

fn layout(d: &Document) -> (Layouter, Layout) {
    let mut l = Layouter::new(fonts());
    let out = l.layout(d, None).clone();
    (l, out)
}

/// 문서를 배치해 PDF로 만들고 `CARGO_TARGET_TMPDIR/name`에 쓴다.
fn write(name: &str, d: &Document) -> (Layout, String) {
    let (l, out) = layout(d);
    let pdf = export(d, &out, l.fonts(), &META, &|_| None).unwrap();
    let path = format!("{}/{name}", env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(&path, pdf).unwrap();
    (out, path)
}

/// 도구를 돌려 표준 출력을 받는다. 없거나 실패하면 시험이 실패한다.
fn tool(cmd: &str, args: &[&str]) -> String {
    let out = Command::new(cmd).args(args).output().unwrap_or_else(|e| {
        panic!("{cmd}를 실행하지 못했습니다(poppler-utils와 qpdf가 있어야 합니다): {e}")
    });
    assert!(
        out.status.success(),
        "{cmd} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// `pdfinfo`의 "이름: 값" 줄의 값.
fn field<'a>(info: &'a str, key: &str) -> Option<&'a str> {
    info.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
        .map(str::trim)
}

fn squeeze(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 문서의 문단 글을 차례로 이은 것.
fn source(d: &Document) -> String {
    d.children(Some("root"))
        .iter()
        .filter_map(|n| d.text(&n.id))
        .collect()
}

// ---- qpdf로 PDF 구조 읽기 ----

/// `qpdf --json`의 객체들.
struct Pdf {
    path: String,
    objs: Value,
}

impl Pdf {
    fn open(path: &str) -> Pdf {
        let j: Value = serde_json::from_str(&tool("qpdf", &["--json=2", path])).unwrap();
        Pdf {
            path: path.to_string(),
            objs: j["qpdf"][1].clone(),
        }
    }

    /// 참조("N 0 R")면 그 객체(스트림이면 사전), 아니면 그대로.
    fn get<'a>(&'a self, v: &'a Value) -> &'a Value {
        match v.as_str().filter(|s| s.ends_with(" R")) {
            Some(r) => {
                let o = &self.objs[format!("obj:{r}")];
                if o["stream"].is_object() {
                    &o["stream"]["dict"]
                } else {
                    &o["value"]
                }
            }
            None => v,
        }
    }

    fn root(&self) -> &Value {
        self.get(&self.objs["trailer"]["value"]["/Root"])
    }

    /// 쪽 사전들(쪽 나무가 한 단이라고 본다. krilla가 그렇게 쓴다).
    fn pages(&self) -> Vec<&Value> {
        let kids = self.get(&self.root()["/Pages"])["/Kids"]
            .as_array()
            .unwrap();
        kids.iter().map(|k| self.get(k)).collect()
    }

    /// 쪽의 내용 스트림을 푼 바이트.
    fn content(&self, page: &Value) -> Vec<u8> {
        let refs = match &page["/Contents"] {
            Value::Array(a) => a.clone(),
            v => vec![v.clone()],
        };
        let mut out = Vec::new();
        for r in refs {
            let n = r.as_str().unwrap().split(' ').next().unwrap().to_string();
            let o = Command::new("qpdf")
                .args([
                    &format!("--show-object={n}"),
                    "--filtered-stream-data",
                    &self.path,
                ])
                .output()
                .expect("qpdf를 실행하지 못했습니다");
            assert!(o.status.success());
            out.extend(o.stdout);
            out.push(b'\n');
        }
        out
    }

    /// 쪽 자원의 글꼴 이름 → (CID → 폭(1/1000 em), 기본 폭).
    fn widths(&self, page: &Value) -> HashMap<String, (HashMap<u32, f64>, f64)> {
        let fonts = self.get(&self.get(&page["/Resources"])["/Font"]);
        let mut out = HashMap::new();
        for (name, f) in fonts.as_object().unwrap() {
            let f = self.get(f);
            assert_eq!(f["/Subtype"], "/Type0", "CID 글꼴만 읽는다");
            let cid = self.get(&f["/DescendantFonts"][0]);
            let w = self.get(&cid["/W"]).as_array().cloned().unwrap_or_default();
            let mut list = HashMap::new();
            let mut i = 0;
            while i < w.len() {
                let first = w[i].as_u64().unwrap() as u32;
                if let Some(arr) = w[i + 1].as_array() {
                    for (k, v) in arr.iter().enumerate() {
                        list.insert(first + k as u32, v.as_f64().unwrap());
                    }
                    i += 2;
                } else {
                    let last = w[i + 1].as_u64().unwrap() as u32;
                    let v = w[i + 2].as_f64().unwrap();
                    list.extend((first..=last).map(|c| (c, v)));
                    i += 3;
                }
            }
            let dw = cid["/DW"].as_f64().unwrap_or(1000.0);
            out.insert(name.trim_start_matches('/').to_string(), (list, dw));
        }
        out
    }
}

// ---- 내용 스트림을 다시 계산해 글리프 원점 얻기 ----

#[derive(Clone, Debug)]
enum Tok {
    Num(f64),
    Name(String),
    Str(Vec<u8>),
    Arr(Vec<Tok>),
    Dict,
    Op(String),
}

fn delim(b: u8) -> bool {
    b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b)
}

fn lex(s: &[u8]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut dict = 0;
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let tok = match b {
            b'%' => {
                while i < s.len() && s[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'[' => {
                stack.push(out.len());
                i += 1;
                continue;
            }
            b']' => {
                let at = stack.pop().unwrap();
                let items = out.split_off(at);
                i += 1;
                Tok::Arr(items)
            }
            b'<' if s.get(i + 1) == Some(&b'<') => {
                dict += 1;
                i += 2;
                continue;
            }
            b'>' if s.get(i + 1) == Some(&b'>') => {
                dict -= 1;
                i += 2;
                if dict == 0 {
                    out.push(Tok::Dict);
                }
                continue;
            }
            b'<' => {
                let end = i + s[i..].iter().position(|&c| c == b'>').unwrap();
                let hex: Vec<u8> = s[i + 1..end]
                    .iter()
                    .copied()
                    .filter(u8::is_ascii_hexdigit)
                    .collect();
                i = end + 1;
                Tok::Str(
                    hex.chunks(2)
                        .map(|c| {
                            let t = std::str::from_utf8(c).unwrap();
                            u8::from_str_radix(&format!("{t:0<2}"), 16).unwrap()
                        })
                        .collect(),
                )
            }
            b'(' => {
                let mut v = Vec::new();
                let mut depth = 1;
                i += 1;
                loop {
                    let c = s[i];
                    i += 1;
                    match c {
                        b'\\' => {
                            let e = s[i];
                            i += 1;
                            match e {
                                b'n' => v.push(b'\n'),
                                b'r' => v.push(b'\r'),
                                b't' => v.push(b'\t'),
                                b'b' => v.push(8),
                                b'f' => v.push(12),
                                b'0'..=b'7' => {
                                    let mut n = u32::from(e - b'0');
                                    for _ in 0..2 {
                                        if matches!(s[i], b'0'..=b'7') {
                                            n = n * 8 + u32::from(s[i] - b'0');
                                            i += 1;
                                        }
                                    }
                                    v.push(n as u8);
                                }
                                b'\n' => {}
                                e => v.push(e),
                            }
                        }
                        b'(' => {
                            depth += 1;
                            v.push(c);
                        }
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            v.push(c);
                        }
                        c => v.push(c),
                    }
                }
                Tok::Str(v)
            }
            b'/' => {
                let start = i + 1;
                i += 1;
                while i < s.len() && !delim(s[i]) {
                    i += 1;
                }
                Tok::Name(String::from_utf8_lossy(&s[start..i]).into_owned())
            }
            _ => {
                let start = i;
                while i < s.len() && !delim(s[i]) {
                    i += 1;
                }
                let w = std::str::from_utf8(&s[start..i]).unwrap();
                match w.parse::<f64>() {
                    Ok(n) => Tok::Num(n),
                    Err(_) => Tok::Op(w.to_string()),
                }
            }
        };
        if dict == 0 {
            out.push(tok);
        }
    }
    out
}

type M = [f64; 6];

/// `a` 다음에 `b`를 적용하는 행렬.
fn mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// 쪽 하나의 글리프 원점들(쪽 왼쪽 위 기준 pt), 그린 차례대로.
fn origins(pdf: &Pdf, page: &Value, height: f64) -> Vec<(f64, f64)> {
    let widths = pdf.widths(page);
    let width = |font: &str, cid: u32| -> f64 {
        let (list, dw) = &widths[font];
        list.get(&cid).copied().unwrap_or(*dw)
    };
    let id: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let (mut ctm, mut saved, mut tm) = (id, Vec::new(), id);
    let (mut font, mut size) = (String::new(), 0.0);
    let mut args: Vec<Tok> = Vec::new();
    let mut out = Vec::new();
    let nums = |a: &[Tok]| -> M {
        let v: Vec<f64> = a
            .iter()
            .map(|t| match t {
                Tok::Num(n) => *n,
                t => panic!("수가 아닙니다: {t:?}"),
            })
            .collect();
        v[v.len() - 6..].try_into().unwrap()
    };
    for t in lex(&pdf.content(page)) {
        let Tok::Op(op) = t else {
            args.push(t);
            continue;
        };
        let mut show = |s: &[u8], tm: &mut M| {
            for c in s.chunks(2) {
                let m = mul(tm, &ctm);
                out.push((m[4], height - m[5]));
                let cid = u32::from(c[0]) << 8 | u32::from(c[1]);
                let tx = width(&font, cid) / 1000.0 * size;
                *tm = mul(&[1.0, 0.0, 0.0, 1.0, tx, 0.0], tm);
            }
        };
        match op.as_str() {
            "q" => saved.push(ctm),
            "Q" => ctm = saved.pop().unwrap(),
            "cm" => ctm = mul(&nums(&args), &ctm),
            "BT" => tm = id,
            "Tm" => tm = nums(&args),
            "Tf" => {
                let Tok::Name(n) = &args[0] else { panic!() };
                let Tok::Num(s) = args[1] else { panic!() };
                (font, size) = (n.clone(), s);
            }
            "Tj" => {
                let Tok::Str(s) = &args[0] else { panic!() };
                show(s, &mut tm);
            }
            "TJ" => {
                let Tok::Arr(a) = &args[0] else { panic!() };
                for item in a {
                    match item {
                        Tok::Str(s) => show(s, &mut tm),
                        Tok::Num(n) => {
                            let tx = -n / 1000.0 * size;
                            tm = mul(&[1.0, 0.0, 0.0, 1.0, tx, 0.0], &tm);
                        }
                        t => panic!("TJ 안의 모르는 값: {t:?}"),
                    }
                }
            }
            // 위치를 바꾸는데 이 계산이 모르는 연산자. krilla가 쓰기 시작하면 여기를 더한다.
            "Td" | "TD" | "T*" | "'" | "\"" | "Tc" | "Tw" | "Tz" | "TL" | "Ts" => {
                panic!("다시 계산하지 못하는 연산자: {op}")
            }
            _ => {}
        }
        args.clear();
    }
    out
}

/// PDF에서 다시 계산한 글리프 원점과 표시 목록의 차이: (글리프 수, 가장 큰 차이 pt).
fn glyph_error(out: &Layout, path: &str) -> (usize, f64) {
    let pdf = Pdf::open(path);
    let pages = pdf.pages();
    assert_eq!(pages.len(), out.pages.len());
    let (mut n, mut worst) = (0, 0.0_f64);
    for (page, dl) in pages.iter().zip(&out.pages) {
        let got = origins(&pdf, page, f64::from(out.page_size.1));
        let want: Vec<(f64, f64)> = dl
            .items
            .iter()
            .flat_map(|i| match i {
                Item::Glyphs { glyphs, .. } => glyphs.as_slice(),
                _ => &[],
            })
            .map(|g| (f64::from(g.x), f64::from(g.y)))
            .collect();
        assert_eq!(got.len(), want.len(), "글리프 수");
        for (g, w) in got.iter().zip(&want) {
            worst = worst.max((g.0 - w.0).abs()).max((g.1 - w.1).abs());
        }
        n += want.len();
    }
    (n, worst)
}

#[test]
fn 글리프_위치가_표시_목록과_001pt_안에서_같다() {
    let (out, path) = write("p1.pdf", &rich());
    let (n, worst) = glyph_error(&out, &path);
    println!("글리프 {n}개, 가장 큰 차이 {worst:.6}pt");
    assert!(n > 1000);
    assert!(worst <= 0.01, "가장 큰 차이 {worst}pt");
}

#[test]
fn 뽑은_글이_원문과_순서까지_같다() {
    let d = rich();
    let (out, path) = write("p2.pdf", &d);
    assert!(out.pages.len() >= 3, "여러 쪽: {}", out.pages.len());
    // 줄이 넘어가는 문단이 있다(같은 문단의 글리프가 기준선 둘 이상에 놓인다).
    let mut lines: Vec<u32> = out.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Glyphs { para, glyphs, .. } if para == "p4" => Some(glyphs[0].y.to_bits()),
            _ => None,
        })
        .collect();
    lines.dedup();
    assert!(lines.len() >= 2, "p4의 줄 {}개", lines.len());
    let text = tool("pdftotext", &["-enc", "UTF-8", &path, "-"]);
    let (got, want) = (squeeze(&text), squeeze(&source(&d)));
    if got != want {
        let at = got
            .chars()
            .zip(want.chars())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let show = |s: &str| {
            s.chars()
                .skip(at.saturating_sub(8))
                .take(24)
                .collect::<String>()
        };
        panic!(
            "{at}번째 글자부터 다름: 원문 「{}」 / 추출 「{}」",
            show(&want),
            show(&got)
        );
    }
    // Pretendard는 "->", "=>"를 글리프 하나로 바꾸고, 풀어 쓴 한글 세 글자도 글리프 하나다.
    let lig: Vec<u32> = out.pages[0]
        .items
        .iter()
        .flat_map(|i| match i {
            Item::Glyphs { para, glyphs, .. } if para == "p3" => glyphs.as_slice(),
            _ => &[],
        })
        .map(|g| g.at)
        .collect();
    let text3 = d.text("p3").unwrap();
    for (s, len) in [("->", 2), ("=>", 2), ("\u{1112}\u{1161}\u{11AB}", 9)] {
        let at = text3.find(s).unwrap() as u32;
        let i = lig.iter().position(|&a| a == at).unwrap();
        assert_eq!(lig[i + 1], at + len, "{s}가 글리프 하나");
    }
    assert!(text.contains("a -> b => c"));
    assert!(text.contains("\u{1112}\u{1161}\u{11AB}글"));
}

/// PDF의 링크 주석을 표시 목록의 `Item::Link`와 견준다(주소, 영역 0.5pt). 링크 수를 돌려준다.
fn check_links(out: &Layout, path: &str) -> usize {
    let pdf = Pdf::open(path);
    let h = f64::from(out.page_size.1);
    let mut count = 0;
    for (page, dl) in pdf.pages().iter().zip(&out.pages) {
        let want: Vec<(&[f32; 4], &str)> = dl
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Link { rect, url } => Some((rect, url.as_str())),
                _ => None,
            })
            .collect();
        let annots = page["/Annots"].as_array().cloned().unwrap_or_default();
        assert_eq!(annots.len(), want.len());
        for (a, (rect, url)) in annots.iter().zip(&want) {
            let a = pdf.get(a);
            assert_eq!(a["/Subtype"], "/Link");
            assert_eq!(pdf.get(&a["/A"])["/URI"], format!("u:{url}"));
            let r: Vec<f64> = a["/Rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let got = [r[0], h - r[3], r[2] - r[0], r[3] - r[1]];
            for (g, w) in got.iter().zip(rect.iter()) {
                assert!((g - f64::from(*w)).abs() <= 0.5, "{got:?} / {rect:?}");
            }
        }
        count += want.len();
    }
    count
}

#[test]
fn 링크_수와_주소와_영역이_같다() {
    let (out, path) = write("p3.pdf", &rich());
    // 링크 run 셋(기울인 링크 포함).
    assert_eq!(check_links(&out, &path), 3);
}

#[test]
fn 글꼴이_담겨_있고_type3이_없다() {
    let (_, path) = write("p4.pdf", &rich());
    let out = tool("pdffonts", &[&path]);
    let rows: Vec<&str> = out.lines().skip(2).collect();
    assert_eq!(rows.len(), 2, "{out}");
    for row in rows {
        assert!(!row.contains("Type 3"), "{row}");
        let f: Vec<&str> = row.split_whitespace().collect();
        let n = f.len();
        // name type… encoding emb sub uni object ID
        assert_eq!(
            (f[n - 5], f[n - 4]),
            ("yes", "yes"),
            "담김, 부분 집합: {row}"
        );
    }
}

#[test]
fn 쪽_수와_쪽_크기가_배치와_같다() {
    for (name, w, h) in [
        ("A4", 595.28, 841.89),
        ("LETTER", 612.0, 792.0),
        ("A4 가로", 841.89, 595.28),
    ] {
        let mut d = rich();
        let m = json!({"t": 56.69, "r": 56.69, "b": 56.69, "l": 56.69});
        apply(
            &mut d,
            json!([{"op": "set", "id": "root", "props": {"page": {"w": w, "h": h, "margin": m}}}]),
        );
        let (out, path) = write("p5.pdf", &d);
        assert_eq!(out.page_size, (w as f32, h as f32));
        let n = out.pages.len();
        let info = tool("pdfinfo", &["-f", "1", "-l", &n.to_string(), &path]);
        assert_eq!(
            field(&info, "Pages"),
            Some(n.to_string().as_str()),
            "{name}"
        );
        let sizes: Vec<(f64, f64)> = info
            .lines()
            .filter(|l| l.starts_with("Page ") && l.contains(" size:"))
            .map(|l| {
                let v: Vec<f64> = l
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect();
                (v[1], v[2])
            })
            .collect();
        assert_eq!(sizes.len(), n, "{name}: {info}");
        for (sw, sh) in sizes {
            assert!(
                (sw - w).abs() <= 0.01 && (sh - h).abs() <= 0.01,
                "{name}: {sw} x {sh}"
            );
        }
    }
}

#[test]
fn 같은_입력이면_같은_바이트() {
    let d = rich();
    let (l1, o1) = layout(&d);
    let (l2, o2) = layout(&d);
    let a = export(&d, &o1, l1.fonts(), &META, &|_| None).unwrap();
    let b = export(&d, &o2, l2.fonts(), &META, &|_| None).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, export(&d, &o1, l1.fonts(), &META, &|_| None).unwrap());
}

#[test]
fn 빈_문서도_한_쪽짜리_pdf() {
    let d = Document::new(Kind::Flow, "빈 문서").unwrap();
    let (out, path) = write("p7.pdf", &d);
    assert_eq!(out.pages.len(), 1);
    let info = tool("pdfinfo", &[&path]);
    assert_eq!(field(&info, "Pages"), Some("1"), "{info}");
}

#[test]
fn 메타데이터는_제목_만든_프로그램_언어다() {
    let (_, path) = write("meta.pdf", &rich());
    let info = tool("pdfinfo", &[&path]);
    assert_eq!(field(&info, "Title"), Some("홍길동 이력서"), "{info}");
    assert_eq!(field(&info, "Creator"), Some("Kari"), "{info}");
    assert_eq!(field(&info, "CreationDate"), None, "{info}");
    let pdf = Pdf::open(&path);
    assert_eq!(pdf.root()["/Lang"], "u:ko");
}

#[test]
fn 배치와_문서가_어긋나면_stale() {
    let d = rich();
    let (l, out) = layout(&d);
    let other = doc(&[json!({"runs": [{"t": "다른 문서"}]})]);
    assert_eq!(
        export(&other, &out, l.fonts(), &META, &|_| None),
        Err(Error::Stale)
    );
    // 문단은 있는데 글이 짧아졌다.
    let mut short = d.clone();
    apply(
        &mut short,
        json!([{"op": "set", "id": "p0", "props": {"runs": [{"t": "짧"}]}}]),
    );
    assert_eq!(
        export(&short, &out, l.fonts(), &META, &|_| None),
        Err(Error::Stale)
    );
}

#[test]
fn 글꼴이_없으면_font_오류() {
    let d = rich();
    let (_, out) = layout(&d);
    let mut one = Fonts::new();
    one.add(font_bytes("Pretendard-Regular.ttf")).unwrap();
    // 굵은 글꼴(1번 파일)이 없다.
    assert!(matches!(
        export(&d, &out, &one, &META, &|_| None),
        Err(Error::Font(_))
    ));
}

#[test]
fn pdf가_받지_못하는_쪽_크기는_page_오류() {
    let d = Document::new(Kind::Flow, "빈 문서").unwrap();
    let (l, mut out) = layout(&d);
    for size in [(1.0, 841.89), (595.28, 20_000.0), (f32::NAN, 100.0)] {
        out.page_size = size;
        assert_eq!(
            export(&d, &out, l.fonts(), &META, &|_| None),
            Err(Error::Page)
        );
    }
}

// ---- 흐름 문서의 생성된 글(목록 표지, 탭 채움선)과 링크 ----

/// 쪽의 생성된 글 항목들의 글, 그린 차례대로.
fn generated(out: &Layout) -> Vec<String> {
    out.pages
        .iter()
        .flat_map(|p| &p.items)
        .filter_map(|i| match i {
            Item::Glyphs { text: Some(t), .. } => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// 목록 문단들(번호와 글머리표, 두 수준).
fn list_paras() -> Vec<Value> {
    let item = |t: &str, kind: &str, level: u64| json!({"runs": [{"t": t}], "pf": {"list": {"kind": kind, "level": level}}});
    vec![
        item("첫째 항목", "ORDERED", 0),
        item("둘째 항목", "ORDERED", 1),
        item("셋째 항목", "ORDERED", 1),
        item("넷째 항목", "ORDERED", 0),
        item("다섯째 항목", "BULLET", 0),
        item("여섯째 항목", "BULLET", 1),
    ]
}

/// 오른쪽 맞춤과 점 채움선 탭이 든 문단들.
fn tab_paras() -> Vec<Value> {
    let tab = json!({"tabs": [{"pos": 400, "align": "RIGHT", "leader": "DOT"}]});
    vec![
        json!({"runs": [{"t": "클로나믹 백엔드 엔지니어\t2023.01 – 2024.05"}], "pf": tab}),
        json!({"runs": [{"t": "견본상사 서버 개발 인턴\t2025.07 – 2026.02"}], "pf": tab}),
    ]
}

#[test]
fn 목록_표지가_문단_글_앞에_차례로_나온다() {
    let (out, path) = write("lists.pdf", &doc(&list_paras()));
    assert_eq!(generated(&out), ["1.", "가.", "나.", "2.", "•", "◦"]);
    let text = squeeze(&tool("pdftotext", &["-enc", "UTF-8", &path, "-"]));
    assert_eq!(
        text,
        "1.첫째항목가.둘째항목나.셋째항목2.넷째항목•다섯째항목◦여섯째항목"
    );
    let (n, worst) = glyph_error(&out, &path);
    assert!(
        worst <= 0.01,
        "표지를 포함한 글리프 {n}개, 가장 큰 차이 {worst}pt"
    );
}

#[test]
fn 탭_채움선은_앞_조각과_뒤_조각_사이에_나온다() {
    let (out, path) = write("tabs.pdf", &doc(&tab_paras()));
    let leaders = generated(&out);
    assert_eq!(leaders.len(), 2);
    assert!(leaders
        .iter()
        .all(|t| t.len() > 5 && t.chars().all(|c| c == '.')));
    // pdftotext는 채움선의 점들을 한 낱말로 뽑는다. 공백만 정리하면 "앞 조각, 채움선, 뒤 조각" 차례다.
    let text = squeeze(&tool("pdftotext", &["-enc", "UTF-8", &path, "-"]));
    let want = format!(
        "클로나믹백엔드엔지니어{}2023.01–2024.05견본상사서버개발인턴{}2025.07–2026.02",
        leaders[0], leaders[1]
    );
    assert_eq!(text, want);
    let (n, worst) = glyph_error(&out, &path);
    assert!(
        worst <= 0.01,
        "채움선을 포함한 글리프 {n}개, 가장 큰 차이 {worst}pt"
    );
}

#[test]
fn 줄이_넘어가는_링크는_줄마다_주석이_되고_주소와_영역이_맞다() {
    let long = SENTENCES[1].repeat(3);
    let d = doc(&[
        json!({"runs": [{"t": "앞 "}, {"t": long, "link": "https://example.com"}, {"t": " 뒤"}]}),
        json!({"runs": [{"t": "색", "link": "https://a.example", "s": {"color": "#FF0000FF"}}]}),
    ]);
    let (out, path) = write("links.pdf", &d);
    let n = check_links(&out, &path);
    assert!(n >= 4, "줄마다 하나(긴 링크 셋 이상과 짧은 링크 하나): {n}");
    // 링크 글에는 밑줄(사각형)이 있다.
    let rects = out.pages[0]
        .items
        .iter()
        .filter(|i| matches!(i, Item::Rect { .. }))
        .count();
    assert!(rects >= n, "밑줄 {rects}개");
    let text = squeeze(&tool("pdftotext", &["-enc", "UTF-8", &path, "-"]));
    assert_eq!(text, squeeze(&source(&d)));
}

/// 눈으로 보는 견본: 목록, 탭, 링크, 여러 쪽. `app/engine/target/kari-shots/10-lists.pdf`와 첫 쪽 그림.
#[test]
fn 견본_pdf를_남긴다() {
    let mut paras: Vec<Value> = vec![
        json!({"style": "title", "runs": [{"t": "홍길동 · 백엔드 엔지니어"}]}),
        json!({"runs": [
            {"t": "github.com/example", "link": "https://github.com/example"},
            {"t": " · "},
            {"t": "hello@example.com", "link": "mailto:hello@example.com"},
        ]}),
    ];
    paras.extend(tab_paras());
    paras.extend(list_paras());
    for i in 0..40 {
        let t: Vec<&str> = (0..3)
            .map(|k| SENTENCES[(i + k) % SENTENCES.len()])
            .collect();
        paras.push(json!({"runs": [{"t": t.join(" ")}]}));
    }
    let d = doc(&paras);
    let (out, path) = write("10-lists.pdf", &d);
    assert!(out.pages.len() >= 2);
    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = format!("{dir}/10-lists.pdf");
    std::fs::copy(&path, &pdf).unwrap();
    let png = format!("{dir}/10-lists-1");
    tool(
        "pdftocairo",
        &[
            "-png",
            "-r",
            "110",
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            &pdf,
            &png,
        ],
    );
    let text = squeeze(&tool("pdftotext", &["-enc", "UTF-8", &pdf, "-"]));
    assert!(text.contains("1.첫째항목가.둘째항목"));
}

/// `pdfinfo`가 적은 쪽마다의 크기.
fn page_sizes(path: &str, n: usize) -> Vec<(f64, f64)> {
    let info = tool("pdfinfo", &["-f", "1", "-l", &n.to_string(), path]);
    info.lines()
        .filter(|l| l.starts_with("Page ") && l.contains(" size:"))
        .map(|l| {
            let v: Vec<f64> = l
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            (v[1], v[2])
        })
        .collect()
}

/// 머리글·바닥글의 글이 쪽마다 뽑히고(`{쪽}`, `{전체}`가 그 쪽의 값), 쪽 크기가 설정과 같다.
#[test]
fn 머리글_바닥글이_쪽마다_뽑히고_쪽_크기가_설정과_같다() {
    for (w, h, skip) in [(595.28, 841.89, false), (595.28, 419.53, true)] {
        let mut d = rich();
        let m = json!({"t": 40, "r": 40, "b": 40, "l": 40});
        apply(
            &mut d,
            json!([{"op": "set", "id": "root", "props": {
                "page": {"preset": "A5", "w": w, "h": h, "margin": m},
                "header": {"l": "김머리", "r": "이력서"},
                "footer": {"c": "{쪽} / {전체}"},
                "hfSkipFirst": skip,
            }}]),
        );
        let (out, path) = write("g7.pdf", &d);
        let n = out.pages.len();
        assert!(n >= 2);
        for (sw, sh) in page_sizes(&path, n) {
            assert!(
                (sw - w).abs() <= 0.01 && (sh - h).abs() <= 0.01,
                "{sw} x {sh}"
            );
        }
        for p in 1..=n {
            let t = squeeze(&tool(
                "pdftotext",
                &[
                    "-enc",
                    "UTF-8",
                    "-f",
                    &p.to_string(),
                    "-l",
                    &p.to_string(),
                    &path,
                    "-",
                ],
            ));
            let shown = !(skip && p == 1);
            assert_eq!(t.contains("김머리"), shown, "{p}쪽: {t}");
            assert_eq!(t.contains("이력서"), shown, "{p}쪽");
            assert_eq!(t.contains(&format!("{p}/{n}")), shown, "{p}쪽: {t}");
        }
        let (k, worst) = glyph_error(&out, &path);
        assert!(
            worst <= 0.01,
            "머리글을 포함한 글리프 {k}개, 가장 큰 차이 {worst}pt"
        );
    }
}

/// 눈으로 보는 이력서 모양의 두 쪽 문서: 머리글에 이름, 바닥글에 `{쪽} / {전체}`, 목록과 오른쪽 맞춤 탭.
/// `app/engine/target/kari-shots/11-resume.pdf`와 두 쪽의 그림.
#[test]
fn 이력서_견본_pdf를_남긴다() {
    let body_w = 595.28 - 2.0 * 56.69;
    let right = json!({"tabs": [{"pos": body_w, "align": "RIGHT", "leader": "NONE"}]});
    let item =
        |t: &str| json!({"runs": [{"t": t}], "pf": {"list": {"kind": "BULLET", "level": 0}}});
    let mut paras: Vec<Value> = vec![
        json!({"style": "title", "runs": [{"t": "홍길동"}]}),
        json!({"runs": [
            {"t": "백엔드 엔지니어 · "},
            {"t": "hello@example.com", "link": "mailto:hello@example.com"},
            {"t": " · "},
            {"t": "github.com/example", "link": "https://github.com/example"},
        ]}),
        json!({"style": "h1", "runs": [{"t": "경력"}]}),
    ];
    for (co, when) in [
        ("견본상사 · 서버 개발 인턴", "2025.07 – 2026.02"),
        ("클로나믹 · 백엔드 엔지니어", "2023.01 – 2024.05"),
        ("예시페이 · 결제 플랫폼 개발", "2020.03 – 2022.12"),
    ] {
        paras.push(
            json!({"runs": [{"t": format!("{co}\t{when}"), "s": {"weight": 700}}], "pf": right}),
        );
        for i in 0..6 {
            paras.push(item(SENTENCES[(i + co.len()) % SENTENCES.len()]));
        }
    }
    paras.push(json!({"style": "h1", "runs": [{"t": "프로젝트"}]}));
    for i in 0..10 {
        paras.push(item(SENTENCES[i % SENTENCES.len()]));
    }
    let mut d = doc(&paras);
    apply(
        &mut d,
        json!([{"op": "set", "id": "root", "props": {
            "header": {"l": "홍길동", "r": "이력서"},
            "footer": {"c": "{쪽} / {전체}"},
        }}]),
    );
    let (out, path) = write("11-resume.pdf", &d);
    assert_eq!(out.pages.len(), 2, "두 쪽이어야 한다");
    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = format!("{dir}/11-resume.pdf");
    std::fs::copy(&path, &pdf).unwrap();
    for p in ["1", "2"] {
        tool(
            "pdftocairo",
            &[
                "-png",
                "-r",
                "110",
                "-f",
                p,
                "-l",
                p,
                "-singlefile",
                &pdf,
                &format!("{dir}/11-resume-{p}"),
            ],
        );
    }
    let text = squeeze(&tool("pdftotext", &["-enc", "UTF-8", &pdf, "-"]));
    assert!(text.contains("1/2") && text.contains("2/2"));
}

// ---- 그림 ----

/// 증명사진처럼 보이는 그림을 `format`으로 담는다.
fn photo(w: i32, h: i32, format: skia_safe::EncodedImageFormat) -> Vec<u8> {
    use skia_safe::{surfaces, Color, Paint};
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

fn img_run(asset: char, w: f64, h: f64) -> Value {
    json!({"t": "\u{FFFC}", "o": {"type": "image", "asset": asset.to_string().repeat(64), "w": w, "h": h}})
}

/// `pdfimages -list`의 그림 줄: (쪽, 폭, 높이, 담은 방식).
fn pdf_images(path: &str) -> Vec<(usize, u32, u32, String)> {
    tool("pdfimages", &["-list", path])
        .lines()
        .skip(2)
        .map(|l| {
            let c: Vec<&str> = l.split_whitespace().collect();
            (
                c[0].parse().unwrap(),
                c[3].parse().unwrap(),
                c[4].parse().unwrap(),
                c[8].to_string(),
            )
        })
        .collect()
}

#[test]
fn 그림의_수와_크기가_맞고_jpeg는_다시_압축하지_않는다() {
    let jpg = photo(300, 400, skia_safe::EncodedImageFormat::JPEG);
    let png = photo(120, 90, skia_safe::EncodedImageFormat::PNG);
    let mut paras = vec![
        json!({"runs": [{"t": "사진 "}, img_run('a', 90.0, 120.0), {"t": " 그림 "}, img_run('b', 120.0, 90.0)]}),
    ];
    for i in 0..30 {
        paras.push(json!({"runs": [{"t": SENTENCES[i % SENTENCES.len()]}]}));
    }
    paras.push(json!({"runs": [img_run('a', 60.0, 80.0), img_run('f', 50.0, 50.0)]}));
    let d = doc(&paras);
    let (l, out) = layout(&d);
    let (j, p) = (jpg.clone(), png.clone());
    let read = move |h: &str| match h.chars().next() {
        Some('a') => Some(j.clone()),
        Some('b') => Some(p.clone()),
        _ => None,
    };
    let bytes = export(&d, &out, l.fonts(), &META, &read).unwrap();
    let path = format!("{}/h6.pdf", env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(&path, &bytes).unwrap();
    // 표시 목록의 그림 넷(읽지 못하는 'z'는 회색 상자)
    let placed: Vec<(usize, [f32; 4])> = out
        .pages
        .iter()
        .enumerate()
        .flat_map(|(i, pg)| {
            pg.items.iter().filter_map(move |it| match it {
                Item::Image { rect, .. } => Some((i, *rect)),
                _ => None,
            })
        })
        .collect();
    assert_eq!(placed.len(), 4);
    let list = pdf_images(&path);
    assert_eq!(list.len(), 3, "{list:?}");
    let pages: Vec<usize> = list.iter().map(|x| x.0).collect();
    let want: Vec<usize> = placed[..3].iter().map(|x| x.0 + 1).collect();
    assert_eq!(pages, want);
    assert_eq!(
        (list[0].1, list[0].2, list[0].3.as_str()),
        (300, 400, "jpeg")
    );
    assert_eq!((list[1].1, list[1].2), (120, 90));
    assert_ne!(list[1].3, "jpeg");
    // 같은 에셋은 한 번만 담긴다(pdfimages의 object 번호가 같다).
    let objs: Vec<String> = tool("pdfimages", &["-list", &path])
        .lines()
        .skip(2)
        .map(|l| l.split_whitespace().nth(10).unwrap_or("").to_string())
        .collect();
    assert_eq!(objs[0], objs[2]);
    // JPEG는 받은 바이트 그대로(다시 압축하지 않았다).
    let prefix = format!("{}/h6-img", env!("CARGO_TARGET_TMPDIR"));
    tool("pdfimages", &["-j", "-f", "1", "-l", "1", &path, &prefix]);
    let first = std::fs::read(format!("{prefix}-000.jpg")).unwrap();
    assert_eq!(first, jpg);
}

/// 눈으로 보는 견본: 사진이 든 이력서 한 쪽. `app/engine/target/kari-shots/14-images.pdf`와 첫 쪽 그림.
#[test]
fn 그림_견본_pdf를_남긴다() {
    let face = photo(300, 400, skia_safe::EncodedImageFormat::JPEG);
    let body_w = 595.28 - 2.0 * 56.69;
    let right = json!({"tabs": [{"pos": body_w, "align": "RIGHT", "leader": "NONE"}]});
    let mut paras = vec![
        json!({"runs": [img_run('a', 90.0, 120.0)], "pf": {"align": "RIGHT", "space": {"after": 0}}}),
        json!({"style": "title", "runs": [{"t": "홍길동"}]}),
        json!({"runs": [{"t": "백엔드 엔지니어 · "}, {"t": "hello@example.com", "link": "mailto:hello@example.com"}]}),
        json!({"style": "h1", "runs": [{"t": "경력"}]}),
        json!({"runs": [{"t": "견본상사 · 서버 개발 인턴\t2025.07 – 2026.02", "s": {"weight": 700}}], "pf": right}),
    ];
    for i in 0..8 {
        paras.push(json!({"runs": [{"t": SENTENCES[i % SENTENCES.len()]}], "pf": {"list": {"kind": "BULLET", "level": 0}}}));
    }
    let mut d = doc(&paras);
    apply(
        &mut d,
        json!([{"op": "set", "id": "root", "props": {"header": {"r": "이력서"}, "footer": {"c": "{쪽} / {전체}"}}}]),
    );
    let (l, out) = layout(&d);
    let f = face.clone();
    let bytes = export(&d, &out, l.fonts(), &META, &move |_: &str| Some(f.clone())).unwrap();
    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = format!("{dir}/14-images.pdf");
    std::fs::write(&pdf, bytes).unwrap();
    tool(
        "pdftocairo",
        &[
            "-png",
            "-r",
            "110",
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            &pdf,
            &format!("{dir}/14-images-1"),
        ],
    );
    assert_eq!(pdf_images(&pdf).len(), 1);
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

/// 표의 글이 행 → 셀 순서로 뽑히고, 둘째 쪽에 다시 그린 머리 행도 글로 뽑힌다. 견본 `15-table-resume.pdf`와
/// 두 쪽의 그림을 남긴다.
#[test]
fn 표의_글이_행_셀_순서로_뽑히고_다시_그린_머리_행도_뽑힌다() {
    let d = table_resume();
    let (l, out) = layout(&d);
    assert_eq!(out.pages.len(), 2);
    let face = photo(300, 400, skia_safe::EncodedImageFormat::JPEG);
    let bytes = export(&d, &out, l.fonts(), &META, &move |_: &str| {
        Some(face.clone())
    })
    .unwrap();
    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = format!("{dir}/15-table-resume.pdf");
    std::fs::write(&pdf, bytes).unwrap();
    for p in ["1", "2"] {
        tool(
            "pdftocairo",
            &[
                "-png",
                "-r",
                "110",
                "-f",
                p,
                "-l",
                p,
                "-singlefile",
                &pdf,
                &format!("{dir}/15-table-resume-{p}"),
            ],
        );
    }
    // 내용 흐름 순서(-raw)로 뽑으면 행 → 셀 순서다.
    let page = |p: &str| {
        squeeze(&tool(
            "pdftotext",
            &["-raw", "-enc", "UTF-8", "-f", p, "-l", p, &pdf, "-"],
        ))
    };
    let one = page("1");
    assert!(
        one.contains("성명홍길동연락처010-1234-5678·hello@example.com주소서울특별시"),
        "{one}"
    );
    assert!(
        one.contains("기간학교전공구분2016.03–2020.02한국대학교컴퓨터공학졸업"),
        "{one}"
    );
    let two = page("2");
    assert!(
        two.starts_with("기간회사담당업무"),
        "둘째 쪽 맨 위의 머리 행: {two}"
    );
    assert!(two.contains("위내용은사실과다름없습니다."));
    assert_eq!(pdf_images(&pdf).len(), 1);
}

/// 글꼴에 없는 글자는 빈 네모(사각형)로 그려지고 뽑은 글에서는 빠질 수 있다. 다른 글의 순서는 그대로다.
#[test]
fn 글꼴에_없는_글자가_있어도_다른_글의_순서는_그대로다() {
    let d = doc(&[
        json!({"runs": [{"t": "성명 홍길동 (洪吉童) 끝"}]}),
        json!({"runs": [{"t": "가나 あア와 이모지 😀 다음"}]}),
    ]);
    let (out, path) = write("missing.pdf", &d);
    let boxes = out.pages[0]
        .items
        .iter()
        .filter(|i| matches!(i, Item::Rect { .. }))
        .count();
    assert_eq!(boxes, 4 * 4, "네모 넷(洪吉童😀), 네모마다 선 넷");
    let text = squeeze(&tool("pdftotext", &["-raw", "-enc", "UTF-8", &path, "-"]));
    // 없는 글자를 빼면 원문과 같은 차례다(붙어 뽑혀도 순서는 지킨다).
    let kept: String = text
        .chars()
        .filter(|c| !matches!(c, '洪' | '吉' | '童' | '😀'))
        .collect();
    assert_eq!(kept, "성명홍길동()끝가나あア와이모지다음", "{text}");
}

/// 위·아래 첨자, 자간(넓힘·좁힘), 형광펜이 든 견본 문단들.
fn char_format_paras() -> Vec<Value> {
    let sp = |t: &str, v: f64| json!({"t": t, "s": {"spacing": v}});
    vec![
        json!({"style": "title", "runs": [{"t": "글자 모양 견본"}]}),
        json!({"runs": [
            {"t": "넓이 E = mc"},
            {"t": "2", "s": {"vertical": "SUPER"}},
            {"t": ", 물 H"},
            {"t": "2", "s": {"vertical": "SUB"}},
            {"t": "O, 각주"},
            {"t": "1)", "s": {"vertical": "SUPER", "underline": true}},
            {"t": " 밑줄 친 아래 첨자 x"},
            {"t": "i+1", "s": {"vertical": "SUB", "underline": true}},
            {"t": " 끝."},
        ]}),
        json!({"runs": [
            sp("자간을 넓힌 글 Spacing +3pt", 3.0),
            {"t": " · "},
            sp("자간을 좁힌 글 Tight -1pt", -1.0),
        ]}),
        json!({"runs": [
            {"t": "형광펜 ", "s": {"bg": "#FFF080FF"}},
            {"t": "초록 바탕", "s": {"bg": "#C8F0C8FF", "weight": 700}},
            {"t": "과 첨자 위 바탕 x"},
            {"t": "n", "s": {"vertical": "SUPER", "bg": "#FFF080FF"}},
        ]}),
        json!({"pf": {"align": "RIGHT"}, "runs": [sp("오른쪽 맞춤에 자간 2pt를 준 줄의 끝", 2.0)]}),
        json!({"pf": {"align": "JUSTIFIED"}, "runs": [sp(&SENTENCES[..3].join(" "), 1.5)]}),
        json!({"pf": {"tabs": [{"pos": 481.9, "align": "RIGHT", "leader": "DOT"}]}, "runs": [
            {"t": "카리 주식회사\t"},
            sp("2024.03 – 2026.11", 1.0),
        ]}),
    ]
}

/// 자간이 있는 글도 PDF의 글리프 위치가 표시 목록과 0.01pt 안에서 같고, pdftotext로 뽑은 글이 원문과 같다(낱말이 갈리지 않는다).
/// 눈으로 보는 견본도 남긴다: `app/engine/target/kari-shots/17-char-format.pdf`, 그 PDF의 첫 쪽 그림, 화면 그림.
#[test]
fn 첨자_자간_형광펜이_pdf와_화면에서_같다() {
    let d = doc(&char_format_paras());
    let (out, path) = write("17-char-format.pdf", &d);
    let (n, worst) = glyph_error(&out, &path);
    println!("글자 모양 견본: 글리프 {n}개, 가장 큰 차이 {worst:.6}pt");
    assert!(n > 100);
    assert!(worst <= 0.01, "가장 큰 차이 {worst}pt");
    let text = tool("pdftotext", &["-enc", "UTF-8", &path, "-"]);
    // 탭 채움선(생성된 점 여러 개)은 원문에 없다. 홀로 선 점은 원문의 마침표다.
    let chars: Vec<char> = squeeze(&text).chars().collect();
    let dot = |i: usize| chars.get(i) == Some(&'.');
    let got: String = (0..chars.len())
        .filter(|&i| !dot(i) || !(i > 0 && dot(i - 1) || dot(i + 1)))
        .map(|i| chars[i])
        .collect();
    assert_eq!(got, squeeze(&source(&d)));
    // 줄 전체에 자간을 준 글은 낱말 단위로 뽑힌다. 한 줄에 자간이 다른 글이 섞이면 pdftotext가 글자 사이를
    // 낱말 사이로 볼 수 있다(글자는 빠지거나 순서가 바뀌지 않는다. 위에서 본다).
    assert!(
        text.contains("오른쪽 맞춤에 자간 2pt를 준 줄의 끝"),
        "pdftotext: {text}"
    );
    // 첨자는 줄인 크기로 들어간다.
    let sizes: Vec<f32> = out.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Glyphs { size, para, .. } if para == "p1" => Some(*size),
            _ => None,
        })
        .collect();
    assert!(
        sizes.iter().any(|s| (s - 10.5 * 0.65).abs() < 1e-4),
        "{sizes:?}"
    );

    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = format!("{dir}/17-char-format.pdf");
    std::fs::copy(&path, &pdf).unwrap();
    let png = format!("{dir}/17-char-format-pdf");
    tool(
        "pdftocairo",
        &[
            "-png",
            "-r",
            "110",
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            &pdf,
            &png,
        ],
    );
    screen_shot(&d, &format!("{dir}/17-char-format-screen.png"));
}

/// 화면과 같은 길(표시 목록 → 그리기)로 첫 쪽을 그려 PNG로 쓴다. 배율 2, 보이는 폭 700 논리 화소.
fn screen_shot(d: &Document, path: &str) {
    use kari_render::{draw, Colors, Faces, Frame, Geo, Scene};
    use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo, Pixmap};
    let files = [
        font_bytes("Pretendard-Regular.ttf"),
        font_bytes("Pretendard-Bold.ttf"),
    ];
    let (_, out) = layout(d);
    let (view_w, scale) = (700.0_f32, 2.0_f32);
    let geo = Geo::new(view_w, out.page_size, out.pages.len(), 0.0); // 쪽 너비에 맞춤
    let view_h = geo.content_height().min(1000.0);
    let (w, h) = ((view_w * scale) as i32, (view_h * scale) as i32);
    let mut px = vec![0u8; w as usize * h as usize * 4];
    let frame = Frame {
        pixels: &mut px,
        width: w,
        height: h,
        stride: w as usize * 4,
        scale,
        scroll_x: 0.0,
        scroll_y: 0.0,
    };
    let scene = Scene {
        layout: &out,
        geo,
        colors: Colors {
            bg: [0xF5, 0xF5, 0xF5, 0xFF],
            selection: [0xE5, 0xF4, 0xFF, 0xFF],
            preedit: [0x0A, 0x6D, 0xC2, 0xFF],
        },
        selection: &[],
        preedit: &[],
        guides: false,
    };
    let faces = Faces::new(files.iter().map(Vec::as_slice));
    let mut images = kari_render::Images::new(Box::new(|_| None), kari_render::IMAGE_BUDGET);
    draw(frame, &faces, &scene, &mut images).unwrap();
    let info = ImageInfo::new((w, h), ColorType::BGRA8888, AlphaType::Premul, None);
    let png = Pixmap::new(&info, &mut px, w as usize * 4)
        .and_then(|p| p.encode(EncodedImageFormat::PNG, None))
        .expect("PNG 인코딩");
    std::fs::write(path, png).unwrap();
}

// ---- 문단 테두리와 바탕 ----

/// 쪽 하나에서 칠한 경로마다의 경계 상자 [x, y, w, h](쪽 왼쪽 위 기준 pt), 칠한 차례대로.
fn fills(pdf: &Pdf, page: &Value, height: f64) -> Vec<[f64; 4]> {
    let id: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let (mut ctm, mut saved) = (id, Vec::new());
    let mut pts: Vec<(f64, f64)> = Vec::new();
    let mut args: Vec<f64> = Vec::new();
    let mut out = Vec::new();
    for t in lex(&pdf.content(page)) {
        let op = match t {
            Tok::Num(n) => {
                args.push(n);
                continue;
            }
            Tok::Op(op) => op,
            _ => {
                args.clear();
                continue;
            }
        };
        let at = |x: f64, y: f64, m: &M| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        match op.as_str() {
            "q" => saved.push(ctm),
            "Q" => ctm = saved.pop().unwrap(),
            "cm" => ctm = mul(&args[args.len() - 6..].try_into().unwrap(), &ctm),
            "m" | "l" => pts.push(at(args[0], args[1], &ctm)),
            "re" => {
                let (x, y, w, h) = (args[0], args[1], args[2], args[3]);
                pts.extend([at(x, y, &ctm), at(x + w, y + h, &ctm)]);
            }
            "f" | "f*" | "F" => {
                let (x0, x1) = pts
                    .iter()
                    .fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
                let (y0, y1) = pts
                    .iter()
                    .fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
                out.push([x0, height - y1, x1 - x0, y1 - y0]);
                pts.clear();
            }
            "n" => pts.clear(),
            _ => {}
        }
        args.clear();
    }
    out
}

/// 상자, 이어진 문단, 쪽을 넘는 문단, 표 칸, 목록에 테두리와 바탕을 준 문서.
fn bordered() -> Document {
    let side = |c: &str, w: f64| json!({"weight": w, "color": c, "gap": 3});
    let boxed = json!({"border": {"top": side("#C00000FF", 1.5), "bottom": side("#1E1E1EFF", 0.5),
        "left": side("#336699FF", 0.75), "right": side("#336699FF", 0.75)}, "shade": "#FFF2CCFF"});
    let mut paras = vec![json!({"style": "title", "runs": [{"t": "테두리 견본"}]})];
    let mut listed = boxed.clone();
    listed["list"] = json!({"kind": "ORDERED", "level": 0});
    listed["shade"] = json!("#EAF5ECFF");
    for (i, t) in SENTENCES.iter().enumerate() {
        let pf = if i < 3 { &boxed } else { &listed };
        paras.push(json!({"runs": [{"t": t}], "pf": pf}));
    }
    let long: Vec<&str> = (0..60).map(|i| SENTENCES[i % SENTENCES.len()]).collect();
    paras.push(json!({"runs": [{"t": long.join(" ")}], "pf": boxed}));
    let mut d = doc(&paras);
    apply(
        &mut d,
        json!([
            {"op": "add", "node": {"id": "t", "type": "table", "parent": "root", "props": {"cols": [200, 200]}}},
            {"op": "add", "node": {"id": "r", "type": "row", "parent": "t"}},
            {"op": "add", "node": {"id": "c0", "type": "cell", "parent": "r"}},
            {"op": "add", "node": {"id": "c1", "type": "cell", "parent": "r"}},
            {"op": "add", "node": {"id": "q0", "type": "para", "parent": "c0", "props": {"runs": [{"t": "칸 안"}], "pf": boxed}}},
            {"op": "add", "node": {"id": "q1", "type": "para", "parent": "c1", "props": {"runs": [{"t": "옆 칸"}]}}},
        ]),
    );
    d
}

#[test]
fn 테두리와_바탕의_사각형이_표시_목록과_001pt_안에서_같다() {
    let (out, path) = write("borders.pdf", &bordered());
    assert!(out.pages.len() >= 2, "쪽을 넘는 문단이 있다");
    let pdf = Pdf::open(&path);
    let (mut n, mut worst) = (0, 0.0_f64);
    for (page, dl) in pdf.pages().iter().zip(&out.pages) {
        let got = fills(&pdf, page, f64::from(out.page_size.1));
        let want: Vec<[f64; 4]> = dl
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Rect { rect, .. } if rect[2] > 0.0 && rect[3] > 0.0 => {
                    Some(rect.map(f64::from))
                }
                _ => None,
            })
            .collect();
        assert_eq!(got.len(), want.len(), "칠한 사각형 수");
        for (g, w) in got.iter().zip(&want) {
            for k in 0..4 {
                worst = worst.max((g[k] - w[k]).abs());
            }
        }
        n += want.len();
    }
    println!("사각형 {n}개, 가장 큰 차이 {worst:.6}pt");
    assert!(n > 20);
    assert!(worst <= 0.01, "가장 큰 차이 {worst}pt");
    let (_, glyphs) = glyph_error(&out, &path);
    assert!(glyphs <= 0.01);
    // 견본: PDF 첫 쪽과 화면 그림.
    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = format!("{dir}/18-borders.pdf");
    std::fs::copy(&path, &pdf).unwrap();
    tool(
        "pdftocairo",
        &[
            "-png",
            "-r",
            "110",
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            &pdf,
            &format!("{dir}/18-borders-1"),
        ],
    );
    screen_shot(&bordered(), &format!("{dir}/18-borders-screen.png"));
}

/// 구역 제목(굵은 글 + 아래 선)을 스타일 하나로 만든 이력서: 사용자 스타일 "구역 제목"(제목 1 바탕, 아래 선 0.75pt).
fn ruled_resume() -> Document {
    let body_w = 595.28 - 2.0 * 56.69;
    let right = json!({"tabs": [{"pos": body_w, "align": "RIGHT", "leader": "NONE"}]});
    let item =
        |t: &str| json!({"runs": [{"t": t}], "pf": {"list": {"kind": "BULLET", "level": 0}}});
    let mut paras: Vec<Value> = vec![
        json!({"style": "title", "runs": [{"t": "홍길동"}]}),
        json!({"style": "subtitle", "runs": [{"t": "백엔드 엔지니어 · hello@example.com · github.com/example"}]}),
        json!({"style": "u1", "runs": [{"t": "경력"}]}),
    ];
    for (co, when) in [
        ("클로나믹 · 백엔드 엔지니어", "2023.01 – 2024.05"),
        ("예시페이 · 결제 플랫폼 개발", "2020.03 – 2022.12"),
    ] {
        paras.push(json!({"style": "h3", "runs": [{"t": format!("{co}\t{when}")}], "pf": right}));
        for i in 0..4 {
            paras.push(item(SENTENCES[(i + co.len()) % SENTENCES.len()]));
        }
    }
    paras.push(json!({"style": "u1", "runs": [{"t": "프로젝트"}]}));
    for i in 0..4 {
        paras.push(item(SENTENCES[i % SENTENCES.len()]));
    }
    paras.push(json!({"style": "u1", "runs": [{"t": "기술"}]}));
    paras.push(json!({"runs": [{"t": "Java · Kotlin · Spring · Kafka · Redis · PostgreSQL · Kubernetes"}]}));
    let mut d = doc(&paras);
    let mut styles = d.default_prop("flow", "styles").unwrap().clone();
    styles["u1"] = json!({"name": "구역 제목", "based": "h1", "next": "body",
        "pf": {"border": {"bottom": {"weight": 0.75, "color": "#1E1E1EFF", "gap": 2}}}});
    apply(
        &mut d,
        json!([{"op": "set", "id": "root", "props": {"styles": styles, "footer": {"c": "{쪽} / {전체}"}}}]),
    );
    d
}

#[test]
fn 구역_제목에_아래_선을_준_이력서_견본_pdf와_화면() {
    let d = ruled_resume();
    let (out, path) = write("18-resume-rule.pdf", &d);
    // 구역 제목마다 아래 선 하나, 글 아래에 거리 2pt.
    let lines: Vec<[f32; 4]> = out.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { rect, color }
                if *color == [0x1E, 0x1E, 0x1E, 0xFF] && (rect[3] - 0.75).abs() < 1e-3 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    for l in &lines {
        assert!((l[0] - 56.69).abs() < 0.01 && (l[2] - (595.28 - 2.0 * 56.69)).abs() < 0.01);
    }
    let pdf = Pdf::open(&path);
    let got = fills(&pdf, pdf.pages()[0], f64::from(out.page_size.1));
    for l in &lines {
        assert!(
            got.iter()
                .any(|g| (0..4).all(|k| (g[k] - f64::from(l[k])).abs() <= 0.01)),
            "PDF에 같은 선이 있다: {l:?}"
        );
    }
    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let file = format!("{dir}/18-resume-rule.pdf");
    std::fs::copy(&path, &file).unwrap();
    tool(
        "pdftocairo",
        &[
            "-png",
            "-r",
            "110",
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            &file,
            &format!("{dir}/18-resume-rule-1"),
        ],
    );
    screen_shot(&d, &format!("{dir}/18-resume-rule-screen.png"));
    let text = squeeze(&tool("pdftotext", &["-enc", "UTF-8", &file, "-"]));
    assert!(text.contains("경력") && text.contains("프로젝트") && text.contains("기술"));
}

// ---- 선을 골라 넣은 이력서 표 ----

/// 선을 골라 넣은 이력서 표(`render/tests/data/table-lines.json`)를 PDF로 내보내 `17-table-lines.pdf`와 그 그림을 남긴다.
/// 선과 바탕의 사각형이 표시 목록의 것과 차례까지 같고 자리 차이가 0.01pt 이하다(화면도 같은 목록에서 그린다).
#[test]
fn 선을_골라_넣은_표는_pdf와_화면의_선_자리가_같다() {
    let path = format!(
        "{}/../render/tests/data/table-lines.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let ops: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut d = Document::new(Kind::Flow, "선을 고른 표").unwrap();
    d.apply_trusted(&ops).unwrap();
    let (l, out) = layout(&d);
    let face = photo(300, 400, skia_safe::EncodedImageFormat::JPEG);
    let bytes = export(&d, &out, l.fonts(), &META, &move |_: &str| {
        Some(face.clone())
    })
    .unwrap();
    let dir = format!("{}/../target/kari-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let file = format!("{dir}/17-table-lines.pdf");
    std::fs::write(&file, bytes).unwrap();
    tool(
        "pdftocairo",
        &[
            "-png",
            "-r",
            "110",
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            &file,
            &format!("{dir}/17-table-lines-pdf"),
        ],
    );
    let pdf = Pdf::open(&file);
    let want: Vec<[f32; 4]> = out.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { rect, .. } if rect[2] > 0.0 && rect[3] > 0.0 => Some(*rect),
            _ => None,
        })
        .collect();
    let got = fills(&pdf, pdf.pages()[0], f64::from(out.page_size.1));
    assert_eq!(got.len(), want.len(), "채운 사각형 수");
    let worst = got
        .iter()
        .zip(&want)
        .flat_map(|(g, w)| (0..4).map(move |i| (g[i] - f64::from(w[i])).abs()))
        .fold(0.0, f64::max);
    println!(
        "선을 고른 표: 사각형 {}개, 가장 큰 차이 {worst:.6}pt",
        got.len()
    );
    assert!(worst <= 0.01, "가장 큰 차이 {worst}pt");
    // 눈금선은 PDF에 들어가지 않는다(표시 목록에 없다).
    assert!(!out.pages[0].guides.is_empty());
}

// ---- 캔버스 장면 ----

/// 대지 둘: 첫째(200×150)에 도형, 잘라낸 프레임, 돌린 사각형, 링크가 든 글 상자. 둘째(300×100)는 판 위 (400, 0).
fn canvas_doc() -> Document {
    let mut d = Document::new(Kind::Canvas, "캔버스 견본").unwrap();
    let solid = |c: &str| json!([{"type": "SOLID", "color": c}]);
    let node = |id: &str, ty: &str, parent: &str, props: Value| {
        let parent = if ty == "page" {
            Value::Null
        } else {
            json!(parent)
        };
        json!({"op": "add", "node": {"id": id, "type": ty, "parent": parent, "props": props}})
    };
    let ops = vec![
        json!({"op": "set", "id": "n1", "props": {"w": 200, "h": 150}}),
        node(
            "r",
            "rect",
            "n1",
            json!({"x": 10, "y": 10, "w": 80, "h": 60, "radius": [20, 20, 20, 20], "fills": solid("#FF0000FF"), "strokes": solid("#000000FF"), "stroke": {"weight": 3, "align": "OUTSIDE"}}),
        ),
        node(
            "e",
            "ellipse",
            "n1",
            json!({"x": 110, "y": 10, "w": 80, "h": 60, "fills": solid("#0000FFFF")}),
        ),
        node(
            "l",
            "line",
            "n1",
            json!({"x": 10, "y": 100, "w": 100, "strokes": solid("#336699FF"), "stroke": {"weight": 4, "align": "CENTER", "capStart": "ROUND"}}),
        ),
        node(
            "f",
            "frame",
            "n1",
            json!({"x": 120, "y": 70, "w": 60, "h": 40, "radius": [6, 6, 6, 6], "fills": solid("#EEEEEEFF")}),
        ),
        node(
            "c",
            "rect",
            "f",
            json!({"x": -10, "y": -10, "w": 100, "h": 100, "fills": solid("#00FF00FF")}),
        ),
        node(
            "o",
            "rect",
            "n1",
            json!({"x": 60, "y": 115, "w": 40, "h": 10, "rotation": 33, "flipX": true, "fills": solid("#FF8000FF")}),
        ),
        node(
            "t",
            "text",
            "n1",
            json!({"x": 10, "y": 112, "runs": [{"t": "한글 "}, {"t": "Kari", "s": {"weight": 700}}, {"t": " 링크", "link": "https://example.com/a"}], "style": {"font": "Pretendard", "weight": 400, "size": 14}}),
        ),
        node(
            "b2",
            "page",
            "",
            json!({"w": 300, "h": 100, "cx": 400, "cy": 0}),
        ),
        node(
            "u",
            "text",
            "b2",
            json!({"x": 20, "y": 30, "runs": [{"t": "둘째 대지의 글"}], "style": {"font": "Pretendard", "weight": 400, "size": 18}, "rotation": 10}),
        ),
    ];
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    assert!(d.check().is_empty());
    d
}

/// 시험 그림 둘: 'a'를 64번 쓴 해시는 40x20 PNG(왼쪽 반 빨강, 오른쪽 반 파랑), 'b'는 20x40 JPEG(초록).
fn canvas_asset(hash: &str) -> Option<Vec<u8>> {
    use skia_safe::{surfaces, Color, EncodedImageFormat, Paint};
    let (w, h, format) = match hash.chars().next()? {
        'a' if hash == "a".repeat(64) => (40, 20, EncodedImageFormat::PNG),
        'b' if hash == "b".repeat(64) => (20, 40, EncodedImageFormat::JPEG),
        _ => return None,
    };
    let mut s = surfaces::raster_n32_premul((w, h)).unwrap();
    let c = s.canvas();
    let mut p = Paint::default();
    if format == EncodedImageFormat::JPEG {
        c.clear(Color::from_rgb(0, 200, 0));
    } else {
        c.clear(Color::from_rgb(255, 0, 0));
        p.set_color(Color::from_rgb(0, 0, 255));
        c.draw_rect(skia_safe::Rect::from_xywh(20.0, 0.0, 20.0, 20.0), &p);
    }
    let img = s.image_snapshot();
    img.peek_pixels().unwrap().encode(format, Some(95))
}

fn canvas_image_size(hash: &str) -> Option<(u32, u32)> {
    match canvas_asset(hash).is_some() {
        true if hash.starts_with('a') => Some((40, 20)),
        true => Some((20, 40)),
        false => None,
    }
}

fn canvas_scene(d: &Document) -> (Layouter, kari_layout::Scene) {
    let mut l = Layouter::new(fonts());
    let s = l.scene(d, &canvas_image_size);
    (l, s)
}

fn write_canvas(name: &str, d: &Document) -> (kari_layout::Scene, String) {
    let (l, s) = canvas_scene(d);
    let pdf = kari_pdf::export_canvas(d, &s, l.fonts(), &META, &canvas_asset).unwrap();
    let path = format!("{}/{name}", env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(&path, pdf).unwrap();
    (s, path)
}

/// 표시 목록을 걸으며 (잘라내기와 경로의 꼭짓점 묶음, 글리프 원점, 링크 경계 상자)를 낸다. 좌표는 대지 왼쪽 위 기준 pt.
type Walked = (Vec<Vec<(f64, f64)>>, Vec<(f64, f64)>, Vec<[f64; 4]>);

fn walk(items: &[Item]) -> Walked {
    let id: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stack = vec![id];
    let (mut groups, mut glyphs, mut links) = (Vec::new(), Vec::new(), Vec::new());
    let at = |m: &M, p: [f32; 2]| {
        let (x, y) = (f64::from(p[0]), f64::from(p[1]));
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    };
    let verts = |m: &M, path: &[kari_layout::Seg]| -> Vec<(f64, f64)> {
        use kari_layout::Seg;
        path.iter()
            .flat_map(|s| match *s {
                Seg::Move(p) | Seg::Line(p) => vec![at(m, p)],
                Seg::Cubic(a, b, p) => vec![at(m, a), at(m, b), at(m, p)],
                Seg::Close => vec![],
            })
            .collect()
    };
    for it in items {
        let cur = *stack.last().unwrap();
        match it {
            Item::Push {
                transform, clip, ..
            } => {
                let t = transform.map(f64::from);
                let m = mul(&t, &cur);
                if let Some(c) = clip {
                    groups.push(verts(&m, c));
                }
                stack.push(m);
            }
            Item::Pop => {
                stack.pop();
            }
            Item::Path { path, .. } => groups.push(verts(&cur, path)),
            Item::Rect { rect: r, .. } => groups.push(vec![
                at(&cur, [r[0], r[1]]),
                at(&cur, [r[0] + r[2], r[1]]),
                at(&cur, [r[0] + r[2], r[1] + r[3]]),
                at(&cur, [r[0], r[1] + r[3]]),
            ]),
            Item::Glyphs { glyphs: g, .. } => {
                glyphs.extend(g.iter().map(|g| at(&cur, [g.x, g.y])));
            }
            Item::Link { rect, .. } => {
                let p = [
                    at(&cur, [rect[0], rect[1]]),
                    at(&cur, [rect[0] + rect[2], rect[1]]),
                    at(&cur, [rect[0], rect[1] + rect[3]]),
                    at(&cur, [rect[0] + rect[2], rect[1] + rect[3]]),
                ];
                let lo = |f: fn(&(f64, f64)) -> f64| p.iter().map(f).fold(f64::MAX, f64::min);
                let hi = |f: fn(&(f64, f64)) -> f64| p.iter().map(f).fold(f64::MIN, f64::max);
                let (x0, y0) = (lo(|q| q.0), lo(|q| q.1));
                links.push([x0, y0, hi(|q| q.0) - x0, hi(|q| q.1) - y0]);
            }
            _ => {}
        }
    }
    (groups, glyphs, links)
}

/// 쪽 하나에서 그린 경로와 잘라내기 경로마다의 꼭짓점들(쪽 왼쪽 위 기준 pt), 나온 차례대로.
fn path_points(pdf: &Pdf, page: &Value, height: f64) -> Vec<Vec<(f64, f64)>> {
    let id: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let (mut ctm, mut saved) = (id, Vec::new());
    let (mut pts, mut args): (Vec<(f64, f64)>, Vec<f64>) = (Vec::new(), Vec::new());
    let mut out = Vec::new();
    let at = |x: f64, y: f64, m: &M| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
    for t in lex(&pdf.content(page)) {
        let op = match t {
            Tok::Num(n) => {
                args.push(n);
                continue;
            }
            Tok::Op(op) => op,
            _ => {
                args.clear();
                continue;
            }
        };
        match op.as_str() {
            "q" => saved.push(ctm),
            "Q" => ctm = saved.pop().unwrap(),
            "cm" => ctm = mul(&args[args.len() - 6..].try_into().unwrap(), &ctm),
            "m" | "l" => pts.push(at(args[0], args[1], &ctm)),
            "c" => pts.extend((0..3).map(|i| at(args[2 * i], args[2 * i + 1], &ctm))),
            "re" => {
                let (x, y, w, h) = (args[0], args[1], args[2], args[3]);
                pts.extend([
                    at(x, y, &ctm),
                    at(x + w, y, &ctm),
                    at(x + w, y + h, &ctm),
                    at(x, y + h, &ctm),
                ]);
            }
            "f" | "f*" | "F" | "S" | "s" | "B" | "B*" | "b" | "b*" | "n" => {
                out.push(pts.drain(..).map(|p| (p.0, height - p.1)).collect());
            }
            _ => {}
        }
        args.clear();
    }
    out
}

fn max_diff(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    assert_eq!(a.len(), b.len(), "점 수");
    a.iter()
        .zip(b)
        .map(|(p, q)| (p.0 - q.0).abs().max((p.1 - q.1).abs()))
        .fold(0.0, f64::max)
}

#[test]
fn 캔버스_쪽은_대지마다_하나이고_쪽_크기가_대지_크기다() {
    let (s, path) = write_canvas("canvas1.pdf", &canvas_doc());
    assert_eq!(s.boards.len(), 2);
    let info = tool("pdfinfo", &[&path]);
    assert_eq!(field(&info, "Pages"), Some("2"));
    let sizes = page_sizes(&path, 2);
    assert_eq!(sizes.len(), 2);
    for ((sw, sh), b) in sizes.iter().zip(&s.boards) {
        assert!((sw - f64::from(b.w)).abs() <= 0.01 && (sh - f64::from(b.h)).abs() <= 0.01);
    }
    assert_eq!(sizes[0], (200.0, 150.0));
    assert_eq!(sizes[1], (300.0, 100.0));
}

#[test]
fn 캔버스_pdf에서_뽑은_글이_원문과_같다() {
    let d = canvas_doc();
    let (_, path) = write_canvas("canvas2.pdf", &d);
    let text = tool("pdftotext", &["-enc", "UTF-8", &path, "-"]);
    let want = format!("{}{}", d.text("t").unwrap(), d.text("u").unwrap());
    assert_eq!(squeeze(&text), squeeze(&want), "{text}");
}

#[test]
fn 캔버스_글꼴이_담겨_있다() {
    let (_, path) = write_canvas("canvas3.pdf", &canvas_doc());
    let out = tool("pdffonts", &[&path]);
    let rows: Vec<&str> = out.lines().skip(2).collect();
    assert_eq!(rows.len(), 2, "굵기 400과 700: {out}");
    for row in rows {
        assert!(
            row.contains("Pretendard") && !row.contains("Type 3"),
            "{row}"
        );
        let f: Vec<&str> = row.split_whitespace().collect();
        assert_eq!((f[f.len() - 5], f[f.len() - 4]), ("yes", "yes"), "{row}");
    }
}

#[test]
fn 캔버스_pdf의_경로_꼭짓점과_글리프_원점과_링크가_표시_목록과_같다() {
    let (s, path) = write_canvas("canvas4.pdf", &canvas_doc());
    let pdf = Pdf::open(&path);
    let (mut paths, mut glyph_n) = (0, 0);
    let (mut worst_path, mut worst_glyph) = (0.0_f64, 0.0_f64);
    for (page, b) in pdf.pages().iter().zip(&s.boards) {
        let h = f64::from(b.h);
        let (want_paths, want_glyphs, want_links) = walk(&b.items);
        let got = path_points(&pdf, page, h);
        assert_eq!(got.len(), want_paths.len(), "{}: 경로 묶음 수", b.id);
        for (g, w) in got.iter().zip(&want_paths) {
            worst_path = worst_path.max(max_diff(g, w));
        }
        paths += got.len();
        let got = origins(&pdf, page, h);
        worst_glyph = worst_glyph.max(max_diff(&got, &want_glyphs));
        glyph_n += got.len();
        // 링크 영역: 돌려도 변환한 경계 상자
        let annots = page["/Annots"].as_array().cloned().unwrap_or_default();
        assert_eq!(annots.len(), want_links.len());
        for (a, w) in annots.iter().zip(&want_links) {
            let a = pdf.get(a);
            assert_eq!(pdf.get(&a["/A"])["/URI"], "u:https://example.com/a");
            let r: Vec<f64> = a["/Rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let got = [r[0], h - r[3], r[2] - r[0], r[3] - r[1]];
            for (g, w) in got.iter().zip(w) {
                assert!((g - w).abs() <= 0.5, "{got:?} / {w:?}");
            }
        }
    }
    println!("경로 묶음 {paths}개 가장 큰 차이 {worst_path:.6}pt, 글리프 {glyph_n}개 가장 큰 차이 {worst_glyph:.6}pt");
    assert!(paths >= 12 && glyph_n >= 10);
    assert!(worst_path <= 0.01, "경로 꼭짓점 {worst_path}pt");
    assert!(worst_glyph <= 0.001, "글리프 원점 {worst_glyph}pt");
}

/// 돌리기와 뒤집기가 겹친 문서: 돌아간 프레임 안의 돌고 뒤집힌 자식, 뒤집힌 프레임 안의 돌아간 자식, 돌아간 선과 글.
#[test]
fn 캔버스에서_돌아간_부모_안의_돌고_뒤집힌_노드도_pdf의_꼭짓점이_표시_목록과_같다() {
    let mut d = Document::new(Kind::Canvas, "돌리기").unwrap();
    let solid = |c: &str| json!([{"type": "SOLID", "color": c}]);
    let node = |id: &str, ty: &str, parent: &str, props: Value| json!({"op": "add", "node": {"id": id, "type": ty, "parent": parent, "props": props}});
    let ops = vec![
        json!({"op": "set", "id": "n1", "props": {"w": 400, "h": 300}}),
        node(
            "f",
            "frame",
            "n1",
            json!({"x": 40, "y": 30, "w": 150, "h": 90, "rotation": 30, "clip": false, "fills": solid("#EEEEEEFF")}),
        ),
        node(
            "k",
            "rect",
            "f",
            json!({"x": 10, "y": 10, "w": 50, "h": 30, "rotation": -40, "flipX": true, "fills": solid("#FF0000FF")}),
        ),
        node(
            "g",
            "frame",
            "n1",
            json!({"x": 220, "y": 40, "w": 120, "h": 80, "flipY": true, "rotation": -160, "fills": solid("#DDDDFFFF")}),
        ),
        node(
            "m",
            "ellipse",
            "g",
            json!({"x": 20, "y": 15, "w": 60, "h": 30, "rotation": 75, "flipY": true, "fills": solid("#00AA00FF")}),
        ),
        node(
            "l",
            "line",
            "n1",
            json!({"x": 60, "y": 220, "w": 120, "rotation": 105, "strokes": solid("#336699FF"), "stroke": {"weight": 3, "align": "CENTER"}}),
        ),
        node(
            "t",
            "text",
            "n1",
            json!({"x": 230, "y": 200, "rotation": -30, "flipX": true, "runs": [{"t": "돌린 글"}], "style": {"font": "Pretendard", "weight": 400, "size": 16}}),
        ),
    ];
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    assert!(d.check().is_empty());
    let (s, path) = write_canvas("canvas-rotated.pdf", &d);
    let pdf = Pdf::open(&path);
    let b = &s.boards[0];
    let h = f64::from(b.h);
    let (want_paths, want_glyphs, _) = walk(&b.items);
    let page = &pdf.pages()[0];
    let got = path_points(&pdf, page, h);
    assert_eq!(got.len(), want_paths.len(), "경로 묶음 수");
    assert!(got.len() >= 5, "{}", got.len());
    let worst = got
        .iter()
        .zip(&want_paths)
        .map(|(g, w)| max_diff(g, w))
        .fold(0.0, f64::max);
    assert!(worst <= 0.01, "경로 꼭짓점 {worst}pt");
    let glyphs = origins(&pdf, page, h);
    assert!(glyphs.len() >= 4);
    let worst = max_diff(&glyphs, &want_glyphs);
    assert!(worst <= 0.001, "글리프 원점 {worst}pt");
}

/// P6 PPM 하나: (너비, 높이, 화소 RGB).
fn ppm(path: &str) -> (usize, usize, Vec<u8>) {
    let b = std::fs::read(path).unwrap();
    let mut at = 0;
    let mut field = || {
        while b[at].is_ascii_whitespace() {
            at += 1;
        }
        let s = at;
        while !b[at].is_ascii_whitespace() {
            at += 1;
        }
        String::from_utf8_lossy(&b[s..at]).to_string()
    };
    assert_eq!(field(), "P6");
    let (w, h): (usize, usize) = (field().parse().unwrap(), field().parse().unwrap());
    assert_eq!(field(), "255");
    at += 1;
    (w, h, b[at..].to_vec())
}

/// 쪽 `n`(1부터)을 72dpi로 그려 (너비, 높이, RGB)로 읽는다.
fn raster(path: &str, n: usize, name: &str) -> (usize, usize, Vec<u8>) {
    let out = format!("{}/{name}", env!("CARGO_TARGET_TMPDIR"));
    let n = n.to_string();
    tool(
        "pdftoppm",
        &["-r", "72", "-f", &n, "-l", &n, "-singlefile", path, &out],
    );
    ppm(&format!("{out}.ppm"))
}

/// 같은 대지를 Skia로 1배율로 그린 (너비, 높이, RGB).
fn skia(s: &kari_layout::Scene, d: &Document, index: usize) -> (usize, usize, Vec<u8>) {
    use kari_render::{draw_canvas, Faces, Frame, Images, IMAGE_BUDGET};
    let files = [
        font_bytes("Pretendard-Regular.ttf"),
        font_bytes("Pretendard-Bold.ttf"),
    ];
    let faces = Faces::new(files.iter().map(Vec::as_slice));
    let b = &s.boards[index];
    let (w, h) = (b.w as i32, b.h as i32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    let mut images = Images::new(Box::new(canvas_asset), IMAGE_BUDGET);
    draw_canvas(
        Frame {
            pixels: &mut px,
            width: w,
            height: h,
            stride: (w * 4) as usize,
            scale: 1.0,
            scroll_x: b.x,
            scroll_y: b.y,
        },
        &faces,
        s,
        [255, 255, 255, 255],
        &mut images,
    )
    .unwrap();
    let _ = d;
    let rgb = px.chunks(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
    (w as usize, h as usize, rgb)
}

#[test]
fn 화면_그리기와_pdf_래스터가_같은_점에서_같은_색이고_프레임_밖은_둘_다_잘린다() {
    let d = canvas_doc();
    let (s, path) = write_canvas("canvas5.pdf", &d);
    let (w, h, a) = skia(&s, &d, 0);
    let (pw, ph, b) = raster(&path, 1, "canvas5-p1");
    assert_eq!((w, h), (pw, ph));
    let px = |v: &[u8], x: usize, y: usize| -> [i32; 3] {
        let i = (y * w + x) * 3;
        [i32::from(v[i]), i32::from(v[i + 1]), i32::from(v[i + 2])]
    };
    // 정해 둔 점: 두 그리기가 모두 이 색이다
    let picks: [((usize, usize), [i32; 3]); 8] = [
        ((50, 40), [255, 0, 0]),
        ((150, 40), [0, 0, 255]),
        ((130, 80), [0, 255, 0]),
        ((150, 105), [0, 255, 0]),
        // 프레임 밖으로 나간 자식: 오른쪽, 왼쪽, 아래
        ((190, 90), [255, 255, 255]),
        ((115, 75), [255, 255, 255]),
        ((150, 112), [255, 255, 255]),
        // 잘린 둥근 모서리: 프레임 왼쪽 위 끝
        ((120, 70), [255, 255, 255]),
    ];
    for (p, want) in picks {
        for (name, v) in [("화면", &a), ("PDF", &b)] {
            let got = px(v, p.0, p.1);
            let ok = got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 6);
            assert!(ok, "{name} {p:?}: {got:?} != {want:?}");
        }
    }
    // 가장자리 안티앨리어싱만 다르다: 크게 다른 화소는 전체의 1.5% 아래
    let far = (0..w * h)
        .filter(|i| (0..3).any(|c| (i32::from(a[i * 3 + c]) - i32::from(b[i * 3 + c])).abs() > 64))
        .count();
    println!("크게 다른 화소 {far} / {}", w * h);
    assert!(far * 1000 < w * h * 15, "크게 다른 화소 {far} / {}", w * h);
}

#[test]
fn 반투명_묶음은_pdf에서도_회색으로_합성된다() {
    let mut d = Document::new(Kind::Canvas, "반투명").unwrap();
    let ops = json!([
        {"op": "set", "id": "n1", "props": {"w": 60, "h": 40}},
        {"op": "add", "node": {"id": "r", "type": "rect", "parent": "n1", "props": {"x": 10, "y": 10, "w": 20, "h": 20, "opacity": 0.5, "fills": [{"type": "SOLID", "color": "#000000FF"}], "strokes": [{"type": "SOLID", "color": "#000000FF"}], "stroke": {"weight": 2, "align": "INSIDE"}}}},
        {"op": "add", "node": {"id": "q", "type": "rect", "parent": "n1", "props": {"x": 35, "y": 10, "w": 20, "h": 20, "opacity": 0.5, "fills": [{"type": "SOLID", "color": "#000000FF"}]}}},
    ]);
    d.apply(&ops, Guard::default()).unwrap();
    let (s, path) = write_canvas("canvas6.pdf", &d);
    let (w, _, b) = raster(&path, 1, "canvas6-p1");
    let (_, _, a) = skia(&s, &d, 0);
    // 칠과 선이 함께인 것은 묶음 하나로, 칠만인 것은 알파에 곱해 합성한다
    for (name, v) in [("화면", &a), ("PDF", &b)] {
        for x in [20, 45] {
            let got = i32::from(v[(20 * w + x) * 3]);
            assert!((got - 128).abs() <= 3, "{name} {x}: {got}");
        }
    }
}

#[test]
fn 캔버스_pdf는_같은_입력이면_같은_바이트이고_대지가_없거나_너무_작으면_오류다() {
    let d = canvas_doc();
    let (l, s) = canvas_scene(&d);
    let go = |s: &kari_layout::Scene| kari_pdf::export_canvas(&d, s, l.fonts(), &META, &|_| None);
    assert_eq!(go(&s).unwrap(), go(&s).unwrap());
    assert!(matches!(
        go(&kari_layout::Scene::default()),
        Err(Error::Pdf(_))
    ));
    let mut tiny = s.clone();
    tiny.boards[0].w = 1.0;
    assert_eq!(go(&tiny), Err(Error::Page));
}

/// (너비, 높이, RGB).
type Shot = (usize, usize, Vec<u8>);

/// 두 그리기의 (너비, 높이, RGB)에서 같은 점의 색이 `tol` 안에서 같고 `want`와도 맞는지 본다.
fn check_points(
    name: &str,
    shots: [(&str, &Shot); 2],
    picks: &[((usize, usize), [i32; 3])],
    tol: i32,
) {
    for (who, (w, _, v)) in shots {
        for (p, want) in picks {
            let i = (p.1 * w + p.0) * 3;
            let got = [i32::from(v[i]), i32::from(v[i + 1]), i32::from(v[i + 2])];
            assert!(
                got.iter().zip(want).all(|(g, w)| (g - w).abs() <= tol),
                "{name} {who} {p:?}: {got:?} != {want:?}"
            );
        }
    }
}

#[test]
fn 잘라낸_프레임_안의_png_채우기와_jpeg_맞추기가_화면과_pdf에서_같다() {
    let mut d = Document::new(Kind::Canvas, "그림").unwrap();
    let img = |a: &str, mode: &str| json!([{"type": "IMAGE", "asset": a.repeat(64), "mode": mode}]);
    let ops = json!([
        {"op": "set", "id": "n1", "props": {"w": 140, "h": 80}},
        {"op": "add", "node": {"id": "f", "type": "frame", "parent": "n1", "props": {"x": 10, "y": 10, "w": 80, "h": 40, "fills": [{"type": "SOLID", "color": "#EEEEEEFF"}]}}},
        {"op": "add", "node": {"id": "p1", "type": "rect", "parent": "f", "props": {"x": 0, "y": 0, "w": 40, "h": 40, "fills": img("a", "FILL")}}},
        {"op": "add", "node": {"id": "p2", "type": "rect", "parent": "f", "props": {"x": 40, "y": 0, "w": 40, "h": 40, "fills": img("b", "FIT")}}},
        {"op": "add", "node": {"id": "p3", "type": "rect", "parent": "f", "props": {"x": 70, "y": 20, "w": 40, "h": 40, "fills": img("a", "FILL")}}},
    ]);
    d.apply(&ops, Guard::default()).unwrap();
    let (s, path) = write_canvas("canvas7.pdf", &d);
    // 목록에 그림 셋이 있고 PDF에 그림 개체가 셋 든다
    let n = s.boards[0]
        .items
        .iter()
        .filter(|i| matches!(i, Item::Image { .. }))
        .count();
    assert_eq!(n, 3);
    let screen = skia(&s, &d, 0);
    let pdf = raster(&path, 1, "canvas7-p1");
    // 틀 (10, 10) 기준. 채우기: 40x20을 40x40에 덮어 가운데를 자른다(빨강 | 파랑). 맞추기: 20x40이 가운데에 들고
    // 양옆은 틀의 회색. 틀 밖으로 나간 셋째 그림은 잘린다(틀 오른쪽 바깥은 대지의 흰색).
    let picks = [
        ((20, 30), [255, 0, 0]),
        ((40, 30), [0, 0, 255]),
        ((55, 30), [238, 238, 238]),
        ((70, 30), [0, 200, 0]),
        ((100, 40), [255, 255, 255]),
        ((100, 60), [255, 255, 255]),
    ];
    check_points("그림", [("화면", &screen), ("PDF", &pdf)], &picks, 14);
}

#[test]
fn 크기가_없는_프레임의_자식은_화면과_pdf_모두에서_보이지_않는다() {
    for (w, h) in [(0, 40), (40, 0), (0, 0)] {
        let mut d = Document::new(Kind::Canvas, "빈 프레임").unwrap();
        let ops = json!([
            {"op": "set", "id": "n1", "props": {"w": 80, "h": 80}},
            {"op": "add", "node": {"id": "f", "type": "frame", "parent": "n1", "props": {"x": 10, "y": 10, "w": w, "h": h}}},
            {"op": "add", "node": {"id": "c", "type": "rect", "parent": "f", "props": {"x": 0, "y": 0, "w": 50, "h": 50, "fills": [{"type": "SOLID", "color": "#FF0000FF"}]}}},
        ]);
        d.apply(&ops, Guard::default()).unwrap();
        let (s, path) = write_canvas("canvas8.pdf", &d);
        let screen = skia(&s, &d, 0);
        let pdf = raster(&path, 1, "canvas8-p1");
        let white = [255, 255, 255];
        let picks = [((20, 20), white), ((40, 40), white), ((55, 55), white)];
        check_points(
            &format!("{w}x{h}"),
            [("화면", &screen), ("PDF", &pdf)],
            &picks,
            2,
        );
    }
}
