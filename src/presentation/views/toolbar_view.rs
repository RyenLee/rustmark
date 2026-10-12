//! 顶部区（全局顶栏）与编辑区顶部：**应用级菜单栏** + 标签条 + 路径面包屑
//! （全局顶栏，S6-07），以及编辑区顶部的**编辑区工具栏**（见 `editor_view::show`），
//! 并承载 S6-02 的标签条与路径面包屑。
//!
//! # 应用级菜单与编辑区工具栏分处不同区域
//!
//! 命令按**作用域**分处两处，而不是把一切挤在顶栏一行里：
//!
//! | 区域 | 内容 | 作用域 | 形态 |
//! |---|---|---|---|
//! | 全局顶栏 `draw_app_menu_bar` | `MenuGroup::APP_LEVEL` | 整个应用（打开 / 保存 / 视图开关 / 设置） | 全部是**下拉菜单** |
//! | 编辑区顶部 `draw_editor_toolbar` | `MenuGroup::Format` | 当前编辑区里的**内容**（加粗 / 列表 / 缩进……） | 按作用分组的**下拉菜单** |
//!
//! 混在一行时，「文件」下拉与「加粗」按钮看起来同级，职责边界随之消失：
//! 用户分不清哪些命令会改变全局状态、哪些只动光标处的文本。
//! 分层之后，「有没有下拉」恰好成了两处的区分特征——
//! 下拉是「一组命令的入口」（应用级），编辑区工具栏是「按作用分组的命令」（编辑区）。
//! 编辑区工具栏不再占用全局顶栏，而是贴在编辑区顶部（上下布局：工具栏在上、文本在下），
//! 作用域与编辑区内容一致，预览区也可独立占满剩余宽度。
//!
//! # 本文件的核心约束：三入口行为一致（S6-07 验收）
//!
//! 菜单项、快捷键、命令面板三者**必须**给出同样的结果，否则用户会看到
//! 「菜单里能做的事，快捷键做不到」。原型正是在这里出的问题（B-02：
//! `Ctrl+Shift+S` 写在菜单里却从未注册）。
//!
//! 本项目的根治方式不是「记得两边都写」，而是**结构上不可能漏**：
//! 菜单项与工具栏命令本身都是遍历 `MenuGroup` × `UiAction::ALL` 生成的，
//! 文案与键位都来自同一个 `UiAction` 的 `label()` / `ShortcutRegistry::display_for()`。
//! 想让某个动作不上菜单，只能从动作表里删它——那样快捷键和命令面板会**同时**消失。
//!
//! # 模块结构
//!
//! facade 保留顶栏入口 `show` 与标签条、面包屑；应用级菜单栏在 `menu_bar`，
//! 编辑区工具栏与控件视觉规范在 `controls`。

mod controls;
mod menu_bar;

pub use controls::{
    FLAT_GROUP, button_text, draw_editor_toolbar, editor_toolbar_groups, toolbar_control_labels,
};
pub use menu_bar::{MENU_LABEL_SHORTCUT_GAP, MENU_ROW_PAD, menu_row_width};

use egui::{Context, Ui};

use crate::app::frame::EditorApp;

/// 绘制全局顶栏：应用级菜单栏 + 标签条 + 路径面包屑。
///
/// 编辑区工具栏已移到编辑区顶部（见 `editor_view::show`），不再占用此处高度。
/// 垂直预算统一从 [`Metrics`](crate::presentation::theme::builtin::Metrics) 取：
/// 应用级菜单栏占 `topbar_height`、标签条占 `tabstrip_height`、面包屑占 `tabbar_height`。
/// 三者之和即顶部面板高度，改主题尺寸时只有一条通路
/// （避免「菜单栏改了，别处没改」的错位）。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let metrics = app.theme().metrics;
    let height = metrics.topbar_height + metrics.tabstrip_height + metrics.tabbar_height;
    egui::TopBottomPanel::top("rustmark.topbar")
        .exact_height(height)
        .show(ctx, |ui| {
            menu_bar::draw_app_menu_bar(app, ui);
            ui.separator();
            draw_tab_strip(app, ui);
            draw_breadcrumb(app, ui);
        });
}

/// 标签条：渲染所有打开的文档，可点击切换、关闭、并显示脏标记。
fn draw_tab_strip(app: &mut EditorApp, ui: &mut Ui) {
    let docs = app.open_document_list();
    ui.horizontal_wrapped(|ui| {
        for doc in &docs {
            let title = if doc.title.is_empty() {
                "未命名"
            } else {
                doc.title.as_str()
            };
            let active = app.active_doc == Some(doc.id);
            let response = ui.selectable_label(active, title);
            if response.clicked() {
                app.switch_document(doc.id);
            }
            if doc.is_dirty {
                ui.weak("●");
            }
            // 关闭按钮：用极小的「×」；点击后关闭该标签并结束本帧渲染
            // （避免继续遍历已失效的 `docs` 列表造成重复关闭）。
            let close = ui.small_button("×");
            if close.clicked() {
                // 关掉**最后一个**标签时回到欢迎页，而不是什么都不做。
                // `close_document` 拒绝关最后一个（「永远至少一页可编辑」），
                // 那个约束保护的是「编辑区突然变成空白灰板」；
                // 欢迎页正是那个灰板的替代品，故此处显式走全关路径。
                if app.open_document_list().len() <= 1 {
                    app.close_all_documents();
                } else {
                    app.close_document(doc.id);
                }
                return;
            }
        }
    });
}

/// 路径面包屑：显示当前文档的保存路径（未保存显示「未命名文档」）。
fn draw_breadcrumb(app: &EditorApp, ui: &mut Ui) {
    let path = app.current_document_path();
    ui.horizontal(|ui| {
        ui.weak("📄");
        ui.label(path);
    });
}
