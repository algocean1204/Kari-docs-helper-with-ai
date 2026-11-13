//! 그림: 넣을 때의 검사와 정리, 그릴 때 푼 그림의 기억.

use std::collections::HashMap;

use skia_safe::{
    surfaces, Codec, CubicResampler, Data, EncodedImageFormat, EncodedOrigin, Image, ImageInfo,
    Paint, Rect, SamplingOptions,
};

/// 넣는 그림의 긴 변 상한(px). A4 폭 300dpi가 2,480px다.
pub const MAX_SIDE: u32 = 4000;
/// 넣는 파일의 크기 상한(바이트).
pub const MAX_BYTES: usize = 20 << 20;
/// 푼 그림을 기억해 두는 합계 상한(바이트).
pub const IMAGE_BUDGET: usize = 64 << 20;
/// 다시 담는 JPEG의 품질.
const JPEG_QUALITY: u32 = 90;

/// 넣을 수 있게 정리한 그림. `bytes`는 저장할 바이트(원래 것이거나 돌리고 줄여 다시 담은 것).
#[derive(Debug)]
pub struct Prepared {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 원래 바이트와 다르다(방향을 돌렸거나 줄였다).
    pub changed: bool,
}

fn is_png(d: &[u8]) -> bool {
    d.starts_with(b"\x89PNG\r\n\x1a\n")
}

fn is_jpeg(d: &[u8]) -> bool {
    d.starts_with(&[0xFF, 0xD8])
}

/// 넣을 그림을 검사하고 정리한다(엔진이 검사한다. 화면만 믿지 않는다).
/// PNG나 JPEG로 실제로 풀려야 하고 1px 이상이어야 한다. JPEG의 방향 정보가 "똑바로"가 아니면 똑바로 돌려,
/// 긴 변이 4,000px를 넘으면 4,000px로 줄여 같은 형식으로 다시 담는다. 그 밖에는 원래 바이트를 그대로 둔다.
pub fn prepare_image(bytes: &[u8]) -> Result<Prepared, &'static str> {
    if bytes.len() > MAX_BYTES {
        return Err("20MB가 넘는 그림은 넣을 수 없습니다.");
    }
    let (png, jpeg) = (is_png(bytes), is_jpeg(bytes));
    if !png && !jpeg {
        return Err("PNG나 JPEG 그림만 넣을 수 있습니다.");
    }
    let mut codec = Codec::from_data(Data::new_copy(bytes)).ok_or("그림을 읽지 못했습니다.")?;
    let origin = codec.origin();
    // 방향 정보대로 돌린 그림이 나온다. 90° 돌림이면 받는 크기의 가로·세로를 바꿔 준다.
    let mut info = codec.info();
    if origin.swaps_width_height() {
        let d = info.dimensions();
        info = info.with_dimensions((d.height, d.width));
    }
    let image = codec
        .get_image(info, None)
        .map_err(|_| "그림을 읽지 못했습니다.")?;
    let (w, h) = (image.width().max(0) as u32, image.height().max(0) as u32);
    if w == 0 || h == 0 {
        return Err("그림의 가로·세로가 1px 이상이어야 합니다.");
    }
    let long = w.max(h);
    let rotate = jpeg && origin != EncodedOrigin::TopLeft;
    if !rotate && long <= MAX_SIDE {
        return Ok(Prepared {
            bytes: bytes.to_vec(),
            width: w,
            height: h,
            changed: false,
        });
    }
    let k = (MAX_SIDE as f64 / long as f64).min(1.0);
    let (nw, nh) = (
        ((w as f64 * k).round() as i32).max(1),
        ((h as f64 * k).round() as i32).max(1),
    );
    let mut surface =
        surfaces::raster_n32_premul((nw, nh)).ok_or("그림을 다시 담지 못했습니다.")?;
    surface.canvas().draw_image_rect_with_sampling_options(
        &image,
        None,
        Rect::from_iwh(nw, nh),
        SamplingOptions::from(CubicResampler::mitchell()),
        &Paint::default(),
    );
    let out = surface.image_snapshot();
    let pixmap = out.peek_pixels().ok_or("그림을 다시 담지 못했습니다.")?;
    let (format, quality) = if jpeg {
        (EncodedImageFormat::JPEG, Some(JPEG_QUALITY))
    } else {
        (EncodedImageFormat::PNG, None)
    };
    let encoded = pixmap
        .encode(format, quality)
        .ok_or("그림을 다시 담지 못했습니다.")?;
    Ok(Prepared {
        bytes: encoded,
        width: nw as u32,
        height: nh as u32,
        changed: true,
    })
}

/// 에셋 해시 → 바이트. 읽지 못하면 `None`.
pub type Loader = Box<dyn Fn(&str) -> Option<Vec<u8>> + Send>;

struct Entry {
    /// 풀지 못했으면 `None`(다시 풀지 않는다).
    image: Option<Image>,
    bytes: usize,
    used: u64,
}

/// 푼 그림의 기억. 에셋 해시로 찾고, 합계가 상한을 넘으면 오래 안 쓴 것부터 버린다.
pub struct Images {
    load: Loader,
    map: HashMap<String, Entry>,
    total: usize,
    tick: u64,
    budget: usize,
}

impl Images {
    pub fn new(load: Loader, budget: usize) -> Images {
        Images {
            load,
            map: HashMap::new(),
            total: 0,
            tick: 0,
            budget,
        }
    }

    /// 기억한 푼 그림의 합계(바이트).
    pub fn bytes(&self) -> usize {
        self.total
    }

    /// 푼 그림. 처음이면 읽어 풀고 기억한다. 읽거나 풀지 못하면 `None`.
    pub fn get(&mut self, asset: &str) -> Option<Image> {
        let load = &self.load;
        let make = || {
            load(asset)
                .and_then(|b| Image::from_encoded(Data::new_copy(&b)))
                .and_then(|i| i.make_raster_image(None, None))
        };
        let image = match self.map.get(asset) {
            Some(_) => None,
            None => Some(make()),
        };
        self.remember(asset.to_string(), image)
    }

    /// 그림의 화소 크기 (가로, 세로). 읽거나 풀지 못하면 `None`.
    pub fn size(&mut self, asset: &str) -> Option<(u32, u32)> {
        self.get(asset)
            .map(|i| (i.width() as u32, i.height() as u32))
    }

    /// 화면 화소 크기(`w`×`h`)로 미리 줄이거나 늘린 그림. 그릴 때마다 보간하지 않고 그대로 옮겨 그리게 한다
    /// (그림 10장을 매 프레임 보간하면 그리기가 두 배 넘게 느려졌다). 크기가 바뀌면(화면 배율) 새로 만든다.
    pub fn sized(&mut self, asset: &str, w: i32, h: i32) -> Option<Image> {
        let key = format!("{asset}@{w}x{h}");
        if self.map.contains_key(&key) {
            return self.remember(key, None);
        }
        let src = self.get(asset)?;
        let mut surface = surfaces::raster_n32_premul((w.max(1), h.max(1)))?;
        surface.canvas().draw_image_rect_with_sampling_options(
            &src,
            None,
            Rect::from_iwh(w, h),
            SamplingOptions::from(CubicResampler::mitchell()),
            &Paint::default(),
        );
        self.remember(key, Some(Some(surface.image_snapshot())))
    }

    /// `fresh`가 있으면 새로 기억하고, 없으면 기억한 것을 쓴다. 합계가 상한을 넘으면 오래 안 쓴 것부터 버린다.
    fn remember(&mut self, key: String, fresh: Option<Option<Image>>) -> Option<Image> {
        self.tick += 1;
        let Some(image) = fresh else {
            let e = self.map.get_mut(&key)?;
            e.used = self.tick;
            return e.image.clone();
        };
        let bytes = image.as_ref().map_or(0, |i| {
            ImageInfo::new_n32_premul(i.dimensions(), None).compute_min_byte_size()
        });
        while self.total + bytes > self.budget {
            let Some(old) = self
                .map
                .iter()
                .filter(|(_, e)| e.bytes > 0)
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(e) = self.map.remove(&old) {
                self.total -= e.bytes;
            }
        }
        if self.total + bytes > self.budget {
            // 혼자서도 상한을 넘는다(넣을 때 4,000px로 줄이므로 원본은 생기지 않는다). 기억하지 않고 그린다.
            return image;
        }
        self.total += bytes;
        self.map.insert(
            key,
            Entry {
                image: image.clone(),
                bytes,
                used: self.tick,
            },
        );
        image
    }
}
