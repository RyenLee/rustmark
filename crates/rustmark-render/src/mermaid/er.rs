//! `erDiagram` 实体关系图：实体（名称 + 属性列表）与带基数标注的关系。
//!
//! 实体落成矩形节点（每个属性一行），关系落成带箭头与标签的边；布局直接
//! 复用流程图的分层管线——ER 图本质就是有向图的一种写法，无需另写一套
//! 定位逻辑。

use super::graph_layout::layout_from_graph;
use super::graph_parse::{intern, leading_word};
use super::model::{Edge, Graph, Head, LineStyle, NodeShape};
use super::*;

/// 每个实体框最多显示的属性行数（再多框会过高，挤占版面）。
const MAX_ATTRS: usize = 6;

/// 基数记号 → 显示文本。顺序无关紧要：记号都是两字符且互不为对方的后缀。
const CARDINALITIES: &[(&str, &str)] = &[
    ("||", "1"),
    ("o|", "0..1"),
    ("|o", "0..1"),
    ("o{", "0..*"),
    ("}o", "0..*"),
    ("|{", "1..*"),
    ("}|", "1..*"),
];

/// `erDiagram` 的布局入口。
pub(super) fn layout_er(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let g = parse_er(src)?;
    if g.nodes.is_empty() {
        return Err(MermaidError::Malformed("ER 图里没有任何实体".to_string()));
    }
    layout_from_graph(g, src, size, measure)
}

/// 解析 ER 图为 [`Graph`]：实体是节点，关系是边。
pub(super) fn parse_er(src: &str) -> Result<Graph, MermaidError> {
    let mut g = Graph {
        horizontal: false,
        reversed: false,
        nodes: Vec::new(),
        node_ids: Vec::new(),
        edges: Vec::new(),
        groups: Vec::new(),
    };
    // 当前打开中的实体块：`(实体名, 已读到的属性行)`。
    let mut in_entity: Option<(String, Vec<String>)> = None;
    let mut seen_header = false;

    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let head = leading_word(line).to_ascii_lowercase();
        if !seen_header {
            seen_header = true;
            if head == "erdiagram" {
                continue;
            }
        }
        // 与结构无关的装饰性指令：忽略而不是让整张图回退到源码。
        if matches!(
            head.as_str(),
            "erdiagram" | "direction" | "acctitle" | "accdescr" | "style" | "classdef" | "class"
        ) {
            continue;
        }

        // 实体块内部：`}` 收尾，其余每行都是属性。
        if in_entity.is_some() {
            if line == "}" {
                if let Some((name, attrs)) = in_entity.take() {
                    let text = entity_label(&name, &attrs);
                    intern(&mut g, &name, Some(text), NodeShape::Rect);
                }
                continue;
            }
            if let Some((_, attrs)) = in_entity.as_mut()
                && attrs.len() < MAX_ATTRS
            {
                attrs.push(line.to_string());
            }
            continue;
        }

        if line == "}" {
            return Err(MermaidError::Malformed(
                "多余的 `}`（没有对应的实体块）".to_string(),
            ));
        }
        // 实体块开头：`NAME {`。
        if let Some(name) = line.strip_suffix('{') {
            let name = unquote(name);
            if name.is_empty() {
                return Err(MermaidError::Malformed("实体名缺失".to_string()));
            }
            in_entity = Some((name, Vec::new()));
            continue;
        }
        // 关系行：`A <基数>--<基数> B : 标签`。
        if line.contains("--") {
            let (rel_part, label) = match line.split_once(':') {
                Some((rel, label)) => (rel.trim(), Some(label.trim().to_string())),
                None => (line, None),
            };
            let rel = parse_relation(rel_part)?;
            let a = intern(&mut g, &rel.from, None, NodeShape::Rect);
            let b = intern(&mut g, &rel.to, None, NodeShape::Rect);
            g.edges.push(Edge {
                from: a,
                to: b,
                label: relation_label(label.as_deref(), rel.from_card, rel.to_card),
                line: LineStyle::Solid,
                head: Head::Arrow,
                bidirectional: false,
            });
            continue;
        }
        // 单独一行的实体名：按隐式声明处理（与 Mermaid 的宽容行为一致）。
        if is_entity_ref(line) {
            let id = unquote(line);
            intern(&mut g, &id, None, NodeShape::Rect);
            continue;
        }
        return Err(MermaidError::Malformed(format!(
            "无法识别的 ER 语句：`{line}`"
        )));
    }

    // EOF 时仍未闭合的实体块：按已读到的属性收尾，不 panic。
    if let Some((name, attrs)) = in_entity {
        let text = entity_label(&name, &attrs);
        intern(&mut g, &name, Some(text), NodeShape::Rect);
    }
    Ok(g)
}

/// 实体节点的显示文本：名称之后每个属性一行（`<br/>` 分隔）。
fn entity_label(name: &str, attrs: &[String]) -> String {
    let mut text = name.to_string();
    for attr in attrs.iter().take(MAX_ATTRS) {
        text.push_str("<br/>");
        text.push_str(attr);
    }
    text
}

/// 一条关系：两端实体名与各自的基数显示文本。
struct Relation {
    from: String,
    to: String,
    from_card: Option<&'static str>,
    to_card: Option<&'static str>,
}

/// 解析 `A <card>--<card> B` 部分；两端缺一即视为畸形。
fn parse_relation(text: &str) -> Result<Relation, MermaidError> {
    let Some((left, right)) = text.split_once("--") else {
        return Err(MermaidError::Malformed(format!("关系缺少 `--`：`{text}`")));
    };
    let (from, from_card) = split_trailing_card(left);
    let (to, to_card) = split_leading_card(right);
    // 端点名里还残留基数符号，说明这一侧的记号不完整（`A ||--|`）。
    let residue = |name: &str| name.chars().any(|c| matches!(c, '|' | '{' | '}'));
    if from.is_empty() || to.is_empty() || residue(&from) || residue(&to) {
        return Err(MermaidError::Malformed(format!(
            "关系两端必须都有实体：`{text}`"
        )));
    }
    Ok(Relation {
        from,
        to,
        from_card,
        to_card,
    })
}

/// 去掉左端实体名**末尾**的基数记号（`CUSTOMER ||` → `CUSTOMER` + `1`）。
fn split_trailing_card(side: &str) -> (String, Option<&'static str>) {
    let side = side.trim();
    for &(token, display) in CARDINALITIES {
        if let Some(name) = side.strip_suffix(token) {
            return (name.trim().to_string(), Some(display));
        }
    }
    (side.to_string(), None)
}

/// 去掉右端实体名**开头**的基数记号（`o{ ORDER` → `ORDER` + `0..*`）。
fn split_leading_card(side: &str) -> (String, Option<&'static str>) {
    let side = side.trim();
    for &(token, display) in CARDINALITIES {
        if let Some(name) = side.strip_prefix(token) {
            return (name.trim().to_string(), Some(display));
        }
    }
    (side.to_string(), None)
}

/// 关系标签：`label（1 → 0..*）`；没有标签时只写基数。
///
/// 任一端基数识别不出就整体省略基数，避免画出半截标注。
fn relation_label(
    label: Option<&str>,
    from_card: Option<&str>,
    to_card: Option<&str>,
) -> Option<String> {
    let cards = match (from_card, to_card) {
        (Some(from), Some(to)) => Some(format!("{from} → {to}")),
        _ => None,
    };
    let label = label.map(str::trim).filter(|s| !s.is_empty());
    match (label, cards) {
        (Some(label), Some(cards)) => Some(format!("{label}（{cards}）")),
        (Some(label), None) => Some(label.to_string()),
        (None, Some(cards)) => Some(cards),
        (None, None) => None,
    }
}

/// 剥掉可能有的成对引号。
fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

/// 裸实体名：不含空白、基数记号、关系符与冒号。
fn is_entity_ref(s: &str) -> bool {
    !s.is_empty()
        && !s
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '|' | '{' | '}' | '-' | ':'))
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
        layout_er(src, 14.0, &f).expect("应能布局")
    }

    #[test]
    fn entity_block_attributes_are_parsed() {
        let g = parse_er(
            "erDiagram\n\
             CUSTOMER {\n\
               string name\n\
               string id PK\n\
             }\n",
        )
        .expect("解析实体块");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.node_ids[0], "CUSTOMER");
        assert_eq!(g.nodes[0].0, "CUSTOMER<br/>string name<br/>string id PK");
        assert_eq!(g.nodes[0].1, NodeShape::Rect);
    }

    #[test]
    fn only_first_six_attributes_are_kept() {
        let mut src = String::from("erDiagram\nX {\n");
        for i in 0..8 {
            src.push_str(&format!("  attr{i}\n"));
        }
        src.push_str("}\n");
        let g = parse_er(&src).expect("解析实体块");
        let lines = split_label_lines(&g.nodes[0].0);
        assert_eq!(lines.len(), 7, "名称 + 最多 6 个属性");
    }

    #[test]
    fn relation_endpoints_and_label_are_parsed() {
        let g = parse_er("erDiagram\nCUSTOMER ||--o{ ORDER : places\n").expect("解析关系");
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.node_ids[g.edges[0].from], "CUSTOMER");
        assert_eq!(g.node_ids[g.edges[0].to], "ORDER");
        assert_eq!(
            g.edges[0].label.as_deref(),
            Some("places（1 → 0..*）"),
            "基数应附在标签后"
        );
        assert_eq!(g.edges[0].line, LineStyle::Solid);
        assert_eq!(g.edges[0].head, Head::Arrow);
    }

    #[test]
    fn all_cardinality_pairs_are_recognized() {
        for (src, want) in [
            ("A ||--o{ B : r\n", "r（1 → 0..*）"),
            ("A }o--|| B : r\n", "r（0..* → 1）"),
            ("A ||--|{ B : r\n", "r（1 → 1..*）"),
            ("A |o--o| B : r\n", "r（0..1 → 0..1）"),
        ] {
            let g = parse_er(&format!("erDiagram\n{src}")).expect("解析关系");
            assert_eq!(g.edges[0].label.as_deref(), Some(want), "{src}");
        }
    }

    #[test]
    fn relation_without_label_keeps_only_cardinality() {
        let g = parse_er("erDiagram\nA ||--|| B\n").expect("解析关系");
        assert_eq!(g.edges[0].label.as_deref(), Some("1 → 1"));
        // 未声明实体块时按裸名补节点。
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].0, "A");
        assert_eq!(g.nodes[1].0, "B");
    }

    #[test]
    fn layout_has_positive_size_and_one_box_per_entity() {
        let src = "erDiagram\n\
                   CUSTOMER ||--o{ ORDER : places\n\
                   ORDER ||--|{ LINE_ITEM : contains\n\
                   CUSTOMER {\n\
                     string name\n\
                   }\n";
        let l = lay(src);
        assert!(l.width > 0.0 && l.height > 0.0);
        let g = parse_er(src).expect("解析");
        assert_eq!(l.node_count(), g.nodes.len(), "每个实体恰好一个框");
        assert_eq!(l.node_count(), 3);
    }

    #[test]
    fn malformed_relation_is_an_error_not_a_panic() {
        let f = m();
        for src in [
            "erDiagram\nA ||--\n",
            "erDiagram\n-- B\n",
            "erDiagram\nA ||--|\n",
        ] {
            let e = layout_er(src, 14.0, &f).expect_err("应报错");
            assert!(matches!(e, MermaidError::Malformed(_)), "实际 {e:?}");
        }
    }

    #[test]
    fn empty_er_is_malformed() {
        let f = m();
        let e = layout_er("erDiagram\n", 14.0, &f).expect_err("应报错");
        assert!(matches!(e, MermaidError::Malformed(_)), "实际 {e:?}");
    }

    #[test]
    fn entity_name_without_block_is_a_node() {
        // 只有实体名、没有属性块：也应能布局，节点文本就是实体名。
        let src = "erDiagram\nCUSTOMER\n";
        let g = parse_er(src).expect("解析裸实体名");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.node_ids[0], "CUSTOMER");
        assert_eq!(g.nodes[0].0, "CUSTOMER");
        assert_eq!(g.nodes[0].1, NodeShape::Rect);
        assert_eq!(lay(src).node_count(), 1);
    }

    #[test]
    fn chinese_entity_names_are_supported() {
        // 中文实体名关系：不 panic，两端节点 id 正确。
        let src = "erDiagram\n用户 ||--o{ 订单 : 下单\n";
        let g = parse_er(src).expect("解析中文实体关系");
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.node_ids[g.edges[0].from], "用户");
        assert_eq!(g.node_ids[g.edges[0].to], "订单");
        assert_eq!(g.edges[0].label.as_deref(), Some("下单（1 → 0..*）"));
        assert_eq!(lay(src).node_count(), 2);
    }

    #[test]
    fn unterminated_entity_block_is_closed_at_eof() {
        // 缺 `}`：EOF 时按已读到的属性收尾，不得 panic、不得丢实体。
        let f = m();
        let src = "erDiagram\nX {\n  string id PK\n";
        let l = layout_er(src, 14.0, &f).expect("应能容错收尾");
        assert_eq!(l.node_count(), 1);
        let g = parse_er(src).expect("解析");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].0, "X<br/>string id PK");
    }

    #[test]
    fn later_entity_block_overrides_earlier_one() {
        // 同名实体块出现两次：只保留一个节点，且以最后一次定义为准。
        let g = parse_er(
            "erDiagram\n\
             X {\n\
               string a\n\
             }\n\
             X {\n\
               string b\n\
             }\n",
        )
        .expect("解析重复实体块");
        assert_eq!(g.nodes.len(), 1, "同名实体只应有一个节点");
        assert_eq!(g.node_ids[0], "X");
        assert_eq!(g.nodes[0].0, "X<br/>string b", "后写覆盖先写");
    }

    #[test]
    fn plain_relation_has_no_label_and_interns_both_ends() {
        // 纯 `A -- B`：没有基数记号，标签为 None，两端各补一个节点。
        let g = parse_er("erDiagram\nA -- B\n").expect("解析无基数关系");
        assert_eq!(g.edges.len(), 1);
        assert!(g.edges[0].label.is_none(), "无标签、无基数时标签应为 None");
        assert_eq!(g.node_ids[g.edges[0].from], "A");
        assert_eq!(g.node_ids[g.edges[0].to], "B");
        assert_eq!(g.nodes.len(), 2, "两端应各补一个节点");
    }

    #[test]
    fn attributes_beyond_six_are_dropped() {
        // 属性超过 6 行：截断到 6 行，第 7 个属性不进入标签。
        let mut src = String::from("erDiagram\nX {\n");
        for i in 0..7 {
            src.push_str(&format!("  attr{i}\n"));
        }
        src.push_str("}\n");
        let g = parse_er(&src).expect("解析实体块");
        let lines = split_label_lines(&g.nodes[0].0);
        assert_eq!(lines.len(), 7, "名称 + 最多 6 个属性");
        assert_eq!(lines[6], "attr5", "第 6 个属性应是最后一行");
        assert!(!lines.iter().any(|l| l == "attr6"), "超出的属性应被丢弃");
    }
}
