//! 组合根与调度层测试（阶段 06 · S6-01）。
//!
//! 组合根是**唯一**能同时看见四个层的地方，也就是分层约束失效时
//! **第一个能察觉**的地方。这类代码天然难测（依赖具体实现），
//! 但恰恰因此更必须测：装配顺序错、provider 拿错、
//! 配置路径写错这类问题，只有在这里才看得见。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate（原先仅 lib 目标
// 受 lib.rs attribute 管辖）。规范 §七-20 允许测试代码 unwrap/expect/
// panic，同 lib.rs 的 `cfg_attr(test, allow)` 惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[path = "../support.rs"]
mod support;

mod bootstrap_test;
mod close_all_confirm_test;
mod crash_log_test;
mod default_extension_guard_test;
mod delete_confirm_test;
mod dialog_runner_test;
mod editor_state_writes_test;
mod export_format_guard_test;
mod frame_test;
mod insert_image_test;
mod open_limit_test;
mod path_file_name_test;
mod preview_image_fetcher_test;
mod remaining_state_writes_test;
mod revert_test;
mod routing_exporter_test;
mod save_all_test;
mod scheduler_test;
mod shortcut_action_guard_test;
mod shortcut_persisted_key_test;
mod tab_cursor_test;
mod text_format_guard_test;
mod ui_state_writes_test;
mod workspace_reveal_test;
mod writing_goal_test;
