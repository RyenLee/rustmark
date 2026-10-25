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
//! - [`presentation`]：表现层的 state / theme / rich / shortcuts
//!   （egui 视图与组合根在 root crate 的 `presentation::views` / `app`；
//!   widgets 布局封装在 V11-62 批次 1 抽去的 `rustmark-ui` crate；
//!   icons 向量图标资源在 V11-58 抽去的 `rustmark-icons` crate）

// V11-65：unwrap 系列与 missing_docs 的 lint 声明已收敛至 root `Cargo.toml`
// 的 `[workspace.lints]` 单点真源（本 crate 经 `[lints] workspace = true` 继承）。
// 规范 §四-15 / DOC-03 的规则内容不变（详见 root `Cargo.toml` 该段注释）；
// 测试代码允许 `unwrap()` 的豁免是 cfg 形态，无法进 workspace.lints 表，
// 保留在本行，与 root lib.rs 同款。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;
pub mod shared;
