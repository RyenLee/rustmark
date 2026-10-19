//! 绘制：把排版结果画进 egui `Painter`，含等比缩放高度的预算。

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

use super::layout::{Item, MathLayout};
use super::model::MathFont;

/// 把版面画到 `rect` 里，返回实际占用的高度。
///
/// `rect` 是**可用区域**；`align` 决定内容在其中的水平落点
/// （块级公式居中、行内公式左对齐）。宽度不足时整体等比缩小（含字号），
/// 与 [`crate::presentation::views::mermaid::paint`] 同一策略：宁可公式小一点，也不要右侧被裁掉——
/// 被裁掉的项在数学上会**改变含义**（`a+b=c` 裁成 `a+b`）。
pub fn paint(
    painter: &Painter,
    rect: Rect,
    layout: &MathLayout,
    color: Color32,
    align: Align2,
) -> f32 {
    let scale = if layout.width > rect.width() && layout.width > 0.0 {
        rect.width() / layout.width
    } else {
        1.0
    };
    let used_w = layout.width * scale;
    let used_h = layout.height() * scale;
    // 基线左端的绝对位置：纵向把内容居中，横向按 `align`。
    let v_extra = (rect.height() - used_h).max(0.0) * 0.5;
    let origin_y = rect.top() + v_extra + layout.ascent * scale;
    let origin_x = match align.x() {
        egui::Align::Min => rect.left(),
        egui::Align::Center => rect.left() + (rect.width() - used_w).max(0.0) * 0.5,
        egui::Align::Max => rect.right() - used_w,
    };
    let to_pos = |x: f32, y: f32| Pos2::new(origin_x + x * scale, origin_y + y * scale);
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
                // 斜体：`FontId` 里没有「斜体位」，但 epaint 的
                // `TextFormat::italics` 会把字形**倾斜合成**（见 epaint
                // `text_layout.rs` 的 italics 分支）。故数学变量（`x`）
                // 走 `LayoutJob` 而不是 `painter.text`。
                //
                // 双线体（`\mathbb{R}`）在 epaint 里没有对应字形族——
                // 这是本引擎**已知的表现力缺口**，不假装支持：`\mathbb`
                // 只做「正体」处理，视觉上与 `\mathrm` 相同。
                let mut job = LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    TextFormat {
                        font_id: FontId::proportional(*size * scale),
                        color,
                        italics: matches!(font, MathFont::Italic | MathFont::Calligraphic),
                        ..Default::default()
                    },
                );
                let galley = painter.layout_job(job);
                // 基线对齐：`Align2` 只有「外框」档位（Top/Center/Bottom），
                // 没有基线档位。直接把 `y` 当外框顶边会让每个公式整体上浮
                // 约 0.2em——分数与上下标会跟着一起飘。故先量一次拿到真实的
                // 基线偏移（行顶 + 首个字形的基线位置）。
                let baseline_offset = galley
                    .rows
                    .first()
                    .map(|row| row.pos.y + row.row.glyphs.first().map_or(0.0, |glyph| glyph.pos.y))
                    .unwrap_or(0.0);
                painter.galley(
                    to_pos(*x, *y) - Vec2::new(0.0, baseline_offset),
                    galley,
                    color,
                );
            }
            Item::Line { from, to, width } => {
                painter.line_segment(
                    [to_pos(from.0, from.1), to_pos(to.0, to.1)],
                    Stroke::new(*width * scale, color),
                );
            }
            Item::Rect { x, y, w, h } => {
                painter.rect_filled(
                    Rect::from_min_size(to_pos(*x, *y), Vec2::new(*w * scale, *h * scale)),
                    0.0,
                    color,
                );
            }
        }
    }
    used_h
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
