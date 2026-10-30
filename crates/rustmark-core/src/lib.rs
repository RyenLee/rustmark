//! # rustmark-core —— rustmark 的核心库（D-3 阶段 4a 拆分产物）
//!
//! 装载领域层与应用层，并以子模块转发 `shared` / `ports` / `actions`；
//! **不含**表现层（V11-62 起非视图子层已全部迁入 `rustmark-ui` 与
//! `rustmark-render`），也不含组合根 `app` 与视图 `presentation::views`。
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
//!
//! 表现层不属于本 crate：非视图子模块（`widgets` / `state` / `shortcuts` /
//! `theme`）在 `rustmark-ui`，`rich` 在 `rustmark-render`，视图与组合根在
//! root crate。core 侧历史保留的 `presentation` doc-only 空壳已于 2026-10-30
//! 删除——它零消费者，只会让「presentation 归哪一层」多出一个错误落点。

// V11-65：unwrap 系列与 missing_docs 的 lint 声明已收敛至 root `Cargo.toml`
// 的 `[workspace.lints]` 单点真源（本 crate 经 `[lints] workspace = true` 继承）。
// 规范 §四-15 / DOC-03 的规则内容不变（详见 root `Cargo.toml` 该段注释）；
// 测试代码允许 `unwrap()` 的豁免是 cfg 形态，无法进 workspace.lints 表，
// 保留在本行，与 root lib.rs 同款。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod application;
pub mod domain;

// TD-04（2026-10-26）：共享内核抽为独立 crate `rustmark-shared`，此处把它的
// `shared` 模块拼回原位——`crate::shared::…`（core 内 100+ 处）与
// `rustmark_core::shared::…`（root / ui / tests）公开路径逐字不变。
// 反向依赖由 Cargo 结构性挡死：shared 不依赖 core，core 依赖 shared。
pub use rustmark_shared::shared;

// R-10 项 A（2026-10-28）：架构端口抽为独立 crate `rustmark-ports`（原住
// `rustmark-shared`）。此处把整个 crate 以 `ports` 之名拼回——`crate::ports::…`
// （core 内）与 `rustmark_core::ports::…`（root / ui / tests）公开路径可用。
// 端口的实现仍在 `rustmark-infra`，经组合根装配；shared 不反向依赖 ports。
pub use rustmark_ports as ports;

// V11-62 批次 3：`shortcuts` 迁入 `rustmark-ui` 后，其 `registry` 仍需 actions crate 的
// 类型（MenuGroup / UiAction / action）。ui crate 不直接依赖 `rustmark-actions`（保持
// ui 依赖面 = egui + core，拓扑断言 `ui_crate_dependency_face_is_egui_plus_core` 不变），
// 故由 core 在此以 `actions` 子模块转发，供 ui 经 `rustmark_core::actions` 间接访问。
pub mod actions {
    //! actions crate 的符号转发层：把 `rustmark_actions` 的 `MenuGroup` / `UiAction` /
    //! `action` 经 core 暴露给下游（ui crate 不直接依赖 `rustmark-actions`）。
    pub use rustmark_actions::{MenuGroup, UiAction, action};
}
