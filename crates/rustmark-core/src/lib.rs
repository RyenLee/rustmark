//! # rustmark-core —— rustmark 的核心库（D-3 阶段 4a 拆分产物）
//!
//! 装载四层与表现层的非视图子模块，**不含**组合根 `app` 与 `presentation::views`。
//! 依赖方向由 cargo 锁死：core 不允许依赖 root（root 通过 re-export 拼接全部
//! `crate::` 路径，见 `src/lib.rs` 与 `src/presentation.rs`）。
//!
//! ```text
//! presentation ──► application ──► domain ◄── infrastructure
//!                                     ▲
//!                                     └── trait 实现反向注入
//! ```
//!
//! - [`shared`]：共享内核（错误、结果、事件总线抽象、ID、文本坐标）
//! - [`domain`]：领域层，零 IO、零 UI 依赖（阶段 03）
//! - [`application`]：应用层，用例编排与 DTO（阶段 04）
//! - [`infrastructure`]：基础设施层，外部库适配（阶段 05）
//! - [`presentation`]：表现层的 state / theme / rich / widgets / icons / shortcuts
//!   （egui 视图与组合根在 root crate 的 `presentation::views` / `app`）

// 与 root lib.rs 同款门禁（规范 §四-15 / DOC-03），两处必须保持一致。
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
#![warn(missing_docs)]

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;
pub mod shared;
