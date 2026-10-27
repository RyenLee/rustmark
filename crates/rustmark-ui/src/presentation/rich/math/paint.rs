//! 几何发射：把排版结果转成 egui-free 的 [`RenderCommand`] 序列（问题 F / O4）。
//!
//! 旧的 `paint` 直接调 egui；现拆为「发射几何」（本文件）＋「唯一 egui 后端绘制」
//! （见 `render` 端口层、`render::apply_egui`）。本文件不再 `use egui`，
//! 使无头 / 导出环境可复用同一套排版而不背负 egui。

use super::super::render::{Pt, RenderCommand, Rgb8, TextAnchor};
use super::layout::{Item, MathLayout};
use super::model::MathFont;

/// 把排版结果发射成一组 egui-free 的绘制命令。
///
/// 坐标沿用 `layout` 的「基线左端」约定（`y` 向上为负）。为复刻旧 `paint` 的
/// 纵向公式 `origin_y = rect.top() + v_extra + ascent * scale`，这里把 `ascent`
/// 折叠进每个 `y`（`y' = y + ascent`），于是下游 `render::apply_egui` 以
/// `rect.top()` 为纵轴原点、配合 `v_center = true`（承担 `v_extra`）即可等价。
///
/// 水平落点（左 / 中 / 右）由调用方经 `apply_egui` 的 `h_align` 传入，本函数
/// **不**预偏移 `x`：预偏移需要 `rect` 宽度，而发射阶段拿不到（见任务说明）。
pub fn emit(layout: &MathLayout, color: Rgb8) -> Vec<RenderCommand> {
    let dy = layout.ascent; // 基线 → 内容顶的位移，折叠进 y
    let c = color;
    let mut cmds: Vec<RenderCommand> = Vec::with_capacity(layout.items.len());
    for item in &layout.items {
        match item {
            Item::Text {
                x,
                y,
                text,
                size,
                font,
            } => {
                if text.is_empty() {
                    continue;
                }
                cmds.push(RenderCommand::Text {
                    at: Pt::new(*x, *y + dy),
                    text: text.clone(),
                    size: *size,
                    italic: matches!(*font, MathFont::Italic | MathFont::Calligraphic),
                    anchor: TextAnchor::BaselineLeft,
                    plate: false,
                    plate_bg: Rgb8::opaque(0, 0, 0),
                    color: c,
                });
            }
            Item::Line { from, to, width } => {
                cmds.push(RenderCommand::Line {
                    a: Pt::new(from.0, from.1 + dy),
                    b: Pt::new(to.0, to.1 + dy),
                    width: *width,
                    dotted: false,
                    color: c,
                });
            }
            Item::Rect { x, y, w, h } => {
                cmds.push(RenderCommand::Rect {
                    pos: Pt::new(*x, *y + dy),
                    size: Pt::new(*w, *h),
                    radius: 0.0,
                    color: c,
                    fill: true,
                });
            }
        }
    }
    cmds
}

/// 按可用宽度等比缩放后占用的高度（调用方排布卡片时用）。
///
/// 提前知道高度是有意义的：块级公式卡要先按这个高度占位，
/// 否则卡片高度会随「这一帧的可用宽度」变化，滚动同步的块矩形跟着抖。
pub fn fitted_height(layout: &MathLayout, max_w: f32) -> f32 {
    let scale = if layout.width > max_w && layout.width > 0.0 {
        max_w / layout.width
    } else {
        1.0
    };
    layout.height() * scale
}
