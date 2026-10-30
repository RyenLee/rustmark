//! 组合根装配（M2：从 root 下沉，2026-10-29）。
//!
//! 本 crate 是「唯一允许同时看见四个层」的装配面的归宿：点名具体 impl
//! （[`bootstrap::production_bundle`]）并把它们注入 core 的 [`compose`]
//! （[`bootstrap::Services::assemble_with_bundle`]）。root 经
//! `pub use rustmark_assembly::bootstrap::*;` 拼回 `rustmark::app::bootstrap::*`
//! 原路径，消费方（`tests/`、`frame/`）零改动。

pub mod bootstrap;
