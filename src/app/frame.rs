//! `frame` 模块的 facade：「拿到 `Services` 之后怎么办」全部由本模块及其子模块回答。
//!
//! # 本模块的三条职责边界
//!
//! 1. **装配**在 [`super::bootstrap`] 完成，本模块只处理「拿到 `Services` 之后怎么办」。
//! 2. **动作执行**集中在 `actions` 子模块：`UiAction` → 具体用例调用。
//!    视图层（`presentation::views`）只负责**画**按钮，点击后同样回到
//!    [`EditorApp::execute`]，从而保证「菜单 / 快捷键 / 命令面板」三个入口
//!    **行为完全一致**（S6-07 验收）。
//! 3. **不做阻塞 IO**：原生对话框只登记意图（[`PendingDialog`]），
//!    真正的同步调用由后台线程执行。
//!
//! # 文件布局
//!
//! 本文件只剩 facade 该有的东西：子模块共享的导入前奏（各子模块经
//! `use super::*` 复用同一批 `use`）、子模块声明与再导出、字体适配器。
//!
//! - `state`：`EditorApp` 的字段表与构造函数——「应用根状态长什么样」；
//! - `r#loop`：帧循环、启动入口 [`run`] 与字体装配——「一帧做什么」；
//! - 其余子模块按职责分文件（动作执行、各视图写入口、截图、导出…）。

use eframe::egui;

use crate::app::bootstrap::Services;
use crate::app::dialog_runner::{DialogOutcome, DialogRequest, DialogRunner};
use rustmark_core::application::document::commands::{
    CloseCommand, FormatCommand, FormatKind, InsertTextCommand, NewDocumentCommand,
    OpenFileCommand, RedoCommand, ReplaceRangeCommand, RevertCommand, SaveCommand,
    SaveToPathCommand, UndoCommand,
};
use rustmark_core::application::document::queries::{
    GetContentQuery, GetDocumentQuery, GetStatsQuery, ListOpenDocumentsQuery,
};
use rustmark_core::application::dto::{
    EditorSettingsView, RecentFileView, SettingsView, StatsView, TabSummary, ThemeView,
};
use rustmark_core::application::dto::{
    ExportRequestView, ExportResultView, LintIssueView, OutlineItemView,
};
use rustmark_core::application::export::commands::{CreateExportJobCommand, RunExportCommand};
use rustmark_core::application::markdown::queries::{LintQuery, RebuildOutlineQuery};
use rustmark_core::application::preferences::commands::{
    ResetSectionCommand, SetEditorCommand, SetShortcutCommand, SetThemeCommand, SettingsSection,
};
use rustmark_core::application::preferences::queries::GetSettingsQuery;
use rustmark_core::application::workspace::commands::{
    CreateEntryCommand, DeleteEntryCommand, MoveEntryCommand, ScanTreeCommand, TouchRecentCommand,
};
use rustmark_core::application::workspace::queries::{ListRecentQuery, ReadFileQuery, SearchQuery};
use rustmark_core::application::workspace::service::TREE_MAX_DEPTH;
use rustmark_core::domain::export::errors::ExportError;
use rustmark_core::domain::preferences::value_objects::ActionId;
use rustmark_core::shared::encoding::TextEncoding;
use rustmark_core::shared::error::AppError;
use rustmark_core::shared::export_format::{ExportFormat, PageSize};
use rustmark_core::shared::fonts::EditorFontChoice;
use rustmark_core::shared::path::{FilePath, file_name_of};
use rustmark_core::shared::result::AppResult;
use rustmark_core::shared::text_format::{self, TextFormats};
use rustmark_core::shared::theme::ThemeKind;
use rustmark_core::shared::time::Timestamp;

use crate::presentation::shortcuts::dispatcher::{Dispatch, ShortcutDispatcher};
use crate::presentation::shortcuts::registry::{ShortcutRegistry, UiAction};
use crate::presentation::state::app_ui_state::{AppUiState, SPLIT_MAX, SPLIT_MIN, SidebarTab};
use crate::presentation::state::command_palette_ui_state::CommandPaletteUiState;
use crate::presentation::state::dialog_ui_state::{DialogUiState, Overlay};
use crate::presentation::state::diff_ui_state::DiffUiState;
use crate::presentation::state::editor_ui_state::EditorUiState;
use crate::presentation::state::export_ui_state::ExportUiState;
use crate::presentation::state::find_replace_ui_state::{
    FIND_INPUT_ID, FindReplaceUiState, Match, REPLACE_INPUT_ID,
};
use crate::presentation::state::preview_ui_state::PreviewUiState;
use crate::presentation::state::quick_open_ui_state::QuickOpenUiState;
use crate::presentation::state::scroll_sync_state::Side;
use crate::presentation::state::settings_ui_state::{SettingsTab, SettingsUiState, Snapshot};
use crate::presentation::state::shortcut_overview_ui_state::ShortcutOverviewUiState;
use crate::presentation::state::sidebar_ui_state::SidebarUiState;
use crate::presentation::state::sidebar_ui_state::{SearchScope, group_search_hits};
use crate::presentation::state::status_bar_ui_state::StatusBarUiState;
use crate::presentation::state::symbol_ui_state::SymbolUiState;
use crate::presentation::theme::builtin::{self, Theme};
use crate::presentation::theme::mapper;
use crate::presentation::views::editor_ops;
use crate::presentation::views::scroll_sync::{LineIndex, RevealIntent, TARGET_MARGIN};
// `floor_char_boundary` 的语义与「为何只下取整」见 `shared::geometry`；
// 程序化设值（大纲跳转、测试、`TextEdit` 回调）能给任意字节偏移，
// 而 `&str[a..b]` 遇到非字符边界会 panic，`panic = "abort"` 下即窗口消失。
use rustmark_core::shared::geometry::{ByteOffset, TextRange, floor_char_boundary};
use rustmark_core::shared::ids::{DocumentId, JobId};

// R-03 B1：这三个类型已迁入 `rustmark-ui`（纯数据 / 请求标签，不含组合根语义），
// 但**公开路径逐字不变**——本文件保留同名 re-export，
// 故 `rustmark::app::frame::PendingDialog` / `WorkspaceEntries` / `WorkspaceSearch`
// 对外仍是同一条路径（`tests/app/dialog_runner_test.rs` 等 6 处调用点零改动）。
pub use crate::presentation::state::pending_dialog::PendingDialog;
pub use crate::presentation::state::sidebar_ui_state::WorkspaceSearch;
pub use crate::presentation::state::workspace_entries::WorkspaceEntries;

// E-25 拆分后的路径转发：`EditorApp` 与 `run` 的原路径**保持不变**，
// 兄弟模块的 `use super::*` 与外部 `rustmark::app::frame::…` 都不必改。
// `IME_*` 是编辑区 IME 守卫窗口（`editor/ime.rs` 直接用），
// `STATS_DEBOUNCE_MS` 是状态栏统计防抖窗口（`editor.rs` 直接用）。
pub(crate) use capture::PendingCapture;
pub use r#loop::run;
pub(super) use r#loop::{IME_ARM_MS, IME_GUARD_MS};
pub use state::EditorApp;
pub(crate) use state::STATS_DEBOUNCE_MS;

// ————————————————————————————————————————————————————————————
// 子模块（facade 之外的实现）
// ————————————————————————————————————————————————————————————

mod actions;
mod capture;
mod dialog;
mod diff;
mod drag_drop;
mod editor;
mod export;
mod files;
mod find;
mod images;
mod r#loop;
mod outline;
mod preview;
mod recovery;
mod scheduler;
mod settings;
mod shortcut_overview;
mod sidebar;
mod state;
mod status_bar;
mod tabs;
mod ui;
mod view_context;
mod workspace;
pub(crate) use drag_drop::classify_dropped;
pub(crate) use scheduler::{Deadline, FramePhase, FrameScheduler};

/// V11-63 阶段 B：端口字体定义 → egui 字体定义的**唯一**适配器。
///
/// 基础设施层（`rustmark_core::infrastructure::fonts`）产出的是 egui 无关的
/// `PortFontDefinitions`；只有组合根知道 egui。本函数做这一次转换，故
/// 「基础设施的字体产物在哪里变成 egui 类型」在结构上只有这一处。
///
/// # 为什么用 `from_blob` 而不是 `from_owned`
///
/// `PortFontData::bytes` 是 `Arc<Vec<u8>>`，而 `FontData::from_blob` 接受
/// `Arc<dyn AsRef<[u8]> + Send + Sync>`——直接 unsize 就把同一份字节交给 egui，
/// 不必为 1.5 MB 的内置 CJK 子集再复制一份。端口类型用 `Arc` 包裹字节
/// 正是为了保住这条零拷贝路径。
///
/// # 为什么只转 `scale`
///
/// egui 0.37 的 `FontTweak` 有 9 个字段（hinting / hinting_target /
/// subpixel_binning / coords / thin_space_width / tab_size …），本项目只设置
/// `scale`（emoji 字体 0.81 / 0.90，与 epaint 内嵌时代原值一致）。其余取 egui
/// 默认值，与端口化之前的行为逐字段等价——端口化前那些字段也从未被赋值。
/// 理由与扩展方式见 `rustmark_core::infrastructure::fonts::port` 的模块文档。
pub fn to_egui_font_definitions(
    defs: crate::infrastructure::fonts::PortFontDefinitions,
) -> egui::FontDefinitions {
    use crate::infrastructure::fonts::PortFontFamily;

    let font_data = defs
        .font_data
        .into_iter()
        .map(|(key, data)| {
            let font = egui::FontData::from_blob(data.bytes, 0).tweak(egui::FontTweak {
                scale: data.scale,
                ..Default::default()
            });
            (key, std::sync::Arc::new(font))
        })
        .collect();

    let families = defs
        .families
        .into_iter()
        .map(|(family, keys)| {
            let family = match family {
                PortFontFamily::Proportional => egui::FontFamily::Proportional,
                PortFontFamily::Monospace => egui::FontFamily::Monospace,
                PortFontFamily::Name(name) => egui::FontFamily::Name(name.into()),
            };
            (family, keys)
        })
        .collect();

    egui::FontDefinitions {
        font_data,
        families,
    }
}

#[cfg(test)]
mod tests;
