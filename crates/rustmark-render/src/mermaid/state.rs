//! `stateDiagram` / `stateDiagram-v2`：状态与迁移。
//!
//! 状态落成圆角矩形、迁移落成带标签的箭头，`[*]` 画成小圆点；
//! 布局直接复用流程图的分层管线——状态图本质就是有向图的一种写法。

use super::graph_layout::layout_from_graph;
use super::graph_parse::{intern, leading_word};
use super::model::{Edge, Graph, Head, LineStyle, NodeShape};
use super::*;

/// `stateDiagram-v2` 的布局入口。
pub(super) fn layout_state(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let g = parse_state(src)?;
    if g.nodes.is_empty() {
        return Err(MermaidError::Malformed("状态图里没有任何状态".to_string()));
    }
    layout_from_graph(g, src, size, measure)
}

/// 解析状态图为 [`Graph`]：状态是节点，迁移是边。
pub(super) fn parse_state(src: &str) -> Result<Graph, MermaidError> {
    let mut g = Graph {
        horizontal: false,
        reversed: false,
        nodes: Vec::new(),
        node_ids: Vec::new(),
        edges: Vec::new(),
        groups: Vec::new(),
    };
    // `[*]` 每出现一次就是一个独立的起止标记，用序号区分。
    let mut dots = 0usize;
    let mut seen_header = false;
    for line in src.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("%%") || line == "}" {
            continue;
        }
        let head = leading_word(line).to_ascii_lowercase();
        if !seen_header {
            seen_header = true;
            if head == "statediagram" {
                continue;
            }
        }
        // 状态图里与结构无关的指令：忽略而不是回退。
        if matches!(
            head.as_str(),
            "statediagram" | "direction" | "classdef" | "class" | "style" | "note" | "end"
        ) {
            continue;
        }
        if head == "state" {
            parse_state_decl(line, &mut g);
            continue;
        }
        let Some((from, rest)) = line.split_once("-->") else {
            // 没有迁移的裸状态声明（如单独一行 `已激活`）。
            let id = line.split_whitespace().next().unwrap_or_default();
            if !id.is_empty() {
                intern(&mut g, id, None, NodeShape::Rounded);
            }
            continue;
        };
        let (to, label) = split_label(rest);
        let a = state_ref(&mut g, from.trim(), &mut dots);
        let b = state_ref(&mut g, to, &mut dots);
        g.edges.push(Edge {
            from: a,
            to: b,
            label,
            line: LineStyle::Solid,
            head: Head::Arrow,
            bidirectional: false,
        });
    }
    Ok(g)
}

/// 迁移末尾的 `: 文案` 拆成（目标状态, 标签）。
fn split_label(rest: &str) -> (&str, Option<String>) {
    match rest.split_once(':') {
        Some((to, label)) => {
            let label = label.trim();
            (to.trim(), (!label.is_empty()).then(|| label.to_string()))
        }
        None => (rest.trim(), None),
    }
}

/// 状态引用：`[*]` 建成圆点，其余按状态名取（或新建）节点。
fn state_ref(g: &mut Graph, name: &str, dots: &mut usize) -> usize {
    if name == "[*]" {
        *dots += 1;
        let id = format!("__dot{dots}");
        return intern(g, &id, Some(String::new()), NodeShape::Dot);
    }
    intern(g, name, None, NodeShape::Rounded)
}

/// `state "描述" as id` / `state id` / `state id {` 三种声明。
fn parse_state_decl(line: &str, g: &mut Graph) {
    let rest = line[leading_word(line).len()..].trim();
    if rest.is_empty() {
        return;
    }
    if let Some(body) = rest.strip_prefix('"')
        && let Some(close) = body.find('"')
    {
        let desc = &body[..close];
        let after = body[close + 1..].trim();
        if let Some(id) = after.strip_prefix("as ") {
            let id = id.trim().trim_end_matches('{').trim();
            if !id.is_empty() {
                intern(g, id, Some(desc.to_string()), NodeShape::Rounded);
            }
            return;
        }
    }
    let id = rest.trim_end_matches('{').trim();
    if !id.is_empty() {
        intern(g, id, None, NodeShape::Rounded);
    }
}
