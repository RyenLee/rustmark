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
//! | 全局顶栏 `draw_app_menu_bar` | [`MenuGroup::APP_LEVEL`] | 整个应用（打开 / 保存 / 视图开关 / 设置） | 全部是**下拉菜单** |
//! | 编辑区顶部 `draw_editor_toolbar` | [`MenuGroup::Format`] | 当前编辑区里的**内容**（加粗 / 列表 / 缩进……） | 按作用分组的**下拉菜单** |
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
//! 菜单项与工具栏命令本身都是遍历 [`MenuGroup`] × [`UiAction::ALL`] 生成的，
//! 文案与键位都来自同一个 [`UiAction`] 的 `label()` / `ShortcutRegistry::display_for()`。
//! 想让某个动作不上菜单，只能从动作表里删它——那样快捷键和命令面板会**同时**消失。

use egui::{Align, Context, Layout, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::shortcuts::registry::{MenuGroup, UiAction};

// ============================================================================
// 菜单项排版常量
// ============================================================================

/// 菜单项文案与其右侧快捷键之间的**最小**空隙（点）。
///
/// 取值只需比「视觉上挨在一起」明显一点：13pt 字号下 16pt 约合一个多汉字宽。
///
/// `pub` 是为了让几何用例能直接断言「文案与快捷键之间至少留这么宽」
/// （见 `tests/presentation/top_menu_test.rs`）。
pub const MENU_LABEL_SHORTCUT_GAP: f32 = 16.0;

/// 菜单项左右各自的内边距（点）。同上，`pub` 供几何用例使用。
pub const MENU_ROW_PAD: f32 = 8.0;

/// 菜单项行高（点）。低于 `interact_size.y` 时取后者，保证可点区域不缩水。
const MENU_ROW_H: f32 = 20.0;

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
            draw_app_menu_bar(app, ui);
            ui.separator();
            draw_tab_strip(app, ui);
            draw_breadcrumb(app, ui);
        });
}

// ============================================================================
// 第 1 层：应用级菜单栏
// ============================================================================

/// 应用级菜单栏：只渲染 [`MenuGroup::APP_LEVEL`]，每一项都是一个下拉菜单。
///
/// 格式编排（[`MenuGroup::Format`]）**不在这里**——它属于编辑区顶部的编辑区工具栏，
/// 作用域不同，见文件头的作用域表。
///
/// # 为什么整行要先占一个 `topbar_height` 的容器
///
/// `ui.horizontal` 的初始行高只有 `interact_size.y`（18pt），而 `ComboBox`
/// 按钮实际高约 23pt（文案 + 上下内边距）。行高比按钮矮时，egui 会把每个
/// 按钮「在当前行高内居中」——行高又随前一个按钮被撑高——于是每个按钮都比
/// 前一个低一点、逐个「下楼梯」，**第一个菜单项明显偏高**（实测 top 差约 5pt，
/// 且顶栏与编辑区工具栏两处都如此）。先把行的可用高度给足（垂直预算本就
/// 规定菜单栏占 [`Metrics::topbar_height`]），所有按钮在同一高度内居中，
/// 自然等高同排。
fn draw_app_menu_bar(app: &mut EditorApp, ui: &mut Ui) {
    let m = app.theme().metrics;
    let mut pending: Option<UiAction> = None;
    ui.allocate_ui(egui::vec2(ui.available_width(), m.topbar_height), |ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            // 左缘内边距与组间距同为 `space_md`：第一个菜单项与后续各项的
            // 左右间距完全一致（行首不额外缩、组间不额外空）。
            ui.add_space(m.space_md);
            for group in MenuGroup::APP_LEVEL {
                let actions: Vec<UiAction> = UiAction::ALL
                    .iter()
                    .copied()
                    .filter(|action| action.group() == *group)
                    .collect();
                // 先量、后画：见 `menu_row_width` 的注释（弹出层宽度 ≠ 内容宽度）。
                let row_w = menu_row_width(ui, app, &actions);

                let mut clicked = None;
                egui::ComboBox::from_id_salt(("menu", group))
                    .selected_text(group.label())
                    .show_ui(ui, |ui| {
                        for action in &actions {
                            if menu_item(ui, app, *action, row_w).clicked() {
                                clicked = Some(*action);
                            }
                        }
                    });
                pending = pending.or(clicked);
            }
        });
    });
    // 统一在此执行：菜单项与工具栏、快捷键、命令面板走**同一条**路径，
    // 这正是「三入口一致」得以成立的地方。
    if let Some(action) = pending {
        app.execute(action);
    }
}

/// 量出这一组菜单项所需的**行宽**。
///
/// # 为什么必须在渲染前把宽度**算**出来
///
/// 弹出层的宽度不由内容决定：`ComboBox` 走的是
/// `Popup::menu(..).width(button.rect.width())`，即**按钮的宽度**
/// （约等于 `Spacing::combo_width`，默认 100pt），而按钮的宽度又只取决于
/// 「当前选中的那个词」（「文件」两个字）。同时，菜单项右侧的快捷键是用
/// `Painter::text` **画**上去的——作画不占位，布局完全看不见它。
///
/// 两件事叠加的结果是：文案自左向右排、快捷键右对齐画到同一行，
/// 而两者被挤在 100pt 内，**必然重叠**。实测（`ui_font_size = 13`）：
///
/// | 分组 | 每项所需宽度 | 实际拿到 |
/// |---|---|---|
/// | 文件 | 114 – 136 pt | 100 pt |
/// | 视图 | 165 – 199 pt | 100 pt |
///
/// 所以这里把整组里「文案 + 间隙 + 快捷键」的最大值算出来，
/// 交给每个菜单项**显式占位**。`Area` 会量取最宽子项并把弹出层撑到该宽度，
/// 于是文案与快捷键之间始终留有 [`MENU_LABEL_SHORTCUT_GAP`]。
///
/// 注意宽度**不会**因此传到按钮上：这里没有调 `ComboBox::width`，
/// 所以菜单栏上的「文件」按钮仍是原来的窄按钮，只有弹出层被撑宽。
///
/// `pub` 是为了让几何用例能直接调用它，断言「算出来的行宽确实容得下
/// 该组最宽的一项」（见 `tests/presentation/top_menu_test.rs`）。
pub fn menu_row_width(ui: &Ui, app: &EditorApp, actions: &[UiAction]) -> f32 {
    let font = egui::FontId::proportional(app.theme().metrics.ui_font_size);
    let registry = app.dispatcher().registry();
    // 亮色/暗色都无所谓：这里只用 `Galley` 的尺寸，颜色不参与度量。
    let probe_color = egui::Color32::WHITE;
    let widest = ui.fonts_mut(|fonts| {
        actions
            .iter()
            .map(|action| {
                let label_w = fonts
                    .layout_no_wrap(action.label().to_owned(), font.clone(), probe_color)
                    .size()
                    .x;
                let combo = registry.display_for(*action);
                let combo_w = if combo.is_empty() {
                    0.0
                } else {
                    fonts
                        .layout_no_wrap(combo, font.clone(), probe_color)
                        .size()
                        .x
                };
                label_w + combo_w
            })
            .fold(0.0_f32, f32::max)
    });
    widest + MENU_LABEL_SHORTCUT_GAP + 2.0 * MENU_ROW_PAD
}

/// 单个菜单项：左侧文案 + 右侧快捷键。
///
/// # 为什么不用 `ui.selectable_label` + 手工画快捷键
///
/// 那正是修复前的写法，也是重叠的来源：`selectable_label` 只占**文字自身**的宽，
/// 之后的 `Painter::text` 把快捷键画到 `rect.right_center()`，
/// 两段于是叠在一起。即便弹出层足够宽（`selectable_label` 会被
/// `Layout::top_down_justified` 拉伸），那也只是「恰好不重叠」的巧合——
/// 而弹出层本身只有 100pt，巧合无从发生。
///
/// 现在改成**一行一次占位**：行宽由 [`menu_row_width`] 预量，两项各自用
/// `Align2` 贴着这一行的左/右边界作画，中间留出固定的间隙。
/// 宽度是算出来的，因此不依赖任何布局施舍。
fn menu_item(ui: &mut Ui, app: &EditorApp, action: UiAction, row_w: f32) -> egui::Response {
    let font = egui::FontId::proportional(app.theme().metrics.ui_font_size);
    let row_h = MENU_ROW_H.max(ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(row_w, row_h), egui::Sense::click());

    if ui.is_rect_visible(rect) {
        // 悬停高亮：沿用菜单的圆角与 `widgets.hovered` 配色，
        // 避免菜单项看起来「点了不会有反应」。
        if response.hovered() {
            ui.painter().rect_filled(
                rect,
                ui.visuals().menu_corner_radius,
                ui.visuals().widgets.hovered.weak_bg_fill,
            );
        }
        ui.painter().text(
            rect.left_center() + egui::vec2(MENU_ROW_PAD, 0.0),
            egui::Align2::LEFT_CENTER,
            action.label(),
            font.clone(),
            ui.visuals().text_color(),
        );
        // 快捷键列右对齐，且**必须**来自注册表而非硬编码字符串——
        // 硬编码是原型 B-02「菜单写了但没注册」的温床。
        let combo = app.dispatcher().registry().display_for(action);
        if !combo.is_empty() {
            ui.painter().text(
                rect.right_center() - egui::vec2(MENU_ROW_PAD, 0.0),
                egui::Align2::RIGHT_CENTER,
                combo,
                font,
                ui.visuals().weak_text_color(),
            );
        }
    }
    response
}

// ============================================================================
// 编辑区工具栏（位于编辑区顶部，见 `editor_view::show`）
// ============================================================================

/// 编辑区工具栏的分组：下拉标题 → 该组动作（均为 [`MenuGroup::Format`]）。
///
/// 18 个格式动作按「工具作用」分成若干下拉，工具栏只占几个下拉标题的宽度，
/// 既放得下全部动作、又不挤占编辑区。整组动作的**并集必须正好等于**
/// [`UiAction::ALL`] 中 `group() == Format` 的全部动作——否则有动作在界面上
/// 无从触发（见 `top_menu_test::editor_toolbar_groups_cover_all_format_actions`）。
pub fn editor_toolbar_groups() -> Vec<(&'static str, Vec<UiAction>)> {
    use UiAction::*;
    vec![
        (
            "格式",
            vec![Bold, Italic, Strikethrough, InlineCode, InsertLink],
        ),
        ("插入", vec![CodeBlock, InsertTable, InsertImage]),
        (
            "列表/结构",
            vec![
                ToggleList,
                ToggleOrderedList,
                ToggleQuote,
                ToggleTask,
                PromoteHeading,
                DemoteHeading,
            ],
        ),
        ("行操作", vec![Indent, Outdent, MoveLineUp, MoveLineDown]),
    ]
}

/// 编辑区工具栏：渲染 [`MenuGroup::Format`] 的**全部**动作，按作用分组成下拉菜单。
///
/// # 为什么是「分组下拉」而不是铺按钮
///
/// 18 个格式动作若全铺成按钮，完整文案约 1270pt，已超 1280 窗口的可用宽，
/// 窄编辑区里必被裁掉（被裁 = 这个动作不存在）。改为分组下拉后，工具栏只占
/// 几个下拉标题的宽度。下拉项复用应用级菜单同一套 `menu_row_width` / `menu_item`
/// 排版，文案与快捷键同样不重叠。
///
/// 工具栏位于**编辑区顶部**（上下布局：工具栏在上、文本在下），作用域与编辑区
/// 内容一致，见 `editor_view::show`。
pub fn draw_editor_toolbar(app: &mut EditorApp, ui: &mut Ui) {
    let m = app.theme().metrics;
    let mut pending: Option<UiAction> = None;
    // 横向滚动而非换行：窗口很窄时仍能把每个下拉点出来；够宽时不出现滚动条。
    egui::ScrollArea::horizontal()
        .id_salt("editor-toolbar-scroll")
        .show(ui, |ui| {
            // 与应用级菜单栏同款修法：行高给足（`editor_toolbar_height`），
            // 所有下拉按钮在同一高度内居中——否则每个按钮比前一个低一点，
            // 第一个菜单项明显偏高（见 `draw_app_menu_bar` 的注释）。
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // 间距规范：左缘内边距取 `space_md`（与应用级菜单栏同值，
                // 也与预览区标题栏同一条左边线），组与组之间同为 `space_md`
                //（只在组之间加，末尾不加——末尾那份间距会让整行视觉重心
                // 偏左，右侧白边看着比左侧宽）。
                ui.add_space(m.space_md);
                for (i, (title, actions)) in editor_toolbar_groups().into_iter().enumerate() {
                    if i > 0 {
                        ui.add_space(m.space_md);
                    }
                    let row_w = menu_row_width(ui, app, &actions);
                    let mut clicked = None;
                    egui::ComboBox::from_id_salt(("editor-toolbar", title))
                        .selected_text(title)
                        .show_ui(ui, |ui| {
                            for action in &actions {
                                if menu_item(ui, app, *action, row_w).clicked() {
                                    clicked = Some(*action);
                                }
                            }
                        });
                    pending = pending.or(clicked);
                }
            });
        });
    if let Some(action) = pending {
        app.execute(action);
    }
}

// ============================================================================
// 标签条与面包屑（S6-02）
// ============================================================================

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
