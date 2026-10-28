//! 共享内核（Shared Kernel）的模块根——`rustmark_shared::shared`（TD-04 抽出）。
//!
//! 可被任意层依赖，但自身**不得依赖** `domain` / `application` /
//! `infrastructure` / `presentation`（见 `docs/architecture-constraints.md`）。
//! 这一条不再只靠词法守卫：crate 的 `Cargo.toml` 只有 serde / thiserror /
//! ahash 三个纯数据依赖，反向依赖任何层都会在 Cargo 解析期成环报错。
//!
//! ## 内部二分（R-02，2026-10-27）
//!
//! `shared/` 内的模块按职责切为两组子目录，落点即语义：
//!
//! - **[`kernel`]** —— 仅余**值对象**（跨层共用纯值）：`theme` `fonts`
//!   `export_format`。端口（`dialog` `event` `system` `image_asset` `time`）
//!   已迁至独立的 `rustmark-ports` crate（R-10 项 A，契约归领域、实现归
//!   infra），由 `rustmark-core` 经 `pub use rustmark_ports;` 拼回
//!   `rustmark_core::ports::…`；
//! - **[`support`]** —— 与层无关的工具：常量 / 编码 / 错误 / 几何 / ID /
//!   LRU / 路径 / 正则 / 结果 / 分词 / 文本格式。
//!
//! 两组均以 `pub use kernel::*;` / `pub use support::*;` 在伞形模块上拼回原
//! `shared::<名>` 路径，故 `rustmark_shared::shared::<名>`、
//! `rustmark_core::shared::<名>` 与拆分前逐字一致，调用点零改动。`shared/`
//! 路径前缀继续保留，故 `layer_of`、§4.5 登记表、可见性白名单与各专项守卫
//! 的判定键（`shared/...`）随搬迁零改动。
//!
//! `rustmark-core` 以 `pub use rustmark_shared::shared;` 桥接，公开路径
//! `rustmark_core::shared::…` 与拆分前逐字一致（V11-58 / V11-62 同款桥接）。
//! 共享内核的准入门槛仍是 §4.5 三问 + 登记表，守卫在 `check-arch` 的
//! `scan_project`——登记键沿用 `shared/<子目录>/<文件名>`，搬迁时手改登记表路径。

// 测试代码允许 unwrap / expect（规范 §七-20）；生产代码保持 deny。
// lint 级别声明在 crate 根 `lib.rs`（模块级重复声明会与 crate 级 cfg_attr
// 叠加出歧义，且 clippy 只认最外层配置）。

pub mod kernel;
pub mod support;

pub use kernel::*;
pub use support::*;
