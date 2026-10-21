//! 「快捷键」分区：改键与冲突提示。
//!
//! 「恢复默认快捷键」按钮**不在本分区**：它在对话框页脚（父级页面，
//! 问题 038）。它是分区级动作，而本分区的主体是一整屏可滚动的键位列表——
//! 放在列表底部要滑很久才能看到，放在页脚则与「恢复本组默认」同级、
//! 不随内容滚动。

use egui::{Align, Layout, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::theme::builtin::Theme;

use super::*;

/// 「快捷键」分区：改键 + 冲突提示。
pub(super) fn shortcuts(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    // 先取快照再进闭包：闭包里要 `&mut app`，而 `entries` 借用了 `app` 的返回值。
    let entries: Vec<(String, String, String)> = app
        .settings()
        .shortcuts
        .iter()
        .map(|s| (s.key.clone(), s.action.clone(), s.combo.clone()))
        .collect();

    ui.horizontal(|ui| {
        ui.weak("操作");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.weak("快捷键 · 点击后按下新组合键");
        });
    });
    ui.separator();

    let mut clear_conflict = false;
    egui::ScrollArea::vertical()
        .id_salt("rustmark.settings.shortcuts")
        .max_height(HEIGHT - 190.0)
        .show(ui, |ui| {
            for (key, label, combo) in &entries {
                let Some(action) = action_of(key) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let recording = app.settings_ui.recording == Some(action);
                        let text = if recording {
                            "录制中…".to_string()
                        } else if combo.is_empty() {
                            "未绑定".to_string()
                        } else {
                            combo.clone()
                        };
                        if ui.button(text).clicked() {
                            if recording {
                                app.settings_ui.stop_recording();
                            } else {
                                // 原键位的登记在 `EditorApp::rebind_shortcut` 里做，
                                // 视图不重复登记（同一处登记两次会让「取消」把
                                // 中间值也当成原值写回）。
                                app.settings_ui.start_recording(action);
                            }
                        }
                    });
                });
            }
        });

    // 冲突提示条独占一行（对齐原型 `.conflict { width: 100% }`）。
    //
    // 用「先收集意图、出闭包后再执行」的写法而不是在闭包里直接改状态：
    // 闭包持有 `&mut ui`，而解除绑定需要 `&mut app`，两者同时可变借用会编译不过；
    // 绕开的代价是「闭包里改 app」这种写法一旦出现就极难读。
    if let Some(conflict) = app.settings_ui.pending_conflict.clone() {
        ui.horizontal(|ui| {
            ui.colored_label(
                theme.error,
                format!(
                    "{} 与「{}」冲突：同一组合键不能绑定两个动作",
                    conflict.combo, conflict.with_label
                ),
            );
            if ui.button("解除原绑定").clicked() {
                clear_conflict = true;
            }
            if ui.button("放弃本次修改").clicked() {
                app.clear_settings_pending_conflict();
            }
        });
    }

    if clear_conflict
        && let Some(conflict) = app.settings_ui.pending_conflict.take()
        && let Some(action) = action_of(&conflict.with_key)
    {
        // 先记住占用者原来的键位，再把它清空——否则「解除原绑定」
        // 就变成「删掉用户的键位」，按「取消」也回不来了。
        app.settings_ui
            .remember_shortcut(action, conflict.with_combo.clone());
        app.rebind_shortcut(action, "");
    }
}
