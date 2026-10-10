//! 跨测试目标共享的测试替身与工具。
//!
//! 由各测试入口通过 `#[path = "../support/mod.rs"] mod support;` 引入
//! （`tests/support/` 下没有 `main.rs`，故不会被 Cargo 当成独立测试目标）。

pub mod fake_dialog;
pub mod keymap;
pub mod text_buffer;
