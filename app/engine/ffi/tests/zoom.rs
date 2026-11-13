//! 보기 배율(`edit.zoom`): 배율마다 커서·선택·찾기 사각형·누른 자리가 맞고, 문서와 PDF는 그대로다.

mod common;

use std::path::PathBuf;

use common::*;
use kari::kari_edit_render_at;
use kari_doc::serde_json::{json, Value};

const ZOOMS: [f64; 4] = [0.5, 1.0, 2.0, 3.0];
const LINE: &str = "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.";

fn caret(s: &Value) -> (f64, f64, f64) {
    let c = &s["caret"];
    (
        c["x"].as_f64().unwrap(),
        c["y"].as_f64().unwrap(),
        c["h"].as_f64().unwrap(),
    )
}

fn state(id: i64) -> Value {
    ok("edit.state", json!({"id": id}))
}

/// 내용 기준 (x, y)를 누른다. 앱처럼 그 자리가 보이게 세로로 스크롤한 채 보이는 영역 기준 y를 보낸다
/// (가로 스크롤은 앱이 x에 더해 보낸다).
fn click(id: i64, x: f64, y: f64, extend: bool) -> Value {
    let scroll = (y - 200.0).max(0.0);
    ok(
        "edit.cmd",
        json!({"id": id, "cmd": {"k": "click", "x": x, "y": y - scroll, "extend": extend},
               "now": 0, "scrollY": scroll}),
    )
}

fn near(a: (f64, f64, f64), b: (f64, f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.05 && (a.1 - b.1).abs() < 0.05 && (a.2 - b.2).abs() < 0.05
}

fn image_path() -> PathBuf {
    PathBuf::from(format!(
        "{}/../render/tests/data/photo.png",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// 두 쪽 넘는 글, 셀에 글이 든 표, 그림 하나가 든 문서.
fn sample() -> i64 {
    let id = new_session();
    let text = vec![LINE; 80].join("\n");
    cmd(id, json!({"k": "paste", "text": text}));
    cmd(id, json!({"k": "enter"}));
    cmd(id, json!({"k": "insertTable", "rows": 2, "cols": 3}));
    insert(id, "표셀");
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    cmd(id, json!({"k": "enter"}));
    let s = ok(
        "edit.insertImage",
        json!({"id": id, "path": image_path(), "now": 0}),
    );
    assert_eq!(s["changed"], true, "{}", s["refused"]);
    // 선택 색을 눈에 띄게(초록) 둔다.
    ok(
        "edit.view",
        json!({"id": id, "width": 1000, "height": 700, "colors": {"selection": "#00FF00FF"}}),
    );
    id
}

#[test]
fn 배율마다_커서_누르기_선택_찾기_표_그림이_맞는다() {
    let _case = Case::new("zoom-points");
    let id = sample();
    let pages = state(id)["pages"].as_u64().unwrap();
    assert!(pages >= 3, "{pages}");
    for z in ZOOMS {
        let s = ok("edit.zoom", json!({"id": id, "zoom": z}));
        assert!((s["zoom"].as_f64().unwrap() - z).abs() < 1e-5);
        let width = s["contentWidth"].as_f64().unwrap();
        let page_w = 595.28 * 4.0 / 3.0 * z;
        assert!(
            (width - (page_w + 48.0).max(1000.0)).abs() < 0.1,
            "{z}: {width}"
        );

        // 첫 줄의 커서 자리들: 그 자리를 누르면 같은 자리, 글자 앞쪽 30%를 누르면 그 글자 앞.
        cmd(id, json!({"k": "move", "to": "docStart"}));
        let mut carets = vec![caret(&state(id))];
        for _ in 0..12 {
            carets.push(caret(&cmd(id, json!({"k": "move", "to": "right"}))));
        }
        for w in carets.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!(
                near(caret(&click(id, a.0, a.1 + a.2 / 2.0, false)), a),
                "{z}: {a:?}"
            );
            let x = a.0 + (b.0 - a.0) * 0.3;
            assert!(
                near(caret(&click(id, x, a.1 + a.2 / 2.0, false)), a),
                "{z}: 글자 앞쪽"
            );
        }
        // 마지막 쪽(문서 끝 바로 앞 문단의 끝)도 같다.
        cmd(id, json!({"k": "move", "to": "docEnd"}));
        let s = cmd(id, json!({"k": "move", "to": "up"}));
        let last = caret(&s);
        assert!(near(
            caret(&click(id, last.0, last.1 + last.2 / 2.0, false)),
            last
        ));

        // 선택: 커서 자리 2~5를 고르면 그 사이가 선택 색으로 그려진다(가로 스크롤을 준 그림에서도).
        let (c2, c5, c6) = (carets[2], carets[5], carets[6]);
        click(id, c2.0, c2.1 + 1.0, false);
        assert_eq!(click(id, c5.0, c5.1 + 1.0, true)["selection"], true);
        let sx = (c2.0 - 300.0).max(0.0).floor();
        let sy = (c2.1 - 200.0).max(0.0).floor();
        let px = render_at(id, sx, sy);
        let green = |x: f64, y: f64| {
            let i = ((y - sy) as usize * 1000 + (x - sx) as usize) * 4;
            let (b, g, r) = (px[i] as i32, px[i + 1] as i32, px[i + 2] as i32);
            g > r + 60 && g > b + 60
        };
        assert!(green((c2.0 + c5.0) / 2.0, c2.1 + 1.0), "{z}: 선택 안");
        assert!(
            !green((c5.0 + c6.0) / 2.0 + 1.0, c2.1 + 1.0),
            "{z}: 선택 밖"
        );

        // 찾기: 지금 일치(선택)의 끝이 찾기 사각형의 오른쪽 끝, 같은 줄.
        ok("edit.find", json!({"id": id, "query": "API"}));
        cmd(id, json!({"k": "move", "to": "docStart"}));
        let s = cmd(id, json!({"k": "findNext", "query": "API"}));
        let end = caret(&s);
        let rects = ok(
            "edit.findRects",
            json!({"id": id, "top": (end.1 - 200.0).max(0.0), "height": 700}),
        )["rects"]
            .as_array()
            .unwrap()
            .clone();
        let hit = rects.iter().any(|r| {
            let (x, y, w) = (
                r["x"].as_f64().unwrap(),
                r["y"].as_f64().unwrap(),
                r["w"].as_f64().unwrap(),
            );
            (x + w - end.0).abs() < 0.05 && (y - end.1).abs() < 0.05
        });
        assert!(hit, "{z}: {end:?} {rects:?}");
        ok("edit.find", json!({"id": id, "query": ""}));

        // 표: 셀 안의 글자 자리를 누르면 셀 안에 커서가 선다.
        let s = cmd(id, json!({"k": "findNext", "query": "표셀"}));
        let cell = caret(&s);
        cmd(id, json!({"k": "move", "to": "docStart"}));
        let s = click(id, cell.0, cell.1 + cell.2 / 2.0, false);
        assert!(s["table"].is_object(), "{z}: 표 안");
        assert!(near(caret(&s), cell), "{z}: 셀 안의 같은 자리");

        // 그림: 그림 가운데를 누르면 그 그림이 골라진다(그림 크기 창이 쓰는 상태).
        let s = cmd(id, json!({"k": "move", "to": "docEnd"}));
        let after = caret(&s);
        assert!(s["image"].is_null());
        let k = 4.0 / 3.0 * z;
        let s = click(id, after.0 - 15.0 * k, after.1 + after.2 - 11.0 * k, false);
        assert!(s["image"].is_object(), "{z}: 그림 고름 {after:?}");
    }
}

/// 1000×700 논리 화소를 배율 1로 그린다.
fn render_at(id: i64, sx: f64, sy: f64) -> Vec<u8> {
    let mut px = vec![0u8; 1000 * 700 * 4];
    let r = unsafe {
        kari_edit_render_at(
            id,
            px.as_mut_ptr(),
            1000,
            700,
            4000,
            1.0,
            sx as f32,
            sy as f32,
        )
    };
    assert_eq!(r, 0);
    px
}

#[test]
fn 가로_스크롤은_그림을_그만큼_옮긴다() {
    let _case = Case::new("zoom-scroll-x");
    let id = sample();
    ok("edit.zoom", json!({"id": id, "zoom": 3}));
    let a = render_at(id, 0.0, 300.0);
    let b = render_at(id, 100.0, 300.0);
    // b의 (x, y) = a의 (x + 100, y)
    let row = |p: &[u8], y: usize, x0: usize, n: usize| {
        p[(y * 1000 + x0) * 4..(y * 1000 + x0 + n) * 4].to_vec()
    };
    let same = (0..700)
        .filter(|y| row(&a, *y, 100, 800) == row(&b, *y, 0, 800))
        .count();
    assert!(same > 690, "같은 줄 {same}/700");
    assert!((0..700).any(|y| row(&a, y, 0, 800) != row(&b, y, 0, 800)));
    // 틀린 값은 거절한다.
    let mut px = vec![0u8; 4];
    let r = unsafe { kari_edit_render_at(id, px.as_mut_ptr(), 1, 1, 4, 1.0, f32::NAN, 0.0) };
    assert_eq!(r, 1);
}

#[test]
fn 배율을_바꿔도_보던_점과_문서와_pdf는_그대로다() {
    let case = Case::new("zoom-same");
    let id = sample();
    cmd(id, json!({"k": "move", "to": "docEnd"}));
    let s = cmd(id, json!({"k": "move", "to": "up"}));
    let (hash, pages) = (s["hash"].clone(), s["pages"].clone());
    let pdf = |name: &str| {
        let path = case.dir.join(name);
        let r = ok("doc.exportPdf", json!({"id": id, "path": path}));
        (r["pages"].clone(), std::fs::read(&path).unwrap())
    };
    let first = pdf("100.pdf");
    let mut at = caret(&s);
    for z in [2.0, 0.5, 0.0, 3.0, 1.0] {
        // 보던 점(지금 커서 자리)을 주면 새 배율에서 그 점의 자리를 돌려준다. 커서도 그 자리로 옮겨 간다.
        let s = ok(
            "edit.zoom",
            json!({"id": id, "zoom": z, "x": at.0, "y": at.1}),
        );
        let now = caret(&s);
        let kept = (
            s["kept"]["x"].as_f64().unwrap(),
            s["kept"]["y"].as_f64().unwrap(),
        );
        assert!(
            (kept.0 - now.0).abs() < 0.05 && (kept.1 - now.1).abs() < 0.1,
            "{z}: {kept:?} {now:?}"
        );
        assert_eq!(s["hash"], hash);
        assert_eq!(s["pages"], pages);
        assert_eq!(s["changed"], false);
        assert_eq!(pdf(&format!("{z}.pdf")), first, "{z}: PDF 바이트");
        at = now;
    }
    assert_eq!(back_hash(id), hash);
    // 틀린 값
    assert_eq!(
        err("edit.zoom", json!({"id": id, "zoom": -1}))["code"],
        "bad_request"
    );
    assert_eq!(
        err("edit.zoom", json!({"id": id, "zoom": 1, "x": 3}))["code"],
        "bad_request"
    );
}
