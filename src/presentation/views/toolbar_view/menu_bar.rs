//! 第 1 层：应用级菜单栏（只渲染 `MenuGroup::APP_LEVEL`）与菜单项绘制。

use egui::{Align, Layout, Ui};

use crate::presentation::shortcuts::registry::{MenuGroup, UiAction};
use crate::presentation::view_context::EditorViewContext;

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
/// 规定菜单栏占 `Metrics::topbar_height`），所有按钮在同一高度内居中，
/// 自然等高同排。
pub(super) fn draw_app_menu_bar(vctx: &mut dyn EditorViewContext, ui: &mut Ui) {
    let m = vctx.theme_state().metrics;
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
                // `&*vctx` 是不可变重借用——`menu_row_width` 只读主题与注册表，
                // 而外层稍后还要可变地借 `vctx` 去 `execute`。
                let row_w = menu_row_width(ui, &*vctx, &actions);

                let mut clicked = None;
                egui::ComboBox::from_id_salt(("menu", group))
                    .selected_text(group.label())
                    .show_ui(ui, |ui| {
                        for action in &actions {
                            if menu_item(ui, &*vctx, *action, row_w).clicked() {
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
        vctx.execute(action);
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
pub fn menu_row_width(ui: &Ui, vctx: &dyn EditorViewContext, actions: &[UiAction]) -> f32 {
    let font = egui::FontId::proportional(vctx.theme_state().metrics.ui_font_size);
    let registry = vctx.dispatcher_state().registry();
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
pub(super) fn menu_item(
    ui: &mut Ui,
    vctx: &dyn EditorViewContext,
    action: UiAction,
    row_w: f32,
) -> egui::Response {
    let font = egui::FontId::proportional(vctx.theme_state().metrics.ui_font_size);
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
        let combo = vctx.dispatcher_state().registry().display_for(action);
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
