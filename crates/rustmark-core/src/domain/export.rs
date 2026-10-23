//! domain::export 上下文模块声明。
//!
//! 支撑域（需求文档 §6.1）：HTML / PDF / 图片导出。
//! 聚合根为 [`aggregate::ExportJob`]。
//!
//! 真正的写入由基础设施层完成（`infrastructure/export`）；领域层负责
//! **选项合法性校验**与 TOC 转义标记（原型缺陷 2.4 的根治点）。

pub mod aggregate;
pub mod errors;
pub mod services;
pub mod value_objects;
