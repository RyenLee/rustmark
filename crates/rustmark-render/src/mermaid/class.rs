//! `classDiagram` 类图的解析与布局。

use crate::render::{Pt, geom::Rect};

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{collect_clicks, finalize, first_md_link, rect_boundary, rect_corners};
use super::*;

/// 关系形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RelKind {
    Inheritance,
    Composition,
    Aggregation,
    Association,
    DashedArrow,
    DashedInheritance,
    Plain,
    Dotted,
}

/// 一个类。
#[derive(Clone, Debug)]
pub(super) struct ClassBox {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) members: Vec<String>,
}

/// 一条关系。
#[derive(Clone, Debug)]
pub(super) struct Rel {
    pub(super) a: String,
    pub(super) b: String,
    pub(super) kind: RelKind,
    pub(super) label: Option<String>,
}

/// 解析后的类图。
#[derive(Clone, Debug)]
pub(super) struct ClassModel {
    pub(super) classes: Vec<ClassBox>,
    pub(super) rels: Vec<Rel>,
}

/// 解析类图源码。
pub(super) fn parse_class(src: &str) -> Result<ClassModel, MermaidError> {
    let mut classes: Vec<ClassBox> = Vec::new();
    let mut rels: Vec<Rel> = Vec::new();
    let mut in_class: Option<usize> = None;
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let lw = leading_word(t);
        if lw == "classDiagram" || lw == "click" {
            continue;
        }
        if let Some(ci) = in_class {
            if t == "}" {
                in_class = None;
                continue;
            }
            if !t.is_empty() {
                classes[ci].members.push(t.to_string());
            }
            continue;
        }
        if let Some(idpart) = t.strip_prefix("class ") {
            let (id, inline_body, closes_inline, has_body) = if let Some(brace) = idpart.find('{') {
                let id = idpart[..brace].trim().to_string();
                let after = &idpart[brace + 1..];
                match after.find('}') {
                    Some(c) => (id, after[..c].trim().to_string(), true, true),
                    None => (id, String::new(), false, true),
                }
            } else {
                (idpart.trim().to_string(), String::new(), false, false)
            };
            let pos = classes.iter().position(|c| c.id == id);
            let idx = match pos {
                Some(p) => p,
                None => {
                    classes.push(ClassBox {
                        id: id.clone(),
                        name: id.clone(),
                        members: Vec::new(),
                    });
                    classes.len() - 1
                }
            };
            if !inline_body.is_empty() {
                for mem in inline_body.split_whitespace() {
                    classes[idx].members.push(mem.to_string());
                }
            }
            in_class = if closes_inline {
                None
            } else if has_body {
                Some(idx)
            } else {
                None
            };
            continue;
        }
        if t == "}" {
            in_class = None;
            continue;
        }
        // 关系行：先按 `:` 拆标签，再找算子。
        if let Some(rel) = parse_rel(t) {
            rels.push(rel);
        }
        // 其它（classDef / 样式等）忽略。
    }
    // 关系端点若未显式声明类，补一个空类框，保证两端都可见。
    for rel in &rels {
        for id in [&rel.a, &rel.b] {
            if !id.is_empty() && !classes.iter().any(|c| c.id == *id) {
                classes.push(ClassBox {
                    id: id.clone(),
                    name: id.clone(),
                    members: Vec::new(),
                });
            }
        }
    }
    Ok(ClassModel { classes, rels })
}

/// 解析一条关系行。
fn parse_rel(t: &str) -> Option<Rel> {
    let ops: &[(&str, RelKind)] = &[
        ("<|--", RelKind::Inheritance),
        ("<|..", RelKind::DashedInheritance),
        ("--|>", RelKind::Inheritance),
        ("..|>", RelKind::DashedInheritance),
        ("*--", RelKind::Composition),
        ("o--", RelKind::Aggregation),
        ("-->", RelKind::Association),
        ("..>", RelKind::DashedArrow),
        ("--", RelKind::Plain),
        ("..", RelKind::Dotted),
    ];
    let (rel_part, label) = match t.find(':') {
        Some(c) => (&t[..c], Some(t[c + 1..].trim().to_string())),
        None => (t, None),
    };
    // 与 `parse_seq_arrow` 同一坑：原来按 `chars()` 数出**字符**下标 `i`，
    // 却拿它去切 `rel_part`（`&str`，下标是**字节**）。关系两端含中文时
    // （如 `用户 --> 订单`）字符数 < 字节数，`&rel_part[..i]` 会落进多字节
    // 字符内部 → panic（`panic = "abort"` 崩进程）。
    // 算子全是纯 ASCII，改按 `char_indices()` 取字节位置，并用
    // `rel_part[i..].starts_with(pat)` 做匹配——下标与切片双方同为字节，
    // 无需再引入 `Vec<char>`。
    for (i, _) in rel_part.char_indices() {
        for (pat, kind) in ops {
            if rel_part[i..].starts_with(pat) {
                let a = rel_part[..i].trim().to_string();
                let b = rel_part[i + pat.len()..].trim().to_string();
                return Some(Rel {
                    a,
                    b,
                    kind: *kind,
                    label,
                });
            }
        }
    }
    None
}

/// 类图布局 + 发图元。
pub(super) fn layout_class(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_class(src)?;
    if model.classes.is_empty() {
        return Err(MermaidError::Malformed("类图没有任何类".to_string()));
    }
    let cols = 3usize;
    let gap_x = size * 1.5;
    let gap_y = size * 1.5;
    let header_h = size * 1.6;
    let member_h = size * 1.2;
    // 每类尺寸。
    let mut box_w = size * 5.0;
    let mut box_hs: Vec<f32> = Vec::new();
    for c in &model.classes {
        let name_w = measure(&c.name, size);
        let mut mw = name_w;
        for m in &c.members {
            let w = measure(m, size * 0.85);
            if w > mw {
                mw = w;
            }
        }
        let w = (mw + size * 1.0).max(size * 5.0);
        if w > box_w {
            box_w = w;
        }
        let h = header_h + member_h * c.members.len().max(1) as f32 + size * 0.6;
        box_hs.push(h);
    }
    let max_h = box_hs.iter().copied().fold(0.0_f32, f32::max);
    let row_h = max_h + gap_y;
    let col_w = box_w + gap_x;
    // 位置。
    let mut rects: Vec<Rect> = Vec::new();
    for (i, _c) in model.classes.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let x = col as f32 * col_w + size;
        let y = row as f32 * row_h + size;
        let h = box_hs[i];
        rects.push(Rect::from_min_size(Pt::new(x, y), Pt::new(box_w, h)));
    }
    let mut prims: Vec<Prim> = Vec::new();
    let mut id_rects: Vec<(String, Rect)> = Vec::new();
    let mut direct: Vec<(Rect, String)> = Vec::new();
    let clicks = collect_clicks(src);
    for (i, c) in model.classes.iter().enumerate() {
        let r = rects[i];
        prims.push(Prim::Poly {
            points: rect_corners(r),
            radius: 2.0,
            fill: true,
            ticks: false,
            fill_color: None,
            stroke_color: None,
        });
        // 类名（居中于表头）。
        prims.push(Prim::Text {
            at: Pt::new(r.left() + box_w * 0.5, r.top() + header_h * 0.5),
            text: c.name.clone(),
            size,
            plate: false,
            color: None,
        });
        // 表头分隔线。
        prims.push(Prim::Line {
            a: Pt::new(r.left(), r.top() + header_h),
            b: Pt::new(r.right(), r.top() + header_h),
            width: 1.0 * (size / 16.0).max(0.7),
            dotted: false,
        });
        // 成员行。
        for (j, m) in c.members.iter().enumerate() {
            prims.push(Prim::Text {
                at: Pt::new(
                    r.left() + size * 0.4,
                    r.top() + header_h + member_h * (j as f32 + 0.5),
                ),
                text: m.clone(),
                size: size * 0.85,
                plate: false,
                color: None,
            });
        }
        id_rects.push((c.id.clone(), r));
        if let Some(url) = first_md_link(&c.name) {
            direct.push((r, url));
        }
        for m in &c.members {
            if let Some(url) = first_md_link(m) {
                direct.push((r, url));
            }
        }
    }
    // 关系连线。
    let line_w = 1.4 * (size / 16.0).max(0.7);
    for rel in &model.rels {
        let ai = model.classes.iter().position(|c| c.id == rel.a);
        let bi = model.classes.iter().position(|c| c.id == rel.b);
        let (Some(ai), Some(bi)) = (ai, bi) else {
            continue;
        };
        let ca = rects[ai].center();
        let cb = rects[bi].center();
        let delta = cb - ca;
        let len = delta.length();
        let dir = if len > 1e-6 {
            delta / len
        } else {
            Pt::new(1.0, 0.0)
        };
        let ha = Pt::new(rects[ai].width() * 0.5, rects[ai].height() * 0.5);
        let hb = Pt::new(rects[bi].width() * 0.5, rects[bi].height() * 0.5);
        let (sa, sb) = (rect_boundary(ca, ha, dir), rect_boundary(cb, hb, -dir));
        let dotted = matches!(
            rel.kind,
            RelKind::Dotted | RelKind::DashedArrow | RelKind::DashedInheritance
        );
        prims.push(Prim::Line {
            a: sa,
            b: sb,
            width: line_w,
            dotted,
        });
        match rel.kind {
            RelKind::Inheritance
            | RelKind::Association
            | RelKind::DashedArrow
            | RelKind::DashedInheritance => {
                prims.push(Prim::Arrow {
                    tip: sb,
                    dir: -dir,
                    len: size * ARROW_LEN,
                    spread: ARROW_SPREAD,
                });
            }
            RelKind::Composition | RelKind::Aggregation => {
                let dm = size * 0.5;
                let d = sb;
                let diamond = vec![
                    Pt::new(d.x - dm, d.y),
                    Pt::new(d.x, d.y - dm * 0.7),
                    Pt::new(d.x + dm, d.y),
                    Pt::new(d.x, d.y + dm * 0.7),
                ];
                prims.push(Prim::Poly {
                    points: diamond,
                    radius: 0.0,
                    fill: rel.kind == RelKind::Composition,
                    ticks: false,
                    fill_color: None,
                    stroke_color: None,
                });
            }
            RelKind::Plain | RelKind::Dotted => {}
        }
        if let Some(label) = &rel.label {
            let mid = Pt::new((sa.x + sb.x) * 0.5, (sa.y + sb.y) * 0.5);
            prims.push(Prim::Text {
                at: mid,
                text: label.clone(),
                size: size * 0.8,
                plate: true,
                color: None,
            });
            if let Some(url) = first_md_link(label) {
                direct.push((
                    Rect::from_center_size(
                        mid,
                        Pt::new(label.chars().count() as f32 * size * 0.5 + size, size * 1.4),
                    ),
                    url,
                ));
            }
        }
    }
    Ok(finalize(prims, id_rects, clicks, direct))
}
