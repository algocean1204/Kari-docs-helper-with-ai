//! 시선 기록. 판정과 구간 만들기는 앱이 하고, 여기서는 앱이 보낸
//! "화면 밖" 구간을 받아 두고 리포트에 실을 모양(질문 번호와 길이)으로 바꾼다. 점수 계산에는 들지 않는다.

use serde::{Deserialize, Serialize};

/// 리포트에 남기는 가장 짧은 구간(ms). 앱도 같은 값으로 거르지만 경계에서 한 번 더 본다.
pub const MIN_MS: u64 = 10_000;

/// "화면 밖" 구간 하나. 시각은 유닉스 ms다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// 구간이 시작될 때의 질문 번호(1부터)
    pub turn: u32,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// 세션 하나의 시선 기록.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gaze {
    /// 시작 시각 순
    pub segments: Vec<Segment>,
    /// 시선 기록을 켜 둔 시간의 합(ms)
    pub tracked_ms: u64,
}

/// 리포트의 한 줄: 질문 번호와 길이(초, 반올림).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub turn: u32,
    pub secs: u64,
}

impl Gaze {
    /// 앱이 보낸 구간을 더한다(기록을 끌 때마다 그때까지의 것을 보낸다). 10초 미만과 거꾸로 된 구간은 버리고,
    /// 이미 받은 시작 시각은 다시 넣지 않는다(같은 것을 두 번 보내도 한 번만 남는다).
    pub fn add(&mut self, segments: Vec<Segment>, tracked_ms: u64) {
        for s in segments {
            let long = s.end_ms >= s.start_ms && s.end_ms - s.start_ms >= MIN_MS;
            if long && !self.segments.iter().any(|x| x.start_ms == s.start_ms) {
                self.segments.push(s);
            }
        }
        self.segments.sort_by_key(|s| s.start_ms);
        self.tracked_ms += tracked_ms;
    }

    /// 리포트에 실을 줄. 구간이 없으면 비었다(화면은 묶음을 보이지 않는다).
    pub fn lines(&self) -> Vec<Line> {
        self.segments
            .iter()
            .map(|s| Line {
                turn: s.turn,
                secs: (s.end_ms - s.start_ms + 500) / 1000,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(turn: u32, start: u64, len: u64) -> Segment {
        Segment {
            turn,
            start_ms: start,
            end_ms: start + len,
        }
    }

    #[test]
    fn keeps_ten_seconds_or_more_once_and_rounds_to_seconds() {
        let mut g = Gaze::default();
        g.add(
            vec![
                seg(2, 50_000, 12_499),
                seg(1, 1_000, 10_000),
                seg(1, 30_000, 9_999),
            ],
            60_000,
        );
        // 거꾸로 된 구간은 버린다.
        g.add(
            vec![Segment {
                turn: 3,
                start_ms: 90_000,
                end_ms: 70_000,
            }],
            0,
        );
        // 같은 것을 다시 보내도 한 번만.
        g.add(vec![seg(1, 1_000, 10_000), seg(3, 80_000, 15_500)], 30_000);
        assert_eq!(
            g.lines(),
            vec![
                Line { turn: 1, secs: 10 },
                Line { turn: 2, secs: 12 },
                Line { turn: 3, secs: 16 },
            ]
        );
        assert_eq!(g.tracked_ms, 90_000);
        // 저장 모양(camelCase)과 되읽기.
        let v = serde_json::to_value(&g).unwrap();
        assert_eq!(v["trackedMs"], 90_000);
        assert_eq!(v["segments"][0]["startMs"], 1_000);
        assert_eq!(serde_json::from_value::<Gaze>(v).unwrap(), g);
    }
}
