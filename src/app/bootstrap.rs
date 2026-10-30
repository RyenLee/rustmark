//! 组合根装配公开门面（M2，2026-10-29：实现已下沉 `rustmark-assembly`）。
//!
//! 经 `pub use rustmark_assembly::bootstrap::*` 拼回
//! `rustmark::app::bootstrap::*` 原路径，消费方（`tests/`、`frame/`）零改动。
//! 装配的真正实现住在 `crates/rustmark-assembly/src/bootstrap.rs`。

pub use rustmark_assembly::bootstrap::*;

/// 预览防抖间隔（毫秒，FR-01）。
///
/// 与统计防抖、高亮静止窗口共用 core 的单一真源。此常量与装配逻辑无关，
/// 留 root 以保持 `thresholds.baseline.yaml` 的 `source` 路径基本不变
/// （仅行号漂移）。
pub const PREVIEW_DEBOUNCE_MS: u64 = rustmark_core::shared::constants::DEBOUNCE_MS;
