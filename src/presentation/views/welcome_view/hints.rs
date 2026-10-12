//! 底部快捷键提示行与 `<kbd>` 小标签。

use egui::{RichText, Ui, Vec2};

use crate::app::frame::EditorApp;
use crate::presentation::theme::builtin::Theme;

use super::actions::shortcut_hints;
use super::*;

/// 底部快捷键提示，**整行水平居中**（与标题、主操作按钮同一条中轴）。
///
/// 与 `actions::primary_actions` 同理：先量出整行宽度再补左侧余量。
/// 间距也一并改成「只在项与项之间加」，否则末尾那一份间距会让整行
/// 视觉重心偏左——居中会「看起来差一点」。
pub(super) fn hints(ui: &mut Ui, theme: &Theme, app: &EditorApp) {
    let items = shortcut_hints(app);
    let row_w: f32 = items
        .iter()
        .enumerate()
        .map(|(i, hint)| {
            kbd_width(ui, &hint.combo)
                + HINT_KBD_GAP
                + hint_label_width(ui, &hint.label)
                + if i > 0 { HINT_ITEM_GAP } else { 0.0 }
        })
        .sum();
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
        for (i, hint) in items.iter().enumerate() {
            if i > 0 {
                ui.add_space(HINT_ITEM_GAP);
            }
            kbd(ui, theme, &hint.combo);
            ui.add_space(HINT_KBD_GAP);
            ui.label(
                RichText::new(&hint.label)
                    .size(HINT_FONT_SIZE)
                    .color(theme.text_secondary),
            );
        }
    });
}

/// 键位小标签的宽度（点），供 [`hints`] 量取整行宽度。
fn kbd_width(ui: &Ui, text: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::monospace(KBD_FONT_SIZE),
        egui::Color32::WHITE,
    );
    galley.size().x + KBD_PAD_X
}

/// 单条提示说明文字的宽度（点），供 [`hints`] 量取整行宽度。
fn hint_label_width(ui: &Ui, label: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(HINT_FONT_SIZE),
            egui::Color32::WHITE,
        )
        .size()
        .x
}

/// 键位小标签（`<kbd>` 样式）。
fn kbd(ui: &mut Ui, theme: &Theme, text: &str) {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::monospace(KBD_FONT_SIZE),
        theme.text_primary,
    );
    let pad = Vec2::new(KBD_PAD_X, 4.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad, egui::Sense::hover());
    let box_rect = egui::Rect::from_center_size(rect.center(), galley.size() + pad);
    ui.painter()
        .rect_filled(box_rect, egui::CornerRadius::same(3), theme.bg_tertiary);
    ui.painter().rect_stroke(
        box_rect,
        egui::CornerRadius::same(3),
        egui::Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        box_rect.left_top() + Vec2::new(6.0, 2.0),
        galley,
        theme.text_primary,
    );
}
