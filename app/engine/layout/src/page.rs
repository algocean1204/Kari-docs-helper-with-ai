//! 쪽 나눔과 배치 해시. 앞으로만 진행한다.

use sha2::{Digest, Sha256};

use crate::{Item, Page};

/// 부동소수 합의 오차를 견디는 여유(pt).
const EPS: f32 = 1e-3;
/// 표 블록의 앞뒤 간격(pt). 문단 간격과 더해지고, 쪽 맨 위에서는 앞 간격을 뺀다. 셀 안 표의 자리 표시에는 두지 않는다.
pub(crate) const TABLE_BEFORE: f32 = 4.0;
pub(crate) const TABLE_AFTER: f32 = 6.0;
/// 셀 안 표의 자리 표시 높이.
pub(crate) const TABLE_H: f32 = 24.0;

/// 쪽 나눔에 쓰는 블록 하나.
pub(crate) struct Block<'a> {
    /// 문단이면 `Some(문단 번호)`, 표면 `None`.
    pub para: Option<usize>,
    /// 표면 `Some(표 번호)`. `heights`는 행 묶음마다의 높이이고 앞의 `header`개가 머리 행 묶음이다.
    pub table: Option<usize>,
    pub header: usize,
    pub heights: &'a [f32],
    pub before: f32,
    pub after: f32,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub break_before: bool,
}

/// 쪽에 놓인 것 하나. `y`는 본문 위 기준.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Entry {
    Line {
        para: usize,
        line: usize,
        y: f32,
        h: f32,
    },
    /// 표의 행 묶음 하나. `repeat`이면 넘어간 쪽 맨 위에 다시 그린 머리 행이다.
    Row {
        table: usize,
        group: usize,
        y: f32,
        repeat: bool,
    },
}

struct Flow {
    pages: Vec<Vec<Entry>>,
    y: f32,
}

impl Flow {
    fn empty(&self) -> bool {
        self.pages.last().is_none_or(Vec::is_empty)
    }
    fn new_page(&mut self) {
        self.pages.push(Vec::new());
        self.y = 0.0;
    }
}

/// 표 하나를 행 묶음 단위로 놓는다. 행은 나누지 않는다. 머리 행과 첫 본문 행이 이 쪽에 들어가지
/// 않으면 표 전체를 다음 쪽에서 시작한다. 넘어간 쪽마다 맨 위에 머리 행을 다시 놓는다.
/// 한 쪽보다 높은 묶음은 빈 쪽에 놓고 넘치게 둔다.
// 한계: 한 쪽보다 높은 행을 나누지 않는다(넘친다), 긴 셀을 받을 때 행 나누기를 넣는다
fn place_table(f: &mut Flow, b: &Block, table: usize, limit: f32) {
    let hs = b.heights;
    let header = b.header.min(hs.len());
    let need: f32 = hs[..(header + 1).min(hs.len())].iter().sum();
    if !f.empty() && f.y + need > limit {
        f.new_page();
    }
    let push = |f: &mut Flow, group: usize, h: f32, repeat: bool| {
        let y = f.y;
        f.pages
            .last_mut()
            .expect("쪽은 하나 이상")
            .push(Entry::Row {
                table,
                group,
                y,
                repeat,
            });
        f.y += h;
    };
    for (gi, &h) in hs.iter().enumerate() {
        if f.y + h > limit && !f.empty() {
            f.new_page();
            if gi >= header {
                for (hg, &hh) in hs[..header].iter().enumerate() {
                    push(f, hg, hh, true);
                }
            }
        }
        push(f, gi, h, false);
    }
}

/// 외톨이 줄 방지. 문단의 남은 줄이 `rest`개이고 지금 쪽에 `fit`개 들어갈 때 이 쪽에 놓을 줄 수.
/// 다음 쪽으로 한 줄만 가면 하나 덜 놓고, 이 쪽에 한 줄만 남으면 모두 넘긴다. 쪽이 비어 있으면 0이 되지 않는다
/// (쪽마다 최소 한 줄. 한 줄도 들어가지 않는 높은 줄도 빈 쪽에는 놓는다).
pub(crate) fn widow(fit: usize, rest: usize, para: bool, empty: bool) -> usize {
    let mut f = fit.min(rest);
    if para && 0 < f && f < rest {
        if rest - f == 1 {
            f -= 1;
        }
        if f == 1 {
            f = 0;
        }
    }
    if empty {
        f.max(1)
    } else {
        f
    }
}

/// 블록들을 쪽에 놓는다. 쪽마다 놓인 것의 목록. 빈 문서도 쪽 하나.
pub(crate) fn paginate(blocks: &[Block], body_h: f32) -> Vec<Vec<Entry>> {
    let mut f = Flow {
        pages: vec![Vec::new()],
        y: 0.0,
    };
    let limit = body_h + EPS;
    for (bi, b) in blocks.iter().enumerate() {
        if b.break_before && !f.empty() {
            f.new_page();
        }
        let lines: f32 = b.heights.iter().sum();
        let fits_here = |f: &Flow| f.y + b.before + lines <= limit;
        if b.keep_lines && !f.empty() && !fits_here(&f) && lines <= limit {
            f.new_page();
        }
        // keepNext: 이 문단이 쪽에 다 들어가고, 다음 블록에 외톨이 줄 규칙을 적용한 뒤 이 쪽에 놓일 줄이
        // 0이면 함께 넘긴다. 다음 블록이 문단이 아니면 그 첫 조각이 들어가는지로 본다. 쪼개지는 문단은 보지 않는다.
        if b.keep_next && !f.empty() && fits_here(&f) {
            if let Some(n) = blocks.get(bi + 1) {
                let mut y = f.y + b.before + lines + b.after + n.before;
                let fit = n
                    .heights
                    .iter()
                    .take_while(|&&h| {
                        y += h;
                        y <= limit
                    })
                    .count();
                // 다음 블록이 표면 머리 행들과 첫 본문 행이 이 쪽에 들어가야 한다.
                let need = if n.table.is_some() {
                    (n.header + 1).min(n.heights.len())
                } else {
                    1
                };
                let placed = widow(fit, n.heights.len(), n.para.is_some(), false);
                if placed == 0 || placed < need {
                    f.new_page();
                }
            }
        }
        if !f.empty() {
            f.y += b.before;
        }
        if let Some(table) = b.table {
            place_table(&mut f, b, table, limit);
            f.y += b.after;
            continue;
        }
        let n_all = b.heights.len();
        let mut li = 0;
        while li < n_all {
            // 지금 쪽에 들어가는 줄 수.
            let mut y = f.y;
            let fit = b.heights[li..]
                .iter()
                .take_while(|&&h| {
                    y += h;
                    y <= limit
                })
                .count();
            let fit = widow(fit, n_all - li, b.para.is_some(), f.empty());
            if fit == 0 {
                f.new_page();
                continue;
            }
            for (k, &h) in b.heights[li..li + fit].iter().enumerate() {
                let y = f.y;
                let e = Entry::Line {
                    para: b.para.unwrap_or(0),
                    line: li + k,
                    y,
                    h,
                };
                f.pages.last_mut().expect("쪽은 하나 이상").push(e);
                f.y += h;
            }
            li += fit;
            if li < n_all {
                f.new_page();
            }
        }
        f.y += b.after;
    }
    f.pages
}

fn put_f(h: &mut Sha256, v: f32) {
    h.update(((v as f64) * 1000.0).round().to_le_bytes());
}

/// 쪽 하나의 해시. 항목의 종류, 글꼴, 글리프 id, 위치(1/1000pt로 반올림)를 순서대로 넣는다.
pub(crate) fn page_digest(page: &Page) -> [u8; 32] {
    let mut h = Sha256::new();
    for item in &page.items {
        match item {
            Item::Glyphs {
                font, size, glyphs, ..
            } => {
                h.update([1u8]);
                h.update(font.file.to_le_bytes());
                h.update(font.index.to_le_bytes());
                put_f(&mut h, *size);
                h.update((glyphs.len() as u64).to_le_bytes());
                for g in glyphs {
                    h.update(g.id.to_le_bytes());
                    put_f(&mut h, g.x);
                    put_f(&mut h, g.y);
                }
            }
            Item::Rect { rect, .. } => {
                h.update([2u8]);
                rect.iter().for_each(|v| put_f(&mut h, *v));
            }
            Item::Link { rect, .. } => {
                h.update([3u8]);
                rect.iter().for_each(|v| put_f(&mut h, *v));
            }
            Item::Image { rect, asset } => {
                h.update([4u8]);
                rect.iter().for_each(|v| put_f(&mut h, *v));
                h.update(asset.as_bytes());
            }
            // 캔버스 항목은 흐름에 나오지 않는다. 해시를 쓰는 곳이 없어도 종류가 다르면 값이 달라지게만 한다.
            Item::Path { path, fill, stroke } => {
                h.update([5u8]);
                h.update(format!("{path:?}{fill:?}{stroke:?}"));
            }
            push @ Item::Push { .. } => {
                h.update([6u8]);
                h.update(format!("{push:?}"));
            }
            Item::Pop => h.update([7u8]),
        }
    }
    h.finalize().into()
}

/// 문서 전체의 배치 해시: 쪽 크기와 쪽마다의 해시를 이은 것.
pub(crate) fn layout_hash(size: (f32, f32), digests: &[[u8; 32]]) -> String {
    let mut h = Sha256::new();
    h.update(b"kari-layout/1\n");
    put_f(&mut h, size.0);
    put_f(&mut h, size.1);
    h.update((digests.len() as u64).to_le_bytes());
    digests.iter().for_each(|d| h.update(d));
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 줄 기준 항목을 쪽 좌표로 옮긴다.
pub(crate) fn shift(item: &Item, dx: f32, dy: f32) -> Item {
    let mv = |r: &[f32; 4]| [r[0] + dx, r[1] + dy, r[2], r[3]];
    match item {
        Item::Glyphs {
            font,
            size,
            slant,
            color,
            glyphs,
            para,
            range,
            text,
        } => Item::Glyphs {
            font: *font,
            size: *size,
            slant: *slant,
            color: *color,
            glyphs: glyphs
                .iter()
                .map(|g| crate::Glyph {
                    id: g.id,
                    x: g.x + dx,
                    y: g.y + dy,
                    at: g.at,
                })
                .collect(),
            para: para.clone(),
            range: *range,
            text: text.clone(),
        },
        Item::Rect { rect, color } => Item::Rect {
            rect: mv(rect),
            color: *color,
        },
        Item::Link { rect, url } => Item::Link {
            rect: mv(rect),
            url: url.clone(),
        },
        Item::Image { rect, asset } => Item::Image {
            rect: mv(rect),
            asset: asset.clone(),
        },
        // 줄 기준 항목에는 나오지 않는다.
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::widow;

    /// 위 세 규칙을 그대로 적은 정답.
    fn want(fit: usize, rest: usize, empty: bool) -> usize {
        let mut f = fit.min(rest);
        if 0 < f && f < rest {
            if rest - f == 1 {
                f -= 1;
            }
            if f == 1 {
                f = 0;
            }
        }
        if empty && f == 0 {
            1
        } else {
            f
        }
    }

    #[test]
    fn 외톨이_줄_표() {
        // (남은 줄 n, 들어가는 줄 f) → 놓을 줄. 손으로 적은 갈래.
        let table = [
            (1, 0, 0),
            (1, 1, 1),
            (2, 1, 0),
            (2, 2, 2),
            (3, 1, 0),
            (3, 2, 0),
            (3, 3, 3),
            (4, 1, 0),
            (4, 2, 2),
            (4, 3, 2),
            (5, 3, 3),
            (5, 4, 3),
            (6, 1, 0),
            (6, 5, 4),
            (6, 6, 6),
        ];
        for (n, f, out) in table {
            assert_eq!(widow(f, n, true, false), out, "n {n}, f {f}");
        }
        for n in 1..=6 {
            for f in 0..=6 {
                for empty in [false, true] {
                    assert_eq!(
                        widow(f, n, true, empty),
                        want(f, n, empty),
                        "n {n} f {f} {empty}"
                    );
                }
                // 표(문단이 아닌 것)는 줄이 하나라 규칙을 보지 않는다.
                assert_eq!(widow(f, n, false, false), f.min(n));
            }
        }
        // 빈 쪽에서는 0이 되지 않는다.
        assert_eq!(widow(2, 3, true, true), 1);
        assert_eq!(widow(0, 4, true, true), 1);
    }
}
