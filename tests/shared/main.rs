//! `shared` 共享内核单元测试入口（S2-03 ~ S2-06、S3-A）。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate（原先仅 lib 目标
// 受 lib.rs attribute 管辖）。规范 §七-20 允许测试代码 unwrap/expect/
// panic，同 lib.rs 的 `cfg_attr(test, allow)` 惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod error_test;
mod export_format_test;
mod geometry_test;
mod ids_test;
mod internal_cycle_guard_test;
mod path_test;
mod theme_guard_test;
mod theme_test;
mod time_test;
