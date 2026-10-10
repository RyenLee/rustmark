//! Markdown 用例命令（阶段 04 · S4-05）。
//!
//! 命令是**纯数据**结构。除渲染/高亮/滚动/大纲四类查询外，本模块仅含一条
//! [`ClearCacheCommand`]（供 [`crate::application::event_subscriptions`] 在
//! `PreferencesChanged` 时主动失效渲染缓存）。

use crate::shared::ids::DocumentId;

/// 失效渲染缓存命令。
///
/// `doc_id = None` 表示清空全部缓存（主题切换后所有文档的预览样式都失效）。
#[derive(Clone, Debug)]
pub struct ClearCacheCommand {
    /// 目标文档；`None` 表示全部。
    pub doc_id: Option<DocumentId>,
}
