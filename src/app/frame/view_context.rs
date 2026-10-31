//! [`EditorViewContext`] 的组合根实现（R-03 B1；2026-10-30 按视图窄化）。
//!
//! # 这个文件是「视图 ↔ 应用」的唯一缝
//!
//! trait 定义在 `rustmark-ui`（视图契约独立于组合根），实现在这里
//! （只有组合根同时看得见 UI 状态与用例服务）。视图因此**只认窄 trait、
//! 不认 `EditorApp`**：每个视图收自己的那一份（`AboutViewAccess` /
//! `EditorViewAccess` / `StatusBarViewAccess` …），只有路由者
//! （`main_view` / `overlays`）收 bundle `&mut dyn EditorViewContext`。
//!
//! 本文件因此实现**能力 trait**（状态直取 / 只读投影 / 命令 / 五组只读查询），
//! 而 17 个按视图窄 trait 由 `rustmark-ui` 侧的 blanket impl 自动得到：
//! `impl<T: 能力集合 + ?Sized> XViewAccess for T {}`，故这里不逐个手写。
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
use crate::presentation::state::editor_ui_state::{EditorUiState, SpellSuggestMenu};
use crate::presentation::state::export_ui_state::ExportUiState;
use crate::presentation::state::find_replace_ui_state::FindReplaceUiState;
use crate::presentation::state::pending_dialog::PendingDialog;
use crate::presentation::state::preview_ui_state::PreviewUiState;
use crate::presentation::state::quick_open_ui_state::QuickOpenUiState;
use crate::presentation::state::scroll_sync_state::BlockRect;
use crate::presentation::state::settings_ui_state::{SettingsTab, SettingsUiState};
use crate::presentation::state::shortcut_overview_ui_state::ShortcutOverviewUiState;
use crate::presentation::state::sidebar_ui_state::{
    SearchScope, SearchToggle, SidebarUiState, WorkspaceSearch,
};
use crate::presentation::state::status_bar_ui_state::StatusBarUiState;
use crate::presentation::state::symbol_ui_state::SymbolUiState;
use crate::presentation::state::workspace_entries::WorkspaceEntries;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::{
    AppUiAccess, ClockAccess, CommandPaletteUiAccess, CommandPort, DialogStackAccess, DiffUiAccess,
    DocumentAccess, DocumentQueries, EditorUiAccess, EditorViewContext, ExportUiAccess,
    ExternalLinks, FindUiAccess, ImageFetcher, MarkdownQueries, PreferencesQueries,
    PreviewUiAccess, QuickOpenUiAccess, SettingsUiAccess, ShortcutOverviewUiAccess,
    SidebarUiAccess, StatusBarUiAccess, SymbolUiAccess, TextEdit, ThemeAccess,
    WorkspaceEntriesAccess,
};
use egui::Context;
use rustmark_core::application::dto::{
    BlockView, EditorSettingsView, ExportRequestView, ExportResultView, GoalProgressView,
    LintIssueView, OutlineItemView, RecentFileView, RenderedView, SettingsView, SpanView,
    StatsView, TabSummary, ThemeView,
};
use rustmark_core::application::markdown::queries::{
    HighlightQuery, HighlightSnippetQuery, MapScrollQuery, RenderPreviewQuery, ScrollTarget,
    SpellCheckQuery, SpellSuggestQuery,
};
use rustmark_core::shared::encoding::TextEncoding;
use rustmark_core::shared::ids::DocumentId;
use rustmark_core::shared::kernel::export_format::PageSize;
use rustmark_core::shared::kernel::fonts::EditorFontChoice;
use rustmark_core::shared::result::AppResult;
use std::sync::Arc;

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
// T2 · UI 状态直取（15 个按面板窄 trait）
// ===========================================================================
//
// 每个 impl 只取一个字段；拆成 15 个块是为了让「谁给哪个视图什么样状态」
// 在实现侧也一一对应（契约侧同构，见 `rustmark_ui` 的模块文档）。

impl AppUiAccess for EditorApp {
    fn ui_state(&mut self) -> &mut AppUiState {
        &mut self.ui
    }
}

impl EditorUiAccess for EditorApp {
    fn editor_state(&mut self) -> &mut EditorUiState {
        &mut self.editor
    }

    fn editor_state_read(&self) -> &EditorUiState {
        &self.editor
    }

    fn editor_text(&self) -> &str {
        &self.editor_text
    }

    fn editor_text_mut(&mut self) -> &mut String {
        &mut self.editor_text
    }
}

impl StatusBarUiAccess for EditorApp {
    fn status_bar_state(&mut self) -> &mut StatusBarUiState {
        &mut self.status_bar
    }

    fn status_bar_state_read(&self) -> &StatusBarUiState {
        &self.status_bar
    }
}

impl SidebarUiAccess for EditorApp {
    fn sidebar_state(&mut self) -> &mut SidebarUiState {
        &mut self.sidebar
    }
}

impl FindUiAccess for EditorApp {
    fn find_state(&mut self) -> &mut FindReplaceUiState {
        &mut self.find
    }
}

impl ExportUiAccess for EditorApp {
    fn export_ui_state(&mut self) -> &mut ExportUiState {
        &mut self.export_ui
    }
}

impl SettingsUiAccess for EditorApp {
    fn settings_ui_state(&mut self) -> &mut SettingsUiState {
        &mut self.settings_ui
    }
}

impl QuickOpenUiAccess for EditorApp {
    fn quick_open_ui_state(&mut self) -> &mut QuickOpenUiState {
        &mut self.quick_open_ui
    }
}

impl CommandPaletteUiAccess for EditorApp {
    fn command_palette_ui_state(&mut self) -> &mut CommandPaletteUiState {
        &mut self.command_palette_ui
    }

    fn command_palette_state_read(&self) -> &CommandPaletteUiState {
        &self.command_palette_ui
    }
}

impl DiffUiAccess for EditorApp {
    fn diff_ui_state(&mut self) -> &mut DiffUiState {
        &mut self.diff_ui
    }
}

impl DialogStackAccess for EditorApp {
    fn dialog_state(&mut self) -> &mut DialogUiState {
        &mut self.dialog
    }
}

impl SymbolUiAccess for EditorApp {
    fn symbol_ui_state(&mut self) -> &mut SymbolUiState {
        &mut self.symbol_ui
    }
}

impl ShortcutOverviewUiAccess for EditorApp {
    fn shortcut_overview_ui_state(&self) -> &ShortcutOverviewUiState {
        &self.shortcut_overview_ui
    }
}

impl WorkspaceEntriesAccess for EditorApp {
    fn workspace_entries_state(&self) -> &WorkspaceEntries {
        &self.workspace_entries
    }
}

impl PreviewUiAccess for EditorApp {
    fn preview_state(&mut self) -> &mut PreviewUiState {
        &mut self.preview
    }

    fn preview_state_read(&self) -> &PreviewUiState {
        &self.preview
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

    fn current_line_count(&self) -> usize {
        EditorApp::current_line_count(self)
    }

    fn cursor_line_col(&self) -> (usize, usize) {
        EditorApp::cursor_line_col(self)
    }

    fn selection_line_col(&self) -> Option<((usize, usize), (usize, usize))> {
        EditorApp::selection_line_col(self)
    }

    fn current_document_dirty(&self) -> bool {
        EditorApp::current_document_dirty(self)
    }

    fn current_document_kind(&self) -> &'static str {
        EditorApp::current_document_kind(self)
    }

    fn should_show_welcome(&self) -> bool {
        EditorApp::should_show_welcome(self)
    }

    fn document_revision(&self, id: DocumentId) -> Option<u64> {
        // 原调用点是 `services.registry.read().ok().and_then(|reg| reg.revision_of(id))`：
        // 读锁获取失败（中毒）按「拿不到版本号」处理，调用方回退到 0。
        self.services
            .registry
            .read()
            .ok()
            .and_then(|reg| reg.revision_of(id))
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

    fn clear_search(&mut self) {
        EditorApp::clear_search(self);
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

    // ---------- 状态栏 / 编辑区 ----------

    fn current_stats_for_status_bar(&mut self) -> Option<StatsView> {
        EditorApp::current_stats_for_status_bar(self)
    }

    fn set_status_bar_encoding(&mut self, encoding: Option<TextEncoding>) {
        EditorApp::set_status_bar_encoding(self, encoding);
    }

    fn arm_status_message_deadline(&mut self) {
        EditorApp::arm_status_message_deadline(self);
    }

    fn save_document(&mut self, save_as: bool) -> AppResult<()> {
        EditorApp::save_document(self, save_as)
    }

    // ---------- 编辑区 ----------

    fn apply_pending_editor_state(&mut self, ctx: &Context) {
        EditorApp::apply_pending_editor_state(self, ctx);
    }

    fn ensure_document(&mut self) -> Option<DocumentId> {
        EditorApp::ensure_document(self)
    }

    fn set_editor_selection(&mut self, cursor: usize, anchor: Option<usize>) {
        EditorApp::set_editor_selection(self, cursor, anchor);
    }

    fn set_editor_text(&mut self, text: String) {
        EditorApp::set_editor_text(self, text);
    }

    fn flush_edit(&mut self, edit: TextEdit, domain_len: usize, transformed: bool) {
        // `editor_ops::TextEdit` 就是契约里的那个 `TextEdit`（root 侧 `pub use` 转发），
        // 故这里不需要任何转换。
        EditorApp::flush_edit(self, edit, domain_len, transformed);
    }

    fn close_spell_menu(&mut self) {
        EditorApp::close_spell_menu(self);
    }

    fn set_spell_menu(&mut self, menu: Option<SpellSuggestMenu>) {
        EditorApp::set_spell_menu(self, menu);
    }

    fn set_editor_scroll_y(&mut self, y: f32) {
        EditorApp::set_editor_scroll_y(self, y);
    }

    fn replace_spelling(&mut self, start: usize, end: usize, replacement: &str) -> AppResult<()> {
        EditorApp::replace_spelling(self, start, end, replacement)
    }

    // ---------- 预览区 ----------

    fn set_preview_blocks(&mut self, blocks: Vec<BlockView>) {
        EditorApp::set_preview_blocks(self, blocks);
    }

    fn store_preview_render(
        &mut self,
        blocks: Vec<BlockView>,
        rects: Vec<BlockRect>,
        scroll_y: f32,
    ) {
        EditorApp::store_preview_render(self, blocks, rects, scroll_y);
    }

    fn replace_source(&mut self, byte_start: usize, byte_end: usize, text: &str) {
        // `replace_source` 在 `EditorApp` 上是 `pub(crate)`——本文件与它同 crate，
        // 故可见性够用；trait 方法本身的可见性由 trait 决定，不受影响。
        EditorApp::replace_source(self, byte_start, byte_end, text);
    }

    fn preview_image_fetcher(&self, url: &str) -> ImageFetcher {
        // 三样东西在这里凑齐后才装箱：加载器（infra 类型，ui 不可名）、
        // 文档目录（`pub(crate)`）、以及「远程不传基准目录」这条装配知识。
        let loader = self.services.assets;
        let base_dir = EditorApp::document_dir(self);
        let url = url.to_string();
        // 与 `image_cache::is_remote_url` 同一判据：远程地址是绝对的，
        // 不该拿本地文档目录去解析（传进 `load` 只会让相对路径规则参与进来）。
        let remote = {
            let trimmed = url.trim();
            trimmed.starts_with("http://") || trimmed.starts_with("https://")
        };
        Box::new(move || {
            let dir = if remote { None } else { base_dir.as_deref() };
            loader
                .load(&url, dir)
                .map(|asset| asset.bytes)
                .map_err(|e| e.to_string())
        })
    }

    fn set_preview_scroll_y(&mut self, y: f32) {
        EditorApp::set_preview_scroll_y(self, y);
    }

    fn set_preview_rect(&mut self, rect: egui::Rect) {
        EditorApp::set_preview_rect(self, rect);
    }

    // ---------- 布局拖拽与落盘 ----------

    fn set_split_dragging(&mut self, dragging: bool) {
        EditorApp::set_split_dragging(self, dragging);
    }

    fn set_sidebar_dragging(&mut self, dragging: bool) {
        EditorApp::set_sidebar_dragging(self, dragging);
    }

    fn persist_split_ratio(&mut self) {
        EditorApp::persist_split_ratio(self);
    }

    fn persist_sidebar_visibility(&mut self) {
        EditorApp::persist_sidebar_visibility(self);
    }

    fn persist_sidebar_width(&mut self) {
        EditorApp::persist_sidebar_width(self);
    }
}

// ===========================================================================
// T5 · 组合根只读查询（5 个窄 trait）
// ===========================================================================
//
// 这里**不再交出服务本身**：每个 impl 只转发一条语义查询，视图拿不到
// `MarkdownAppService` / `DocumentAppService` / `PreferencesAppService` /
// `ExternalOpener` 这些类型名，也就没法顺带调服务的其它方法。

impl ClockAccess for EditorApp {
    fn now_ms(&self) -> u64 {
        // 「组合根服务」直接取时间戳：`Timestamp::millis()` 返回 `u64`。
        // `EditorApp` 上没有同名 inherent 方法，故走 `self.services.` 全路径即可。
        self.services.markdown.now().millis()
    }
}

impl MarkdownQueries for EditorApp {
    fn highlight(&self, query: HighlightQuery) -> AppResult<Arc<Vec<SpanView>>> {
        self.services.markdown.highlight(query)
    }

    fn highlight_snippet(&self, query: HighlightSnippetQuery) -> AppResult<Vec<SpanView>> {
        self.services.markdown.highlight_snippet(query)
    }

    fn spell_check(&self, query: SpellCheckQuery) -> Vec<(usize, usize)> {
        self.services.markdown.spell_check(query)
    }

    fn spell_suggestions(&self, query: SpellSuggestQuery) -> Vec<String> {
        self.services.markdown.spell_suggestions(query)
    }

    fn map_scroll(&self, query: MapScrollQuery) -> AppResult<ScrollTarget> {
        self.services.markdown.map_scroll(query)
    }

    fn render_preview(&self, query: RenderPreviewQuery) -> AppResult<RenderedView> {
        self.services.markdown.render_preview(query)
    }
}

impl DocumentQueries for EditorApp {
    fn document_encoding(&self, id: DocumentId) -> Option<TextEncoding> {
        self.services.documents.encoding_of(id)
    }
}

impl PreferencesQueries for EditorApp {
    fn has_writing_goal(&self) -> bool {
        self.services.preferences.has_writing_goal()
    }

    fn goal_progress(&self, words: usize) -> GoalProgressView {
        self.services.preferences.goal_progress(words)
    }
}

impl ExternalLinks for EditorApp {
    fn open_external(&self, url: &str) -> std::io::Result<()> {
        self.services.opener.open(url)
    }
}

// ===========================================================================
// bundle
// ===========================================================================

impl EditorViewContext for EditorApp {}
