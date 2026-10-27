//! 共享内核的**工具**分组（R-02 二分，2026-10-27）。
//!
//! 这一组是与层无关的可复用纯逻辑：阈值常量、编码标签、统一错误与结果、
//! 文本坐标值对象、强类型 ID、有界 LRU、路径值对象、迷你正则引擎、分词与
//! 文本格式白名单。它们支撑 kernel 与上层，自身零 IO。
//!
//! 落点保留在 `shared/support/`，由 `shared.rs` 以 `pub use support::*` 拼回
//! `shared::<名>`，调用点零改动（与 V11-57 / V11-58 / V11-62 同款桥接）。

pub mod constants;
pub mod encoding;
pub mod error;
pub mod geometry;
pub mod ids;
pub mod lru;
pub mod path;
pub mod pattern;
pub mod result;
pub mod text;
pub mod text_format;
