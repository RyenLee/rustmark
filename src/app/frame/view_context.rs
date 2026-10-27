//! [`EditorViewContext`] 的组合根实现（R-03 B1）。
//!
//! # 这个文件是「视图 ↔ 应用」的唯一缝
//!
//! trait 定义在 `rustmark-ui`（视图契约独立于组合根），实现在这里
//! （只有组合根同时看得见 UI 状态与用例服务）。视图因此**只认 trait、
//! 不认 `EditorApp`**，而 B1 的迁移可以按视图逐个推进：已迁移的收
//! `&mut dyn EditorViewContext`，未迁移的仍收 `&mut EditorApp`——因为
//! `EditorApp` 实现了该 trait，`&mut EditorApp` 会在调用点自动强制转换为
//! `&mut dyn EditorViewContext`，两者能在同一次渲染里共存
//! （`overlays::show` 正是这种混合态）。
//!
//! # 为什么每个方法都写 `EditorApp::x(self, …)` 而不是 `self.x(…)`
//!
//! 本实现里有**大量**方法与 `EditorApp` 的 inherent 方法**同名**
//! （`theme_kind_label` / `open_document_list` / `workspace_root` /
//! `current_stats` / `insert_at_cursor` / `execute` / `settings` /
//! `open_path` / `new_document` / `request_dialog` / …）。`self.x(…)` 会按
//! 「inherent 优先」解析到那个同名方法，今天恰好是我们要的；但一旦将来
//! inherent 被删或改名，同一条语句就会静默变成**递归调用自己**——栈溢出，
//! 且编译器不会提示。写成全路径是把这个隐患在结构上掐掉：`EditorApp::x`
//! 只可能指向 inherent（trait 方法必须走 `<EditorApp as Trait>::x`），
//! 指向不明会直接编译失败。
//!
//! **规则**：凡是 trait 方法名与 `EditorApp` 的 inherent 方法同名，一律写
//! `EditorApp::x(self, …)`；名字不同的（如 `theme_state` → `EditorApp::theme`）
//! 也一律写全路径——保持**一致**比「挑着用」更重要，读者不必逐个判断
//! 哪条走全路径、哪条走 `self.`。
//!
//! # 状态访问器为什么不需要这层保护
//!
//! `ui_state` / `dialog_state` / `sidebar_state` 这些名字在 `EditorApp` 上
//! 不存在同名的 inherent 方法或字段，故直接取字段即可。
//! 这正是 trait 侧统一加 `_state` 后缀的原因——见
//! `rustmark_ui::presentation::view_context` 的模块文档。

use super::EditorApp;
use crate::presentation::shortcuts::dispatcher::ShortcutDispatcher;
use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::state::app_ui_state::AppUiState;
use crate::presentation::state::command_palette_ui_state::CommandPaletteUiState;
use crate::presentation::state::dialog_ui_state::DialogUiState;
use crate::presentation::state::diff_ui_state::DiffUiState;
use crate::presentation::state::editor_ui_state::EditorUiState;
use crate::presentation::state::export_ui_state::ExportUiState;
use crate::presentation::state::find_replace_ui_state::FindReplaceUiState;
use crate::presentation::state::pending_dialog::PendingDialog;
use crate::presentation::state::quick_open_ui_state::QuickOpenUiState;
use crate::presentation::state::settings_ui_state::{SettingsTab, SettingsUiState};
use crate::presentation::state::shortcut_overview_ui_state::ShortcutOverviewUiState;
use crate::presentation::state::sidebar_ui_state::{
    SearchScope, SearchToggle, SidebarUiState, WorkspaceSearch,
};
use crate::presentation::state::symbol_ui_state::SymbolUiState;
use crate::presentation::state::workspace_entries::WorkspaceEntries;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::{
    CommandPort, DocumentAccess, EditorViewContext, ServiceQueries, ThemeAccess, UiStateAccess,
};
use rustmark_core::application::dto::{
    EditorSettingsView, ExportRequestView, ExportResultView, LintIssueView, OutlineItemView,
    RecentFileView, SettingsView, StatsView, TabSummary, ThemeView,
};
use rustmark_core::shared::ids::DocumentId;
use rustmark_core::shared::kernel::export_format::PageSize;
use rustmark_core::shared::kernel::fonts::EditorFontChoice;
use rustmark_core::shared::result::AppResult;

// ===========================================================================
// T1 · ThemeAccess
// ===========================================================================

impl ThemeAccess for EditorApp {
    fn theme_state(&self) -> &Theme {
        EditorApp::theme(self)
    }

    fn dispatcher_state(&self) -> &ShortcutDispatcher {
        EditorApp::dispatcher(self)
    }

    fn theme_kind_label(&self) -> &'static str {
        EditorApp::theme_kind_label(self)
    }
}

// ===========================================================================
// T2 · UiStateAccess
// ===========================================================================

impl UiStateAccess for EditorApp {
    fn ui_state(&mut self) -> &mut AppUiState {
        &mut self.ui
    }

    fn editor_state(&mut self) -> &mut EditorUiState {
        &mut self.editor
    }

    fn sidebar_state(&mut self) -> &mut SidebarUiState {
        &mut self.sidebar
    }

    fn find_state(&mut self) -> &mut FindReplaceUiState {
        &mut self.find
    }

    fn export_ui_state(&mut self) -> &mut ExportUiState {
        &mut self.export_ui
    }

    fn settings_ui_state(&mut self) -> &mut SettingsUiState {
        &mut self.settings_ui
    }

    fn quick_open_ui_state(&mut self) -> &mut QuickOpenUiState {
        &mut self.quick_open_ui
    }

    fn command_palette_ui_state(&mut self) -> &mut CommandPaletteUiState {
        &mut self.command_palette_ui
    }

    fn command_palette_query(&self) -> &str {
        &self.command_palette_ui.query
    }

    fn diff_ui_state(&mut self) -> &mut DiffUiState {
        &mut self.diff_ui
    }

    fn dialog_state(&mut self) -> &mut DialogUiState {
        &mut self.dialog
    }

    fn symbol_ui_state(&mut self) -> &mut SymbolUiState {
        &mut self.symbol_ui
    }

    fn shortcut_overview_ui_state(&self) -> &ShortcutOverviewUiState {
        &self.shortcut_overview_ui
    }

    fn workspace_entries_state(&self) -> &WorkspaceEntries {
        &self.workspace_entries
    }

    fn editor_text(&self) -> &str {
        &self.editor_text
    }
}

// ===========================================================================
// T3 · DocumentAccess
// ===========================================================================

impl DocumentAccess for EditorApp {
    fn open_document_list(&self) -> Vec<TabSummary> {
        EditorApp::open_document_list(self)
    }

    fn workspace_root(&self) -> String {
        EditorApp::workspace_root(self)
    }

    fn current_stats(&self) -> Option<StatsView> {
        EditorApp::current_stats(self)
    }

    fn active_doc(&self) -> Option<DocumentId> {
        self.active_doc
    }

    fn current_document_path(&self) -> String {
        EditorApp::current_document_path(self)
    }

    fn current_content(&self) -> Option<String> {
        EditorApp::current_content(self)
    }

    fn recent_files(&self) -> Vec<RecentFileView> {
        EditorApp::recent_files(self)
    }

    fn current_outline(&self) -> Vec<OutlineItemView> {
        EditorApp::current_outline(self)
    }

    fn lint_current_document(&self) -> Vec<LintIssueView> {
        EditorApp::lint_current_document(self)
    }

    fn settings(&self) -> SettingsView {
        EditorApp::settings(self)
    }

    fn system_prefers_dark(&self) -> bool {
        EditorApp::system_prefers_dark(self)
    }

    fn editor_font_choices(&self) -> &[EditorFontChoice] {
        EditorApp::editor_font_choices(self)
    }

    fn unsupported_format_notice(&self, path: &str) -> String {
        EditorApp::unsupported_format_notice(path)
    }
}

// ===========================================================================
// T4 · CommandPort
// ===========================================================================

impl CommandPort for EditorApp {
    // ---------- 动作总线 ----------

    fn execute(&mut self, action: UiAction) {
        EditorApp::execute(self, action);
    }

    fn insert_at_cursor(&mut self, text: &str) -> AppResult<()> {
        EditorApp::insert_at_cursor(self, text)
    }

    fn set_shortcut_overview_query(&mut self, query: String) {
        EditorApp::set_shortcut_overview_query(self, query);
    }

    // ---------- 文件 / 工作区 ----------

    fn new_document(&mut self) -> AppResult<()> {
        EditorApp::new_document(self)
    }

    fn open_path(&mut self, path: &str) -> AppResult<()> {
        EditorApp::open_path(self, path)
    }

    fn open_workspace_file(&mut self, path: &str) -> AppResult<()> {
        EditorApp::open_workspace_file(self, path)
    }

    fn delete_entry(&mut self, path: &str) -> AppResult<()> {
        EditorApp::delete_entry(self, path)
    }

    fn refresh_workspace(&mut self) -> usize {
        EditorApp::refresh_workspace(self)
    }

    fn can_open_as_text(&self, path: &str) -> bool {
        EditorApp::can_open_as_text(self, path)
    }

    fn report_unsupported_format(&mut self, path: &str) {
        EditorApp::report_unsupported_format(self, path);
    }

    fn read_workspace_file(&self, path: &str) -> AppResult<String> {
        EditorApp::read_workspace_file(self, path)
    }

    fn request_dialog(&mut self, kind: PendingDialog) {
        EditorApp::request_dialog(self, kind);
    }

    fn set_status_error(&mut self, text: &str) {
        EditorApp::set_status_error(self, text);
    }

    // ---------- 标签 ----------

    fn switch_document(&mut self, id: DocumentId) {
        EditorApp::switch_document(self, id);
    }

    fn close_document(&mut self, id: DocumentId) {
        EditorApp::close_document(self, id);
    }

    fn close_all_with_confirm(&mut self) {
        EditorApp::close_all_with_confirm(self);
    }

    fn set_tabstrip_scroll_x(&mut self, x: f32) {
        EditorApp::set_tabstrip_scroll_x(self, x);
    }

    fn set_tabstrip_last_active(&mut self, id: Option<u64>) {
        EditorApp::set_tabstrip_last_active(self, id);
    }

    // ---------- 查找替换 ----------

    fn sync_find(&mut self) {
        EditorApp::sync_find(self);
    }

    fn find_step(&mut self, forward: bool) {
        EditorApp::find_step(self, forward);
    }

    fn set_find_focused(&mut self, focused: bool) {
        EditorApp::set_find_focused(self, focused);
    }

    fn find_replace_current(&mut self) {
        EditorApp::find_replace_current(self);
    }

    fn find_replace_all(&mut self) {
        EditorApp::find_replace_all(self);
    }

    // ---------- 大纲 / 搜索 / 跳转 ----------

    fn goto_outline_line(&mut self, line: usize) {
        EditorApp::goto_outline_line(self, line);
    }

    fn goto_byte_offset(&mut self, offset: usize) {
        EditorApp::goto_byte_offset(self, offset);
    }

    fn select_outline_item(&mut self, text: String) {
        EditorApp::select_outline_item(self, text);
    }

    fn select_sidebar_path(&mut self, path: String) {
        EditorApp::select_sidebar_path(self, path);
    }

    fn set_search_query(&mut self, query: String) {
        EditorApp::set_search_query(self, query);
    }

    fn set_search_scope(&mut self, scope: SearchScope) {
        EditorApp::set_search_scope(self, scope);
    }

    fn toggle_search_option(&mut self, which: SearchToggle) {
        EditorApp::toggle_search_option(self, which);
    }

    fn reset_search_options(&mut self) {
        EditorApp::reset_search_options(self);
    }

    fn search_workspace(&self) -> AppResult<WorkspaceSearch> {
        EditorApp::search_workspace(self)
    }

    fn restore_create_draft(&mut self, parent: String, name: String, is_directory: bool) {
        EditorApp::restore_create_draft(self, parent, name, is_directory);
    }

    fn restore_rename_draft(&mut self, path: String, name: String) {
        EditorApp::restore_rename_draft(self, path, name);
    }

    // ---------- 编辑开关 ----------

    fn set_word_wrap(&mut self, on: bool) {
        EditorApp::set_word_wrap(self, on);
    }

    fn set_spell_check(&mut self, on: bool) {
        EditorApp::set_spell_check(self, on);
    }

    fn set_line_numbers(&mut self, on: bool) {
        EditorApp::set_line_numbers(self, on);
    }

    fn set_load_remote_images(&mut self, on: bool) {
        EditorApp::set_load_remote_images(self, on);
    }

    // ---------- 工作区条目增删改 ----------

    fn create_entry(&mut self, parent: &str, name: &str, is_directory: bool) -> AppResult<()> {
        EditorApp::create_entry(self, parent, name, is_directory)
    }

    fn rename_entry(&mut self, path: &str, new_name: &str) -> AppResult<()> {
        EditorApp::rename_entry(self, path, new_name)
    }

    // ---------- 差异 ----------

    fn set_diff_error(&mut self, message: String) {
        EditorApp::set_diff_error(self, message);
    }

    fn clear_diff_selection(&mut self) {
        EditorApp::clear_diff_selection(self);
    }

    // ---------- 导出 ----------

    fn set_export_long_image(&mut self, long: bool) {
        EditorApp::set_export_long_image(self, long);
    }

    fn set_export_include_toc(&mut self, on: bool) {
        EditorApp::set_export_include_toc(self, on);
    }

    fn set_export_embed_images(&mut self, embed: bool) {
        EditorApp::set_export_embed_images(self, embed);
    }

    fn set_export_page_size(&mut self, size: PageSize) {
        EditorApp::set_export_page_size(self, size);
    }

    fn set_export_paginate(&mut self, paginate: bool) {
        EditorApp::set_export_paginate(self, paginate);
    }

    fn set_pdf_include_toc(&mut self, toc: bool) {
        EditorApp::set_pdf_include_toc(self, toc);
    }

    fn begin_viewport_capture(&mut self, path: &str) -> AppResult<()> {
        EditorApp::begin_viewport_capture(self, path)
    }

    fn begin_long_capture(&mut self, path: &str, pixels_per_point: f32) -> AppResult<()> {
        EditorApp::begin_long_capture(self, path, pixels_per_point)
    }

    fn export_current_document(
        &mut self,
        request: ExportRequestView,
        path: &str,
    ) -> AppResult<ExportResultView> {
        EditorApp::export_current_document(self, request, path)
    }

    // ---------- 设置 ----------

    fn apply_theme_field(&mut self, mutate: &mut dyn FnMut(&mut ThemeView)) {
        // `&mut dyn FnMut(..)` 本身实现了 `FnOnce(..)`（std 对 `&mut F where
        // F: FnMut + ?Sized` 有 impl），故可直接交给原签名。
        EditorApp::apply_theme_field(self, mutate);
    }

    fn apply_editor_field(&mut self, mutate: &mut dyn FnMut(&mut EditorSettingsView)) {
        EditorApp::apply_editor_field(self, mutate);
    }

    fn set_settings_section(&mut self, section: SettingsTab) {
        EditorApp::set_settings_section(self, section);
    }

    fn reset_current_settings_section(&mut self) {
        EditorApp::reset_current_settings_section(self);
    }

    fn reset_all_shortcuts(&mut self) {
        EditorApp::reset_all_shortcuts(self);
    }

    fn cancel_settings(&mut self) {
        EditorApp::cancel_settings(self);
    }

    fn confirm_settings(&mut self) {
        EditorApp::confirm_settings(self);
    }

    fn clear_settings_messages(&mut self) {
        EditorApp::clear_settings_messages(self);
    }

    fn clear_settings_pending_conflict(&mut self) {
        EditorApp::clear_settings_pending_conflict(self);
    }

    fn rebind_shortcut(&mut self, action: UiAction, combo: &str) {
        EditorApp::rebind_shortcut(self, action, combo);
    }
}

// ===========================================================================
// T5 · ServiceQueries（批 1 为空）
// ===========================================================================

impl ServiceQueries for EditorApp {}

// ===========================================================================
// bundle
// ===========================================================================

impl EditorViewContext for EditorApp {}
