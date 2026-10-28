//! # rustmark-ports —— 架构端口（契约）crate（R-10 项 A，2026-10-28）
//!
//! 跨层契约（端口）的**真源**：原生对话框端口、事件总线、外部打开器端口、
//! 图片导入端口、可注入时钟。它们被 domain / application / infrastructure /
//! presentation 多层共用，自身零 IO、零业务归属层。
//!
//! ## 为什么从 `rustmark-shared` 抽出（R-10 问题 D）
//!
//! 端口（架构契约）原本与值对象（`theme` / `fonts` / `export_format`）混在
//! `rustmark-shared` 工具箱里，违背「契约归领域、实现归 infra」。项 A 把端口
//! 抽成独立 crate：值对象留在 `shared/kernel/`，端口迁至本 crate。
//!
//! ## 依赖方向（结构化防环）
//!
//! 本 crate **单向**依赖 `rustmark-shared`（端口内部复用其 `error` / `path` /
//! `text_format` / `geometry` / `ids` / `result` 等纯值类型）；
//! `rustmark-shared` **不**反向依赖本 crate——否则 Cargo 解析期即成环
//! 报错。调用点改走 `rustmark_core::ports::…`（core 以
//! `pub use rustmark_ports;` 拼回），`shared` 不写 `pub use rustmark_ports::*`
//! 桥接（那会制造循环依赖）。
//!
//! ## 接入
//!
//! `rustmark-core` 以 `pub use rustmark_ports;` 把模块拼回原位：
//! `rustmark_core::ports::…`（root / ui / tests 的公开路径）逐字可用；core 内
//! `crate::ports::…` 亦可用。端口的实现仍在 `rustmark-infra`，经组合根装配。

// 测试代码允许 unwrap / expect（规范 §七-20）；生产代码保持 deny
// （与 shared 的 crate 级 lint 等价）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod dialog;
pub mod event;
pub mod image_asset;
pub mod system;
pub mod time;
