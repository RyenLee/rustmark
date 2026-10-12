//! 全局搜索标签：搜索框、选项开关、命中分组渲染与跳转。

use egui::{Align, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::sidebar_ui_state::{SEARCH_TOGGLES, SearchGroup};
use crate::presentation::theme::builtin::Theme;

use super::files::display_name;
use super::*;

/// 搜索标签。
pub(super) fn search_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let mut query = app.sidebar.search_query.clone();
    let response = ui.add(
        egui::TextEdit::singleline(&mut query)
            .id(egui::Id::new(SEARCH_INPUT_ID))
            .hint_text("在所有文件中搜索")
            .desired_width(ui.available_width()),
    );
    if response.changed() {
        app.sidebar.search_query = query;
    }
    toggle_row(app, ui, theme);
    ui.separator();

    let Some(result) = app.search_workspace() else {
        ui.colored_label(theme.error, "搜索失败，请检查工作区目录");
        return;
    };
    if !app.sidebar.should_search() {
        ui.weak("输入关键词以在工作区所有文件中搜索");
        return;
    }
    if result.is_empty() {
        ui.weak(format!(
            "没有找到匹配「{}」的内容",
            app.sidebar.search_query
        ));
        return;
    }
    let mut jump: Option<(String, usize)> = None;
    ScrollArea::vertical()
        .id_salt("rustmark.sidebar.search_results")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for group in &result.groups {
                group_header(ui, theme, group);
                for hit in &group.hits {
                    if hit_row(ui, theme, group, hit.line, &hit.snippet, hit.highlight).clicked() {
                        jump = Some((group.path.clone(), hit.line));
                    }
                }
            }
            if result.truncated {
                ui.colored_label(theme.warning, "结果较多，已显示前 300 条，请缩小搜索范围");
            }
        });
    if let Some((path, line)) = jump {
        jump_to_hit(app, &path, line);
    }
}

/// 三个搜索开关。
///
/// 开关是**双态**的：选中后再点一次是取消，不是无操作（旧实现里
/// `clicked() && !on` 把「已开启时再点」整个吞掉，开关点不灭）。
fn toggle_row(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let options = app.sidebar.search_options;
    ui.horizontal(|ui| {
        for toggle in SEARCH_TOGGLES {
            let on = toggle.is_on(&options);
            let text = RichText::new(toggle.badge()).color(if on {
                theme.accent
            } else {
                theme.text_secondary
            });
            if ui
                .selectable_label(on, text)
                .on_hover_text(toggle.hint())
                .clicked()
            {
                app.sidebar.search_options = options.toggled(toggle);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 一键重置：三个开关全部关闭。已全关时置灰（点了没反应会让人
            // 以为按钮坏了，置灰则明示「已是默认状态」）。
            ui.add_enabled(!options.is_plain(), egui::Button::new("重置").small())
                .on_hover_text("关闭全部搜索选项（区分大小写 / 全字匹配 / 正则）");
            ui.weak(format!("{} 条结果", count_hits(app)));
        });
    });
}

/// 命中总数（开关行右侧的计数）。
///
/// 复用 `search_workspace` 的结果而不是再查一次：每次点击开关都重扫全盘
/// 会让界面在点「Aa」时卡一下。代价是这一帧的计数用的是**当前查询**的结果——
/// 而这正是用户要看到的数字。
fn count_hits(app: &mut EditorApp) -> usize {
    app.search_workspace().map(|r| r.total()).unwrap_or(0)
}

/// 分组头（文件名 + 命中数）。
fn group_header(ui: &mut Ui, theme: &Theme, group: &SearchGroup) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&group.file_name).color(theme.text_primary));
        ui.weak(format!("{}", group.hits.len()));
    });
}

/// 单条命中（行号 + 片段，高亮命中词）。
fn hit_row(
    ui: &mut Ui,
    theme: &Theme,
    _group: &SearchGroup,
    line: usize,
    snippet: &str,
    highlight: (usize, usize),
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_space(ROW_PAD);
        ui.weak(format!("{}", line + 1));
        let chars: Vec<char> = snippet.chars().collect();
        let (start, end) = highlight;
        let end = end.min(chars.len());
        let start = start.min(end);
        if start == end {
            ui.label(chars.iter().collect::<String>());
        } else {
            ui.label(RichText::new(chars[..start].iter().collect::<String>()));
            ui.label(
                RichText::new(chars[start..end].iter().collect::<String>())
                    .background_color(theme.find_current_bg),
            );
            ui.label(chars[end..].iter().collect::<String>());
        }
    })
    .response
}

/// 跳到某文件某行（先切文件，再按行号定位）。
fn jump_to_hit(app: &mut EditorApp, path: &str, line: usize) {
    let already_open = app.current_document_path().eq_ignore_ascii_case(path);
    if !already_open && let Err(e) = app.open_workspace_file(path) {
        app.sidebar
            .fail(format!("无法打开 {}：{e}", display_name(path)));
        return;
    }
    // 行号 → 字节偏移，与大纲跳转走同一个换算，不各算一遍。
    let starts = crate::presentation::views::scroll_sync::line_start_offsets(&app.editor_text);
    let Some(&offset) = starts.get(line) else {
        return;
    };
    app.goto_byte_offset(offset);
}
