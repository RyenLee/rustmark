//! 基础设施层集成测试入口（阶段 05）。
//!
//! 各适配器的用例统一在此装配。共享测试替身位于 `tests/support/`，
//! 由本入口以 `#[path]` 引入，不进入 `release` 产物。

#[path = "../support/mod.rs"]
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
