//! `mindmap` 思维导图：按缩进建树，从左到右分层排布。
//!
//! 语法：
//!
//! ```text
//! mindmap
//!   root((根节点))
//!     分支A
//!       子节点1
//!       子节点2
//!     分支B[方框]
//! ```
//!
//! 行首空格数决定层级，tab 按 4 个空格计；同一缩进按出现顺序排列。
//! 行内形状包裹（`((圆))` / `(圆角)` / `[方框]` / `{{六边形}}`）会被剥离，
//! `id[文本]` 取括号内文本，纯 `id` 直接当文本。空内容返回 `Malformed`。

use std::collections::VecDeque;

use crate::render::{Pt, Rgb8, geom::Rect};

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{collect_clicks, finalize, rect_corners};
use super::*;

/// 树节点（arena 存储，`children` 为 `nodes` 下标）。
#[derive(Clone, Debug)]
pub(super) struct MindNode {
    pub(super) text: String,
    pub(super) children: Vec<usize>,
}

/// 解析后的思维导图。
#[derive(Clone, Debug)]
pub(super) struct MindModel {
    pub(super) nodes: Vec<MindNode>,
    /// 根节点下标（首条节点）。
    pub(super) root: usize,
}

/// 行首缩进宽度（空格 1、tab 4），返回缩进与去掉缩进后的内容。
fn leading_indent(line: &str) -> (usize, &str) {
    let mut width = 0usize;
    let mut bytes = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => {
                width += 1;
                bytes += 1;
            }
            '\t' => {
                width += 4;
                bytes += 1;
            }
            _ => break,
        }
    }
    (width, &line[bytes..])
}

/// 去掉行内的形状包裹，只留显示文本。
fn strip_shape(raw: &str) -> String {
    let t = raw.trim();
    // 找到第一个形状开括号，并记住配对的闭括号与开括号长度。
    let mut opener: Option<(usize, &'static str, usize)> = None;
    for (i, ch) in t.char_indices() {
        match ch {
            '(' => {
                let double = t[i + 1..].starts_with('(');
                opener = Some((
                    i,
                    if double { "))" } else { ")" },
                    if double { 2 } else { 1 },
                ));
                break;
            }
            '[' => {
                opener = Some((i, "]", 1));
                break;
            }
            '{' => {
                let double = t[i + 1..].starts_with('{');
                opener = Some((
                    i,
                    if double { "}}" } else { "}" },
                    if double { 2 } else { 1 },
                ));
                break;
            }
            _ => {}
        }
    }
    let Some((open_at, close, open_len)) = opener else {
        return t.to_string();
    };
    let Some(close_at) = t.rfind(close) else {
        return t.to_string();
    };
    if close_at < open_at + open_len {
        return t.to_string();
    }
    let inner = t[open_at + open_len..close_at].trim();
    if !inner.is_empty() {
        return inner.to_string();
    }
    // `[]` 这类空包裹退回 id 部分，避免画出空节点。
    let id = t[..open_at].trim();
    if id.is_empty() {
        t.to_string()
    } else {
        id.to_string()
    }
}

/// 解析思维导图源码。
pub(super) fn parse_mindmap(src: &str) -> Result<MindModel, MermaidError> {
    let mut nodes: Vec<MindNode> = Vec::new();
    // (缩进宽度, 节点下标)：维护当前分支的祖先链。
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut header_seen = false;
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }
        if !header_seen {
            header_seen = true;
            if leading_word(trimmed) == "mindmap" {
                continue;
            }
        }
        let (indent, rest) = leading_indent(line);
        let text = strip_shape(rest);
        if text.is_empty() {
            return Err(MermaidError::Malformed(format!(
                "思维导图节点缺少文本：{trimmed}"
            )));
        }
        let parent = if nodes.is_empty() {
            None
        } else {
            while let Some(&(depth, _)) = stack.last() {
                if depth >= indent {
                    stack.pop();
                } else {
                    break;
                }
            }
            Some(stack.last().map(|(_, i)| *i).unwrap_or(0))
        };
        let idx = nodes.len();
        nodes.push(MindNode {
            text,
            children: Vec::new(),
        });
        if let Some(p) = parent
            && let Some(node) = nodes.get_mut(p)
        {
            node.children.push(idx);
        }
        stack.push((indent, idx));
    }
    if !header_seen || nodes.is_empty() {
        return Err(MermaidError::Malformed("思维导图没有任何节点".to_string()));
    }
    Ok(MindModel { nodes, root: 0 })
}

/// 按亮度选文字色：浅底用深字，深底用白字。
fn contrast_text(color: Rgb8) -> Rgb8 {
    let lum = 0.299 * color.r as f32 + 0.587 * color.g as f32 + 0.114 * color.b as f32;
    if lum > 150.0 {
        Rgb8::opaque(0x33, 0x33, 0x33)
    } else {
        Rgb8::opaque(255, 255, 255)
    }
}

/// 思维导图布局 + 发图元。
pub(super) fn layout_mindmap(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_mindmap(src)?;
    let n = model.nodes.len();
    let margin = size;
    let line_w = 1.4 * (size / 16.0).max(0.7);
    let gap_x = size * 1.8;
    let gap_y = size * 0.7;
    let node_h = size * 1.9;
    let pad_x = size * 0.8;

    let node_w: Vec<f32> = model
        .nodes
        .iter()
        .map(|node| (measure(&node.text, size * 0.9) + pad_x * 2.0).max(size * 2.0))
        .collect();

    // 从根做一次遍历，得到树深度（顺带校验每个节点都被挂上）。
    let mut depth = vec![0usize; n];
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(model.root);
    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &child in &model.nodes[i].children {
            depth[child] = depth[i] + 1;
            queue.push_back(child);
        }
    }

    // 每层最大节点宽 → 列左边界。
    let mut col_w: Vec<f32> = Vec::new();
    for (i, w) in node_w.iter().enumerate() {
        let d = depth[i];
        if d >= col_w.len() {
            col_w.resize(d + 1, 0.0);
        }
        col_w[d] = col_w[d].max(*w);
    }
    let mut col_x: Vec<f32> = Vec::with_capacity(col_w.len());
    let mut x = margin;
    for w in &col_w {
        col_x.push(x);
        x += *w + gap_x;
    }

    // 子树高度：后序（`order` 的反向）保证子节点先算。
    let mut subtree: Vec<f32> = vec![node_h; n];
    for &i in order.iter().rev() {
        let children = &model.nodes[i].children;
        if children.is_empty() {
            continue;
        }
        let mut sum = 0.0_f32;
        for &child in children {
            sum += subtree[child];
        }
        sum += gap_y * (children.len() as f32 - 1.0);
        subtree[i] = subtree[i].max(sum);
    }

    // 分配每个节点的中心点：父节点在本子树带内垂直居中，子节点依次堆叠。
    let mut centers = vec![Pt::zero(); n];
    let top = margin;
    let mut pending: Vec<(usize, f32)> = vec![(model.root, top)];
    while let Some((i, band_top)) = pending.pop() {
        let d = depth[i];
        let cx = col_x[d] + col_w[d] * 0.5;
        let cy = band_top + subtree[i] * 0.5;
        centers[i] = Pt::new(cx, cy);
        let children = &model.nodes[i].children;
        if children.is_empty() {
            continue;
        }
        let mut sum = 0.0_f32;
        for &child in children {
            sum += subtree[child];
        }
        sum += gap_y * (children.len() as f32 - 1.0);
        let mut child_top = band_top + (subtree[i] - sum) * 0.5;
        for &child in children {
            pending.push((child, child_top));
            child_top += subtree[child] + gap_y;
        }
    }

    let rects: Vec<Rect> = (0..n)
        .map(|i| Rect::from_center_size(centers[i], Pt::new(node_w[i], node_h)))
        .collect();

    let mut prims: Vec<Prim> = Vec::new();
    // 连线先发：父右中点 → 中间 x 的横段，再折向子左中点。
    for &i in &order {
        let parent = rects[i];
        for &child in &model.nodes[i].children {
            let rect = rects[child];
            let mid_x = (parent.right() + rect.left()) * 0.5;
            let py = parent.center().y;
            let cy = rect.center().y;
            prims.push(Prim::Line {
                a: Pt::new(parent.right(), py),
                b: Pt::new(mid_x, py),
                width: line_w,
                dotted: false,
            });
            prims.push(Prim::Line {
                a: Pt::new(mid_x, py),
                b: Pt::new(rect.left(), cy),
                width: line_w,
                dotted: false,
            });
        }
    }
    // 节点框与文字。
    let root_children = &model.nodes[model.root].children;
    for (i, node) in model.nodes.iter().enumerate() {
        let rect = rects[i];
        let (fill, text_color) = if i == model.root {
            let color = palette_color(0);
            (Some(color), Some(contrast_text(color)))
        } else if depth[i] == 1 {
            let ordinal = root_children.iter().position(|&c| c == i).unwrap_or(0);
            let color = palette_color(ordinal + 1);
            (Some(color), Some(contrast_text(color)))
        } else {
            (None, None)
        };
        prims.push(Prim::Poly {
            points: rect_corners(rect),
            radius: size * 0.4,
            fill: true,
            ticks: false,
            fill_color: fill,
            stroke_color: None,
        });
        prims.push(Prim::Text {
            at: rect.center(),
            text: node.text.clone(),
            size: size * 0.9,
            plate: false,
            color: text_color,
        });
    }

    Ok(finalize(prims, Vec::new(), collect_clicks(src), Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> impl Fn(&str, f32) -> f32 {
        |s: &str, size: f32| s.chars().count() as f32 * size * 0.5
    }

    fn lay(src: &str) -> MermaidLayout {
        let f = m();
        layout_mindmap(src, 14.0, &f).expect("应能布局")
    }

    #[test]
    fn parses_indent_tree() {
        let src = concat!(
            "mindmap\n",
            "  root((根节点))\n",
            "    分支A\n",
            "      子节点1\n",
            "      子节点2\n",
            "    分支B[方框]\n",
        );
        let model = parse_mindmap(src).expect("解析思维导图");
        assert_eq!(model.nodes.len(), 5);
        assert_eq!(model.nodes[model.root].text, "根节点");
        assert_eq!(model.nodes[model.root].children.len(), 2);
        let branch_a = model.nodes[model.root].children[0];
        assert_eq!(model.nodes[branch_a].text, "分支A");
        assert_eq!(model.nodes[branch_a].children.len(), 2);
        let branch_b = model.nodes[model.root].children[1];
        assert_eq!(model.nodes[branch_b].text, "方框");
        assert!(model.nodes[branch_b].children.is_empty());
    }

    #[test]
    fn tab_counts_as_four_spaces() {
        let model = parse_mindmap("mindmap\nroot\n\tchild\n").expect("解析思维导图");
        assert_eq!(model.nodes[model.root].children.len(), 1);
        assert_eq!(
            model.nodes[model.nodes[model.root].children[0]].text,
            "child"
        );
    }

    #[test]
    fn strips_shape_wrappers() {
        assert_eq!(strip_shape("root((根节点))"), "根节点");
        assert_eq!(strip_shape("(圆角)"), "圆角");
        assert_eq!(strip_shape("[方框]"), "方框");
        assert_eq!(strip_shape("{{六边形}}"), "六边形");
        assert_eq!(strip_shape("id[文本]"), "文本");
        assert_eq!(strip_shape("纯id"), "纯id");
    }

    #[test]
    fn layout_has_positive_size_and_prims() {
        let l = lay("mindmap\n 根\n   分支A\n     子节点1\n     子节点2\n   分支B[方框]\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(prim_count(&l) > 0);
        assert_eq!(l.node_count(), 5);
    }

    #[test]
    fn empty_input_is_malformed() {
        let f = m();
        for src in ["", "mindmap\n", "mindmap\n   \n%% 只有注释\n"] {
            assert!(
                matches!(
                    layout_mindmap(src, 14.0, &f),
                    Err(MermaidError::Malformed(_))
                ),
                "{src:?} 应报 Malformed"
            );
        }
    }

    #[test]
    fn single_node_lays_out() {
        let l = lay("mindmap\n  只有根\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert_eq!(l.node_count(), 1);
    }

    #[test]
    fn deep_chain_grows_width_without_panic() {
        let mut src = String::from("mindmap\n");
        for depth in 0..10 {
            src.push_str(&"  ".repeat(depth));
            src.push_str(&format!("层{depth}\n"));
        }
        let deep = lay(&src);
        let single = lay("mindmap\n  只有根\n");
        assert_eq!(deep.node_count(), 10);
        assert!(
            deep.width > single.width,
            "深链宽度应随深度增长：{} > {}",
            deep.width,
            single.width
        );
    }

    #[test]
    fn three_level_tree_keeps_all_nodes() {
        let l = lay("mindmap\n root\n   A\n     a1\n     a2\n     a3\n   B\n");
        assert_eq!(l.node_count(), 6, "根 + 2 分支 + 3 子节点");
    }

    #[test]
    fn mixed_shape_wrappers_strip_to_plain_text() {
        let model = parse_mindmap("mindmap\n root((根))\n   id((圆))\n   id[方框]\n")
            .expect("解析思维导图");
        let texts: Vec<&str> = model.nodes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(texts, vec!["根", "圆", "方框"]);
    }

    #[test]
    fn sibling_root_lines_keep_all_nodes() {
        let src = "mindmap\n  甲\n  乙\n  丙\n";
        let model = parse_mindmap(src).expect("解析思维导图");
        assert_eq!(model.nodes.len(), 3, "同缩进的多个根行不应丢节点");
        assert_eq!(model.nodes[model.root].children.len(), 2);
        assert_eq!(lay(src).node_count(), 3);
    }
}
