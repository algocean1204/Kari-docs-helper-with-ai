//! 다른 앱의 파일 가져오기(`kari-import`): Word 문서(DOCX), 한글 문서(HWPX), HTML, Google Docs의 웹 페이지 꾸러미(zip),
//! PDF를 붙여넣기와 같은 조각(`Fragment`)으로 읽는다. 조각을 새 문서에 넣는 일은 부르는 쪽(`kari-ffi`)이 한다.
//!
//! 화면, 파일, DB, 스레드를 모른다. 바이트를 받아 조각과 옮기지 못한 것의 목록을 돌려준다. 그림 바이트는 부르는 쪽이
//! 검사해 에셋으로 저장하고 해시를 돌려준다. 아무것도 실행하지 않고, 네트워크의 그림을 내려받지 않는다.
//! 형식은 확장자가 아니라 내용으로 가린다: zip이고 `word/document.xml`이 있으면 DOCX, HWPX 꾸러미면 HWPX, 다른 zip이면
//! 웹 페이지 꾸러미, 처음 1KB에 `%PDF-`가 있으면 PDF, 그 밖은 HTML이다. 옛 한글(HWP), 옛 Word(.doc), 암호가 걸린
//! 문서(OLE, 암호 표시가 있는 zip 항목)는 읽지 않고 까닭만 알린다.

mod docx;
mod html;
mod hwpx;
pub mod pdf;
mod pdftext;
mod xml;
mod zip;

use std::collections::{BTreeSet, HashMap};

pub use hwpx::{ENCRYPTED, HWP_LEGACY, OLD_WORD, UNREADABLE};
use kari_doc::serde_json::Value;
pub use kari_edit::{FragPara, Fragment};

/// 가져오는 파일의 크기 상한(바이트).
pub const MAX_FILE: usize = 50 << 20;
/// 한 파일에서 넣는 그림의 수와 크기 상한. 그림 하나는 그림 넣기와 같은 20MB까지다.
pub const MAX_IMAGES: usize = 300;
pub const MAX_IMAGE: usize = 20 << 20;
pub const MAX_IMAGE_TOTAL: usize = 200 << 20;
/// 한 번에 만드는 문단 수 상한(붙여넣기와 같다).
pub const MAX_PARAS: usize = 50_000;

pub const TOO_LARGE: &str = "50MB가 넘는 파일은 가져올 수 없습니다.";
pub const EMPTY: &str = "가져올 글이 없습니다.";
pub const TOO_MANY_PARAS: &str = "문단이 5만 개를 넘어 가져올 수 없습니다.";

/// 가져온 것: 조각과, 원본에 있었지만 옮기지 못했거나 비슷하게만 옮긴 것(사람이 읽는 한 줄씩).
#[derive(Debug)]
pub struct Imported {
    pub fragment: Fragment,
    pub lost: Vec<String>,
    /// PDF의 쪽 수(다른 형식은 None).
    pub pages: Option<usize>,
}

/// 그림 바이트 → (에셋 해시, 화소 크기). 받을 수 없는 그림(PNG·JPEG가 아님, 깨짐)이면 `None`.
pub type Store<'a> = dyn FnMut(&[u8]) -> Option<(String, (u32, u32))> + 'a;

/// 옮기지 못한 것의 개수.
#[derive(Default)]
pub(crate) struct Report {
    pub fonts: BTreeSet<String>,
    pub image_bad: usize,
    pub image_web: usize,
    pub image_linked: usize,
    pub image_over: usize,
    pub objects: usize,
    pub textboxes: usize,
    pub shapes: usize,
    pub notes: usize,
    pub comments: usize,
    pub header_footer: bool,
    pub math: usize,
    pub links: usize,
    pub nested: usize,
    pub big: usize,
    pub split: usize,
    /// 지운 제어 문자와 개체 자리 글자.
    pub dropped: usize,
    /// 가리킨 글자·문단 모양이 없어 기본 모양으로 둔 곳.
    pub refs: usize,
    pub pdf_images: usize,
    /// 글이 없고 그림이 덮은 쪽(스캔 쪽).
    pub blank_pages: usize,
    pub bad_pages: usize,
    /// 글자 표가 없어 읽지 못한 글자와 그 글꼴 수.
    pub pdf_unknown: usize,
    pub pdf_bad_fonts: usize,
    /// 펼치지 않은 폼(자기를 다시 부르거나 너무 깊게 겹친 것, 읽지 못한 것).
    pub pdf_forms: usize,
    /// 선 없이 열만 맞춘 표(글 문단으로 둠).
    pub pdf_loose_tables: usize,
}

/// 글꼴 목록에 적지 않는 이름: 앱의 글꼴과 CSS의 일반 글꼴.
const GENERIC: [&str; 9] = [
    "pretendard",
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "-apple-system",
    "blinkmacsystemfont",
];

impl Report {
    pub fn font(&mut self, name: &str) {
        let n = name.trim();
        if !n.is_empty() && !GENERIC.contains(&n.to_ascii_lowercase().as_str()) {
            self.fonts.insert(n.chars().take(64).collect());
        }
    }

    fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.fonts.is_empty() {
            let names: Vec<&str> = self.fonts.iter().take(4).map(String::as_str).collect();
            let more = self.fonts.len().saturating_sub(4);
            let more = if more > 0 {
                format!(" 외 {more}개")
            } else {
                String::new()
            };
            out.push(format!(
                "글꼴 {}{more}(Pretendard로 바꿈)",
                names.join(", ")
            ));
        }
        let counts = [
            (
                self.image_bad,
                "그림",
                "개",
                "읽지 못했거나 PNG·JPEG가 아니라 뺌",
            ),
            (self.image_web, "웹 주소의 그림", "개", "내려받지 않아 뺌"),
            (
                self.image_linked,
                "HTML 밖의 그림 파일",
                "개",
                "함께 고른 파일이 아니라 뺌",
            ),
            (
                self.image_over,
                "그림",
                "개",
                "개수나 크기의 상한을 넘어 뺌",
            ),
            (
                self.objects,
                "그림이 아닌 개체",
                "개",
                "SVG, 동영상, 입력 칸 등이라 뺌",
            ),
            (self.textboxes, "글 상자", "개", "뺌"),
            (self.shapes, "도형·차트", "개", "뺌"),
            (
                self.pdf_images,
                "그림",
                "개",
                "PDF의 그림은 아직 옮기지 않아 뺌",
            ),
            (
                self.blank_pages,
                "스캔 쪽",
                "개",
                "글이 없어 뺌. 스캔본은 다음에 지원",
            ),
            (self.bad_pages, "읽지 못한 쪽", "개", "깨져 있어 뺌"),
            (
                self.pdf_forms,
                "펼치지 못한 그림 묶음(폼)",
                "개",
                "자기를 다시 부르거나 너무 깊게 겹쳐 뺌",
            ),
            (self.pdf_loose_tables, "선 없는 표", "개", "글로 둠"),
            (self.notes, "각주·미주", "개", "뺌"),
            (self.comments, "메모", "개", "뺌"),
            (self.math, "수식", "개", "뺌"),
            (self.links, "링크", "개", "주소를 쓸 수 없어 글만 둠"),
            (self.nested, "표 안의 표", "개", "글로 풀어 씀"),
            (self.big, "큰 표", "개", "50행이나 20열을 넘어 글로 풀어 씀"),
            (
                self.split,
                "긴 문단",
                "개",
                "한 문단의 글자 수 상한을 넘어 여러 문단으로 나눔",
            ),
            (
                self.dropped,
                "넣을 수 없는 글자",
                "자",
                "제어 문자 등이라 뺌",
            ),
            (
                self.refs,
                "글자·문단 모양",
                "곳",
                "가리킨 모양이 없어 기본 모양으로 둠",
            ),
        ];
        out.extend(
            counts
                .into_iter()
                .filter(|(n, ..)| *n > 0)
                .map(|(n, what, unit, how)| format!("{what} {n}{unit}({how})")),
        );
        if self.pdf_bad_fonts > 0 {
            out.push(format!(
                "글을 읽을 수 없는 글꼴 {}개(글자 {}개를 옮기지 못함)",
                self.pdf_bad_fonts, self.pdf_unknown
            ));
        }
        if self.header_footer {
            out.push("머리글·바닥글(뺌)".into());
        }
        out
    }
}

/// 가져오는 동안의 그림 저장과 기록.
pub(crate) struct Ctx<'a, 's> {
    store: &'a mut Store<'s>,
    images: usize,
    bytes: usize,
    pub rep: Report,
    pub body: (f64, f64),
    /// 그림 자리(꾸러미 안의 이름, data: 주소)마다 한 번 받은 결과. 같은 그림을 여러 번 가리켜도 한 번만 풀고 검사한다.
    seen: HashMap<String, Got>,
}

/// 그림 자리 하나의 결과.
#[derive(Clone)]
enum Got {
    Ok(String, (u32, u32)),
    Bad,
    Over,
}

impl Ctx<'_, '_> {
    /// 자리 `key`의 그림. 처음이면 `read`로 바이트를 얻어 저장하고, 다시 오면 그 결과를 쓴다(빠진 것도 다시 센다).
    pub fn image_at(
        &mut self,
        key: &str,
        read: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Option<(String, (u32, u32))> {
        let got = match self.seen.get(key) {
            Some(g) => g.clone(),
            None => {
                let (bad, over) = (self.rep.image_bad, self.rep.image_over);
                let got = match read() {
                    Some(b) => match self.image(&b) {
                        Some((a, px)) => Got::Ok(a, px),
                        None if self.rep.image_over > over => Got::Over,
                        None => Got::Bad,
                    },
                    None => Got::Bad,
                };
                // 처음 것은 아래에서 센다.
                (self.rep.image_bad, self.rep.image_over) = (bad, over);
                self.seen.insert(key.to_string(), got.clone());
                got
            }
        };
        match got {
            Got::Ok(a, px) => Some((a, px)),
            Got::Bad => {
                self.rep.image_bad += 1;
                None
            }
            Got::Over => {
                self.rep.image_over += 1;
                None
            }
        }
    }

    /// 그림 바이트를 저장한다. 상한을 넘거나 받지 못하면 센 뒤 `None`.
    pub fn image(&mut self, data: &[u8]) -> Option<(String, (u32, u32))> {
        if self.images >= MAX_IMAGES
            || data.len() > MAX_IMAGE
            || self.bytes + data.len() > MAX_IMAGE_TOTAL
        {
            self.rep.image_over += 1;
            return None;
        }
        match (self.store)(data) {
            Some(got) => {
                self.images += 1;
                self.bytes += data.len();
                Some(got)
            }
            None => {
                self.rep.image_bad += 1;
                None
            }
        }
    }
}

/// 파일 바이트를 읽는다. `body`는 새 문서의 본문 상자(pt)로 표의 열 폭과 그림 크기의 상한이다.
/// 받지 못하면 화면에 보일 한 문장.
pub fn import(bytes: &[u8], body: (f64, f64), store: &mut Store) -> Result<Imported, &'static str> {
    if bytes.len() > MAX_FILE {
        return Err(TOO_LARGE);
    }
    if let Some(why) = hwpx::legacy(bytes) {
        return Err(why);
    }
    let mut cx = Ctx {
        store,
        images: 0,
        bytes: 0,
        rep: Report::default(),
        body,
        seen: HashMap::new(),
    };
    let mut pages = None;
    // zip을 먼저 본다(앞 1KB에 `%PDF-`가 든 zip도 있다).
    let paras = if zip::is_zip(bytes) {
        let z = zip::Zip::open(bytes)?;
        if z.encrypted() {
            return Err(ENCRYPTED);
        }
        if z.has("word/document.xml") {
            docx::read(&z, &mut cx)?
        } else if hwpx::is_hwpx(&z) {
            hwpx::read(&z, &mut cx)?
        } else {
            html::read_zip(&z, &mut cx)?
        }
    } else if pdf::is_pdf(bytes) {
        let (paras, n) = pdf::read(bytes, &mut cx)?;
        pages = Some(n);
        paras
    } else {
        html::read_file(bytes, &mut cx)?
    };
    let mut paras = paras;
    let blank = |p: &FragPara| p.runs.is_empty() && p.table.is_none() && p.props.is_empty();
    while paras.last().is_some_and(blank) {
        paras.pop();
    }
    let lead = paras.iter().take_while(|p| blank(p)).count();
    paras.drain(..lead);
    if paras.is_empty() {
        return Err(EMPTY);
    }
    if paras.len() > MAX_PARAS {
        return Err(TOO_MANY_PARAS);
    }
    Ok(Imported {
        fragment: Fragment {
            text: String::new(),
            paras,
            styles: Default::default(),
        },
        lost: cx.rep.lines(),
        pages,
    })
}

/// 미리 보기에 보일 요약.
#[derive(Debug, PartialEq)]
pub struct Summary {
    /// 표 밖의 문단 수.
    pub paras: usize,
    pub tables: usize,
    pub images: usize,
    /// 글자 수(개체 자리를 뺀 문자 수. 표 칸 안 포함).
    pub chars: usize,
    /// (단계, 글): 제목 0, 부제와 제목 1이 1, 제목 2~6이 2~6. 앞 200개.
    pub headings: Vec<(u8, String)>,
}

pub fn summary(f: &Fragment) -> Summary {
    let runs = f.all_runs();
    let images = runs.iter().filter(|r| r.get("o").is_some()).count();
    let chars = runs
        .iter()
        .filter(|r| r.get("o").is_none())
        .filter_map(|r| r.get("t").and_then(Value::as_str))
        .map(|t| t.chars().count())
        .sum();
    let level = |s: &str| match s {
        "title" => Some(0),
        "subtitle" | "h1" => Some(1),
        _ => s
            .strip_prefix('h')
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (2..=6).contains(n)),
    };
    let headings = f
        .paras
        .iter()
        .filter_map(|p| {
            let lv = level(p.props.get("style")?.as_str()?)?;
            let text: String = p
                .runs
                .iter()
                .filter(|r| r.get("o").is_none())
                .filter_map(|r| r.get("t").and_then(Value::as_str))
                .collect::<String>()
                .replace(['\n', '\t'], " ")
                .chars()
                .take(120)
                .collect();
            Some((lv, text))
        })
        .take(200)
        .collect();
    Summary {
        paras: f.paras.iter().filter(|p| p.table.is_none()).count(),
        tables: f.paras.iter().filter(|p| p.table.is_some()).count(),
        images,
        chars,
        headings,
    }
}

/// 글자 모양 → run의 `s`(문단 스타일 위에 덮는 값).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Look {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<[u8; 3]>,
    pub size: Option<f64>,
    pub vertical: Option<&'static str>,
}

impl Look {
    /// run 하나. `heading`이면 굵기와 크기를 문단 스타일에 맡긴다.
    pub fn run(&self, t: &str, link: Option<&str>, heading: bool) -> Value {
        let mut s = kari_doc::serde_json::Map::new();
        if self.bold && !heading {
            s.insert("weight".into(), 700.into());
        }
        for (on, k) in [
            (self.italic, "italic"),
            (self.underline, "underline"),
            (self.strike, "strike"),
        ] {
            if on {
                s.insert(k.into(), true.into());
            }
        }
        if let (Some([r, g, b]), None) = (self.color, link) {
            s.insert("color".into(), format!("#{r:02X}{g:02X}{b:02X}FF").into());
        }
        if let Some(z) = self.size.filter(|_| !heading) {
            s.insert("size".into(), z.into());
        }
        if let Some(v) = self.vertical {
            s.insert("vertical".into(), v.into());
        }
        let mut run = kari_doc::serde_json::Map::new();
        run.insert("t".into(), t.into());
        if !s.is_empty() {
            run.insert("s".into(), Value::Object(s));
        }
        if let Some(l) = link {
            run.insert("link".into(), l.into());
        }
        Value::Object(run)
    }
}

/// 6자리 16진 색 → 색. 거의 검정(기본 글자색과 같다)과 거의 흰색(흰 종이에서 안 보인다)은 기본으로 둔다(붙여넣기와 같다).
pub(crate) fn hex_color(v: &str) -> Option<[u8; 3]> {
    let v = v.trim().trim_start_matches('#');
    if v.len() != 6 || !v.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).ok();
    let rgb = [p(0)?, p(2)?, p(4)?];
    let (max, min) = (*rgb.iter().max()?, *rgb.iter().min()?);
    (max > 0x33 && min < 0xEE).then_some(rgb)
}

/// 링크로 쓸 주소: http, https, mailto, tel만.
pub(crate) fn link(url: &str) -> Option<String> {
    let u = url.trim();
    let scheme = u.split_once(':')?.0.to_ascii_lowercase();
    ["http", "https", "mailto", "tel"]
        .contains(&scheme.as_str())
        .then(|| kari_edit::normalize_url(u))?
}

#[cfg(test)]
mod tests;
