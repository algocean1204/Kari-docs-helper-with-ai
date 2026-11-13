//! 앱과 엔진의 경계.
//!
//! `kari_call`, `kari_free`가 JSON 요청을 받아 JSON 응답을 돌려준다. 화면 스레드는 명령 잠금을 잡지 않는
//! C 함수(`kari_font_add`, `kari_edit_gen`, `kari_edit_render`, `kari_edit_render_at`)을 바로 부른다.
//! `ai.*`와 `iv.*`는 네트워크를 기다리므로, `doc.exportPdf`는 배치와 파일 쓰기가 길므로 명령 잠금을 스스로 짧게 잡는다.

mod ai;
mod cmd;
mod dashboard;
mod docx;
mod edit;
mod fail;
mod iv;
mod library;
mod pdf;
mod session;
mod state;
mod styles;
mod task;

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use kari_doc::serde_json::{self, json, Value};
use kari_render::Frame;

use fail::{bad, fail, Args, Res};

fn handle(request: Option<&str>) -> Res {
    let request = request.ok_or_else(|| bad("요청이 UTF-8 문자열이 아닙니다."))?;
    let req: Value =
        serde_json::from_str(request).map_err(|e| bad(format!("요청이 JSON이 아닙니다: {e}")))?;
    let m = req
        .get("m")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("요청에 이름(m)이 없습니다."))?;
    let a = req.get("a").unwrap_or(&Value::Null);
    if !a.is_null() && !a.is_object() {
        return Err(bad("인자(a)는 객체입니다."));
    }
    if matches!(m, "ai.tasks" | "ai.setTask" | "ai.resetTasks") {
        return task::dispatch(m, &Args(a));
    }
    if m.starts_with("ai.") {
        return ai::dispatch(m, &Args(a));
    }
    if m.starts_with("iv.") {
        return iv::dispatch(m, &Args(a));
    }
    if m == "doc.exportPdf" {
        return pdf::export_pdf(&Args(a));
    }
    if m == "doc.exportDocx" {
        return docx::export_docx(&Args(a));
    }
    if m == "doc.exportTxt" {
        return docx::export_txt(&Args(a));
    }
    let mut state = library::engine();
    if m.starts_with("edit.") {
        edit::dispatch(&mut state, m, &Args(a))
    } else {
        library::dispatch(&mut state, m, &Args(a))
    }
}

/// 패닉을 잡아 오류 응답으로 바꾸고, 결과를 응답 JSON으로 만든다.
fn guarded(f: impl FnOnce() -> Res) -> String {
    let res = catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(fail("panic", "엔진 안에서 처리하지 못한 오류가 났습니다.")));
    match res {
        Ok(v) => json!({"ok": v}),
        Err(f) => json!({"err": {"code": f.code, "message": f.message, "detail": f.detail}}),
    }
    .to_string()
}

/// 요청 JSON(`{"m", "a"}`)을 받아 응답 JSON(`{"ok"}` 또는 `{"err"}`)을 돌려준다. 어느 스레드에서 불러도 된다.
///
/// # Safety
/// `request`는 NUL로 끝나는 문자열을 가리키거나 null이어야 한다.
/// 돌려준 문자열은 `kari_free`로 한 번만 푼다.
#[no_mangle]
pub unsafe extern "C" fn kari_call(request: *const c_char) -> *mut c_char {
    let text = if request.is_null() {
        None
    } else {
        CStr::from_ptr(request).to_str().ok()
    };
    // JSON 문자열에는 NUL이 들어가지 않는다(serde_json이 \u0000으로 적는다).
    CString::new(guarded(|| handle(text)))
        .unwrap_or_else(|_| c"{\"err\":{\"code\":\"panic\"}}".into())
        .into_raw()
}

/// `kari_call`이 돌려준 문자열을 푼다. null이면 아무것도 하지 않는다.
///
/// # Safety
/// `response`는 `kari_call`이 돌려준 것이고 아직 풀지 않은 것이어야 한다.
#[no_mangle]
pub unsafe extern "C" fn kari_free(response: *mut c_char) {
    if !response.is_null() {
        drop(CString::from_raw(response));
    }
}

/// 글꼴 파일(TrueType/OpenType) 하나를 넘긴다. 편집 세션은 열 때까지 넘긴 글꼴로 배치하고 그린다.
/// 0 성공, 1 읽을 수 없는 글꼴이나 틀린 인자, 3 엔진 안의 오류. 어느 스레드에서 불러도 된다.
///
/// # Safety
/// `data`는 `len` 바이트를 읽을 수 있어야 한다(`len`이 0이면 null이어도 된다).
#[no_mangle]
pub unsafe extern "C" fn kari_font_add(data: *const u8, len: usize) -> i32 {
    if data.is_null() || len == 0 {
        return 1;
    }
    let bytes = std::slice::from_raw_parts(data, len);
    catch_unwind(|| if session::font_add(bytes) { 0 } else { 1 }).unwrap_or(3)
}

/// 문서의 바뀜 번호. 그릴 것이 달라질 때마다 오른다. 세션이 없으면 0. 명령 잠금을 잡지 않는다.
#[no_mangle]
pub extern "C" fn kari_edit_gen(doc: i64) -> u64 {
    catch_unwind(|| session::gen(doc)).unwrap_or(0)
}

/// 문서를 화소 버퍼(BGRA, 미리 곱한 알파)에 그린다. `width`, `height`, `stride`는 화소와 바이트,
/// `scale`은 논리 화소당 화소, `scroll_y`는 논리 화소. 명령 잠금을 잡지 않는다(앞 사본 잠금만 짧게).
/// 0 성공, 1 틀린 인자, 2 세션 없음, 3 엔진 안의 오류.
///
/// # Safety
/// `pixels`는 `stride * height` 바이트를 쓸 수 있어야 하고, 그리는 동안 다른 곳에서 쓰지 않아야 한다.
#[no_mangle]
pub unsafe extern "C" fn kari_edit_render(
    doc: i64,
    pixels: *mut u8,
    width: i32,
    height: i32,
    stride: i32,
    scale: f32,
    scroll_y: f32,
) -> i32 {
    kari_edit_render_at(doc, pixels, width, height, stride, scale, 0.0, scroll_y)
}

/// `kari_edit_render`에 가로 스크롤(`scroll_x`, 논리 화소)을 더한 것. 배율을 키워 쪽이 영역보다 넓을 때 쓴다.
///
/// # Safety
/// `kari_edit_render`와 같다.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn kari_edit_render_at(
    doc: i64,
    pixels: *mut u8,
    width: i32,
    height: i32,
    stride: i32,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
) -> i32 {
    let (Ok(w), Ok(h), Ok(st)) = (
        usize::try_from(width),
        usize::try_from(height),
        usize::try_from(stride),
    ) else {
        return 1;
    };
    let len = st.checked_mul(h);
    if pixels.is_null() || w == 0 || h == 0 || w.checked_mul(4).is_none_or(|row| st < row) {
        return 1;
    }
    let Some(len) = len.filter(|l| *l <= isize::MAX as usize) else {
        return 1;
    };
    let buf = std::slice::from_raw_parts_mut(pixels, len);
    let frame = Frame {
        pixels: buf,
        width,
        height,
        stride: st,
        scale,
        scroll_x,
        scroll_y,
    };
    catch_unwind(AssertUnwindSafe(|| session::render(doc, frame))).unwrap_or(3)
}

/// 측정용: 마지막 `edit.*` 호출이 앞 사본 잠금을 잡은 시간의 합(ns).
#[doc(hidden)]
pub fn edit_held_ns(doc: i64) -> u64 {
    session::held_ns(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 잠근 채 패닉해도 경계를 넘지 않고, 다음 호출이 정상으로 돈다.
    #[test]
    fn panic_stays_inside() {
        let out: Value = serde_json::from_str(&guarded(|| {
            let _locked = library::engine();
            panic!("시험용 패닉");
        }))
        .unwrap();
        assert_eq!(out["err"]["code"], "panic");
        let next: Value = serde_json::from_str(&guarded(|| {
            handle(Some(r#"{"m":"settings.get","a":{"key":"k"}}"#))
        }))
        .unwrap();
        assert_eq!(next["err"]["code"], "not_open");
    }
}
