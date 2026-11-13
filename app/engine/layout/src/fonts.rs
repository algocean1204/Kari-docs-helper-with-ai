//! 등록한 글꼴. 시스템 글꼴과 OS의 대체 글꼴을 쓰지 않는다.

use std::collections::HashMap;
use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{FontContext, FontData};

use crate::{Error, FontId};

/// 이름이 없을 때 쓰는 기본 글꼴.
pub(crate) const DEFAULT_FAMILY: &str = "Pretendard";

pub struct Fonts {
    pub(crate) cx: FontContext,
    /// 등록한 파일의 바이트. 번호가 `FontId.file`이다.
    files: Vec<Arc<Vec<u8>>>,
    /// (blob id, 파일 안 번호) → FontId
    ids: HashMap<(u64, u32), FontId>,
    /// 글꼴 이름 → 가진 굵기
    weights: HashMap<String, Vec<f32>>,
    /// 처음 등록한 글꼴 이름. Pretendard가 없을 때의 기본 글꼴.
    first: Option<String>,
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts::new()
    }
}

impl Fonts {
    /// 빈 글꼴 모음. 시스템 글꼴을 쓰지 않는다.
    pub fn new() -> Fonts {
        let collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        Fonts {
            cx: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            files: Vec::new(),
            ids: HashMap::new(),
            weights: HashMap::new(),
            first: None,
        }
    }

    /// TrueType/OpenType 파일 하나를 등록한다.
    pub fn add(&mut self, data: Vec<u8>) -> Result<(), Error> {
        let data = Arc::new(data);
        let blob = Blob::new(data.clone());
        let id = blob.id();
        let families = self.cx.collection.register_fonts(blob, None);
        if families.is_empty() {
            return Err(Error::Font("글꼴 파일을 읽지 못했습니다.".into()));
        }
        let file = self.files.len() as u32;
        self.files.push(data);
        for (family, infos) in families {
            let Some(name) = self.cx.collection.family_name(family).map(str::to_string) else {
                continue;
            };
            for info in infos {
                self.ids.insert(
                    (id, info.index()),
                    FontId {
                        file,
                        index: info.index(),
                    },
                );
                self.weights
                    .entry(name.clone())
                    .or_default()
                    .push(info.weight().value());
            }
            self.first.get_or_insert(name);
        }
        Ok(())
    }

    /// 등록한 글꼴 파일의 바이트(복사하지 않고 빌려 준다). `file`은 `FontId.file`.
    pub fn data(&self, file: u32) -> Option<&Arc<Vec<u8>>> {
        self.files.get(file as usize)
    }

    pub(crate) fn id(&self, font: &FontData) -> Option<FontId> {
        self.ids.get(&(font.data.id(), font.index)).copied()
    }

    /// 실제로 쓸 글꼴 이름. 없는 이름이면 기본 글꼴.
    pub(crate) fn family<'a>(&'a self, name: &'a str) -> &'a str {
        if self.weights.contains_key(name) {
            name
        } else if self.weights.contains_key(DEFAULT_FAMILY) {
            DEFAULT_FAMILY
        } else {
            self.first.as_deref().unwrap_or(DEFAULT_FAMILY)
        }
    }

    /// 가진 굵기 가운데 가까운 것. 같으면 굵은 쪽(500은 400, 600은 700).
    pub(crate) fn weight(&self, family: &str, want: f32) -> f32 {
        let Some(have) = self.weights.get(family) else {
            return want;
        };
        have.iter()
            .copied()
            .min_by(|a, b| {
                let (da, db) = ((a - want).abs(), (b - want).abs());
                da.total_cmp(&db).then(b.total_cmp(a))
            })
            .unwrap_or(want)
    }
}
