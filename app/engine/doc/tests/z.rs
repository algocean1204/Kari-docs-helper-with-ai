//! z 키: 순서, 유효성, 길이 표 다섯 줄, 무작위 자리에 20,000번 넣기, 틀린 입력.

use kari_doc::{z_between, z_n_between, Document, Error, Guard, Kind};
use serde_json::json;

/// 시험 쪽에서 따로 쓴 형식 검사(구현의 검사에 기대지 않는다).
fn valid(k: &str) -> bool {
    let b = k.as_bytes();
    let Some(&head) = b.first() else { return false };
    let n = match head {
        b'a'..=b'z' => (head - b'a') as usize + 2,
        b'A'..=b'Z' => (b'Z' - head) as usize + 2,
        _ => return false,
    };
    let smallest = format!("A{}", "0".repeat(26));
    b.len() <= 1024
        && b.len() >= n
        && b.iter().all(u8::is_ascii_alphanumeric)
        && !(b.len() > n && b.ends_with(b"0"))
        && k[..n] != smallest
}

fn between(a: Option<&str>, b: Option<&str>) -> String {
    let k = z_between(a, b).unwrap_or_else(|| panic!("{a:?} {b:?}"));
    assert!(valid(&k), "{k}");
    assert!(
        a.is_none_or(|a| a < k.as_str()) && b.is_none_or(|b| k.as_str() < b),
        "{a:?} < {k} < {b:?}"
    );
    k
}

#[test]
fn 표의_순서대로_고른다() {
    let cases = [
        (None, None, "a0"),
        (Some("a0"), None, "a1"),
        (Some("az"), None, "b00"),
        (Some("a0V"), None, "a1"),
        (None, Some("a0"), "Zz"),
        (None, Some("Z0"), "Yzz"),
        (None, Some("a0V"), "Zz"),
        // 사이: a의 바로 다음
        (Some("a0"), Some("a1"), "a01"),
        (Some("a0z"), Some("a1"), "a0z1"),
        (Some("a0V"), Some("a1"), "a0W"),
        // b의 바로 앞
        (Some("a0"), Some("a01"), "a00z"),
        (Some("a0V"), Some("a0V1"), "a0V0z"),
        (Some("a0V"), Some("a0W"), "a0VV"),
        // 중간값
        (Some("a01"), Some("a02"), "a01V"),
    ];
    for (a, b, want) in cases {
        assert_eq!(between(a, b), want, "{a:?} {b:?}");
    }
}

#[test]
fn 길이_표_다섯_줄() {
    let longest = |keys: &[String]| keys.iter().map(String::len).max().unwrap();
    let mut rows = Vec::new();

    let mut keys = vec![between(None, None)];
    for _ in 0..100_000 {
        keys.push(between(Some(keys.last().unwrap()), None));
    }
    rows.push(("맨 뒤에 이어서 100,000", longest(&keys), 8));

    let mut keys = vec![between(None, None)];
    for _ in 0..100_000 {
        keys.push(between(None, Some(keys.last().unwrap())));
    }
    rows.push(("맨 앞에 이어서 100,000", longest(&keys), 8));

    let (lo, hi) = ("a0".to_string(), "a1".to_string());
    let mut keys = vec![lo.clone()];
    for _ in 0..10_000 {
        keys.push(between(Some(keys.last().unwrap()), Some(&hi)));
    }
    rows.push(("같은 자리 뒤로 10,000", longest(&keys), 200));

    let mut keys = vec![hi.clone()];
    for _ in 0..10_000 {
        keys.push(between(Some(&lo), Some(keys.last().unwrap())));
    }
    rows.push(("같은 자리 앞으로 10,000", longest(&keys), 200));

    for (a, b) in [
        (None, None),
        (Some("a0"), Some("a1")),
        (Some("a0"), Some("a01")),
    ] {
        let keys = z_n_between(a, b, 10_000).unwrap();
        assert_eq!(keys.len(), 10_000);
        assert!(keys.iter().all(|k| valid(k)));
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
        assert!(
            a.is_none_or(|a| a < keys[0].as_str()) && b.is_none_or(|b| keys[9_999].as_str() < b)
        );
        rows.push(("z_n_between 10,000", longest(&keys), 12));
    }

    for (name, got, limit) in &rows {
        eprintln!("{name}: 가장 긴 키 {got}자 (기준 {limit}자 이하)");
    }
    assert!(rows.iter().all(|(_, got, limit)| got <= limit), "{rows:?}");
}

#[test]
fn 무작위_자리에_20000번_넣는다() {
    let mut keys: Vec<String> = Vec::new();
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..20_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let i = (seed % (keys.len() as u64 + 1)) as usize;
        let a = i.checked_sub(1).map(|j| keys[j].as_str());
        let b = keys.get(i).map(String::as_str);
        let k = between(a, b);
        keys.insert(i, k);
    }
    assert!(keys.windows(2).all(|w| w[0] < w[1]));
    eprintln!(
        "무작위 20,000: 가장 긴 키 {}자",
        keys.iter().map(String::len).max().unwrap()
    );
}

#[test]
fn 틀린_입력은_none() {
    let long = format!("a0{}", "1".repeat(1023));
    let smallest = format!("A{}", "0".repeat(26));
    for bad in [
        "",
        "a",
        "0a",
        "a00",
        "b1",
        "a-",
        "가",
        long.as_str(),
        smallest.as_str(),
    ] {
        assert_eq!(z_between(Some(bad), None), None, "{bad}");
        assert_eq!(z_between(None, Some(bad)), None, "{bad}");
        assert_eq!(z_n_between(Some(bad), None, 3), None, "{bad}");
    }
    assert_eq!(z_between(Some("a1"), Some("a0")), None);
    assert_eq!(z_between(Some("a1"), Some("a1")), None);
    assert_eq!(z_n_between(None, None, 0), Some(vec![]));
    // 가장 작은 정수 바로 위: 그 앞에 넣으면 유효한 키이거나 None이어야 한다.
    let floor1 = format!("A{}1", "0".repeat(25));
    assert!(valid(&floor1));
    for k in [z_between(None, Some(&floor1))].into_iter().chain(
        z_n_between(None, Some(&floor1), 3)
            .into_iter()
            .flatten()
            .map(Some),
    ) {
        assert!(
            k.as_deref().is_none_or(|k| valid(k) && k < floor1.as_str()),
            "{k:?}"
        );
    }
    let near = format!("a0{}", "1".repeat(1022)); // 1,024자: 바로 다음은 1,025자
    assert!(valid(&near));
    assert_eq!(
        z_between(Some(&near), Some(&format!("a0{}2", "1".repeat(1021)))),
        None
    );
}

#[test]
fn op에_명시한_z는_유효한_키여야_한다() {
    let mut d = Document::new(Kind::Canvas, "z").unwrap();
    for z in ["a00", "0a", "V", "a-b"] {
        let r = d.apply(
            &json!([{"op": "add", "node": {"id": "q", "type": "rect", "parent": "n1", "z": z}}]),
            Guard::default(),
        );
        assert!(
            matches!(r, Err(Error::Op { ref key, .. }) if key == "z"),
            "{z}"
        );
    }
    let a = d
        .apply(
            &json!([{"op": "add", "node": {"type": "rect", "parent": "n1"}}]),
            Guard::default(),
        )
        .unwrap();
    assert_eq!(a.ops[0]["node"]["z"], json!("a0"), "형제가 없으면 첫 키");
    assert_eq!(d.node("n1").unwrap().z, "a0");
}
