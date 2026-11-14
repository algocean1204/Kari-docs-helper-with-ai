//! 넣는 그림의 거절: 치수만 큰 그림, 가운데서 잘린 그림.

use kari_render::{prepare_image, MAX_PIXELS};
use skia_safe::{surfaces, Color, EncodedImageFormat};

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in data {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
    out
}

/// 머리의 치수만 `w`×`h`이고 몸은 거의 없는 PNG.
fn header_only(w: u32, h: u32) -> Vec<u8> {
    let mut ihdr = [w.to_be_bytes(), h.to_be_bytes()].concat();
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(
        b"IDAT",
        &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// 잘 줄지 않는 `w`×`h` 잡음 PNG.
fn noise(w: i32, h: i32) -> Vec<u8> {
    let mut s = surfaces::raster_n32_premul((w, h)).unwrap();
    s.canvas().clear(Color::BLACK);
    let mut seed = 12345u32;
    let mut paint = skia_safe::Paint::default();
    for y in 0..h {
        for x in 0..w {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            paint.set_color(Color::from_rgb(
                (seed >> 8) as u8,
                (seed >> 16) as u8,
                (seed >> 24) as u8,
            ));
            s.canvas().draw_rect(
                skia_safe::Rect::from_xywh(x as f32, y as f32, 1.0, 1.0),
                &paint,
            );
        }
    }
    let img = s.image_snapshot();
    img.peek_pixels()
        .unwrap()
        .encode(EncodedImageFormat::PNG, None)
        .unwrap()
}

#[test]
fn 치수만_큰_그림은_풀기_전에_거절한다() {
    let side = (MAX_PIXELS as f64).sqrt() as u32 + 1;
    let e = prepare_image(&header_only(side, side)).unwrap_err();
    assert!(e.contains("너무 큽니다"), "{e}");
    let e = prepare_image(&header_only(16000, 16000)).unwrap_err();
    assert!(e.contains("너무 큽니다"), "{e}");
}

#[test]
fn 가운데서_잘린_그림은_거절한다() {
    let full = noise(200, 200);
    assert!(prepare_image(&full).is_ok());
    let cut = &full[..full.len() * 6 / 10];
    let e = prepare_image(cut).unwrap_err();
    assert!(e.contains("끝까지"), "{e}");
}
