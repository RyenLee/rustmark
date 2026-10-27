//! 编辑区工具栏（位于编辑区顶部，见 `editor_view::show`）与控件视觉规范。

use egui::{Align, CornerRadius, Layout, RichText, Ui};

use crate::presentation::icons;
use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::theme::builtin::{Metrics, Theme};
use crate::presentation::view_context::EditorViewContext;

use super::menu_bar::{menu_item, menu_row_width};

/// 「格式」组：唯一**平铺成按钮**的分组（见 [`draw_editor_toolbar`] 的注释）。
///
/// `pub` 是为了让几何用例能直接用它区分「平铺分组」与「下拉分组」
/// （见 `tests/presentation/toolbar_visual_test.rs`）。
pub const FLAT_GROUP: &str = "格式";

/// 下拉触发器右侧的三角。
///
/// 实测本机字体下**有**字形；`▾`（U+25BE）**没有**，用它会画出一个豆腐块方框
/// （字形覆盖的口径与教训见 `tests/presentation/toolbar_visual_test.rs`；
/// 工具栏动作图标已全部改为向量绘制，见 [`crate::presentation::icons`]）。
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

/// 工具栏控件的两种形态。
///
/// 渲染与用例都从这个枚举取值（见 [`toolbar_controls`]），「界面上有几个
/// 控件、各是什么」因此只有一处定义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolbarControl {
    /// 平铺**图标按钮**：一个动作 = 一个按钮，只有图形、没有文字。
    ///
    /// 原实现是「字形图标 + 短文案」（`B 加粗`）。文字在 16pt 图标旁边
    /// 只起注释作用，却把按钮撑成长短不一的块；且字形图标依赖字体覆盖
    /// （豆腐块风险，见 [`crate::presentation::icons`] 的模块文档）。
    /// 现改为纯向量图形：一行按钮因此是一条整齐的正方形栅格。
    Flat(UiAction),
    /// 分组下拉触发器：标题文字 + 右侧三角，动作展开后才出现。
    ///
    /// 标题是**分组名**（「插入」/「列表/结构」/「行操作」）而不是某个
    /// 动作名——它回答「这组是干什么的」，故保留文字；这与「动作图标」
    /// 不是一类东西，不纳入纯图形化。
    Dropdown(&'static str),
}

/// 工具栏上**每一个控件**，按从左到右的顺序。
///
/// # 为什么要单独给一个函数，而不是让用例自己遍历 `editor_toolbar_groups`
///
/// 「工具栏上到底有几个控件、各是什么形态」此前散落在两处：渲染时按
/// `title == FLAT_GROUP` 决定平铺还是下拉，用例则假设「所有分组标题都可见」——
/// 一旦某个分组改成平铺，用例就去找一个根本不存在的标题而失败（实测：
/// 「格式」组平铺后，`top_menu_test` 里找「格式」标题的断言全红）。
///
/// 现在渲染与用例都从这个函数取值，「用例检查的控件」与「实际画出的控件」
/// 不可能再对不上：改分组形态只需改这里一处。
pub fn toolbar_controls() -> Vec<ToolbarControl> {
    let mut out = Vec::new();
    for (title, actions) in editor_toolbar_groups() {
        if title == FLAT_GROUP {
            out.extend(actions.into_iter().map(ToolbarControl::Flat));
        } else {
            out.push(ToolbarControl::Dropdown(title));
        }
    }
    out
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
/// 两档共用同一套**尺寸 / 圆角 / 配色 / 间距**（等高、齐平、同色，见
/// [`control_height`] 与 [`control_button`] 的说明），差别在内容形态：
/// 平铺档是**纯图形图标按钮**（[`icon_button`]，语义靠悬停），
/// 下拉档是**分组标题 + 三角**（标题是分组名，不是动作名，故保留文字）。
/// 用户看到的因此仍是「一排同类控件」，而不是两种混排。
///
/// 工具栏位于**编辑区顶部**（上下布局：工具栏在上、文本在下），作用域与编辑区
/// 内容一致，见 `editor_view::show`。
pub fn draw_editor_toolbar(vctx: &mut dyn EditorViewContext, ui: &mut Ui) {
    // 主题先取出来（`vctx` 在闭包里还要可变借用，不能同时持有 `&Theme`）。
    let theme = vctx.theme_state().clone();
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
                        draw_group_dropdown(vctx, ui, &theme, title, &actions, h)
                    };
                    pending = pending.or(hit);
                }
            });
        });

    // 统一在此执行：菜单项与工具栏、快捷键、命令面板走**同一条**路径，
    // 这正是「三入口一致」得以成立的地方。
    if let Some(action) = pending {
        vctx.execute(action);
    }
}

/// 平铺图标按钮的**图形边长**（点）。
///
/// 16×16 设计栅格（见 [`crate::presentation::icons`]）缩放到 17pt：
/// 30pt 的按钮里上下左右各留约 6.5pt 呼吸位，图标既不被边框咬住，
/// 也不会小到失去辨识。
const ICON_SIZE: f32 = 17.0;

/// 一个平铺动作的**纯图形按钮**：固定正方形、向量图标、悬停提示补语义。
///
/// # 为什么手绘而不继续用 `egui::Button`
///
/// `Button` 的内容只能是文字（或另一个 widget），装不进向量图形；而把图形
/// 塞进 `Button` 又会回到「先量文案宽、再画图形」的老路。这里按项目既有
/// 的图标按钮范式手绘（与标签条「全部关闭」按钮同源）：
///
/// - **底色与 egui `Button` 完全同源**：取 `visuals.widgets.style(&response)`
///   的 `weak_bg_fill`——常态 / 悬停 / 按下分别落在主题映射的
///   `bg_tertiary` / `accent_soft` / `accent_soft` 上，不是另抄一套颜色；
/// - **图标色与按钮文字色同源**：`style.fg_stroke.color`，于是文字按钮与
///   图标按钮在任何主题下深浅一致；
/// - **可发现性靠悬停**：按钮上没有文字，完整说法（「提升标题级别」这种）
///   由 `on_hover_text(action.label())` 补足——与「全部关闭」图标按钮
///   同一哲学（041 起该按钮即如此）。
fn icon_button(ui: &mut Ui, theme: &Theme, action: UiAction, h: f32) -> egui::Response {
    // 固定正方形：一行平铺按钮因此是一条整齐的栅格。
    let (rect, response) = ui.allocate_exact_size(egui::vec2(h, h), egui::Sense::click());
    let style = ui.visuals().widgets.style(&response);
    ui.painter()
        .rect_filled(rect, control_radius(&theme.metrics), style.weak_bg_fill);
    let colors = icons::IconColors {
        primary: style.fg_stroke.color,
        accent: theme.accent,
    };
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(ICON_SIZE));
    // R-05：动作 → 图标键 的映射上移到 `presentation::icon_key_for`。
    // 无图标键的动作（返回 `None`）跳过绘制，与旧行为一致。
    if let Some(key) = crate::presentation::icon_key_for(action) {
        icons::paint_icon(ui.painter(), icons::Icon::Action(key), icon_rect, colors);
    }
    response
}

/// 把「格式」分组的动作平铺成按钮。
///
/// 组内间距取 `space_xs`（比组间的分隔线间距小一档），于是「同组」在视觉上聚拢、
/// 「不同组」被分隔线自然切开——分组关系不需要额外的分组标题来说明。
///
/// 按钮上**只有图形**（见 [`icon_button`]），完整说法由**悬停提示**补足：
/// 可发现性靠悬停保证，不靠把按钮撑宽。返回被点击的动作（没有则为 `None`），
/// 由调用方统一走 `vctx.execute`。
fn draw_flat_actions(ui: &mut Ui, theme: &Theme, actions: &[UiAction], h: f32) -> Option<UiAction> {
    let mut hit = None;
    for (i, action) in actions.iter().enumerate() {
        if i > 0 {
            ui.add_space(theme.metrics.space_xs);
        }
        let resp = icon_button(ui, theme, *action, h).on_hover_text(action.label());
        if resp.clicked() {
            hit = Some(*action);
        }
    }
    hit
}

/// 低频分组：一个与平铺按钮**同规格**的按钮 + 其右侧的下拉菜单。
///
/// # 为什么不用 `ComboBox`
///
/// `ComboBox` 的按钮宽度默认取 `Spacing::combo_width`（100pt），与文案长度无关——
/// 「插入」与「列表/结构」因此一样宽，横向空间被白白吃掉，宽度也不再传递
/// 「这个分组有多重」的信息；它的高度也不受控（实测 23，与 `ui.button` 的 22 错位）。
/// 换成 `Button` + [`egui::containers::menu::MenuButton`]（egui 的菜单按钮本身就是
/// `Button`）之后，触发器宽度自然跟随文案。
///
/// # 与平铺图标按钮如何保持同规格
///
/// 触发器要在右侧挂三角（`right_text`），无法与纯图形的 [`icon_button`] 合并
/// 成一个函数，但**尺寸 / 圆角 / 配色全部同源**：高度取 [`control_height`]、
/// 圆角取 [`control_radius`]，底色与文字色走 egui 的
/// `visuals.widgets.{inactive,hovered,active}`——主题映射已把三者统一
/// （见本文件头「视觉规范」一节），因此不是把颜色抄两遍。
fn draw_group_dropdown(
    vctx: &mut dyn EditorViewContext,
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
        // `&*vctx` 是不可变重借用——菜单项只读主题与注册表。
        let row_w = menu_row_width(ui, &*vctx, actions);
        for action in actions {
            if menu_item(ui, &*vctx, *action, row_w).clicked() {
                clicked = Some(*action);
            }
        }
    });
    clicked
}
