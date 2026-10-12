//! 侧栏视图（S6-11）：文件树 / 大纲 / 在文件中查找 / 格式检查四个标签页。
//!
//! # 四个标签的数据来源各不相同，别搞混
//!
//! | 标签 | 数据来源 | 刷新时机 |
//! |---|---|---|
//! | 文件 | `EditorApp::workspace_entries`（已装载快照） | 打开文件夹 / 文件操作后 |
//! | 大纲 | `EditorApp::current_outline()`（每次现算） | 文档内容变化时自动跟随 |
//! | 搜索 | `EditorApp::search_workspace()`（每次真扫盘） | 每次显示搜索标签时 |
//! | 检查 | `EditorApp::lint_current_document()`（每次现算，V11-08） | 每次显示检查标签时 |
//!
//! 文件树**不**每帧扫盘（那是递归 `read_dir`）；大纲、搜索、检查**每帧**现算
//! （大纲只解析已缓存的块，搜索有 300 条命中上限兜底，检查是 O(行数) 线性扫描）。
//! 「哪些该缓存、哪些不该」的判据是：**代价高且能被显式失效**才缓存。
//!
//! # 右键菜单为什么自己画而不用 `egui::ContextMenu`
//!
//! egui 0.33 **没有** `ContextMenu`（只有 `MenuButton`，那是给菜单栏用的）。
//! 且菜单需要「记住作用于哪一行」，这天然是状态，故放在
//! [`SidebarUiState::context_menu_path`] 而不是 egui 的 popup id。
//!
//! # 模块结构
//!
//! facade 保留侧栏骨架（show/draw/tab_bar）与底部提示；文件树在 `files`，
//! 大纲在 `outline`，格式检查在 `lint`，全局搜索在 `search`。

mod files;
mod lint;
mod outline;
mod search;

use egui::{Align, Context, Layout, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::app_ui_state::SidebarTab;
use crate::presentation::theme::builtin::Theme;

/// 每行高度（对齐原型 `.row { height: 24px }`）。
const ROW_HEIGHT: f32 = 24.0;
/// 层级缩进步进（对齐原型 `row.depth * 14`）。
const INDENT_STEP: f32 = 14.0;
/// 行左侧基础内边距（对齐原型 `paddingLeft: 6`）。
const ROW_PAD: f32 = 6.0;
/// 大纲行右侧**行号预留宽**（点）。
///
/// 标题占位时要把它从可用宽里扣掉，右侧的行号才有地方站；超过四位数的
/// 行号（10000+ 行的文档）会向左侵占标题的截断省略号，可接受。
const OUTLINE_LINE_NO_W: f32 = 30.0;
/// 搜索输入框的固定 `Id`（焦点落点 + 可测锚点）。
pub const SEARCH_INPUT_ID: &str = "rustmark.sidebar.search";

/// 绘制侧栏（作为 `SidePanel::left` 的一部分）。
///
/// 宽度由 `main_view` 用 `exact_width` 固定，这里不再重复决定——
/// 宽度若在两处各算一次，改主题尺寸时必然只改到一处。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let width = app.theme().metrics.sidebar_width;
    egui::SidePanel::left("rustmark.sidebar")
        .exact_width(width)
        .resizable(false)
        .show(ctx, |ui| draw(app, ui, ctx));
}

fn draw(app: &mut EditorApp, ui: &mut Ui, ctx: &Context) {
    let theme = app.theme().clone();
    ui.painter()
        .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, theme.bg_secondary);
    tab_bar(app, ui, &theme);
    ui.separator();
    // 切到搜索标签时把焦点交给搜索框：搜索的全部交互都从这个框开始，
    // 让用户切过来还要再点一次才输得进字，会以为这个标签坏了。
    //
    // 判据是「上一帧画的不是搜索」而不是「当前没有焦点」：编辑区
    // 同样没有焦点，若无差别请求，用户在搜索结果里点开一个文件后
    // 焦点会被侧栏抢回去，键盘输入直接打到搜索框里。
    let want_search_focus = app.sidebar.search_just_activated(app.ui.sidebar_tab);
    match app.ui.sidebar_tab {
        SidebarTab::Files => files::files_tab(app, ui, &theme),
        SidebarTab::Outline => outline::outline_tab(app, ui, &theme),
        SidebarTab::Search => search::search_tab(app, ui, &theme),
        SidebarTab::Lint => lint::lint_tab(app, ui, &theme),
    }
    messages(app, ui, &theme);
    if want_search_focus && !ctx.memory(|m| m.has_focus(egui::Id::new(SEARCH_INPUT_ID))) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(SEARCH_INPUT_ID)));
    }
    let current = app.ui.sidebar_tab;
    app.sidebar.mark_drawn(current);
}

/// 顶部标签栏。
fn tab_bar(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let m = theme.metrics;
    // 三条顶栏里的第一条：与编辑区工具栏、预览区标题栏**同一条规矩**——
    // 同高（`editor_toolbar_height`）、内容垂直居中、栏下同一根分隔线。
    // 此前标签行只取控件默认高（实测 23，文字中心 y=113.5），而两条
    // 34 高的栏文字中心在 y=119，三个顶栏互相对不齐。
    ui.allocate_ui(
        egui::vec2(ui.available_width(), m.editor_toolbar_height),
        |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // 左缘取刻度 `space_sm`：与文件树行的 `ROW_PAD`（同为 6）对齐，
                // 标签文字与树行文字在同一条竖线上，侧栏才不显得「上歪下正」。
                ui.add_space(m.space_sm);
                for tab in SidebarTab::ALL {
                    let selected = app.ui.sidebar_tab == *tab;
                    if ui.selectable_label(selected, tab.label()).clicked() {
                        app.ui.toggle_sidebar_tab(*tab);
                    }
                }
                // 「打开文件夹」入口已上移到顶部「文件」菜单（见
                // `UiAction::OpenFolder`），侧栏不再重复提供——同一功能
                // 两个入口会让「哪个才是正路」变得含糊。
                //
                // 只保留「刷新」，贴右缘：树是快照，文件也会在本应用之外被改
                // （别的编辑器、git 拉取、构建产物被清理）。不给刷新入口，
                // 用户会看到「树里有这个文件但点开说文件不存在」，像是应用坏了。
                if app.workspace_entries.is_loaded()
                    && ui
                        .with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(m.space_sm);
                            ui.small_button("刷新").clicked()
                        })
                        .inner
                {
                    app.refresh_workspace();
                }
            });
        },
    );
    // 与编辑区工具栏、预览区标题栏同款栏下分隔线（同一根水平线）。
    ui.separator();
    if !app.workspace_entries.is_loaded() && app.ui.sidebar_tab == SidebarTab::Files {
        ui.colored_label(
            theme.text_secondary,
            "尚未打开工作区目录，请用顶部「文件」菜单里的「打开文件夹」开始。",
        );
    }
}

/// 底部提示行。
fn messages(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    if let Some(error) = app.sidebar.error.clone() {
        ui.colored_label(theme.error, format!("⚠ {error}"));
    }
    if let Some(notice) = app.sidebar.notice.clone() {
        ui.colored_label(theme.success, notice);
    }
}
