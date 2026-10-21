//! 快捷键一览浮层（S6-11f，对齐原型 `WorkbenchView.vue` 的快捷键弹窗）。
//!
//! # 三处显示同源（原型缺陷 B-08 根治）
//!
//! 菜单、命令面板、本浮层的键位**全部**来自
//! [`ShortcutRegistry::display_for`]（经 `ActionTable::entry`）。
//! 用户在设置里改键后三处同时变化——这不是「记得同步」，
//! 而是**只有一份数据**。
//!
//! # 分组沿用 `MenuGroup` 而不是另造
//!
//! 菜单已经按 `MenuGroup` 分组了。一览若另造一套（「全局 / 编辑区内 /
//! 标签页」，原型就是这么分的），用户会发现「菜单里挨着的两个操作
//! 在一览里隔了两段」——同一件事有两种分类法必然对不上。

use egui::{Align, Context, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::shortcuts::registry::{MenuGroup, UiAction};
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::state::shortcut_overview_ui_state::{
    ShortcutOverviewUiState, ShortcutSection,
};
use crate::presentation::theme::builtin::Theme;

/// 对话框默认尺寸（对齐原型 `BaseModal :width="440" :height="520"`）。
const WIDTH: f32 = 460.0;
const HEIGHT: f32 = 520.0;

/// 过滤框的固定 `Id`（打开后的焦点落点 + 可测锚点）。
pub const FILTER_INPUT_ID: &str = "rustmark.shortcut_overview.filter";

/// 绘制快捷键一览浮层。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::ShortcutOverview.title())
        .id(egui::Id::unique("rustmark.overlay.shortcut_overview"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    // 焦点只在「浮层刚打开」时给一次，否则会让用户无法把焦点移到过滤框
    // 之外（表现为「点哪儿都不上焦点」）。
    //
    // 判据必须是**打开事件**（`take_open_focus` 的边沿），而不是
    // 「过滤框当前没有焦点」——后者恒为真：`Memory::has_focus` 认不到
    // 窗口（`Area`）的 id。详见 `DialogUiState::take_open_focus` 的文档。
    if app.dialog.take_open_focus(Overlay::ShortcutOverview) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique(FILTER_INPUT_ID)));
    }
    if !open {
        app.dialog.close(Overlay::ShortcutOverview);
    }
}

/// 主体。
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    filter_row(app, ui);
    ui.add_space(theme.metrics.space_sm);
    let sections = collect(app);
    list(ui, &theme, &sections);
    ui.separator();
    footer(app, ui, &theme, &sections);
}

/// 过滤框。
fn filter_row(app: &mut EditorApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("筛选").size(12.0));
        let mut query = app.shortcut_overview_ui.query.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut query)
                    .id(egui::Id::unique(FILTER_INPUT_ID))
                    .hint_text("按操作名或关键字筛选")
                    .desired_width(ui.available_width() - 48.0),
            )
            .changed()
        {
            app.set_shortcut_overview_query(query);
        }
    });
}

/// 从**生效**注册表收集分组。
pub fn collect(app: &EditorApp) -> Vec<ShortcutSection> {
    let table = app.dispatcher().registry().table();
    let groups: Vec<(MenuGroup, Vec<_>)> = MenuGroup::ALL
        .iter()
        .map(|group| (*group, table.group(*group)))
        .collect();
    crate::presentation::state::shortcut_overview_ui_state::sections(
        groups,
        app.shortcut_overview_ui.query(),
    )
}

/// 分组列表。
fn list(ui: &mut Ui, theme: &Theme, sections: &[ShortcutSection]) {
    ScrollArea::vertical()
        .id_salt("rustmark.shortcut_overview.list")
        .max_height(HEIGHT - 150.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let total: usize = sections.iter().map(|s| s.rows.len()).sum();
            if total == 0 {
                ui.add_space(theme.metrics.space_xl);
                ui.centered_and_justified(|ui| {
                    ui.colored_label(theme.text_secondary, "没有匹配的操作。");
                });
                return;
            }
            for section in sections {
                if section.is_empty() {
                    continue;
                }
                ui.add_space(theme.metrics.space_sm);
                ui.colored_label(theme.accent, section.group.label());
                ui.separator();
                for row in &section.rows {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(row.label).size(12.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if row.combo.is_empty() {
                                // 空键位显示「未绑定」而不是留白：
                                // 留白看起来像没渲染出来。
                                ui.colored_label(
                                    theme.text_secondary.gamma_multiply(0.8),
                                    "未绑定",
                                );
                            } else {
                                kbd(ui, theme, &row.combo);
                            }
                        });
                    });
                }
            }
        });
}

/// 键位小标签。
fn kbd(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.monospace(RichText::new(text).size(11.0).color(theme.text_primary));
}

/// 页脚：统计与关闭。
fn footer(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, sections: &[ShortcutSection]) {
    let shown: usize = sections.iter().map(|s| s.rows.len()).sum();
    ui.horizontal(|ui| {
        ui.colored_label(
            theme.text_secondary,
            format!("显示 {shown} / {} 项操作", UiAction::ALL.len()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("关闭").clicked() {
                app.dialog.close(Overlay::ShortcutOverview);
            }
        });
    });
}

/// 打开时重置过滤词：`execute` → `open_overlay` 走这一条。
///
/// 视图里的「关闭」按钮**不**调它——关闭是收尾，重置是开场，
/// 混在关闭路径里会让「打开时重置」这件事的归属变得要看代码才知道。
/// 需要在测试里验证重置行为时，直接调本函数。
pub fn reset_filter_on_open(state: &mut ShortcutOverviewUiState) {
    state.open();
}
