//!
//! 核心域（需求文档 §6.1）：文本编辑、光标、撤销、查找。
//! 聚合根为 [`aggregate::Document`]。

pub mod aggregate;
pub mod errors;
pub mod events;
pub mod repository;
pub mod services;
pub mod undo;
pub mod value_objects;
