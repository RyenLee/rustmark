//! # rustmark —— 原生轻量级 Markdown 编辑器
//!
//! 分层结构（依赖方向严格单向，见 `docs/architecture-constraints.md`）：
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
//! - [`presentation`]：表现层，egui 视图与 UI State（阶段 06）
//! - [`app`]：组合根，唯一允许依赖全部层的位置

// V11-12（规范 §四-15）：生产代码禁用 `unwrap()` / `expect()` / `panic!`。
//
// 此前该规则**只靠人工 review**，没有任何机械检查（`check-arch` 只管分层依赖）。
// 这里用 `warn` 级 lint + CI/验证链的 `-D warnings` 组成门禁：
// 本地 `cargo clippy` 只告警，`cargo clippy --all-targets -- -D warnings`（与 CI 同款）
// 会直接红。确有不变式保障的位置用 `#[allow]` + 注释说明理由，逐处裁决。
//
// 测试代码允许 `unwrap()`（规范 §七-20），故 test 配置下整体豁免。
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod app;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;
pub mod shared;
