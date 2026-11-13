//! 시험 공통: 글꼴 읽기, 문서 만들기, 말뭉치.
#![allow(dead_code)]

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Guard, Kind};
use kari_layout::{Fonts, Item, Layout, Layouter, Pos};

/// 이력서 문장 20개.
pub const CORPUS: [&str; 20] = [
    "Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.",
    "Redis 캐시를 도입하면서 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.",
    "PostgreSQL 쿼리 실행 계획을 분석해 인덱스를 재설계하고 느린 쿼리 37건을 제거했습니다.",
    "결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.",
    "Kubernetes 클러스터의 오토스케일링 정책을 조정해 피크 시간대 비용을 28% 절감했습니다.",
    "장애 발생 시 서킷 브레이커와 재시도 정책이 어떻게 동작하는지 문서로 정리했습니다.",
    "팀원 4명과 함께 모놀리식 서비스를 6개의 마이크로서비스로 분리하는 작업을 주도했습니다.",
    "CI/CD 파이프라인을 GitHub Actions로 옮기고 배포 시간을 25분에서 7분으로 단축했습니다.",
    "트랜잭션 격리 수준을 READ COMMITTED에서 REPEATABLE READ로 바꾼 이유와 부작용을 검토했습니다.",
    "대용량 로그를 처리하기 위해 배치 작업을 스트리밍 방식으로 전환했습니다.",
    "서비스 간 통신에 gRPC를 도입하면서 스키마 호환성 규칙을 팀 표준으로 만들었습니다.",
    "부하 테스트 결과를 바탕으로 커넥션 풀 크기와 타임아웃 값을 다시 정했습니다.",
    "사용자 인증 흐름을 OAuth 2.0과 OpenID Connect 기반으로 재구성했습니다.",
    "데이터 정합성 문제를 추적하기 위해 변경 데이터 캡처(CDC)를 적용했습니다.",
    "신규 입사자를 위한 온보딩 문서와 코드 리뷰 가이드를 작성했습니다.",
    "검색 응답 속도를 개선하기 위해 Elasticsearch 인덱스 매핑과 샤드 구성을 조정했습니다.",
    "모니터링 대시보드를 Grafana로 구성하고 알림 기준을 SLO에 맞춰 다시 설계했습니다.",
    "레거시 코드의 테스트 커버리지를 31%에서 74%로 끌어올렸습니다.",
    "동시성 문제를 재현하기 위해 낙관적 락과 비관적 락을 각각 적용해 비교했습니다.",
    "프로젝트 기간: 2023.01 – 2024.05, 역할: 백엔드 개발 및 인프라 운영",
];

pub fn font_bytes(name: &str) -> Vec<u8> {
    let path = format!("{}/../../assets/fonts/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn fonts() -> Fonts {
    let mut f = Fonts::new();
    f.add(font_bytes("Pretendard-Regular.ttf")).unwrap();
    f.add(font_bytes("Pretendard-Bold.ttf")).unwrap();
    f
}

pub fn layouter() -> Layouter {
    Layouter::new(fonts())
}

/// 문단 `p0`, `p1`, …을 가진 문서. `paras`는 문단마다의 props.
pub fn doc(paras: &[Value]) -> Document {
    let mut d = Document::new(Kind::Flow, "시험").unwrap();
    let mut ops = vec![json!({"op": "del", "id": "n1"})];
    for (i, p) in paras.iter().enumerate() {
        ops.push(json!({"op": "add", "node": {"id": format!("p{i}"), "type": "para", "parent": "root", "props": p}}));
    }
    apply(&mut d, Value::Array(ops));
    d
}

/// 글만 가진 문단들.
pub fn text_doc(texts: &[&str]) -> Document {
    let paras: Vec<Value> = texts.iter().map(|t| json!({"runs": [{"t": t}]})).collect();
    doc(&paras)
}

pub fn apply(d: &mut Document, ops: Value) -> kari_doc::Applied {
    d.apply(&ops, Guard::default()).unwrap()
}

/// 루트 속성을 바꾼다.
pub fn set_root(d: &mut Document, props: Value) {
    apply(d, json!([{"op": "set", "id": "root", "props": props}]));
}

/// 쪽 크기와 여백(왼쪽 위 오른쪽 아래 같게)을 정한다.
pub fn set_page(d: &mut Document, w: f64, h: f64, margin: f64) {
    let m = json!({"t": margin, "r": margin, "b": margin, "l": margin});
    set_root(d, json!({"page": {"w": w, "h": h, "margin": m}}));
}

/// 문단의 줄들 (시작, 끝). `line_end`와 `after_wrap`으로 차례로 걷는다.
pub fn lines_of(l: &Layouter, para: &str, len: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut p = Pos::new(para, 0);
    loop {
        let start = l.line_start(&p, true).off;
        let (e, _) = l.line_end(&p, true);
        out.push((start, e.off));
        if e.off >= len {
            return out;
        }
        // 줄바꿈 문자 앞이면 그 뒤가 다음 줄이다.
        let next = if l.caret(&e, true) == l.caret(&e, false) {
            e.off + 1
        } else {
            e.off
        };
        p = Pos::new(para, next);
        assert!(out.len() < 10_000, "줄 걷기가 끝나지 않는다");
    }
}

/// 문단의 모든 커서 자리(`next`로 걷는다).
pub fn positions(l: &Layouter, para: &str) -> Vec<usize> {
    let mut out = vec![0];
    let mut p = Pos::new(para, 0);
    while let Some(n) = l.next(&p) {
        if n.para != para {
            break;
        }
        out.push(n.off);
        p = n;
    }
    out
}

pub fn glyph_items(lay: &Layout) -> impl Iterator<Item = (usize, &Item)> {
    lay.pages
        .iter()
        .enumerate()
        .flat_map(|(i, p)| p.items.iter().map(move |it| (i, it)))
        .filter(|(_, it)| matches!(it, Item::Glyphs { .. }))
}

/// 글자 크기별 글리프 묶음 수와 글꼴. (크기, 글꼴 파일, 기울임)
pub fn styles_of(lay: &Layout, para: &str) -> Vec<(f32, u32, bool)> {
    glyph_items(lay)
        .filter_map(|(_, it)| match it {
            Item::Glyphs {
                size,
                font,
                slant,
                para: p,
                ..
            } if p == para => Some((*size, font.file, *slant)),
            _ => None,
        })
        .collect()
}

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}
