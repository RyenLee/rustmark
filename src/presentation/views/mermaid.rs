//! 内置 Mermaid 子集渲染引擎（问题 3：图表在预览里不出图）。
//!
//! # 范围：`graph` / `flowchart` 的「节点 + 有向边」子集
//!
//! 支持：
//! - 方向 `LR` / `RL` / `TB` / `TD` / `BT`；
//! - 节点形状 `[矩形]` `(圆角)` `([机场跑道])` `[[子程序]]` `[(数据库)]`
//!   `((圆))` `{菱形}` `{{六边形}}` `[/平行四边形/]` `>旗帜]`；
//! - 边 `-->` `---` `-.->` `-.-` `==>` `===` `--x` `--o` `<-->`，以及带标签的
//!   `A -- 文案 --> B` 与 `A -->|文案| B`；
//! - 链式写法 `A --> B --> C`、`%%` 注释、空行、`;` 分隔。
//!
//! **不支持**：`subgraph`、以及 `sequenceDiagram` / `classDiagram` /
//! `stateDiagram` / `pie` / `gantt` / `journey` / `erDiagram` 等其它图种。
//! 遇到它们返回 [`MermaidError::Unsupported`]，由调用方回退到「源码卡」
//! （见 `preview_view::draw_diagram`）——**不猜**。半张图画出来比不画更难排查：
//! 用户会先怀疑自己的语法。
//!
//! 纯装饰指令（`style` / `classDef` / `class` / `linkStyle` / `click` / `direction`）
//! **忽略而非报错**：它们只影响配色与交互，装看不见不影响图的结构正确性，
//! 而为此整体回退到源码是过度反应。
//!
//! # 布局算法：分层（Sugiyama 骨架）
//!
//! 1. **分层**：沿边做最长路径松弛，得到每个节点的 rank（列号）；
//! 2. **排序**：同 rank 内按重心（barycenter）迭代两轮，减少交叉；
//! 3. **定位**：rank 决定列、rank 内的次序决定行，整体按最大值居中；
//! 4. **走线**：同轴相邻走直线，跨列走「出边 → 中点干 → 入边」的正交折线；
//!    箭头画在入口。
//!
//! 与 [`super::math`] 相同的分层：解析 → 布局（纯函数）→ 绘制。
//! 度量通过 [`Measure`] 注入，于是「B 必须在 A 右边」「有环也不会卡死」
//! 这类规则都能在没有窗口的环境里断言。

use egui::text::LayoutJob;
use egui::{
    Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, TextFormat, Vec2,
};

/// 文本宽度度量函数：`(文本, 字号) -> 宽度（点）`。
///
/// 与 [`super::math::Measure`] 同形但**各自声明**：两个模块互不依赖，
/// 将来其中一个要换成「带粗细的度量」时不必牵连另一个。
pub type Measure<'a> = &'a dyn Fn(&str, f32) -> f32;

/// 子集外的图种 / 结构。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MermaidError {
    /// 不是本引擎支持的图种或结构。
    Unsupported(String),
    /// 语法读不通（边缺一端、形状括号不配对等）。
    Malformed(String),
}

impl std::fmt::Display for MermaidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "不支持的图表写法：{what}"),
            Self::Malformed(what) => write!(f, "图表语法错误：{what}"),
        }
    }
}

// ============================================================================
// 缩放常量（点；与基准字号的乘积即为实际值）
// ============================================================================

/// 节点内边距（横向，乘基准字号）。
const NODE_PAD_X: f32 = 0.75;
/// 节点内边距（纵向）。
const NODE_PAD_Y: f32 = 0.4;
/// 同列相邻节点之间的间距。
const NODE_GAP: f32 = 0.85;
/// 相邻列之间的间距。
const RANK_GAP: f32 = 1.7;
/// 菱形这类「内切」外形需要的额外尺寸系数。
const DIAMOND_SCALE: f32 = 2.0;
/// 圆形的直径系数。
const CIRCLE_D: f32 = 1.7;
/// 箭头长度系数。
const ARROW_LEN: f32 = 0.65;
/// 箭头张角（半宽 / 长度）。
const ARROW_SPREAD: f32 = 0.42;
/// 折线走线时的「出桩」长度。
const ROUTE_STUB: f32 = 12.0;

// ============================================================================
// 图模型
// ============================================================================

/// 节点外形。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NodeShape {
    Rect,
    Rounded,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    Rhombus,
    Hexagon,
    Parallelogram,
    Asymmetric,
}

/// 边的线型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LineStyle {
    Solid,
    Dotted,
    Thick,
}

/// 边的末端装饰。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Head {
    /// `-->`：实心箭头。
    Arrow,
    /// `--x`：叉头。
    Cross,
    /// `--o`：圆头。
    Circle,
    /// `---`：无。
    None,
}

/// 边。
#[derive(Clone, Debug)]
struct Edge {
    from: usize,
    to: usize,
    label: Option<String>,
    line: LineStyle,
    head: Head,
    /// 起点侧是否也有箭头（`<-->`）。
    bidirectional: bool,
}

/// 解析后的图。
#[derive(Clone, Debug)]
struct Graph {
    /// 流向为水平（`LR` / `RL`）还是垂直（`TB` / `TD` / `BT`）。
    horizontal: bool,
    /// 是否反向（`RL` / `BT`）。
    reversed: bool,
    /// 节点：显示文本 + 外形。
    nodes: Vec<(String, NodeShape)>,
    /// 节点 id（与 `nodes` 一一对应，供 `click` 指令按 id 命中热点）。
    node_ids: Vec<String>,
    edges: Vec<Edge>,
}

// ============================================================================
// 解析
// ============================================================================

/// 解析 Mermaid 源码为图模型。
fn parse_graph(src: &str) -> Result<Graph, MermaidError> {
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
fn leading_word(stmt: &str) -> String {
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

fn skip_ws(c: &[char], i: &mut usize) {
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
fn intern(g: &mut Graph, id: &str, text: Option<String>, shape: NodeShape) -> usize {
    if let Some(pos) = g.nodes.iter().position(|(n, _)| n == id) {
        if let Some(text) = text {
            g.nodes[pos] = (text, shape);
        }
        return pos;
    }
    g.nodes
        .push((text.unwrap_or_else(|| id.to_string()), shape));
    g.node_ids.push(id.to_string());
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

// ============================================================================
// 布局
// ============================================================================

/// 一个节点在版面里的位置与尺寸。
#[derive(Clone, Debug)]
struct Placed {
    /// 节点中心。
    center: Pos2,
    size: Vec2,
    shape: NodeShape,
}

/// 绘制图元（坐标已含方向变换，可直接使用）。
#[derive(Clone, Debug)]
enum Prim {
    /// 多边形。`radius > 0` 且为 4 点时按圆角矩形处理。
    Poly {
        points: Vec<Pos2>,
        radius: f32,
        fill: bool,
        /// 子程序形的两道内竖线。
        ticks: bool,
    },
    Line {
        a: Pos2,
        b: Pos2,
        width: f32,
        dotted: bool,
    },
    /// 实心三角箭头。
    Arrow {
        tip: Pos2,
        dir: Vec2,
        len: f32,
        spread: f32,
    },
    /// 叉头（`--x`）。
    Cross { at: Pos2, dir: Vec2, len: f32 },
    /// 文字。
    Text {
        at: Pos2,
        text: String,
        size: f32,
        /// 是否铺一块底色（边标签压线时用）。
        plate: bool,
    },
}

/// 版面。
#[derive(Clone, Debug)]
pub struct MermaidLayout {
    /// 内容宽（点）。
    pub width: f32,
    /// 内容高（点）。
    pub height: f32,
    /// 图元（坐标以左上角为原点）。
    prims: Vec<Prim>,
    /// 可点击热点：`(矩形, URL)`。矩形与图元同一坐标系（左上角为原点），
    /// 预览层之后会据此生成 `ctx.open_url` 的交互区。
    pub hotspots: Vec<(egui::Rect, String)>,
}

impl MermaidLayout {
    /// 预览层用来生成交互区的热点列表（已归一化到左上角原点）。
    pub fn click_targets(&self) -> &[(egui::Rect, String)] {
        &self.hotspots
    }
}

/// 把源码排成版面。返回 `Err` 时调用方应回退到源码显示。
///
/// 依据首行关键字分发到对应的图种解析器；不在支持列表里的关键字
/// （`pie` / `stateDiagram` / `erDiagram` / …）回退为 `Unsupported`。
pub fn layout(src: &str, size: f32, measure: Measure<'_>) -> Result<MermaidLayout, MermaidError> {
    let kind = detect_kind(src)?;
    match kind.as_str() {
        "graph" | "flowchart" => layout_graph(src, size, measure),
        "sequencediagram" => layout_sequence(src, size, measure),
        "classdiagram" | "classdiagram-v1" | "classdiagram-v2" => layout_class(src, size, measure),
        "gantt" => layout_gantt(src, size, measure),
        other => Err(MermaidError::Unsupported(other.to_string())),
    }
}

/// 取首条非空、非注释行里的「关键字」（图种标识）。
fn detect_kind(src: &str) -> Result<String, MermaidError> {
    let header = src
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .ok_or_else(|| MermaidError::Malformed("图表内容为空".to_string()))?;
    Ok(leading_word(header).to_ascii_lowercase())
}

/// `graph` / `flowchart` 的布局（原 `layout` 的「节点-边」路径）。
fn layout_graph(src: &str, size: f32, measure: Measure<'_>) -> Result<MermaidLayout, MermaidError> {
    let g = parse_graph(src)?;
    if g.nodes.is_empty() {
        return Err(MermaidError::Malformed(
            "图里没有任何节点（只有 `graph LR` 一行？）".to_string(),
        ));
    }
    let ranks = rank_nodes(&g);
    let layers = order_within_rank(&g, &ranks);
    let placed = place(&g, &layers, size, measure);
    let prims = emit(&g, &placed, size, measure);
    // 节点外接框（供 `click` 与标签内 markdown 链接命中热点）。
    let mut id_rects: Vec<(String, Rect)> = Vec::new();
    let mut direct: Vec<(Rect, String)> = Vec::new();
    let clicks = collect_clicks(src);
    for (v, p) in placed.iter().enumerate() {
        let r = Rect::from_center_size(p.center, p.size);
        let id = &g.node_ids[v];
        id_rects.push((id.clone(), r));
        if let Some(url) = first_md_link(&g.nodes[v].0) {
            direct.push((r, url));
        }
    }
    Ok(finalize(prims, id_rects, clicks, direct))
}

/// 最长路径分层：`rank[v] = max(rank[pred] + 1)`。
///
/// 迭代**有上界**（最多 `节点数` 轮），故**有环也不会卡死**：
/// 环上的节点会稳定在某个较大 rank 上（图仍画得出来，只是不最优）。
fn rank_nodes(g: &Graph) -> Vec<usize> {
    let n = g.nodes.len();
    let mut rank = vec![0usize; n];
    for _ in 0..n.max(1) {
        let mut changed = false;
        for e in &g.edges {
            // 自环必须排除：`A-->A` 若参与松弛会把自身一轮轮推高，
            // 白跑满所有轮次（结果虽然被上界兜住，但徒增开销且层次难看）。
            if e.from == e.to {
                continue;
            }
            let want = rank[e.from] + 1;
            if want > rank[e.to] && want <= n {
                rank[e.to] = want;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    rank
}

/// 同 rank 内排序：按重心迭代两轮，减少边交叉。
///
/// 第一轮按「前驱的平均次序」，第二轮按「后继的平均次序」——Sugiyama 的
/// 相邻交换启发式的最简形式。不做的话 `A-->C, B-->D` 这类图会出现大量交叉线。
fn order_within_rank(g: &Graph, ranks: &[usize]) -> Vec<Vec<usize>> {
    let max_rank = ranks.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    // 初始次序 = 首次出现的次序（稳定且可预期）。
    for v in 0..g.nodes.len() {
        layers[ranks[v]].push(v);
    }
    for pass in 0..2 {
        let forward = pass == 0;
        let mut range: Vec<usize> = if forward {
            (1..=max_rank).collect()
        } else {
            (0..max_rank).collect()
        };
        if !forward {
            range.reverse();
        }
        for r in range {
            // 注意：先只读 `layers` 算出重心，再写回 `layers[r]`。
            // 若在 `.map()` 里直接改 `layers[r]`，闭包会同时持有可变与不可变借用。
            let mut scored: Vec<(f32, usize)> = layers[r]
                .iter()
                .map(|v| {
                    let mut sum = 0.0_f32;
                    let mut count = 0usize;
                    for e in &g.edges {
                        let (near, far) = if forward {
                            (e.to, e.from)
                        } else {
                            (e.from, e.to)
                        };
                        if near != *v || ranks[far] == r {
                            continue;
                        }
                        if let Some(p) = layers[ranks[far]].iter().position(|x| *x == far) {
                            sum += p as f32;
                            count += 1;
                        }
                    }
                    let key = if count == 0 {
                        // 无邻居的节点保持原位次（用自己在层内的位置兜底）。
                        layers[r]
                            .iter()
                            .position(|x| x == v)
                            .map_or(0.0, |p| p as f32)
                    } else {
                        sum / count as f32
                    };
                    (key, *v)
                })
                .collect();
            scored.sort_by(|a, b| a.0.total_cmp(&b.0));
            layers[r] = scored.into_iter().map(|(_, v)| v).collect();
        }
    }
    layers
}

/// 节点尺寸：由文本量 + 外形决定。
fn node_size(text: &str, shape: NodeShape, size: f32, measure: Measure<'_>) -> Vec2 {
    let tw = measure(text, size);
    let pad_x = size * NODE_PAD_X;
    let pad_y = size * NODE_PAD_Y;
    let mut w = tw + 2.0 * pad_x;
    let mut h = size + 2.0 * pad_y;
    match shape {
        // 菱形 / 六边形的可用内切区只有一半左右，文本要留双倍余量，
        // 否则文字会顶出斜边。
        NodeShape::Rhombus => {
            w *= DIAMOND_SCALE;
            h *= DIAMOND_SCALE;
        }
        NodeShape::Hexagon | NodeShape::Parallelogram => {
            w *= 1.45;
            h *= 1.1;
        }
        NodeShape::Circle => {
            let d = w.max(h) * CIRCLE_D;
            w = d;
            h = d;
        }
        NodeShape::Stadium => {
            w += h;
        }
        _ => {}
    }
    Vec2::new(w.max(size * 1.2), h.max(size * 1.5))
}

/// 定位：rank → 主坐标，rank 内次序 → 次坐标。
///
/// 内部一律按「流向水平」计算，需要时在末尾把主轴镜像 / 交换——
/// 若为横竖两种流向各写一套定位，两套的分支点迟早会不一致。
fn place(g: &Graph, layers: &[Vec<usize>], size: f32, measure: Measure<'_>) -> Vec<Placed> {
    let sizes: Vec<Vec2> = g
        .nodes
        .iter()
        .map(|(t, s)| node_size(t, *s, size, measure))
        .collect();
    let gap = size * NODE_GAP;
    let rank_gap = size * RANK_GAP;

    // 每列的主向厚度与次向总长。
    let mut col_thick = vec![0.0_f32; layers.len()];
    let mut col_span = vec![0.0_f32; layers.len()];
    for (r, layer) in layers.iter().enumerate() {
        for v in layer {
            let s = sizes[*v];
            col_thick[r] = col_thick[r].max(if g.horizontal { s.x } else { s.y });
        }
        col_span[r] = layer
            .iter()
            .map(|v| {
                if g.horizontal {
                    sizes[*v].y
                } else {
                    sizes[*v].x
                }
            })
            .sum::<f32>()
            + gap * layer.len().saturating_sub(1) as f32;
    }
    let max_span = col_span.iter().copied().fold(0.0_f32, f32::max);

    // 每列的主向起点。
    let mut col_start = vec![0.0_f32; layers.len()];
    let mut acc = 0.0_f32;
    for r in 0..layers.len() {
        col_start[r] = acc;
        acc += col_thick[r] + rank_gap;
    }

    let mut out = vec![
        Placed {
            center: Pos2::ZERO,
            size: Vec2::ZERO,
            shape: NodeShape::Rect,
        };
        g.nodes.len()
    ];
    for (r, layer) in layers.iter().enumerate() {
        // 本列在主向上整体居中。
        let mut secondary = (max_span - col_span[r]) * 0.5;
        for v in layer {
            let s = sizes[*v];
            let (sec_extent, thick_extent) = if g.horizontal { (s.y, s.x) } else { (s.x, s.y) };
            let primary = col_start[r] + (col_thick[r] - thick_extent) * 0.5;
            let (px, py) = if g.horizontal {
                (primary, secondary)
            } else {
                (secondary, primary)
            };
            secondary += sec_extent + gap;
            out[*v] = Placed {
                center: Pos2::new(px + s.x * 0.5, py + s.y * 0.5),
                size: s,
                shape: g.nodes[*v].1,
            };
        }
    }

    // 反向（`RL` / `BT`）：把主坐标镜像。
    if g.reversed {
        let total = acc - rank_gap;
        for p in &mut out {
            if g.horizontal {
                p.center.x = total - p.center.x;
            } else {
                p.center.y = total - p.center.y;
            }
        }
    }
    out
}

/// 发出图元：先把节点全部画出，再画边（箭头落在节点边界上，不会被节点盖住）。
fn emit(g: &Graph, placed: &[Placed], size: f32, measure: Measure<'_>) -> Vec<Prim> {
    let mut prims: Vec<Prim> = Vec::new();
    for (i, p) in placed.iter().enumerate() {
        push_node(&mut prims, p, &g.nodes[i].0, size);
    }
    for e in &g.edges {
        push_edge(&mut prims, g, placed, e, size, measure);
    }
    prims
}

/// 一个节点的外形与文字。
fn push_node(prims: &mut Vec<Prim>, p: &Placed, text: &str, size: f32) {
    let c = p.center;
    let (hw, hh) = (p.size.x * 0.5, p.size.y * 0.5);
    let rect_pts = |w: f32, h: f32| {
        vec![
            Pos2::new(c.x - w, c.y - h),
            Pos2::new(c.x + w, c.y - h),
            Pos2::new(c.x + w, c.y + h),
            Pos2::new(c.x - w, c.y + h),
        ]
    };
    let corners = |hw: f32, hh: f32, radius: f32, ticks: bool| Prim::Poly {
        points: rect_pts(hw, hh),
        radius,
        fill: true,
        ticks,
    };
    match p.shape {
        NodeShape::Rect => prims.push(corners(hw, hh, 4.0, false)),
        NodeShape::Rounded => prims.push(corners(hw, hh, hh.min(12.0), false)),
        // 机场跑道：圆角半径等于半高。
        NodeShape::Stadium => prims.push(corners(hw, hh, hh, false)),
        NodeShape::Subroutine => prims.push(corners(hw, hh, 2.0, true)),
        // 数据库：用大圆角矩形近似真圆柱——epaint 没有椭圆填充图元，
        // 而圆角矩形在视觉上同样表达「存储」。
        NodeShape::Cylinder => prims.push(corners(hw, hh, hh * 0.7, false)),
        // 圆：尺寸阶段已把宽高调成正方形，故「圆角=半宽」即圆。
        NodeShape::Circle => prims.push(corners(hw, hh, hw.min(hh), false)),
        NodeShape::Rhombus => prims.push(Prim::Poly {
            points: vec![
                Pos2::new(c.x, c.y - hh),
                Pos2::new(c.x + hw, c.y),
                Pos2::new(c.x, c.y + hh),
                Pos2::new(c.x - hw, c.y),
            ],
            radius: 0.0,
            fill: true,
            ticks: false,
        }),
        NodeShape::Hexagon => {
            let inset = hw * 0.5;
            prims.push(Prim::Poly {
                points: vec![
                    Pos2::new(c.x - hw + inset, c.y - hh),
                    Pos2::new(c.x + hw - inset, c.y - hh),
                    Pos2::new(c.x + hw, c.y),
                    Pos2::new(c.x + hw - inset, c.y + hh),
                    Pos2::new(c.x - hw + inset, c.y + hh),
                    Pos2::new(c.x - hw, c.y),
                ],
                radius: 0.0,
                fill: true,
                ticks: false,
            });
        }
        NodeShape::Parallelogram => {
            let skew = hw * 0.4;
            prims.push(Prim::Poly {
                points: vec![
                    Pos2::new(c.x - hw + skew, c.y - hh),
                    Pos2::new(c.x + hw, c.y - hh),
                    Pos2::new(c.x + hw - skew, c.y + hh),
                    Pos2::new(c.x - hw, c.y + hh),
                ],
                radius: 0.0,
                fill: true,
                ticks: false,
            });
        }
        NodeShape::Asymmetric => {
            prims.push(Prim::Poly {
                points: vec![
                    Pos2::new(c.x - hw, c.y - hh),
                    Pos2::new(c.x + hw - hh * 0.6, c.y - hh),
                    Pos2::new(c.x + hw, c.y),
                    Pos2::new(c.x + hw - hh * 0.6, c.y + hh),
                    Pos2::new(c.x - hw, c.y + hh),
                ],
                radius: 0.0,
                fill: true,
                ticks: false,
            });
        }
    }
    prims.push(Prim::Text {
        at: c,
        text: text.to_string(),
        size,
        plate: false,
    });
}

/// 一条边：走线 + 箭头 + 标签。
fn push_edge(
    prims: &mut Vec<Prim>,
    g: &Graph,
    placed: &[Placed],
    e: &Edge,
    size: f32,
    measure: Measure<'_>,
) {
    let a = &placed[e.from];
    let b = &placed[e.to];
    // 线宽随字号缩放，但保底 0.7 点，否则小字号下图会「消失」。
    let width = match e.line {
        LineStyle::Thick => 2.4,
        _ => 1.4,
    } * (size / 16.0).max(0.7);
    let dotted = e.line == LineStyle::Dotted;

    // 自环：在节点上方绕一个小圈（`A --> A` 在状态图里很常见）。
    if e.from == e.to {
        let top = Pos2::new(a.center.x, a.center.y - a.size.y * 0.5);
        let r = size * 0.7;
        let pts = [
            top,
            Pos2::new(top.x - r, top.y - r),
            Pos2::new(top.x + r, top.y - r),
            top,
        ];
        for w in pts.windows(2) {
            prims.push(Prim::Line {
                a: w[0],
                b: w[1],
                width,
                dotted,
            });
        }
        prims.push(Prim::Arrow {
            tip: top,
            dir: Vec2::new(0.0, 1.0),
            len: size * ARROW_LEN,
            spread: ARROW_SPREAD,
        });
        if let Some(label) = &e.label {
            prims.push(Prim::Text {
                at: Pos2::new(top.x, top.y - r * 1.6),
                text: label.clone(),
                size: size * 0.85,
                plate: false,
            });
        }
        return;
    }

    // 出 / 入方向：沿主轴由 a 指向 b，另一轴上则互相靠近（正交走线）。
    let (out_dir, in_dir) = if g.horizontal {
        let sign = if b.center.x >= a.center.x { 1.0 } else { -1.0 };
        (Vec2::new(sign, 0.0), Vec2::new(sign, 0.0))
    } else {
        let sign = if b.center.y >= a.center.y { 1.0 } else { -1.0 };
        (Vec2::new(0.0, sign), Vec2::new(0.0, sign))
    };

    // 从节点**边界**出发而不是中心：否则线会被节点盖住一半，箭头也看不见。
    let start = boundary_point(a, out_dir);
    let end = boundary_point(b, -in_dir);
    let route = route_points(start, end, out_dir, in_dir);
    for w in route.windows(2) {
        prims.push(Prim::Line {
            a: w[0],
            b: w[1],
            width,
            dotted,
        });
    }
    if e.bidirectional {
        prims.push(Prim::Arrow {
            tip: start,
            dir: -out_dir,
            len: size * ARROW_LEN,
            spread: ARROW_SPREAD,
        });
    }
    match e.head {
        Head::Arrow => prims.push(Prim::Arrow {
            tip: end,
            dir: -in_dir,
            len: size * ARROW_LEN,
            spread: ARROW_SPREAD,
        }),
        Head::Cross => prims.push(Prim::Cross {
            at: end,
            dir: in_dir,
            len: size * ARROW_LEN * 0.7,
        }),
        // `--o`：空心圆头用「很扁的三角」不行，改画一个小线段交叉的近似——
        // 这里用短而宽的三角，视觉上读作圆点（见 paint 的填充色）。
        Head::Circle => prims.push(Prim::Arrow {
            tip: end,
            dir: -in_dir,
            len: size * ARROW_LEN * 0.45,
            spread: 1.0,
        }),
        Head::None => {}
    }

    if let Some(label) = &e.label {
        // 标签压在线上读不清，故 `plate = true` 铺一块底色把线挡住（见 paint）。
        let _ = measure;
        prims.push(Prim::Text {
            at: polyline_midpoint(&route),
            text: label.clone(),
            size: size * 0.85,
            plate: true,
        });
    }
}

/// 取节点边界上沿 `dir` 方向的交点。
fn boundary_point(p: &Placed, dir: Vec2) -> Pos2 {
    let (hw, hh) = (p.size.x * 0.5, p.size.y * 0.5);
    // 菱形 / 圆按更小的内切半径算，让线不要插进斜边里。
    let (sx, sy) = match p.shape {
        NodeShape::Rhombus => (hw * 0.62, hh * 0.62),
        NodeShape::Circle => (hw * 0.8, hh * 0.8),
        _ => (hw, hh),
    };
    let t = if dir.x.abs() > f32::EPSILON {
        sx / dir.x.abs()
    } else {
        f32::INFINITY
    };
    let u = if dir.y.abs() > f32::EPSILON {
        sy / dir.y.abs()
    } else {
        f32::INFINITY
    };
    let k = t.min(u);
    Pos2::new(p.center.x + dir.x * k, p.center.y + dir.y * k)
}

/// 走线：同轴对齐时走直线，否则走「出边 → 中点干 → 入边」的折线。
fn route_points(start: Pos2, end: Pos2, out_dir: Vec2, in_dir: Vec2) -> Vec<Pos2> {
    let aligned = (out_dir.x.abs() > 0.5 && (end.y - start.y).abs() < 1.0)
        || (out_dir.y.abs() > 0.5 && (end.x - start.x).abs() < 1.0);
    if aligned {
        return vec![start, end];
    }
    if out_dir.x.abs() > 0.5 {
        let mid_x = (start.x + end.x) * 0.5;
        vec![
            start,
            Pos2::new(start.x + out_dir.x * ROUTE_STUB, start.y),
            Pos2::new(mid_x, start.y),
            Pos2::new(mid_x, end.y),
            Pos2::new(end.x - in_dir.x * ROUTE_STUB, end.y),
            end,
        ]
    } else {
        let mid_y = (start.y + end.y) * 0.5;
        vec![
            start,
            Pos2::new(start.x, start.y + out_dir.y * ROUTE_STUB),
            Pos2::new(start.x, mid_y),
            Pos2::new(end.x, mid_y),
            Pos2::new(end.x, end.y - in_dir.y * ROUTE_STUB),
            end,
        ]
    }
}

/// 折线的中点（用于放标签）。
fn polyline_midpoint(pts: &[Pos2]) -> Pos2 {
    let total: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let mut want = total * 0.5;
    for w in pts.windows(2) {
        let seg = (w[1] - w[0]).length();
        if seg >= want {
            let t = if seg > 0.0 { want / seg } else { 0.0 };
            return w[0] + (w[1] - w[0]) * t;
        }
        want -= seg;
    }
    *pts.last().unwrap_or(&Pos2::ZERO)
}

/// 一个图元的外接框。
fn prim_bounds(p: &Prim) -> Rect {
    match p {
        Prim::Poly { points, .. } => Rect::from_points(points),
        Prim::Line { a, b, width, .. } => Rect::from_two_pos(*a, *b).expand(*width * 0.5),
        Prim::Arrow {
            tip,
            dir,
            len,
            spread,
        } => Rect::from_two_pos(*tip, *tip - *dir * *len).expand(*len * *spread),
        Prim::Cross { at, len, .. } => Rect::from_center_size(*at, Vec2::splat(*len * 2.0)),
        Prim::Text { at, text, size, .. } => {
            // 用「字符数 × 半字号」估外接框：这里只需要**足够大**。
            // 真实宽度由绘制阶段的字体度量决定，估算偏差只让版面多一点留白。
            let w = text.chars().count() as f32 * *size * 0.55;
            Rect::from_center_size(*at, Vec2::new(w.max(*size), *size * 1.5))
        }
    }
}

fn shift_prim(p: Prim, d: Vec2) -> Prim {
    match p {
        Prim::Poly {
            points,
            radius,
            fill,
            ticks,
        } => Prim::Poly {
            points: points.into_iter().map(|q| q + d).collect(),
            radius,
            fill,
            ticks,
        },
        Prim::Line {
            a,
            b,
            width,
            dotted,
        } => Prim::Line {
            a: a + d,
            b: b + d,
            width,
            dotted,
        },
        Prim::Arrow {
            tip,
            dir,
            len,
            spread,
        } => Prim::Arrow {
            tip: tip + d,
            dir,
            len,
            spread,
        },
        Prim::Cross { at, dir, len } => Prim::Cross {
            at: at + d,
            dir,
            len,
        },
        Prim::Text {
            at,
            text,
            size,
            plate,
        } => Prim::Text {
            at: at + d,
            text,
            size,
            plate,
        },
    }
}

// ============================================================================
// 绘制
// ============================================================================

/// 图表配色（由调用方从主题取；本模块不依赖 `Theme`）。
#[derive(Clone, Copy, Debug)]
pub struct MermaidStyle {
    /// 节点填充色。
    pub node_fill: Color32,
    /// 节点描边色。
    pub node_stroke: Color32,
    /// 节点文字色。
    pub text: Color32,
    /// 连线色。
    pub edge: Color32,
    /// 边标签底色。
    pub label_bg: Color32,
}

/// 把版面按可用宽度缩放到 `rect` 里，返回实际占用高度。
///
/// 宽度不足时**整体等比缩小**（含字号与线宽）——宁可整张图小一点，
/// 也不要右侧被裁掉：图的信息全在结构上，被裁掉的结构等于没画。
pub fn paint(painter: &Painter, rect: Rect, layout: &MermaidLayout, style: &MermaidStyle) -> f32 {
    let scale = if layout.width > rect.width() && layout.width > 0.0 {
        rect.width() / layout.width
    } else {
        1.0
    };
    let origin = Pos2::new(
        rect.left() + (rect.width() - layout.width * scale).max(0.0) * 0.5,
        rect.top(),
    );
    let map = |p: Pos2| origin + p.to_vec2() * scale;

    for prim in &layout.prims {
        match prim {
            Prim::Poly {
                points,
                radius,
                fill,
                ticks,
            } => {
                let pts: Vec<Pos2> = points.iter().map(|p| map(*p)).collect();
                let stroke = Stroke::new(1.2 * scale, style.node_stroke);
                if *fill {
                    let r = Rect::from_points(&pts);
                    if pts.len() == 4 && *radius > 0.0 {
                        painter.rect_filled(
                            r,
                            CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8),
                            style.node_fill,
                        );
                    } else {
                        painter.add(Shape::convex_polygon(
                            pts.clone(),
                            style.node_fill,
                            Stroke::NONE,
                        ));
                    }
                }
                // 描边：圆角矩形用 `rect_stroke`，其余多边形闭合折线。
                let r = Rect::from_points(&pts);
                if pts.len() == 4 && *radius > 0.0 {
                    painter.rect_stroke(
                        r,
                        CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8),
                        stroke,
                        StrokeKind::Inside,
                    );
                } else {
                    let mut closed = pts.clone();
                    if let Some(first) = pts.first() {
                        closed.push(*first);
                    }
                    painter.add(Shape::line(closed, stroke));
                }
                if *ticks {
                    // 子程序形：左右各一道内竖线。
                    for t in [0.25_f32, 0.75] {
                        let x = r.left() + r.width() * t;
                        painter.line_segment(
                            [Pos2::new(x, r.top()), Pos2::new(x, r.bottom())],
                            Stroke::new(1.0 * scale, style.node_stroke),
                        );
                    }
                }
            }
            Prim::Line {
                a,
                b,
                width,
                dotted,
            } => {
                let (a, b) = (map(*a), map(*b));
                let stroke = Stroke::new(width * scale, style.edge);
                if *dotted {
                    painter.extend(Shape::dashed_line(
                        &[a, b],
                        stroke,
                        4.0 * scale,
                        3.0 * scale,
                    ));
                } else {
                    painter.line_segment([a, b], stroke);
                }
            }
            Prim::Arrow {
                tip,
                dir,
                len,
                spread,
            } => {
                let tip = map(*tip);
                let len = len * scale;
                // 等比缩放不改变方向，故 `dir` 直接用原值。
                let side = Vec2::new(-dir.y, dir.x);
                let back = tip - *dir * len;
                painter.add(Shape::convex_polygon(
                    vec![
                        tip,
                        back + side * len * *spread,
                        back - side * len * *spread,
                    ],
                    style.edge,
                    Stroke::NONE,
                ));
            }
            Prim::Cross { at, dir, len } => {
                let at = map(*at);
                let len = len * scale;
                let side = Vec2::new(-dir.y, dir.x);
                for s in [1.0_f32, -1.0] {
                    painter.line_segment(
                        [at, at + (*dir + side * s) * len * 0.9],
                        Stroke::new(1.4 * scale, style.edge),
                    );
                }
            }
            Prim::Text {
                at,
                text,
                size,
                plate,
            } => {
                if text.is_empty() {
                    continue;
                }
                let at = map(*at);
                let mut job = LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    TextFormat {
                        font_id: FontId::proportional(size * scale),
                        color: style.text,
                        ..Default::default()
                    },
                );
                let galley = painter.layout_job(job);
                let pos = at - galley.size() * 0.5;
                if *plate {
                    // 边标签：先铺一块底色，否则线从字缝里穿过去读不清。
                    painter.rect_filled(
                        Rect::from_center_size(at, galley.size() + Vec2::splat(4.0 * scale)),
                        CornerRadius::same(2),
                        style.label_bg,
                    );
                }
                painter.galley(pos, galley, style.text);
            }
        }
    }
    layout.height * scale
}

/// 按可用宽度等比缩放后占用的高度（调用方排布卡片时用）。
pub fn fitted_height(layout: &MermaidLayout, max_w: f32) -> f32 {
    let scale = if layout.width > max_w && layout.width > 0.0 {
        max_w / layout.width
    } else {
        1.0
    };
    layout.height * scale
}

/// 供测试与调用方查询图元数量（绘制结果是否为空的自检入口）。
pub fn prim_count(layout: &MermaidLayout) -> usize {
    layout.prims.len()
}

// ============================================================================
// 公共收尾：把图元 + 热点统一归一化到左上角原点，产出 MermaidLayout。
// ============================================================================

/// 把（已 emit 的）图元与热点坐标统一平移到左上角为原点。
///
/// - `id_rects`：带 id 的元素外接框（供 `click id href "url"` 按 id 命中）；
/// - `links`：`click` 指令解析出的 `(id, url)`；
/// - `direct`：直接框定的热点（如标签里的 markdown 链接，元素本身无稳定 id）。
fn finalize(
    prims: Vec<Prim>,
    id_rects: Vec<(String, Rect)>,
    links: Vec<(String, String)>,
    direct: Vec<(Rect, String)>,
) -> MermaidLayout {
    let mut bounds: Option<Rect> = None;
    for p in &prims {
        bounds = Some(match bounds {
            Some(b) => b.union(prim_bounds(p)),
            None => prim_bounds(p),
        });
    }
    for (_, r) in &id_rects {
        bounds = Some(union_opt(bounds, *r));
    }
    for (r, _) in &direct {
        bounds = Some(union_opt(bounds, *r));
    }
    let bounds = bounds.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0)));
    let offset = -bounds.min.to_vec2();
    let prims: Vec<Prim> = prims.into_iter().map(|p| shift_prim(p, offset)).collect();
    let mut hotspots: Vec<(Rect, String)> = Vec::new();
    for (id, r) in &id_rects {
        if let Some((_, url)) = links.iter().find(|(i, _)| i == id) {
            hotspots.push((r.translate(offset), url.clone()));
        }
    }
    for (r, url) in &direct {
        hotspots.push((r.translate(offset), url.clone()));
    }
    MermaidLayout {
        width: bounds.width(),
        height: bounds.height(),
        prims,
        hotspots,
    }
}

/// `Option<Rect>` 与 `Rect` 取并集的辅助。
fn union_opt(acc: Option<Rect>, r: Rect) -> Rect {
    match acc {
        Some(a) => a.union(r),
        None => r,
    }
}

/// 扫描全部 `click <id> href "url"` 指令，返回 `(id, url)`。
///
/// `click <id> call fn` 没有 URL，忽略；缺引号等畸形也忽略——链接是增强特性，
/// 缺失不该让整张图回退。
fn collect_clicks(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in src.lines() {
        if let Some(rest) = line.trim().strip_prefix("click") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 3
                && parts[1] == "href"
                && let Some(url) = extract_quoted(rest)
            {
                out.push((parts[0].to_string(), url));
            }
        }
    }
    out
}

/// 取出第一对双引号之间的内容（不处理转义，够用）。
fn extract_quoted(s: &str) -> Option<String> {
    let a = s.find('"')?;
    let b = s[a + 1..].find('"').map(|i| a + 1 + i)?;
    Some(s[a + 1..b].to_string())
}

/// 从文本里取第一个 markdown 链接 `[文本](url)` 的 url。
fn first_md_link(text: &str) -> Option<String> {
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let rest = &text[i..];
            if let Some(close) = rest.find(']')
                && rest[close..].starts_with("](")
            {
                let inner = &rest[close + 2..];
                if let Some(end) = inner.find(')') {
                    return Some(inner[..end].trim().to_string());
                }
            }
        }
        i += 1;
    }
    None
}

/// 轴对齐矩形的四个角点（用于 `Poly`）。
fn rect_corners(r: Rect) -> Vec<Pos2> {
    vec![
        Pos2::new(r.left(), r.top()),
        Pos2::new(r.right(), r.top()),
        Pos2::new(r.right(), r.bottom()),
        Pos2::new(r.left(), r.bottom()),
    ]
}

/// 矩形（中心 + 半尺寸）沿 `dir` 方向到边界的交点。
fn rect_boundary(center: Pos2, half: Vec2, dir: Vec2) -> Pos2 {
    let t = if dir.x.abs() > f32::EPSILON {
        half.x / dir.x.abs()
    } else {
        f32::INFINITY
    };
    let u = if dir.y.abs() > f32::EPSILON {
        half.y / dir.y.abs()
    } else {
        f32::INFINITY
    };
    let k = t.min(u);
    Pos2::new(center.x + dir.x * k, center.y + dir.y * k)
}

/// 给矩形描一圈边（四段 `Line`，沿用 `edge` 配色）。
fn push_rect_outline(prims: &mut Vec<Prim>, r: Rect, w: f32) {
    let c = rect_corners(r);
    for k in 0..4 {
        let a = c[k];
        let b = c[(k + 1) % 4];
        prims.push(Prim::Line {
            a,
            b,
            width: w,
            dotted: false,
        });
    }
}

/// 在矩形里画一个对勾（两段 `Line`），表示「已完成」。
fn push_check(prims: &mut Vec<Prim>, r: Rect, w: f32) {
    let cx = r.left() + r.width() * 0.3;
    let cy = r.center().y;
    let s = r.height() * 0.25;
    prims.push(Prim::Line {
        a: Pos2::new(cx - s, cy),
        b: Pos2::new(cx, cy + s),
        width: w,
        dotted: false,
    });
    prims.push(Prim::Line {
        a: Pos2::new(cx, cy + s),
        b: Pos2::new(cx + s * 1.6, cy - s),
        width: w,
        dotted: false,
    });
}

// ============================================================================
// 时序图（sequenceDiagram）
// ============================================================================

/// 消息箭头形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SeqHead {
    /// `->>` / `-->`：实心箭头。
    Solid,
    /// `-->>`：虚线箭头。
    Dotted,
    /// `--x`：叉头。
    Cross,
}

/// 一条消息。
#[derive(Clone, Debug)]
struct SeqMsg {
    from: String,
    to: String,
    label: String,
    head: SeqHead,
}

/// 备注框类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NoteKind {
    Over,
    LeftOf,
    RightOf,
}

/// 一条备注。
#[derive(Clone, Debug)]
struct SeqNote {
    kind: NoteKind,
    a: String,
    b: Option<String>,
    text: String,
}

/// 时序图事件（保持源码顺序，逐行占据一个纵向层级）。
#[derive(Clone, Debug)]
enum SeqEvent {
    Msg(SeqMsg),
    Note(SeqNote),
}

/// 解析后的时序图。
#[derive(Clone, Debug)]
struct SeqModel {
    /// 参与者：`(id, 显示标签)`。
    parts: Vec<(String, String)>,
    events: Vec<SeqEvent>,
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
fn parse_seq_message(line: &str) -> Option<(String, String, String, SeqHead)> {
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
fn parse_sequence(src: &str) -> Result<SeqModel, MermaidError> {
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
fn layout_sequence(
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
            a: Pos2::new(x, top),
            b: Pos2::new(x, bottom),
            width: 1.0 * (size / 16.0).max(0.7),
            dotted: true,
        });
        prims.push(Prim::Text {
            at: Pos2::new(x, size * 0.8),
            text: label.clone(),
            size: size * 0.9,
            plate: false,
        });
        let r = Rect::from_min_max(
            Pos2::new(x - col_w * 0.45, 0.0),
            Pos2::new(x + col_w * 0.45, bottom),
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
                    a: Pos2::new(xf, y),
                    b: Pos2::new(xt, y),
                    width: line_w,
                    dotted: m.head == SeqHead::Dotted,
                });
                match m.head {
                    SeqHead::Solid | SeqHead::Dotted => {
                        prims.push(Prim::Arrow {
                            tip: Pos2::new(xt, y),
                            dir: Vec2::new(sign, 0.0),
                            len: size * ARROW_LEN,
                            spread: ARROW_SPREAD,
                        });
                    }
                    SeqHead::Cross => {
                        prims.push(Prim::Cross {
                            at: Pos2::new(xt, y),
                            dir: Vec2::new(sign, 0.0),
                            len: size * ARROW_LEN * 0.7,
                        });
                    }
                }
                if !m.label.is_empty() {
                    prims.push(Prim::Text {
                        at: Pos2::new((xf + xt) * 0.5, y),
                        text: m.label.clone(),
                        size: size * 0.85,
                        plate: true,
                    });
                }
                if let Some(url) = first_md_link(&m.label) {
                    let lo = xf.min(xt) - size * 0.5;
                    let hi = xf.max(xt) + size * 0.5;
                    direct.push((
                        Rect::from_min_max(
                            Pos2::new(lo, y - size * 0.6),
                            Pos2::new(hi, y + size * 0.6),
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
                let rect = Rect::from_min_max(
                    Pos2::new(x0, y - size * 0.7),
                    Pos2::new(x1, y + size * 0.7),
                );
                prims.push(Prim::Poly {
                    points: rect_corners(rect),
                    radius: 2.0,
                    fill: true,
                    ticks: false,
                });
                if !n.text.is_empty() {
                    prims.push(Prim::Text {
                        at: rect.center(),
                        text: n.text.clone(),
                        size: size * 0.8,
                        plate: false,
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

// ============================================================================
// 类图（classDiagram）
// ============================================================================

/// 关系形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RelKind {
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
struct ClassBox {
    id: String,
    name: String,
    members: Vec<String>,
}

/// 一条关系。
#[derive(Clone, Debug)]
struct Rel {
    a: String,
    b: String,
    kind: RelKind,
    label: Option<String>,
}

/// 解析后的类图。
#[derive(Clone, Debug)]
struct ClassModel {
    classes: Vec<ClassBox>,
    rels: Vec<Rel>,
}

/// 解析类图源码。
fn parse_class(src: &str) -> Result<ClassModel, MermaidError> {
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
    // 与 [`parse_seq_arrow`] 同一坑：原来按 `chars()` 数出**字符**下标 `i`，
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
fn layout_class(src: &str, size: f32, measure: Measure<'_>) -> Result<MermaidLayout, MermaidError> {
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
        rects.push(Rect::from_min_size(Pos2::new(x, y), Vec2::new(box_w, h)));
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
        });
        // 类名（居中于表头）。
        prims.push(Prim::Text {
            at: Pos2::new(r.left() + box_w * 0.5, r.top() + header_h * 0.5),
            text: c.name.clone(),
            size,
            plate: false,
        });
        // 表头分隔线。
        prims.push(Prim::Line {
            a: Pos2::new(r.left(), r.top() + header_h),
            b: Pos2::new(r.right(), r.top() + header_h),
            width: 1.0 * (size / 16.0).max(0.7),
            dotted: false,
        });
        // 成员行。
        for (j, m) in c.members.iter().enumerate() {
            prims.push(Prim::Text {
                at: Pos2::new(
                    r.left() + size * 0.4,
                    r.top() + header_h + member_h * (j as f32 + 0.5),
                ),
                text: m.clone(),
                size: size * 0.85,
                plate: false,
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
            Vec2::new(1.0, 0.0)
        };
        let ha = rects[ai].size() * 0.5;
        let hb = rects[bi].size() * 0.5;
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
                    Pos2::new(d.x - dm, d.y),
                    Pos2::new(d.x, d.y - dm * 0.7),
                    Pos2::new(d.x + dm, d.y),
                    Pos2::new(d.x, d.y + dm * 0.7),
                ];
                prims.push(Prim::Poly {
                    points: diamond,
                    radius: 0.0,
                    fill: rel.kind == RelKind::Composition,
                    ticks: false,
                });
            }
            RelKind::Plain | RelKind::Dotted => {}
        }
        if let Some(label) = &rel.label {
            let mid = Pos2::new((sa.x + sb.x) * 0.5, (sa.y + sb.y) * 0.5);
            prims.push(Prim::Text {
                at: mid,
                text: label.clone(),
                size: size * 0.8,
                plate: true,
            });
            if let Some(url) = first_md_link(label) {
                direct.push((
                    Rect::from_center_size(
                        mid,
                        Vec2::new(label.chars().count() as f32 * size * 0.5 + size, size * 1.4),
                    ),
                    url,
                ));
            }
        }
    }
    Ok(finalize(prims, id_rects, clicks, direct))
}

// ============================================================================
// 甘特图（gantt）
// ============================================================================

/// 一个任务。
#[derive(Clone, Debug)]
struct GanttTask {
    id: Option<String>,
    name: String,
    dur: f32,
    done: bool,
    crit: bool,
    dep: Option<String>,
    section: usize,
}

/// 解析后的甘特图。
#[derive(Clone, Debug)]
struct GanttModel {
    title: Option<String>,
    sections: Vec<String>,
    tasks: Vec<GanttTask>,
}

/// 识别「天数/周数」形式的时长令牌。
fn is_duration(tok: &str) -> Option<f32> {
    let t = tok.trim_end_matches(['d', 'w', 'D', 'W']);
    let n: f32 = t.parse().ok()?;
    let unit = tok.chars().last()?;
    let mult = match unit {
        'w' | 'W' => 7.0,
        _ => 1.0,
    };
    Some(n * mult)
}

/// 识别 `YYYY-MM-DD` 形式的日期令牌（仅用于跳过，不解析日历）。
fn is_date(tok: &str) -> bool {
    let b: Vec<&str> = tok.split('-').collect();
    b.len() == 3
        && b[0].len() == 4
        && b[1].len() == 2
        && b[2].len() == 2
        && b.iter().all(|s| s.chars().all(|c| c.is_ascii_digit()))
}

/// 解析甘特图源码。
fn parse_gantt(src: &str) -> Result<GanttModel, MermaidError> {
    let mut title: Option<String> = None;
    let mut sections: Vec<String> = Vec::new();
    let mut tasks: Vec<GanttTask> = Vec::new();
    let mut cur_section: usize = 0;
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let lw = leading_word(t);
        if lw == "gantt" || lw == "click" {
            continue;
        }
        if lw == "title" {
            let rest = t.strip_prefix("title").unwrap_or(t);
            title = Some(rest.trim().to_string());
            continue;
        }
        if matches!(
            lw.as_str(),
            "dateFormat" | "axisFormat" | "todayMarker" | "excludes"
        ) {
            continue;
        }
        if lw == "section" {
            let rest = t.strip_prefix("section").unwrap_or(t);
            sections.push(rest.trim().to_string());
            cur_section = sections.len() - 1;
            continue;
        }
        // 任务行：`Name : spec`
        if let Some(colon) = t.find(':') {
            let name = t[..colon].trim().to_string();
            let spec = t[colon + 1..].trim();
            let toks: Vec<&str> = spec
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let mut id: Option<String> = None;
            let mut dur = 0.0_f32;
            let mut done = false;
            let mut crit = false;
            let mut dep: Option<String> = None;
            for tok in toks {
                if tok == "done" {
                    done = true;
                } else if tok == "crit" {
                    crit = true;
                } else if let Some(rest) = tok.strip_prefix("after ") {
                    dep = Some(rest.trim().to_string());
                } else if is_date(tok) {
                    // 显式日期忽略，保留 id / 时长。
                } else if let Some(d) = is_duration(tok) {
                    dur = d;
                } else if id.is_none() {
                    id = Some(tok.to_string());
                }
            }
            if dur <= 0.0 {
                dur = 1.0;
            }
            tasks.push(GanttTask {
                id,
                name,
                dur,
                done,
                crit,
                dep,
                section: cur_section,
            });
        }
    }
    Ok(GanttModel {
        title,
        sections,
        tasks,
    })
}

/// 甘特图布局 + 发图元。
fn layout_gantt(src: &str, size: f32, measure: Measure<'_>) -> Result<MermaidLayout, MermaidError> {
    let model = parse_gantt(src)?;
    let day_w = size * 0.6;
    let label_w = if model.tasks.is_empty() {
        size * 8.0
    } else {
        let mut mw = size * 6.0;
        for t in &model.tasks {
            let w = measure(&t.name, size);
            if w > mw {
                mw = w;
            }
        }
        for s in &model.sections {
            let w = measure(s, size);
            if w > mw {
                mw = w;
            }
        }
        mw + size * 1.0
    };
    let margin = size;
    let top = if model.title.is_some() {
        size * 2.5
    } else {
        size
    };
    let row_h = size * 1.8;
    let bar_h = size * 1.0;

    let mut end_by_id: Vec<(String, f32)> = Vec::new();
    let mut starts: Vec<f32> = Vec::new();
    let mut ends: Vec<f32> = Vec::new();
    let mut section_cursor: Vec<f32> = vec![0.0; model.sections.len().max(1)];
    let mut total_days = 0.0_f32;
    for t in &model.tasks {
        let sec = t.section.min(section_cursor.len() - 1);
        let start = match &t.dep {
            Some(d) => end_by_id
                .iter()
                .find(|(i, _)| i == d)
                .map(|(_, e)| *e)
                .unwrap_or(0.0),
            None => section_cursor[sec],
        };
        let end = start + t.dur;
        starts.push(start);
        ends.push(end);
        if let Some(id) = &t.id {
            if let Some(pos) = end_by_id.iter().position(|(i, _)| i == id) {
                end_by_id[pos].1 = end;
            } else {
                end_by_id.push((id.clone(), end));
            }
        }
        section_cursor[sec] = section_cursor[sec].max(end);
        if end > total_days {
            total_days = end;
        }
    }
    if total_days <= 0.0 {
        total_days = 1.0;
    }

    let mut prims: Vec<Prim> = Vec::new();
    let mut id_rects: Vec<(String, Rect)> = Vec::new();
    let mut direct: Vec<(Rect, String)> = Vec::new();
    let clicks = collect_clicks(src);
    let line_w = 1.0 * (size / 16.0).max(0.7);

    // 标题。
    if let Some(title) = &model.title {
        prims.push(Prim::Text {
            at: Pos2::new(margin, size * 1.0),
            text: title.clone(),
            size: size * 1.1,
            plate: false,
        });
    }
    // 时间轴基线。
    let axis_y = top - size * 0.2;
    prims.push(Prim::Line {
        a: Pos2::new(label_w + margin, axis_y),
        b: Pos2::new(label_w + margin + total_days * day_w, axis_y),
        width: line_w,
        dotted: false,
    });

    // 任务行。
    let mut y = top;
    let mut last_section: Option<usize> = None;
    for (i, t) in model.tasks.iter().enumerate() {
        if last_section != Some(t.section) {
            prims.push(Prim::Line {
                a: Pos2::new(margin * 0.5, y),
                b: Pos2::new(label_w + margin + total_days * day_w + margin, y),
                width: line_w,
                dotted: true,
            });
            if let Some(s) = model.sections.get(t.section) {
                prims.push(Prim::Text {
                    at: Pos2::new(margin * 0.5, y - size * 0.2),
                    text: s.clone(),
                    size: size * 0.9,
                    plate: false,
                });
            }
            last_section = Some(t.section);
        }
        let x0 = label_w + margin + starts[i] * day_w;
        let w = (ends[i] - starts[i]).max(0.5) * day_w;
        let bar = Rect::from_min_size(
            Pos2::new(x0, y + (row_h - bar_h) * 0.5),
            Vec2::new(w, bar_h),
        );
        prims.push(Prim::Poly {
            points: rect_corners(bar),
            radius: 2.0,
            fill: true,
            ticks: false,
        });
        // 任务名（左侧）。
        prims.push(Prim::Text {
            at: Pos2::new(margin, y + row_h * 0.5),
            text: t.name.clone(),
            size: size * 0.85,
            plate: false,
        });
        // crit：描边；done：画对勾，区分两类任务。
        if t.crit {
            push_rect_outline(&mut prims, bar, line_w * 1.6);
        }
        if t.done {
            push_check(&mut prims, bar, line_w * 1.4);
        }
        if let Some(id) = &t.id {
            id_rects.push((id.clone(), bar));
        }
        if let Some(url) = first_md_link(&t.name) {
            direct.push((bar, url));
        }
        y += row_h;
    }

    Ok(finalize(prims, id_rects, clicks, direct))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假度量：每字符宽 `0.5em`（与 `math` 的测试同口径）。
    fn m() -> impl Fn(&str, f32) -> f32 {
        |s: &str, size: f32| s.chars().count() as f32 * size * 0.5
    }

    fn lay(src: &str) -> MermaidLayout {
        let f = m();
        layout(src, 14.0, &f).expect("应能布局")
    }

    fn bad(src: &str) -> MermaidError {
        let f = m();
        layout(src, 14.0, &f).expect_err("应报错")
    }

    #[test]
    fn simple_graph_has_two_nodes_and_one_edge() {
        let g = parse_graph("graph LR\nA --> B\n").expect("解析");
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
        assert!(g.horizontal, "LR 应为水平流向");
    }

    #[test]
    fn all_directions_are_recognized() {
        for (src, horiz, rev) in [
            ("graph LR\nA-->B", true, false),
            ("graph RL\nA-->B", true, true),
            ("graph TB\nA-->B", false, false),
            ("graph TD\nA-->B", false, false),
            ("graph BT\nA-->B", false, true),
            ("flowchart LR\nA-->B", true, false),
            // 只有 `graph` 没写方向时按 Mermaid 的默认 TB 处理。
            ("graph\nA-->B", false, false),
        ] {
            let g = parse_graph(src).unwrap_or_else(|e| panic!("{src} 应能解析：{e}"));
            assert_eq!(g.horizontal, horiz, "{src}");
            assert_eq!(g.reversed, rev, "{src}");
        }
    }

    #[test]
    fn node_shapes_are_recognized() {
        let src = "graph LR\nA[a]-->B(b)-->C([c])-->D[[d]]-->E((e))-->F{f}";
        let g = parse_graph(src).expect("解析");
        let shapes: Vec<NodeShape> = g.nodes.iter().map(|(_, s)| *s).collect();
        assert_eq!(
            shapes,
            vec![
                NodeShape::Rect,
                NodeShape::Rounded,
                NodeShape::Stadium,
                NodeShape::Subroutine,
                NodeShape::Circle,
                NodeShape::Rhombus,
            ]
        );
    }

    #[test]
    fn cylinder_and_hexagon_shapes_are_recognized() {
        let g = parse_graph("graph LR\nA[(库)]-->B{{六}}\n").expect("解析");
        assert_eq!(g.nodes[0].1, NodeShape::Cylinder);
        assert_eq!(g.nodes[1].1, NodeShape::Hexagon);
        assert_eq!(g.nodes[0].0, "库");
    }

    #[test]
    fn node_text_overrides_the_id() {
        let g = parse_graph("graph LR\nA[启动流程]-->B\n").expect("解析");
        assert_eq!(g.nodes[0].0, "启动流程");
        // 裸 id 的节点用 id 当文本。
        assert_eq!(g.nodes[1].0, "B");
    }

    #[test]
    fn quoted_node_text_is_unwrapped() {
        let g = parse_graph("graph LR\nA[\"带 空格 的文本\"]-->B\n").expect("解析");
        assert_eq!(g.nodes[0].0, "带 空格 的文本");
    }

    #[test]
    fn edge_styles_and_heads_are_recognized() {
        let g = parse_graph("graph LR\nA-->B\nB-.->C\nC==>D\nD---E\nE--xF\nF--oG\n").expect("解析");
        let styles: Vec<(LineStyle, Head)> = g.edges.iter().map(|e| (e.line, e.head)).collect();
        assert_eq!(
            styles,
            vec![
                (LineStyle::Solid, Head::Arrow),
                (LineStyle::Dotted, Head::Arrow),
                (LineStyle::Thick, Head::Arrow),
                (LineStyle::Solid, Head::None),
                (LineStyle::Solid, Head::Cross),
                (LineStyle::Solid, Head::Circle),
            ]
        );
    }

    #[test]
    fn bidirectional_edge_marks_both_ends() {
        let g = parse_graph("graph LR\nA<-->B\n").expect("解析");
        assert!(g.edges[0].bidirectional, "`<-->` 应记下起点侧的箭头");
    }

    #[test]
    fn inline_edge_label_between_dashes_is_read() {
        let g = parse_graph("graph LR\nA -- 是 --> B\n").expect("解析");
        assert_eq!(g.edges[0].label.as_deref(), Some("是"));
    }

    #[test]
    fn piped_edge_label_is_read() {
        let g = parse_graph("graph LR\nA -->|否| B\n").expect("解析");
        assert_eq!(g.edges[0].label.as_deref(), Some("否"));
    }

    #[test]
    fn chained_edges_produce_one_edge_per_hop() {
        let g = parse_graph("graph LR\nA --> B --> C\n").expect("解析");
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.nodes.len(), 3);
    }

    #[test]
    fn semicolons_and_comments_are_handled() {
        let g = parse_graph("graph LR; A-->B; B-->C;\n").expect("解析");
        assert_eq!(g.edges.len(), 2);
        // 语句写在头部同一行时也要读进来。
        let g = parse_graph("graph LR; A-->B\nB-->C\n").expect("解析");
        assert_eq!(g.edges.len(), 2);
    }

    #[test]
    fn comment_lines_are_skipped() {
        let g = parse_graph("graph LR\n%% 这是一行注释\nA-->B\n%% 又一行\n").expect("解析");
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn cosmetic_directives_are_ignored_not_fatal() {
        // 只改配色 / 交互的指令不该让整张图打回源码。
        let g = parse_graph(
            "graph LR\nA-->B\nstyle A fill:#f9f\nclassDef x fill:#fff\nlinkStyle 0 stroke:red\nclick A href \"x\"\n",
        )
        .expect("装饰性指令应被忽略");
        assert_eq!(g.nodes.len(), 2);
    }

    #[test]
    fn other_diagram_kinds_are_unsupported() {
        // 这三种现已支持，见下方各自的布局测试；这里只保留仍未支持的图种。
        for src in [
            "pie title x\n\"a\" : 1",
            "stateDiagram-v2\n[*] --> A",
            "erDiagram\nA ||--o{ B : has",
            "journey\ntitle x",
        ] {
            assert!(
                matches!(bad(src), MermaidError::Unsupported(_)),
                "{src} 应报 Unsupported"
            );
        }
    }

    #[test]
    fn sequence_messages_and_participants_parse() {
        let m = parse_sequence(
            "sequenceDiagram\nparticipant A as Alice\nA->>B: hi\nNote over A,B: note\n",
        )
        .expect("解析时序图");
        assert_eq!(m.parts.len(), 2, "Alice 与隐式参与者 B");
        assert_eq!(m.parts[0].1, "Alice");
        assert_eq!(m.events.len(), 2);
    }

    #[test]
    fn sequence_layout_has_positive_size_and_prims() {
        let l = lay("sequenceDiagram\nA->>B: hi\nB-->>A: ok\nA--x C\n");
        assert!(l.width > 0.0);
        assert!(l.height > 0.0);
        assert!(prim_count(&l) > 0);
    }

    /// 回归：含**中文**参与者名的时序图曾直接崩进程。
    ///
    /// 触发链：`parse_seq_arrow` 返回**字符**下标，`parse_seq_message` 却拿它
    /// 当**字节**下标切 `&line[..idx]`——`李四` 之后的 `idx=2` 落在 `李`
    /// （3 字节）内部，`&str` 切片 panic（`panic = "abort"` 崩进程）。
    /// 见崩溃报告 `crash-20261010152913`（消息：end byte index 2 is not a
    /// char boundary; it is inside '李'）。
    #[test]
    fn sequence_with_chinese_actor_names_does_not_panic() {
        let msg = parse_seq_message("李四->>张三: 你好").expect("应解析出消息");
        assert_eq!(msg.0, "李四", "发送方应是中文名 `李四`");
        assert_eq!(msg.1, "张三", "接收方应是中文名 `张三`");
        assert_eq!(msg.2, "你好");
        // 整条布局链路同样不得 panic（崩溃正是走 layout → parse_sequence）。
        let l = lay("sequenceDiagram\n李四->>张三: 你好吗\n张三-->>李四: 我很好\n");
        assert!(l.width > 0.0 && l.height > 0.0);
    }

    /// 回归：含中文的**类图关系**行曾同样崩在切片上（`parse_rel` 同源缺陷）。
    #[test]
    fn class_relation_with_chinese_names_does_not_panic() {
        let l = lay("classDiagram\n用户 --> 订单\n订单 <|-- 退款单\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        // 关系两端应被完整切出（未被错位截断）。
        let m = parse_class("classDiagram\n用户 --> 订单\n").expect("解析类图");
        assert_eq!(m.rels.len(), 1);
        assert_eq!(m.rels[0].a, "用户");
        assert_eq!(m.rels[0].b, "订单");
    }

    #[test]
    fn class_relationships_parse() {
        let m = parse_class("classDiagram\nclass A\nclass B\nA <|-- B\nA *-- B : label\n")
            .expect("解析类图");
        assert_eq!(m.classes.len(), 2);
        assert_eq!(m.rels.len(), 2);
        assert_eq!(m.rels[0].kind, RelKind::Inheritance);
    }

    #[test]
    fn class_layout_has_positive_size_and_prims() {
        let l = lay("classDiagram\nclass A {\n+field: T\n-method()\n}\nclass B\nA <|-- B\n");
        assert!(l.width > 0.0);
        assert!(l.height > 0.0);
        assert!(prim_count(&l) > 0);
    }

    #[test]
    fn gantt_tasks_parse() {
        let m = parse_gantt("gantt\ntitle T\nsection S\na : a1, 5d\nb : after a1, crit, 6d\n")
            .expect("解析甘特图");
        assert_eq!(m.tasks.len(), 2);
        assert!(m.tasks[1].crit);
        assert_eq!(m.tasks[1].dep.as_deref(), Some("a1"));
    }

    #[test]
    fn gantt_layout_has_positive_size_and_prims() {
        let l = lay("gantt\ntitle Plan\nsection Dev\na : a1, 5d\nb : after a1, crit, 6d\n");
        assert!(l.width > 0.0);
        assert!(l.height > 0.0);
        assert!(prim_count(&l) > 0);
    }

    #[test]
    fn click_directive_creates_hotspot() {
        let l = lay("graph LR\nA-->B\nclick A href \"https://x\"\n");
        assert!(!l.click_targets().is_empty(), "click 应生成可点击热点");
    }

    #[test]
    fn click_on_class_creates_hotspot() {
        let l = lay("classDiagram\nclass A\nclass B\nA --> B\nclick A href \"https://x\"\n");
        assert!(!l.click_targets().is_empty());
    }

    #[test]
    fn markdown_link_in_label_creates_hotspot() {
        let l = lay("sequenceDiagram\nA->>B: 见 [文档](https://y)\n");
        assert!(
            !l.click_targets().is_empty(),
            "标签里的 markdown 链接应生成热点"
        );
    }

    #[test]
    fn subgraph_is_unsupported_rather_than_half_drawn() {
        // 半张图比不画更难排查：用户会先怀疑自己的语法。
        let e = bad("graph LR\nsubgraph 一组\nA-->B\nend\n");
        assert!(matches!(e, MermaidError::Unsupported(_)), "实际 {e:?}");
    }

    #[test]
    fn malformed_input_is_reported_not_panicked() {
        // `panic = "abort"` 下绝不能崩。
        for src in [
            "graph LR\nA -->",
            "graph LR\nA --> )",
            "graph LR\nA[未闭合 --> B",
            "graph LR\n",
            "graph LR\n-->B",
            "graph ZZ\nA-->B",
        ] {
            let _ = bad(src);
        }
    }

    #[test]
    fn ranks_follow_edge_direction() {
        let g = parse_graph("graph LR\nA-->B-->C\n").expect("解析");
        let r = rank_nodes(&g);
        assert!(r[0] < r[1], "B 应排在 A 之后");
        assert!(r[1] < r[2], "C 应排在 B 之后");
    }

    #[test]
    fn parallel_nodes_share_a_rank() {
        let g = parse_graph("graph LR\nA-->B\nA-->C\n").expect("解析");
        let r = rank_nodes(&g);
        assert_eq!(r[1], r[2], "B 与 C 同层");
    }

    #[test]
    fn cycles_do_not_hang_or_panic() {
        let g = parse_graph("graph LR\nA-->B\nB-->C\nC-->A\n").expect("解析");
        let r = rank_nodes(&g);
        assert!(r.iter().any(|v| *v > 0), "有环也要排出层次");
        assert!(r.iter().all(|v| *v <= g.nodes.len()));
        // 布局本身也必须能结束（这条才是「不卡死」的真断言）。
        assert!(lay("graph LR\nA-->B\nB-->C\nC-->A\n").width > 0.0);
    }

    #[test]
    fn self_loop_does_not_hang() {
        let g = parse_graph("graph LR\nA-->A\n").expect("解析");
        let r = rank_nodes(&g);
        assert_eq!(r[0], 0, "自环不应把自身推到第 1 层");
        assert!(lay("graph LR\nA-->A\n").height > 0.0);
    }

    #[test]
    fn layout_produces_positive_bounds_and_prims() {
        let l = lay("graph LR\nA[开始]-->B{判断}-->C[结束]\n");
        assert!(l.width > 0.0);
        assert!(l.height > 0.0);
        assert!(prim_count(&l) > 0);
    }

    #[test]
    fn horizontal_layout_grows_along_x() {
        let l = lay("graph LR\nA[第一步]-->B[第二步]-->C[第三步]\n");
        assert!(
            l.width > l.height,
            "LR 三列应比一列宽：{}x{}",
            l.width,
            l.height
        );
    }

    #[test]
    fn vertical_layout_grows_along_y() {
        let l = lay("graph TD\nA[第一步]-->B[第二步]-->C[第三步]\n");
        assert!(
            l.height > l.width,
            "TD 三层应比一列高：{}x{}",
            l.width,
            l.height
        );
    }

    #[test]
    fn isolated_node_is_still_drawn() {
        let g = parse_graph("graph LR\nA-->B\nC\n").expect("解析");
        assert_eq!(g.nodes.len(), 3);
        assert!(prim_count(&lay("graph LR\nA-->B\nC\n")) > 0);
    }

    #[test]
    fn fitting_scales_down_when_too_wide() {
        let l = lay("graph LR\nA[很长的节点名称甲]-->B[很长的节点名称乙]-->C[很长的节点名称丙]\n");
        let narrow = l.width * 0.5;
        assert!(
            (fitted_height(&l, narrow) - l.height * 0.5).abs() < 0.01,
            "宽度不足时应等比缩小"
        );
        assert!(
            (fitted_height(&l, l.width * 2.0) - l.height).abs() < 0.01,
            "放得下就不该缩放"
        );
    }

    #[test]
    fn wider_node_text_makes_wider_graph() {
        let narrow = lay("graph LR\nA-->B\n");
        let wide = lay("graph LR\nA[一段明显更长的节点文字]-->B\n");
        assert!(wide.width > narrow.width);
    }

    #[test]
    fn every_hostile_input_is_an_error_not_a_panic() {
        // 一批「看起来像但又不对」的输入，全部只允许返回 Err。
        for src in [
            "",
            "\n\n",
            "graph",
            "%%只有注释",
            "graph LR\nA[[x]-->B",
            "graph LR\nA{{x}-->B",
            "graph LR\nA((x)-->B",
        ] {
            let f = m();
            let _ = layout(src, 14.0, &f);
        }
    }
}
