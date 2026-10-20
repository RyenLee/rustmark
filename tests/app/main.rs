//! 组合根与调度层测试（阶段 06 · S6-01）。
//!
//! 组合根是**唯一**能同时看见四个层的地方，也就是分层约束失效时
//! **第一个能察觉**的地方。这类代码天然难测（依赖具体实现），
//! 但恰恰因此更必须测：装配顺序错、provider 拿错、
//! 配置路径写错这类问题，只有在这里才看得见。

#[path = "../support/mod.rs"]
mod support;

mod bootstrap_test;
mod close_all_confirm_test;
mod crash_log_test;
mod default_extension_guard_test;
mod delete_confirm_test;
mod dialog_runner_test;
mod export_format_guard_test;
mod frame_test;
mod insert_image_test;
mod open_limit_test;
mod path_file_name_test;
mod revert_test;
mod routing_exporter_test;
mod save_all_test;
mod scheduler_test;
mod shortcut_action_guard_test;
mod shortcut_persisted_key_test;
mod text_format_guard_test;
mod workspace_reveal_test;
