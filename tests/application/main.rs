//! 应用层集成测试入口（阶段 04 · S4-11）。
//!
//! 各服务的主路径与错误路径、跨服务事件链路用例统一在此装配。
//! 共享测试替身位于 `tests/common/fakes.rs`，由本入口以 `#[path]` 引入，
//! 不进入 `release` 产物。

#[path = "../common/fakes.rs"]
mod fakes;

mod document_service_test;
mod event_flow_test;
mod export_service_test;
mod format_service_test;
mod markdown_service_test;
mod preferences_service_test;
mod workspace_service_test;
