//! 基础设施层集成测试入口（阶段 05）。
//!
//! 各适配器的用例统一在此装配。共享测试替身位于 `tests/support/`，
//! 由本入口以 `#[path]` 引入，不进入 `release` 产物。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate（原先仅 lib 目标
// 受 lib.rs attribute 管辖）。规范 §七-20 允许测试代码 unwrap/expect/
// panic，同 lib.rs 的 `cfg_attr(test, allow)` 惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[path = "../support.rs"]
mod support;

mod asset_loader_test;
mod event_bus_test;
mod exporter_test;
mod fs_document_repository_test;
mod highlighter_test;
mod html_sanitize_test;
mod pdf_exporter_test;
mod preferences_repository_test;
mod pulldown_parser_test;
mod rope_buffer_test;
mod rope_prop_test;
mod spell_checker_test;
mod spell_suggest_test;
mod viewport_capture_test;
mod workspace_repository_test;
