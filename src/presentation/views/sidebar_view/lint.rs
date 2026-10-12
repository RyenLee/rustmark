//! 格式检查（lint，V11-08）标签：规则空态、问题列表与行跳转。

use egui::{Align, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::theme::builtin::Theme;

use super::*;

/// 检查结果行高（比文件树的 24 略高：一行放下「规则名 + 说明」两段文字）。
const LINT_ROW_HEIGHT: f32 = 34.0;

/// 检查标签。
///
/// 与大纲同源（见文件头表格）：`lint_document` 是 O(行数) 的线性扫描，
/// 每帧现算即可，不缓存。
pub(super) fn lint_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let rules = app.settings().editor.lint;
    if !rules_enabled(&rules) {
        ui.weak("所有检查规则均已关闭；可在「设置 → 编辑器」里开启。");
        return;
    }
    let issues = app.lint_current_document();
    if issues.is_empty() {
        ui.weak("未发现格式问题");
        return;
    }
    ui.weak(format!("{} 个问题，点击行可跳到问题所在行：", issues.len()));
    ScrollArea::vertical()
        .id_salt("rustmark.sidebar.lint")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for issue in &issues {
                if lint_row(ui, theme, issue).clicked() {
                    // 与大纲跳转同一条通路：光标落到该行行首（0-based 行号
                    // 由 `goto_outline_line` 统一换算）。
                    app.goto_outline_line(issue.line);
                }
            }
        });
}

/// 四条规则里是否还有开着的（决定空态文案走「无问题」还是「全关闭」）。
fn rules_enabled(rules: &crate::application::dto::LintRulesView) -> bool {
    rules.heading_jump || rules.list_marker_mix || rules.trailing_whitespace || rules.unclosed_fence
}

/// 一条检查结果：规则名 + 说明 + 右侧行号，整行可点。
fn lint_row(
    ui: &mut Ui,
    theme: &Theme,
    issue: &crate::application::dto::LintIssueView,
) -> egui::Response {
    ui.allocate_ui(egui::vec2(ui.available_width(), LINT_ROW_HEIGHT), |ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(ROW_PAD);
            ui.label(RichText::new(&issue.rule_label).color(theme.text_primary));
            ui.label(RichText::new(&issue.message).color(theme.text_secondary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // 行号展示按人类习惯 +1（DTO 里是 0-based，见 `LintIssueView`）。
                ui.add_space(ROW_PAD);
                ui.weak((issue.line + 1).to_string());
            });
        });
        ui.response()
    })
    .inner
}
