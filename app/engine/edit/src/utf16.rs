//! UTF-8 바이트와 UTF-16 위치 바꾸기. Flutter 입력 채널과 오가는 곳은 여기 하나다.

/// UTF-8 바이트 위치 → UTF-16 위치. 글자 경계가 아니면 앞 경계로, 길이를 넘으면 끝.
pub fn to_utf16(text: &str, byte: usize) -> usize {
    let mut b = byte.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    text[..b].encode_utf16().count()
}

/// UTF-16 위치 → UTF-8 바이트 위치. 대리 쌍의 가운데면 앞 경계로, 길이를 넘으면 끝.
pub fn from_utf16(text: &str, unit: usize) -> usize {
    let mut u = 0;
    for (i, c) in text.char_indices() {
        u += c.len_utf16();
        if u > unit {
            return i;
        }
    }
    text.len()
}
