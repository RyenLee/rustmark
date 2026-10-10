//! infrastructure::spell 模块声明（拼写检查，路线①零依赖）。

pub mod dictionary;
pub mod simple_checker;

pub use simple_checker::VendoredSpellChecker;
