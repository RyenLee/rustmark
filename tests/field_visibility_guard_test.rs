//! D-3 阶段 4b-5：core **字段级可见性**白名单（R-02 续，方案 §4.2 第 2 条）。
//!
//! # 守的是哪个方向
//!
//! 4b 收紧 `pub` 字段时，**收紧方向**由编译器守门：字段改成 `pub(crate)` 后，
//! shell（root `src/` 与 root `tests/`）的每个读点都会报 E0616 / E0451，
//! 逐个核对后才决定恢复 `pub` 还是补受控方法——这就是方案说的「对照读点
//! 逐字段核对」（方案初估 340 处，编译器实测报出 547 对读点/构造点，
//! 已全部核对为 core 外合法需求）。但**放宽方向编译器沉默**：把
//! `pub(crate) scroll_x` 改回 `pub scroll_x` 不报任何错，接口面悄悄涨回去。
//!
//! 本测试把「core 里哪些字段被批准保持 `pub`」钉成显式清单
//! （阶段 2 借用清单的同款模式）：
//!
//! - 新增 `pub` 字段（含把 `pub(crate)` 放宽回 `pub`）→ 实际集合多出条目
//!   → 报红，必须先动本文件的清单——改测试 = 评审必经，接口面**增长留痕**；
//! - 字段被收紧为 `pub(crate)` 或被删除 → 清单出现过期项 → 同样报红；
//! - 判据是**集合相等**（双向断言）：现实与清单强制同步，字段级可见性的
//!   任何变化都要落到清单上才能通过。
//!
//! # 判定口径（对照阶段 2）
//!
//! - **范围 = `crates/rustmark-core/src` + 从 core 抽出的叶子 crate**
//!   （当前 `crates/rustmark-icons/src`，V11-58；`crates/rustmark-ui/src`，
//!   V11-62 批次 1 起，批次 2 纳入 state 的 168 条）：4b 收的是 core 的面
//!   （review §4.2「core 的 693+ pub 面收缩到真实接口面」）；shell 的 `src/`
//!   是应用根，其 pub 面不属本批次出口。抽出的叶子 crate 的 pub 面同样跨
//!   crate 边界，不纳入就等于**静默放弃覆盖**（V11-57 的 shortcut guard
//!   刚因「搬迁丢锚点」栽过一次——缺口要响，不要静）；
//! - **只收全可见性 `pub`**：`pub(crate)` / `pub(super)` / `pub(in …)` 是
//!   收紧方向的产物，其「core 内部」语义已由编译器保证，无需登记；
//! - **词法真源 = `arch_check::lex_dump`**（与阶段 2 同口径）：注释与字符串
//!   不产生 token——本文件头自己就写着 `pub fake: u64` 的形状，靠词法剔除；
//! - **元组结构体字段**（`pub Vec<…>` 形态，存量 1 处）按 `tuple.N` 登记；
//! - `pub fn` / `pub const` / `pub static` 等**非字段**项不收：它们在词法上
//!   是 `pub` + 关键字 + 名字 + `:`，比字段多出一个关键字（注入用例钉死
//!   这个区分）。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate（原先仅 lib 目标
// 受 lib.rs attribute 管辖）。规范 §七-20 允许测试代码 unwrap/expect/
// panic，同 lib.rs 的 `cfg_attr(test, allow)` 惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 校验器词法的本地接入（与 `tests/presentation/state_borrow_guard_test.rs`
/// 同款先例）。
///
/// `allow(dead_code)` 是必要的：本测试只消费 `lex_dump`，校验器的
/// `scan` / `check_file` 等入口在本编译单元不可达——测试目标的门禁是
/// `-D warnings`，没有这条豁免会在死代码告警上翻车。
#[allow(dead_code)]
mod arch_check {
    include!("../tools/arch_check.rs");
}

/// 已批准保持 `pub` 的 core 与抽出叶子 crate 的字段。
///
/// 键格式（V11-58 起两种前缀，V11-62 起三种）：
///
/// - core 条目（历史口径，548 条不动）：`<相对 crates/rustmark-core/src
///   的路径>::<结构体>::<字段>`；
/// - 抽出的叶子 crate 条目：`<crate 短名>/<相对该 crate src 的路径>::
///   <结构体>::<字段>`（当前 icons：`icons/lib.rs::…`；ui：
///   `ui/presentation/state/…`；**shared（TD-04）：`shared/dialog.rs::…`
///   ——前缀恰好复现搬迁前的 core 相对路径，故 4 条 shared 键零改动**；
///   **infra（V11-64）：`infrastructure/…::…`——同理零改键**）。前缀是**防碰撞**
///   的键命名空间——core 与叶子 crate 各有一个 `lib.rs`，裸路径会把两边的
///   条目合并成同键，白名单 quietly 少算。
///
/// 每一条都是 4b-5 对照外部读点（E0616 读 / E0451 构造）逐字段核对后的
/// 结论：该字段在 core 外有合法读点，或外部需要直接构造（VO 的元组字段）。
/// icons 的两条是 V11-58 随搬迁**改键**（原 `presentation/icons.rs::…`），
/// 结论本身不变（root 视图 `icons::IconColors { accent, primary }` 构造点
/// 即证据）。
///
/// 增删规则：
///
/// - **新增条目** = 接口面增长：先证明「谁在 crate 外读它/构造它」
///   （编译器报点即证据），再登记——登记本身让增长在评审里可见；
/// - **删除条目** = 字段已收紧 / 已删：编译器保证外部没有读点，清单
///   同步移除，双向断言会把忘了删的情况报红。
const APPROVED_PUB_FIELDS: &[&str] = &[
    // total: 559
    "application/document/commands.rs::CloseCommand::force",
    "application/document/commands.rs::CloseCommand::id",
    "application/document/commands.rs::FormatCommand::id",
    "application/document/commands.rs::FormatCommand::indent_unit",
    "application/document/commands.rs::FormatCommand::kind",
    "application/document/commands.rs::FormatCommand::range",
    "application/document/commands.rs::InsertTextCommand::id",
    "application/document/commands.rs::InsertTextCommand::offset",
    "application/document/commands.rs::InsertTextCommand::text",
    "application/document/commands.rs::OpenFileCommand::path",
    "application/document/commands.rs::RedoCommand::id",
    "application/document/commands.rs::ReplaceRangeCommand::id",
    "application/document/commands.rs::ReplaceRangeCommand::range",
    "application/document/commands.rs::ReplaceRangeCommand::text",
    "application/document/commands.rs::RevertCommand::id",
    "application/document/commands.rs::SaveCommand::id",
    "application/document/commands.rs::SaveCommand::save_as",
    "application/document/commands.rs::SaveToPathCommand::id",
    "application/document/commands.rs::SaveToPathCommand::path",
    "application/document/commands.rs::SetSelectionCommand::id",
    "application/document/commands.rs::SetSelectionCommand::selection",
    "application/document/commands.rs::UndoCommand::id",
    "application/document/find.rs::FindOptions::case_sensitive",
    "application/document/find.rs::FindOptions::regex",
    "application/document/find.rs::FindOptions::whole_word",
    "application/document/find.rs::Match::end",
    "application/document/find.rs::Match::start",
    "application/document/find.rs::ReplaceSummary::count",
    "application/document/find.rs::ReplaceSummary::result",
    "application/document/find.rs::ReplaceSummary::will_match_again",
    "application/document/queries.rs::GetContentQuery::id",
    "application/document/queries.rs::GetContentQuery::range",
    "application/document/queries.rs::GetDocumentQuery::id",
    "application/document/queries.rs::GetSelectionQuery::id",
    "application/document/queries.rs::GetStatsQuery::id",
    "application/document/service.rs::DocumentAppService::stats_cache_hits",
    "application/dto/document.rs::DocumentView::byte_len",
    "application/dto/document.rs::DocumentView::cursor",
    "application/dto/document.rs::DocumentView::id",
    "application/dto/document.rs::DocumentView::is_dirty",
    "application/dto/document.rs::DocumentView::path",
    "application/dto/document.rs::DocumentView::selection",
    "application/dto/document.rs::DocumentView::title",
    "application/dto/document.rs::OutlineItemView::level",
    "application/dto/document.rs::OutlineItemView::line",
    "application/dto/document.rs::OutlineItemView::text",
    "application/dto/document.rs::OutlineView::tuple.0",
    "application/dto/document.rs::RangeView::end",
    "application/dto/document.rs::RangeView::start",
    "application/dto/document.rs::SearchHitView::col",
    "application/dto/document.rs::SearchHitView::line",
    "application/dto/document.rs::SearchHitView::path",
    "application/dto/document.rs::SearchHitView::snippet",
    "application/dto/document.rs::StatsView::characters",
    "application/dto/document.rs::StatsView::characters_no_space",
    "application/dto/document.rs::StatsView::code_blocks",
    "application/dto/document.rs::StatsView::headings",
    "application/dto/document.rs::StatsView::images",
    "application/dto/document.rs::StatsView::lines",
    "application/dto/document.rs::StatsView::links",
    "application/dto/document.rs::StatsView::paragraphs",
    "application/dto/document.rs::StatsView::reading_minutes",
    "application/dto/document.rs::StatsView::tasks_done",
    "application/dto/document.rs::StatsView::tasks_total",
    "application/dto/document.rs::StatsView::title",
    "application/dto/document.rs::StatsView::words",
    "application/dto/document.rs::TabSummary::byte_len",
    "application/dto/document.rs::TabSummary::id",
    "application/dto/document.rs::TabSummary::is_dirty",
    "application/dto/document.rs::TabSummary::path",
    "application/dto/document.rs::TabSummary::title",
    "application/dto/markdown.rs::BlockView::alt",
    "application/dto/markdown.rs::BlockView::content",
    "application/dto/markdown.rs::BlockView::definitions",
    "application/dto/markdown.rs::BlockView::diagram",
    "application/dto/markdown.rs::BlockView::display",
    "application/dto/markdown.rs::BlockView::id",
    "application/dto/markdown.rs::BlockView::image_align",
    "application/dto/markdown.rs::BlockView::image_height",
    "application/dto/markdown.rs::BlockView::image_width",
    "application/dto/markdown.rs::BlockView::images",
    "application/dto/markdown.rs::BlockView::items",
    "application/dto/markdown.rs::BlockView::kind",
    "application/dto/markdown.rs::BlockView::label",
    "application/dto/markdown.rs::BlockView::lang",
    "application/dto/markdown.rs::BlockView::level",
    "application/dto/markdown.rs::BlockView::marker",
    "application/dto/markdown.rs::BlockView::ordered",
    "application/dto/markdown.rs::BlockView::spans",
    "application/dto/markdown.rs::BlockView::table",
    "application/dto/markdown.rs::BlockView::url",
    "application/dto/markdown.rs::DefinitionBodyView::spans",
    "application/dto/markdown.rs::DefinitionBodyView::text",
    "application/dto/markdown.rs::DefinitionEntryView::definitions",
    "application/dto/markdown.rs::DefinitionEntryView::term",
    "application/dto/markdown.rs::DefinitionEntryView::term_spans",
    "application/dto/markdown.rs::ImageSpecView::align",
    "application/dto/markdown.rs::ImageSpecView::alt",
    "application/dto/markdown.rs::ImageSpecView::height",
    "application/dto/markdown.rs::ImageSpecView::url",
    "application/dto/markdown.rs::ImageSpecView::width",
    "application/dto/markdown.rs::InlineImageView::image",
    "application/dto/markdown.rs::InlineImageView::range",
    "application/dto/markdown.rs::ListItemView::checked",
    "application/dto/markdown.rs::ListItemView::depth",
    "application/dto/markdown.rs::ListItemView::end",
    "application/dto/markdown.rs::ListItemView::images",
    "application/dto/markdown.rs::ListItemView::ordinal",
    "application/dto/markdown.rs::ListItemView::spans",
    "application/dto/markdown.rs::ListItemView::start",
    "application/dto/markdown.rs::ListItemView::text",
    "application/dto/markdown.rs::RenderedView::blocks",
    "application/dto/markdown.rs::RenderedView::truncated",
    "application/dto/markdown.rs::RenderedView::version",
    "application/dto/markdown.rs::SpanView::range",
    "application/dto/markdown.rs::SpanView::style",
    "application/dto/markdown.rs::SpanView::url",
    "application/dto/markdown.rs::TableView::aligns",
    "application/dto/markdown.rs::TableView::header",
    "application/dto/markdown.rs::TableView::header_spans",
    "application/dto/markdown.rs::TableView::rows",
    "application/dto/markdown.rs::TableView::rows_spans",
    "application/dto/preferences.rs::CustomThemePaletteView::accent",
    "application/dto/preferences.rs::CustomThemePaletteView::bg_primary",
    "application/dto/preferences.rs::CustomThemePaletteView::bg_secondary",
    "application/dto/preferences.rs::CustomThemePaletteView::border",
    "application/dto/preferences.rs::CustomThemePaletteView::code_bg",
    "application/dto/preferences.rs::CustomThemePaletteView::selection_bg",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_bold",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_code",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_heading",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_italic",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_link",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_marker",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_meta",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_other",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_quote",
    "application/dto/preferences.rs::CustomThemePaletteView::syn_table",
    "application/dto/preferences.rs::CustomThemePaletteView::text_primary",
    "application/dto/preferences.rs::CustomThemePaletteView::text_secondary",
    "application/dto/preferences.rs::EditorSettingsView::auto_save_interval",
    "application/dto/preferences.rs::EditorSettingsView::lint",
    "application/dto/preferences.rs::EditorSettingsView::load_remote_images",
    "application/dto/preferences.rs::EditorSettingsView::max_open_documents",
    "application/dto/preferences.rs::EditorSettingsView::show_line_numbers",
    "application/dto/preferences.rs::EditorSettingsView::sidebar_visible",
    "application/dto/preferences.rs::EditorSettingsView::sidebar_width",
    "application/dto/preferences.rs::EditorSettingsView::spell_check",
    "application/dto/preferences.rs::EditorSettingsView::split_ratio",
    "application/dto/preferences.rs::EditorSettingsView::tab_size",
    "application/dto/preferences.rs::EditorSettingsView::word_wrap",
    "application/dto/preferences.rs::EditorSettingsView::writing_goal",
    "application/dto/preferences.rs::GoalProgressView::goal",
    "application/dto/preferences.rs::GoalProgressView::percent",
    "application/dto/preferences.rs::GoalProgressView::reached",
    "application/dto/preferences.rs::GoalProgressView::words",
    "application/dto/preferences.rs::LintIssueView::line",
    "application/dto/preferences.rs::LintIssueView::message",
    "application/dto/preferences.rs::LintIssueView::rule",
    "application/dto/preferences.rs::LintIssueView::rule_label",
    "application/dto/preferences.rs::LintRulesView::heading_jump",
    "application/dto/preferences.rs::LintRulesView::list_marker_mix",
    "application/dto/preferences.rs::LintRulesView::trailing_whitespace",
    "application/dto/preferences.rs::LintRulesView::unclosed_fence",
    "application/dto/preferences.rs::SettingsView::editor",
    "application/dto/preferences.rs::SettingsView::shortcuts",
    "application/dto/preferences.rs::SettingsView::theme",
    "application/dto/preferences.rs::ShortcutView::action",
    "application/dto/preferences.rs::ShortcutView::combo",
    "application/dto/preferences.rs::ShortcutView::key",
    "application/dto/preferences.rs::ThemeView::custom",
    "application/dto/preferences.rs::ThemeView::font_family",
    "application/dto/preferences.rs::ThemeView::font_size",
    "application/dto/preferences.rs::ThemeView::kind",
    "application/dto/preferences.rs::ThemeView::line_height",
    "application/dto/workspace.rs::ConflictView::combo",
    "application/dto/workspace.rs::ExportRequestView::embed_images",
    "application/dto/workspace.rs::ExportRequestView::format",
    "application/dto/workspace.rs::ExportRequestView::include_toc",
    "application/dto/workspace.rs::ExportRequestView::page_size",
    "application/dto/workspace.rs::ExportRequestView::paginate",
    "application/dto/workspace.rs::ExportRequestView::theme",
    "application/dto/workspace.rs::ExportResultView::bytes",
    "application/dto/workspace.rs::ExportResultView::format",
    "application/dto/workspace.rs::ExportResultView::output_path",
    "application/dto/workspace.rs::FileEntryView::is_directory",
    "application/dto/workspace.rs::FileEntryView::path",
    "application/dto/workspace.rs::FormatResultView::changed",
    "application/dto/workspace.rs::FormatResultView::selection",
    "application/dto/workspace.rs::FormatResultView::text",
    "application/dto/workspace.rs::RecentFileView::last_opened",
    "application/dto/workspace.rs::RecentFileView::path",
    "application/dto/workspace.rs::SearchResultView::hits",
    "application/dto/workspace.rs::SearchResultView::truncated",
    "application/export/commands.rs::CreateExportJobCommand::destination",
    "application/export/commands.rs::CreateExportJobCommand::doc_id",
    "application/export/commands.rs::CreateExportJobCommand::request",
    "application/export/commands.rs::RunExportCommand::job_id",
    "application/export/queries.rs::JobStatusQuery::job_id",
    "application/markdown/commands.rs::ClearCacheCommand::doc_id",
    "application/markdown/queries.rs::HighlightQuery::doc_id",
    "application/markdown/queries.rs::HighlightQuery::lang",
    "application/markdown/queries.rs::HighlightQuery::range",
    "application/markdown/queries.rs::HighlightSnippetQuery::lang",
    "application/markdown/queries.rs::HighlightSnippetQuery::text",
    "application/markdown/queries.rs::LintQuery::doc_id",
    "application/markdown/queries.rs::LintQuery::rules",
    "application/markdown/queries.rs::MapScrollQuery::doc_id",
    "application/markdown/queries.rs::MapScrollQuery::source",
    "application/markdown/queries.rs::RebuildOutlineQuery::doc_id",
    "application/markdown/queries.rs::RenderPreviewQuery::doc_id",
    "application/markdown/queries.rs::RenderPreviewQuery::version",
    "application/markdown/queries.rs::SpellCheckQuery::doc_id",
    "application/markdown/queries.rs::SpellCheckQuery::enabled",
    "application/markdown/queries.rs::SpellSuggestQuery::doc_id",
    "application/markdown/queries.rs::SpellSuggestQuery::limit",
    "application/markdown/queries.rs::SpellSuggestQuery::offset",
    "application/preferences/commands.rs::ResetSectionCommand::section",
    "application/preferences/commands.rs::SetEditorCommand::editor",
    "application/preferences/commands.rs::SetShortcutCommand::action",
    "application/preferences/commands.rs::SetShortcutCommand::combo",
    "application/preferences/commands.rs::SetThemeCommand::theme",
    "application/workspace/commands.rs::CloseTabCommand::dirty",
    "application/workspace/commands.rs::CloseTabCommand::doc_id",
    "application/workspace/commands.rs::CreateEntryCommand::is_directory",
    "application/workspace/commands.rs::CreateEntryCommand::path",
    "application/workspace/commands.rs::DeleteEntryCommand::path",
    "application/workspace/commands.rs::ForceCloseTabCommand::doc_id",
    "application/workspace/commands.rs::ForgetRecentCommand::path",
    "application/workspace/commands.rs::MoveEntryCommand::from",
    "application/workspace/commands.rs::MoveEntryCommand::to",
    "application/workspace/commands.rs::OpenTabCommand::doc_id",
    "application/workspace/commands.rs::ScanTreeCommand::max_depth",
    "application/workspace/commands.rs::ScanTreeCommand::root",
    "application/workspace/commands.rs::TouchRecentCommand::path",
    "application/workspace/queries.rs::ReadFileQuery::path",
    "application/workspace/queries.rs::SearchQuery::case_sensitive",
    "application/workspace/queries.rs::SearchQuery::only_document",
    "application/workspace/queries.rs::SearchQuery::only_open_documents",
    "application/workspace/queries.rs::SearchQuery::pattern",
    "application/workspace/queries.rs::SearchQuery::regex",
    "application/workspace/queries.rs::SearchQuery::whole_word",
    "application/workspace/service.rs::WorkspaceAppService::search_cache_hits",
    "application/workspace/service.rs::WorkspaceAppService::search_cache_misses",
    "domain/document/repository.rs::DocumentSnapshot::content",
    "domain/document/repository.rs::DocumentSnapshot::path",
    "domain/document/repository.rs::DocumentSnapshot::saved_at",
    "domain/markdown/value_objects/block.rs::ImageDisplay::align",
    "domain/markdown/value_objects/block.rs::ImageDisplay::height",
    "domain/markdown/value_objects/block.rs::ImageDisplay::width",
    "domain/markdown/value_objects/render.rs::Misspelling::range",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::accent",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::bg_primary",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::bg_secondary",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::border",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::code_bg",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::selection_bg",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_bold",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_code",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_heading",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_italic",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_link",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_marker",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_meta",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_other",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_quote",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::syn_table",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::text_primary",
    "domain/preferences/value_objects/theme.rs::CustomThemePalette::text_secondary",
    "domain/preferences/value_objects/theme.rs::LintSettings::heading_jump",
    "domain/preferences/value_objects/theme.rs::LintSettings::list_marker_mix",
    "domain/preferences/value_objects/theme.rs::LintSettings::trailing_whitespace",
    "domain/preferences/value_objects/theme.rs::LintSettings::unclosed_fence",
    "domain/preferences/value_objects/theme.rs::Theme::custom",
    // V11-58：icons 抽去独立 crate `rustmark-icons`（单文件 lib.rs），
    // 条目带 crate 短名前缀——见下方键格式说明。
    "icons/lib.rs::IconColors::accent",
    "icons/lib.rs::IconColors::primary",
    "infrastructure/export/system_pdf_exporter/backend.rs::PdfBackend::available",
    "infrastructure/export/system_pdf_exporter/backend.rs::PdfBackend::command",
    "infrastructure/export/system_pdf_exporter/backend.rs::PdfBackend::reason",
    "infrastructure/export/system_pdf_exporter/backend.rs::PdfBackend::writes_pdf",
    // V11-63 阶段 A 端口化：viewport_capture 产 `PortFrame`（RGBA8 字节，
    // egui 无关）替代 `egui::ColorImage`；两字段是核心外（app/frame/capture.rs
    // 消费者）合法读点——接口增长留痕，正是本守卫本意。
    "infrastructure/export/viewport_capture.rs::PortFrame::size",
    "infrastructure/export/viewport_capture.rs::PortFrame::pixels",
    "infrastructure/fonts.rs::FontSetup::cjk_source",
    "infrastructure/fonts.rs::FontSetup::definitions",
    "infrastructure/fonts.rs::FontSetup::editor_fonts",
    // V11-63 阶段 B 端口化：core 的字体产物改为 egui 无关的端口类型。
    // `PortFont*` 的字段是组合根适配器（`src/app/frame.rs` 的
    // `to_egui_font_definitions`）在 crate 外的**读点**——适配器要逐字段
    // 重建 egui 类型，必须能读；`FontSource` 三字段是迁到
    // `tests/presentation/font_glyph_test.rs` 的端到端字形测试的读点
    // （原为 core 内单测，`pub(crate)` 够用；迁出后需 `pub`）。
    "infrastructure/fonts/discovery.rs::FontSource::display_name",
    "infrastructure/fonts/discovery.rs::FontSource::key",
    "infrastructure/fonts/discovery.rs::FontSource::path",
    "infrastructure/fonts/port.rs::PortFontData::bytes",
    "infrastructure/fonts/port.rs::PortFontData::scale",
    "infrastructure/fonts/port.rs::PortFontDefinitions::families",
    "infrastructure/fonts/port.rs::PortFontDefinitions::font_data",
    "infrastructure/persistence/asset_loader/policy.rs::AssetLimits::download_timeout",
    "infrastructure/persistence/asset_loader/policy.rs::AssetLimits::max_inline_bytes",
    "infrastructure/persistence/asset_loader/policy.rs::AssetLimits::max_local_bytes",
    "infrastructure/persistence/asset_loader/policy.rs::LoadedAsset::alt",
    "infrastructure/persistence/asset_loader/policy.rs::LoadedAsset::bytes",
    "infrastructure/persistence/asset_loader/policy.rs::LoadedAsset::mime",
    "infrastructure/persistence/fs_workspace_repository.rs::TraversalRules::ignored_dirs",
    "infrastructure/persistence/fs_workspace_repository.rs::TraversalRules::max_entries",
    "infrastructure/persistence/fs_workspace_repository.rs::TraversalRules::skip_hidden",
    "ui/presentation/rich/math/layout.rs::MathLayout::ascent",
    "ui/presentation/rich/math/layout.rs::MathLayout::width",
    "ui/presentation/rich/mermaid/model.rs::MermaidLayout::height",
    "ui/presentation/rich/mermaid/model.rs::MermaidLayout::width",
    "ui/presentation/rich/mermaid/primitives/paint.rs::MermaidStyle::edge",
    "ui/presentation/rich/mermaid/primitives/paint.rs::MermaidStyle::label_bg",
    "ui/presentation/rich/mermaid/primitives/paint.rs::MermaidStyle::node_fill",
    "ui/presentation/rich/mermaid/primitives/paint.rs::MermaidStyle::node_stroke",
    "ui/presentation/rich/mermaid/primitives/paint.rs::MermaidStyle::text",
    "ui/presentation/theme/builtin.rs::Theme::accent",
    "ui/presentation/theme/builtin.rs::Theme::accent_soft",
    "ui/presentation/theme/builtin.rs::Theme::bg_primary",
    "ui/presentation/theme/builtin.rs::Theme::bg_secondary",
    "ui/presentation/theme/builtin.rs::Theme::bg_tertiary",
    "ui/presentation/theme/builtin.rs::Theme::border",
    "ui/presentation/theme/builtin.rs::Theme::code_bg",
    "ui/presentation/theme/builtin.rs::Theme::current_line",
    "ui/presentation/theme/builtin.rs::Theme::diff_add",
    "ui/presentation/theme/builtin.rs::Theme::diff_add_bg",
    "ui/presentation/theme/builtin.rs::Theme::diff_del",
    "ui/presentation/theme/builtin.rs::Theme::diff_del_bg",
    "ui/presentation/theme/builtin.rs::Theme::editor_font_family",
    "ui/presentation/theme/builtin.rs::Theme::editor_font_size",
    "ui/presentation/theme/builtin.rs::Theme::editor_line_height",
    "ui/presentation/theme/builtin.rs::Theme::error",
    "ui/presentation/theme/builtin.rs::Theme::find_bg",
    "ui/presentation/theme/builtin.rs::Theme::find_current_bg",
    "ui/presentation/theme/builtin.rs::Theme::kind",
    "ui/presentation/theme/builtin.rs::Theme::metrics",
    "ui/presentation/theme/builtin.rs::Theme::selection_bg",
    "ui/presentation/theme/builtin.rs::Theme::success",
    "ui/presentation/theme/builtin.rs::Theme::syn_bold",
    "ui/presentation/theme/builtin.rs::Theme::syn_code",
    "ui/presentation/theme/builtin.rs::Theme::syn_heading",
    "ui/presentation/theme/builtin.rs::Theme::syn_italic",
    "ui/presentation/theme/builtin.rs::Theme::syn_link",
    "ui/presentation/theme/builtin.rs::Theme::syn_marker",
    "ui/presentation/theme/builtin.rs::Theme::syn_meta",
    "ui/presentation/theme/builtin.rs::Theme::syn_other",
    "ui/presentation/theme/builtin.rs::Theme::syn_quote",
    "ui/presentation/theme/builtin.rs::Theme::syn_table",
    "ui/presentation/theme/builtin.rs::Theme::text_primary",
    "ui/presentation/theme/builtin.rs::Theme::text_secondary",
    "ui/presentation/theme/builtin.rs::Theme::warning",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::divider_width",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::editor_toolbar_height",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::pane_padding",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::preview_font_size",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::preview_line_height",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::radius_button",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::radius_card",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::radius_dialog",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::sidebar_width",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::space_lg",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::space_md",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::space_sm",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::space_xl",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::space_xs",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::space_xxl",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::space_xxs",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::statusbar_height",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::tabbar_height",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::tabstrip_height",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::topbar_height",
    "ui/presentation/theme/builtin/metrics.rs::Metrics::ui_font_size",
    "ui/presentation/theme/mapper.rs::StyleSemantics::background",
    "ui/presentation/theme/mapper.rs::StyleSemantics::color",
    "ui/presentation/theme/mapper.rs::StyleSemantics::italics",
    "ui/presentation/theme/mapper.rs::StyleSemantics::monospace",
    "ui/presentation/theme/mapper.rs::StyleSemantics::size_scale",
    "ui/presentation/theme/mapper.rs::StyleSemantics::strikethrough",
    "ui/presentation/theme/mapper.rs::StyleSemantics::underline",
    "ui/presentation/theme/mapper/galley_cache.rs::HighlightStats::cached_lines",
    "ui/presentation/theme/mapper/galley_cache.rs::HighlightStats::patched",
    "ui/presentation/theme/mapper/galley_cache.rs::HighlightStats::reshaped_lines",
    "ui/presentation/theme/mapper/galley_cache.rs::HighlightStats::total_lines",
    "ui/presentation/theme/mapper/span_job.rs::SpanHit::c0",
    "ui/presentation/theme/mapper/span_job.rs::SpanHit::c1",
    "ui/presentation/theme/mapper/span_job.rs::SpanHit::idx",
    "ui/presentation/theme/mapper/squiggle.rs::FindMark::current",
    "ui/presentation/theme/mapper/squiggle.rs::FindMark::end",
    "ui/presentation/theme/mapper/squiggle.rs::FindMark::start",
    "ui/presentation/theme/mapper/squiggle.rs::SpellMark::end",
    "ui/presentation/theme/mapper/squiggle.rs::SpellMark::start",
    "ui/presentation/theme/mapper/squiggle.rs::Squiggle::phase",
    "ui/presentation/theme/mapper/squiggle.rs::Squiggle::rect",
    "shared/dialog.rs::FileFilter::extensions",
    "shared/fonts.rs::EditorFontChoice::display_name",
    "shared/fonts.rs::EditorFontChoice::key",
    "shared/image_asset.rs::ImportedImage::link",
    "ui/presentation/shortcuts/registry/keys.rs::Modifiers::alt",
    "ui/presentation/shortcuts/registry/keys.rs::Modifiers::ctrl",
    "ui/presentation/shortcuts/registry/keys.rs::Modifiers::meta",
    "ui/presentation/shortcuts/registry/keys.rs::Modifiers::shift",
    "ui/presentation/shortcuts/registry/palette.rs::ActionEntry::action",
    "ui/presentation/shortcuts/registry/palette.rs::ActionEntry::combo",
    "ui/presentation/shortcuts/registry/palette.rs::ActionEntry::group",
    "ui/presentation/shortcuts/registry/palette.rs::ActionEntry::label",
    "ui/presentation/shortcuts/registry/palette.rs::ScoredAction::entry",
    "ui/presentation/shortcuts/registry/palette.rs::ScoredAction::hits",
    "ui/presentation/state/app_ui_state.rs::AppUiState::active_tab",
    "ui/presentation/state/app_ui_state.rs::AppUiState::dispatcher",
    "ui/presentation/state/app_ui_state.rs::AppUiState::focus_mode",
    "ui/presentation/state/app_ui_state.rs::AppUiState::preview_visible",
    "ui/presentation/state/app_ui_state.rs::AppUiState::sidebar_dragging",
    "ui/presentation/state/app_ui_state.rs::AppUiState::sidebar_tab",
    "ui/presentation/state/app_ui_state.rs::AppUiState::sidebar_visible",
    "ui/presentation/state/app_ui_state.rs::AppUiState::sidebar_width",
    "ui/presentation/state/app_ui_state.rs::AppUiState::split_dragging",
    "ui/presentation/state/app_ui_state.rs::AppUiState::split_ratio",
    "ui/presentation/state/app_ui_state.rs::AppUiState::status_bar_visible",
    "ui/presentation/state/app_ui_state.rs::AppUiState::sync",
    "ui/presentation/state/app_ui_state.rs::AppUiState::tab_count",
    "ui/presentation/state/app_ui_state.rs::AppUiState::tabstrip_last_active",
    "ui/presentation/state/app_ui_state.rs::AppUiState::tabstrip_scroll_x",
    "ui/presentation/state/app_ui_state.rs::AppUiState::theme",
    "ui/presentation/state/command_palette_ui_state.rs::CommandPaletteUiState::query",
    "ui/presentation/state/command_palette_ui_state.rs::CommandPaletteUiState::selected",
    "ui/presentation/state/command_palette_ui_state.rs::CommandRow::entry",
    "ui/presentation/state/command_palette_ui_state.rs::CommandRow::hits",
    "ui/presentation/state/diff_ui_state.rs::DiffUiState::degraded",
    "ui/presentation/state/diff_ui_state.rs::DiffUiState::error",
    "ui/presentation/state/diff_ui_state.rs::DiffUiState::left_title",
    "ui/presentation/state/diff_ui_state.rs::DiffUiState::only_changes",
    "ui/presentation/state/diff_ui_state.rs::DiffUiState::right_path",
    "ui/presentation/state/diff_ui_state.rs::DiffUiState::right_title",
    "ui/presentation/state/diff_ui_state.rs::DiffUiState::rows",
    "ui/presentation/state/diff_ui_state.rs::DiffUiState::stat",
    "ui/presentation/state/diff_ui_state/model.rs::CharSeg::kind",
    "ui/presentation/state/diff_ui_state/model.rs::CharSeg::text",
    "ui/presentation/state/diff_ui_state/model.rs::DiffStat::added",
    "ui/presentation/state/diff_ui_state/model.rs::DiffStat::removed",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::anchor",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::composing",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::cursor",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::line_numbers",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::load_remote_images",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::scroll_y",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::spell_check",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::spell_menu",
    "ui/presentation/state/editor_ui_state.rs::EditorUiState::word_wrap",
    "ui/presentation/state/editor_ui_state.rs::SpellSuggestMenu::byte_end",
    "ui/presentation/state/editor_ui_state.rs::SpellSuggestMenu::byte_start",
    "ui/presentation/state/editor_ui_state.rs::SpellSuggestMenu::suggestions",
    "ui/presentation/state/editor_ui_state.rs::SpellSuggestMenu::word",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::awaiting_capture",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::embed_images",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::error",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::file_stem",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::format",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::include_toc",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::long_image",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::notice",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::output_dir",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::page_size",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::paginate",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::resolved_path",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::running",
    "ui/presentation/state/export_ui_state.rs::ExportUiState::theme",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::current",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::error",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::find_focused",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::find_text",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::finder",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::keep_query_across_documents",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::last_replace_count",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::last_replace_grows",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::options",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::replace_open",
    "ui/presentation/state/find_replace_ui_state.rs::FindReplaceUiState::replace_text",
    "ui/presentation/state/preview_ui_state.rs::PreviewUiState::blocks",
    "ui/presentation/state/preview_ui_state.rs::PreviewUiState::degraded",
    "ui/presentation/state/preview_ui_state.rs::PreviewUiState::last_doc",
    "ui/presentation/state/preview_ui_state.rs::PreviewUiState::last_key",
    "ui/presentation/state/preview_ui_state.rs::PreviewUiState::rects",
    "ui/presentation/state/preview_ui_state.rs::PreviewUiState::render_count",
    "ui/presentation/state/preview_ui_state.rs::PreviewUiState::scroll_y",
    "ui/presentation/state/quick_open_ui_state.rs::QuickOpenItem::group",
    "ui/presentation/state/quick_open_ui_state.rs::QuickOpenItem::name",
    "ui/presentation/state/quick_open_ui_state.rs::QuickOpenItem::path",
    "ui/presentation/state/quick_open_ui_state.rs::QuickOpenUiState::query",
    "ui/presentation/state/quick_open_ui_state.rs::QuickOpenUiState::selected",
    "ui/presentation/state/scroll_sync_state.rs::BlockRect::bottom",
    "ui/presentation/state/scroll_sync_state.rs::BlockRect::id",
    "ui/presentation/state/scroll_sync_state.rs::BlockRect::top",
    "ui/presentation/state/scroll_sync_state.rs::PaneGeometry::content_h",
    "ui/presentation/state/scroll_sync_state.rs::PaneGeometry::viewport_h",
    "ui/presentation/state/scroll_sync_state.rs::ScrollSync::drive_count",
    "ui/presentation/state/scroll_sync_state.rs::ScrollSync::enabled",
    "ui/presentation/state/scroll_sync_state.rs::ScrollSync::last_driven",
    "ui/presentation/state/settings_ui_state.rs::PendingConflict::combo",
    "ui/presentation/state/settings_ui_state.rs::PendingConflict::with_combo",
    "ui/presentation/state/settings_ui_state.rs::PendingConflict::with_key",
    "ui/presentation/state/settings_ui_state.rs::PendingConflict::with_label",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::dirty",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::error",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::notice",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::pending_conflict",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::recording",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::section",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::shortcut_rollback",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::snapshot",
    "ui/presentation/state/settings_ui_state.rs::SettingsUiState::writing_goal_text",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::auto_save_interval",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::font_family",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::font_size",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::line_height",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::lint",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::load_remote_images",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::max_open_documents",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::show_line_numbers",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::spell_check",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::tab_size",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::theme_kind",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::word_wrap",
    "ui/presentation/state/settings_ui_state.rs::Snapshot::writing_goal",
    "ui/presentation/state/shortcut_overview_ui_state.rs::ShortcutOverviewUiState::query",
    "ui/presentation/state/shortcut_overview_ui_state.rs::ShortcutRow::action",
    "ui/presentation/state/shortcut_overview_ui_state.rs::ShortcutRow::combo",
    "ui/presentation/state/shortcut_overview_ui_state.rs::ShortcutRow::label",
    "ui/presentation/state/shortcut_overview_ui_state.rs::ShortcutSection::group",
    "ui/presentation/state/shortcut_overview_ui_state.rs::ShortcutSection::rows",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::context_menu_path",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::context_menu_pos",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::creating",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::error",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::expanded_dirs",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::notice",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::outline_max_level",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::outline_selected",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::rename_draft",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::renaming_path",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::search_options",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::search_query",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::search_scope",
    "ui/presentation/state/sidebar_ui_state.rs::SidebarUiState::selected_path",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchGroup::file_name",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchGroup::hits",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchGroup::path",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchHitRow::highlight",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchHitRow::line",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchHitRow::snippet",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchOptions::case_sensitive",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchOptions::regex",
    "ui/presentation/state/sidebar_ui_state/search.rs::SearchOptions::whole_word",
    "ui/presentation/state/sidebar_ui_state/tree.rs::TreeRow::depth",
    "ui/presentation/state/sidebar_ui_state/tree.rs::TreeRow::is_directory",
    "ui/presentation/state/sidebar_ui_state/tree.rs::TreeRow::is_last",
    "ui/presentation/state/sidebar_ui_state/tree.rs::TreeRow::name",
    "ui/presentation/state/sidebar_ui_state/tree.rs::TreeRow::path",
    "ui/presentation/state/status_bar_ui_state.rs::StatusBarUiState::encoding",
    "ui/presentation/state/status_bar_ui_state.rs::StatusBarUiState::goal",
    "ui/presentation/state/status_bar_ui_state.rs::StatusBarUiState::message",
    "ui/presentation/state/status_bar_ui_state.rs::StatusMessage::expires_at",
    "ui/presentation/state/status_bar_ui_state.rs::StatusMessage::is_error",
    "ui/presentation/state/status_bar_ui_state.rs::StatusMessage::text",
    "ui/presentation/state/symbol_ui_state.rs::SymbolUiState::group",
    "ui/presentation/state/symbol_ui_state.rs::SymbolUiState::query",
    "ui/presentation/state/symbol_ui_state.rs::SymbolUiState::selected",
    "ui/presentation/state/symbol_ui_state/catalog.rs::SymbolItem::glyph",
    "ui/presentation/state/symbol_ui_state/catalog.rs::SymbolItem::label",
    "ui/presentation/state/symbol_ui_state/catalog.rs::SymbolItem::text",
];

/// 递归收集 `.rs` 文件。
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// 解析 `lex_dump` 的一行 `12:Ident(app)` → `(行号, kind, text)`。
///
/// `allow@…`（豁免指令行）不是 token，跳过——它是词法的副产物，
/// 混进 token 流会让窗口匹配错位。
fn parse_dump_line(line: &str) -> Option<(usize, String, String)> {
    if line.starts_with("allow@") {
        return None;
    }
    let (ln, rest) = line.split_once(':')?;
    let ln: usize = ln.parse().ok()?;
    let (kind, tail) = rest.split_once('(')?;
    let text = tail.strip_suffix(')')?;
    Some((ln, kind.to_string(), text.to_string()))
}

/// `pub` 之后可直接出现的**项**关键字（`pub struct` / `pub fn` / …）。
///
/// 排除它们不是关键词黑名单，而是语法事实：字段名**不可能**是 Rust 关键字。
/// 宏体内 `pub struct $name(u64)` 的 `$` 归 `Other`，与 `:` 同类——
/// 没有这层排除，`pub struct $…` 会被读成「名叫 `struct` 的字段」。
const AFTER_PUB_KEYWORDS: &[&str] = &[
    "struct",
    "enum",
    "union",
    "fn",
    "const",
    "static",
    "type",
    "mod",
    "use",
    "trait",
    "unsafe",
    "extern",
    "async",
    "impl",
    "macro_rules",
];

/// 从一份源码里收集全可见性（`pub`）字段 → `<结构体>::<字段>`（按出现顺序）。
///
/// 三种形态（词法级，注释与字符串不产生 token）：
///
/// 1. **受限可见性**：`pub(crate)` / `pub(super)` / `pub(in …)` / `pub(self)`
///    → 直接跳过，不是 `pub` 面；
/// 2. **元组结构体字段**：`struct X(pub T);`——`struct` 关键字与字段体之间
///    （`pending` 状态）出现的 `pub` 只能是元组字段，记 `X::tuple.N`；
/// 3. **命名字段**：`pub` + 名字 + `:`（`:` 归 `Other` token）——归属到当前
///    花括号跟踪的结构体体。
///
/// 为什么 `pub const` / `pub fn` / `pub struct` 不会误收：它们在 `pub` 后
/// 是**关键字**再跟名字，即 `pub` + Ident + Ident，不满足「`pub` + Ident +
/// `Other`」的字段形状（`:` 才是 `Other`）。
fn pub_fields_in(source: &str) -> Vec<String> {
    let dump = arch_check::lex_dump(source);
    let toks: Vec<(usize, String, String)> = dump.lines().filter_map(parse_dump_line).collect();
    let mut out = Vec::new();
    let mut depth: usize = 0usize;
    // 刚见 `struct X`、体还没开始：名字挂起中（也覆盖元组体 `struct X(…)`）。
    let mut pending: Option<String> = None;
    // 当前所在的结构体**花括号体**：(体的开括号深度, 名字)。
    let mut current: Option<(usize, String)> = None;
    // 当前元组体内的字段序号（`struct Tup(pub u8, pub u16)` → tuple.0 / 1）。
    let mut tuple_seq: usize = 0;

    for (i, (_, kind, text)) in toks.iter().enumerate() {
        match kind.as_str() {
            // `struct` 后的第一个 Ident 是结构体名（generics / where 子句
            // 只会在名字之后出现，不会插进来第二个 Ident）。
            "Ident" if text == "struct" => {
                if let Some((_, name_kind, name_text)) = toks.get(i + 1)
                    && name_kind == "Ident"
                {
                    pending = Some(name_text.clone());
                    tuple_seq = 0;
                }
            }
            "LBrace" => {
                depth += 1;
                if let Some(name) = pending.take() {
                    current = Some((depth, name));
                }
            }
            "RBrace" => {
                // 只在结构体体自己的深度上收口：字段类型 / 表达式里的
                // 内层花括号先减到，不该提前清掉 current。
                if matches!(&current, Some((d, _)) if *d == depth) {
                    current = None;
                }
                depth = depth.saturating_sub(1);
            }
            // 单位结构体 / 元组结构体的收尾：挂起状态必须在这里清掉，
            // 否则后面顶层的 `pub use` 等会被当成下一个元组字段。
            "Semi" => pending = None,
            "Ident" if text == "pub" => {
                let Some((_, next_kind, next_text)) = toks.get(i + 1) else {
                    continue;
                };
                // 形态 1：受限可见性（可见性括号内是关键字，不是类型）。
                if next_kind == "LParen"
                    && let Some((_, inner_kind, inner_text)) = toks.get(i + 2)
                    && inner_kind == "Ident"
                    && matches!(inner_text.as_str(), "crate" | "super" | "in" | "self")
                {
                    continue;
                }
                // 形态 2：元组结构体字段——`struct X` 的体还没吃 `;`，
                // 这里的 `pub` 只能是元组字段的可见性（generics / where
                // 子句里不可能出现 `pub`；受限可见性已被形态 1 跳过）。
                if let Some(name) = &pending {
                    out.push(format!("{name}::tuple.{tuple_seq}"));
                    tuple_seq += 1;
                    continue;
                }
                // 形态 3：命名字段 `pub name:`（i+2 必须是 `Other`——
                // `:` 归 Other；`pub struct` / `pub fn` 等在这里是
                // Ident + Ident，不满足）。关键字排除见 AFTER_PUB_KEYWORDS。
                if next_kind == "Ident"
                    && !AFTER_PUB_KEYWORDS.contains(&next_text.as_str())
                    && let Some((_, after_kind, _)) = toks.get(i + 2)
                    && after_kind == "Other"
                {
                    let owner = current
                        .as_ref()
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| "<unattributed>".to_string());
                    out.push(format!("{owner}::{next_text}"));
                }
            }
            _ => {}
        }
    }
    out
}

/// 扫描 core 与抽出的叶子 crate 的全部 `.rs`，返回 `<键>::<结构体>::<字段>`
/// 集合与未能归属到任何结构体体的命中（正常应为空，非空 = 匹配逻辑有洞）。
///
/// 键前缀见 [`APPROVED_PUB_FIELDS`]：core / shared / infra 为 `""`（历史口径，
/// 559 条不动；后两者保留 `shared/` `infrastructure/` 子目录，裸相对路径恰好
/// 复现搬迁前的 core 键），icons / ui 为 crate 短名 + `/`（防 `lib.rs` 同键碰撞）。
fn scan_pub_fields() -> (BTreeSet<String>, Vec<String>) {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let roots: [(&str, PathBuf); 5] = [
        ("", base.join("crates").join("rustmark-core").join("src")),
        (
            "icons/",
            base.join("crates").join("rustmark-icons").join("src"),
        ),
        ("", base.join("crates").join("rustmark-shared").join("src")),
        ("", base.join("crates").join("rustmark-infra").join("src")),
        ("ui/", base.join("crates").join("rustmark-ui").join("src")),
    ];

    let mut actual: BTreeSet<String> = BTreeSet::new();
    let mut unattributed = Vec::new();
    for (prefix, root) in &roots {
        let mut files = Vec::new();
        collect_rs(root, &mut files);
        files.sort();
        assert!(!files.is_empty(), "{root:?} 下未找到任何 .rs 文件");

        for path in &files {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(source) = std::fs::read_to_string(path) else {
                continue;
            };
            for entry in pub_fields_in(&source) {
                match entry.strip_prefix("<unattributed>::") {
                    Some(field) => unattributed.push(format!("{prefix}{rel}::{field}")),
                    None => {
                        actual.insert(format!("{prefix}{rel}::{entry}"));
                    }
                }
            }
        }
    }
    (actual, unattributed)
}

#[test]
fn pub_fields_match_the_whitelist() {
    let (actual, unattributed) = scan_pub_fields();
    assert!(
        unattributed.is_empty(),
        "有 `pub` 字段未能归属到任何结构体体——匹配逻辑出现新形状，\
         先修 pub_fields_in 再谈清单：{unattributed:?}"
    );

    let expected: BTreeSet<String> = APPROVED_PUB_FIELDS.iter().map(|s| s.to_string()).collect();
    let missing: Vec<_> = actual.difference(&expected).collect();
    let stale: Vec<_> = expected.difference(&actual).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "core 与抽出叶子 crate 的字段级 pub 白名单与实际不一致（双向断言：现实与清单强制同步）。\n\
         \n\
         未登记的 `pub` 字段（接口面增长或 pub(crate) 被放宽回 pub）：\
         若确属 crate 外有合法读点/构造点，先拿编译器报点作证据，再登记到 \
         APPROVED_PUB_FIELDS（改本清单 = 评审必经）：{missing:?}\n\
         \n\
         清单中的过期项（字段已收紧为 pub(crate) 或已删除，清单须同步移除）：{stale:?}\n\
         \n\
         当前实际存量：{actual:?}"
    );
}

/// 匹配逻辑自身的**注入用例**（与阶段 2 / `arch_violation_fixture` 同思路）：
///
/// 真实存量扫描只验证「现状与清单相等」——若 `pub_fields_in` 本身写错了
/// （把注释里的示例收进来、把 `pub fn` 当字段、漏掉元组字段），
/// 现状扫描照样「通过」。判据的正确性只能靠注入现形。
mod pub_fields_in_injection {
    use super::pub_fields_in;

    /// 必收：命名字段、元组字段；必不收：注释 / 字符串里的同形示例、
    /// `pub(crate)` 与 `pub(in …)` 受限可见性、`pub fn` / `pub const` /
    /// `pub static` 等非字段项。
    #[test]
    fn collects_pub_fields_and_ignores_non_field_shapes() {
        let src = r#"
            /// 文档示例：`pub doc_fake: u64`
            // 行注释示例：pub comment_fake: u64
            let s = "pub string_fake: u64";
            pub struct Demo {
                pub real: u64,
                pub(crate) crate_only: u64,
                pub(in crate::presentation) scoped: u64,
                /// 字段文档里的 `pub doc_field_fake: u64`
                pub documented: String,
            }
            pub struct Tup(pub u8, pub(crate) u16);
            pub fn not_a_field(x: u32) -> u32 { x }
            pub const NOT_A_FIELD: u32 = 1;
            pub static lower_static: u32 = 2;
        "#;
        assert_eq!(
            pub_fields_in(src),
            vec!["Demo::real", "Demo::documented", "Tup::tuple.0",],
            "只收全可见性字段：注释/字符串剔除、受限可见性跳过、非字段 pub 不误收"
        );
    }

    /// 结构体归属跨 impl / 多结构体不串：`impl` 的花括号不是结构体体，
    /// `pub fn` 也不该被记成字段。
    #[test]
    fn attributes_fields_to_their_own_struct_across_impls() {
        let src = r#"
            struct Alpha { pub x: u8 }
            impl Alpha { pub fn get(&self) -> u8 { self.x } }
            struct Beta { pub y: u8 }
            pub struct Gamma { pub z: u8 }
        "#;
        assert_eq!(
            pub_fields_in(src),
            vec!["Alpha::x", "Beta::y", "Gamma::z"],
            "字段必须归属声明它的结构体；impl 体内的 pub fn 不入清单"
        );
    }

    /// 宏体里的 `$` 归 `Other`（与 `:` 同形）：`pub struct $name(u64)` 若按
    /// 「`pub` + Ident + `Other`」直读会变成「名叫 `struct` 的字段」。
    /// 关键字排除（字段名不可能是关键字）必须把这类项形态全部挡掉。
    #[test]
    fn skips_item_keywords_glued_by_macro_metavariables() {
        let src = r#"
            macro_rules! declare_id {
                ($name:ident) => {
                    pub struct $name(u64);
                    pub const $name: u32 = 1;
                    pub fn $name() {}
                };
            }
        "#;
        assert!(
            pub_fields_in(src).is_empty(),
            "宏体内的项声明不该被读成字段：{:?}",
            pub_fields_in(src)
        );
    }
}
