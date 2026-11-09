//! 「关于」分区与底部实时预览。

use egui::Ui;

use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::SettingsViewAccess;

use super::*;

/// 「关于」分区。
pub(super) fn about(vctx: &mut dyn SettingsViewAccess, ui: &mut Ui, theme: &Theme) {
    ui.heading("Rust Markdown Editor");
    ui.label(format!("版本 {VERSION}"));
    ui.colored_label(
        theme.text_secondary,
        "原生轻量 · 秒级启动 · 低内存占用。基于 Rust + egui 的即时模式实现。",
    );
    ui.add_space(theme.metrics.space_sm);
    ui.colored_label(
        theme.text_secondary,
        format!("已打开文档：{} 个", vctx.open_document_list().len()),
    );
}

/// 实时预览（对齐原型右下角 `.preview`）。
///
/// 用**当前生效**的主题与字号渲染，而不是硬编码字符串：
/// 原型的预览文字写死「深色主题 · 预览示例」，改成浅色后仍显示「深色主题」，
/// 用户会以为设置没生效。
///
/// 三行示例刻意走 [`preview_line`] 而不是 `ui.strong` / `ui.label` /
/// `ui.monospace`：后者用 egui 默认 `TextFormat`，既不带本主题的配色，
/// 也不带「行高」与「字体族」。设置页改行高时预览会纹丝不动——那正是
/// 「预览未同步变化」最直观的暴露点。
pub(super) fn live_preview(ui: &mut Ui, theme: &Theme, source_kind: &str) {
    ui.weak("预览");
    let kind_label = if source_kind.trim().eq_ignore_ascii_case("custom") {
        "自定义主题"
    } else {
        // 走真源的显示名表，不再手写一份 `match`——
        // 这份手写 `match` 是 R-09 收敛掉的**第六份**主题映射副本。
        match theme.kind {
            rustmark_core::shared::theme::ThemeKind::Dark => "深色主题",
            rustmark_core::shared::theme::ThemeKind::Light => "浅色主题",
            rustmark_core::shared::theme::ThemeKind::System => "跟随系统主题",
            rustmark_core::shared::theme::ThemeKind::Custom => "自定义主题",
        }
    };
    egui::Frame::new()
        .fill(theme.bg_primary)
        .stroke(egui::Stroke::new(1.0_f32, theme.border))
        .inner_margin(10.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            preview_line(ui, theme, &format!("{kind_label} · 预览示例"), false);
            preview_line(ui, theme, "正文示例：普通文字。", false);
            preview_line(ui, theme, "inline code / fn main() {}", true);
        });
}

/// 用当前主题的**预览度量**排一行：`TextFormat::line_height` 一旦给出，
/// epaint 就不再按字体度量自行算行高，故行高设置能在这里立即反映出来。
/// 字体族取 egui 的 `Proportional` / `Monospace`（与预览区渲染同源），
/// 换字体后 `set_fonts` 重建的族顺序也会同步体现。
fn preview_line(ui: &mut Ui, theme: &Theme, text: &str, mono: bool) {
    let size = theme.metrics.preview_font_size;
    let family = if mono {
        egui::FontFamily::Monospace
    } else {
        egui::FontFamily::Proportional
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(size, family),
            line_height: Some(size * theme.metrics.preview_line_height),
            color: theme.text_primary,
            ..Default::default()
        },
    );
    ui.label(job);
}
