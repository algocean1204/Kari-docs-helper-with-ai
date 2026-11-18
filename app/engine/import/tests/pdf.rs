//! PDF 가져오기 시험. 견본 PDF는 시험 안에서 Kari의 PDF 내보내기로 만들어 다시 읽는다(사람과 회사는 지어낸 것).
//! 암호를 건 견본은 qpdf로 만든다(PDF 내보내기 시험처럼, 없으면 건너뛰지 않고 실패한다). 시간 상한은 release 기준이라
//! debug 빌드에서는 돌지 않거나 시간만 보지 않는다.

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_import::{import, pdf, summary, FragPara, Fragment, Imported, TOO_LARGE};
use kari_layout::{Fonts, Layouter};

const BODY: (f64, f64) = (481.9, 728.5);

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

fn run(bytes: &[u8]) -> Result<Imported, &'static str> {
    import(bytes, BODY, &mut |_| None)
}

/// 견본의 덩이. 원본 문서를 만들고, 가져온 결과를 같은 꼴로 바꿔 견준다.
#[derive(Clone, Debug, PartialEq)]
enum B {
    /// 제목 단계(1~3)와 글. 원본 스타일은 title, h1, h2.
    H(u8, String),
    /// 문단: (글, 굵게)들.
    P(Vec<(String, bool)>),
    Bullet(String),
    Num(String),
    /// 굵은 왼쪽 글과 오른쪽 탭 뒤의 기간.
    Tab(String, String),
    /// 표: 행마다 칸의 글. 첫 행은 굵게(머리 행).
    Table(Vec<Vec<String>>),
}

fn h(l: u8, t: &str) -> B {
    B::H(l, t.into())
}
fn p(t: &str) -> B {
    B::P(vec![(t.into(), false)])
}
fn pb(parts: &[(&str, bool)]) -> B {
    B::P(parts.iter().map(|(t, b)| (t.to_string(), *b)).collect())
}
fn bullet(t: &str) -> B {
    B::Bullet(t.into())
}
fn num(t: &str) -> B {
    B::Num(t.into())
}
fn tab(a: &str, b: &str) -> B {
    B::Tab(a.into(), b.into())
}
fn table(rows: &[&[&str]]) -> B {
    B::Table(
        rows.iter()
            .map(|r| r.iter().map(|c| c.to_string()).collect())
            .collect(),
    )
}

const LONG: &str = "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했고, 트랜잭션 격리 수준을 \
                    바꾼 이유와 부작용을 검토해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.";

/// 한 단 이력서: 제목 세 단계, 오른쪽 탭의 기간, 글머리표·번호 목록, 줄이 넘어가는 문단과 굵은 낱말.
fn one_column() -> Vec<B> {
    vec![
        h(1, "홍길동"),
        p("백엔드 엔지니어 · hello@example.com · 010-0000-0000"),
        h(2, "경력"),
        tab("견본상사 · 서버 개발", "2021.03 – 2024.02"),
        bullet("Kafka 기반 이벤트 파이프라인을 설계했습니다."),
        bullet(LONG),
        bullet("레거시 코드의 테스트 커버리지를 31%에서 74%로 끌어올렸습니다."),
        tab("예시페이 · 결제 플랫폼 개발", "2019.01 – 2021.02"),
        bullet("CI/CD 파이프라인을 옮기고 배포 시간을 25분에서 7분으로 줄였습니다."),
        h(3, "주요 성과"),
        num("정산 배치를 다시 짜 처리 시간을 절반으로 줄였습니다."),
        num("장애 대응 문서를 만들어 평균 복구 시간을 줄였습니다."),
        h(2, "자기소개"),
        pb(&[
            ("저는 ", false),
            ("결제 시스템", true),
            ("을 안정적으로 운영하는 일을 좋아합니다. 작은 지표라도 매일 보고, 문제가 생기면 원인을 끝까지 \
              찾아 같은 장애가 다시 나지 않게 고칩니다. 함께 일한 동료들과 배운 것을 글로 남겨 나눕니다.", false),
        ]),
        p("함께 성장하는 팀에서 일하고 싶습니다."),
    ]
}

/// 표가 있는 이력서: 머리 행이 굵은 학력 표와 항목·내용 표.
fn with_tables() -> Vec<B> {
    vec![
        h(1, "이력서"),
        h(2, "인적 사항"),
        table(&[
            &["항목", "내용"],
            &["성명", "홍길동"],
            &["연락처", "010-0000-0000"],
            &["전자우편", "hello@example.com"],
        ]),
        h(2, "학력"),
        table(&[
            &["기간", "학교", "전공", "구분"],
            &["2016.03 – 2020.02", "한국대학교", "컴퓨터공학", "졸업"],
            &["2013.03 – 2016.02", "서울고등학교", "이과", "졸업"],
        ]),
        h(2, "자격"),
        bullet("정보처리기사 (2020)"),
        bullet("SQLD (2021)"),
    ]
}

fn style_of(level: u8) -> &'static str {
    ["title", "h1", "h2"][usize::from(level - 1)]
}

fn runs_of(b: &B) -> Value {
    match b {
        B::H(_, t) | B::Bullet(t) | B::Num(t) => json!([{"t": t}]),
        B::P(parts) => Value::Array(
            parts
                .iter()
                .map(|(t, bold)| {
                    if *bold {
                        json!({"t": t, "s": {"weight": 700}})
                    } else {
                        json!({"t": t})
                    }
                })
                .collect(),
        ),
        B::Tab(a, b) => json!([{"t": format!("{a}\t{b}"), "s": {"weight": 700}}]),
        B::Table(_) => json!([]),
    }
}

/// 덩이들로 흐름 문서를 만든다(A4, 기본 여백).
fn flow(blocks: &[B]) -> Document {
    let mut d = Document::new(Kind::Flow, "견본").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    let body_w = 595.28 - 2.0 * 56.69;
    for (i, b) in blocks.iter().enumerate() {
        let id = format!("b{i}");
        let props = match b {
            B::H(l, _) => json!({"style": style_of(*l), "runs": runs_of(b)}),
            B::Bullet(_) => {
                json!({"runs": runs_of(b), "pf": {"list": {"kind": "BULLET", "level": 0}}})
            }
            B::Num(_) => {
                json!({"runs": runs_of(b), "pf": {"list": {"kind": "ORDERED", "level": 0}}})
            }
            B::Tab(..) => json!({"runs": runs_of(b),
                "pf": {"tabs": [{"pos": body_w, "align": "RIGHT", "leader": "NONE"}]}}),
            B::P(_) => json!({"runs": runs_of(b)}),
            B::Table(rows) => {
                let n = rows[0].len();
                ops.push(
                    json!({"op": "add", "node": {"id": id, "type": "table", "parent": "root",
                    "props": {"cols": vec![body_w / n as f64; n], "headerRows": 1}}}),
                );
                for (r, row) in rows.iter().enumerate() {
                    let rid = format!("{id}r{r}");
                    ops.push(
                        json!({"op": "add", "node": {"id": rid, "type": "row", "parent": id}}),
                    );
                    for (c, t) in row.iter().enumerate() {
                        let cid = format!("{id}c{r}_{c}");
                        ops.push(json!({"op": "add", "node": {"id": cid, "type": "cell", "parent": rid}}));
                        let s = if r == 0 {
                            json!({"weight": 700})
                        } else {
                            json!({})
                        };
                        ops.push(json!({"op": "add", "node": {"id": format!("{id}p{r}_{c}"), "type": "para",
                            "parent": cid, "props": {"runs": [{"t": t, "s": s}]}}}));
                    }
                }
                continue;
            }
        };
        ops.push(json!({"op": "add", "node": {"id": id, "type": "para", "parent": "root", "props": props}}));
    }
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    d
}

fn flow_pdf(d: &Document) -> Vec<u8> {
    let mut l = Layouter::new(fonts());
    let out = l.layout(d, None).clone();
    kari_pdf::export(d, &out, l.fonts(), &META, &|_| None).unwrap()
}

/// 두 단 이력서(캔버스): 왼쪽 좁은 단(연락처, 기술)과 오른쪽 넓은 단(이름, 경력). 두 단의 줄 간격이 다르다.
/// 읽는 차례는 왼쪽 단, 오른쪽 단이다.
fn two_column() -> (Document, Vec<B>) {
    let mut d = Document::new(Kind::Canvas, "두 단").unwrap();
    let mut ops = vec![json!({"op": "set", "id": "n1", "props": {"w": 595, "h": 842}})];
    let mut n = 0;
    let mut text = |x: f64, y: f64, w: f64, size: f64, runs: Value| {
        n += 1;
        json!({"op": "add", "node": {"id": format!("t{n}"), "type": "text", "parent": "n1", "props": {
            "x": x, "y": y, "w": w, "autoResize": "HEIGHT", "runs": runs,
            "style": {"font": "Pretendard", "weight": 400, "size": size}}}})
    };
    let bold = |t: &str| json!([{"t": t, "s": {"weight": 700}}]);
    let plain = |t: &str| json!([{"t": t}]);
    let left: Vec<(f64, B, Value)> = vec![
        (14.0, h(2, "연락처"), bold("연락처")),
        (10.5, p("hello@example.com"), plain("hello@example.com")),
        (10.5, p("010-0000-0000"), plain("010-0000-0000")),
        (14.0, h(2, "기술"), bold("기술")),
        (10.5, bullet("Rust"), plain("• Rust")),
        (10.5, bullet("Flutter"), plain("• Flutter")),
        (10.5, bullet("PostgreSQL"), plain("• PostgreSQL")),
        (14.0, h(2, "언어"), bold("언어")),
        (10.5, p("한국어, 영어"), plain("한국어, 영어")),
    ];
    let mut y = 75.0;
    let mut want = Vec::new();
    for (size, b, runs) in left {
        ops.push(text(40.0, y, 140.0, size, runs));
        want.push(b);
        y += 27.0;
    }
    let right: Vec<(f64, f64, B, Value)> = vec![
        (24.0, 40.0, h(1, "홍길동"), bold("홍길동")),
        (10.5, 22.0, p("백엔드 엔지니어"), plain("백엔드 엔지니어")),
        (14.0, 24.0, h(2, "경력"), bold("경력")),
        (
            10.5,
            19.0,
            pb(&[("견본상사 · 백엔드 개발", true)]),
            bold("견본상사 · 백엔드 개발"),
        ),
        (10.5, 70.0, p(LONG), plain(LONG)),
        (14.0, 24.0, h(2, "프로젝트"), bold("프로젝트")),
        (
            10.5,
            19.0,
            bullet("주문 이벤트 파이프라인"),
            plain("• 주문 이벤트 파이프라인"),
        ),
        (
            10.5,
            19.0,
            bullet("정산 배치 다시 짜기"),
            plain("• 정산 배치 다시 짜기"),
        ),
    ];
    let mut y = 50.0;
    for (size, step, b, runs) in right {
        ops.push(text(215.0, y, 340.0, size, runs));
        want.push(b);
        y += step;
    }
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    (d, want)
}

fn canvas_pdf(d: &Document) -> Vec<u8> {
    let mut l = Layouter::new(fonts());
    let s = l.scene(d, &|_| None);
    kari_pdf::export_canvas(d, &s, l.fonts(), &META, &|_| None).unwrap()
}

fn runs_text(runs: &[Value]) -> String {
    runs.iter().filter_map(|r| r["t"].as_str()).collect()
}

/// 가져온 조각을 견본과 같은 꼴로 바꾼다.
fn shape(f: &Fragment) -> Vec<B> {
    f.paras
        .iter()
        .map(|p: &FragPara| {
            if let Some(t) = &p.table {
                return B::Table(
                    t.rows
                        .iter()
                        .map(|(_, cells)| {
                            cells
                                .iter()
                                .map(|c| {
                                    c.as_ref()
                                        .map(|(_, ps)| {
                                            ps.iter()
                                                .map(|p| runs_text(&p.runs))
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        })
                                        .unwrap_or_default()
                                })
                                .collect()
                        })
                        .collect(),
                );
            }
            let text = runs_text(&p.runs);
            if let Some(s) = p.props.get("style").and_then(Value::as_str) {
                let l = match s {
                    "h1" => 1,
                    "h2" => 2,
                    _ => 3,
                };
                return B::H(l, text);
            }
            let pf = p.props.get("pf");
            match pf.and_then(|pf| pf["list"]["kind"].as_str()) {
                Some("BULLET") => return B::Bullet(text),
                Some("ORDERED") => return B::Num(text),
                _ => {}
            }
            if pf.is_some_and(|pf| pf.get("tabs").is_some()) {
                if let Some((a, b)) = text.split_once('\t') {
                    return B::Tab(a.into(), b.into());
                }
            }
            let mut parts: Vec<(String, bool)> = Vec::new();
            for r in &p.runs {
                let t = r["t"].as_str().unwrap_or("").to_string();
                let bold = r["s"]["weight"] == json!(700);
                match parts.last_mut() {
                    Some((s, b)) if *b == bold => s.push_str(&t),
                    _ => parts.push((t, bold)),
                }
            }
            B::P(parts)
        })
        .collect()
}

#[test]
fn 한_단_이력서를_제목_목록_탭_굵게와_띄어쓰기까지_옮긴다() {
    let want = one_column();
    let got = run(&flow_pdf(&flow(&want))).unwrap();
    assert_eq!(shape(&got.fragment), want);
    assert_eq!(got.pages, Some(1));
    assert_eq!(got.lost, Vec::<String>::new());
    // 번호 목록은 적힌 번호부터 센다.
    let first_num = got
        .fragment
        .paras
        .iter()
        .find(|p| {
            p.props
                .get("pf")
                .is_some_and(|pf| pf["list"]["kind"] == "ORDERED")
        })
        .unwrap();
    assert_eq!(first_num.props["pf"]["list"]["start"], json!(1));
    let s = summary(&got.fragment);
    assert_eq!(
        s.headings,
        [
            (1, "홍길동".into()),
            (2, "경력".into()),
            (3, "주요 성과".into()),
            (2, "자기소개".into())
        ]
    );
}

#[test]
fn 표가_있는_이력서의_표를_칸과_머리_행으로_옮긴다() {
    let want = with_tables();
    let got = run(&flow_pdf(&flow(&want))).unwrap();
    assert_eq!(shape(&got.fragment), want);
    let tables: Vec<_> = got
        .fragment
        .paras
        .iter()
        .filter_map(|p| p.table.as_ref())
        .collect();
    assert_eq!(tables.len(), 2);
    for t in tables {
        assert_eq!(t.props["headerRows"], json!(1));
        let cols: f64 = t.props["cols"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_f64)
            .sum();
        assert!(
            (cols - BODY.0).abs() < 1.0,
            "열 폭의 합이 본문 폭이다: {cols}"
        );
    }
}

#[test]
fn 두_단_이력서는_왼쪽_단을_읽은_뒤_오른쪽_단을_읽는다() {
    let (d, want) = two_column();
    let got = run(&canvas_pdf(&d)).unwrap();
    assert_eq!(shape(&got.fragment), want);
}

/// 여러 쪽 이력서: 머리글(왼쪽 이름, 오른쪽 "이력서")과 쪽 번호 바닥글.
fn paged() -> (Document, Vec<B>) {
    let mut blocks = vec![h(1, "홍길동")];
    for i in 0..60 {
        blocks.push(bullet(&format!(
            "{i}번째 성과: 요청 키와 상태 머신으로 결제를 다시 짰습니다."
        )));
    }
    let mut d = flow(&blocks);
    d.apply(
        &json!([{"op": "set", "id": "root", "props": {
            "header": {"l": "홍길동", "r": "이력서"},
            "footer": {"c": "{쪽} / {전체}"},
        }}]),
        Guard::default(),
    )
    .unwrap();
    (d, blocks)
}

#[test]
fn 여러_쪽의_머리글과_바닥글은_빼고_알린다() {
    let (d, blocks) = paged();
    let got = run(&flow_pdf(&d)).unwrap();
    assert!(got.pages.unwrap() >= 2);
    assert_eq!(shape(&got.fragment), blocks);
    assert_eq!(got.lost, ["머리글·바닥글(뺌)"]);
}

#[test]
fn 깨진_pdf와_pdf가_아닌_것은_정해진_문장으로_거절한다() {
    let good = flow_pdf(&flow(&[p("견본상사")]));
    assert_eq!(run(b"%PDF-1.7\n garbage").unwrap_err(), pdf::BROKEN);
    // 앞부분만 남은 파일(글이 든 내용 스트림이 잘렸다).
    assert_eq!(run(&good[..good.len() / 3]).unwrap_err(), pdf::BROKEN);
    assert!(run(&good).is_ok());
}

#[test]
fn 큰_파일은_읽기_전에_거절한다() {
    let mut big = b"%PDF-1.7\n".to_vec();
    big.resize(kari_import::MAX_FILE + 1, b' ');
    assert_eq!(run(&big).unwrap_err(), TOO_LARGE);
}

fn qpdf(args: &[&str]) {
    let out = Command::new("qpdf")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("qpdf를 실행하지 못했습니다(qpdf가 있어야 합니다): {e}"));
    assert!(
        out.status.success(),
        "qpdf {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn 여는_암호가_걸린_pdf는_열_수_없다고_알리고_보기_제한만_건_것은_읽는다() {
    let dir = env!("CARGO_TARGET_TMPDIR");
    let src = format!("{dir}/import-plain.pdf");
    std::fs::write(&src, flow_pdf(&flow(&[p("견본상사 백엔드 개발")]))).unwrap();
    for (name, user, bits) in [
        ("user", "u1", "256"),
        ("owner128", "", "128"),
        ("owner256", "", "256"),
    ] {
        let out = format!("{dir}/import-{name}.pdf");
        let mut args = vec!["--encrypt", user, "o1", bits];
        if bits == "128" {
            args.push("--use-aes=y");
        }
        args.extend(["--", &src, &out]);
        qpdf(&args);
        let got = run(&std::fs::read(&out).unwrap());
        if user.is_empty() {
            assert_eq!(
                shape(&got.unwrap().fragment),
                [p("견본상사 백엔드 개발")],
                "{name}"
            );
        } else {
            assert_eq!(got.unwrap_err(), pdf::LOCKED);
        }
    }
}

// ---- 품질: 견본마다 제목·목록·표 칸을 맞춘 비율과 글자 일치율 ----

/// 글로 펼친 꼴(덩이마다 한 줄, 표 칸은 탭).
fn flat(bs: &[B]) -> String {
    bs.iter()
        .map(|b| match b {
            B::H(_, t) | B::Bullet(t) | B::Num(t) => t.clone(),
            B::P(ps) => ps.iter().map(|(t, _)| t.as_str()).collect(),
            B::Tab(a, b) => format!("{a}\t{b}"),
            B::Table(rows) => rows
                .iter()
                .map(|r| r.join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn lev(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(x != y))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

struct Score {
    headings: (usize, usize),
    lists: (usize, usize),
    cells: (usize, usize),
    chars: f64,
    words: f64,
    /// 공백을 뺀 글이 같은가.
    same_text: bool,
    /// (빠진 공백 + 더한 공백) / 원본 공백 수, 한글 음절 사이에 잘못 든 공백 수.
    space_err: f64,
    hangul_gap: usize,
}

/// 공백 자리: 공백을 뺀 글에서 몇 번째 글자 앞에 공백이 있는가.
fn gaps(s: &str) -> (Vec<char>, std::collections::BTreeSet<usize>) {
    let (mut chars, mut at) = (Vec::new(), std::collections::BTreeSet::new());
    for c in s.chars() {
        if c.is_whitespace() {
            at.insert(chars.len());
        } else {
            chars.push(c);
        }
    }
    (chars, at)
}

fn score(want: &[B], got: &[B]) -> Score {
    let hit = |f: &dyn Fn(&B) -> bool| {
        let w: Vec<&B> = want.iter().filter(|b| f(b)).collect();
        (w.iter().filter(|b| got.contains(b)).count(), w.len())
    };
    let headings = hit(&|b| matches!(b, B::H(..)));
    let lists = hit(&|b| matches!(b, B::Bullet(_) | B::Num(_)));
    let mut cells = (0, 0);
    let got_tables: Vec<&Vec<Vec<String>>> = got
        .iter()
        .filter_map(|b| match b {
            B::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    for (k, b) in want
        .iter()
        .filter_map(|b| match b {
            B::Table(t) => Some(t),
            _ => None,
        })
        .enumerate()
    {
        for (r, row) in b.iter().enumerate() {
            for (c, t) in row.iter().enumerate() {
                cells.1 += 1;
                if got_tables.get(k).and_then(|g| g.get(r)?.get(c)) == Some(t) {
                    cells.0 += 1;
                }
            }
        }
    }
    let (a, b): (Vec<char>, Vec<char>) =
        (flat(want).chars().collect(), flat(got).chars().collect());
    let chars = 1.0 - lev(&a, &b) as f64 / a.len().max(b.len()) as f64;
    let words_of = |s: String| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    let (ww, mut gw) = (words_of(flat(want)), words_of(flat(got)));
    let mut same = 0;
    for w in &ww {
        if let Some(i) = gw.iter().position(|g| g == w) {
            gw.swap_remove(i);
            same += 1;
        }
    }
    let ((wc, wg), (gc, gg)) = (gaps(&flat(want)), gaps(&flat(got)));
    let wrong = wg.symmetric_difference(&gg).count();
    let hangul = |c: char| ('가'..='힣').contains(&c);
    let hangul_gap = gg
        .difference(&wg)
        .filter(|&&i| i > 0 && i < gc.len() && hangul(gc[i - 1]) && hangul(gc[i]))
        .count();
    Score {
        headings,
        lists,
        cells,
        chars,
        words: same as f64 / ww.len() as f64,
        same_text: wc == gc,
        space_err: wrong as f64 / wg.len().max(1) as f64,
        hangul_gap,
    }
}

/// 자간을 넓힌 문단(글자 사이 3pt).
fn spaced() -> (Document, Vec<B>) {
    let t = "홍길동 백엔드 엔지니어 · 견본상사에서 Kari로 결제 API를 만들었습니다.";
    let mut d = flow(&[p("x")]);
    d.apply(
        &json!([{"op": "set", "id": "b0", "props": {"runs": [{"t": t, "s": {"spacing": 3}}]}}]),
        Guard::default(),
    )
    .unwrap();
    (d, vec![p(t)])
}

#[test]
fn 견본_다섯의_정확도를_잰다() {
    let (two, two_want) = two_column();
    let (many, many_want) = paged();
    let (sp, sp_want) = spaced();
    let cases: Vec<(&str, Vec<B>, Vec<u8>)> = vec![
        ("한 단", one_column(), flow_pdf(&flow(&one_column()))),
        ("표", with_tables(), flow_pdf(&flow(&with_tables()))),
        ("두 단", two_want, canvas_pdf(&two)),
        ("여러 쪽(머리글·바닥글)", many_want, flow_pdf(&many)),
        ("자간 넓힘", sp_want, flow_pdf(&sp)),
    ];
    let pct = |(a, b): (usize, usize)| {
        if b == 0 {
            100.0
        } else {
            a as f64 * 100.0 / b as f64
        }
    };
    for (name, want, bytes) in cases {
        let got = shape(&run(&bytes).unwrap().fragment);
        let s = score(&want, &got);
        println!(
            "품질 {name}: 제목 {}/{} ({:.0}%), 목록 {}/{} ({:.0}%), 표 칸 {}/{} ({:.0}%), 글자 일치율 {:.1}%, 어절 일치율 {:.1}%, 공백 오류 {:.2}%, 한글 사이 잘못 든 공백 {}",
            s.headings.0, s.headings.1, pct(s.headings),
            s.lists.0, s.lists.1, pct(s.lists),
            s.cells.0, s.cells.1, pct(s.cells),
            s.chars * 100.0, s.words * 100.0, s.space_err * 100.0, s.hangul_gap,
        );
        // 공백 뺀 글 100%, 공백 오류 0.5% 이하, 한글 음절 사이 잘못 든 공백 0.
        assert!(s.same_text, "{name}");
        assert!(s.space_err <= 0.005 && s.hangul_gap == 0, "{name}");
        assert!(s.chars >= 0.99 && s.words >= 0.99, "{name}");
        assert_eq!(
            (pct(s.headings), pct(s.lists), pct(s.cells)),
            (100.0, 100.0, 100.0),
            "{name}"
        );
    }
}

#[test]
fn 자간을_넓힌_글은_글자_사이에_공백을_넣지_않는다() {
    let (d, want) = spaced();
    assert_eq!(shape(&run(&flow_pdf(&d)).unwrap().fragment), want);
}

/// 합친 칸이 있는 표: 첫 행의 앞 칸이 두 열, 둘째 행의 첫 칸이 두 행을 차지한다.
fn merged() -> Document {
    let mut d = Document::new(Kind::Flow, "합친 칸").unwrap();
    let body_w = 595.28 - 2.0 * 56.69;
    let mut ops = vec![
        json!({"op": "del", "id": "n1"}),
        json!({"op": "add", "node": {"id": "t", "type": "table", "parent": "root",
            "props": {"cols": vec![body_w / 3.0; 3]}}}),
    ];
    let cells: [[(Option<&str>, Value); 3]; 3] = [
        [
            (Some("기간과 학교"), json!({"span": {"rows": 1, "cols": 2}})),
            (None, json!({})),
            (Some("구분"), json!({})),
        ],
        [
            (Some("2016 – 2020"), json!({"span": {"rows": 2, "cols": 1}})),
            (Some("한국대학교"), json!({})),
            (Some("졸업"), json!({})),
        ],
        [
            (None, json!({})),
            (Some("서울고등학교"), json!({})),
            (Some("졸업"), json!({})),
        ],
    ];
    for (r, row) in cells.iter().enumerate() {
        ops.push(
            json!({"op": "add", "node": {"id": format!("r{r}"), "type": "row", "parent": "t"}}),
        );
        for (c, (t, props)) in row.iter().enumerate() {
            let mut props = props.clone();
            if t.is_none() {
                props["covered"] = json!(true);
            }
            ops.push(json!({"op": "add", "node": {"id": format!("c{r}{c}"), "type": "cell", "parent": format!("r{r}"), "props": props}}));
            ops.push(json!({"op": "add", "node": {"id": format!("p{r}{c}"), "type": "para", "parent": format!("c{r}{c}"),
                "props": {"runs": [{"t": t.unwrap_or("")}]}}}));
        }
    }
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    d
}

#[test]
fn 선으로_그린_표의_합친_칸을_옮긴다() {
    let got = run(&flow_pdf(&merged())).unwrap();
    let t = got
        .fragment
        .paras
        .iter()
        .find_map(|p| p.table.as_ref())
        .expect("표");
    let cell = |r: usize, c: usize| {
        t.rows[r].1[c].as_ref().map(|(props, ps)| {
            let text: String = ps
                .iter()
                .flat_map(|p| &p.runs)
                .filter_map(|x| x["t"].as_str())
                .collect();
            (props.get("span").cloned(), text)
        })
    };
    assert_eq!(
        cell(0, 0),
        Some((Some(json!({"rows": 1, "cols": 2})), "기간과 학교".into()))
    );
    assert_eq!(cell(0, 1), None);
    assert_eq!(cell(0, 2), Some((None, "구분".into())));
    assert_eq!(
        cell(1, 0),
        Some((Some(json!({"rows": 2, "cols": 1})), "2016 – 2020".into()))
    );
    assert_eq!(cell(1, 1), Some((None, "한국대학교".into())));
    assert_eq!(cell(2, 0), None);
    assert_eq!(cell(2, 2), Some((None, "졸업".into())));
    assert_eq!(got.lost, Vec::<String>::new());
}

#[test]
fn 두_단_쪽의_머리글과_쪽_번호는_본문에_넣지_않는다() {
    // 대지 둘(두 쪽): 같은 자리의 머리글과 쪽 번호, 두 단의 본문.
    let mut d = Document::new(Kind::Canvas, "두 단 두 쪽").unwrap();
    let mut ops = vec![json!({"op": "set", "id": "n1", "props": {"w": 595, "h": 842}})];
    ops.push(
        json!({"op": "add", "node": {"id": "b2", "type": "page", "parent": null,
        "props": {"w": 595, "h": 842, "cx": 700, "cy": 0}}}),
    );
    let mut n = 0;
    let mut text = |board: &str, x: f64, y: f64, w: f64, t: &str| {
        n += 1;
        json!({"op": "add", "node": {"id": format!("t{n}"), "type": "text", "parent": board, "props": {
            "x": x, "y": y, "w": w, "autoResize": "HEIGHT", "runs": [{"t": t}],
            "style": {"font": "Pretendard", "weight": 400, "size": 10.5}}}})
    };
    let mut want = Vec::new();
    for (k, board) in ["n1", "b2"].iter().enumerate() {
        ops.push(text(board, 40.0, 20.0, 300.0, "홍길동 이력서"));
        ops.push(text(board, 290.0, 810.0, 40.0, &format!("{}", k + 1)));
        for i in 0..4 {
            let t = format!("{}쪽 왼쪽 {i}", k + 1);
            ops.push(text(board, 40.0, 80.0 + 27.0 * i as f64, 140.0, &t));
            want.push(p(&t));
        }
        // 오른쪽 단의 줄은 길이가 달라(앞 줄이 짧게 끝나) 한 줄씩 문단이다.
        let right = |i: usize| format!("{}쪽 오른쪽 {i} {}", k + 1, "가".repeat(5 * i));
        for i in 0..5 {
            ops.push(text(board, 215.0, 60.0 + 21.0 * i as f64, 340.0, &right(i)));
        }
        want.extend((0..5).map(|i| p(right(i).trim_end())));
    }
    // 왼쪽 단 넷을 읽은 뒤 오른쪽 단 다섯. 쪽마다 같은 차례.
    d.apply(&Value::Array(ops), Guard::default()).unwrap();
    let got = run(&canvas_pdf(&d)).unwrap();
    assert_eq!(shape(&got.fragment), want);
    assert_eq!(got.lost, ["머리글·바닥글(뺌)"]);
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

/// 이 시험 스레드에서 난 멈춤(panic) 수. 읽기 안의 멈춤은 오류로 바뀌어도 여기에 센다.
static PANICS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn 무작위로_바꾼_pdf_64개는_2초_안에_멈춤_없이_끝난다() {
    let me = std::thread::current().name().map(str::to_string);
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().name().map(str::to_string) == me {
            PANICS.fetch_add(1, Relaxed);
        }
        prev(info);
    }));
    let (d, _) = paged();
    let base = flow_pdf(&d);
    let mut ok = 0;
    for seed in 0..64u64 {
        let mut g = Rng(0x5044_4630 ^ (seed * 0x9E37_79B9 + 1));
        let mut b = base.clone();
        for _ in 0..1 + g.below(3) {
            match g.below(3) {
                // 자르기
                0 => b.truncate(g.below(b.len())),
                // 뒤집기(1~8바이트)
                1 => {
                    for _ in 0..1 + g.below(8) {
                        let at = g.below(b.len());
                        b[at] ^= 1 << g.below(8);
                    }
                }
                // 덧붙이기
                _ => b.extend((0..1 + g.below(64)).map(|_| g.next() as u8)),
            }
        }
        let t = Instant::now();
        let got = run(&b);
        let took = t.elapsed();
        assert!(
            cfg!(debug_assertions) || took < Duration::from_secs(2),
            "씨앗 {seed}: {took:?}"
        );
        if let Ok(g) = got {
            assert!(g.fragment.paras.len() <= 50_000);
            ok += 1;
        }
    }
    let _ = std::panic::take_hook();
    assert_eq!(PANICS.load(Relaxed), 0, "멈춤");
    eprintln!("받은 견본 {ok}개 / 64");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release에서만 잰다")]
fn 백_쪽_pdf의_미리_보기는_5초_안이다() {
    let mut blocks = vec![h(1, "홍길동")];
    while blocks.len() < 2400 {
        blocks.push(h(2, "경력"));
        for _ in 0..20 {
            blocks.push(bullet(LONG));
        }
    }
    let pdf = flow_pdf(&flow(&blocks));
    let mut times: Vec<Duration> = (0..3)
        .map(|_| {
            let t = Instant::now();
            let got = run(&pdf).unwrap();
            assert!(got.pages.unwrap() >= 100, "{:?}", got.pages);
            summary(&got.fragment);
            t.elapsed()
        })
        .collect();
    times.sort();
    eprintln!(
        "100쪽 넘는 PDF 미리 보기(가운데): {:?}, 쪽 {}",
        times[1],
        run(&pdf).unwrap().pages.unwrap()
    );
    assert!(times[1] < Duration::from_secs(5), "{:?}", times[1]);
}
