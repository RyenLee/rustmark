//! # rustmark-infra —— 基础设施层 crate（V11-64 抽出，2026-10-26）
//!
//! 承载外部库适配与 IO 实现：解析（pulldown-cmark）/ 高亮（syntect）/
//! 文本缓冲（ropey）/ 原生对话框（rfd）/ 远程图片（ureq + rustls）/
//! 图片解码（image）/ 导出（无头浏览器 + PNG 编码）/ 字体发现与装配。
//!
//! ## 依赖方向
//!
//! `rustmark-infra → rustmark-core`：端口与领域类型在 core，实现在本 crate
//! （依赖倒置的实现侧）。反向依赖由 Cargo 锁死。core 的 `infrastructure`
//! 模块已不存在——依赖 core 的消费者不必再被 syntect / ureq / rfd / image
//! 拖进编译图，这是 V11-64 的收益本身。
//!
//! ## 为什么保留 `infrastructure/` 路径前缀
//!
//! 与 `rustmark-shared` 保留 `shared/`、`rustmark-ui` 保留 `presentation/`
//! 同理：`layer_of` 按根内相对路径判层，`infrastructure/x.rs` 自然判
//! `Layer::Infrastructure`（`tools/arch_check.rs`）——层禁令、可见性白名单键
//! （`infrastructure/…`）与各专项守卫随搬迁零改动。
//!
//! root 以 `pub use rustmark_infra::infrastructure;` 拼回 `crate::infrastructure`
//! 原路径，组合根 `src/app/**` 的装配代码与 `tests/infrastructure/` 的
//! `rustmark::infrastructure::…` 引用逐字不变。

// 测试代码允许 unwrap / expect / panic（规范 §七-20）；生产代码沿用
// workspace lints 的 warn + CI `-D warnings` 门禁（V11-12 设计不变）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod infrastructure;
