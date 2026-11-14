//! 캔버스 무작위 걷기: 무작위 문서에 누르기·끌기(옮기기, 크기)·영역 고르기·두 번 누름·취소·되돌리기·다시 하기와
//! 만들기·지우기·복제·속성 바꾸기·글 상자 편집, 돌리기·수정 키 끌기·⌥ 복제·뒤집기·정렬·분배·복사와 붙여넣기를 섞는다.
//!
//! 모형은 편집기의 계산을 되풀이하지 않고 결과만 본다:
//! - 만들기: 놓인 자리의 부모 규칙, 크기와 끈 두 점, 하나만 골라짐. 지우기: 남은 노드 집합. 복제: 개수와 속성과 비킨 자리.
//! - 속성: 고른 노드마다 요청한 값이 그대로 저장됨. 글 편집: 글과 커서를 따로 둔 모형(문자열과 선택)이 문서와 같음.
//! - 옮기기: 고른 노드의 판 위 네 모서리가 포인터의 이동만큼 그대로 옮겨진다.
//! - 크기: 한 노드는 새 상자의 네 모서리가 `기준점 ~ 포인터`로 이루어진 시작 지역 좌표계의 상자와 같은 점들이다
//!   (뒤집힘 여부와 상관없이). 선은 움직이지 않은 끝이 그대로이고 끝이 포인터로 간다.
//! - 돌리기: 고른 노드의 판 위 네 모서리가 눌렀을 때의 상자 가운데 둘레로 포인터가 돈 각만큼 돈 자리다.
//! - 뒤집기: 부모 좌표의 네 모서리가 기준 축의 거울 자리다. 정렬·분배: 경계가 맞고 다른 축과 크기는 그대로다.
//! - 붙여넣기: 개수, 새 id, 속성, 부모 규칙, 뿌리들이 함께 옮겨짐.
//! - 맞춰 끌기(`steps::snap_step`): 붙은 보정이 화면 거리 안이고 붙은 곳(변·가운데·같은 간격)이 있으며, 되돌리기 한 번에 돌아옴.
//! - 모든 걸음: 뒤 사본의 불변식, 끌기 중 커밋 0번, 손을 떼면 최대 한 묶음, 두 사본의 해시, 되돌리기 쌓기.

use kari_doc::serde_json::{json, Value};
use kari_doc::Document;
use kari_edit::{follow_groups, watch_groups, End, Phase};
use kari_layout::{apply, inv, Layouter};

use super::{canvas, steps, CanvasT};
use crate::common::{fonts, Rng};

const SESSIONS: usize = 30;
const STEPS: usize = 60;

/// 걸어 본 것의 합: (커밋한 묶음, 옮기기, 크기 바꾸기, 되돌리기, 영역 고르기).
#[derive(Default, Debug)]
pub struct Tally {
    pub commits: usize,
    pub moves: usize,
    pub resizes: usize,
    pub undos: usize,
    pub marquees: usize,
    pub stale: usize,
    pub creates: usize,
    pub deletes: usize,
    pub dups: usize,
    pub sets: usize,
    pub texts: usize,
    pub rotates: usize,
    pub mods: usize,
    pub alt_dups: usize,
    pub flips: usize,
    pub aligns: usize,
    pub pastes: usize,
    pub snaps: usize,
    /// 맞춰 끌기 걸음 가운데 실제로 붙어 포인터 자리와 달라진 것.
    pub snapped: usize,
    pub groups: usize,
    pub ungroups: usize,
    pub toggles: usize,
    pub orders: usize,
    pub places: usize,
}

/// 되돌리기 쌓기의 모형: 커밋마다의 해시 내력과 지금 자리.
pub struct Hist {
    hash: Vec<String>,
    at: usize,
    seen: usize,
    /// 이미 센 `CanvasT::pres`.
    pres: usize,
}

impl Hist {
    /// 새 커밋이 있으면 내력을 잇는다(다시 하기 쪽은 버린다). 한 걸음이 낸 커밋은 하나다.
    /// 누름이 글 상자 편집을 끝내며 빈 글 상자를 지운 묶음은 따로 센다.
    pub fn sync(&mut self, t: &CanvasT, tag: &str) {
        let pre = t.pres - self.pres;
        self.pres = t.pres;
        assert!(
            t.commits <= self.seen + 1 + pre,
            "{tag}: 한 걸음에 묶음 하나"
        );
        self.sync_all(t, tag);
    }

    /// 커밋이 여럿이어도 커밋마다의 해시로 내력을 잇는다.
    pub fn sync_all(&mut self, t: &CanvasT, tag: &str) {
        while self.seen < t.commits {
            self.hash.truncate(self.at + 1);
            self.hash.push(t.hashes[self.seen].clone());
            self.at += 1;
            self.seen += 1;
        }
        assert_eq!(t.done.len(), self.at, "{tag}: 되돌리기 쌓기 길이");
    }
}

fn fresh() -> Layouter {
    Layouter::new(fonts())
}

fn paint() -> Value {
    json!([{"type": "SOLID", "color": "#336699FF"}])
}

/// 노드 하나의 무작위 속성.
fn props(r: &mut Rng, kind: &str) -> Value {
    let mut p = json!({
        "x": r.below(300) as f64 - 20.0 + r.below(100) as f64 / 10.0,
        "y": r.below(200) as f64 - 20.0,
        "w": (r.below(180) + 1) as f64,
        "h": (r.below(120) + 1) as f64,
    });
    if kind == "line" {
        p["h"] = json!(0);
        p["strokes"] = json!([{"type": "SOLID", "color": "#000000FF"}]);
    } else if kind != "group" && kind != "text" {
        p["fills"] = paint();
    }
    if kind == "text" {
        p["runs"] = json!([{"t": "가나다 abc"}]);
        p["fills"] = json!([{"type": "SOLID", "color": "#000000FF"}]);
        p["autoResize"] = json!(*r.pick(&["WIDTH_AND_HEIGHT", "HEIGHT", "NONE"]));
    }
    if r.chance(40) {
        p["rotation"] = json!(*r.pick(&[15.0, 37.0, 90.0, -60.0, 180.0, -90.0, 45.0]));
    }
    if r.chance(15) {
        p["flipX"] = json!(true);
    }
    if r.chance(10) {
        p["flipY"] = json!(true);
    }
    if r.chance(5) {
        p["visible"] = json!(false);
    }
    if r.chance(5) {
        p["locked"] = json!(true);
    }
    if kind == "frame" && r.chance(40) {
        p["clip"] = json!(false);
    }
    if kind == "rect" && r.chance(30) {
        p["radius"] = json!(r.below(30));
    }
    p
}

fn random_doc(r: &mut Rng) -> Document {
    let mut nodes: Vec<(String, &str, String, Value)> = Vec::new();
    let mut n = 0;
    let next = |n: &mut usize| {
        *n += 1;
        format!("c{n}")
    };
    let mut parents = vec!["n1".to_string()];
    for _ in 0..r.below(14) + 4 {
        let kind = *r.pick(&[
            "rect", "ellipse", "frame", "frame", "line", "text", "group", "rect",
        ]);
        let parent = r.pick(&parents).clone();
        let id = next(&mut n);
        let p = props(r, kind);
        if matches!(kind, "frame" | "group") {
            parents.push(id.clone());
        }
        nodes.push((id, kind, parent, p));
    }
    let mut refs: Vec<(&str, &str, &str, Value)> = nodes
        .iter()
        .map(|(i, k, p, v)| (i.as_str(), *k, p.as_str(), v.clone()))
        .collect();
    if r.chance(40) {
        refs.push((
            "p2",
            "page",
            "",
            json!({"cx": 500, "cy": r.below(100), "w": 300, "h": 250}),
        ));
        refs.push(("q1", "rect", "p2", props(r, "rect")));
    }
    fit_groups(canvas(&refs))
}

/// 그룹의 상자를 자식의 경계에 맞춘 문서(앱에서 만든 그룹은 늘 맞춰져 있다). 빈 그룹은 그대로 둔다.
pub fn fit_groups(mut d: Document) -> Document {
    let touch: Vec<Value> = d
        .nodes()
        .filter(|n| n.kind == "group")
        .filter_map(|g| d.children(Some(&g.id)).first().map(|k| k.id.clone()))
        .map(|k| json!({"op": "set", "id": k, "props": {}}))
        .collect();
    let watch = watch_groups(&d, &touch);
    follow_groups(&mut d, &mut fresh(), &watch, true).unwrap();
    d
}

/// 판 위의 점 하나(노드 안, 손잡이, 이름표, 빈 곳 가운데에서).
pub fn target(r: &mut Rng, t: &mut CanvasT) -> (f64, f64) {
    let ids: Vec<String> = t.back.nodes().map(|n| n.id.clone()).collect();
    match r.below(10) {
        0..=3 => {
            let id = r.pick(&ids).clone();
            let g = t.lay.node_geo(&t.front, &id).unwrap();
            let (u, v) = (r.below(100) as f64 / 100.0, r.below(100) as f64 / 100.0);
            let a = apply(&g.world(), g.w * u, g.h * v);
            (a[0], a[1])
        }
        4..=6 => {
            let c = t.ed.chrome(&t.front, &mut t.lay);
            match c.handles.is_empty() {
                true => (r.below(600) as f64 - 50.0, r.below(400) as f64 - 50.0),
                false => {
                    let h = r.pick(&c.handles).1;
                    // 가끔은 손잡이의 바깥(돌리기 자리): 손잡이들의 가운데에서 멀어지는 쪽으로 화면 10 화소
                    if r.chance(30) {
                        let n = c.handles.len() as f64;
                        let mid = c
                            .handles
                            .iter()
                            .fold([0.0, 0.0], |m, h| [m[0] + h.1[0] / n, m[1] + h.1[1] / n]);
                        let (dx, dy) = (h[0] - mid[0], h[1] - mid[1]);
                        let len = dx.hypot(dy).max(1e-9);
                        return (h[0] + dx / len * 10.0 * t.px, h[1] + dy / len * 10.0 * t.px);
                    }
                    (
                        h[0] + r.below(5) as f64 - 2.0,
                        h[1] + r.below(5) as f64 - 2.0,
                    )
                }
            }
        }
        7 => (r.below(80) as f64, -(r.below(20) as f64) - 3.0),
        _ => (r.below(900) as f64 - 100.0, r.below(500) as f64 - 100.0),
    }
}

/// `p`를 `o` 둘레로 `rad`만큼(화면 기준 시계 방향이 양수) 돌린 점.
pub fn turn(p: [f64; 2], o: [f64; 2], rad: f64) -> [f64; 2] {
    let (s, c) = rad.sin_cos();
    let (x, y) = (p[0] - o[0], p[1] - o[1]);
    [o[0] + c * x - s * y, o[1] + s * x + c * y]
}

/// 고른 것을 감싼 상자의 가운데(판 좌표): 하나면 그 노드의 가운데, 여럿이면 부모 좌표 경계의 가운데.
pub fn frame_center(geos: &[kari_layout::NodeGeo]) -> [f64; 2] {
    if let [g] = geos {
        return apply(&g.world(), g.w / 2.0, g.h / 2.0);
    }
    let pts = geos.iter().flat_map(|g| g.parent_quad());
    let r = pts.fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |r, p| {
        [
            r[0].min(p[0]),
            r[1].min(p[1]),
            r[2].max(p[0]),
            r[3].max(p[1]),
        ]
    });
    apply(
        &geos[0].parent_world,
        (r[0] + r[2]) / 2.0,
        (r[1] + r[3]) / 2.0,
    )
}

pub fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol
}

pub fn run(seed: u64) -> Tally {
    let mut r = Rng(seed);
    let mut tally = Tally::default();
    for session in 0..SESSIONS {
        let doc = random_doc(&mut r);
        let start = doc.hash();
        let mut t = CanvasT::new(doc, fresh);
        t.px = *r.pick(&[1.0, 0.5, 2.0, 0.1]);
        // 되돌리기 쌓기의 모형: 해시의 내력과 지금 자리
        let mut h = Hist {
            hash: vec![start.clone()],
            at: 0,
            seen: 0,
            pres: 0,
        };
        for step in 0..STEPS {
            let tag = format!("씨앗 {seed:#x} 문서 {session} 걸음 {step}");
            let before_commits = t.commits;
            match r.below(200) {
                0..=44 => {
                    let p0 = target(&mut r, &mut t);
                    drag(&mut r, &mut t, p0, &mut tally, &tag);
                    // 누르기가 편집 중이던 빈 글 상자를 지우는 묶음을 끌기의 묶음과 따로 낼 수 있다.
                    h.sync_all(&t, &tag);
                }
                45..=54 => {
                    let p = target(&mut r, &mut t);
                    t.pointer(Phase::Down, p.0, p.1, r.chance(20), r.chance(20));
                    t.pointer(Phase::Up, p.0, p.1, false, false);
                }
                55..=59 => {
                    let p = target(&mut r, &mut t);
                    t.pointer(Phase::Double, p.0, p.1, false, r.chance(20));
                }
                60..=65 => {
                    // 두 점 사이를 끈다(빈 곳에서 시작하면 영역 고르기, 노드에서 시작하면 옮기기)
                    let (p0, p1) = (target(&mut r, &mut t), target(&mut r, &mut t));
                    t.drag(p0, p1);
                    tally.marquees += 1;
                }
                66..=69 => {
                    // ⇧ 영역 고르기: 더하던 선택(부모가 다를 수 있다)과 섞이지 않아야 한다
                    let (p0, p1) = (target(&mut r, &mut t), target(&mut r, &mut t));
                    t.pointer(Phase::Down, p0.0, p0.1, true, false);
                    t.pointer(
                        Phase::Move,
                        (p0.0 + p1.0) / 2.0,
                        (p0.1 + p1.1) / 2.0,
                        true,
                        false,
                    );
                    t.pointer(Phase::Move, p1.0, p1.1, true, false);
                    t.pointer(Phase::Up, p1.0, p1.1, true, false);
                    tally.marquees += 1;
                }
                70..=73 => {
                    // 떼는 신호 없이 끊긴 끌기 뒤 새로 누른다. 또는 끌기 중 두 번 누른다
                    let p0 = target(&mut r, &mut t);
                    t.down(p0.0, p0.1);
                    let p1 = target(&mut r, &mut t);
                    t.mv(p1.0, p1.1);
                    let p2 = target(&mut r, &mut t);
                    if r.chance(50) {
                        t.pointer(Phase::Double, p2.0, p2.1, false, false);
                        assert!(!t.dirty, "{tag}: 두 번 누름이 미리 본 것을 버렸다");
                    } else {
                        t.down(p2.0, p2.1);
                        let p3 = target(&mut r, &mut t);
                        t.mv(p3.0, p3.1);
                        t.mv(p3.0 + 9.0, p3.1 + 4.0);
                    }
                    let p4 = target(&mut r, &mut t);
                    t.up(p4.0, p4.1);
                    tally.stale += 1;
                }
                74..=84 => {
                    if t.undo() {
                        assert!(h.at >= t.steps, "{tag}: 되돌릴 게 없는데 되돌렸다");
                        h.at -= t.steps;
                        tally.undos += 1;
                        assert_eq!(t.back.hash(), h.hash[h.at], "{tag}: 되돌리기 모형");
                    } else {
                        assert_eq!(h.at, 0, "{tag}");
                    }
                }
                85..=89 => {
                    if t.redo() {
                        h.at += t.steps;
                        assert_eq!(t.back.hash(), h.hash[h.at], "{tag}: 다시 하기 모형");
                    } else {
                        assert_eq!(h.at, h.hash.len() - 1, "{tag}");
                    }
                }
                90..=99 => {
                    // 끌기 도중 취소
                    let p0 = target(&mut r, &mut t);
                    t.down(p0.0, p0.1);
                    // 누름이 글 상자 편집을 끝내며 빈 글 상자를 지웠을 수 있다. 그 뒤부터 본다.
                    let (h0, before_commits) = (t.back.hash(), t.commits);
                    let p1 = target(&mut r, &mut t);
                    t.mv((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
                    t.mv(p1.0, p1.1);
                    t.pointer(Phase::Cancel, p1.0, p1.1, false, false);
                    assert_eq!(t.back.hash(), h0, "{tag}: 취소는 문서를 안 바꾼다");
                    assert_eq!(t.commits, before_commits);
                }
                _ => steps::edit_step(&mut r, &mut t, &mut h, &mut tally, &tag),
            }
            // 새 커밋이 있으면 내력을 잇는다(다시 하기 쪽은 버린다). 편집 걸음은 명령마다 이미 이었다.
            // 되돌리기와 다시 하기는 `commits`를 늘리지 않는다. 늘었다면 끌기의 커밋이다.
            h.sync(&t, &tag);
            let _ = before_commits;
            assert!(t.back.check().is_empty(), "{tag}: {:?}", t.back.check());
            // 자식이 있는 그룹의 상자는 늘 자식의 경계다(커밋마다 따라 맞춘다).
            super::layers::groups_fit(&mut t, &tag);
            if !t.dirty {
                assert_eq!(t.front.hash(), t.back.hash(), "{tag}: 두 사본의 해시");
            }
            for id in t.sel() {
                assert!(t.back.node(id).is_some(), "{tag}: 없는 노드를 골랐다");
            }
            // 고른 것은 모두 한 부모의 노드다(손잡이와 크기가 부모 좌표 한 곳에서 계산된다)
            let parents: std::collections::HashSet<_> = t
                .sel()
                .iter()
                .map(|i| t.back.node(i).unwrap().parent.clone())
                .collect();
            assert!(
                parents.len() <= 1,
                "{tag}: 부모가 다른 노드를 함께 골랐다 {:?}",
                t.sel()
            );
        }
        // 끝까지 되돌리면 처음 해시
        while t.undo() {}
        assert_eq!(t.back.hash(), start, "{session}번째 문서를 끝까지 되돌렸다");
        tally.commits += h.hash.len() - 1;
    }
    tally
}

/// 누르기 → 몇 걸음 끌기 → 떼기 한 번과 그 결과를 모형으로 본다.
fn drag(r: &mut Rng, t: &mut CanvasT, p0: (f64, f64), tally: &mut Tally, tag: &str) {
    let down = t.down(p0.0, p0.1);
    // 누름이 글 상자 편집을 끝내며 빈 글 상자를 지웠을 수 있다(그 누름의 묶음이다). 끌기는 그 뒤부터 본다.
    let hash0 = t.back.hash();
    let commits0 = t.commits;
    let handle = down.handle;
    // 눌렀을 때의 선택과 기하(뒤 사본 기준)
    let sel: Vec<String> = t.sel().iter().map(|s| s.to_string()).collect();
    let geos: Vec<_> = sel
        .iter()
        .filter_map(|i| t.lay.node_geo(&t.back, i))
        .collect();
    let steps = r.below(4) + 1;
    let mut last = p0;
    for s in 0..steps {
        // 가끔은 문턱 안, 가끔은 크게
        let reach = *r.pick(&[1.0, 8.0, 40.0, 200.0]);
        last = (
            p0.0 + (r.below(200) as f64 - 100.0) / 100.0 * reach,
            p0.1 + (r.below(200) as f64 - 100.0) / 100.0 * reach,
        );
        t.mv(last.0, last.1);
        assert_eq!(
            t.commits, commits0,
            "{tag}: 끌기 중 커밋 0번 ({s}번째 걸음)"
        );
        assert_eq!(t.back.hash(), hash0, "{tag}: 끌기 중 뒤 사본");
    }
    let out = t.up(last.0, last.1);
    assert!(t.commits <= commits0 + 1, "{tag}");
    assert!(
        !t.ed.dragging() && t.ed.marquee().is_none(),
        "{tag}: 뗐으니 끌기가 끝났다"
    );
    if t.commits == commits0 {
        assert_eq!(t.back.hash(), hash0, "{tag}: 커밋이 없으면 그대로");
        return;
    }
    let delta = (last.0 - p0.0, last.1 - p0.1);
    match out.end {
        End::Commit("옮기기") => {
            tally.moves += 1;
            for (g, id) in geos.iter().zip(&sel) {
                let after = t.lay.node_geo(&t.back, id).unwrap();
                // 잠근 것(레이어 패널에서 함께 골랐어도)은 판에서 끌어도 그대로다.
                let d = if super::steps::locked_up(t, id) {
                    (0.0, 0.0)
                } else {
                    delta
                };
                for (a, b) in g.quad().iter().zip(after.quad()) {
                    assert!(
                        near([a[0] + d.0, a[1] + d.1], b, 5e-3 * t.px.max(1.0) + 3e-3),
                        "{tag}: {id} 옮기기 {a:?} + {d:?} != {b:?}"
                    );
                }
            }
        }
        End::Commit("돌리기") => {
            tally.rotates += 1;
            assert_eq!(handle, Some("rotate"), "{tag}");
            let o = frame_center(&geos);
            let ang = |p: (f64, f64)| (p.1 - o[1]).atan2(p.0 - o[0]);
            let rad = ang(last) - ang(p0);
            for (g, id) in geos.iter().zip(&sel) {
                let after = t.lay.node_geo(&t.back, id).unwrap();
                for (a, b) in g.quad().iter().zip(after.quad()) {
                    let want = turn(*a, o, rad);
                    assert!(
                        near(want, b, 0.02),
                        "{tag}: {id} 돌리기 {a:?} → {want:?} != {b:?}"
                    );
                }
            }
        }
        End::Commit("크기 바꾸기") => {
            tally.resizes += 1;
            let h = handle.expect("손잡이를 눌렀다");
            let [g] = geos.as_slice() else {
                // 여럿: 모두 축에 나란할 때 부모 좌표 경계가 기준점과 포인터 사이다
                return;
            };
            let after = t.lay.node_geo(&t.back, &g.id).unwrap();
            let w0 = g.world();
            let m = inv(&w0).unwrap();
            let q = apply(&m, last.0, last.1);
            if g.kind == "line" {
                let e0 = [apply(&w0, 0.0, 0.0), apply(&w0, g.w, 0.0)];
                let w1 = after.world();
                let e1 = [apply(&w1, 0.0, 0.0), apply(&w1, after.w, 0.0)];
                let moved = usize::from(h == "p1");
                assert!(
                    near(e1[1 - moved], e0[1 - moved], 0.03),
                    "{tag}: 선의 고정 끝 {e1:?} {e0:?}"
                );
                assert!(
                    near(e1[moved], [last.0, last.1], 0.03),
                    "{tag}: 선의 끌린 끝"
                );
                return;
            }
            // 그룹은 자식이 따라 늘어난 뒤 자식의 경계로 맞춰진다: 자식이 모두 축에 나란한 고정 크기일 때만 요청한 상자다.
            if g.kind == "text"
                || g.kind == "page"
                || (g.kind == "group" && !super::steps::plain_group(t, &g.id))
            {
                return;
            }
            // 새 상자 = 시작 지역 좌표계에서 (기준점, 포인터)로 이루어진 상자(안 움직이는 축은 그대로)
            let axis = |at: usize, size: f64, m: f64| -> (f64, f64) {
                match at {
                    1 => (0.0, size),
                    0 => (size.min(m), size.max(m)),
                    _ => (0.0f64.min(m), 0.0f64.max(m)),
                }
            };
            let (ix, iy) = match h {
                "nw" => (0, 0),
                "n" => (1, 0),
                "ne" => (2, 0),
                "e" => (2, 1),
                "se" => (2, 2),
                "s" => (1, 2),
                "sw" => (0, 2),
                _ => (0, 1),
            };
            let (x0, x1) = axis(ix, g.w, q[0]);
            let (y0, y1) = axis(iy, g.h, q[1]);
            let want = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| apply(&w0, p[0], p[1]));
            let got = after.quad();
            for a in got {
                assert!(
                    want.iter().any(|b| near(a, *b, 0.03)),
                    "{tag}: {} 크기 {got:?} / {want:?}",
                    g.id
                );
            }
            for b in want {
                assert!(
                    got.iter().any(|a| near(*a, b, 0.03)),
                    "{tag}: {} 크기(반대) {got:?} / {want:?}",
                    g.id
                );
            }
        }
        other => panic!("{tag}: 커밋 이름이 이상하다: {other:?}"),
    }
}
