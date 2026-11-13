//! `doc.exportPdf`(흐름 문서는 쪽마다, 캔버스 문서는 대지마다 쪽 하나). 명령 잠금은 문서를 복사하는 동안만 잡고,
//! 배치와 PDF 만들기와 파일 쓰기는 잠금 밖에서 한다.

use std::cell::RefCell;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use kari_doc::serde_json::{json, Value};
use kari_doc::{Document, Kind};
use kari_layout::Layouter;
use kari_pdf::{export, export_canvas, Error, Meta};
use kari_render::{Images, IMAGE_BUDGET};

use crate::fail::{bad, fail, Args, Fail, Res};
use crate::library::{engine, on};
use crate::session;

pub(crate) fn export_pdf(a: &Args) -> Res {
    let (path, doc, assets) = doc_copy(a)?;
    let fonts =
        session::current_fonts().ok_or_else(|| bad("글꼴을 먼저 넘겨야 합니다(kari_font_add)."))?;
    let mut lay = Layouter::new(fonts);
    let read = |h: &str| kari_store::read_asset(&assets, h);
    let pdf_err = |e: Error| {
        let code = if e == Error::Stale {
            "stale"
        } else {
            "bad_request"
        };
        fail(code, e.to_string())
    };
    let (bytes, pages) = if doc.kind() == Kind::Canvas {
        // 그림의 크기는 화면과 같은 길(`Images`)로 읽어 놓임이 화면과 같게 한다.
        let dir = assets.clone();
        let mut images = Images::new(
            Box::new(move |h| kari_store::read_asset(&dir, h)),
            IMAGE_BUDGET,
        );
        let images = RefCell::new(&mut images);
        let scene = lay.scene(&doc, &|h| images.borrow_mut().size(h));
        let meta = Meta {
            title: doc.name(),
            lang: "ko",
        };
        let bytes = export_canvas(&doc, &scene, lay.fonts(), &meta, &read).map_err(pdf_err)?;
        (bytes, scene.boards.len())
    } else {
        lay.layout(&doc, None);
        let root = doc.children(None).into_iter().find(|n| n.kind == "flow");
        let lang = root
            .and_then(|r| doc.prop(&r.id, "lang"))
            .and_then(Value::as_str)
            .unwrap_or("ko");
        let meta = Meta {
            title: doc.name(),
            lang,
        };
        let bytes = export(&doc, lay.output(), lay.fonts(), &meta, &read).map_err(pdf_err)?;
        (bytes, lay.output().pages.len())
    };
    write_whole(&path, &bytes, "PDF")?;
    Ok(json!({"pages": pages, "bytes": bytes.len()}))
}

/// 인자의 경로(절대 경로)와 내보낼 흐름 문서의 사본, 에셋 폴더.
pub(crate) fn flow_doc(a: &Args, what: &str) -> Result<(PathBuf, Document, PathBuf), Fail> {
    let (path, doc, assets) = doc_copy(a)?;
    if doc.kind() != Kind::Flow {
        return Err(bad(format!("흐름 문서만 {what}로 내보냅니다.")));
    }
    Ok((path, doc, assets))
}

/// 인자의 경로(절대 경로)와 문서의 사본, 에셋 폴더. 명령 잠금은 복사하는 동안만 잡는다.
/// 편집 세션이 있으면 그 앞 사본, 메모리에 연 문서가 있으면 그것, 없으면 저장소에서 읽는다.
/// 캔버스 문서는 저장한 뒤 사본(뒤 사본)을 쓴다: 끌기 미리 보기나 조합 중인 글이 PDF에 들어가지 않는다.
fn doc_copy(a: &Args) -> Result<(PathBuf, Document, PathBuf), Fail> {
    let id = a.id("id")?;
    let path = PathBuf::from(a.str("path")?);
    if !path.is_absolute() {
        return Err(bad("path는 절대 경로입니다."));
    }
    let (doc, assets) = {
        let mut state = engine();
        let e = on(&mut state)?;
        let doc = match (e.docs.get(&id), session::get(id)) {
            (Some(back), Some(s)) => s.lock(back.doc()).doc.clone(),
            (Some(back), None) => back.doc().clone(),
            (None, _) => e.lib.open_doc(id)?.doc().clone(),
        };
        (doc, e.lib.assets_dir())
    };
    Ok((path, doc, assets))
}

/// 임시 이름으로 다 쓴 뒤 이름을 바꾼다. 어느 길로 가든 임시 파일과 쓰다 만 파일을 남기지 않는다.
/// `what`은 오류 메시지에 쓰는 형식 이름이다.
pub(crate) fn write_whole(path: &Path, bytes: &[u8], what: &str) -> Result<(), Fail> {
    write_via(path, bytes, &std::env::temp_dir(), what)
}

/// `spare`는 같은 폴더에 임시 파일을 만들지 못할 때 쓰는 폴더(앱의 임시 폴더).
fn write_via(path: &Path, bytes: &[u8], spare: &Path, what: &str) -> Result<(), Fail> {
    let name = path
        .file_name()
        .ok_or_else(|| bad("path에 파일 이름이 없습니다."))?;
    let tmp_name = format!(".{}.kari-tmp", name.to_string_lossy());
    let io = |e: std::io::Error| fail("io", format!("{what} 파일을 쓰지 못했습니다: {e}"));
    let near = path.with_file_name(&tmp_name);
    let tmp = match put(&near, bytes) {
        Ok(()) => near,
        // 샌드박스 앱은 저장 대화 상자에서 고른 파일만 쓸 수 있어 같은 폴더에 임시 파일을 만들지 못한다.
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            let far = spare.join(&tmp_name);
            put(&far, bytes).map_err(io)?;
            far
        }
        Err(e) => return Err(io(e)),
    };
    if fs::rename(&tmp, path).is_ok() {
        return Ok(());
    }
    let _ = fs::remove_file(&tmp);
    // 다른 볼륨(외장 디스크, 네트워크 폴더)이거나 샌드박스가 옮기기를 거절하면 고른 경로에 바로 쓴다.
    put(path, bytes).map_err(io)
}

/// `path`에 쓴다. 열었는데 쓰다 실패하면 그 파일을 지운다. 열지 못했으면 아무것도 지우지 않는다
/// (원래 있던 파일을 지우지 않게).
fn put(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;
    f.write_all(bytes).inspect_err(|_| {
        let _ = fs::remove_file(path);
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// 시험마다 빈 폴더 둘: 고를 폴더와 임시 폴더.
    fn dirs(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("kari-pdf-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (dest, spare) = (root.join("dest"), root.join("spare"));
        fs::create_dir_all(&dest).unwrap();
        fs::create_dir_all(&spare).unwrap();
        (dest, spare)
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn chmod(p: &Path, mode: u32) {
        fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn cleanup(dest: &Path) {
        chmod(dest, 0o755);
        let _ = fs::remove_dir_all(dest.parent().unwrap());
    }

    #[test]
    fn near_refused_goes_through_spare_and_renames() {
        let (dest, spare) = dirs("spare");
        // 같은 폴더의 임시 이름에 쓸 수 없는 파일을 두어 거절을 만든다(옮기기는 된다).
        let block = dest.join(".a.pdf.kari-tmp");
        fs::write(&block, b"").unwrap();
        chmod(&block, 0o444);
        write_via(&dest.join("a.pdf"), b"%PDF", &spare, "PDF")
            .ok()
            .unwrap();
        assert_eq!(fs::read(dest.join("a.pdf")).unwrap(), b"%PDF");
        assert_eq!(
            files(&dest),
            [".a.pdf.kari-tmp", "a.pdf"],
            "막은 파일만 남는다"
        );
        assert!(files(&spare).is_empty());
        cleanup(&dest);
    }

    #[test]
    fn failed_rename_writes_the_chosen_file_directly() {
        // 고른 파일만 쓸 수 있는 폴더(샌드박스와 같은 모양): 임시 파일도, 옮기기도 거절된다.
        let (dest, spare) = dirs("direct");
        fs::write(dest.join("a.pdf"), b"old").unwrap();
        chmod(&dest, 0o555);
        write_via(&dest.join("a.pdf"), b"%PDF new", &spare, "PDF")
            .ok()
            .unwrap();
        assert_eq!(fs::read(dest.join("a.pdf")).unwrap(), b"%PDF new");
        assert_eq!(files(&dest), ["a.pdf"]);
        assert!(files(&spare).is_empty());
        cleanup(&dest);
    }

    #[test]
    fn everything_fails_and_nothing_is_left() {
        let (dest, spare) = dirs("fail");
        chmod(&dest, 0o555);
        let e = write_via(&dest.join("a.pdf"), b"%PDF", &spare, "PDF").unwrap_err();
        assert_eq!(e.code, "io");
        assert!(files(&dest).is_empty());
        assert!(files(&spare).is_empty());
        cleanup(&dest);
    }
}
