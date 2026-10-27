//! 渲染端口层：把「画什么」（egui-free 的几何命令）与「怎么画」（唯一 egui 后端）
//! 解耦。
//!
//! # 目标（问题 F / O4）
//!
//! `rich` 的排版产物只描述**几何图元**，不依赖 `egui` 的绘制 API；
//! 只有本目录的 [`egui`] 子模块允许 `use egui`，把 [`RenderCommand`] 翻译成
//! `Painter` 调用。这样导出 / headless 可复用同一套排版，而不必背负 egui。
//!
//! # 坐标约定
//!
//! 命令里的坐标都是**内容局部坐标**（排版引擎自己的坐标系，原点与缩放由调用方
//! 通过 [`egui::apply_egui`] 传入的 `rect` / 自然尺寸决定）。后端只做
//! `abs = origin + coord * scale` 的线性映射，不关心坐标系语义。
//!
//! [`egui`]: crate::presentation::rich::render::egui

mod egui;
pub mod geom;

pub use egui::{EguiBackend, apply_egui, rgb8_from_color32};
pub use geom::Rect;

use std::ops::{Add, Div, Mul, Neg, Sub};

/// 二维点（取代 egui `Pos2` / `Vec2` 的几何语义；同时用作「点」与「自由向量」）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    /// 横轴坐标。
    pub x: f32,
    /// 纵轴坐标。
    pub y: f32,
}

impl Pt {
    /// 构造点。
    pub const fn new(x: f32, y: f32) -> Self {
        Pt { x, y }
    }
    /// 原点 `(0, 0)`。
    pub const fn zero() -> Self {
        Pt { x: 0.0, y: 0.0 }
    }
    /// 两轴同值的点（构造等向尺寸 / 偏移时用）。
    pub const fn splat(v: f32) -> Self {
        Pt { x: v, y: v }
    }
}

/// 颜色（取代 egui `Color32`；非预乘 RGBA 八位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb8 {
    /// 红（0–255）。
    pub r: u8,
    /// 绿（0–255）。
    pub g: u8,
    /// 蓝（0–255）。
    pub b: u8,
    /// 透明度（0–255，255 为不透明）。
    pub a: u8,
}

impl Rgb8 {
    /// 构造颜色。
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgb8 { r, g, b, a }
    }
    /// 不透明颜色。
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Rgb8::new(r, g, b, 255)
    }
    /// 灰度（三通道同值）。
    pub const fn gray(v: u8) -> Self {
        Rgb8::opaque(v, v, v)
    }
}

/// 水平对齐（取代 egui `Align` 里表现层实际用到的档位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    /// 贴左。
    Left,
    /// 居中。
    Center,
    /// 贴右。
    Right,
}

/// 文字锚点：数学用基线左端，图表标签用中心。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAnchor {
    /// 锚点在文字的基线左端（数学排版，需按字形度量回退基线偏移）。
    BaselineLeft,
    /// 锚点在文字的中心（图表节点 / 边标签）。
    Center,
}

/// 一条绘制命令（egui-free 的纯几何描述）。
#[derive(Clone, Debug)]
pub enum RenderCommand {
    /// 文本。
    Text {
        /// 锚点位置（内容局部坐标）。
        at: Pt,
        /// 文本内容。
        text: String,
        /// 字号（点，未缩放；后端按 `scale` 缩放）。
        size: f32,
        /// 是否斜体（数学变量 / 花体）。
        italic: bool,
        /// 锚定方式（基线左端 / 中心）。
        anchor: TextAnchor,
        /// 是否先铺一块底色（边标签压线时用）。
        plate: bool,
        /// `plate` 的底色（仅 `plate` 时生效）。
        plate_bg: Rgb8,
        /// 文字颜色。
        color: Rgb8,
    },
    /// 线段（分数线、根号、括号、边线…）。
    Line {
        /// 起点（内容局部坐标）。
        a: Pt,
        /// 终点（内容局部坐标）。
        b: Pt,
        /// 线宽（点，未缩放）。
        width: f32,
        /// 是否虚线。
        dotted: bool,
        /// 颜色。
        color: Rgb8,
    },
    /// 轴对齐实心 / 描边矩形（数学用）。
    Rect {
        /// 左上角（内容局部坐标）。
        pos: Pt,
        /// 尺寸（宽、高）。
        size: Pt,
        /// 圆角半径（点，未缩放）。
        radius: f32,
        /// 颜色。
        color: Rgb8,
        /// `true` 填充、`false` 描边。
        fill: bool,
    },
    /// 多边形（节点轮廓）。`radius > 0` 且为 4 点时按圆角矩形处理。
    Poly {
        /// 顶点（内容局部坐标）。
        points: Vec<Pt>,
        /// 圆角半径（点，未缩放；注意：后端按绝对像素用，不随 `scale` 缩放）。
        radius: f32,
        /// `true` 填充。
        fill: bool,
        /// 子程序形的两道内竖线。
        ticks: bool,
        /// 填充色（覆盖 `style` 节点色时逐条指定）。
        fill_color: Rgb8,
        /// 描边色（覆盖 `style` 节点色时逐条指定）。
        stroke_color: Rgb8,
    },
    /// 实心三角箭头。
    Arrow {
        /// 箭尖（内容局部坐标）。
        tip: Pt,
        /// 指向（单位方向，不随 `scale` 缩放）。
        dir: Pt,
        /// 长度（点，未缩放）。
        len: f32,
        /// 张角系数（箭翼相对长度的占比）。
        spread: f32,
        /// 颜色。
        color: Rgb8,
    },
    /// 叉头（`--x`）。
    Cross {
        /// 中心（内容局部坐标）。
        at: Pt,
        /// 指向（单位方向，不随 `scale` 缩放）。
        dir: Pt,
        /// 半长（点，未缩放）。
        len: f32,
        /// 颜色。
        color: Rgb8,
    },
}

/// 渲染后端：把命令画到某个目标（egui 只是其中一种）。
pub trait RenderBackend {
    /// 绘制一条命令。
    fn draw(&mut self, cmd: &RenderCommand);
}

// Pt 的运算符（具体方法见 `geom`，此处补 `Add` / `Sub` / `Neg` / `Mul<f32>`
// 以便表达式风格书写，与 `geom` 里的方法互为补充）。
impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}
impl Neg for Pt {
    type Output = Pt;
    fn neg(self) -> Pt {
        Pt::new(-self.x, -self.y)
    }
}
impl Mul<f32> for Pt {
    type Output = Pt;
    fn mul(self, s: f32) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
}
impl Mul<f32> for &Pt {
    type Output = Pt;
    fn mul(self, s: f32) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
}
impl Div<f32> for Pt {
    type Output = Pt;
    fn div(self, s: f32) -> Pt {
        Pt::new(self.x / s, self.y / s)
    }
}
