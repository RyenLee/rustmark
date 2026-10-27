//! 视图窄接口 [`EditorViewContext`]（R-03 B1，2026-10-27 开工）。
//!
//! # 它解决的是什么问题
//!
//! R-03 B2 之前，`src/presentation/views/` 下 55 个视图文件里有一批一律用
//! `use crate::app::frame::EditorApp`，把**整个应用根状态**当参数——
//! 视图因此既不能脱离组合根编译，也无法独立单测，而 root crate 被
//! 14k 行视图撑成了仓库最大的一块。B1 要把这条依赖剪断。
//!
//! # 为什么是 `trait` 而不是「一堆字段引用的结构体」
//!
//! 直觉写法是 `struct EditorViewContext<'a> { dialog: &'a mut DialogUiState, ... }`，
//! 逐个字段借用。**这条路在 Rust 里走不通**：视图不仅要读写状态，还要调命令
//! （`current_stats` / `insert_at_cursor` / `open_document_list` …），
//! 而这些命令的实现在 `EditorApp` 上，需要**同一个** `&mut EditorApp`。
//! 于是调用点会同时出现「`&mut self.dialog`」与「`&mut self`」——借用检查器直接拒绝。
//!
//! trait 方案下视图只拿**一个** `&mut dyn EditorViewContext`：状态访问与命令
//! 都是它的方法，天然只有一个独占借用，没有拆分借用的矛盾。
//!
//! # 形态：5 个能力 trait + 1 个 bundle
//!
//! [`EditorViewContext`] 本身**没有自有方法**，它只是把 5 个能力 trait
//! 捆成一张「视图能看到的全部能力」的网：
//!
//! | 能力 trait | 覆盖什么 | 批 1 方法数 |
//! |---|---|---|
//! | [`ThemeAccess`] | 主题 + 快捷键注册表 | 3 |
//! | [`UiStateAccess`] | UI 状态直取（字段级） | 15 |
//! | [`DocumentAccess`] | 文档 / 标签 / 只读查询 | 12 |
//! | [`CommandPort`] | 语义命令（含 `execute`） | 45 |
//! | [`ServiceQueries`] | 组合根服务查询 | 0（批 2 填充） |
//!
//! 为什么不摊平成一个大 trait：摊平后 ≈ `EditorApp`（183 方法 + 40 字段）的
//! 62%，正是「trait 上照抄 `EditorApp` 全部成员」的**假解耦**——接口面没变小，
//! 只是换了个名字。分组之后，「这个视图能碰到什么」一眼可辨，
//! 且 Rust 1.86 起的 **trait object upcasting**（1.99 已实测）让
//! `&mut dyn ThemeAccess` 能自动转成 `&mut dyn EditorViewContext`，
//! 将来再拆细粒度窄 trait 时**每个视图只改一行签名**。
//!
//! # 为什么放在 `rustmark-ui` 而不是 root
//!
//! **契约要独立于组合根**——这是唯一的理由。
//!
//! views 按 ADR-013 决策 3 **留 root**（`docs/architecture-constraints.md`
//! §10.1 硬约束 2：presentation 外移采用「views 留 root 的中间 crate」形态，
//! 否则 R-11/R-12 的「views ∈ root、state ∈ 另一 crate」跨 crate 字段保护
//! 会被降级成同 crate 内约束）。故「为将来搬迁铺路」这条理由**不成立**，
//! 别拿它当决策依据。
//!
//! 留 ui 的真实收益是**契约的位置**：[`EditorViewContext`] 描述的是
//! 「视图能对应用状态做什么」，而应用状态（`state/`）本来就住在 ui 这一侧。
//! trait 若留在 root，它引用的类型（`DialogUiState` / `SettingsUiState` / …）
//! 会横跨两个 crate，契约与实现彼此纠缠；放 ui 则 ui 能**自洽地**描述
//! 「我的状态 + 视图能对它们做什么」，root 只负责提供实现。
//!
//! # 端口里为什么收 `UiAction`
//!
//! 工具栏 / 菜单 / 命令面板走的是 `execute(UiAction)`。
//!
//! **本 crate 不新增 `ui → actions` 依赖边**——`crates/rustmark-ui/Cargo.toml`
//! 的依赖面由 `tests/workspace_topology_test.rs` 断言恰为
//! `egui + rustmark-core`。`UiAction` 经 `rustmark_core::actions`
//! （= `rustmark_actions::UiAction`）**已经可达**：
//! `rustmark-core/src/lib.rs` 有
//! `pub mod actions { pub use rustmark_actions::{MenuGroup, UiAction, action}; }`，
//! 而本 crate 的 `presentation/shortcuts/registry.rs` 已在 re-export 它。
//! 故 trait 里引用 `UiAction` **不产生**新的 crate 边。
//!
//! 收它的理由是**不变量**：菜单 / 快捷键 / 命令面板三处必须走**同一条**
//! 执行路径（`src/app/frame.rs`：「视图层只负责**画**按钮，点击后同样回到
//! `EditorApp::execute`」）。`execute` 若不进 trait，视图就只能在 root 侧
//! 另留一个 execute 分支——那等于**在视图里复制一份分派**，
//! 是这条不变量的头号反例。
//!
//! `UiAction` 是**零依赖契约 crate** 里的封闭词汇表，不可能携带实现细节，
//! 放进 trait 不构成泄露。
//!
//! # object-safety 三条硬约束
//!
//! trait 方法**不得含泛型参数**（含 APIT），否则 trait 失去 dyn-compatible：
//!
//! | `EditorApp` 原签名 | trait 里的合法形态 |
//! |---|---|
//! | `fn set_diff_error(&mut self, message: impl Into<String>)` | `fn set_diff_error(&mut self, message: String)` |
//! | `fn apply_theme_field(&mut self, mutate: impl FnOnce(&mut ThemeView))` | `fn apply_theme_field(&mut self, mutate: &mut dyn FnMut(&mut ThemeView))` |
//! | `fn apply_editor_field(&mut self, mutate: impl FnOnce(&mut EditorSettingsView))` | `fn apply_editor_field(&mut self, mutate: &mut dyn FnMut(&mut EditorSettingsView))` |
//!
//! # `_state` 后缀为什么不是啰嗦
//!
//! 访问器统一带 `_state`，是为了与 `EditorApp` 的同名 inherent 方法**永不撞名**。
//! 一旦撞名，`impl EditorViewContext for EditorApp` 里的
//! `fn theme(&self) -> &Theme { self.theme() }` 会按「inherent 优先」解析成
//! 调用自身之外的那个方法——今天侥幸正确，明天 inherent 一删就变成无限递归，
//! 编译器一句提示都没有。本项目已经为「字段 `settings` 与方法 `settings()`
//! 同名」付过一次排查成本（见 `EditorApp::settings_ui` 的字段注释），不再付第二次。
//!
//! # 增量契约
//!
//! **只加当前批次视图真正用到的成员**，不预先铺满。成员表随批次增长，
//! 每加一个都必须有视图在用——否则它只是把 `EditorApp` 的上帝面
//! 原样复制到这个 trait 上，等于没解耦。

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
use rustmark_core::application::dto::{
    EditorSettingsView, ExportRequestView, ExportResultView, LintIssueView, OutlineItemView,
    RecentFileView, SettingsView, StatsView, TabSummary, ThemeView,
};
use rustmark_core::shared::ids::DocumentId;
use rustmark_core::shared::kernel::export_format::PageSize;
use rustmark_core::shared::kernel::fonts::EditorFontChoice;
use rustmark_core::shared::result::AppResult;

// ===========================================================================
// T1 · ThemeAccess —— 主题与快捷键注册表
// ===========================================================================

/// 主题与快捷键注册表的只读访问。
pub trait ThemeAccess {
    /// 当前主题。
    fn theme_state(&self) -> &Theme;

    /// 生效的快捷键注册表（菜单 / 命令面板 / 一览三处同源）。
    fn dispatcher_state(&self) -> &ShortcutDispatcher;

    /// 当前主题的显示名（「浅色」「深色」「跟随系统」…）。
    fn theme_kind_label(&self) -> &'static str;
}

// ===========================================================================
// T2 · UiStateAccess —— UI 状态直取
// ===========================================================================

/// 表现层 UI 状态的直取（字段级访问，不夹带业务语义）。
pub trait UiStateAccess {
    /// 应用级 UI 状态（含快捷键分发器）。
    fn ui_state(&mut self) -> &mut AppUiState;

    /// 编辑器状态（光标 / 选区 / 滚动 / IME 组字）。
    fn editor_state(&mut self) -> &mut EditorUiState;

    /// 侧栏状态（文件树展开 / 搜索输入 / 大纲）。
    fn sidebar_state(&mut self) -> &mut SidebarUiState;

    /// 查找替换状态。
    fn find_state(&mut self) -> &mut FindReplaceUiState;

    /// 导出浮层状态。
    fn export_ui_state(&mut self) -> &mut ExportUiState;

    /// 设置浮层状态。
    fn settings_ui_state(&mut self) -> &mut SettingsUiState;

    /// 快速打开浮层状态。
    fn quick_open_ui_state(&mut self) -> &mut QuickOpenUiState;

    /// 命令面板状态。
    fn command_palette_ui_state(&mut self) -> &mut CommandPaletteUiState;

    /// 命令面板的查询词（只读）。
    ///
    /// [`Self::command_palette_ui_state`] 是 `&mut self` 方法，**纯读**的
    /// 过滤路径（命令面板按查询词过滤动作表）拿不到它；本视图的
    /// `rows()` 不改任何状态，故给一条 `&self` 只读出口——否则为了
    /// 读一个词就得把整个上下文可变地借走。
    fn command_palette_query(&self) -> &str;

    /// 差异对比浮层状态。
    fn diff_ui_state(&mut self) -> &mut DiffUiState;

    /// 浮层栈状态（打开 / 关闭 / 栈顶 / 首次打开取焦）。
    fn dialog_state(&mut self) -> &mut DialogUiState;

    /// 符号面板状态（分组 / 查询 / 选中项）。
    fn symbol_ui_state(&mut self) -> &mut SymbolUiState;

    /// 快捷键一览的过滤词与分组缓存。
    ///
    /// 只读：改过滤词走 [`CommandPort::set_shortcut_overview_query`] 这条命令，
    /// 不在视图里直接写字段——重置逻辑归命令所有，视图只负责画。
    fn shortcut_overview_ui_state(&self) -> &ShortcutOverviewUiState;

    /// 工作区文件树的**已装载条目**快照。
    fn workspace_entries_state(&self) -> &WorkspaceEntries;

    /// 编辑区文本副本（只读）。
    ///
    /// 它是领域内容的**视图副本**，不是领域真值——领域是单一真相，
    /// 每次领域命令后由组合根回读覆盖（见 `EditorApp::refresh_text`）。
    fn editor_text(&self) -> &str;
}

// ===========================================================================
// T3 · DocumentAccess —— 文档 / 标签 / 只读查询
// ===========================================================================

/// 文档、标签与其它**只读**的领域投影查询。
pub trait DocumentAccess {
    /// 已打开文档的摘要列表（标签栏顺序）。
    fn open_document_list(&self) -> Vec<TabSummary>;

    /// 工作区根路径；未打开工作区时为空串。
    fn workspace_root(&self) -> String;

    /// 当前文档的统计；无活动文档时为 `None`。
    ///
    /// 口径唯一：与状态栏取自同一个 `compute_stats`，视图**不得**自行重算。
    fn current_stats(&self) -> Option<StatsView>;

    /// 当前活动文档 ID；无活动文档时为 `None`。
    fn active_doc(&self) -> Option<DocumentId>;

    /// 当前文档的路径；无活动文档时为空串。
    fn current_document_path(&self) -> String;

    /// 当前文档的全文；无活动文档时为 `None`。
    fn current_content(&self) -> Option<String>;

    /// 最近文件列表（倒序，最新的在最上面）。
    fn recent_files(&self) -> Vec<RecentFileView>;

    /// 当前文档的大纲条目。
    fn current_outline(&self) -> Vec<OutlineItemView>;

    /// 当前文档的 lint 问题列表。
    fn lint_current_document(&self) -> Vec<LintIssueView>;

    /// 当前生效的设置视图（领域 DTO）。
    fn settings(&self) -> SettingsView;

    /// 系统当前是否偏好深色（「跟随系统」用）。
    fn system_prefers_dark(&self) -> bool;

    /// 设置页「字体」下拉的可选项。
    fn editor_font_choices(&self) -> &[EditorFontChoice];
}

// ===========================================================================
// T4 · CommandPort —— 语义命令
// ===========================================================================

/// 视图可以下达的**语义命令**（不含只读查询）。
///
/// 命令与查询分属两个 trait，是为了让「谁动了状态」在类型上可辨：
/// 只读方法全在 [`DocumentAccess`]，凡是能改状态的一律在这里。
pub trait CommandPort {
    // ---------- 动作总线 ----------

    /// 执行一个表现层动作（菜单 / 快捷键 / 命令面板三入口同源）。
    ///
    /// 视图**只画按钮**，点击后一律回到这里——这是「三入口一致」不变量的
    /// 物理保证：菜单里能干的，命令面板里也得能干。
    fn execute(&mut self, action: UiAction);

    /// 在光标处插入文本（进领域 → 成为一个撤销点 → 刷新预览）。
    fn insert_at_cursor(&mut self, text: &str) -> AppResult<()>;

    /// 设置快捷键一览的过滤词（含重置语义，见实现方）。
    fn set_shortcut_overview_query(&mut self, query: String);

    // ---------- 文件 / 工作区 ----------

    /// 新建一份空白文档。
    fn new_document(&mut self) -> AppResult<()>;

    /// 打开一个本机路径（文件或目录）。
    fn open_path(&mut self, path: &str) -> AppResult<()>;

    /// 打开工作区里的一个文件（走工作区管线：守卫 / 拒绝文案）。
    fn open_workspace_file(&mut self, path: &str) -> AppResult<()>;

    /// 删除工作区里的一个条目（目录 / 文件）。
    fn delete_entry(&mut self, path: &str) -> AppResult<()>;

    /// 重新扫描工作区，返回条目数。
    fn refresh_workspace(&mut self) -> usize;

    /// 该路径是否在「可按文本打开」的格式白名单内。
    fn can_open_as_text(&self, path: &str) -> bool;

    /// 报告「该格式不支持按文本打开」。
    fn report_unsupported_format(&mut self, path: &str);

    /// 读一个工作区文件的全文。
    fn read_workspace_file(&self, path: &str) -> AppResult<String>;

    /// 请求一次原生对话框（**在后台线程执行**）。
    fn request_dialog(&mut self, kind: PendingDialog);

    /// 在状态栏显示一条错误提示。
    fn set_status_error(&mut self, text: &str);

    // ---------- 标签 ----------

    /// 切换到指定文档。
    fn switch_document(&mut self, id: DocumentId);

    /// 关闭指定文档。
    fn close_document(&mut self, id: DocumentId);

    /// 关闭全部文档（先弹确认）。
    fn close_all_with_confirm(&mut self);

    /// 标签条的水平滚动位置。
    fn set_tabstrip_scroll_x(&mut self, x: f32);

    /// 标签条最后一个获得焦点的文档（`None` = 无焦点）。
    fn set_tabstrip_last_active(&mut self, id: Option<u64>);

    // ---------- 查找替换 ----------

    /// 把查找框内容同步进领域。
    fn sync_find(&mut self);

    /// 查找上一步 / 下一步。
    fn find_step(&mut self, forward: bool);

    /// 设置查找框的焦点状态。
    fn set_find_focused(&mut self, focused: bool);

    /// 替换当前命中。
    fn find_replace_current(&mut self);

    /// 替换全部命中。
    fn find_replace_all(&mut self);

    // ---------- 大纲 / 搜索 / 跳转 ----------

    /// 跳转到指定行。
    fn goto_outline_line(&mut self, line: usize);

    /// 跳转到指定字节偏移。
    fn goto_byte_offset(&mut self, offset: usize);

    /// 选中大纲里的某个标题（用于高亮定位）。
    fn select_outline_item(&mut self, text: String);

    /// 选中侧栏里的某个路径。
    fn select_sidebar_path(&mut self, path: String);

    /// 设置全局搜索的查询词。
    fn set_search_query(&mut self, query: String);

    /// 设置全局搜索的范围。
    fn set_search_scope(&mut self, scope: SearchScope);

    /// 切换一个搜索选项开关。
    fn toggle_search_option(&mut self, which: SearchToggle);

    /// 复位全部搜索选项开关。
    fn reset_search_options(&mut self);

    /// 执行一次工作区搜索，返回分组结果。
    fn search_workspace(&self) -> AppResult<WorkspaceSearch>;

    /// 回填「新建条目」的草稿（取消编辑后恢复显示）。
    fn restore_create_draft(&mut self, parent: String, name: String, is_directory: bool);

    /// 回填「重命名」的草稿。
    fn restore_rename_draft(&mut self, path: String, name: String);

    // ---------- 编辑开关 ----------

    /// 设置自动换行。
    fn set_word_wrap(&mut self, on: bool);

    /// 设置拼写检查开关。
    fn set_spell_check(&mut self, on: bool);

    /// 设置行号栏显示。
    fn set_line_numbers(&mut self, on: bool);

    /// 设置是否加载文档里的远程图片（PRIV-01）。
    fn set_load_remote_images(&mut self, on: bool);

    // ---------- 差异 ----------

    /// 显示一条差异错误。
    ///
    /// 原签名是 `impl Into<String>`——trait 方法不得含泛型参数（object-safety），
    /// 故这里收具体的 `String`。
    fn set_diff_error(&mut self, message: String);

    /// 清掉当前的差异选择。
    fn clear_diff_selection(&mut self);

    // ---------- 导出 ----------

    /// 设置「长图导出」。
    fn set_export_long_image(&mut self, long: bool);

    /// 设置「导出含目录」。
    fn set_export_include_toc(&mut self, on: bool);

    /// 设置「导出内嵌图片」。
    fn set_export_embed_images(&mut self, embed: bool);

    /// 设置导出纸张尺寸。
    fn set_export_page_size(&mut self, size: PageSize);

    /// 设置导出分页。
    fn set_export_paginate(&mut self, paginate: bool);

    /// 设置 PDF 是否含目录。
    fn set_pdf_include_toc(&mut self, toc: bool);

    /// 开始视口截图导出。
    fn begin_viewport_capture(&mut self, path: &str) -> AppResult<()>;

    /// 开始长图截图导出。
    fn begin_long_capture(&mut self, path: &str, pixels_per_point: f32) -> AppResult<()>;

    /// 执行导出。
    fn export_current_document(
        &mut self,
        request: ExportRequestView,
        path: &str,
    ) -> AppResult<ExportResultView>;

    // ---------- 设置 ----------

    /// 就地修改主题字段（一次调用内完成「读 → 改 → 写回」）。
    ///
    /// 原签名是 `impl FnOnce(&mut ThemeView)`——trait 方法不得含泛型参数
    /// （object-safety），故这里收 `&mut dyn FnMut(&mut ThemeView)`。
    fn apply_theme_field(&mut self, mutate: &mut dyn FnMut(&mut ThemeView));

    /// 就地修改编辑器设置字段。
    ///
    /// 同 [`Self::apply_theme_field`]：原签名 `impl FnOnce(&mut EditorSettingsView)`
    /// 不 dyn-compatible。
    fn apply_editor_field(&mut self, mutate: &mut dyn FnMut(&mut EditorSettingsView));

    /// 切换设置页分区。
    fn set_settings_section(&mut self, section: SettingsTab);

    /// 复位当前设置分区的未保存改动。
    fn reset_current_settings_section(&mut self);

    /// 复位全部快捷键绑定。
    fn reset_all_shortcuts(&mut self);

    /// 取消设置页的改动。
    fn cancel_settings(&mut self);

    /// 确认并保存设置页的改动。
    fn confirm_settings(&mut self);

    /// 清掉设置页的提示消息。
    fn clear_settings_messages(&mut self);

    /// 清掉设置页待处理的快捷键冲突。
    fn clear_settings_pending_conflict(&mut self);

    /// 重新绑定一个动作的键位。
    fn rebind_shortcut(&mut self, action: UiAction, combo: &str);
}

// ===========================================================================
// T5 · ServiceQueries —— 组合根服务查询
// ===========================================================================

/// 组合根**服务**的只读查询（`app.services.*` 的唯一出口）。
///
/// **批 1 为空**：批 1 的 24 个视图**零 `app.services` 引用**——
/// 边界正是按「视图是否需要 `services`」划的（见 R-03 B1 方案 §2.2）。
/// 批 2（编辑区 / 预览 / 滚动同步）才会往里加方法。
///
/// 保留这个空 trait 是为了让「组合根服务查询」这条能力在契约上**占好位**：
/// 将来它长出方法时，`EditorViewContext` 的 supertrait 列表不用改，
/// 视图签名也不用改。
pub trait ServiceQueries {}

// ===========================================================================
// bundle
// ===========================================================================

/// 视图可用的窄接口：只暴露「这个视图真正需要」的状态与命令。
///
/// 实现方是 root 的 `EditorApp`（见 `src/app/frame/view_context.rs`）。
/// 视图侧一律以 `&mut dyn EditorViewContext` 接收，**不得**再出现
/// `EditorApp`——由 `tools/arch_check.rs::check_view_decoupling` 机械拦截。
///
/// 本 trait **没有自有方法**：它是 5 个能力 trait 的 bundle，
/// 拆开是为了让「这个视图能碰到什么」在类型上可辨（见模块文档）。
pub trait EditorViewContext:
    ThemeAccess + UiStateAccess + DocumentAccess + CommandPort + ServiceQueries
{
}
