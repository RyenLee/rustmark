//! [`EditorViewContext`] 的组合根实现（R-03 B1）。
//!
//! # 这个文件是「视图 ↔ 应用」的唯一缝
//!
//! trait 定义在 `rustmark-ui`（视图最终要住进去），实现在这里（只有组合根
//! 同时看得见 UI 状态与用例服务）。视图因此**只认 trait、不认 `EditorApp`**，
//! 而 B1 的迁移可以按视图逐个推进：已迁移的收 `&mut dyn EditorViewContext`，
//! 未迁移的仍收 `&mut EditorApp`——因为 `EditorApp` 实现了该 trait，
//! `&mut EditorApp` 会在调用点自动强制转换为 `&mut dyn EditorViewContext`，
//! 两者能在同一次渲染里共存（`overlays::show` 正是这种混合态）。
//!
//! # 为什么每个方法都写 `EditorApp::x(self, …)` 而不是 `self.x(…)`
//!
//! 本实现里有 6 个方法与 `EditorApp` 的 inherent 方法**同名**
//! （`theme` / `dispatcher` / `theme_kind_label` / `open_document_list` /
//! `workspace_root` / `current_stats` / `insert_at_cursor` /
//! `set_shortcut_overview_query`）。`self.x(…)` 会按「inherent 优先」解析到
//! 那个同名方法，今天恰好是我们要的；但一旦将来 inherent 被删或改名，
//! 同一条语句就会静默变成**递归调用自己**——栈溢出，且编译器不会提示。
//! 写成全路径是把这个隐患在结构上掐掉：`EditorApp::x` 只可能指向 inherent
//! （trait 方法必须走 `<EditorApp as EditorViewContext>::x`），指向不明会直接编译失败。
//!
//! # 状态访问器为什么不需要这层保护
//!
//! `dialog_state` / `symbol_ui_state` / `shortcut_overview_ui_state` 这些名字
//! 在 `EditorApp` 上不存在同名的 inherent 方法或字段，故直接取字段即可。
//! 这正是 trait 侧统一加 `_state` 后缀的原因——见
//! `rustmark_ui::presentation::view_context` 的模块文档。

use super::EditorApp;
use crate::presentation::shortcuts::dispatcher::ShortcutDispatcher;
use crate::presentation::state::dialog_ui_state::DialogUiState;
use crate::presentation::state::shortcut_overview_ui_state::ShortcutOverviewUiState;
use crate::presentation::state::symbol_ui_state::SymbolUiState;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::EditorViewContext;
use rustmark_core::application::dto::{StatsView, TabSummary};
use rustmark_core::shared::result::AppResult;

impl EditorViewContext for EditorApp {
    // ------------------------------------------------------------------
    // 状态访问（直接取字段，无遮蔽风险）
    // ------------------------------------------------------------------

    fn dialog_state(&mut self) -> &mut DialogUiState {
        &mut self.dialog
    }

    fn symbol_ui_state(&mut self) -> &mut SymbolUiState {
        &mut self.symbol_ui
    }

    fn shortcut_overview_ui_state(&self) -> &ShortcutOverviewUiState {
        &self.shortcut_overview_ui
    }

    fn theme_state(&self) -> &Theme {
        EditorApp::theme(self)
    }

    fn dispatcher_state(&self) -> &ShortcutDispatcher {
        EditorApp::dispatcher(self)
    }

    // ------------------------------------------------------------------
    // 只读查询
    // ------------------------------------------------------------------

    fn theme_kind_label(&self) -> &'static str {
        EditorApp::theme_kind_label(self)
    }

    fn open_document_list(&self) -> Vec<TabSummary> {
        EditorApp::open_document_list(self)
    }

    fn workspace_root(&self) -> String {
        EditorApp::workspace_root(self)
    }

    fn current_stats(&self) -> Option<StatsView> {
        EditorApp::current_stats(self)
    }

    // ------------------------------------------------------------------
    // 语义命令
    // ------------------------------------------------------------------

    fn insert_at_cursor(&mut self, text: &str) -> AppResult<()> {
        EditorApp::insert_at_cursor(self, text)
    }

    fn set_shortcut_overview_query(&mut self, query: String) {
        EditorApp::set_shortcut_overview_query(self, query);
    }
}
