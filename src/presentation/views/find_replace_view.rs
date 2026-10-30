//! 查找 / 替换面板视图（S6-08）。
//!
//! # 布局逐项对齐原型 `FindReplacePanel.vue`
//!
//! ```text
//!   ┌──────────────────────────────────┐
//!   │ [查找      ] [Aa] [.*] [字]  1/12 │   ← 三个开关 + 计数
//!   │ [替换      ] [替换] [全部]        │   ← 仅 `Ctrl+H` 打开时
//!   │ [↑ 上一个] [↓ 下一个]     [× 关闭]│
//!   ├──────────────────────────────────┤
//!   │ 已启用 2 个匹配选项 / 上次替换 … │
//!   └──────────────────────────────────┘
//! ```
//!
//! 三个开关（`Aa` / `.*` / 全字）**正交**，可任意组合；
//! 匹配规则全在 [`crate::presentation::state::find_replace_ui_state`]，
//! 本文件只负责画与收集点击——**不在视图里做任何文本变换**，
//! 写入一律经 [`EditorApp`] 的 `find_*` 方法落到领域。
//!
//! # 为什么用 `egui::Window` 而不是原型那样 `position: absolute`
//!
//! egui 的 `Window` 自带焦点管理、Esc 关闭、跨帧位置保持；
//! 手写 `absolute` 面板要自己实现这三件事，而焦点一旦丢给编辑区
//! 就再也回不来（原型 `watch(() => ui.findOpen)` + `nextTick(focus)`
//! 就是在补这个）。`collapsible(false)` 让它表现得像面板而非窗口——
//! 那个标题栏复选框正是「像窗口」的最大破绽。
//!
//! # 焦点为什么归查找框而不是编辑区
//!
//! 用户按 `Ctrl+F` 的意图是「马上打字」。焦点若留在编辑区，
//! 得再点一次面板才能输入——而这一下点击很可能落在编辑区里，
//! 顺手就改了文档。
//!
//! 但**不能每帧无条件抢焦点**：面板非模态，用户可能只是看一眼命中列表
//! 然后把焦点移回编辑区继续写，被强行拽回面板会让人无法工作。
//! 故用 [`FindReplaceUiState::find_focused`] 作「用户尚未表态」的一次性标记：
//! 打开时置 `true`，被任何控件真正拿到焦点后置 `false`。

use egui::{Align, Align2, Color32, Context, Layout, RichText, Ui};

use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::state::find_replace_ui_state::{
    FIND_INPUT_ID, OptionKind, REPLACE_INPUT_ID,
};
use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::FindReplaceViewAccess;

/// 面板宽度（原型 `.find-panel { width: 340px }`）。
const PANEL_W: f32 = 340.0;

/// 计数标签的固定宽度（点）。
///
/// 不固定的话「无结果」（3 字）与「12/128」（6 字）宽度不同，
/// 用户每敲一个字符整行就重新排版，看起来在抖动。
const COUNT_W: f32 = 56.0;

/// 绘制查找替换面板（仅在 [`Overlay::Find`] 位于栈顶时由 [`super::overlays`] 调用）。
pub fn show(vctx: &mut dyn FindReplaceViewAccess, ctx: &Context) {
    if !vctx.dialog_state().is_open(Overlay::Find) {
        return;
    }
    let mut open = true;
    egui::Window::new(Overlay::Find.title())
        .id(egui::Id::unique("rustmark.find_panel"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(PANEL_W)
        .anchor(Align2::RIGHT_TOP, egui::vec2(16.0, 8.0))
        .show(ctx, |ui| draw(vctx, ui));
    if !open {
        // 走 `DialogUiState` 而不是就地清状态：否则 `dialog.is_open(Overlay::Find)`
        // 与实际显示会不一致（下一次别的动作触发 `open_overlay` 时
        // 面板会「莫名其妙又出现」）。
        vctx.dialog_state().close(Overlay::Find);
    }
}

/// 面板主体。
fn draw(vctx: &mut dyn FindReplaceViewAccess, ui: &mut Ui) {
    let theme = vctx.theme_state().clone();
    ui.set_width(PANEL_W);

    draw_find_row(vctx, ui, &theme);
    draw_error(vctx, ui, &theme);
    if vctx.find_state().replace_open {
        draw_replace_row(vctx, ui);
    }
    draw_nav_row(vctx, ui);
    ui.separator();
    draw_summary(vctx, ui, &theme);
}

/// 第一行：查找框 + 三个开关 + 计数。
fn draw_find_row(vctx: &mut dyn FindReplaceViewAccess, ui: &mut Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        // D-3 阶段 3：clone → 就地编辑 → 变化才值写回（写回在重扫之前，
        // `sync_find` 读到的必须是新查询）。
        let mut find_text = vctx.find_state().find_text.clone();
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut find_text)
                    .id(egui::Id::unique(FIND_INPUT_ID))
                    .hint_text("查找")
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("输入后自动查找；Enter 跳到下一个");
        // 只在「用户尚未表态」时抢焦点，且**每帧**都请求：
        // egui 的 `request_focus` 只对当帧有效，隔帧调用会失效。
        if vctx.find_state().find_focused && !response.has_focus() {
            response.request_focus();
        }
        if response.has_focus() {
            vctx.set_find_focused(true);
        }
        if response.changed() {
            // D-3 阶段 3：值写回先行（字段的可变引用不出 state）。
            vctx.find_state().set_find_text(find_text);
            // 改了查询必须重扫：面板显示的 `n/m` 与「下一个」跳转目标
            // 都来自 `finder`，不重扫就会「计数说 12 条、点下去跳到旧位置」。
            vctx.sync_find();
        }

        // Enter / Shift+Enter 跳到下一个 / 上一个（对齐原型
        // `@keydown.enter.prevent="onQueryEnter"`）。
        //
        // 放在**分发器之外**是刻意的：`Esc` 走 `UiAction::CloseOverlay`
        // 由全局分发器处理（那才是「改绑 Esc 后仍能关面板」的保证），
        // 而 Enter 在编辑器里是「插入换行」，绝不能被全局劫持——
        // 故只在查找框**确实有焦点**时才消费。
        if response.has_focus() {
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter {
                if ui.input(|i| i.modifiers.shift) {
                    vctx.find_step(false);
                } else if vctx.find_state().replace_open && ui.input(|i| i.modifiers.ctrl) {
                    // `Ctrl+Enter` = 替换当前（主流编辑器的惯例）。
                    vctx.find_replace_current();
                } else {
                    vctx.find_step(true);
                }
            }
        }

        option_button(
            ui,
            vctx,
            OptionKind::CaseSensitive,
            "Aa",
            "区分大小写",
            theme,
        );
        option_button(ui, vctx, OptionKind::Regex, ".*", "正则表达式", theme);
        option_button(ui, vctx, OptionKind::WholeWord, "字", "全字匹配", theme);

        ui.allocate_ui(egui::vec2(COUNT_W, ui.spacing().interact_size.y), |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = vctx.find_state().count_label();
                if !label.is_empty() {
                    ui.label(RichText::new(label).color(theme.text_secondary));
                }
            });
        });
    });
}

/// 一个选项开关按钮（`Aa` / `.*` / 全字）。
fn option_button(
    ui: &mut Ui,
    vctx: &mut dyn FindReplaceViewAccess,
    which: OptionKind,
    text: &str,
    tip: &str,
    theme: &Theme,
) {
    let active = match which {
        OptionKind::CaseSensitive => vctx.find_state().options.case_sensitive,
        OptionKind::Regex => vctx.find_state().options.regex,
        OptionKind::WholeWord => vctx.find_state().options.whole_word,
    };
    let response = ui
        .add(
            egui::Button::new(RichText::new(text).color(if active {
                theme.accent
            } else {
                theme.text_secondary
            }))
            .selected(active),
        )
        .on_hover_text(tip);
    if response.clicked() {
        vctx.find_state().toggle(which);
        // 选项变了必须重扫：同一段文本在 `Aa` 开关下命中数完全不同。
        vctx.sync_find();
    }
}

/// 错误提示（无效正则等）。
///
/// 用 `theme.error` 而不是 egui 的默认红：主题 token 是唯一配色来源，
/// 且浅色主题下 egui 默认红会与背景撞得看不清。
fn draw_error(vctx: &mut dyn FindReplaceViewAccess, ui: &mut Ui, theme: &Theme) {
    let error = vctx.find_state().error.clone();
    if let Some(error) = error {
        ui.label(RichText::new(error).color(theme.error).small());
    }
}

/// 第二行：替换框 + 替换 / 全部替换。
fn draw_replace_row(vctx: &mut dyn FindReplaceViewAccess, ui: &mut Ui) {
    let has_current = vctx.find_state().current_match().is_some();
    let has_any = vctx.find_state().match_count() > 0;
    ui.horizontal(|ui| {
        // D-3 阶段 3：clone → 就地编辑 → 变化才值写回。
        let mut replace_text = vctx.find_state().replace_text.clone();
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut replace_text)
                    .id(egui::Id::unique(REPLACE_INPUT_ID))
                    .hint_text("替换")
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("留空即删除命中内容");
        if response.changed() {
            vctx.find_state().set_replace_text(replace_text);
        }
        if response.has_focus() {
            // 替换框吃掉焦点时必须**放弃**抢焦点，否则用户在替换框里
            // 每敲一个字符都会被拽回查找框，一个字也打不进去。
            vctx.set_find_focused(false);
        }

        if ui
            .add_enabled(has_current, egui::Button::new("替换"))
            .on_hover_text("替换当前命中并跳到下一条")
            .clicked()
        {
            vctx.find_replace_current();
        }
        if ui
            .add_enabled(has_any, egui::Button::new("全部"))
            .on_hover_text("替换全部命中（算作一次撤销）")
            .clicked()
        {
            vctx.find_replace_all();
        }
    });
}

/// 第三行：上一个 / 下一个 / 关闭。
fn draw_nav_row(vctx: &mut dyn FindReplaceViewAccess, ui: &mut Ui) {
    let has_match = vctx.find_state().match_count() > 0;
    let mut close = false;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(has_match, egui::Button::new("↑ 上一个"))
            .on_hover_text("Shift+Enter")
            .clicked()
        {
            vctx.find_step(false);
        }
        if ui
            .add_enabled(has_match, egui::Button::new("↓ 下一个"))
            .on_hover_text("Enter")
            .clicked()
        {
            vctx.find_step(true);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 用 ×(U+00D7) 而非 ✕(U+2715)：后者无字形覆盖，会渲染成方框。
            close = ui.button("× 关闭").clicked();
        });
    });
    if close {
        vctx.dialog_state().close(Overlay::Find);
        // 关闭后焦点归编辑区：用户多半是「找一下换个词继续写」。
        // 走 egui 的 `request_focus` 而非只设 `editor.focused = true`——
        // 后者只是个 UI 标记，egui 内部焦点仍停在面板上，
        // 用户接着按键会打进面板而不是编辑器。
        ui.ctx().memory_mut(|m| {
            m.request_focus(crate::presentation::state::editor_ui_state::editor_id())
        });
    }
}

/// 底部一行：选项状态与上次替换结果。
fn draw_summary(vctx: &mut dyn FindReplaceViewAccess, ui: &mut Ui, theme: &Theme) {
    let active_count = vctx.find_state().options.active_count();
    if active_count > 0 {
        ui.label(
            RichText::new(format!("已启用 {active_count} 个匹配选项"))
                .small()
                .color(theme.text_secondary),
        );
    }
    let last_replace_count = vctx.find_state().last_replace_count;
    let grows = vctx.find_state().last_replace_grows;
    if let Some(count) = last_replace_count {
        let mut text = format!("上次替换 {count} 处");
        if grows {
            text.push_str(" · 替换文本仍会再次命中，再点会越变越长");
        }
        ui.label(
            RichText::new(text)
                .small()
                .color(warning_color(grows, theme)),
        );
    }
}

/// 增生警告用警告色，普通提示用次文本色。
fn warning_color(grows: bool, theme: &Theme) -> Color32 {
    if grows {
        theme.warning
    } else {
        theme.text_secondary
    }
}
