//! 内置 LaTeX 子集排版引擎（问题 3：数学公式在预览里不出图）。
//!
//! # 为什么自己写，而不是塞一个 KaTeX / MathJax / resvg
//!
//! 三条硬约束把这个方向排除掉了（均已在 ADR / 依赖评审中记录）：
//! 1. **KaTeX 与 MathJax 都是 JS**——`eframe` 里没有 JS 运行时，要么内嵌
//!    一个浏览器控件（体积 +100MB 量级），要么走 `egui::Html`（不存在）；
//! 2. **`resvg` / `usvg` 带 C 依赖**（`fontdb` + `ttf-parser` 尚可，
//!    但 `resvg` 的几何与滤镜栈会显著抬高构建复杂度与产物体积）；
//! 3. 本项目 `panic = "abort"`，任何第三方排版库里的 `unwrap` 都是一次
//!    进程终止，而不是可捕获的异常。
//!
//! 于是这里给出的是**够用的 LaTeX 子集**：常见数学符号、分数、根式、
//! 上下标、大运算符带上下限、矩阵、尺寸自适应的括号、重音符号。
//! 遇到子集外的写法**不猜**——返回 [`MathError::Unsupported`]，
//! 由调用方回退到「等宽源码卡」（见 `preview_view::draw_math`）。
//!
//! # 架构：解析 → 排版（纯函数）→ 绘制，三段分离
//!
//! ```text
//! LaTeX 字符串 ──parse──> Node 树 ──layout──> MathLayout ──paint──> egui Painter
//! ```
//!
//! 中间的 `layout` 刻意做成**纯函数**（度量通过 `&dyn Fn(&str, f32) -> f32`
//! 注入，不碰 `egui::Painter`）：这样「分数比分母高」「上下标缩到 0.72 倍」
//! 「矩阵列宽取最宽项」这类**排版规则**都能在无头环境里断言，
//! 而不是只能靠肉眼看截图。
//!
//! # 坐标约定（整个模块统一）
//!
//! 一切以**基线左端**为原点：`y` 向上为负、向下为正。
//! - `ascent`：基线以上的高度（正值）；
//! - `descent`：基线以下的深度（正值）；
//! - 盒子的总高 = `ascent + descent`，宽 = `width`。
//!
//! 这个约定让「上下标」成了「把子盒基线下移 / 上移若干」这种一行代码的事，
//! 也是 TeX 自己的盒模型（`height` / `depth`）。若改用「左上角为原点」，
//! 每次嵌套都要重算整体高度，分数与根式会立刻写错。
//!
//! # 模块结构
//!
//! 三段管线各占一个私有子模块，公共入口（`layout` / `paint` /
//! `fitted_height`）、错误类型、排版常量与度量类型留在本 facade：
//!
//! - `symbols`：命令 → 字形的命名表与函数名 / 环境名单；
//! - `model`：语法树（`Node` / `MathFont` / `Accent`）与树变换；
//! - `parser`：单遍下降解析器；
//! - `layout`：纯函数排版（盒模型 + 注入度量）；
//! - `paint`：egui 绘制与缩放后高度；
//! - `tests`：无头排版规则断言。

mod layout;
mod model;
mod paint;
mod parser;
mod symbols;

#[cfg(test)]
mod tests;

pub use self::layout::MathLayout;
pub use self::paint::{fitted_height, paint};

use self::layout::{Ctx, layout_nodes};
use self::model::mark_stacked;
use self::parser::parse;

/// 排版遇到子集外的写法。
///
/// **只有一个变体是刻意的**：对用户而言「这块公式没能排版」是同一件事，
/// 细分「哪个命令不支持」只对开发者有意义，而那属于日志。
/// 渲染层据此整体回退到源码卡，不做「能排一半就先排一半」——
/// 半成品公式比源码更难读，还会让人以为公式写错了。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathError {
    /// 出现了子集外的命令 / 结构。
    Unsupported(String),
    /// 括号不配对、参数缺失等语法问题。
    Malformed(String),
}

impl std::fmt::Display for MathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "不支持的写法：{what}"),
            Self::Malformed(what) => write!(f, "公式语法错误：{what}"),
        }
    }
}

/// 上下标 / 分子分母等「小一号」文字的缩放系数。
///
/// 0.72 取自 TeX 的 `scriptstyle`（10pt → 7pt），比 0.5 更接近印刷惯例：
/// 缩到一半会让 `x^{2}` 的 2 几乎读不出。
const SCRIPT_SCALE: f32 = 0.72;

/// 嵌套缩放的下限：`x^{y^{z^{w}}}` 这种写法再缩下去就是一小撮像素。
const SCRIPT_MIN_SCALE: f32 = 0.5;

/// 上标相对基线抬升的比例（相对**当前**字号）。
const SUP_SHIFT: f32 = 0.45;
/// 下标相对基线下沉的比例。
const SUB_SHIFT: f32 = 0.2;

/// 分数线的上下留白（em）。
const FRAC_GAP: f32 = 0.16;
/// 分数线粗细（em）。
const RULE_THICK: f32 = 0.05;

/// 根号勾形相对被开方内容的高度/宽度比例。
const SQRT_HOOK_RATIO: f32 = 0.28;
const SQRT_GAP: f32 = 0.12;

/// 大运算符在上下限模式下的额外上下留白（em）。
const LIMIT_GAP: f32 = 0.12;

/// 文本宽度度量函数：`(文本, 字号) -> 宽度（点）`。
///
/// 注入而非直接调 `egui`，是为了让排版规则可单测（见模块文档）。
pub type Measure<'a> = &'a dyn Fn(&str, f32) -> f32;

/// 把 LaTeX 排成一份可绘制的版面。
///
/// 返回 `Err` 时调用方应回退到源码显示（见模块文档）。
pub fn layout(
    latex: &str,
    size: f32,
    display: bool,
    measure: Measure<'_>,
) -> Result<MathLayout, MathError> {
    let mut nodes = parse(latex, display)?;
    mark_stacked(&mut nodes, display);
    let ctx = Ctx {
        base: size,
        measure,
    };
    let mut box_ = layout_nodes(&nodes, size, &ctx);
    // 空公式（`$$ $$`）不留 0 高度的怪块。
    if box_.w <= 0.0 {
        box_.w = size * 0.5;
    }
    Ok(box_.finish())
}

/// 符号表的全部条目（`(命令名, 字形)`），供字形覆盖测试与将来的符号面板使用。
///
/// 暴露它是为了一个具体的质量要求：数学符号若落在已装载字体的覆盖之外，
/// 预览里就是一个**方框**——不报错、不崩溃，肉眼在几十个符号里几乎看不出来
/// （工具栏图标踩过同样的坑，见 `toolbar_visual_test` 的第 1 条不变量）。
/// 只有把这张表交给自动化断言逐字形核对，才守得住。
pub fn symbol_table() -> &'static [(&'static str, &'static str)] {
    self::symbols::SYMBOLS
}
