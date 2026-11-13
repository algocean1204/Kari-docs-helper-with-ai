//! 꾸러미(zip) 쓰기. 파일마다 deflate로 줄이고, 줄지 않으면(그림) 그대로 담는다.
//! 시각은 1980-01-01 00:00으로 고정한다(같은 입력이면 같은 바이트).

// 한계: zip64를 쓰지 않아 꾸러미와 파일 하나가 4GB를 넘지 못한다, 그림 하나가 20MB까지라 닿지 않는다

/// DOS 날짜 1980-01-01.
const DATE: u16 = 0x0021;

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// (이름, 내용)을 차례대로 담은 zip 바이트.
pub(crate) fn zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let crc = crc32fast::hash(data);
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let (method, body): (u16, &[u8]) = if packed.len() < data.len() {
            (8, &packed)
        } else {
            (0, data)
        };
        let offset = out.len() as u32;
        let head = |out: &mut Vec<u8>| {
            u16le(out, 20); // 풀 때 필요한 판(2.0)
            u16le(out, 0); // 표시
            u16le(out, method);
            u16le(out, 0); // 시각
            u16le(out, DATE);
            u32le(out, crc);
            u32le(out, body.len() as u32);
            u32le(out, data.len() as u32);
            u16le(out, name.len() as u16);
            u16le(out, 0); // 덧붙임
        };
        u32le(&mut out, 0x0403_4b50);
        head(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(body);

        u32le(&mut central, 0x0201_4b50);
        u16le(&mut central, 20); // 만든 판
        head(&mut central);
        u16le(&mut central, 0); // 설명
        u16le(&mut central, 0); // 디스크
        u16le(&mut central, 0); // 안쪽 속성
        u32le(&mut central, 0); // 바깥 속성
        u32le(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let at = out.len() as u32;
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, files.len() as u16);
    u16le(&mut out, files.len() as u16);
    u32le(&mut out, central.len() as u32);
    u32le(&mut out, at);
    u16le(&mut out, 0);
    out
}
