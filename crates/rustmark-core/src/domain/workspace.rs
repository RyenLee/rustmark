//! domain::workspace 上下文模块声明。
//!
//! 支撑域（需求文档 §6.1）：文件树、大纲、最近文件、标签页。
//! 聚合根为 [`aggregate::Workspace`]。
//!
//! **目录遍历与磁盘 IO 由基础设施层完成**（`infrastructure/persistence`），
//! 领域层只维护结构与规则（重名、移动合法性、最近文件上限）。

pub mod aggregate;
pub mod errors;
pub mod events;
pub mod repository;
pub mod value_objects;
