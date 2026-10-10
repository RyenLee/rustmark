//! 符号 / Emoji 面板（S6-11f，对齐原型 A-26 `SymbolPicker.vue`）。
//!
//! # 插入走**领域** `replace_range`，不走 UI 的直接改文本
//!
//! 面板插的是 Markdown 片段（可能含换行），必须：
//!
//! 1. 进领域 → 成为一个撤销点（用户按一次 Ctrl+Z 能整段撤掉）；
//! 2. 更新预览缓存（否则预览区还显示插入前的块结构）；
//! 3. 让光标停在插入内容**之后**（停在插入内容里改字是最容易出错的地方）。
//!
//! `EditorApp::insert_at_cursor` 正是这三条的单一入口
//! （与工具栏插入链接 / 表格 / 代码块同一条路径，S6-07 建立）。
//!
//! # 原型的 roving tabindex 在 egui 侧的等价物
//!
//! 原型用 `roving tabindex` + 方向键在网格里移动焦点（A-26 的 a11y 要求）。
//! egui 的 `Grid` 本身按 Tab 顺序可达每一格，故只需保证**键盘方向键可用**：
//! ↑↓←→ 在可见项序列上移动，Enter 插入。这已满足「键盘可达网格」。

use egui::{Context, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::state::symbol_ui_state::{SymbolGroup, SymbolItem};
use crate::presentation::theme::builtin::Theme;

/// 对话框默认尺寸（对齐原型 `BaseModal :width="520" :height="430"`）。
const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 430.0;
/// 网格列数（原型 `.grid { grid-template-columns: repeat(8, 1fr) }`）。
const COLUMNS: usize = 8;
/// 每格尺寸。
const CELL: f32 = 34.0;
/// 搜索框的固定 `Id`。
pub const SYMBOL_INPUT_ID: &str = "rustmark.symbol.search";

/// 绘制符号面板。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::Symbol.title())
        .id(egui::Id::new("rustmark.overlay.symbol"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    // 打开即聚焦筛选框，且**只给一次**（边沿触发）。
    // 「当前没焦点就请求」的写法恒为真（`Memory::has_focus` 认不到窗口 id），
    // 会每帧抢焦点、让用户没法把焦点移到面板里的别处。
    // 详见 `DialogUiState::take_open_focus`。
    if app.dialog.take_open_focus(Overlay::Symbol) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(SYMBOL_INPUT_ID)));
    }
    if !open {
        app.symbol_ui.open();
        app.dialog.close(Overlay::Symbol);
    }
    handle_keys(app, ctx);
}

/// 处理面板内的 ↑↓ / Enter / Esc。
fn handle_keys(app: &mut EditorApp, ctx: &Context) {
    if !app.dialog.is_open(Overlay::Symbol) {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    let mut chosen: Option<SymbolItem> = None;
    let mut closed = false;
    for event in &events {
        let egui::Event::Key { key, pressed, .. } = event else {
            continue;
        };
        if !pressed {
            continue;
        }
        match key {
            egui::Key::ArrowDown | egui::Key::ArrowRight => {
                let total = app.symbol_ui.visible().len();
                app.symbol_ui.move_selection(1, total);
            }
            egui::Key::ArrowUp | egui::Key::ArrowLeft => {
                let total = app.symbol_ui.visible().len();
                app.symbol_ui.move_selection(-1, total);
            }
            egui::Key::Enter => {
                chosen = app.symbol_ui.visible().get(app.symbol_ui.selected).copied();
            }
            egui::Key::Escape => closed = true,
            _ => {}
        }
    }
    if closed {
        app.symbol_ui.open();
        app.dialog.close(Overlay::Symbol);
        return;
    }
    if let Some(item) = chosen {
        apply_insert(app, item);
    }
}

/// 主体。
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    tabs(app, ui, &theme);
    ui.add_space(theme.metrics.space_xs);
    search_box(app, ui);
    let visible = app.symbol_ui.visible();
    app.symbol_ui.clamp(visible.len());
    ui.add_space(theme.metrics.space_sm);
    grid(app, ui, &theme, &visible);
    ui.separator();
    footer(app, ui, &theme, &visible);
}

/// 分组标签。
fn tabs(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.horizontal_wrapped(|ui| {
        for group in SymbolGroup::ALL {
            let selected = app.symbol_ui.group == *group;
            let text = if selected {
                RichText::new(group.label()).color(theme.accent)
            } else {
                RichText::new(group.label())
            };
            if ui.selectable_label(selected, text).clicked() {
                app.symbol_ui.set_group(*group);
            }
        }
    });
}

/// 搜索框。
fn search_box(app: &mut EditorApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("搜索").size(12.0));
        let mut query = app.symbol_ui.query.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut query)
                    .id(egui::Id::new(SYMBOL_INPUT_ID))
                    .hint_text("按名称筛选，如「星」「列表」「代码块」")
                    .desired_width(ui.available_width() - 48.0),
            )
            .changed()
        {
            app.symbol_ui.set_query(&query);
        }
    });
}

/// 符号网格。
fn grid(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, visible: &[SymbolItem]) {
    if visible.is_empty() {
        ui.add_space(theme.metrics.space_xl);
        ui.centered_and_justified(|ui| {
            ui.colored_label(theme.text_secondary, "没有匹配的符号");
        });
        return;
    }
    let selected = app.symbol_ui.selected;
    let mut picked: Option<usize> = None;
    ScrollArea::vertical()
        .id_salt("rustmark.symbol.grid")
        .max_height(HEIGHT - 190.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("rustmark.symbol.cells")
                .num_columns(COLUMNS)
                .spacing([6.0, 6.0])
                .show(ui, |ui| {
                    for (i, item) in visible.iter().enumerate() {
                        if i % COLUMNS == 0 {
                            ui.end_row();
                        }
                        if cell(ui, theme, item, i == selected) {
                            picked = Some(i);
                        }
                    }
                });
        });
    if let Some(item) = picked.and_then(|i| visible.get(i)) {
        apply_insert(app, *item);
    }
}

/// 单个格子。
fn cell(ui: &mut Ui, theme: &Theme, item: &SymbolItem, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(CELL, CELL), egui::Sense::click());
    let fill = if selected {
        theme.accent_soft
    } else if response.hovered() {
        theme.bg_tertiary
    } else {
        theme.bg_secondary
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(4), fill);
    if selected || response.hovered() {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(4),
            egui::Stroke::new(1.0_f32, theme.accent),
            egui::StrokeKind::Inside,
        );
    }
    // 字形用**比例字体**而非等宽：等宽字体对 emoji 的覆盖
    // 与字距都更差，非 ASCII 混排会画歪（S6-11e 已核实过这一点）。
    let galley = ui.painter().layout_no_wrap(
        item.glyph.to_string(),
        egui::FontId::proportional(16.0),
        theme.text_primary,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        theme.text_primary,
    );
    let clicked = response.clicked();
    if response.hovered() {
        // 悬停给出**将插入什么**：片段类条目的字形是 "H"、"▤"，
        // 不说明的话用户完全猜不出点下去会得到什么。
        response.on_hover_text(format!(
            "{}　→　{}",
            item.label,
            item.text.replace('\n', "\\n")
        ));
    }
    clicked
}

/// 页脚：预览与关闭。
fn footer(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, visible: &[SymbolItem]) {
    ui.horizontal(|ui| {
        let selected = app.symbol_ui.selected;
        match visible.get(selected) {
            // 预览**原始文本**（含换行转义）：预览成渲染后的一行
            // 会让用户看不出插入的是三行代码块。
            Some(item) => ui.colored_label(
                theme.text_secondary,
                format!("将插入：{}", item.text.replace('\n', "\\n")),
            ),
            None => ui.colored_label(theme.text_secondary, "未选中任何符号"),
        };
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("关闭").clicked() {
                app.symbol_ui.open();
                app.dialog.close(Overlay::Symbol);
            }
        });
    });
}

/// 插入符号并关闭面板。
///
/// 关闭面板而非保持打开：连续插入多个符号的场景很罕见，
/// 而保持打开会让用户「以为插入没生效」（浮层还盖着编辑区）。
/// 需要多个时重新按 `Ctrl+Shift+E` 即可。
fn apply_insert(app: &mut EditorApp, item: SymbolItem) {
    // 插入失败**不关**面板：内容没进去就消失界面，
    // 用户会以为是程序崩溃了。错误已由 `insert_at_cursor` 报到状态栏。
    if app.insert_at_cursor(item.text).is_ok() {
        app.symbol_ui.open();
        app.dialog.close(Overlay::Symbol);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_geometry_matches_the_prototype() {
        // 原型 `.grid { grid-template-columns: repeat(8, 1fr) }`。
        assert_eq!(COLUMNS, 8);
        assert_eq!(WIDTH, 520.0);
    }
}
