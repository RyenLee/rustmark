//! 大纲标签：标题层级过滤与点击跳转。

use egui::{Align, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::widgets;

use super::*;

/// 大纲标签。
pub(super) fn outline_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    level_filter(app, ui);
    // 提示放在列表**上方**：下面的滚动区 `auto_shrink([false, false])`
    // 会吃掉全部剩余高，若放在它后面，提示会被推出面板底缘、永远
    // 不可见（036 修复时发现，旧版同样存在——压扁缺陷反而让它「溢出
    // 可见」过）。控件行 + 指引 + 列表，从上往下正好是阅读顺序。
    ui.colored_label(
        theme.text_secondary,
        "点击标题可跳转；标题级别的调整请在编辑区用 Ctrl+[ / Ctrl+]。",
    );
    ui.separator();
    let items = app.current_outline();
    if items.is_empty() {
        // 先区分「没打开文档」与「文档里真没标题」：前者是作用域问题，
        // 提示应指路去打开文件，而不是让人以为大纲坏了（V11-28）。
        if app.active_doc.is_none() {
            ui.weak("没有打开的文档");
            ui.weak("打开一个 .md 后，这里会列出它的标题层级。");
        } else {
            ui.weak("当前文档没有标题");
        }
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
                // 「整行可点」与「限宽省略」两条不变式都由 `widgets` 承担：
                // 前者保证行内容（多段 hover sense）不吞掉点击，后者保证段高
                // 跟随行首实测值而不是 0（问题 065 的错位根因）。
                let (response, _) = widgets::row::clickable_row(
                    ui,
                    ("rustmark.outline.row", item.line, &item.text),
                    |ui| {
                        ui.add_space(ROW_PAD + (item.level.saturating_sub(1)) as f32 * INDENT_STEP);
                        let hash = ui.label(
                            RichText::new("#".repeat(item.level as usize))
                                .color(theme.text_secondary),
                        );
                        // 标题占「剩余宽 − 行号预留」并截断：过长的标题不再把
                        // 固定宽的侧栏撑出黑边（SidePanel 是 `exact_width`，
                        // 溢出内容只会被裁掉）。
                        widgets::text::elided_segment(ui, hash.rect, OUTLINE_LINE_NO_W, text);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // `item.line` 是 0-based（与源码偏移同源），状态栏与用户数行
                            // 都按 1-based，故显示时 +1，使大纲行号与正文行号对齐（#12）。
                            ui.weak((item.line + 1).to_string());
                        });
                    },
                );
                if response.clicked() {
                    app.sidebar.outline_selected = Some(item.text.clone());
                    jump = Some(item.line);
                }
            }
        });
    if let Some(line) = jump {
        app.goto_outline_line(line);
    }
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
