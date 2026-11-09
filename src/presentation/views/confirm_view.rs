//! 应用内「确认 / 取消」浮层（全部关闭前的未保存提示）。
//!
//! 与系统原生消息框（rfd `MessageDialog`）相对：窗口、配色、按钮、圆角
//! 全部走本项目主题与 egui 控件，交互与设置 / 导出等浮层一致，不再出现
//! 白底灰按钮 + 黄色警告图标那种与深色界面割裂的系统窗口。
//!
//! 范围刻意收窄：目前只服务「全部关闭」这一条确认路径；删除 / 还原确认
//! 仍走原生对话框端口，不在本文件内改动。

use egui::{Align, Context, Layout, RichText};

use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::ConfirmViewAccess;

/// 对话框宽度（比设置浮窗窄：确认框只有一句话 + 两个按钮）。
const WIDTH: f32 = 400.0;

/// 绘制确认浮层。
///
/// 三条退出路径——「确定」按钮、 「取消」按钮、标题栏 ×（`open` 被 egui
/// 置为 false）——都收敛到 [`ConfirmViewAccess::resolve_close_all_confirm`]：
/// 关闭动作与业务兑现不能分开，否则 `Esc` 那条路径会只关浮层、不清
/// 「已有一个确认在飞」的闩锁（与设置浮层 `cancel_settings` 的教训同源）。
pub fn show(vctx: &mut dyn ConfirmViewAccess, ctx: &Context) {
    let theme: Theme = vctx.theme_state().clone();
    // 先取成自有 String：窗口闭包会把它移进去，不能留着对 vctx 的借用。
    let Some(message) = vctx.dialog_state().confirm_message().map(str::to_owned) else {
        return;
    };

    let mut open = true;
    let mut confirm = false;
    let mut cancel = false;
    egui::Window::new(Overlay::Confirm.title())
        .id(egui::Id::unique("rustmark.overlay.confirm"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .default_width(WIDTH)
        .show(ctx, |ui| {
            ui.set_width(WIDTH - 24.0);
            ui.add_space(theme.metrics.space_sm);
            ui.label(RichText::new(message).color(theme.text_primary));
            ui.add_space(theme.metrics.space_lg);
            ui.separator();
            ui.add_space(theme.metrics.space_sm);
            // 与设置浮窗 footer 同一写法：右对齐、先「取消」后主按钮，
            // 主按钮文字用强调色（export_view 的按钮配色约定）。
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("取消").clicked() {
                    cancel = true;
                }
                let ok = egui::Button::new(RichText::new("确定").color(theme.accent));
                if ui.add(ok).clicked() {
                    confirm = true;
                }
            });
        });

    if confirm {
        vctx.resolve_close_all_confirm(true);
    } else if cancel || !open {
        vctx.resolve_close_all_confirm(false);
    }
}
