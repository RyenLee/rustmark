//! 排版（纯函数）：语法树 → 基线坐标系下的绘制指令，度量经 `Ctx` 注入。
//!
//! 排版主逻辑（`layout_nodes` / `layout_node`）拆至兄弟模块
//! [`layout_fns`](layout/layout_fns.rs)。

use super::model::{Accent, MathFont, Node};
use super::*;

mod layout_fns;

pub(crate) use layout_fns::layout_nodes;

// ============================================================================
// 类型定义
// ============================================================================

/// 绘制指令（坐标相对**基线左端**）。
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Item {
    /// 一段文本，`y` 是它的基线。
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        font: MathFont,
    },
    /// 一条线段（分数线、根号、括号…）。
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
    },
    /// 一个实心矩形（分数线可复用，重音的点也用）。
    Rect { x: f32, y: f32, w: f32, h: f32 },
}

/// 排版结果。
#[derive(Clone, Debug, PartialEq)]
pub struct MathLayout {
    /// 内容宽度（点）。
    pub width: f32,
    /// 基线以上高度（点，正值）。
    pub ascent: f32,
    /// 基线以下深度（点，正值）。
    pub(crate) descent: f32,
    /// 绘制指令。
    pub(super) items: Vec<Item>,
}

impl MathLayout {
    /// 总高度。
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

/// 一个中间盒（排版过程中使用）。
#[derive(Clone, Debug, Default)]
pub(super) struct Layout {
    pub(super) w: f32,
    asc: f32,
    desc: f32,
    items: Vec<Item>,
}

impl Layout {
    /// 按 `dx` 右移并追加内容（`dy` 相对当前基线）。
    fn push(&mut self, other: &Layout, dx: f32, dy: f32) {
        for item in &other.items {
            self.items.push(shift(item, dx, dy));
        }
        self.w = self.w.max(dx + other.w);
    }

    /// 让本盒至少容纳 `asc` / `desc` 的高度。
    fn grow(&mut self, asc: f32, desc: f32) {
        self.asc = self.asc.max(asc);
        self.desc = self.desc.max(desc);
    }

    /// 以本盒的内容（宽 `w`）整体下移 `dy` 后返回新盒——用于分数、上下限。
    fn offset_items(&self, dx: f32, dy: f32) -> Vec<Item> {
        self.items.iter().map(|i| shift(i, dx, dy)).collect()
    }

    /// 收成最终结果（以基线左端为原点）。
    pub(super) fn finish(self) -> MathLayout {
        MathLayout {
            width: self.w,
            ascent: self.asc,
            descent: self.desc,
            items: self.items,
        }
    }
}

/// 平移一条绘制指令。
fn shift(item: &Item, dx: f32, dy: f32) -> Item {
    match *item {
        Item::Text {
            x,
            y,
            ref text,
            size,
            font,
        } => Item::Text {
            x: x + dx,
            y: y + dy,
            text: text.clone(),
            size,
            font,
        },
        Item::Line { from, to, width } => Item::Line {
            from: (from.0 + dx, from.1 + dy),
            to: (to.0 + dx, to.1 + dy),
            width,
        },
        Item::Rect { x, y, w, h } => Item::Rect {
            x: x + dx,
            y: y + dy,
            w,
            h,
        },
    }
}

/// 排版上下文：基准字号 + 文本度量。
///
/// # 为什么要把基准字号一路带下去
///
/// 角标的字号是**逐层相对父级**缩的（`x^{y^{z}}` 每深一层再乘
/// [`SCRIPT_SCALE`]）。若下限也按「相对父级」算，三层之后就是
/// `0.72³ ≈ 0.37` 倍——实测 `x^{y^{z^{w}}}` 最深层只剩 5.9pt，已经看不清了。
/// 下限必须锚在**公式的基准字号**上（`base * SCRIPT_MIN_SCALE`），
/// 于是深层的缩放比例会自动收敛，而不再一路乘下去。
pub(super) struct Ctx<'a> {
    /// 公式的基准字号（正文大小）。
    pub(super) base: f32,
    pub(super) measure: Measure<'a>,
}

impl Ctx<'_> {
    /// 下一层角标的字号。
    fn script_size(&self, size: f32) -> f32 {
        (size * SCRIPT_SCALE).max(self.base * SCRIPT_MIN_SCALE)
    }
}
