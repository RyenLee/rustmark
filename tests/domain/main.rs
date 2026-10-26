//! 领域层单元测试入口（阶段 03，S3-01 ~ S3-13）。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate（原先仅 lib 目标
// 受 lib.rs attribute 管辖）。规范 §七-20 允许测试代码 unwrap/expect/
// panic，同 lib.rs 的 `cfg_attr(test, allow)` 惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[path = "../support.rs"]
mod support;

mod document_test;
mod editing_test;
mod error_catalog_test;
mod export_test;
mod markdown_test;
mod preferences_test;
mod text_buffer_test;
mod undo_policy_test;
mod undo_test;
mod workspace_test;
