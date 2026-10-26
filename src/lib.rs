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
// V11-65：lint 声明已收敛至 root `Cargo.toml` 的 `[workspace.lints]` 单点真源
// （本 crate 经 `[lints] workspace = true` 继承），不再于此处重复声明。
// 测试代码允许 `unwrap()`（规范 §七-20），故 test 配置下整体豁免——
// 豁免是 cfg 形态，无法进 workspace.lints 表，保留在本行。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
// DOC-03（API 文档）：所有公开项必须有 rustdoc 注释。
//
// 本地 `cargo check` 只告警，CI 的 `-D warnings` 会把它升级为失败——
// 新增公开项漏写文档与漏写 `unwrap` 同样过不了门禁。
// 确属不可文档化的项（生成代码等）用 `#[allow(missing_docs)]` + 理由注释逐处裁决。
//
// V11-65：同 unwrap 系列，声明已收敛至 `[workspace.lints.rust]`。

pub mod app;
pub mod presentation;

// D-3 阶段 4a：四层住在 `rustmark-core`，此处 re-export 拼回原 `crate::<层>` 路径，
// 存量源码与测试的 `crate::domain::…` 等引用零改动（反向依赖由 cargo 锁死：
// core 无法引用 root 的 `crate::app`，根 crate 的私有项也无法渗入 core）。
// TD-04（2026-10-26）：shared 已抽为 rustmark-shared，core 以
// `pub use rustmark_shared::shared;` 原样转发，这里继续经 core 取用。
// V11-64（2026-10-26）：infrastructure 已抽为 rustmark-infra，此处拼回
// `crate::infrastructure` —— 组合根 `src/app/**` 的装配代码与
// `tests/infrastructure/` 的 `rustmark::infrastructure::…` 引用零改动。
pub use rustmark_core::{application, domain, shared};
pub use rustmark_infra::infrastructure;
