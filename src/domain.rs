//! 领域层：聚合、值对象、领域事件、领域服务与仓储 trait（阶段 03）。
//!
//! 边界（`.agent/architecture.md`）：可依赖 `shared` / `ropey` / `chrono` /
//! `serde` / `thiserror`；**禁止** `egui` / `eframe` / `rfd` / `syntect` /
//! `pulldown_cmark` / `std::fs` / `std::io` / `anyhow`（由 `check-arch` 机械拦截）。
//!
//! 领域层**零 IO**：文件读写、目录遍历、原生对话框一律由基础设施层实现此处定义的 trait。
//!
//! 以下 lint 在本层（含全部子模块）升级为 `deny`：
//! - `panic = "abort"`（需求 §8.1）下 `unwrap` / `expect` / `panic!` 失败即进程终止，
//!   故领域层一律返回 `Result` 或饱和值；
//! - `println!` / `eprintln!` 会污染 stdout，诊断信息应走错误或事件。

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    clippy::print_stdout,
    clippy::print_stderr
)]

pub mod document;
pub mod export;
pub mod markdown;
pub mod preferences;
pub mod workspace;
