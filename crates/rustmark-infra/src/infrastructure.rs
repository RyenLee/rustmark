//! 基础设施层：仓储实现与外部库适配（阶段 05）。

pub mod dialogs;
/// 事件基础设施：进程内事件总线的具体实现。
pub mod events;
pub mod export;
pub mod fonts;
pub mod highlighting;
pub(crate) mod image_policy;
pub mod parsing;
pub mod persistence;
pub mod spell;
pub mod system;
/// 文本存储适配：基于 `ropey` 的文本缓冲区实现。
pub mod text;
