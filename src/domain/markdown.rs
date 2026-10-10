//! domain::markdown 上下文模块声明。
//!
//! 核心域（需求文档 §6.1）：解析、高亮、预览。聚合根为 [`aggregate::RenderedDocument`]。
//!
//! 本上下文**没有** `events` 与 `repository` 子模块：解析结果是从文档派生的
//! 缓存，不独立产生领域事件，也不落盘（由应用层的预览缓存持有）。

pub mod aggregate;
pub mod errors;
pub mod services;
pub mod value_objects;
