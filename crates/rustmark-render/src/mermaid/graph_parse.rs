//! `graph` / `flowchart`「节点 + 有向边」子集的解析器。

use super::model::{Edge, Graph, Head, LineStyle, NodeShape};
use super::*;

/// 解析 Mermaid 源码为图模型。
pub(super) fn parse_graph(src: &str) -> Result<Graph, MermaidError> {
    let header = src
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .ok_or_else(|| MermaidError::Malformed("图表内容为空".to_string()))?;
    // 头部形如 `graph LR` / `flowchart TD;`；`;` 之后可能还带着第一条语句。
    let header = header.trim_end_matches(';').trim();
    let mut words = header.splitn(2, char::is_whitespace);
    let kind = words.next().unwrap_or_default().to_ascii_lowercase();
    if kind != "graph" && kind != "flowchart" {
        return Err(MermaidError::Unsupported(kind));
    }
    let rest = words.next().unwrap_or("");
    let mut tail_iter = rest.splitn(2, |c: char| c.is_whitespace() || c == ';');
    let dir = tail_iter.next().unwrap_or("TB").trim().to_ascii_uppercase();
    let tail = tail_iter
        .next()
        .unwrap_or("")
        .trim_start_matches([';', ' ']);
    let (horizontal, reversed) = match dir.as_str() {
        "" | "TB" | "TD" => (false, false),
        "LR" => (true, false),
        "RL" => (true, true),
        "BT" => (false, true),
        other => return Err(MermaidError::Unsupported(format!("方向 `{other}`"))),
    };

    let mut g = Graph {
        horizontal,
        reversed,
        nodes: Vec::new(),
        node_ids: Vec::new(),
        edges: Vec::new(),
    };
    if !tail.is_empty() {
        parse_line(tail, &mut g)?;
    }
    // 头部那一行之后的其余行。
    let mut seen_header = false;
    for line in src.lines().map(str::trim) {
        if !seen_header {
            if line.is_empty() || line.starts_with("%%") {
                continue;
            }
            seen_header = true;
            continue;
        }
        parse_line(line, &mut g)?;
    }
    Ok(g)
}

/// 解析一行（可能含多条语句，用 `;` 分隔）。
fn parse_line(line: &str, g: &mut Graph) -> Result<(), MermaidError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("%%") {
        return Ok(());
    }
    for stmt in line.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        parse_statement(stmt, g)?;
    }
    Ok(())
}

/// 取语句开头的标识符（用于识别指令关键字）。
pub(super) fn leading_word(stmt: &str) -> String {
    stmt.chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// 纯装饰指令：忽略。
fn is_cosmetic(stmt: &str) -> bool {
    matches!(
        leading_word(stmt).as_str(),
        "style"
            | "classDef"
            | "class"
            | "linkStyle"
            | "click"
            | "direction"
            | "accTitle"
            | "accDescr"
    )
}

/// 结构上不支持：宁可整体回退，也不要画出半张图。
fn unsupported_kind(stmt: &str) -> Option<String> {
    let head = leading_word(stmt);
    if matches!(head.as_str(), "subgraph" | "end") {
        return Some(head);
    }
    // `sequenceDiagram` / `classDiagram` / `gantt` 已由 `layout` 分发处理，
    // 不再在此拦截；其余图种（pie / journey / …）仍整体回退。
    if matches!(
        head.as_str(),
        "pie"
            | "journey"
            | "gitGraph"
            | "mindmap"
            | "timeline"
            | "quadrantChart"
            | "xychart"
            | "sankey"
            | "requirement"
    ) {
        return Some(head);
    }
    None
}

/// 解析一条语句：`节点 (边 节点)*`。
fn parse_statement(stmt: &str, g: &mut Graph) -> Result<(), MermaidError> {
    if is_cosmetic(stmt) {
        return Ok(());
    }
    if let Some(what) = unsupported_kind(stmt) {
        return Err(MermaidError::Unsupported(what));
    }
    let chars: Vec<char> = stmt.chars().collect();
    let mut i = 0usize;
    let mut prev = parse_node(&chars, &mut i, g)?;
    loop {
        skip_ws(&chars, &mut i);
        if i >= chars.len() {
            break;
        }
        let edge = parse_edge(&chars, i)?;
        i += edge.consumed;
        skip_ws(&chars, &mut i);
        let next = parse_node(&chars, &mut i, g)?;
        g.edges.push(Edge {
            from: prev,
            to: next,
            label: edge.label,
            line: edge.line,
            head: edge.head,
            bidirectional: edge.bidirectional,
        });
        prev = next;
    }
    Ok(())
}

/// 读一个端点前的空白跳过（`flowchart.js` 解析器同样复用）。
pub(super) fn skip_ws(c: &[char], i: &mut usize) {
    while *i < c.len() && c[*i].is_whitespace() {
        *i += 1;
    }
}

/// 读一个节点（id + 可选的形状与文本），返回它在 `g.nodes` 里的下标。
///
/// 同一 id 重复出现时复用同一下标，但**后写的形状与文本会覆盖先写的**——
/// 与 Mermaid 的行为一致（`A[旧]` 之后写 `A[新]`，显示新的）。
fn parse_node(c: &[char], i: &mut usize, g: &mut Graph) -> Result<usize, MermaidError> {
    skip_ws(c, i);
    let start = *i;
    while *i < c.len() {
        let ch = c[*i];
        if ch.is_alphanumeric() || ch == '_' {
            *i += 1;
            continue;
        }
        // 连字符算 id 的一部分（`my-node`），但若它**引出一条边**就必须停。
        //
        // `A-->B` 是最常见的写法：不设这道闸，id 会被读成 `A--`，
        // 随后 `>` 被当成「形状开始」，报出「形状没有闭合」这种
        // 与真实原因八竿子打不着的错——实测就是这么误导的。
        if ch == '-' {
            let next = c.get(*i + 1).copied();
            if matches!(
                next,
                Some('-') | Some('>') | Some('.') | Some('x') | Some('o')
            ) {
                break;
            }
            *i += 1;
            continue;
        }
        break;
    }
    if start == *i {
        return Err(MermaidError::Malformed(format!(
            "期望节点名，实际遇到 `{}`",
            c.get(*i).copied().unwrap_or(' ')
        )));
    }
    let id: String = c[start..*i].iter().collect();

    // 形状：**先长后短**，否则 `[[` 会被 `[` 抢走、`(("` 会被 `(` 抢走。
    let rest = &c[*i..];
    let (shape, open, close) = if starts_with(rest, "[[") {
        (NodeShape::Subroutine, "[[", "]]")
    } else if starts_with(rest, "((") {
        (NodeShape::Circle, "((", "))")
    } else if starts_with(rest, "([") {
        (NodeShape::Stadium, "([", "])")
    } else if starts_with(rest, "[(") {
        (NodeShape::Cylinder, "[(", ")]")
    } else if starts_with(rest, "{{") {
        (NodeShape::Hexagon, "{{", "}}")
    } else if starts_with(rest, "[/") {
        (NodeShape::Parallelogram, "[/", "/]")
    } else if starts_with(rest, "[\\") {
        (NodeShape::Parallelogram, "[\\", "\\]")
    } else if starts_with(rest, "[") {
        (NodeShape::Rect, "[", "]")
    } else if starts_with(rest, "(") {
        (NodeShape::Rounded, "(", ")")
    } else if starts_with(rest, "{") {
        (NodeShape::Rhombus, "{", "}")
    } else if starts_with(rest, ">") {
        (NodeShape::Asymmetric, ">", "]")
    } else {
        // 裸 id：默认矩形，文本就是 id。
        return Ok(intern(g, &id, None, NodeShape::Rect));
    };

    *i += open.chars().count();
    let text_start = *i;
    while *i < c.len() && !starts_with(&c[*i..], close) {
        *i += 1;
    }
    if *i >= c.len() {
        return Err(MermaidError::Malformed(format!(
            "节点 `{id}` 的形状 `{open}` 没有用 `{close}` 闭合"
        )));
    }
    let raw: String = c[text_start..*i].iter().collect();
    *i += close.chars().count();
    // Mermaid 允许 `A["文本"]`；剥掉成对的引号。
    let text = raw.trim().trim_matches('"').trim().to_string();
    let text = if text.is_empty() { id.clone() } else { text };
    Ok(intern(g, &id, Some(text), shape))
}

fn starts_with(c: &[char], pat: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    c.len() >= p.len() && c[..p.len()] == p[..]
}

/// 按 id 取（或新建）节点下标。
///
/// # 查重必须比 **id**，不能比显示文本
///
/// 这里曾经写成 `g.nodes.iter().position(|(n, _)| n == id)`——`n` 是节点的
/// **显示文本**而不是 id。于是「带标签的节点被再次以 id 引用」时永远找不到旧节点：
///
/// ```text
/// graph LR
///   A[长方形] -- 链接 --> B((圆))
///   A --> C(圆角长方形)
///   B --> D{菱形}
///   C --> D
/// ```
///
/// 实测解析结果是 **8 个节点**（应为 4 个）：A/长方形、B/圆 之后，`A` 与 `B`
/// 各又新建了一个以 id 为文本的空壳节点，`C`、`D` 同理重复；图面上于是多出
/// 四个写着 `A`/`B`/`C`/`D` 的框，连线也被引到重复节点上。
/// 这是**最常规**的流程图写法（先定义带标签的节点，再按 id 串联），
/// 所以看上去像「流程图整体渲染失效」。
///
/// 之所以此前没被发现：`graph LR / A --> B` 这种最简用例里，
/// 节点文本恰好等于 id，文本比对「碰巧」是对的。
pub(super) fn intern(g: &mut Graph, id: &str, text: Option<String>, shape: NodeShape) -> usize {
    if let Some(pos) = g.node_ids.iter().position(|n| n == id) {
        // 重复出现时**后写的形状与文本覆盖先写的**（与 Mermaid 一致）；
        // 只写 id（`text == None`）则保留原有标签，不被 id 顶掉。
        if let Some(text) = text {
            g.nodes[pos] = (text, shape);
        }
        return pos;
    }
    g.node_ids.push(id.to_string());
    g.nodes
        .push((text.unwrap_or_else(|| id.to_string()), shape));
    g.nodes.len() - 1
}

/// 一条边的解析结果。
struct ParsedEdge {
    line: LineStyle,
    head: Head,
    label: Option<String>,
    bidirectional: bool,
    consumed: usize,
}

/// 读一条边。
///
/// # 为什么用「扫描找收尾符」而不是穷举全部写法
///
/// Mermaid 的边语法有两套并存：标签**夹在**中间（`A -- 文案 --> B`）与
/// 标签写在**管道里**（`A -->|文案| B`）。前者意味着「标签」与「收尾符」
/// 之间没有固定分隔，穷举要写 `--` × `==` × `-.` × 收尾符 × 有无标签的组合。
/// 改为：先定线型（看开头两字符），再向后扫描找**第一个收尾符**，
/// 中间的部分就是标签。一套逻辑覆盖全部组合。
fn parse_edge(c: &[char], start: usize) -> Result<ParsedEdge, MermaidError> {
    let mut i = start;
    // 双向：`A <--> B`。起点侧多一个 `<`。
    let bidirectional = c[i] == '<';
    if bidirectional {
        i += 1;
    }
    let line = if starts_with(&c[i..], "==") {
        LineStyle::Thick
    } else if starts_with(&c[i..], "-.") {
        LineStyle::Dotted
    } else if starts_with(&c[i..], "-") {
        // `--` 与单个 `-` 同归实线：前者是 mermaid 的标准写法，后者是
        // `A - B` 这类宽松输入的容错。两者后面的步进完全一致，故合并。
        LineStyle::Solid
    } else {
        return Err(MermaidError::Malformed(format!(
            "期望边（`-->` 之类），实际遇到 `{}`",
            c[i]
        )));
    };
    i += 2;
    let label_start = i;
    let mut label_end = i;
    let mut found: Option<(Head, usize)> = None;
    while i < c.len() {
        if let Some((head, len)) = match_closer(&c[i..]) {
            found = Some((head, len));
            break;
        }
        i += 1;
        label_end = i;
    }
    let Some((head, len)) = found else {
        return Err(MermaidError::Malformed(
            "边的收尾符缺失（应为 `>` / `-` / `x` / `o`）".to_string(),
        ));
    };
    let mut consumed = i + len;
    let raw: String = c[label_start..label_end].iter().collect();
    let mut label = raw.trim().to_string();
    // `A -->|文案| B`：管道里的标签排在收尾符**之后**，且优先级更高。
    if c.get(consumed) == Some(&'|') {
        let body_start = consumed + 1;
        let mut j = body_start;
        while j < c.len() && c[j] != '|' {
            j += 1;
        }
        if j < c.len() {
            let piped: String = c[body_start..j].iter().collect();
            if !piped.trim().is_empty() {
                label = piped.trim().to_string();
            }
            consumed = j + 1;
        }
    }
    Ok(ParsedEdge {
        line,
        head,
        label: (!label.is_empty()).then_some(label),
        bidirectional,
        consumed: consumed - start,
    })
}

/// 在当前前缀匹配一个收尾符（**先长后短**，否则 `-->` 会被 `-` 抢走）。
fn match_closer(c: &[char]) -> Option<(Head, usize)> {
    const TABLE: &[(&str, Head)] = &[
        ("-->", Head::Arrow),
        ("---", Head::None),
        ("--x", Head::Cross),
        ("--o", Head::Circle),
        ("==>", Head::Arrow),
        ("===", Head::None),
        (".->", Head::Arrow),
        (".-", Head::None),
        ("->", Head::Arrow),
        (">", Head::Arrow),
        ("x", Head::Cross),
        ("o", Head::Circle),
        ("-", Head::None),
        ("=", Head::None),
    ];
    TABLE
        .iter()
        .find(|(pat, _)| starts_with(c, pat))
        .map(|(pat, head)| (*head, pat.chars().count()))
}
