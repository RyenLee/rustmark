//! 编辑区工具栏（位于编辑区顶部，见 `editor_view::show`）与控件视觉规范。

use egui::{Align, CornerRadius, Layout, RichText, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::theme::builtin::{Metrics, Theme};

use super::menu_bar::{menu_item, menu_row_width};

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

/// 编辑区工具栏的分组：下拉标题 → 该组动作（均为 `MenuGroup::Format`）。
///
/// 18 个格式动作按「工具作用」分成若干组，工具栏只占几个分组按钮的宽度，
/// 既放得下全部动作、又不挤占编辑区。整组动作的**并集必须正好等于**
/// `UiAction::ALL` 中 `group() == Format` 的全部动作——否则有动作在界面上
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
// 现在统一到一条规则：**控件外观只有一个来源**（`control_button`）。
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

/// 编辑区工具栏：渲染 `MenuGroup::Format` 的**全部**动作。
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
/// 两档的**外观完全一致**（同一个 `control_button`），差别只有「右侧有没有三角」，
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
/// `Button`）之后，触发器与平铺按钮共用 `control_button` 的尺寸与配色，
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
