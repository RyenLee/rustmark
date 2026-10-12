//! Mermaid 各图种共用的数据模型：解析产物 `Graph`、布局产物 `Prim` / `MermaidLayout`。

use egui::{Pos2, Rect, Vec2};

/// 节点外形。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum NodeShape {
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
pub(super) enum LineStyle {
    Solid,
    Dotted,
    Thick,
}

/// 边的末端装饰。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Head {
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
pub(super) struct Edge {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) label: Option<String>,
    pub(super) line: LineStyle,
    pub(super) head: Head,
    /// 起点侧是否也有箭头（`<-->`）。
    pub(super) bidirectional: bool,
}

/// 解析后的图。
#[derive(Clone, Debug)]
pub(super) struct Graph {
    /// 流向为水平（`LR` / `RL`）还是垂直（`TB` / `TD` / `BT`）。
    pub(super) horizontal: bool,
    /// 是否反向（`RL` / `BT`）。
    pub(super) reversed: bool,
    /// 节点：显示文本 + 外形。
    pub(super) nodes: Vec<(String, NodeShape)>,
    /// 节点 id（与 `nodes` 一一对应，供 `click` 指令按 id 命中热点）。
    pub(super) node_ids: Vec<String>,
    pub(super) edges: Vec<Edge>,
}

/// 一个节点在版面里的位置与尺寸。
#[derive(Clone, Debug)]
pub(super) struct Placed {
    /// 节点中心。
    pub(super) center: Pos2,
    pub(super) size: Vec2,
    pub(super) shape: NodeShape,
}

/// 绘制图元（坐标已含方向变换，可直接使用）。
#[derive(Clone, Debug)]
pub(super) enum Prim {
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
    pub(super) prims: Vec<Prim>,
    /// 可点击热点：`(矩形, URL)`。矩形与图元同一坐标系（左上角为原点），
    /// 预览层之后会据此生成 `ctx.open_url` 的交互区。
    pub hotspots: Vec<(Rect, String)>,
}

impl MermaidLayout {
    /// 预览层用来生成交互区的热点列表（已归一化到左上角原点）。
    pub fn click_targets(&self) -> &[(Rect, String)] {
        &self.hotspots
    }

    /// 版面里画出的**节点框**数量。
    ///
    /// 每个节点恰好产出一个**填充的多边形**图元（见 `push_node`：矩形 / 圆 /
    /// 菱形 / 六边形…各一种轮廓，`fill` 均为 `true`），而边只有折线与箭头、
    /// 边标签是文字，都不计入。因此这个数就是图的节点数，可用来把
    /// 「同一个 id 被引用多次不得多出框」这条不变量钉在**版面**层面
    /// （回归：`intern` 曾按「显示文本」而非「id」查重，`graph LR` 里
    /// 先定义带标签节点、再按 id 串联这种最常规写法会让 4 个节点排成 8 个）。
    pub fn node_count(&self) -> usize {
        self.prims
            .iter()
            .filter(|p| matches!(p, Prim::Poly { fill: true, .. }))
            .count()
    }
}
