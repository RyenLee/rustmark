//! 快捷键总览（`ShortcutOverviewUiState`）的写入口。
//!
//! # 为什么单独成文件
//!
//! R-11 子批 3 把 `shortcut_overview_view` 对 `app.shortcut_overview_ui.*` 的直写
//! 收敛为方法调用，与 [`super::editor`] / [`super::sidebar`] / [`super::ui`] 同构：
//! 表现层只读状态、只提意图，写操作一律走这里。

use super::*;

impl EditorApp {
    /// 快捷键总览的筛选框文本（响应 `TextEdit::changed` 的回写）。
    pub fn set_shortcut_overview_query(&mut self, query: String) {
        self.shortcut_overview_ui.query = query;
    }
}
