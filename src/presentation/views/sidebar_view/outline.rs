//! 大纲标签：标题层级过滤与点击跳转。

use egui::{Align, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::theme::builtin::Theme;

use super::*;

/// 大纲标签。
pub(super) fn outline_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    level_filter(app, ui);
    ui.separator();
    let items = app.current_outline();
    if items.is_empty() {
        ui.weak("当前文档没有标题");
        return;
    }
    let selected = app.sidebar.outline_selected.clone();
    let mut jump: Option<usize> = None;
    ScrollArea::vertical()
        .id_salt("rustmark.sidebar.outline")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for item in &items {
                if !app.sidebar.outline_level_visible(item.level) {
                    continue;
                }
                let is_active = selected.as_deref() == Some(item.text.as_str());
                let text = RichText::new(&item.text).color(if is_active {
                    theme.accent
                } else if item.level >= 4 {
                    theme.text_secondary
                } else {
                    theme.text_primary
                });
                let response = ui
                    .horizontal(|ui| {
                        ui.add_space(ROW_PAD + (item.level.saturating_sub(1)) as f32 * INDENT_STEP);
                        ui.label(
                            RichText::new("#".repeat(item.level as usize))
                                .color(theme.text_secondary),
                        );
                        // 标题占「剩余宽 − 行号预留」并**截断**：过长的标题不再把
                        // 固定宽的侧栏撑出一条不可见的黑边（SidePanel 是
                        // `exact_width`，溢出内容只会被裁掉）。
                        let avail = (ui.available_width() - OUTLINE_LINE_NO_W).max(0.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(avail, 0.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(text).truncate());
                            },
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.weak(item.line.to_string());
                        });
                    })
                    .response;
                if response.clicked() {
                    app.sidebar.outline_selected = Some(item.text.clone());
                    jump = Some(item.line);
                }
            }
        });
    if let Some(line) = jump {
        app.goto_outline_line(line);
    }
    ui.colored_label(
        theme.text_secondary,
        "点击标题可跳转；标题级别的调整请在编辑区用 Ctrl+[ / Ctrl+]。",
    );
}

/// 大纲层级过滤条。
fn level_filter(app: &mut EditorApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.weak("层级");
        for level in 1..=6u8 {
            let selected = app.sidebar.outline_max_level == level;
            if ui.selectable_label(selected, level.to_string()).clicked() && !selected {
                app.sidebar.set_outline_max_level(level);
            }
        }
    });
}
