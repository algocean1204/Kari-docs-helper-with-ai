//! 문단의 테두리와 바탕. 문단의 글 상자(왼쪽·오른쪽 들여쓰기를 뺀 폭) 기준이다.
//!
//! 위 선은 첫 줄 위에, 아래 선은 마지막 줄 아래에 `gap`만큼 띄워 긋고, 선과 `gap`은 문단 높이에 더한다.
//! 이웃한 문단의 지정이 같으면 한 상자로 이어 사이의 위·아래 선을 긋지 않는다(배치기가 가른다).

use crate::style::{ParaFormat, Side};
use crate::Item;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Frame {
    /// [위, 아래, 왼쪽, 오른쪽].
    pub sides: [Option<Side>; 4],
    pub shade: Option<[u8; 4]>,
    /// 글 상자의 왼쪽(그릇의 글 상자 왼쪽에서). 목록 표지와 내어쓴 첫 줄이 상자 안에 든다.
    pub x0: f32,
    /// 오른쪽 들여쓰기.
    pub right: f32,
}

impl Frame {
    pub fn of(pf: &ParaFormat) -> Option<Box<Frame>> {
        let d = pf.deco.as_ref()?;
        Some(Box::new(Frame {
            sides: d.border,
            shade: d.shade,
            x0: pf.indent_left.min(pf.left + pf.first),
            right: pf.right,
        }))
    }

    /// 첫 줄 위에 더하는 높이(선 + 틈).
    pub fn top(&self) -> f32 {
        self.sides[0].map_or(0.0, |s| s.weight + s.gap)
    }

    /// 마지막 줄 아래에 더하는 높이.
    pub fn bottom(&self) -> f32 {
        self.sides[1].map_or(0.0, |s| s.weight + s.gap)
    }

    /// 세로 [y0, y1]에 놓인 조각 하나를 그린다. `left`, `width`는 그릇의 글 상자(쪽 좌표).
    /// `top`, `bottom`이 거짓이면 그 끝은 잘린 자리이거나 이어진 상자의 안쪽이라 선을 긋지 않는다.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        items: &mut Vec<Item>,
        left: f32,
        width: f32,
        y0: f32,
        y1: f32,
        top: bool,
        bottom: bool,
    ) {
        if y1 <= y0 {
            return;
        }
        let [t, b, l, r] = self.sides;
        let (xl, xr) = (left + self.x0, left + width - self.right);
        // 바탕은 선 안쪽(왼쪽·오른쪽 선과 글 사이의 틈까지)을 칠한다.
        let sx0 = l.map_or(xl, |s| xl - s.gap);
        let sx1 = r.map_or(xr, |s| xr + s.gap);
        let sy0 = match t {
            Some(s) if top => y0 + s.weight,
            _ => y0,
        };
        let sy1 = match b {
            Some(s) if bottom => y1 - s.weight,
            _ => y1,
        };
        let mut rect = |rect: [f32; 4], color: [u8; 4]| {
            if rect[2] > 0.0 && rect[3] > 0.0 {
                items.push(Item::Rect { rect, color });
            }
        };
        if let Some(c) = self.shade {
            rect([sx0, sy0, sx1 - sx0, sy1 - sy0], c);
        }
        // 위·아래 선은 왼쪽·오른쪽 선의 바깥 끝까지 그어 모서리를 메운다.
        let ox0 = l.map_or(sx0, |s| sx0 - s.weight);
        let ox1 = r.map_or(sx1, |s| sx1 + s.weight);
        if let (Some(s), true) = (t, top) {
            rect([ox0, y0, ox1 - ox0, s.weight], s.color);
        }
        if let (Some(s), true) = (b, bottom) {
            rect([ox0, y1 - s.weight, ox1 - ox0, s.weight], s.color);
        }
        if let Some(s) = l {
            rect([sx0 - s.weight, y0, s.weight, y1 - y0], s.color);
        }
        if let Some(s) = r {
            rect([sx1, y0, s.weight, y1 - y0], s.color);
        }
    }
}

/// 배치기가 문단마다 정한 테두리 높이와 이웃과의 이음.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Edge {
    /// 첫 줄 위, 마지막 줄 아래에 더한 높이.
    pub top: f32,
    pub bottom: f32,
    /// 앞(뒤) 문단과 한 상자로 이어졌다.
    pub up: bool,
    pub down: bool,
}

impl Edge {
    /// 쪽을 다시 쓸지 가르는 값에 섞는다.
    pub fn key(&self) -> u64 {
        ((self.top.to_bits() as u64) << 32 | self.bottom.to_bits() as u64)
            ^ ((self.up as u64) << 1 | self.down as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }
}
