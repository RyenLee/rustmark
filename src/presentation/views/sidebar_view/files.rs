//! 文件树标签：树行渲染、右键菜单、新建/重命名编辑与删除反馈。

use egui::{RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::sidebar_ui_state::{NameEdit, TreeRow, tree_rows};
use crate::presentation::theme::builtin::Theme;
use crate::presentation::widgets;
use crate::shared::path::file_name_of;

use super::*;

/// 展开箭头列宽（目录专用；文件留等宽空位保持图标对齐）。
const ARROW_W: f32 = 12.0;
/// 类型图标列宽（文件夹 / 页面）。
const ICON_W: f32 = 16.0;
/// 图标与文件名之间的间距。
const ICON_GAP: f32 = 4.0;

/// 文件树标签。
pub(super) fn files_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    if !app.workspace_entries.is_loaded() {
        return;
    }
    dismiss_menu_on_outside_click(app, ui);
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
                egui::Id::unique("rustmark.sidebar.blank"),
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
    /// 双击了白名单**外**的文件（问题 045）：入口按构造不生效，
    /// 但要把原因报到状态栏。
    denied: Option<String>,
    select: Option<String>,
    menu: Option<String>,
}

impl FileIntent {
    /// 记录「打开某文件」（后一次点击覆盖前一次，与原型一致）。
    fn request_open(&mut self, path: &str) {
        self.open = Some(path.to_string());
        self.menu = None;
    }

    /// 记录「这个格式打不开」（与 [`Self::request_open`] 互斥）。
    fn request_denied(&mut self, path: &str) {
        self.denied = Some(path.to_string());
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
///
/// # 三个可读性要点（问题 037）
///
/// 1. **目录与文件必须一眼可辨**：旧实现用字形 `▾` / `▸` 当展开箭头，
///    而 `▾`（U+25BE）在本机字体链路里**没有字形**（字形覆盖的坑与
///    工具栏图标废止字形的完整理由见 `presentation::icons` 的模块文档），
///    展开态目录显示的是一个豆腐块方框；
///    文件则完全没有标识。现在图标全部改用**向量绘制**（`Painter`），
///    不依赖任何字体覆盖：目录 = 填充文件夹形，文件 = 描边页面形。
/// 2. **树结构要看得见**：每一级缩进画一条竖向导引线 + 一根横向 tick，
///    层级关系不再只靠空格猜。
/// 3. **双击才打开**：单击只选中（高亮即反馈），双击文件才在编辑区 /
///    预览区打开——与资源管理器、VS Code 的行为一致，避免「点一下
///    就切走当前文档」打断编辑。
/// 4. **白名单外的格式置灰且打不开**（问题 045）：只有
///    [`EditorApp::can_open_as_text`] 放行的文件可双击打开；图片 /
///    二进制等以次要色绘制（一眼可辨「这一项不能用」），双击不派打开
///    意图、只把原因报到状态栏。判据与应用层的打开守卫**同源**——
///    文件树置灰的东西，打开管线也一定拒绝，反之亦然。
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
    // 目录没有「格式」可言（双击是展开/收起），故只对文件做白名单判定。
    let openable = row.is_directory || app.can_open_as_text(&row.path);
    // 整行可点（含右键）由 `widgets::row::clickable_row` 保证：行内容由
    // 箭头、图标、文件名等多段 `Ui` 拼成，默认只有 hover sense，会盖在
    // 行交互之上、吞掉点击（问题 1：文件树点击与右键均无响应）。
    // 右键同属 click 大类，一个 sense 同时覆盖。
    let mut icon_rect = egui::Rect::NOTHING;
    let (response, _) = widgets::row::clickable_row(ui, ("rustmark.file.row", &row.path), |ui| {
        // 缩进 + 树导引线占位：每一级缩进等宽，导引线画在级边界上。
        ui.add_space(ROW_PAD + row.depth as f32 * INDENT_STEP);
        // 目录的展开箭头占固定宽度，文件留同样的空位——
        // 否则同级文件与目录的图标会左右错开，扫视时很难对齐。
        let (arrow, _) =
            ui.allocate_exact_size(egui::vec2(ARROW_W, ROW_HEIGHT), egui::Sense::hover());
        let _ = arrow;
        // 图标列：目录画文件夹、文件画页面（向量，见 [`file_row`] 文档）。
        let (icon, _) =
            ui.allocate_exact_size(egui::vec2(ICON_W, ROW_HEIGHT), egui::Sense::hover());
        icon_rect = icon;
        ui.add_space(ICON_GAP);
        // 文件名同样截断：与大纲同一缺陷类，长文件名会撑出侧栏。
        //
        // 文字色三分（问题 045）：白名单外的文件用次要色——与导出浮窗里
        // 「暂未开放」的格式同一种表达（`export_view::format_row`），
        // 用户扫一眼就知道这一项不能打开，不必先双击一次才知道。
        // 选中态一律用 `theme.text_primary`：全仓纪律（037 / 039）——
        // 选中底色在用户自定义主题里可以是实色强调蓝，强调色文字压上去
        // 会糊成一片。
        let name_color = if is_selected || openable {
            theme.text_primary
        } else {
            theme.text_secondary
        };
        ui.add(egui::Label::new(RichText::new(&row.name).color(name_color)).truncate());
    });
    // 白名单外的行挂一句悬停说明：置灰只表达了「不能用」，说不清「为什么」。
    // 文案取自应用层（[`EditorApp::unsupported_format_notice`]）——
    // 与双击时报到状态栏的是**同一句**，不会出现两种说法。
    let response = if openable {
        response
    } else {
        response.on_hover_text(EditorApp::unsupported_format_notice(&row.path))
    };
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
    // 树形导引线（在底色之上、内容之下看不出来——线与图标/文字不重叠，
    // 画在底色之上即可）。
    paint_tree_guides(ui.painter(), row, response.rect, theme);
    paint_row_icon(ui.painter(), row, expanded, icon_rect, theme);
    // 交互：目录单击展开/收起；文件单击选中、**双击**打开。
    if response.clicked() {
        if row.is_expandable() {
            intent.toggle = Some(row.path.clone());
        } else {
            intent.select = Some(row.path.clone());
        }
    }
    if response.double_clicked() && !row.is_expandable() {
        // 打开入口按格式白名单**分流**（问题 045）：白名单外的文件不派
        // `open` 意图（等于禁用入口），改派 `denied` 只报原因。分流放在
        // 这里而不是让 `open_workspace_file` 拒绝，是为了让「文件树允许
        // 双击的东西」与「打开管线接受的东西」在**构造上**一致：
        // 用户不会遇到「双击没反应」或「双击了才被告知不行」。
        if openable {
            intent.request_open(&row.path);
        } else {
            intent.request_denied(&row.path);
        }
    }
    if response.secondary_clicked() {
        intent.menu = Some(row.path.clone());
    }
    if menu_open {
        context_menu(ui, app, theme, row);
    }
}

/// 画一行的树形导引线：每一级缩进一条竖线，本行深度一根横向 tick。
///
/// 逐行画线段（而不是连续长线）的好处是**行与行之间天然衔接**——
/// 相邻两行各自画的那段竖线在视觉上连成一条，不需要额外维护
/// 「哪个祖先的线要继续往下画」的状态。最后一级若是该层最后一项
/// （`is_last`），竖线只画半格、止于 tick——表示这根线到此为止。
fn paint_tree_guides(painter: &egui::Painter, row: &TreeRow, rect: egui::Rect, theme: &Theme) {
    let color = theme.border.gamma_multiply(0.8);
    let top = rect.top();
    let bottom = rect.bottom();
    let mid = rect.center().y;
    for depth in 0..row.depth {
        let x = ROW_PAD + depth as f32 * INDENT_STEP + INDENT_STEP * 0.5;
        let stop = if depth + 1 == row.depth && row.is_last {
            mid
        } else {
            bottom
        };
        painter.line_segment(
            [egui::pos2(x, top), egui::pos2(x, stop)],
            egui::Stroke::new(1.0_f32, color),
        );
    }
    if row.depth > 0 {
        // 本行的横向 tick：从最深的竖线连到图标列。
        let x_from = ROW_PAD + (row.depth - 1) as f32 * INDENT_STEP + INDENT_STEP * 0.5;
        let x_to = ROW_PAD + row.depth as f32 * INDENT_STEP;
        painter.line_segment(
            [egui::pos2(x_from, mid), egui::pos2(x_to, mid)],
            egui::Stroke::new(1.0_f32, color),
        );
    }
}

/// 画行首图标与展开箭头（**全向量**，不依赖字体字形）。
///
/// `icon_rect` 是布局时分配的图标列矩形；箭头紧贴它左侧。
/// 目录 = 填充文件夹形（tab +  body 两个矩形拼出），文件 = 描边页面形
/// （带折角）。选中态整体提亮一档，让高亮行更有存在感。
fn paint_row_icon(
    painter: &egui::Painter,
    row: &TreeRow,
    expanded: bool,
    icon_rect: egui::Rect,
    theme: &Theme,
) {
    if icon_rect == egui::Rect::NOTHING {
        return;
    }
    let cy = icon_rect.center().y;
    // —— 展开箭头（仅目录）——
    if row.is_expandable() {
        let s = 3.0; // 三角形半边长
        let cx = icon_rect.left() + 4.0;
        let points = if expanded {
            // 朝下 ▼
            vec![
                egui::pos2(cx - s, cy - s * 0.6),
                egui::pos2(cx + s, cy - s * 0.6),
                egui::pos2(cx, cy + s * 0.8),
            ]
        } else {
            // 朝右 ▶
            vec![
                egui::pos2(cx - s * 0.6, cy - s),
                egui::pos2(cx + s * 0.8, cy),
                egui::pos2(cx - s * 0.6, cy + s),
            ]
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            theme.text_secondary,
            egui::epaint::PathStroke::NONE,
        ));
    }
    // —— 类型图标 ——
    let ix = icon_rect.left() + ARROW_W;
    if row.is_directory {
        // 文件夹：顶部小 tab + 主体矩形，填充强调色。
        let tab = egui::Rect::from_min_size(egui::pos2(ix, cy - 5.0), egui::vec2(5.0, 2.0));
        let body = egui::Rect::from_min_size(egui::pos2(ix, cy - 3.0), egui::vec2(13.0, 9.0));
        painter.rect_filled(tab, egui::CornerRadius::same(1), theme.accent);
        painter.rect_filled(body, egui::CornerRadius::same(2), theme.accent);
    } else {
        // 文件：带折角的页面轮廓，描边次要色。
        let x0 = ix + 1.0;
        let y0 = cy - 7.0;
        let w = 10.0;
        let h = 14.0;
        let fold = 3.5;
        let points = vec![
            egui::pos2(x0, y0),
            egui::pos2(x0 + w - fold, y0),
            egui::pos2(x0 + w, y0 + fold),
            egui::pos2(x0 + w, y0 + h),
            egui::pos2(x0, y0 + h),
        ];
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.2_f32, theme.text_secondary),
        ));
        // 折角斜线：让「这是纸」读得出来。
        painter.line_segment(
            [
                egui::pos2(x0 + w - fold, y0),
                egui::pos2(x0 + w - fold, y0 + fold),
            ],
            egui::Stroke::new(1.2_f32, theme.text_secondary),
        );
        painter.line_segment(
            [
                egui::pos2(x0 + w - fold, y0 + fold),
                egui::pos2(x0 + w, y0 + fold),
            ],
            egui::Stroke::new(1.2_f32, theme.text_secondary),
        );
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
                    .id(egui::Id::unique(key))
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
            app.restore_create_draft(parent, name, is_directory);
        }
        NameEdit::Rename { path, name } => {
            app.restore_rename_draft(path, name);
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
    file_name_of(path).to_string()
}

/// 执行文件树的点击意图（在渲染结束后统一做，见 `FileIntent`）。
fn run_file_intent(app: &mut EditorApp, intent: FileIntent) {
    let clicked = intent.select.is_some()
        || intent.toggle.is_some()
        || intent.open.is_some()
        || intent.denied.is_some()
        || intent.menu.is_some();
    if let Some(path) = intent.select {
        // 单选只改高亮，不开文档——打开是双击的动作（见 [`file_row`]）。
        app.select_sidebar_path(path);
    }
    if let Some(path) = intent.toggle {
        app.sidebar.toggle_expanded(&path);
    }
    if let Some(path) = intent.menu {
        app.sidebar.open_context_menu(&path);
    } else if clicked {
        // 左键点到**别的行**也要关菜单（原型 `window click → closeMenu` 的
        // 行内一半）：只靠底部空白区的判定，点行时菜单会赖着不走（问题 040）。
        // 树外那一半在 [`dismiss_menu_on_outside_click`]。
        app.sidebar.close_context_menu();
    }
    // 白名单外的格式：不进打开管线，只把原因报到状态栏（问题 045）。
    // 文案与置灰行的悬停说明同源，见 [`EditorApp::report_unsupported_format`]。
    if let Some(path) = intent.denied {
        app.report_unsupported_format(&path);
    }
    if let Some(path) = intent.open
        && let Err(e) = app.open_workspace_file(&path)
    {
        app.sidebar
            .fail(format!("无法打开 {}：{e}", display_name(&path)));
    }
}

/// 右键菜单开着的帧里，点击落点在文件树**之外**（编辑区 / 预览区 / 侧栏
/// 头部……）就直接关掉它（原型 `window click → closeMenu` 的树外一半）。
///
/// 树内的点击（行 / 空白 / 菜单按钮）由各自的处理路径负责关闭，这里只兜
/// 「点在文件树裁剪矩形之外」的情况——菜单是行内自绘的，树外没有谁能
/// 替它收尾，不兜的话点完编辑区菜单还挂着（问题 040 截图实证）。
fn dismiss_menu_on_outside_click(app: &mut EditorApp, ui: &mut Ui) {
    if app.sidebar.context_menu_path.is_none() {
        return;
    }
    let (clicked, pos) = ui.input(|i| (i.pointer.primary_clicked(), i.pointer.interact_pos()));
    let inside_tree = pos.is_some_and(|p| ui.clip_rect().contains(p));
    if clicked && !inside_tree {
        app.sidebar.close_context_menu();
    }
}
