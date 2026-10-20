//! 「关于」分区与底部实时预览。

use egui::Ui;

use crate::app::frame::EditorApp;
use crate::presentation::theme::builtin::Theme;

use super::*;

/// 「关于」分区。
pub(super) fn about(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.heading("Rust Markdown Editor");
    ui.label(format!("版本 {VERSION}"));
    ui.colored_label(
        theme.text_secondary,
        "原生轻量 · 秒级启动 · 低内存占用。基于 Rust + egui 的即时模式实现。",
    );
    ui.add_space(theme.metrics.space_sm);
    ui.colored_label(
        theme.text_secondary,
        format!("已打开文档：{} 个", app.open_document_list().len()),
    );
}

/// 实时预览（对齐原型右下角 `.preview`）。
///
/// 用**当前生效**的主题与字号渲染，而不是硬编码字符串：
/// 原型的预览文字写死「深色主题 · 预览示例」，改成浅色后仍显示「深色主题」，
/// 用户会以为设置没生效。
pub(super) fn live_preview(ui: &mut Ui, theme: &Theme, source_kind: &str) {
    ui.weak("预览");
    let kind_label = if source_kind.trim().eq_ignore_ascii_case("custom") {
        "自定义主题"
    } else {
        // 走真源的显示名表，不再手写一份 `match`——
        // 这份手写 `match` 是 R-09 收敛掉的**第六份**主题映射副本。
        match theme.kind {
            crate::shared::theme::ThemeKind::Dark => "深色主题",
            crate::shared::theme::ThemeKind::Light => "浅色主题",
            crate::shared::theme::ThemeKind::System => "跟随系统主题",
            crate::shared::theme::ThemeKind::Custom => "自定义主题",
        }
    };
    egui::Frame::new()
        .fill(theme.bg_primary)
        .stroke(egui::Stroke::new(1.0_f32, theme.border))
        .inner_margin(10.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong(format!("{kind_label} · 预览示例"));
            ui.label("正文示例：普通文字。");
            ui.monospace("inline code / fn main() {}");
        });
}
