//! # rustmark-actions —— UI 动作词汇表（V11-57 抽出）
//!
//! 菜单 / 工具栏 / 命令面板 / 快捷键**四处共用**的唯一动作表：有哪些动作、
//! 各自的分组 / 中文显示名 / 稳定持久化键 / 默认键位 / 检索关键词。
//!
//! **零依赖契约 crate**：不依赖 egui、不依赖 core、不依赖 root——只描述
//! 「有哪些动作」，不描述「动作怎么画、怎么派发」。
//!
//! # 依赖方向（V11-57 后）
//!
//! ```text
//! root ──► rustmark-core ──► rustmark-actions ◄── rustmark-icons（V11-58）
//! ```
//!
//! core 的 `presentation::shortcuts::registry` 经 `pub use` 把本 crate 的
//! [`action`] 模块拼回 `registry::action` 原路径，存量 `registry::UiAction`
//! 引用零改动。
//!
//! # 与领域层的对齐契约
//!
//! [`UiAction::persisted_key`] 与领域 `ActionId::key()` **逐字一致**——
//! 表现层不得依赖领域（分层禁令），两者只能靠这份字符串契约对齐；
//! 「表里的键名写错 / 少写一个」编译期查不出，由 `tests/app/` 与
//! `tests/presentation/` 的两条差分用例守住。

pub mod action;

// 便捷再导出：词汇表的主入口在 [`action`] 模块里，crate 根再导出一层，
// 消费者（core 的 registry re-export、V11-58 的 icons）少写一层路径。
pub use action::{MenuGroup, UiAction};
