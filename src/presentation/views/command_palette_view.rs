//! 命令面板（S6-12，对齐原型 A-24 `CommandPalette.vue` + `lib/commands.ts`）。
//!
//! # 键位在浮层内自己处理，不走全局分发器
//!
//! 与快速打开（S6-11f）同一理由：↑↓ / Enter / Esc 属于面板自身导航，
//! 塞进全局动作表就要为它加例外，而例外会越加越多、互相干扰。
//! 原型是 `<input @keydown.down.prevent>`，egui 没有「输入框内 keydown」，
//! 故用 `ctx.input(|i| i.events)` 读原始事件。
//!
//! # 命令清单**不在这里构造**
//!
//! 取自 [`ActionTable::all`] / [`ActionTable::search`]——即菜单与
//! 快捷键一览用的是**同一张动作表**。本视图不持有任何命令列表，
//! 「面板能搜到的 = 菜单能看到的」这条契约因此是**结构性成立**的，
//! 而不是靠两边同步维护。
//!
//! # 模态：背景不可交互
//!
//! 原型用 `aria-modal="true"` + `Tab` 焦点陷阱。egui 无此语义，
//! 折中做法是**浮层打开时吞掉全局快捷键**（见 `frame::collect_actions`
//! 的 `openable_overlay` 规则），使背景既收不到快捷键也收不到点击
//! ——egui 的模态 `Window` 会遮挡下方区域，点击落在面板外即关闭。

use egui::{Align, Context, Layout, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::command_palette_ui_state::CommandRow;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::theme::builtin::Theme;

/// 对话框默认尺寸（对齐原型 `.palette { width: 540px }`）。
const WIDTH: f32 = 540.0;
const HEIGHT: f32 = 460.0;
/// 搜索框高度（原型 `.search` 内边距 + 输入行）。
const SEARCH_H: f32 = 40.0;
/// 每行高度（原型 `.row { height: 34px }`）。
const ROW_H: f32 = 34.0;
/// 搜索框的固定 `Id`。
pub const COMMAND_PALETTE_INPUT_ID: &str = "rustmark.command_palette.search";

/// 绘制命令面板。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::CommandPalette.title())
        .id(egui::Id::new("rustmark.overlay.command_palette"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    // 打开即聚焦搜索框：用户的下一个动作一定是打字。
    if !ctx.memory(|m| m.has_focus(egui::Id::new(COMMAND_PALETTE_INPUT_ID)))
        && !ctx.memory(|m| m.has_focus(egui::Id::new("rustmark.overlay.command_palette")))
    {
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(COMMAND_PALETTE_INPUT_ID)));
    }
    if !open {
        app.command_palette_ui.open();
        app.dialog.close(Overlay::CommandPalette);
    }
    handle_keys(app, ctx);
}

/// 处理面板内的 ↑↓ / Enter / Esc。
///
/// 判据是「**本帧**有按键事件」而非「焦点在搜索框」——
/// 后者在 egui 里每帧都成立，会导致按住 ↑ 时反复触底。
fn handle_keys(app: &mut EditorApp, ctx: &Context) {
    if !app.dialog.is_open(Overlay::CommandPalette) {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    let mut chosen: Option<CommandRow> = None;
    let mut closed = false;
    for event in &events {
        let egui::Event::Key { key, pressed, .. } = event else {
            continue;
        };
        if !pressed {
            continue;
        }
        match key {
            egui::Key::ArrowDown => {
                let total = rows(app).len();
                app.command_palette_ui.move_selection(1, total);
            }
            egui::Key::ArrowUp => {
                let total = rows(app).len();
                app.command_palette_ui.move_selection(-1, total);
            }
            egui::Key::Enter => {
                let list = rows(app);
                chosen = list.get(app.command_palette_ui.selected).cloned();
            }
            egui::Key::Escape => closed = true,
            _ => {}
        }
    }
    if closed {
        app.command_palette_ui.open();
        app.dialog.close(Overlay::CommandPalette);
        return;
    }
    if let Some(row) = chosen {
        apply_pick(app, &row);
    }
}

/// 命令行列表（按查询过滤）。
///
/// 结果数可能超出一屏，故**不截断**——命令面板的价值就在「能翻到
/// 第 30 条」，截到 12 条会让它退化成快速打开面板。
pub fn rows(app: &EditorApp) -> Vec<CommandRow> {
    app.dispatcher()
        .registry()
        .table()
        .search(&app.command_palette_ui.query)
        .into_iter()
        .map(|s| CommandRow {
            entry: s.entry,
            hits: s.hits,
        })
        .collect()
}

/// 主体。
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    search_box(app, ui);
    let list = rows(app);
    // 结果数随查询变化 → 夹紧选中项，否则会「高亮一个不存在的行」。
    app.command_palette_ui.clamp(list.len());
    results(app, ui, &theme, &list);
    ui.separator();
    footer(ui, &theme, list.is_empty());
}

/// 页脚：操作提示 + 命令总数。
fn footer(ui: &mut Ui, theme: &Theme, empty: bool) {
    ui.horizontal(|ui| {
        // 空列表时说「↑ ↓ 选择」是骗人：没有可选项。
        let hint = if empty {
            "没有匹配的命令 · Esc 关闭"
        } else {
            "↑ ↓ 选择 · Enter 执行 · Esc 关闭"
        };
        ui.colored_label(theme.text_secondary, hint);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let total = crate::presentation::shortcuts::registry::UiAction::ALL.len();
            ui.colored_label(
                theme.text_secondary.gamma_multiply(0.8),
                format!("共 {total} 条命令"),
            );
        });
    });
}

/// 搜索框。
fn search_box(app: &mut EditorApp, ui: &mut Ui) {
    let mut query = app.command_palette_ui.query.clone();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SEARCH_H),
        egui::Sense::click(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, app.theme().bg_primary);
    // 真实输入能力靠 `TextEdit`：外层仍然自己画背景与分隔线。
    let edit = ui.put(
        rect,
        egui::TextEdit::singleline(&mut query)
            .id(egui::Id::new(COMMAND_PALETTE_INPUT_ID))
            .frame(false)
            .desired_width(rect.width() - 24.0)
            .text_color(app.theme().text_primary)
            .hint_text("输入命令，如「保存」「主题」「预览」…"),
    );
    if edit.changed() {
        app.command_palette_ui.set_query(&query);
    }
    if response.clicked() {
        ui.memory_mut(|m| m.request_focus(egui::Id::new(COMMAND_PALETTE_INPUT_ID)));
    }
}

/// 结果列表。返回被点击的行下标（`None` = 未点击）。
fn results(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, list: &[CommandRow]) {
    if list.is_empty() {
        ui.add_space(theme.metrics.space_xl);
        ui.centered_and_justified(|ui| {
            ui.vertical(|ui| {
                ui.colored_label(theme.text_secondary, "没有匹配的命令");
                ui.add_space(theme.metrics.space_xs);
                ui.colored_label(
                    theme.text_secondary.gamma_multiply(0.75),
                    "试试「保存」「撤销」「主题」「导出」",
                );
            });
        });
        return;
    }
    let mut picked: Option<usize> = None;
    let selected = app.command_palette_ui.selected;
    ScrollArea::vertical()
        .id_salt("rustmark.command_palette.list")
        .max_height(HEIGHT - SEARCH_H - 80.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, row) in list.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), ROW_H),
                    egui::Sense::click(),
                );
                // `Response::hovered()` / `clicked()` 取 bool 后即可释放
                // 借用——`on_hover_text` 是**按值**收 `Response` 的，
                // 直接链式调用会 move 掉它，后面再用就编译不过。
                let hovered = response.hovered();
                let clicked = response.clicked();
                if i == selected {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), theme.accent_soft);
                } else if hovered {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), theme.bg_tertiary);
                }
                draw_label(ui, theme, row, rect, i == selected);
                draw_meta(ui, theme, row, rect);
                if hovered {
                    response.on_hover_text(row.entry.label.to_string());
                }
                if clicked {
                    picked = Some(i);
                }
            }
        });
    if let Some(i) = picked {
        apply_pick(app, &list[i]);
    }
}

/// 左侧标签：命中字符用强调色 + 加粗（对齐原型 `.label :deep(mark.hl)`）。
///
/// 原型用 `<mark>` 包裹命中字符并染成 `accent` 且 `font-weight: 700`。
/// 这里按**字符下标**逐段切分渲染——`hits` 是字符坐标不是字节偏移，
/// 直接按字节切会把中文标签劈开。
fn draw_label(ui: &mut Ui, theme: &Theme, row: &CommandRow, rect: egui::Rect, active: bool) {
    let text: Vec<char> = row.entry.label.chars().collect();
    let base = if active {
        theme.accent
    } else {
        theme.text_primary
    };
    let x0 = rect.left() + 8.0;
    let center_y = rect.center().y;
    if row.hits.is_empty() {
        // 无命中（未查询，或只命中英文关键词）→ 整段一种颜色。
        let galley = ui.painter().layout_no_wrap(
            row.entry.label.to_string(),
            egui::FontId::proportional(13.0),
            base,
        );
        ui.painter().galley(
            egui::pos2(x0, center_y - galley.size().y / 2.0),
            galley,
            base,
        );
        return;
    }
    // 有命中：命中段染强调色，其余用基色。
    // 越界的下标直接跳过而不是 panic——`hits` 来自关键词匹配时
    // 可能超出标签长度（`registry::search` 已把那种情况置空，
    // 这里再兜一层，因为越界切片会终止进程）。
    let mut cursor = x0;
    let mut idx = 0usize;
    while idx < text.len() {
        let hit = row.hits.contains(&idx);
        let start = idx;
        while idx < text.len() && row.hits.contains(&idx) == hit {
            idx += 1;
        }
        let segment: String = text[start..idx].iter().collect();
        let color = if hit { theme.accent } else { base };
        let galley = ui.painter().layout_no_wrap(
            segment,
            egui::FontId::proportional(if hit { 13.5 } else { 13.0 }),
            color,
        );
        // 宽度必须在 `galley` 被消耗前取（`Painter::galley` 按值收）。
        let w = galley.size().x;
        ui.painter().galley(
            egui::pos2(cursor, center_y - galley.size().y / 2.0),
            galley,
            color,
        );
        cursor += w;
    }
}

/// 右侧元信息：分组徽标 + 快捷键（对齐原型 `.meta`）。
fn draw_meta(ui: &mut Ui, theme: &Theme, row: &CommandRow, rect: egui::Rect) {
    let mut x = rect.right() - 8.0;
    // 快捷键（等宽，原型 `.keys`）。
    if !row.entry.combo.is_empty() {
        let galley = ui.painter().layout_no_wrap(
            row.entry.combo.clone(),
            egui::FontId::monospace(11.0),
            theme.text_secondary,
        );
        let w = galley.size().x;
        x -= w;
        ui.painter().galley(
            egui::pos2(x, rect.center().y - galley.size().y / 2.0),
            galley,
            theme.text_secondary,
        );
        x -= 8.0;
    }
    // 分组徽标（原型 `.group`，带边框的小标签）。
    let galley = ui.painter().layout_no_wrap(
        row.group().to_string(),
        egui::FontId::proportional(11.0),
        theme.text_secondary,
    );
    let w = galley.size().x;
    let badge = egui::Rect::from_min_size(
        egui::pos2(x - w - 10.0, rect.center().y - 9.0),
        egui::vec2(w + 10.0, 18.0),
    );
    ui.painter().rect_stroke(
        badge,
        egui::CornerRadius::same(3),
        egui::Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(badge.left() + 5.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme.text_secondary,
    );
}

/// 执行选中的命令，并关闭面板。
///
/// 走 [`EditorApp::execute`] 而**不是**直接调动作的具体逻辑：
/// 面板与菜单共用分发器，才能保证「面板里能干的，菜单里也能干」。
///
/// 顺序：先关面板再执行。反过来的话，执行动作若又打开别的浮层
/// （比如「导出」），会出现「面板关掉 → 新面板打开 → 旧面板的关闭
/// 逻辑又把新面板关掉」的次序问题。
///
/// # 为什么是 `pub`
///
/// 它是「选中项 → 执行」的**唯一入口**（点击与 Enter 共用），
/// 私有就没法从集成测试驱动——第一版测试因此只能调
/// `EditorApp::execute`，**整条 `apply_pick` 路径一次都没跑到**，
/// 却因为别的原因变红，看起来「验过了」。
pub fn apply_pick(app: &mut EditorApp, row: &CommandRow) {
    app.command_palette_ui.open();
    app.dialog.close(Overlay::CommandPalette);
    app.execute(row.action());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_constants_match_the_prototype() {
        // 原型 `.palette { width: 540px }` / `.row { height: 34px }`。
        assert_eq!(WIDTH, 540.0);
        assert_eq!(ROW_H, 34.0);
    }

    /// 高亮下标越界时必须**跳过**而不是切片 panic。
    ///
    /// 这是纯函数的等价逻辑（`draw_label` 里的分段循环），
    /// 在此把边界钉住：真实渲染里 `panic = "abort"` 会终止进程。
    #[test]
    fn out_of_range_hits_are_skipped_rather_than_panicking() {
        let chars: Vec<char> = "加粗".chars().collect();
        // 模拟 `hits` 越界（长度 2 的标签却收到下标 0..4）。
        let hits = [0usize, 1, 2, 3];
        let mut segments = 0usize;
        let mut idx = 0usize;
        while idx < chars.len() {
            let hit = hits.contains(&idx);
            while idx < chars.len() && hits.contains(&idx) == hit {
                idx += 1;
            }
            segments += 1;
        }
        assert_eq!(segments, 1, "两个字符都是命中 → 合成一段");
        assert_eq!(idx, chars.len(), "循环必须终止于标签末尾而非 hits 长度");
    }
}
