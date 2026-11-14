//! 캔버스 명령: 이름과 인자로 부르는 편집(만들기, 지우기, 복제, 방향키 옮기기, 고르기, 속성 바꾸기, 뒤집기, 정렬과 분배,
//! 붙여넣기, 글 상자 편집 시작과 끝).
//! 결과는 op 묶음(`CPlan`)이라 저장, 되돌리기, 버전, 충돌 규칙을 그대로 탄다.

use kari_doc::serde_json::{Map, Value};
use kari_doc::Document;
use kari_layout::Layouter;

use super::arrange::{self, Align};
use super::clip::CanvasClip;
use super::make::{self, Tool, DUP_OFFSET};
use super::props;
use super::text::TextCmd;
use super::{apply, boards, move_ops, CanvasEditor};

/// 명령 하나의 결과. `ops`가 있으면 부르는 쪽이 앞 사본에 적용하고 뒤 사본에 커밋한다.
#[derive(Clone, Debug, Default)]
pub struct CPlan {
    /// 비면 문서는 그대로다(선택만 바뀌었을 수 있다).
    pub ops: Vec<Value>,
    /// 기록에 보일 이름.
    pub label: &'static str,
    /// 이어 치는 글의 묶기 번호. 같은 번호의 연달은 커밋은 되돌리기 한 번이다.
    pub group: Option<u64>,
    /// 거절했으면 그 이유. 이때 ops는 비고 문서는 그대로다.
    pub refused: Option<&'static str>,
    /// 조합 중인 글만 확정했다. 이것을 적용하고 배치한 뒤 같은 명령을 다시 준다.
    pub again: bool,
    /// 미리 보기다: 앞 사본에만 적용하고 커밋하지 않는다(수치 끌기).
    pub preview: bool,
}

impl CPlan {
    fn refuse(why: &'static str) -> CPlan {
        CPlan {
            refused: Some(why),
            ..CPlan::default()
        }
    }

    fn ops(ops: Vec<Value>, label: &'static str) -> CPlan {
        CPlan {
            ops,
            label,
            ..CPlan::default()
        }
    }
}

/// 캔버스 명령. 좌표는 판 좌표다.
#[derive(Clone, Debug)]
pub enum CanvasCmd {
    /// 만들기 도구를 고른다(`None`이면 고르기). 다음 포인터 누름이 그 도구로 만든다.
    Tool(Option<Tool>),
    /// 노드 하나를 만든다. `to`가 없으면 한 번 누른 것이라 기본 크기로 놓는다.
    Create {
        tool: Tool,
        from: (f64, f64),
        to: Option<(f64, f64)>,
        shift: bool,
    },
    /// 고른 것을 지운다.
    Delete,
    /// 고른 것을 복제한다. `offset`이 없으면 부모 좌표로 (10, 10) 비켜 놓는다.
    Duplicate { offset: Option<(f64, f64)> },
    /// 고른 것을 판의 가로세로로 옮긴다(방향키: 1, ⇧는 10).
    Nudge { dx: f64, dy: f64 },
    /// 같은 부모의 모든 노드를 고른다(고른 것이 없으면 모든 대지). 글 상자를 편집 중이면 글 전체를 고른다.
    SelectAll,
    /// 선택을 정한다.
    Select(Vec<String>),
    /// 고른 노드마다 속성을 바꾼다. `preview`이면 앞 사본에만 적용한다.
    Set {
        props: Map<String, Value>,
        preview: bool,
    },
    /// 고른 것을 뒤집는다(가로: 왼쪽과 오른쪽이 바뀐다). 여럿이면 묶음의 가운데가 기준이다.
    Flip { vertical: bool },
    /// 고른 것을 맞춘다. 하나면 부모에, 여럿이면 고른 것들의 경계에.
    Align(Align),
    /// 고른 것(셋 이상) 사이의 간격을 같게 한다.
    Distribute { vertical: bool },
    /// 복사해 둔 조각을 붙인다. `same_doc`은 조각이 이 문서에서 온 것인지, `center`는 보이는 영역의 가운데(판 좌표)다.
    Paste {
        clip: CanvasClip,
        same_doc: bool,
        in_place: bool,
        center: (f64, f64),
    },
    /// 고른 글 상자 하나의 편집을 시작한다(글 전체를 고른다).
    EditText,
    /// 글 상자 편집을 끝낸다. 글이 비었으면 그 상자를 지운다.
    EndText,
}

impl CanvasEditor {
    /// 명령 하나를 계산한다. 선택은 바로 바꾸고 문서는 읽기만 한다(적용이 실패하면 부르는 쪽이 편집기를 되돌린다).
    pub fn command(
        &mut self,
        doc: &Document,
        lay: &mut Layouter,
        cmd: &CanvasCmd,
        now: u64,
    ) -> CPlan {
        // 글 상자를 편집하는 동안에는 글 상자 밖의 편집(지우기, 복제, 옮기기, 만들기)을 받지 않는다.
        let busy = self.text.is_some()
            && matches!(
                cmd,
                CanvasCmd::Delete
                    | CanvasCmd::Duplicate { .. }
                    | CanvasCmd::Nudge { .. }
                    | CanvasCmd::Create { .. }
                    | CanvasCmd::Flip { .. }
                    | CanvasCmd::Align(_)
                    | CanvasCmd::Distribute { .. }
                    | CanvasCmd::Paste { .. }
            );
        if busy {
            return CPlan::refuse("글 상자를 편집하는 중에는 쓸 수 없습니다.");
        }
        match cmd {
            CanvasCmd::Tool(tool) => {
                let ops = self.end_text(doc);
                self.tool = tool.clone();
                CPlan::ops(ops, "빈 글 상자 지우기")
            }
            CanvasCmd::Create {
                tool,
                from,
                to,
                shift,
            } => {
                let parent = match make::parent_for(doc, lay, tool, *from) {
                    Ok(p) => p,
                    Err(e) => return CPlan::refuse(e),
                };
                let id = make::fresh_ids(doc, 1).remove(0);
                match make::create(doc, lay, tool, parent.as_deref(), &id, (*from, *to), *shift) {
                    Ok(ops) => {
                        self.created(doc, &id, tool);
                        CPlan::ops(ops, tool.label())
                    }
                    Err(e) => CPlan::refuse(e),
                }
            }
            CanvasCmd::Delete => match make::delete(doc, &self.sel) {
                Ok(ops) => {
                    self.sel.clear();
                    CPlan::ops(ops, "지우기")
                }
                Err(e) => CPlan::refuse(e),
            },
            CanvasCmd::Duplicate { offset } => {
                let off = offset.unwrap_or((DUP_OFFSET, DUP_OFFSET));
                match make::duplicate(doc, &self.sel, off) {
                    Ok((ops, roots)) => {
                        if !roots.is_empty() {
                            self.sel = roots;
                        }
                        CPlan::ops(ops, "복제")
                    }
                    Err(e) => CPlan::refuse(e),
                }
            }
            CanvasCmd::Nudge { dx, dy } => {
                let roots = make::roots(doc, &self.sel);
                let mut starts: Vec<_> = self
                    .starts(doc, lay)
                    .into_iter()
                    .filter(|s| roots.contains(&s.geo.id))
                    .collect();
                CPlan::ops(move_ops(&mut starts, (*dx, *dy), false), "옮기기")
            }
            CanvasCmd::SelectAll => {
                if self.text.is_some() {
                    return self.text(doc, lay, &TextCmd::SelectAll, now);
                }
                self.select_all(doc);
                CPlan::default()
            }
            CanvasCmd::Select(ids) => {
                // 글 상자 편집 중이었으면 끝낸다(글이 빈 상자는 지워지므로 고를 수 없다).
                let ops = self.end_text(doc);
                let gone: Vec<&str> = ops.iter().filter_map(|o| o["id"].as_str()).collect();
                let ids = ids
                    .iter()
                    .filter(|i| !gone.contains(&i.as_str()))
                    .cloned()
                    .collect();
                self.select(doc, ids);
                CPlan::ops(ops, "빈 글 상자 지우기")
            }
            CanvasCmd::Set { props, preview } => match props::set_ops(doc, lay, &self.sel, props) {
                Ok(ops) => {
                    let name_only = props.len() == 1 && props.contains_key("name");
                    let mut p = CPlan::ops(
                        ops,
                        if name_only {
                            "이름 바꾸기"
                        } else {
                            "속성 바꾸기"
                        },
                    );
                    p.preview = *preview;
                    p
                }
                Err(e) => CPlan::refuse(e),
            },
            CanvasCmd::Flip { vertical } => {
                let Some(f) = self.frame(doc, lay) else {
                    return CPlan::refuse("고른 것이 없습니다.");
                };
                let r = f.rect;
                let pivot = apply(&f.world, (r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
                let mut starts = self.starts(doc, lay);
                match arrange::flip_ops(&mut starts, pivot, *vertical) {
                    Ok(ops) => CPlan::ops(ops, "뒤집기"),
                    Err(e) => CPlan::refuse(e),
                }
            }
            CanvasCmd::Align(to) => {
                let mut starts = self.starts(doc, lay);
                match arrange::align_ops(doc, lay, &mut starts, *to) {
                    Ok(ops) => CPlan::ops(ops, "정렬"),
                    Err(e) => CPlan::refuse(e),
                }
            }
            CanvasCmd::Distribute { vertical } => {
                let mut starts = self.starts(doc, lay);
                match arrange::distribute_ops(&mut starts, *vertical) {
                    Ok(ops) => CPlan::ops(ops, "간격 고르게"),
                    Err(e) => CPlan::refuse(e),
                }
            }
            CanvasCmd::Paste {
                clip,
                same_doc,
                in_place,
                center,
            } => match self.paste(doc, lay, clip, *same_doc, *in_place, *center) {
                Ok(ops) => CPlan::ops(ops, "붙여넣기"),
                Err(e) => CPlan::refuse(e),
            },
            CanvasCmd::EditText => match self.sel.as_slice() {
                [id] if doc.node(id).is_some_and(|n| n.kind == "text") => {
                    let id = id.clone();
                    // 다른 글 상자를 편집하던 중이었으면 먼저 끝낸다.
                    let ops = if self.editing() == Some(id.as_str()) {
                        Vec::new()
                    } else {
                        self.end_text(doc)
                    };
                    self.begin_text(doc, &id, None);
                    CPlan::ops(ops, "빈 글 상자 지우기")
                }
                _ => CPlan::refuse("글 상자 하나를 골라야 합니다."),
            },
            CanvasCmd::EndText => CPlan::ops(self.end_text(doc), "빈 글 상자 지우기"),
        }
    }

    /// 새로 만든 노드를 고르고 도구를 고르기로 돌린다. 글 상자는 바로 편집을 시작한다.
    pub(crate) fn created(&mut self, doc: &Document, id: &str, tool: &Tool) {
        self.tool = None;
        self.sel = vec![id.to_string()];
        if matches!(tool, Tool::Text) {
            self.begin_text(doc, id, Some((0, false)));
        }
    }

    /// 같은 부모의 보이고 잠기지 않은 노드를 모두 고른다. 고른 것이 없으면 모든 대지.
    fn select_all(&mut self, doc: &Document) {
        let parent = self
            .sel
            .first()
            .and_then(|i| doc.node(i))
            .map(|n| n.parent.clone());
        let ids: Vec<String> = match parent {
            None => boards(doc).into_iter().map(|b| b.id).collect(),
            Some(p) => doc
                .children(p.as_deref())
                .into_iter()
                .filter(|n| {
                    doc.prop(&n.id, "visible").and_then(Value::as_bool) != Some(false)
                        && doc.prop(&n.id, "locked").and_then(Value::as_bool) != Some(true)
                })
                .map(|n| n.id.clone())
                .collect(),
        };
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|i| doc.prop(i, "locked").and_then(Value::as_bool) != Some(true))
            .collect();
        self.select(doc, ids);
    }
}
