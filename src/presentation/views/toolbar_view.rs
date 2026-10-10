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

use egui::{Align, Context, CornerRadius, Layout, RichText, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::shortcuts::registry::{MenuGroup, UiAction};
use crate::presentation::theme::builtin::{Metrics, Theme};

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

/// 「格式」组：唯一**平铺成按钮**的分组（见 [`draw_editor_toolbar`] 的注释）。
///
/// `pub` 是为了让几何用例能直接用它区分「平铺分组」与「下拉分组」
/// （见 `tests/presentation/toolbar_visual_test.rs`）。
pub const FLAT_GROUP: &str = "格式";

/// 下拉触发器右侧的三角。
///
/// 实测本机字体下**有**字形；`▾`（U+25BE）**没有**，用它会画出一个豆腐块方框
/// （口径与清单见 [`UiAction::icon`] 与 `tests/presentation/toolbar_visual_test.rs`）。
const DROPDOWN_ARROW: &str = "⏷";

/// 编辑区工具栏的分组：下拉标题 → 该组动作（均为 [`MenuGroup::Format`]）。
///
/// 18 个格式动作按「工具作用」分成若干组，工具栏只占几个分组按钮的宽度，
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

// ============================================================================
// 视觉规范：工具栏上**所有**控件共用同一套尺寸、圆角、配色与间距
// ============================================================================
//
// 这一节是「工具栏样式与整体设计不一致」的修复主体。此前工具栏是
// 「一排 `ui.button`（文字按钮）+ 三个 `ComboBox`（固定 100pt 宽）」的混搭：
//
// | 实测（1280×800 无头渲染，量 `Shape::Rect`） | 控件框 |
// |---|---|
// | 「格式」组的 `ui.button` | `y=[108,130]`，高 22 |
// | `ComboBox`「插入」 | `y=[110,133]`，高 23，宽固定 100 |
// | `ComboBox`「列表/结构」 | `y=[110,133]`，高 23，宽固定 100 |
//
// 三个问题一次暴露：
//   1. **不等高、不齐平**——两档控件的顶边差 2pt、高度差 1pt，
//      整行看上去「下楼梯」（与顶栏曾经踩过的坑同源）；
//   2. **宽度不成比例**——「插入」两个字和「列表/结构」五个字一样宽（都 100pt），
//      既浪费横向空间，又让「宽度」这条视觉线索失去意义；
//   3. **没有图标、没有分组**——动作只能靠文字辨认，不同分组之间也没有分隔，
//      整行是一片等距按钮。
//
// 现在统一到一条规则：**控件外观只有一个来源**（[`control_button`]）。
// 高度 / 圆角 / 配色 / 悬停反馈全部取自主题——`theme::mapper::apply` 已把
// `widgets.{inactive,hovered,active}` 的 `weak_bg_fill` / `bg_stroke` / `fg_stroke`
// 映射到 `bg_tertiary` / `accent_soft` / `border` / `accent`，
// 于是「平铺按钮」与「分组下拉」像素级同款，只有右侧多一个三角。

/// 工具栏控件的统一高度（点）。
///
/// # 为什么必须显式规定
///
/// 按钮用「文案高 + `button_padding`」、下拉用「文案高与图标高取大 + 内边距」，
/// 两者各自算高、再各自 `Align::Center`——依赖「正好算得一样高」这个巧合。
/// 实测差 1–2pt 且顶边错位（见上一节表格）。与其继续追巧合，不如把高度
/// 收敛成一个值：**工具栏行高 − 上下各一个咬合间距**，即控件在行内
/// 上下各留一格呼吸位。所有控件取同一个值，等高与齐平就同时成立。
fn control_height(m: &Metrics) -> f32 {
    (m.editor_toolbar_height - 2.0 * m.space_xxs).max(20.0)
}

/// 控件圆角：与主题的按钮圆角同源，不写死数字。
fn control_radius(m: &Metrics) -> CornerRadius {
    CornerRadius::same(m.radius_button.round().clamp(0.0, 255.0) as u8)
}

/// 工具栏按钮上的文案：`图标 + 空格 + 工具栏专用短文案`。
///
/// 用 [`UiAction::toolbar_label`] 而不是 [`UiAction::label`]：后者是菜单 /
/// 命令面板里的完整说法（「提升标题级别」），铺到一行按钮上会横向溢出。
/// 完整说法由悬停提示补足（见 [`draw_flat_actions`]）。
///
/// 图标与文案之间是**一个普通空格**而非布局间距——它们属于同一段文字，
/// 用 `add_space` 会在动作没有图标时留下一段看不懂的空白。
pub fn button_text(action: UiAction) -> String {
    match action.icon() {
        Some(icon) => format!("{icon} {}", action.toolbar_label()),
        None => action.toolbar_label().to_string(),
    }
}

/// 工具栏上**每一个控件**的可见文案，按从左到右的顺序。
///
/// 「平铺分组」（[`FLAT_GROUP`]）里的每个动作各是一个按钮，文案见 [`button_text`]；
/// 其余分组各是一个下拉，**只画分组标题**，动作要展开才出现。
///
/// # 为什么要单独给一个函数，而不是让用例自己遍历 `editor_toolbar_groups`
///
/// 「工具栏上到底有几个控件、每个叫什么」此前散落在两处：渲染时按
/// `title == FLAT_GROUP` 决定平铺还是下拉，用例则假设「所有分组标题都可见」——
/// 一旦某个分组改成平铺，用例就去找一个根本不存在的标题而失败（实测：
/// 「格式」组平铺后，`top_menu_test` 里找「格式」标题的断言全红）。
///
/// 现在渲染与用例都从这个函数取值，「用例检查的控件」与「实际画出的控件」
/// 不可能再对不上：改分组形态只需改这里一处。
pub fn toolbar_control_labels() -> Vec<String> {
    let mut out = Vec::new();
    for (title, actions) in editor_toolbar_groups() {
        if title == FLAT_GROUP {
            out.extend(actions.iter().map(|a| button_text(*a)));
        } else {
            out.push(title.to_string());
        }
    }
    out
}

/// 一个工具栏控件的**统一外观**。
///
/// # 刻意不指定 `fill` / `stroke`
///
/// 指定它们会把悬停 / 按下反馈一并写死，按钮就成了死按钮。这里让 egui 走
/// `visuals.widgets.{inactive,hovered,active}`——主题映射里这三种状态的
/// 底色 / 描边 / 文字色已经统一（见本节的说明）。这样「统一配色」不是把
/// 颜色抄两遍，而是**只有一个来源**：改主题时两者必然一起变。
///
/// 尺寸同理：`min_size` 只钉高度（宽度随文案），`corner_radius` 取自主题，
/// 内边距沿用 egui 的 `button_padding`——平铺按钮与下拉触发器都走这里，
/// 于是它们不可能长得不一样。
fn control_button(ui: &mut Ui, theme: &Theme, text: impl Into<String>, h: f32) -> egui::Response {
    let m = theme.metrics;
    ui.add(
        egui::Button::new(RichText::new(text.into()).size(m.ui_font_size - 1.0))
            .min_size(egui::vec2(0.0, h))
            .corner_radius(control_radius(&m)),
    )
}

/// 组与组之间的竖直分隔线。
///
/// # 高度必须是确定值
///
/// egui 的 `Separator` 在**水平**布局里取 `available_size_before_wrap().y` 当高度；
/// 工具栏外面套着 `ScrollArea::horizontal()`，其内部 `ui` 的可用高度语义随内容变化。
/// 而 `settings_view::body` 已经因「分隔线撑高容器 → 容器变高 → 分隔线更高」
/// 这个正反馈出现过浮窗尺寸爆炸（见该文件 `body` 的修复记录）。
/// 这里不给它机会：高度按控件高取一个写死的比例。
///
/// 分隔线**自带**两侧留白（占位宽度即组间距），调用方不必再 `add_space`——
/// 「组间距」因此只有一处定义，不会出现「有的地方空 8、有的地方空 12」。
fn group_divider(ui: &mut Ui, theme: &Theme, h: f32) {
    let gap = theme.metrics.space_md;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(gap, h * 0.62), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, theme.border),
    );
}

/// 编辑区工具栏：渲染 [`MenuGroup::Format`] 的**全部**动作。
///
/// # 一层控件，两种入口形态
///
/// 全部 18 个动作都在这一行里，但**入口形态按频次分两档**：
///
/// - 「格式」组（加粗 / 斜体 / 删除线 / 行内 / 链接）是最高频动作，
///   每次排版都要反复点，故**平铺成按钮**，省掉「先展开下拉」这一步；
/// - 其余三组（插入 / 列表结构 / 行操作）用**分组按钮 + 下拉**，
///   避免整条工具栏被 18 个按钮挤爆——窄窗口里最右侧的按钮会被裁掉，
///   而「被裁掉」等于「这个功能不存在」。
///
/// 两档的**外观完全一致**（同一个 [`control_button`]），差别只有「右侧有没有三角」，
/// 因此用户看到的是「一排同类按钮」，而不是「两种控件混排」。
///
/// 工具栏位于**编辑区顶部**（上下布局：工具栏在上、文本在下），作用域与编辑区
/// 内容一致，见 `editor_view::show`。
pub fn draw_editor_toolbar(app: &mut EditorApp, ui: &mut Ui) {
    // 主题先取出来（`app` 在闭包里还要可变借用，不能同时持有 `&Theme`）。
    let theme = app.theme().clone();
    let h = control_height(&theme.metrics);
    let mut pending: Option<UiAction> = None;

    // 横向滚动而非换行：窗口很窄时仍能把每个按钮点出来；够宽时不出现滚动条。
    egui::ScrollArea::horizontal()
        .id_salt("editor-toolbar-scroll")
        .show(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // `item_spacing.x` 归零：否则 egui 会在每个控件之间**自动**塞
                // 一个 `Spacing::item_spacing.x`（默认 8），叠加到 `add_space`
                // 显式给的值上——实测「组内 4 + 自动 8 = 12」与
                // 「组间 8 + 自动 8×2 = 24」两处间距都翻倍，
                // 于是「同组紧、异组松」这个层级差被自动间距冲淡。
                // 归零后间距**只**由下面每个 `add_space` / 分隔线宽度决定，
                // 全行间距因此可以一眼算清（组内 `space_xs`、组间 `space_lg`）。
                ui.spacing_mut().item_spacing.x = 0.0;
                // 左缘内边距与应用级菜单栏、预览区标题同值——三处的左边线对齐。
                ui.add_space(theme.metrics.space_md);
                for (i, (title, actions)) in editor_toolbar_groups().into_iter().enumerate() {
                    if i > 0 {
                        group_divider(ui, &theme, h);
                    }
                    let hit = if title == FLAT_GROUP {
                        draw_flat_actions(ui, &theme, &actions, h)
                    } else {
                        draw_group_dropdown(app, ui, &theme, title, &actions, h)
                    };
                    pending = pending.or(hit);
                }
            });
        });

    // 统一在此执行：菜单项与工具栏、快捷键、命令面板走**同一条**路径，
    // 这正是「三入口一致」得以成立的地方。
    if let Some(action) = pending {
        app.execute(action);
    }
}

/// 把「格式」分组的动作平铺成按钮。
///
/// 组内间距取 `space_xs`（比组间的分隔线间距小一档），于是「同组」在视觉上聚拢、
/// 「不同组」被分隔线自然切开——分组关系不需要额外的分组标题来说明。
///
/// 按钮上只有短文案（见 [`button_text`]），完整说法由**悬停提示**补足：
/// 可发现性靠悬停保证，不靠把按钮撑宽。返回被点击的动作（没有则为 `None`），
/// 由调用方统一走 `app.execute`。
fn draw_flat_actions(ui: &mut Ui, theme: &Theme, actions: &[UiAction], h: f32) -> Option<UiAction> {
    let mut hit = None;
    for (i, action) in actions.iter().enumerate() {
        if i > 0 {
            ui.add_space(theme.metrics.space_xs);
        }
        let resp = control_button(ui, theme, button_text(*action), h).on_hover_text(action.label());
        if resp.clicked() {
            hit = Some(*action);
        }
    }
    hit
}

/// 低频分组：一个与平铺按钮**同款**的按钮 + 其右侧的下拉菜单。
///
/// # 为什么不用 `ComboBox`
///
/// `ComboBox` 的按钮宽度默认取 `Spacing::combo_width`（100pt），与文案长度无关——
/// 「插入」与「列表/结构」因此一样宽，横向空间被白白吃掉，宽度也不再传递
/// 「这个分组有多重」的信息；它的高度也不受控（实测 23，与 `ui.button` 的 22 错位）。
/// 换成 `Button` + [`egui::containers::menu::MenuButton`]（egui 的菜单按钮本身就是
/// `Button`）之后，触发器与平铺按钮共用 [`control_button`] 的尺寸与配色，
/// 宽度自然跟随文案。
fn draw_group_dropdown(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    actions: &[UiAction],
    h: f32,
) -> Option<UiAction> {
    let m = theme.metrics;
    let mut clicked = None;
    let button = egui::Button::new(RichText::new(title).size(m.ui_font_size - 1.0))
        // 三角经 `right_text` 参与宽度计算：按钮自己会为它留位，
        // 不会像「先量文案宽、再画三角」那样在换字号后压到文字上。
        .right_text(
            RichText::new(DROPDOWN_ARROW)
                .size(m.ui_font_size - 3.0)
                .color(theme.text_secondary),
        )
        .min_size(egui::vec2(0.0, h))
        .corner_radius(control_radius(&m));
    // `egui::menu`（根模块）是**已废弃**的旧菜单 API；新 API 在 `containers::menu`。
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui: &mut Ui| {
        // 先量、后画：见 `menu_row_width` 的注释（弹出层宽度 ≠ 内容宽度）。
        let row_w = menu_row_width(ui, app, actions);
        for action in actions {
            if menu_item(ui, app, *action, row_w).clicked() {
                clicked = Some(*action);
            }
        }
    });
    clicked
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
