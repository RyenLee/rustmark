//! 文件树标签：树行渲染、右键菜单、新建/重命名编辑与删除反馈。

use egui::{RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::sidebar_ui_state::{NameEdit, TreeRow, tree_rows};
use crate::presentation::theme::builtin::Theme;

use super::*;

/// 文件树标签。
pub(super) fn files_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
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
            // 「新建」待输入行：固定排在最前，用户点完「新建」立刻看到输入框。
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
            // 文件名同样截断：与大纲同一缺陷类，长文件名会撑出侧栏。
            ui.add(
                egui::Label::new(RichText::new(&row.name).color(if is_selected {
                    theme.accent
                } else {
                    theme.text_primary
                }))
                .truncate(),
            );
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
pub(super) fn display_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// 执行文件树的点击意图（在渲染结束后统一做，见 `FileIntent`）。
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
