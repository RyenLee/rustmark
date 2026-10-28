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
//! **不支持**：`subgraph`、以及 `stateDiagram` / `pie` / `journey` /
//! `erDiagram` / `mindmap` 等其它 Mermaid 图种。
//! 遇到它们返回 [`MermaidError::Unsupported`]，由调用方回退到「源码卡」
//! （见 `preview_view::draw_diagram`）——**不猜**。半张图画出来比不画更难排查：
//! 用户会先怀疑自己的语法。
//!
//! 另支持 **flowchart.js** 语法（`示例文档.md` 的「FLowchart流程图」一节，
//! `id=>type: 文本` 定义 + `->` 连线，含其首行著名的 `flowchat` 笔误标题），
//! 见 [`flow_js`]；它与 Mermaid 流程图共用同一套 `Graph` 模型与布局管线。
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
//!
//! # 模块结构
//!
//! - `model`：各图种共用的数据模型（`Graph` / `Prim` / [`MermaidLayout`]）；
//! - `graph_parse` / `graph_layout`：流程图解析与 Sugiyama 分层布局；
//! - `flow_js`：flowchart.js 语法的解析（复用同一布局管线）；
//! - `primitives`：图元生成、坐标归一化收尾与 `Prim` → `RenderCommand` 翻译；
//! - `sequence` / `class` / `gantt`：时序图 / 类图 / 甘特图；
//! - `tests`：外置单元测试。

mod class;
mod flow_js;
mod gantt;
mod graph_layout;
mod graph_parse;
mod model;
mod primitives;
mod sequence;

#[cfg(test)]
mod tests;

pub use self::model::MermaidLayout;
pub use self::primitives::{MermaidStyle, commands, fitted_height, prim_count};

use self::class::layout_class;
use self::gantt::layout_gantt;
use self::graph_layout::layout_graph;
use self::graph_parse::leading_word;
use self::sequence::layout_sequence;

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

/// 把源码排成版面。返回 `Err` 时调用方应回退到源码显示。
///
/// 依据首行关键字分发到对应的图种解析器；不在支持列表里的关键字
/// （`pie` / `stateDiagram` / `erDiagram` / …）回退为 `Unsupported`。
pub fn layout(src: &str, size: f32, measure: Measure<'_>) -> Result<MermaidLayout, MermaidError> {
    // flowchart.js 语法（`id=>type` 定义 + `->` 连线）优先识别：示例文档
    // 的这一节还带着 `flowchat` 笔误标题，不能让它落到 Mermaid 的
    // `detect_kind` 里被当成图种关键字报「不支持」。
    if flow_js::is_flowchart_js(src) {
        return flow_js::layout(src, size, measure);
    }
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
