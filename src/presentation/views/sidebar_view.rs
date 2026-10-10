//! 侧栏视图（S6-11）：文件树 / 大纲 / 在文件中查找三个标签页。
//!
//! # 三个标签的数据来源各不相同，别搞混
//!
//! | 标签 | 数据来源 | 刷新时机 |
//! |---|---|---|
//! | 文件 | `EditorApp::workspace_entries`（已装载快照） | 打开文件夹 / 文件操作后 |
//! | 大纲 | `EditorApp::current_outline()`（每次现算） | 文档内容变化时自动跟随 |
//! | 搜索 | `EditorApp::search_workspace()`（每次真扫盘） | 每次显示搜索标签时 |
//!
//! 文件树**不**每帧扫盘（那是递归 `read_dir`），大纲与搜索**每帧**现算
//! （前者只解析已缓存的块，后者有 300 条命中上限兜底）。
//! 「哪些该缓存、哪些不该」的判据是：**代价高且能被显式失效**才缓存。
//!
//! # 右键菜单为什么自己画而不用 `egui::ContextMenu`
//!
//! egui 0.33 **没有** `ContextMenu`（只有 `MenuButton`，那是给菜单栏用的）。
//! 且菜单需要「记住作用于哪一行」，这天然是状态，故放在
//! [`SidebarUiState::context_menu_path`] 而不是 egui 的 popup id。

use egui::{Align, Context, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::app_ui_state::SidebarTab;
use crate::presentation::state::sidebar_ui_state::{
    NameEdit, SEARCH_TOGGLES, SearchGroup, TreeRow, tree_rows,
};
use crate::presentation::theme::builtin::Theme;

/// 每行高度（对齐原型 `.row { height: 24px }`）。
const ROW_HEIGHT: f32 = 24.0;
/// 层级缩进步进（对齐原型 `row.depth * 14`）。
const INDENT_STEP: f32 = 14.0;
/// 行左侧基础内边距（对齐原型 `paddingLeft: 6`）。
const ROW_PAD: f32 = 6.0;
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
        SidebarTab::Files => files_tab(app, ui, &theme),
        SidebarTab::Outline => outline_tab(app, ui, &theme),
        SidebarTab::Search => search_tab(app, ui, &theme),
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
    ui.horizontal_wrapped(|ui| {
        // 左缘取刻度 `space_sm`：与文件树行的 `ROW_PAD`（同为 6）对齐，
        // 标签文字与树行文字在同一条竖线上，侧栏才不显得「上歪下正」。
        ui.add_space(theme.metrics.space_sm);
        for tab in SidebarTab::ALL {
            let selected = app.ui.sidebar_tab == *tab;
            if ui.selectable_label(selected, tab.label()).clicked() {
                app.ui.toggle_sidebar_tab(*tab);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 「打开文件夹」入口已上移到顶部「文件」菜单（见 [`UiAction::OpenFolder`]），
            // 侧栏不再重复提供——同一功能两个入口会让「哪个才是正路」变得含糊。
            //
            // 只保留「刷新」：树是快照，文件也会在本应用之外被改（别的编辑器、
            // git 拉取、构建产物被清理）。不给刷新入口，用户会看到「树里有这个
            // 文件但点开说文件不存在」，像是应用坏了。
            if app.workspace_entries.is_loaded() && ui.small_button("刷新").clicked() {
                app.refresh_workspace();
            }
        });
    });
    if !app.workspace_entries.is_loaded() && app.ui.sidebar_tab == SidebarTab::Files {
        ui.colored_label(
            theme.text_secondary,
            "尚未打开工作区目录，请用顶部「文件」菜单里的「打开文件夹」开始。",
        );
    }
}

// ===========================================================================
// 文件树
// ===========================================================================

/// 文件树标签。
fn files_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    if !app.workspace_entries.is_loaded() {
        return;
    }
    let root = app.workspace_root();
    let expanded: Vec<String> = app.sidebar.expanded_dirs.clone();
    let rows = tree_rows(&app.workspace_entries.items, &root, &|path| {
        expanded.iter().any(|p| p == path)
    });
    let selected = app.sidebar.selected_path.clone();
    let renaming = app.sidebar.renaming_path.clone();
    let creating = app.sidebar.creating.clone();

    let mut intent = FileIntent::default();
    ScrollArea::vertical()
        .id_salt("rustmark.sidebar.files")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // 「新建」待输入行：固定排在最前，用户点完「新��」立刻看到输入框。
            if let Some((parent, _, is_dir)) = &creating {
                name_editor(
                    ui,
                    app,
                    theme,
                    &format!("rustmark.sidebar.create.{parent}"),
                    &format!("{}", is_dir),
                );
            }
            for row in &rows {
                if row.is_editing(&renaming) {
                    name_editor(ui, app, theme, &row.path, &row.name);
                    continue;
                }
                file_row(ui, app, theme, row, &selected, &mut intent);
            }
            // 空白处左键 = 关闭菜单 / 结束编辑（原型 `window click → closeMenu`）。
            let blank = ui.available_size_before_wrap();
            let response = ui.interact(
                egui::Rect::from_min_size(ui.cursor().min, blank),
                egui::Id::new("rustmark.sidebar.blank"),
                egui::Sense::click(),
            );
            if response.clicked() {
                app.sidebar.close_context_menu();
            }
        });
    run_file_intent(app, intent);
}

/// 文件树里一次点击的**意图**（先收集，最后统一执行）。
///
/// 为什么不在循环里直接执行：点击目录要改 `expanded_dirs`，
/// 而渲染下一行时还要读它——边渲染边改会让「展开一个父目录后，
/// 它的子目录在同一帧里出现」这种时序依赖难以推理。
#[derive(Default)]
struct FileIntent {
    toggle: Option<String>,
    open: Option<String>,
    menu: Option<String>,
}

impl FileIntent {
    /// 记录「打开某文件」（后一次点击覆盖前一次，与原型一致）。
    fn request_open(&mut self, path: &str) {
        self.open = Some(path.to_string());
        self.menu = None;
    }
}

/// 一行是否正处于重命名编辑中。
trait RowEditing {
    fn is_editing(&self, renaming: &Option<String>) -> bool;
}

impl RowEditing for TreeRow {
    fn is_editing(&self, renaming: &Option<String>) -> bool {
        renaming.as_deref() == Some(self.path.as_str())
    }
}

/// 单行文件 / 目录。
fn file_row(
    ui: &mut Ui,
    app: &mut EditorApp,
    theme: &Theme,
    row: &TreeRow,
    selected: &Option<String>,
    intent: &mut FileIntent,
) {
    let is_selected = selected.as_deref() == Some(row.path.as_str());
    let expanded = app.sidebar.is_expanded(&row.path);
    let menu_open = app.sidebar.menu_is_for(&row.path);
    let response = ui
        .horizontal(|ui| {
            ui.add_space(ROW_PAD + row.depth as f32 * INDENT_STEP);
            // 目录的展开箭头占固定宽度，文件留同样的空位——
            // 否则同级文件与目录的图标会左右错开，扫视时很难对齐。
            if row.is_expandable() {
                let arrow = if expanded { "▾" } else { "▸" };
                ui.label(RichText::new(arrow).color(theme.text_secondary));
            } else {
                ui.add_space(12.0);
            }
            ui.label(RichText::new(&row.name).color(if is_selected {
                theme.accent
            } else {
                theme.text_primary
            }));
        })
        .response;
    // 选中 / 菜单高亮画成一整条固定高度的底色（对齐原型 `.row.active`）：
    // 只给文字加底色的话，底色会紧贴字高，在 24px 栅格下像一条下划线。
    if is_selected || menu_open {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                response.rect.min,
                egui::vec2(ui.available_width(), ROW_HEIGHT),
            ),
            egui::CornerRadius::ZERO,
            if is_selected {
                theme.accent_soft
            } else {
                theme.bg_tertiary
            },
        );
    }
    if response.clicked() {
        if row.is_expandable() {
            intent.toggle = Some(row.path.clone());
        } else {
            intent.request_open(&row.path);
        }
    }
    if response.secondary_clicked() {
        intent.menu = Some(row.path.clone());
    }
    if menu_open {
        context_menu(ui, app, theme, row);
    }
}

/// 右键菜单（新建 / 新建文件夹 / 重命名 / 删除）。
fn context_menu(ui: &mut Ui, app: &mut EditorApp, theme: &Theme, row: &TreeRow) {
    let mut intent = MenuIntent::default();
    egui::Frame::new()
        .fill(theme.bg_tertiary)
        .stroke(egui::Stroke::new(1.0_f32, theme.border))
        .corner_radius(theme.metrics.radius_card)
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.set_min_width(140.0);
            if ui.button("新建文件").clicked() {
                intent.create_file = Some(row.path.clone());
            }
            if ui.button("新建文件夹").clicked() {
                intent.create_folder = Some(row.path.clone());
            }
            ui.separator();
            if ui.button("重命名").clicked() {
                intent.rename = Some(row.path.clone());
            }
            if ui
                .button(RichText::new("删除").color(theme.error))
                .clicked()
            {
                intent.delete = Some(row.path.clone());
            }
        });
    run_menu_intent(app, row, intent);
}

/// 右键菜单的一次点击意图。
#[derive(Default)]
struct MenuIntent {
    create_file: Option<String>,
    create_folder: Option<String>,
    rename: Option<String>,
    delete: Option<String>,
}

/// 执行右键菜单意图。
fn run_menu_intent(app: &mut EditorApp, row: &TreeRow, intent: MenuIntent) {
    if let Some(parent) = intent.create_file {
        app.sidebar.start_create(Some(&parent), false);
    } else if let Some(parent) = intent.create_folder {
        app.sidebar.start_create(Some(&parent), true);
    } else if intent.rename.is_some() {
        app.sidebar.start_rename(&row.path, &row.name);
    } else if let Some(path) = intent.delete {
        delete_with_feedback(app, &path);
    } else {
        return;
    }
    app.sidebar.close_context_menu();
}

/// 名字编辑行（新建与重命名共用一套渲染与提交）。
fn name_editor(ui: &mut Ui, app: &mut EditorApp, theme: &Theme, key: &str, label: &str) {
    ui.horizontal(|ui| {
        ui.add_space(ROW_PAD);
        // 编辑态才有草稿：取不到就退化为空输入框并直接返回（非编辑态本不该
        // 进入本函数），而不是 `expect` 终止进程（规范 §四-15 / V11-12）。
        let response = if let Some(draft) = app.sidebar.draft_mut() {
            ui.add(
                egui::TextEdit::singleline(draft)
                    .id(egui::Id::new(key))
                    .desired_width(ui.available_width() - 60.0),
            )
        } else {
            return;
        };
        // 提交：回车或点「√」。两者都必须走**同一段** `commit_name_edit`，
        // 否则两条路径的行为会漂移（比如回车漏了「名字为空」的检查）。
        //
        // 按钮字用 √ / × 而非 ✓ / ✕：后者（U+2713 / U+2715）**不在**任何内置字体
        // 与微软雅黑的字形覆盖内，会渲染成方框。√(U+221A) 与 ×(U+00D7) 覆盖面最广。
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let ok = ui.small_button("√");
        let cancel = ui.small_button("×");
        if enter || ok.clicked() {
            commit_name_edit(app, label);
        } else if cancel.clicked() {
            app.sidebar.cancel_edit();
        }
    });
    ui.colored_label(theme.text_secondary, "回车提交 · Esc 或 × 取消");
}

/// 提交一次名字编辑（新建或重命名）。
fn commit_name_edit(app: &mut EditorApp, label: &str) {
    let Some(edit) = app.sidebar.take_edit() else {
        return;
    };
    if !edit.has_usable_name() {
        app.sidebar.fail("名字不能为空");
        // 编辑态**不**退出：草稿已被取走，重新放回去让用户改。
        restore_draft(app, edit, label);
        return;
    }
    let name = edit.name().to_string();
    let outcome = match &edit {
        NameEdit::Create {
            parent,
            is_directory,
            ..
        } => app
            .create_entry(parent, &name, *is_directory)
            .map(|_| format!("已创建 {name}")),
        NameEdit::Rename { path, .. } => app
            .rename_entry(path, &name)
            .map(|_| format!("已重命名为 {name}")),
    };
    match outcome {
        Ok(notice) => app.sidebar.succeed(notice),
        Err(e) => {
            app.sidebar.fail(e.to_string());
            restore_draft(app, edit, label);
        }
    }
}

/// 提交失败时把草稿放回编辑态（让用户改后重试，而不是重新敲一遍）。
fn restore_draft(app: &mut EditorApp, edit: NameEdit, label: &str) {
    match edit {
        NameEdit::Create {
            parent,
            name,
            is_directory,
        } => {
            app.sidebar.creating = Some((parent, name, is_directory));
        }
        NameEdit::Rename { path, name } => {
            app.sidebar.renaming_path = Some(path);
            app.sidebar.rename_draft = name;
        }
    }
    let _ = label;
}

/// 删除并给出反馈。
fn delete_with_feedback(app: &mut EditorApp, path: &str) {
    match app.delete_entry(path) {
        Ok(()) => app
            .sidebar
            .succeed(format!("已删除 {}", display_name(path))),
        Err(e) => app.sidebar.fail(e.to_string()),
    }
}

/// 路径末段（显示用）。
fn display_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// 执行文件树的点击意图（在渲染结束后统一做，见 [`FileIntent`]）。
fn run_file_intent(app: &mut EditorApp, intent: FileIntent) {
    if let Some(path) = intent.toggle {
        app.sidebar.toggle_expanded(&path);
    }
    if let Some(path) = intent.menu {
        app.sidebar.open_context_menu(&path);
    }
    if let Some(path) = intent.open
        && let Err(e) = app.open_workspace_file(&path)
    {
        app.sidebar
            .fail(format!("无法打开 {}：{e}", display_name(&path)));
    }
}

// ===========================================================================
// 大纲
// ===========================================================================

/// 大纲标签。
fn outline_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
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
                        ui.label(text);
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

// ===========================================================================
// 全局搜索
// ===========================================================================

/// 搜索标签。
fn search_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
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
                && !on
            {
                app.sidebar.search_options = options.toggled(toggle);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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

// ===========================================================================
// 提示
// ===========================================================================

/// 底部提示行。
fn messages(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    if let Some(error) = app.sidebar.error.clone() {
        ui.colored_label(theme.error, format!("⚠ {error}"));
    }
    if let Some(notice) = app.sidebar.notice.clone() {
        ui.colored_label(theme.success, notice);
    }
}
