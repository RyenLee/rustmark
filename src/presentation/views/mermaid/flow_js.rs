//! flowchart.js 语法的解析与布局（`示例文档.md` 的「FLowchart流程图」一节）。
//!
//! # 背景：为什么这一节此前整体降级成源码卡
//!
//! 流行甚广的 editor.md 示例文档里有一节 flowchart.js 语法
//! （`id=>type: 文本` 定义行 + `->` 连线行），而且首行带着那处著名的
//! 笔误 **`flowchat`**（少个 `r`）。内置引擎原先只认 Mermaid 语法，
//! `detect_kind` 把 `flowchat` 当图种关键字 → 不在支持列表 →
//! `Unsupported("flowchat")` → 整卡回退。用户看到「不支持的图表写法：
//! flowchat」，与「支持列表里的 flowchart」只差一个字母，极具误导性——
//! 看上去像是引擎连 flowchart 都不认。
//!
//! 即使把笔误改对也无济于事：正文是 flowchart.js 语法（`=>` 定义、`->`
//! 连线），Mermaid 解析器会在 `=` 处报「期望边」。**语法家族不同**才是根因。
//!
//! # 方案：映射到同一个 `Graph` 模型
//!
//! flowchart.js 的「定义 + 连线」可以无损映射到与 Mermaid 流程图相同的
//! [`Graph`]（`start`/`end` → 胶囊形、`condition` → 菱形、`operation` →
//! 矩形、`inputoutput` → 平行四边形、`subroutine` → 子程序形），于是
//! 分层布局与绘制全部复用 [`layout_from_graph`]，新增的只有解析。
//!
//! # 识别与分发
//!
//! [`is_flowchart_js`] 只认 flowchart.js **独有**的「行首 `id=>type` 定义行」；
//! Mermaid 流程图的任何合法行都不以 `标识=>` 开头（节点文本里的 `=>` 前面
//! 必然带 `[` 等形状字符），故两套语法互不误伤。首行的 `flowchart` /
//! `flowchat`（笔误）/ `flow` 标题一律当作可忽略的图种行——flowchart.js
//! 没有方向参数，恒为自上而下。
//!
//! # 范围
//!
//! 支持：全部七种节点类型、链式连线 `a->b->c`、来源侧括号标签
//! `cond(yes)->e`（落到它引出的边上）、`-->` 的宽容写法、`%%` 注释与空行。
//! 不支持：flowchart.js 的 `subgraph`（整卡回退，与 Mermaid 路径同一策略）。

use super::graph_layout::layout_from_graph;
use super::graph_parse::{intern, leading_word, skip_ws};
use super::model::{Edge, Graph, Head, LineStyle, NodeShape};
use super::*;

/// 是否为 flowchart.js 语法：存在「`id=>type`」定义行。
///
/// 这是 flowchart.js **独有**的签名：`st=>start: 开始`。Mermaid 流程图
/// 的合法行没有「行首裸标识 + `=>`」的形态（节点文本里的 `=>` 前面必然
/// 是 `[` / `(` 等形状字符，`id` 段检查会拒绝），不会误伤。
pub(super) fn is_flowchart_js(src: &str) -> bool {
    src.lines().map(str::trim).any(is_definition)
}

/// `id=>type: 文本` 定义行的形态检查。
fn is_definition(line: &str) -> bool {
    match line.split_once("=>") {
        Some((id, _)) => {
            !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// flowchart.js 源码 → 版面。
pub(super) fn layout(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let g = parse(src)?;
    if g.nodes.is_empty() {
        return Err(MermaidError::Malformed(
            "图里没有任何节点（只有 `flowchart` 标题行？）".to_string(),
        ));
    }
    layout_from_graph(g, src, size, measure)
}

/// 解析 flowchart.js 源码为图模型。
pub(super) fn parse(src: &str) -> Result<Graph, MermaidError> {
    let mut g = Graph {
        // flowchart.js 没有方向参数，恒为自上而下。
        horizontal: false,
        reversed: false,
        nodes: Vec::new(),
        node_ids: Vec::new(),
        edges: Vec::new(),
    };
    let mut seen_header = false;
    for line in src.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        // 首个非空行若是图种标题（`flowchart` / 笔误 `flowchat` / `flow`）
        // 则忽略。带 `=>` 的行不可能是标题（如 `flow=>operation: …` 这种
        // 以 id 为 "flow" 的定义），不能跳过。
        if !seen_header {
            seen_header = true;
            let head = leading_word(line).to_ascii_lowercase();
            if !line.contains("=>") && matches!(head.as_str(), "flowchart" | "flowchat" | "flow") {
                continue;
            }
        }
        if is_definition(line) {
            parse_definition(line, &mut g)?;
        } else {
            parse_edge_line(line, &mut g)?;
        }
    }
    Ok(g)
}

/// 把 `id=>type: 文本` 定义落成节点。
///
/// 同一 id 重复定义时后写的覆盖先写的（`intern` 的既有约定，
/// 与 Mermaid `A[旧]` 后 `A[新]` 显示新的行为一致）。
fn parse_definition(line: &str, g: &mut Graph) -> Result<(), MermaidError> {
    let (id, rest) = match line.split_once("=>") {
        Some((id, rest)) => (id, rest.trim_start()),
        None => {
            return Err(MermaidError::Malformed(
                "定义行应写成 `id=>type: 文本`".to_string(),
            ));
        }
    };
    // 冒号容错：ASCII `:` 与全角 `：` 都接受。
    let (type_word, text) = match rest.split_once([':', '：']) {
        Some((t, text)) => (t.trim(), Some(text.trim())),
        None => (rest, None),
    };
    let text = text.filter(|t| !t.is_empty()).unwrap_or(id);
    intern(g, id, Some(text.to_string()), flow_shape(type_word));
    Ok(())
}

/// flowchart.js 节点类型 → 外形。未认识的类型按矩形处理（宽松）。
fn flow_shape(kind: &str) -> NodeShape {
    match kind.to_ascii_lowercase().as_str() {
        "start" | "end" => NodeShape::Stadium,
        "condition" => NodeShape::Rhombus,
        "subroutine" => NodeShape::Subroutine,
        "inputoutput" => NodeShape::Parallelogram,
        // operation / parallel / 未认识类型：矩形。
        _ => NodeShape::Rect,
    }
}

/// 解析连线行：`st->op->cond`、`cond(yes)->e`、`cond(no) -> op`。
///
/// 括号标签写在**来源**引用上（flowchart.js 的约定），落到它引出的那条边上；
/// 链式写法里中间节点的标签留给它自己的出边。箭头容许一个以上的 `-`
/// （`->` 与 `-->` 同收）。
fn parse_edge_line(line: &str, g: &mut Graph) -> Result<(), MermaidError> {
    let c: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    let mut prev = read_endpoint(&c, &mut i, g)?;
    loop {
        skip_ws(&c, &mut i);
        let mut dashes = 0usize;
        while c.get(i) == Some(&'-') {
            i += 1;
            dashes += 1;
        }
        if dashes == 0 || c.get(i) != Some(&'>') {
            return Err(MermaidError::Malformed(format!(
                "连线应写成 `a->b`，这一行读不通：`{line}`"
            )));
        }
        i += 1;
        skip_ws(&c, &mut i);
        let next = read_endpoint(&c, &mut i, g)?;
        g.edges.push(Edge {
            from: prev.0,
            to: next.0,
            label: prev.1,
            line: LineStyle::Solid,
            head: Head::Arrow,
            bidirectional: false,
        });
        // `next` 的括号标签是它**自己**出边的，随节点下标一起传递。
        prev = next;
        skip_ws(&c, &mut i);
        if i >= c.len() {
            return Ok(());
        }
    }
}

/// 读一个端点：`id` 或 `id(标签)`。返回（节点下标，括号标签）。
///
/// 只引用未定义的 id 时按 Mermaid 的宽容约定先建一个裸节点。
fn read_endpoint(
    c: &[char],
    i: &mut usize,
    g: &mut Graph,
) -> Result<(usize, Option<String>), MermaidError> {
    let start = *i;
    while c
        .get(*i)
        .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
    {
        *i += 1;
    }
    if start == *i {
        return Err(MermaidError::Malformed(format!(
            "期望节点名，实际遇到 `{}`",
            c.get(*i).copied().unwrap_or(' ')
        )));
    }
    let id: String = c[start..*i].iter().collect();
    skip_ws(c, i);
    let label = if c.get(*i) == Some(&'(') {
        *i += 1;
        let s = *i;
        while c.get(*i).is_some_and(|ch| *ch != ')') {
            *i += 1;
        }
        if c.get(*i).is_none() {
            return Err(MermaidError::Malformed(format!(
                "节点 `{id}` 的标签括号没有闭合"
            )));
        }
        let label: String = c[s..*i].iter().collect();
        *i += 1; // ')'
        let label = label.trim();
        (!label.is_empty()).then(|| label.to_string())
    } else {
        None
    };
    Ok((intern(g, &id, None, NodeShape::Rect), label))
}
