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
    // 没打开文档时先说清作用域（V11-28）：检查是文档级能力，空工作区
    // 下显示「无问题」会让人误以为全盘已检，其实根本没东西可检。
    if app.active_doc.is_none() {
        ui.weak("没有打开的文档");
        ui.weak("打开一个 .md 后，这里会列出它发现的格式问题。");
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
///
/// # 说明文字必须截断，不能放任溢出（问题 024）
///
/// `Layout::left_to_right` 里的裸 `label` **不换行**，说明一长就会把本行的
/// `min_rect` 撑出面板（内容区右界 232pt，实测溢到 407.9pt）。而 egui 的
/// `SidePanel` 给后续布局保留的是**内容 response.rect 的最大 x**（egui
/// `panel.rs` 的 `allocate_left_panel`），于是侧栏实际占掉 [0, 415.9]——
/// [240, 415.9] 之间没有任何面板绘制，真实渲染下露出交换链清屏色（黑），
/// 编辑区也被整体推右。用户看到的正是「切到检查后，侧栏右侧与编辑区之间
/// 一条竖向黑框」。
///
/// 修法与大纲行同款（见 `outline.rs`）：说明占「剩余宽 − 行号预留」并
/// `truncate()`，超出部分显示省略号，绝不把布局撑出面板。
///
/// # 整行必须显式带 `Sense::click()`（问题 1）
///
/// `allocate_ui` 与 `horizontal` 一样走 `scope`，子 Ui 的默认 sense 是
/// **hover**（`UiBuilder::sense` 默认值），其 `Response::clicked()` 恒为
/// false——症状是「检查结果行点击无响应」。故这里不用 `allocate_ui`，
/// 改用 `scope_builder` + `max_rect` 显式声明点击语义。
fn lint_row(
    ui: &mut Ui,
    theme: &Theme,
    issue: &crate::application::dto::LintIssueView,
) -> egui::Response {
    let row_rect = egui::Rect::from_min_size(
        ui.cursor().min,
        egui::vec2(ui.available_width(), LINT_ROW_HEIGHT),
    );
    // 整行显式声明 `Sense::click()`：规则名 + 说明 + 行号多段 `Ui` 默认只有
    // hover sense，会盖在行交互之上、吞掉点击（与大纲行同款缺陷）。先
    // `with_layout` 排出内容，再用 `ui.interact` 在 `row_rect` 上挂 click-sense
    // 命中区（问题 1：检查结果行点击无响应）。
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(ROW_PAD);
        ui.label(RichText::new(&issue.rule_label).color(theme.text_primary));
        // 说明占「剩余宽 − 行号预留」并**截断**：行号预留与大纲同款
        // `OUTLINE_LINE_NO_W`，超长行号向左侵吞省略号，可接受（侧栏
        // 宽度固定，省略号是唯一不失真的降级）。
        let avail = (ui.available_width() - OUTLINE_LINE_NO_W).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(avail, 0.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&issue.message).color(theme.text_secondary))
                        .truncate(),
                );
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 行号展示按人类习惯 +1（DTO 里是 0-based，见 `LintIssueView`）。
            ui.add_space(ROW_PAD);
            ui.weak((issue.line + 1).to_string());
        });
    });
    let row_id = ui.scope_id().with((
        "rustmark.lint.row",
        issue.line,
        &issue.rule_label,
        &issue.message,
    ));
    ui.interact(row_rect, row_id, egui::Sense::click())
}
