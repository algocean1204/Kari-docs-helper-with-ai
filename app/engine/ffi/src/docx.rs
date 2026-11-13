//! `doc.exportDocx`, `doc.exportTxt`. PDF 내보내기와 같은 길이다: 잠금 안에서 문서를 복사하고,
//! 만들기와 파일 쓰기는 잠금 밖에서 한다.

use kari_doc::serde_json::json;

use crate::fail::{Args, Res};
use crate::library::{engine, on};
use crate::pdf::{flow_doc, write_whole};

pub(crate) fn export_docx(a: &Args) -> Res {
    let (path, doc, assets) = flow_doc(a, "Word 문서")?;
    // 문서 속성의 날짜는 문서를 마지막으로 고친 때다(내보낸 시각을 넣지 않는다).
    let modified = {
        let id = a.id("id")?;
        let mut state = engine();
        let e = on(&mut state)?;
        e.lib
            .docs(false)?
            .into_iter()
            .find(|d| d.id == id)
            .map(|d| d.updated)
    };
    let read = |h: &str| kari_store::read_asset(&assets, h);
    let out = kari_docx::export(&doc, modified, &read);
    write_whole(&path, &out.bytes, "Word")?;
    Ok(json!({"bytes": out.bytes.len(), "lost": out.lost}))
}

pub(crate) fn export_txt(a: &Args) -> Res {
    let (path, doc, _) = flow_doc(a, "텍스트")?;
    let bytes = kari_docx::text(&doc).into_bytes();
    write_whole(&path, &bytes, "텍스트")?;
    Ok(json!({"bytes": bytes.len()}))
}
