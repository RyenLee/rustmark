//! 品牌区与顶部光晕。

use egui::{RichText, Ui, Vec2};

use crate::presentation::theme::builtin::Theme;

use super::*;

/// 品牌区。
pub(super) fn brand(ui: &mut Ui, theme: &Theme) {
    ui.label(
        RichText::new(LOGO)
            .size(30.0)
            .strong()
            .color(theme.text_primary),
    );
    ui.add_space(theme.metrics.space_md);
    ui.label(RichText::new(SLOGAN).size(13.0).color(theme.text_secondary));
    ui.add_space(theme.metrics.space_lg);
    // 版本号做成一个圆角小标签（原型 `.version`）。
    let galley = ui.painter().layout_no_wrap(
        format!("v{VERSION}"),
        egui::FontId::proportional(11.0),
        theme.text_secondary,
    );
    let pad = Vec2::new(16.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad, egui::Sense::hover());
    let box_rect = egui::Rect::from_center_size(rect.center(), galley.size() + pad);
    ui.painter().rect_stroke(
        box_rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        box_rect.left_top() + Vec2::new(8.0, 2.0),
        galley,
        theme.text_secondary,
    );
}

/// 顶部两团光晕。
pub(super) fn paint_glow(painter: &egui::Painter, rect: egui::Rect, theme: &Theme) {
    let center1 = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.22);
    let center2 = egui::pos2(
        rect.left() + rect.width() * 0.12,
        rect.top() + rect.height() * 0.88,
    );
    for (center, size) in [
        (center1, Vec2::new(rect.width() * 0.6, rect.height() * 0.45)),
        (center2, Vec2::new(rect.width() * 0.4, rect.height() * 0.3)),
    ] {
        // 用一个实心椭圆 + 低透明度模拟径向渐变：egui 没有渐变 API，
        // 而画 20 个递减透明度的椭圆去逼近渐变，在深色主题下
        // 能看到明显的同心环。实心低透明是更诚实的近似。
        // `ellipse` 在 egui 0.33 里叫 `circle`，且**只**接受半径。
        // 要椭圆得用 `add(Shape::ellipse)`；这里取短半轴画正圆，
        // 视觉上仍是柔和光晕（原型是椭圆，差异只在轮廓形状）。
        painter.circle_filled(
            center,
            size.x.min(size.y) / 2.0,
            theme.accent_soft.gamma_multiply(0.3),
        );
    }
}
