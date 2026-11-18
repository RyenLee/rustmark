//! `requirementDiagram` 需求图：需求 / 元素框与它们之间的关系。
//!
//! 每个 `requirement` / `element` 块落成一个矩形节点（名称 + 字段），
//! `A - 关系词 -> B` 之类的语句落成带标签的箭头；布局复用流程图的分层管线。

use super::graph_layout::layout_from_graph;
use super::graph_parse::{intern, leading_word};
use super::model::{Edge, Graph, Head, LineStyle, NodeShape};
use super::*;

/// 需求图允许的关系动词。
const RELATION_WORDS: &[&str] = &[
    "contains",
    "satisfies",
    "derives",
    "traces",
    "copies",
    "verifies",
    "refines",
];

/// 块类型：决定节点标签里输出哪些字段。
#[derive(Clone, Copy)]
enum BlockKind {
    Requirement,
    Element,
}

/// 一个尚未闭合的块。
struct OpenBlock {
    id: String,
    kind: BlockKind,
    fields: Vec<(String, String)>,
}

/// `requirementDiagram` 的布局入口。
pub(super) fn layout_requirement(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let g = parse_requirement(src)?;
    if g.nodes.is_empty() {
        return Err(MermaidError::Malformed("需求图没有任何节点".to_string()));
    }
    layout_from_graph(g, src, size, measure)
}

/// 解析需求图为 [`Graph`]：需求 / 元素是节点，关系是边。
pub(super) fn parse_requirement(src: &str) -> Result<Graph, MermaidError> {
    let mut g = Graph {
        horizontal: false,
        reversed: false,
        nodes: Vec::new(),
        node_ids: Vec::new(),
        edges: Vec::new(),
        groups: Vec::new(),
    };
    let mut open: Option<OpenBlock> = None;
    let mut seen_header = false;

    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let head = leading_word(line).to_ascii_lowercase();
        if !seen_header {
            seen_header = true;
            if head == "requirementdiagram" {
                continue;
            }
        }

        // 块内部：`}` 收尾，其余每行是 `key: value`。
        if open.is_some() {
            if line == "}" {
                if let Some(block) = open.take() {
                    finish_block(&mut g, block);
                }
                continue;
            }
            if let Some((key, value)) = line.split_once(':')
                && let Some(block) = open.as_mut()
            {
                block
                    .fields
                    .push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
            continue;
        }

        if matches!(
            head.as_str(),
            "requirementdiagram"
                | "direction"
                | "acctitle"
                | "accdescr"
                | "style"
                | "classdef"
                | "class"
        ) {
            continue;
        }
        if head == "requirement" {
            start_block(
                &mut g,
                &mut open,
                BlockKind::Requirement,
                &line[head.len()..],
            )?;
            continue;
        }
        if head == "element" {
            start_block(&mut g, &mut open, BlockKind::Element, &line[head.len()..])?;
            continue;
        }

        let Some(rel) = parse_relation(line)? else {
            return Err(MermaidError::Malformed(format!(
                "无法识别的需求图语句：`{line}`"
            )));
        };
        let a = intern(&mut g, &rel.from, None, NodeShape::Rect);
        let b = intern(&mut g, &rel.to, None, NodeShape::Rect);
        g.edges.push(Edge {
            from: a,
            to: b,
            label: Some(rel.word.to_string()),
            line: LineStyle::Solid,
            head: Head::Arrow,
            bidirectional: false,
        });
    }

    // EOF 时仍未闭合的块：按已读到的字段收尾，不 panic。
    if let Some(block) = open {
        finish_block(&mut g, block);
    }
    Ok(g)
}

/// 开始一个 `requirement NAME {` / `element NAME {` 块（也可以没有 `{`）。
fn start_block(
    g: &mut Graph,
    open: &mut Option<OpenBlock>,
    kind: BlockKind,
    rest: &str,
) -> Result<(), MermaidError> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(MermaidError::Malformed("需求 / 元素名缺失".to_string()));
    }
    let (name, has_body) = match rest.strip_suffix('{') {
        Some(name) => (name.trim(), true),
        None => (rest, false),
    };
    let name = name.trim().trim_matches('"').trim().to_string();
    if name.is_empty() {
        return Err(MermaidError::Malformed("需求 / 元素名缺失".to_string()));
    }
    if has_body {
        *open = Some(OpenBlock {
            id: name,
            kind,
            fields: Vec::new(),
        });
    } else {
        intern(g, &name, None, NodeShape::Rect);
    }
    Ok(())
}

/// 块结束：按类型拼出节点标签并登记节点。
fn finish_block(g: &mut Graph, block: OpenBlock) {
    let text = match block.kind {
        BlockKind::Requirement => requirement_label(&block.id, &block.fields),
        BlockKind::Element => element_label(&block.id, &block.fields),
    };
    intern(g, &block.id, Some(text), NodeShape::Rect);
}

/// 取字段值（同名以后写的为准）；空值不输出。
fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// 需求节点：名称 + `id` / `text` / `risk` / `verifymethod`（只输出出现的字段）。
fn requirement_label(id: &str, fields: &[(String, String)]) -> String {
    let mut lines = vec![id.to_string()];
    for key in ["id", "text", "risk", "verifymethod"] {
        if let Some(value) = field(fields, key) {
            let value = if key == "text" {
                // 长文本按原样保留，但最多 3 行，避免框过高。
                split_label_lines(value)
                    .into_iter()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("<br/>")
            } else {
                value.to_string()
            };
            lines.push(format!("{key}: {value}"));
        }
    }
    lines.join("<br/>")
}

/// 元素节点：名称 + `type`（若存在）。
fn element_label(id: &str, fields: &[(String, String)]) -> String {
    match field(fields, "type") {
        Some(t) => format!("{id}<br/>type: {t}"),
        None => id.to_string(),
    }
}

/// 一条关系：起点、终点与关系词。
struct Relation {
    from: String,
    to: String,
    word: &'static str,
}

/// 解析 `A - contains -> B` / `A <- satisfies - B`。
///
/// 箭头指向即边的方向：`->` 从左侧指向右侧，`<-` 从右侧指向左侧。
fn parse_relation(line: &str) -> Result<Option<Relation>, MermaidError> {
    if let Some(idx) = line.find("->") {
        let left = line[..idx].trim();
        let to = line[idx + 2..].trim();
        let Some((from, word)) = left.rsplit_once('-') else {
            return Err(MermaidError::Malformed(format!(
                "需求关系缺少关系词：`{line}`"
            )));
        };
        let from = from.trim();
        let word = relation_word(word.trim())?;
        if from.is_empty() || to.is_empty() {
            return Err(MermaidError::Malformed(format!(
                "需求关系两端必须都有节点：`{line}`"
            )));
        }
        return Ok(Some(Relation {
            from: from.to_string(),
            to: to.to_string(),
            word,
        }));
    }
    if let Some(idx) = line.find("<-") {
        let left = line[..idx].trim();
        let rest = line[idx + 2..].trim();
        let Some((word, to)) = rest.split_once('-') else {
            return Err(MermaidError::Malformed(format!(
                "需求关系缺少关系词：`{line}`"
            )));
        };
        let word = relation_word(word.trim())?;
        let to = to.trim();
        if left.is_empty() || to.is_empty() {
            return Err(MermaidError::Malformed(format!(
                "需求关系两端必须都有节点：`{line}`"
            )));
        }
        // 箭头指向左：边从右侧节点指向左侧节点。
        return Ok(Some(Relation {
            from: to.to_string(),
            to: left.to_string(),
            word,
        }));
    }
    Ok(None)
}

/// 校验关系动词（大小写不敏感），返回规范写法。
fn relation_word(word: &str) -> Result<&'static str, MermaidError> {
    RELATION_WORDS
        .iter()
        .copied()
        .find(|w| w.eq_ignore_ascii_case(word))
        .ok_or_else(|| MermaidError::Malformed(format!("不支持的需求关系 `{word}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假度量：每字符宽 `0.5em`（与其它图种的测试同口径）。
    fn m() -> impl Fn(&str, f32) -> f32 {
        |s: &str, size: f32| s.chars().count() as f32 * size * 0.5
    }

    fn lay(src: &str) -> MermaidLayout {
        let f = m();
        layout_requirement(src, 14.0, &f).expect("应能布局")
    }

    #[test]
    fn requirement_block_fields_are_parsed() {
        let g = parse_requirement(
            "requirementDiagram\n\
             requirement req1 {\n\
               id: 1\n\
               text: 系统必须支持导出\n\
               risk: high\n\
               verifymethod: test\n\
             }\n",
        )
        .expect("解析需求块");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.node_ids[0], "req1");
        assert_eq!(
            g.nodes[0].0,
            "req1<br/>id: 1<br/>text: 系统必须支持导出<br/>risk: high<br/>verifymethod: test"
        );
        assert_eq!(g.nodes[0].1, NodeShape::Rect);
    }

    #[test]
    fn only_present_fields_are_shown() {
        let g = parse_requirement(
            "requirementDiagram\n\
             requirement req1 {\n\
               text: 只写了文本\n\
             }\n",
        )
        .expect("解析需求块");
        assert_eq!(g.nodes[0].0, "req1<br/>text: 只写了文本");
    }

    #[test]
    fn element_block_shows_type() {
        let g = parse_requirement(
            "requirementDiagram\n\
             element e1 {\n\
               type: simulation\n\
             }\n",
        )
        .expect("解析元素块");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].0, "e1<br/>type: simulation");
    }

    #[test]
    fn forward_relation_points_right() {
        let g = parse_requirement(
            "requirementDiagram\n\
             requirement req1 {\n\
               id: 1\n\
             }\n\
             element e1 {\n\
               type: simulation\n\
             }\n\
             req1 - contains -> e1\n",
        )
        .expect("解析关系");
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.node_ids[g.edges[0].from], "req1");
        assert_eq!(g.node_ids[g.edges[0].to], "e1");
        assert_eq!(g.edges[0].label.as_deref(), Some("contains"));
        assert_eq!(g.edges[0].head, Head::Arrow);
    }

    #[test]
    fn backward_relation_points_left() {
        let g = parse_requirement(
            "requirementDiagram\n\
             requirement req1\n\
             requirement req2\n\
             req1 <- satisfies - req2\n",
        )
        .expect("解析关系");
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].from, 1, "箭头指向左：边应从 req2 指向 req1");
        assert_eq!(g.edges[0].to, 0);
        assert_eq!(g.edges[0].label.as_deref(), Some("satisfies"));
    }

    #[test]
    fn undeclared_endpoints_are_interned() {
        let g = parse_requirement("requirementDiagram\nreq1 - derives -> e9\n").expect("解析关系");
        assert_eq!(g.nodes.len(), 2, "两端未声明时应各补一个节点");
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn layout_has_positive_size_and_one_box_per_node() {
        let src = "requirementDiagram\n\
                   requirement req1 {\n\
                     id: 1\n\
                     text: 导出\n\
                   }\n\
                   element e1 {\n\
                     type: simulation\n\
                   }\n\
                   req1 - contains -> e1\n";
        let l = lay(src);
        assert!(l.width > 0.0 && l.height > 0.0);
        let g = parse_requirement(src).expect("解析");
        assert_eq!(l.node_count(), g.nodes.len());
        assert_eq!(l.node_count(), 2);
    }

    #[test]
    fn unterminated_block_does_not_panic() {
        let f = m();
        // 缺 `}`：只要求不 panic；按已读到的字段收尾。
        let l = layout_requirement("requirementDiagram\nrequirement r1 {\n id: 1\n", 14.0, &f)
            .expect("应能容错收尾");
        assert_eq!(l.node_count(), 1);
    }

    #[test]
    fn empty_requirement_is_malformed() {
        let f = m();
        let e = layout_requirement("requirementDiagram\n", 14.0, &f).expect_err("应报错");
        assert!(matches!(e, MermaidError::Malformed(_)), "实际 {e:?}");
    }

    #[test]
    fn unknown_relation_word_is_malformed() {
        let f = m();
        let e = layout_requirement("requirementDiagram\nreq1 - likes -> e1\n", 14.0, &f)
            .expect_err("应报错");
        assert!(matches!(e, MermaidError::Malformed(_)), "实际 {e:?}");
    }

    #[test]
    fn long_text_field_is_truncated_to_three_lines() {
        // `text:` 超过 3 行：只保留前 3 行，第 4 行丢弃。
        let g = parse_requirement(
            "requirementDiagram\n\
             requirement req1 {\n\
               text: a<br/>b<br/>c<br/>d\n\
             }\n",
        )
        .expect("解析长文本");
        let lines = split_label_lines(&g.nodes[0].0);
        assert_eq!(lines.len(), 4, "名称 + 最多 3 行文本");
        assert_eq!(lines[3], "c", "第 3 行文本应是最后一行");
        assert!(!lines.iter().any(|l| l == "d"), "第 4 行文本应被截断");
    }

    #[test]
    fn requirement_without_body_creates_name_only_node() {
        // 没有 `{`：只生成一个以名字为标签的节点。
        let src = "requirementDiagram\nrequirement r1\n";
        let g = parse_requirement(src).expect("解析无块需求");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.node_ids[0], "r1");
        assert_eq!(g.nodes[0].0, "r1", "没有字段时标签只有名字");
        assert_eq!(lay(src).node_count(), 1);
    }

    #[test]
    fn repeated_field_uses_last_value() {
        // 同一字段重复出现：以最后一次为准，且节点不重复。
        let g = parse_requirement(
            "requirementDiagram\n\
             requirement req1 {\n\
               text: 第一段\n\
               text: 第二段\n\
             }\n",
        )
        .expect("解析重复字段");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].0, "req1<br/>text: 第二段");
    }

    #[test]
    fn undeclared_endpoints_keep_edge_direction() {
        // 两端都未声明：自动补节点，箭头方向仍是从左到右。
        let g = parse_requirement("requirementDiagram\ne9 - verifies -> req9\n").expect("解析关系");
        assert_eq!(g.nodes.len(), 2, "两端未声明时应各补一个节点");
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.node_ids[g.edges[0].from], "e9");
        assert_eq!(g.node_ids[g.edges[0].to], "req9");
        assert_eq!(g.edges[0].label.as_deref(), Some("verifies"));
    }

    #[test]
    fn element_without_type_shows_name_only() {
        // `element` 块没有 `type` 字段：标签只有名字。
        let g = parse_requirement(
            "requirementDiagram\n\
             element e1 {\n\
               docref: 某文档\n\
             }\n",
        )
        .expect("解析元素块");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].0, "e1", "没有 type 时标签只有名字");
    }

    #[test]
    fn comment_only_diagram_is_malformed() {
        // 空图（只有注释与装饰指令）：返回 Malformed。
        let f = m();
        let e = layout_requirement("requirementDiagram\n%% 只有注释\ndirection LR\n", 14.0, &f)
            .expect_err("没有节点应报错");
        assert!(matches!(e, MermaidError::Malformed(_)), "实际 {e:?}");
    }

    #[test]
    fn unknown_relation_word_is_case_insensitive_malformed() {
        // 未知关系词（大小写不敏感地匹配不到）：返回 Malformed。
        let f = m();
        let e = layout_requirement("requirementDiagram\nreq1 - LIKES -> e1\n", 14.0, &f)
            .expect_err("应报错");
        assert!(matches!(e, MermaidError::Malformed(_)), "实际 {e:?}");
    }
}
