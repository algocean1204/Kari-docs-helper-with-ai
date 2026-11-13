//! 보기: 쪽을 보이는 영역 안에 놓는 자리. 단위는 논리 화소, 내용 기준(스크롤을 빼지 않음).

/// 위 여백, 쪽 사이, 아래 여백, 양옆의 최소 여백(논리 화소).
pub const PAD: f32 = 24.0;
/// 100%에서 1pt가 차지하는 논리 화소(96dpi).
const PX_PER_PT: f32 = 4.0 / 3.0;
/// 보기 배율의 범위(1 = 100%).
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 3.0;

/// 쪽의 놓임. `k`는 1pt당 논리 화소, `left`는 쪽 왼쪽, `page_w`·`page_h`는 쪽 크기(논리 화소),
/// `width`는 내용 전체의 너비(보이는 영역보다 좁지 않다. 넓으면 가로로 스크롤한다).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geo {
    pub k: f32,
    pub left: f32,
    pub page_w: f32,
    pub page_h: f32,
    pub pages: usize,
    pub width: f32,
}

impl Geo {
    /// `view_width`는 보이는 영역의 너비(논리 화소). 0 이하면 아직 모르는 것으로 본다.
    /// `zoom`은 보기 배율(1 = 100%)이고 0 이하면 쪽 너비에 맞춘다(양옆 24를 남김). 어느 쪽이든 50~300%로 자른다.
    /// 쪽과 양옆 여백이 영역보다 넓으면 줄이지 않고 내용을 넓힌다(가로 스크롤).
    pub fn new(view_width: f32, page_size: (f32, f32), pages: usize, zoom: f32) -> Geo {
        let (pw, ph) = (page_size.0.max(1.0), page_size.1.max(1.0));
        let z = if zoom > 0.0 {
            zoom
        } else if view_width > 0.0 {
            (view_width - 2.0 * PAD) / pw / PX_PER_PT
        } else {
            1.0
        };
        let k = PX_PER_PT * z.clamp(ZOOM_MIN, ZOOM_MAX);
        let width = (pw * k + 2.0 * PAD).max(view_width);
        Geo {
            k,
            left: (width - pw * k) / 2.0,
            page_w: pw * k,
            page_h: ph * k,
            pages,
            width,
        }
    }

    /// 실제로 쓰는 배율(1 = 100%). 쪽 너비에 맞춤이면 그때 나온 값이다.
    pub fn zoom(&self) -> f32 {
        self.k / PX_PER_PT
    }

    /// 쪽 `page`의 위(내용 기준).
    pub fn top(&self, page: usize) -> f32 {
        PAD + page as f32 * (self.page_h + PAD)
    }

    /// 내용 전체의 높이: 위 여백 + 쪽들과 그 사이 + 아래 여백.
    pub fn content_height(&self) -> f32 {
        PAD + self.pages as f32 * (self.page_h + PAD)
    }

    /// 쪽 기준 pt → 내용 기준 논리 화소.
    pub fn to_px(&self, page: usize, x: f32, y: f32) -> (f32, f32) {
        (self.left + x * self.k, self.top(page) + y * self.k)
    }

    /// 내용 기준 논리 화소 → (쪽, 쪽 기준 pt). 쪽 사이의 틈은 가운데에서 위아래 쪽으로 나눈다.
    pub fn to_pt(&self, x: f32, y: f32) -> (usize, f32, f32) {
        let last = self.pages.max(1) - 1;
        let at = ((y - PAD / 2.0) / (self.page_h + PAD)).floor();
        let page = if at.is_finite() && at > 0.0 {
            (at as usize).min(last)
        } else {
            0
        };
        (
            page,
            (x - self.left) / self.k,
            (y - self.top(page)) / self.k,
        )
    }

    /// 이 보기의 한 점(내용 기준 논리 화소)이 `other` 보기에서 놓이는 자리. 배율을 바꿔도 보던 곳을 지키는 데 쓴다.
    pub fn carry(&self, other: &Geo, x: f32, y: f32) -> (f32, f32) {
        let (page, px, py) = self.to_pt(x, y);
        other.to_px(page, px, py)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_at_100_and_fit() {
        let g = Geo::new(1200.0, (595.0, 842.0), 2, 1.0);
        assert!((g.k - 4.0 / 3.0).abs() < 1e-6);
        assert!((g.left - (1200.0 - 595.0 * g.k) / 2.0).abs() < 1e-3);
        assert!((g.content_height() - (24.0 * 3.0 + 2.0 * g.page_h)).abs() < 1e-3);
        assert_eq!(g.width, 1200.0);
        // 쪽 너비에 맞춤: 양옆 24를 남기고 줄인다
        let n = Geo::new(500.0, (595.0, 842.0), 1, 0.0);
        assert!((n.page_w - 452.0).abs() < 1e-3 && (n.left - 24.0).abs() < 1e-3);
        assert!((n.zoom() - 452.0 / 595.0 / (4.0 / 3.0)).abs() < 1e-5);
    }

    #[test]
    fn zoom_scrolls_sideways_and_stays_in_range() {
        // 200%: 쪽이 영역보다 넓으면 줄이지 않고 내용을 넓힌다
        let g = Geo::new(800.0, (595.0, 842.0), 1, 2.0);
        assert!((g.zoom() - 2.0).abs() < 1e-6);
        assert!((g.width - (595.0 * g.k + 48.0)).abs() < 1e-3);
        assert!((g.left - 24.0).abs() < 1e-3);
        // 범위 밖은 자른다. 아주 넓은 창의 맞춤도 300%를 넘지 않는다
        assert!((Geo::new(800.0, (595.0, 842.0), 1, 9.0).zoom() - 3.0).abs() < 1e-6);
        assert!((Geo::new(800.0, (595.0, 842.0), 1, 0.1).zoom() - 0.5).abs() < 1e-6);
        assert!((Geo::new(1e5, (595.0, 842.0), 1, 0.0).zoom() - 3.0).abs() < 1e-6);
        assert!((Geo::new(0.0, (595.0, 842.0), 1, 0.0).zoom() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn px_and_pt_round_trip() {
        for zoom in [0.5, 1.0, 2.0, 3.0] {
            let g = Geo::new(1000.0, (595.0, 842.0), 3, zoom);
            for page in 0..3 {
                let (x, y) = g.to_px(page, 100.0, 200.0);
                let (p, px, py) = g.to_pt(x, y);
                assert_eq!(p, page);
                assert!((px - 100.0).abs() < 1e-2 && (py - 200.0).abs() < 1e-2);
            }
            assert_eq!(g.to_pt(0.0, -50.0).0, 0);
            assert_eq!(g.to_pt(0.0, 1e9).0, 2);
            assert_eq!(g.to_pt(0.0, f32::NAN).0, 0);
        }
    }

    #[test]
    fn carry_keeps_the_same_paper_point() {
        let a = Geo::new(1000.0, (595.0, 842.0), 50, 1.0);
        let b = Geo::new(1000.0, (595.0, 842.0), 50, 2.0);
        // 30쪽의 (100pt, 200pt)
        let (x, y) = a.to_px(30, 100.0, 200.0);
        let (bx, by) = a.carry(&b, x, y);
        assert_eq!(b.to_pt(bx, by).0, 30);
        let (ex, ey) = b.to_px(30, 100.0, 200.0);
        assert!((bx - ex).abs() < 1e-2 && (by - ey).abs() < 1e-1);
    }
}
