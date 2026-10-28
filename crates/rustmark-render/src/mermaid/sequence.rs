//! `sequenceDiagram` 时序图的解析与布局。

use crate::render::{Pt, geom::Rect};

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{collect_clicks, finalize, first_md_link, rect_corners};
use super::*;

/// 消息箭头形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SeqHead {
    /// `->>` / `-->`：实心箭头。
    Solid,
    /// `-->>`：虚线箭头。
    Dotted,
    /// `--x`：叉头。
    Cross,
}

/// 一条消息。
#[derive(Clone, Debug)]
pub(super) struct SeqMsg {
    from: String,
    to: String,
    label: String,
    head: SeqHead,
}

/// 备注框类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum NoteKind {
    Over,
    LeftOf,
    RightOf,
}

/// 一条备注。
#[derive(Clone, Debug)]
pub(super) struct SeqNote {
    kind: NoteKind,
    a: String,
    b: Option<String>,
    text: String,
}

/// 时序图事件（保持源码顺序，逐行占据一个纵向层级）。
#[derive(Clone, Debug)]
pub(super) enum SeqEvent {
    Msg(SeqMsg),
    Note(SeqNote),
}

/// 解析后的时序图。
#[derive(Clone, Debug)]
pub(super) struct SeqModel {
    /// 参与者：`(id, 显示标签)`。
    pub(super) parts: Vec<(String, String)>,
    pub(super) events: Vec<SeqEvent>,
}

/// 在参与者列表里确保存在某 id（首次出现时按 id 作标签）。
fn seq_ensure(parts: &mut Vec<(String, String)>, id: &str) {
    if id.is_empty() {
        return;
    }
    if !parts.iter().any(|(i, _)| i == id) {
        parts.push((id.to_string(), id.to_string()));
    }
}

/// 在字符串里定位时序箭头，返回 `(起点**字节**下标, 算子**字节**长度, 形态)`。
///
/// # 为什么必须返回字节下标（而非字符下标）
///
/// 调用方 [`parse_seq_message`] 拿这三个值去切**原字符串**
/// （`&line[..idx]` / `&line[idx + oplen..]`），而 `&str` 的下标是**字节**。
/// 若这里按 `chars()` 数出字符下标，含中文的行就会切进多字节字符中间——
/// `&line[..2]` 落在 `李`（占 3 字节）内部，直接 panic（`panic = "abort"`
/// 崩进程，见崩溃报告 `crash-20261010152913`，触发源码 `李四->>张三: …`）。
///
/// 算子全是纯 ASCII，按 `char_indices()` 扫到的字节位置天然落在字符边界上，
/// 故既安全、又能把字节长度 `pat.len()` 直接交给调用方做切片。
fn parse_seq_arrow(s: &str) -> Option<(usize, usize, SeqHead)> {
    let ops: &[(&str, SeqHead)] = &[
        ("-->>", SeqHead::Dotted),
        ("->>", SeqHead::Solid),
        ("-->", SeqHead::Solid),
        ("--x", SeqHead::Cross),
        ("->", SeqHead::Solid),
    ];
    for (i, _) in s.char_indices() {
        for (pat, head) in ops {
            if s[i..].starts_with(pat) {
                return Some((i, pat.len(), *head));
            }
        }
    }
    None
}

/// 解析一条消息行：`A->>B: text` 等。
pub(super) fn parse_seq_message(line: &str) -> Option<(String, String, String, SeqHead)> {
    let (idx, oplen, head) = parse_seq_arrow(line)?;
    // 取箭头**之前**的部分作发送方。用 `char::is_alphanumeric`（Unicode 版）
    // 而不是 `is_ascii_alphanumeric`：中文参与者名（`李四->>张三`）也是合法 id，
    // ASCII 版会把 `李四` 整段过滤成空串——图仍能画出来，但发送方丢失，
    // 消息箭头从「无名」发出，等于静默画错。
    let from: String = line[..idx]
        .trim()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let rest = &line[idx + oplen..];
    let (to, label) = match rest.find(':') {
        Some(c) => (
            rest[..c].trim().to_string(),
            rest[c + 1..].trim().to_string(),
        ),
        None => (rest.trim().to_string(), String::new()),
    };
    Some((from, to, label, head))
}

/// 把 `id as Label` / `id` 拆开。
fn split_as(idpart: &str) -> (String, String) {
    if let Some(as_idx) = idpart.find(" as ") {
        (
            idpart[..as_idx].trim().to_string(),
            idpart[as_idx + 4..].trim().to_string(),
        )
    } else {
        (idpart.trim().to_string(), idpart.trim().to_string())
    }
}

/// 把 `A,B` 或 `A : text` 这类尾巴拆成 `(id 串, 文案)`。
fn split_label(s: &str) -> (String, String) {
    match s.find(':') {
        Some(c) => (s[..c].trim().to_string(), s[c + 1..].trim().to_string()),
        None => (s.trim().to_string(), String::new()),
    }
}

/// 解析时序图源码。
pub(super) fn parse_sequence(src: &str) -> Result<SeqModel, MermaidError> {
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut events: Vec<SeqEvent> = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let lw = leading_word(t);
        if lw == "sequenceDiagram" || lw == "click" {
            continue;
        }
        if t.starts_with("participant ") || t.starts_with("actor ") {
            let kw_len = if t.starts_with("participant ") {
                "participant ".len()
            } else {
                "actor ".len()
            };
            let (id, label) = split_as(&t[kw_len..]);
            seq_ensure(&mut parts, &id);
            // 显式标签覆盖默认（id 作标签）。
            if let Some(pos) = parts.iter().position(|(i, _)| i == &id) {
                parts[pos].1 = label;
            }
            continue;
        }
        if t.starts_with("activate ") || t.starts_with("deactivate ") {
            continue;
        }
        if let Some(body) = t.strip_prefix("Note ") {
            if let Some(rest) = body.strip_prefix("over ") {
                let (ids, text) = split_label(rest);
                let ids: Vec<&str> = ids.split(',').map(str::trim).collect();
                let a = ids.first().copied().unwrap_or("").to_string();
                let b = ids.get(1).map(|s| s.to_string());
                seq_ensure(&mut parts, &a);
                if let Some(b) = &b {
                    seq_ensure(&mut parts, b);
                }
                events.push(SeqEvent::Note(SeqNote {
                    kind: NoteKind::Over,
                    a,
                    b,
                    text,
                }));
            } else if let Some(rest) = body.strip_prefix("left of ") {
                let (a, text) = split_label(rest);
                seq_ensure(&mut parts, &a);
                events.push(SeqEvent::Note(SeqNote {
                    kind: NoteKind::LeftOf,
                    a,
                    b: None,
                    text,
                }));
            } else if let Some(rest) = body.strip_prefix("right of ") {
                let (a, text) = split_label(rest);
                seq_ensure(&mut parts, &a);
                events.push(SeqEvent::Note(SeqNote {
                    kind: NoteKind::RightOf,
                    a,
                    b: None,
                    text,
                }));
            }
            continue;
        }
        if let Some((from, to, label, head)) = parse_seq_message(t) {
            seq_ensure(&mut parts, &from);
            seq_ensure(&mut parts, &to);
            events.push(SeqEvent::Msg(SeqMsg {
                from,
                to,
                label,
                head,
            }));
        }
    }
    Ok(SeqModel { parts, events })
}

/// 时序图布局 + 发图元。
pub(super) fn layout_sequence(
    src: &str,
    size: f32,
    _measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_sequence(src)?;
    if model.parts.is_empty() {
        return Err(MermaidError::Malformed("时序图没有任何参与者".to_string()));
    }
    let col_w = size * 6.0;
    let margin_x = size * 2.0;
    let top = size * 2.5;
    let row_h = size * 2.4;
    let nrows = model.events.len().max(1);
    let xs: Vec<f32> = (0..model.parts.len())
        .map(|i| margin_x + i as f32 * col_w)
        .collect();
    let bottom = top + nrows as f32 * row_h + size;

    let mut prims: Vec<Prim> = Vec::new();
    let mut id_rects: Vec<(String, Rect)> = Vec::new();
    let mut direct: Vec<(Rect, String)> = Vec::new();
    let clicks = collect_clicks(src);

    // 生命线 + 参与者标签。
    for (i, (id, label)) in model.parts.iter().enumerate() {
        let x = xs[i];
        prims.push(Prim::Line {
            a: Pt::new(x, top),
            b: Pt::new(x, bottom),
            width: 1.0 * (size / 16.0).max(0.7),
            dotted: true,
        });
        prims.push(Prim::Text {
            at: Pt::new(x, size * 0.8),
            text: label.clone(),
            size: size * 0.9,
            plate: false,
            color: None,
        });
        let r = Rect::from_min_max(
            Pt::new(x - col_w * 0.45, 0.0),
            Pt::new(x + col_w * 0.45, bottom),
        );
        id_rects.push((id.clone(), r));
        if let Some(url) = first_md_link(label) {
            direct.push((r, url));
        }
    }

    // 事件行。
    let line_w = 1.4 * (size / 16.0).max(0.7);
    for (k, ev) in model.events.iter().enumerate() {
        let y = top + (k as f32 + 0.5) * row_h;
        match ev {
            SeqEvent::Msg(m) => {
                let fi = model.parts.iter().position(|(i, _)| i == &m.from);
                let ti = model.parts.iter().position(|(i, _)| i == &m.to);
                let (Some(fi), Some(ti)) = (fi, ti) else {
                    continue;
                };
                let xf = xs[fi];
                let xt = xs[ti];
                let sign = if xt >= xf { 1.0 } else { -1.0 };
                prims.push(Prim::Line {
                    a: Pt::new(xf, y),
                    b: Pt::new(xt, y),
                    width: line_w,
                    dotted: m.head == SeqHead::Dotted,
                });
                match m.head {
                    SeqHead::Solid | SeqHead::Dotted => {
                        prims.push(Prim::Arrow {
                            tip: Pt::new(xt, y),
                            dir: Pt::new(sign, 0.0),
                            len: size * ARROW_LEN,
                            spread: ARROW_SPREAD,
                        });
                    }
                    SeqHead::Cross => {
                        prims.push(Prim::Cross {
                            at: Pt::new(xt, y),
                            dir: Pt::new(sign, 0.0),
                            len: size * ARROW_LEN * 0.7,
                        });
                    }
                }
                if !m.label.is_empty() {
                    prims.push(Prim::Text {
                        at: Pt::new((xf + xt) * 0.5, y),
                        text: m.label.clone(),
                        size: size * 0.85,
                        plate: true,
                        color: None,
                    });
                }
                if let Some(url) = first_md_link(&m.label) {
                    let lo = xf.min(xt) - size * 0.5;
                    let hi = xf.max(xt) + size * 0.5;
                    direct.push((
                        Rect::from_min_max(
                            Pt::new(lo, y - size * 0.6),
                            Pt::new(hi, y + size * 0.6),
                        ),
                        url,
                    ));
                }
            }
            SeqEvent::Note(n) => {
                let ai = model.parts.iter().position(|(i, _)| i == &n.a);
                let ai = match ai {
                    Some(i) => i,
                    None => continue,
                };
                let (x0, x1) = match n.kind {
                    NoteKind::Over => {
                        let bi =
                            n.b.as_ref()
                                .and_then(|b| model.parts.iter().position(|(i, _)| i == b));
                        let (lo, hi) = match bi {
                            Some(bi) => (xs[ai].min(xs[bi]), xs[ai].max(xs[bi])),
                            None => (xs[ai] - col_w * 0.4, xs[ai] + col_w * 0.4),
                        };
                        (lo - size, hi + size)
                    }
                    NoteKind::LeftOf => (xs[ai] - col_w * 1.3, xs[ai] - col_w * 0.3),
                    NoteKind::RightOf => (xs[ai] + col_w * 0.3, xs[ai] + col_w * 1.3),
                };
                let rect =
                    Rect::from_min_max(Pt::new(x0, y - size * 0.7), Pt::new(x1, y + size * 0.7));
                prims.push(Prim::Poly {
                    points: rect_corners(rect),
                    radius: 2.0,
                    fill: true,
                    ticks: false,
                    fill_color: None,
                    stroke_color: None,
                });
                if !n.text.is_empty() {
                    prims.push(Prim::Text {
                        at: rect.center(),
                        text: n.text.clone(),
                        size: size * 0.8,
                        plate: false,
                        color: None,
                    });
                }
                if let Some(url) = first_md_link(&n.text) {
                    direct.push((rect, url));
                }
            }
        }
    }

    Ok(finalize(prims, id_rects, clicks, direct))
}
