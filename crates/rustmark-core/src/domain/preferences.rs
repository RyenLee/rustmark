//!
//! 通用域（需求文档 §6.1）：主题、编辑器设置、快捷键。
//!
//! 本上下文的关键规则是 [`value_objects::ShortcutTable::conflicts`]——
//! 把「哪些组合允许、哪些冲突」做成可单测的纯规则，根治原型「四处真相」缺陷。

pub mod aggregate;
pub mod errors;
pub mod repository;
pub mod value_objects;
