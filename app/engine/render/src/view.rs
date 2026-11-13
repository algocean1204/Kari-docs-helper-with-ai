//! 캔버스의 판 보기. 판 좌표와 논리 화소 사이의 변환, 이동, 확대, 맞추기.
//!
//! 상태는 배율과 화면 왼쪽 위(논리 화소 0,0)에 놓인 판 좌표 둘이다. 레티나 배율은 보기가 아니라 그리기의 몫이다
//! (`Frame.scale` = 배율 × 화소 비율).

/// 보기 배율의 범위(1 = 100%): 2%~25600%.
pub const CANVAS_ZOOM_MIN: f64 = 0.02;
pub const CANVAS_ZOOM_MAX: f64 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasView {
    pub zoom: f64,
    /// 화면 왼쪽 위에 놓인 판 좌표.
    pub x: f64,
    pub y: f64,
}

impl Default for CanvasView {
    fn default() -> Self {
        CanvasView {
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
        }
    }
}

fn clamp_zoom(z: f64) -> f64 {
    if z.is_finite() {
        z.clamp(CANVAS_ZOOM_MIN, CANVAS_ZOOM_MAX)
    } else {
        1.0
    }
}

impl CanvasView {
    /// 판 좌표 → 화면(논리 화소).
    pub fn to_screen(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.x) * self.zoom, (y - self.y) * self.zoom)
    }

    /// 화면(논리 화소) → 판 좌표.
    pub fn to_board(&self, sx: f64, sy: f64) -> (f64, f64) {
        (sx / self.zoom + self.x, sy / self.zoom + self.y)
    }

    /// 화면 델타(논리 화소)만큼 내용을 끌어 옮긴다.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.x -= dx / self.zoom;
        self.y -= dy / self.zoom;
    }

    /// 배율을 `zoom`으로 바꾼다(범위로 자른다). 화면의 `(sx, sy)` 아래 판 점은 그대로다.
    pub fn zoom_at(&mut self, zoom: f64, sx: f64, sy: f64) {
        let (bx, by) = self.to_board(sx, sy);
        self.zoom = clamp_zoom(zoom);
        self.x = bx - sx / self.zoom;
        self.y = by - sy / self.zoom;
    }

    /// 판 사각형 `[x, y, w, h]`가 `(width, height)` 영역의 가운데에 `margin`(논리 화소)을 남기고 들어오게 한다.
    /// 100%를 넘겨 키울 수 있다. 크기가 없는 사각형이면 100%로 그 자리를 가운데에 둔다.
    pub fn fit(&mut self, r: [f64; 4], width: f64, height: f64, margin: f64) {
        let (aw, ah) = (
            (width - 2.0 * margin).max(1.0),
            (height - 2.0 * margin).max(1.0),
        );
        self.zoom = if r[2] > 0.0 && r[3] > 0.0 {
            clamp_zoom((aw / r[2]).min(ah / r[3]))
        } else {
            1.0
        };
        self.x = r[0] + r[2] / 2.0 - width / 2.0 / self.zoom;
        self.y = r[1] + r[3] / 2.0 - height / 2.0 / self.zoom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWS: [CanvasView; 4] = [
        CanvasView {
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
        },
        CanvasView {
            zoom: 0.02,
            x: -1e5,
            y: 3e4,
        },
        CanvasView {
            zoom: 256.0,
            x: 12.345,
            y: -0.001,
        },
        CanvasView {
            zoom: 0.37,
            x: 800.5,
            y: -450.25,
        },
    ];

    #[test]
    fn 판에서_화면으로_갔다_오면_제자리다() {
        for v in VIEWS {
            for (x, y) in [
                (0.0, 0.0),
                (1920.0, 1080.0),
                (-5000.5, 7777.25),
                (1e-3, -1e-3),
            ] {
                let (sx, sy) = v.to_screen(x, y);
                let (bx, by) = v.to_board(sx, sy);
                assert!(
                    (bx - x).abs() < 1e-6 && (by - y).abs() < 1e-6,
                    "{v:?} {x},{y}"
                );
            }
            let (bx, by) = v.to_board(33.0, 44.0);
            let (sx, sy) = v.to_screen(bx, by);
            assert!((sx - 33.0).abs() < 1e-6 && (sy - 44.0).abs() < 1e-6);
        }
    }

    #[test]
    fn 커서_아래_판_점이_확대해도_같다() {
        for v in VIEWS {
            for z in [0.02, 0.5, 1.0, 3.7, 256.0] {
                let mut w = v;
                let cursor = (300.0, 200.0);
                let before = w.to_board(cursor.0, cursor.1);
                w.zoom_at(z, cursor.0, cursor.1);
                let after = w.to_board(cursor.0, cursor.1);
                assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
                assert!((w.zoom - z).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn 범위_끝에서_멈춘다() {
        let mut v = CanvasView::default();
        v.zoom_at(1e9, 10.0, 10.0);
        assert_eq!(v.zoom, CANVAS_ZOOM_MAX);
        let at = v.to_board(10.0, 10.0);
        v.zoom_at(v.zoom * 2.0, 10.0, 10.0);
        assert_eq!(v.zoom, CANVAS_ZOOM_MAX);
        assert_eq!(v.to_board(10.0, 10.0), at);
        v.zoom_at(0.0, 10.0, 10.0);
        assert_eq!(v.zoom, CANVAS_ZOOM_MIN);
        v.zoom_at(f64::NAN, 10.0, 10.0);
        assert_eq!(v.zoom, 1.0);
    }

    #[test]
    fn 이동은_화면_델타만큼_내용을_끈다() {
        let mut v = CanvasView {
            zoom: 2.0,
            x: 10.0,
            y: 20.0,
        };
        let p = v.to_screen(50.0, 60.0);
        v.pan(30.0, -8.0);
        let q = v.to_screen(50.0, 60.0);
        assert!((q.0 - p.0 - 30.0).abs() < 1e-9 && (q.1 - p.1 + 8.0).abs() < 1e-9);
    }

    #[test]
    fn 모두_보기는_사각형을_여백과_함께_가운데에_둔다() {
        let mut v = CanvasView::default();
        // 대지 둘을 아우르는 사각형: 가로가 빡빡한 영역
        v.fit([-100.0, 50.0, 4000.0, 1000.0], 1000.0, 800.0, 20.0);
        assert!((v.zoom - 960.0 / 4000.0).abs() < 1e-12);
        let (l, t) = v.to_screen(-100.0, 50.0);
        let (r, b) = v.to_screen(3900.0, 1050.0);
        assert!((l - 20.0).abs() < 1e-9 && (1000.0 - r - 20.0).abs() < 1e-9);
        assert!((t + b - 800.0).abs() < 1e-9, "세로는 가운데");
        // 작은 내용은 100%를 넘겨 키운다
        v.fit([0.0, 0.0, 100.0, 50.0], 1000.0, 800.0, 0.0);
        assert!((v.zoom - 10.0).abs() < 1e-12);
        // 범위로 자른다
        v.fit([0.0, 0.0, 0.001, 0.001], 1000.0, 800.0, 0.0);
        assert_eq!(v.zoom, CANVAS_ZOOM_MAX);
        v.fit([0.0, 0.0, 1e9, 1e9], 1000.0, 800.0, 0.0);
        assert_eq!(v.zoom, CANVAS_ZOOM_MIN);
        // 크기가 없으면 100%로 그 점을 가운데에
        v.fit([200.0, 300.0, 0.0, 0.0], 1000.0, 800.0, 0.0);
        assert_eq!(v.zoom, 1.0);
        assert_eq!(v.to_screen(200.0, 300.0), (500.0, 400.0));
    }
}
