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
//! # 作用域分组（V11-28）
//!
//! 四个标签的作用域并不同级：「文件」是**工作区级**，大纲 / 搜索 / 检查是
//! **文档级**。它们不再平铺在同一行标签栏里，而是收进左侧**活动栏**
//! （按作用域分两组，组间一条分隔线），右侧是单一内容面板——面板标题
//! 带作用域声明（如「大纲 · 当前文档」），无打开文档时文档级面板明确显空状态。
//!
//! 「搜索」内部还有一个范围切换器（当前文档 / 打开的文档）：
//! 它决定 `search_workspace` 把哪个文档 ID 交给应用层，无活动文档时
//! 「当前文档」范围返回空结果（见该方法文档），视图只负责空状态文案。
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
/// 左侧活动栏（按作用域分组的纵向导航列）的固定宽。
///
/// 设计要点 V11-28：活动栏 48pt；内容面板据此得到 `sidebar_width - 48` 的可用宽。
/// 固定宽才能让水平布局把剩余宽干净地让给内容面板，避免两边抢宽。
const ACTIVITY_BAR_W: f32 = 48.0;

/// 绘制侧栏（作为 `SidePanel::left` 的一部分）。
///
/// 宽度由调用方（`main_view`）算好传入——它同时要把同一个宽度用在
/// 侧栏分割条上，两处各算一次必然漂移。宽度若在两处各算一次，
/// 改主题尺寸时必然只改到一处（同 `Metrics` 单一真源的理由）。
///
/// 传入的是**生效宽**（已按窗口做动态夹紧），不是 `AppUiState` 里
/// 的用户值：窄窗口下两者不相等，面板必须按生效宽排，否则会超出
/// 「侧栏 ≤ 主体区 60%」的约束。
pub fn show(app: &mut EditorApp, ui: &mut Ui, width: f32) {
    let ctx = ui.ctx().clone();
    egui::Panel::left("rustmark.sidebar")
        .exact_size(width)
        .resizable(false)
        .show(ui, |ui| draw(app, ui, &ctx));
}

fn draw(app: &mut EditorApp, ui: &mut Ui, ctx: &Context) {
    let theme = app.theme().clone();
    ui.painter()
        .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, theme.bg_secondary);
    // 活动栏（按作用域分两组）+ 单内容面板：纯展示层重组，
    // 「文件」是工作区级、「大纲 / 搜索 / 检查」是文档级，分组一眼可辨。
    // 任何一行都靠截断把自己限制在可用宽内——水平布局里放多个不截断的
    // 控件会撑爆面板（见 025 / 033），活动栏本身只放固定宽的导航项。
    //
    // 必须用 `horizontal_top` 而不是 `horizontal`：后者把子 Ui 的初始高
    // 定为 `spacing.interact_size.y`（18pt）、交叉轴居中（egui 0.33 文档
    // 原话），活动栏与内容面板会被整体压进这 18pt 里——背景条到半屏
    // 就断掉、大纲提示溢出面板之外（V11-29 截图）。`horizontal_top`
    // 初始高取 `available_size_before_wrap()`（面板全高），内容顶对齐。
    ui.horizontal_top(|ui| {
        ui.allocate_ui(egui::vec2(ACTIVITY_BAR_W, ui.available_height()), |ui| {
            activity_bar(app, ui, &theme)
        });
        ui.separator();
        content_panel(app, ui, &theme);
    });
    // 切到搜索标签时把焦点交给搜索框：搜索的全部交互都从这个框开始，
    // 让用户切过来还要再点一次才输得进字，会以为这个标签坏了。
    //
    // 判据是「上一帧画的不是搜索」而不是「当前没有焦点」：编辑区
    // 同样没有焦点，若无差别请求，用户在搜索结果里点开一个文件后
    // 焦点会被侧栏抢回去，键盘输入直接打到搜索框里。
    let want_search_focus = app.sidebar.search_just_activated(app.ui.sidebar_tab);
    if want_search_focus && !ctx.memory(|m| m.has_focus(egui::Id::unique(SEARCH_INPUT_ID))) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique(SEARCH_INPUT_ID)));
    }
    let current = app.ui.sidebar_tab;
    app.sidebar.mark_drawn(current);
}

/// 左侧活动栏：纵向导航，按作用域分两组（工作区级 / 文档级）。
///
/// 复用 `selectable_label` 的「选中即高亮」语言，与标签栏、大纲层级条同源；
/// 两组之间用一条细分隔线取代「四个标签平铺在同一行」——作用域不同的面板
/// 不该排在一起让人误以为是同级能力（V11-28）。
fn activity_bar(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    // 比内容面板更深的底色，把导航列从面板里视觉上分出来。
    ui.painter()
        .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, theme.bg_primary);
    ui.vertical_centered(|ui| {
        ui.add_space(6.0);
        // 工作区级：仅「文件」（资源管理器）。
        activity_entry(app, ui, SidebarTab::Files, "工作区");
        ui.separator();
        // 文档级：跟随活动文档，文档切换结果随之更新、无文档时显空状态。
        activity_entry(app, ui, SidebarTab::Outline, "当前文档");
        activity_entry(app, ui, SidebarTab::Search, "可切换范围");
        activity_entry(app, ui, SidebarTab::Lint, "当前文档");
        ui.add_space(6.0);
    });
}

/// 活动栏里的一个条目：选中高亮 + 悬停提示作用域。
fn activity_entry(app: &mut EditorApp, ui: &mut Ui, tab: SidebarTab, scope_hint: &str) {
    let selected = app.ui.sidebar_tab == tab;
    let response = ui.selectable_label(selected, tab.label());
    if response.clicked() {
        app.ui.toggle_sidebar_tab(tab);
        // 菜单只属于「文件」面板；切走后面板不再渲染它，回来时它还挂着
        // 会显得凭空出现。切标签即关闭（与原型「点击窗口任意处关菜单」一致）。
        app.sidebar.close_context_menu();
    }
    response.on_hover_text(format!("{} · {}", tab.label(), scope_hint));
}

/// 右侧内容面板：顶部作用域声明标题 + 当前标签内容 + 底部提示。
fn content_panel(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let current = app.ui.sidebar_tab;
    ui.vertical(|ui| {
        content_header(app, ui, theme, current);
        ui.separator();
        match current {
            SidebarTab::Files => {
                if app.workspace_entries.is_loaded() {
                    files::files_tab(app, ui, theme);
                } else {
                    ui.colored_label(
                        theme.text_secondary,
                        "尚未打开工作区目录，请用顶部「文件」菜单里的「打开文件夹」开始。",
                    );
                }
            }
            SidebarTab::Outline => outline::outline_tab(app, ui, theme),
            SidebarTab::Search => search::search_tab(app, ui, theme),
            SidebarTab::Lint => lint::lint_tab(app, ui, theme),
        }
        messages(app, ui, theme);
    });
}

/// 内容面板顶部的作用域声明标题（设计要点：标题带作用域，文档级面板
/// 在「无文档」时明确告知，不再让人以为面板坏了）。
///
/// 「搜索」的精确范围由 `search` 标签内的切换行声明，这里只给面板名，
/// 避免与作用域切换行上的「当前文档 / 打开的文档」重复出现、干扰点击。
fn content_header(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, tab: SidebarTab) {
    let m = theme.metrics;
    let scope: Option<String> = match tab {
        SidebarTab::Files => Some("工作区".into()),
        SidebarTab::Outline | SidebarTab::Lint => Some(
            if app.active_doc.is_some() {
                "当前文档"
            } else {
                "无文档"
            }
            .into(),
        ),
        SidebarTab::Search => None,
    };
    ui.allocate_ui(
        egui::vec2(ui.available_width(), m.editor_toolbar_height),
        |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(m.space_sm);
                ui.strong(tab.label());
                if let Some(ref s) = scope {
                    ui.add_space(4.0);
                    ui.weak(s);
                }
                // 「文件」面板专属的刷新入口：树是快照，外部改动不会自动进树。
                if tab == SidebarTab::Files && app.workspace_entries.is_loaded() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(m.space_sm);
                        if ui.small_button("刷新").clicked() {
                            app.refresh_workspace();
                        }
                    });
                }
            });
        },
    );
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
