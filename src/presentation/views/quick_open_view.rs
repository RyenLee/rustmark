//! 快速打开浮层（S6-11f，对齐原型 A-25 `QuickOpen.vue`）。
//!
//! # 键位在**浮层内**自己处理，不走全局分发器
//!
//! 原型是 `<input @keydown.down.prevent>`：焦点在输入框里时按 ↓
//! 移动高亮、Enter 打开、Esc 关闭。egui 没有「输入框内的 keydown」，
//! 所以本视图用 `ctx.input(|i| i.events)` 直接读原始按键事件。
//!
//! # 为什么不用 `collect_actions`
//!
//! `EditorApp::collect_actions` 里有一条「查找框聚焦时全局快捷键让位」
//! 的规则（S6-08 修的缺陷 4）。若把快速打开的 ↑↓ 也塞进分发器，
//! 就得再为它加一条例外，而例外会越加越多、互相干扰。
//! **面板内部的导航键属于面板自己**，不进全局动作表——
//! 命令面板同理（S6-12）。

use egui::{Align, Context, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::state::quick_open_ui_state::{
    MAX_CANDIDATES, QuickOpenItem, candidates_from_workspace, filter_candidates,
};
use crate::presentation::theme::builtin::Theme;

/// 对话框默认尺寸（对齐原型 `.palette { width: 480px }`）。
const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 420.0;
/// 搜索框高度（原型 `.search { height: 38px }`）。
const SEARCH_H: f32 = 38.0;
/// 每行高度（原型 `.row { height: 28px }`）。
const ROW_H: f32 = 28.0;
/// 搜索框的固定 `Id`。
pub const QUICK_OPEN_INPUT_ID: &str = "rustmark.quick_open.search";

/// 绘制快速打开浮层。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::QuickOpen.title())
        .id(egui::Id::unique("rustmark.overlay.quick_open"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    // 面板一打开焦点就落在搜索框：用户的下一个动作一定是打字。
    //
    // 只给一次（边沿触发）：「当前没焦点就请求」那种写法恒为真——
    // `Memory::has_focus` 认不到窗口（`Area`）的 id，于是每帧都抢焦点，
    // 用户点不开别处。详见 `DialogUiState::take_open_focus`。
    if app.dialog.take_open_focus(Overlay::QuickOpen) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique(QUICK_OPEN_INPUT_ID)));
    }
    if !open {
        app.quick_open_ui.open();
        app.dialog.close(Overlay::QuickOpen);
    }
    handle_keys(app, ctx);
}

/// 处理面板内的 ↑↓ / Enter / Esc。
///
/// 判据是「**本帧**有按键事件」而非「焦点在搜索框」——
/// 后者在 egui 里每帧都成立，会导致按住 ↑ 时反复触底。
fn handle_keys(app: &mut EditorApp, ctx: &Context) {
    if !app.dialog.is_open(Overlay::QuickOpen) {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    let mut chosen: Option<QuickOpenItem> = None;
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
                let total = candidates(app).len();
                app.quick_open_ui.move_selection(1, total);
            }
            egui::Key::ArrowUp => {
                let total = candidates(app).len();
                app.quick_open_ui.move_selection(-1, total);
            }
            egui::Key::Enter => {
                let list = candidates(app);
                chosen = list.get(app.quick_open_ui.selected).cloned();
            }
            egui::Key::Escape => closed = true,
            _ => {}
        }
    }
    if closed {
        app.quick_open_ui.open();
        app.dialog.close(Overlay::QuickOpen);
        return;
    }
    if let Some(item) = chosen {
        apply_pick(app, &item);
    }
}

/// 候选列表（工作区条目 + 最近文件，按查询过滤）。
pub fn candidates(app: &EditorApp) -> Vec<QuickOpenItem> {
    let root = app.workspace_entries.root.clone().unwrap_or_default();
    let all = candidates_from_workspace(
        &root,
        &app.workspace_entries.items,
        &app.recent_files(),
        // 过滤前给足额度：过滤发生在候选构造**之后**，
        // 若构造时就截到 12 条，输入查询后连匹配项都凑不满。
        MAX_CANDIDATES * 8,
    );
    filter_candidates(&all, &app.quick_open_ui.query, MAX_CANDIDATES)
}

/// 主体。
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    search_box(app, ui);
    let list = candidates(app);
    // 候选数可能因查询而变少 → 夹紧选中项，否则会「高亮一个不存在的行」。
    app.quick_open_ui.clamp(list.len());
    results(app, ui, &theme, &list);
    ui.separator();
    footer(app, ui, &theme, list.is_empty());
}

/// 页脚：系统文件对话框入口 + 操作提示。
///
/// # 为什么这里要有「浏览本机文件…」
///
/// `Ctrl+O` 是**本浮层**的开启动作（`Overlay::QuickOpen.opened_by`），
/// 原生文件对话框在 `execute` 里拿不到入口——那会写出一段
/// 永远不可达的分支（`openable_overlay` 先把 `Ctrl+O` 拦走了）。
/// 面板内留一个显式按钮，是把「工作区外的文件也能打开」这件事
/// 交给用户一次点击，而不是让它消失。
fn footer(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, empty: bool) {
    ui.horizontal(|ui| {
        if ui.button("浏览本机文件…").clicked() {
            app.quick_open_ui.open();
            app.dialog.close(Overlay::QuickOpen);
            app.request_dialog(crate::app::frame::PendingDialog::OpenFile);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 空列表时说「↑ ↓ 选择」是骗人：没有可选项。
            let hint = if empty {
                "没有匹配项 · Esc 关闭"
            } else {
                "↑ ↓ 选择 · Enter 打开 · Esc 关闭"
            };
            ui.colored_label(theme.text_secondary, hint);
        });
    });
}

/// 搜索框。
///
/// # 文字只由 `TextEdit` 画一遍（问题 037）
///
/// 旧实现在这里**手绘**了一份占位/查询文本的 galley，又在同一矩形上
/// `ui.put` 了一个带 `hint_text` 的 `TextEdit`——同一段字被画两遍
/// （错开一两个像素），用户看到的就是「搜索栏有重叠的重影文案」。
/// 手绘那份是多余的：`TextEdit` 自己会按焦点态渲染查询文本与占位提示，
/// 颜色也已由 `.text_color` / 主题的 `weak_text_color` 给出。
/// 这里只保留框底与点击聚焦。
fn search_box(app: &mut EditorApp, ui: &mut Ui) {
    let mut query = app.quick_open_ui.query.clone();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SEARCH_H),
        egui::Sense::click(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, app.theme().bg_primary);
    // 真实输入能力靠 `TextEdit`：外层仍然自己画边框与分隔线。
    let edit = ui.put(
        rect,
        egui::TextEdit::singleline(&mut query)
            .id(egui::Id::unique(QUICK_OPEN_INPUT_ID))
            .frame(egui::Frame::NONE)
            .desired_width(rect.width())
            .text_color(app.theme().text_primary)
            .hint_text("按文件名快速打开…"),
    );
    if edit.changed() {
        app.quick_open_ui.set_query(&query);
    }
    if response.clicked() {
        ui.memory_mut(|m| m.request_focus(egui::Id::unique(QUICK_OPEN_INPUT_ID)));
    }
}

/// 结果列表。返回被点击的行下标（`None` = 未点击）。
///
/// 行内文件名必须**截断到剩余宽**：右侧还排着来源分组（「工作区」等），
/// 旧实现用 `layout_no_wrap` 画全名，长文件名会直接盖在分组文字上
/// （与欢迎页最近文件同一缺陷类）。截断只做一个 galley（缺陷类 033：
/// 多个截断标签会把固定宽容器撑出面板）。
fn results(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, list: &[QuickOpenItem]) {
    if list.is_empty() {
        ui.add_space(theme.metrics.space_xl);
        ui.centered_and_justified(|ui| {
            ui.colored_label(theme.text_secondary, "没有匹配的文件");
        });
        return;
    }
    let mut picked: Option<usize> = None;
    let selected = app.quick_open_ui.selected;
    ScrollArea::vertical()
        .id_salt("rustmark.quick_open.list")
        .max_height(HEIGHT - SEARCH_H - 80.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, item) in list.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), ROW_H),
                    egui::Sense::click(),
                );
                if i == selected {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), theme.accent_soft);
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), theme.bg_tertiary);
                }
                // 文字色三分（问题 045，与文件树同一判据）：白名单外的文件
                // 用次要色——快速打开列的是**工作区全部文件**，其中图片 /
                // 二进制打不开；不置灰的话用户会以为「点了没反应」。
                // 选中态一律用正文色：全仓纪律（037 / 039）——选中底色在
                // 用户自定义主题里可以是实色强调蓝，强调色文字压上去会糊掉。
                let openable = app.can_open_as_text(&item.path);
                let name_color = if openable {
                    theme.text_primary
                } else {
                    theme.text_secondary
                };
                // 右侧显示分组来源：两个同名文件分属不同来源时，
                // 只看名字分不清（对齐原型的 `aria-selected` 语义）。
                let group = ui.painter().layout_no_wrap(
                    item.group.label().to_string(),
                    egui::FontId::proportional(11.0),
                    theme.text_secondary,
                );
                let group_w = group.size().x;
                ui.painter().galley(
                    egui::pos2(
                        rect.right() - 8.0 - group_w,
                        rect.center().y - group.size().y / 2.0,
                    ),
                    group,
                    theme.text_secondary,
                );
                // 文件名可用宽 = 行宽 − 左内边距 − 分组宽 − 间距 − 右内边距；
                // 太窄（窗口被压）时舍弃分组文字，保证名字至少有整行可用。
                let mut name_avail = rect.width() - 8.0 - 8.0 - 8.0 - group_w;
                if name_avail < 80.0 {
                    name_avail = rect.width() - 16.0;
                }
                let name_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + 8.0, rect.top()),
                    egui::vec2(name_avail.max(0.0), rect.height()),
                );
                ui.put(
                    name_rect,
                    egui::Label::new(RichText::new(&item.name).color(name_color).size(13.0))
                        .truncate(),
                );
                let clicked = response.clicked();
                if response.hovered() {
                    response.on_hover_text(item.path.clone());
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

/// 把选中的候选打开，并关闭面板。
///
/// 抽出来而非在视图里就地调 `open_path`，有两个原因：
///
/// 1. 点击路径与 Enter 路径必须走**同一段**代码——两处各写一遍，
///    改一处忘一处就会出现「鼠标点得开、键盘打不开」。
/// 2. 关闭浮层与打开文件要放在同一处：顺序错了会闪现
///    「新文件已载入而浮层还盖在上面」，用户以为没生效而连点。
fn apply_pick(app: &mut EditorApp, item: &QuickOpenItem) {
    // 格式白名单外：**不关浮层**、不开文档，只说明原因（问题 045）。
    // 关掉浮层会让这次点击看起来「生效了」——用户以为文件开了，
    // 却发现标签没变。留在浮层上，他还能立刻改选另一个文件。
    if !app.can_open_as_text(&item.path) {
        app.report_unsupported_format(&item.path);
        return;
    }
    app.quick_open_ui.open();
    app.dialog.close(Overlay::QuickOpen);
    // 失败由 `open_path` **自己**报到状态栏（见其文档），
    // 这里不重复报——两处都报会出现两条几乎一样的消息。
    let _ = app.open_path(&item.path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_constants_match_the_prototype() {
        // 原型 `.palette { width: 480px }` / `.search { height: 38px }`。
        assert_eq!(WIDTH, 480.0);
        assert_eq!(SEARCH_H, 38.0);
        assert_eq!(ROW_H, 28.0);
    }
}
