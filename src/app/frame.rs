//! eframe 调度与启动期骨架：先显示窗口，非关键路径异步初始化（S6-01 / S8-03）。
//!
//! # 本文件的三条职责边界
//!
//! 1. **装配**在 [`super::bootstrap`] 完成，本文件只处理「拿到 `Services` 之后怎么办」。
//! 2. **动作执行**集中在本文件：`UiAction` → 具体用例调用。
//!    视图层（`presentation::views`）只负责**画**按钮，点击后同样回到
//!    [`EditorApp::execute`]，从而保证「菜单 / 快捷键 / 命令面板」三个入口
//!    **行为完全一致**（S6-07 验收）。
//! 3. **不做阻塞 IO**：原生对话框只登记意图（[`PendingDialog`]），
//!    真正的同步调用由后台线程执行。

use eframe::egui;

use crate::app::bootstrap::Services;
use crate::app::dialog_runner::{DialogOutcome, DialogRequest, DialogRunner};
use crate::application::document::commands::{
    CloseCommand, FormatCommand, FormatKind, InsertTextCommand, NewDocumentCommand,
    OpenFileCommand, RedoCommand, ReplaceRangeCommand, SaveCommand, SaveToPathCommand, UndoCommand,
};
use crate::application::document::queries::{
    GetContentQuery, GetDocumentQuery, GetStatsQuery, ListOpenDocumentsQuery,
};
use crate::application::dto::{
    DocumentView, EditorSettingsView, RecentFileView, SettingsView, StatsView, ThemeView,
};
use crate::application::dto::{
    ExportRequestView, ExportResultView, OutlineItemView, SearchResultView,
};
use crate::application::export::commands::{CreateExportJobCommand, RunExportCommand};
use crate::application::markdown::queries::RebuildOutlineQuery;
use crate::application::preferences::commands::{
    ResetSectionCommand, SetEditorCommand, SetShortcutCommand, SetThemeCommand, SettingsSection,
};
use crate::application::preferences::queries::GetSettingsQuery;
use crate::application::workspace::commands::{
    CreateEntryCommand, DeleteEntryCommand, MoveEntryCommand, ScanTreeCommand, TouchRecentCommand,
};
use crate::application::workspace::queries::{ListRecentQuery, ReadFileQuery, SearchQuery};
use crate::application::workspace::service::TREE_MAX_DEPTH;
use crate::domain::export::errors::ExportError;
use crate::domain::preferences::value_objects::ActionId;
use crate::shared::error::AppError;
use crate::shared::path::FilePath;
use crate::shared::result::AppResult;

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
use crate::presentation::state::settings_ui_state::{SettingsUiState, Snapshot};
use crate::presentation::state::shortcut_overview_ui_state::ShortcutOverviewUiState;
use crate::presentation::state::sidebar_ui_state::SidebarUiState;
use crate::presentation::state::sidebar_ui_state::{SearchGroup, group_hits, locate_in_snippet};
use crate::presentation::state::status_bar_ui_state::StatusBarUiState;
use crate::presentation::state::symbol_ui_state::SymbolUiState;
use crate::presentation::theme::builtin::{self, BuiltinTheme, Theme};
use crate::presentation::theme::mapper;
use crate::presentation::views::editor_ops;
// `floor_char_boundary` 的语义与「为何只下取整」见 `shared::geometry`；
// 程序化设值（大纲跳转、测试、`TextEdit` 回调）能给任意字节偏移，
// 而 `&str[a..b]` 遇到非字符边界会 panic，`panic = "abort"` 下即窗口消失。
use crate::shared::geometry::{ByteOffset, TextRange, floor_char_boundary};
use crate::shared::ids::{DocumentId, JobId};

/// 一次待执行的阻塞对话框请求。
///
/// **S6-11f 起本类型只作为请求的标签**，`PendingDialog::SaveFile`
/// 缺少「建议文件名」这个必需参数，故实际请求改用
/// [`crate::app::dialog_runner::DialogRequest`]。
/// 保留本枚举是因为 `PendingDialog` 已被侧栏 / 欢迎页 / 设置引用，
/// 而那三处传的都是 `OpenFile` / `PickFolder`（无需额外参数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingDialog {
    /// 打开文件对话框。
    OpenFile,
    /// 另存为对话框。
    ///
    /// **已弃用**：走这条会丢掉建议文件名，对话框只能给一个空名。
    /// 正确做法是 [`crate::app::dialog_runner::DialogRequest::SaveFile`]。
    SaveFile,
    /// 选择工作区目录。
    PickFolder,
}

/// 工作区文件树的**已装载条目**。
///
/// # 为什么不每帧调 `scan_tree`
///
/// 扫描是**磁盘 IO**（`read_dir` 递归 + 最多 5000 条）。每帧做一次，
/// 哪怕窗口最小化也在后台狂读磁盘，文件树还会因为「顺序随文件系统返回序变化」
/// 而不停抖动。故只在打开文件夹 / 文件操作之后显式扫一次，视图每帧只读这份快照。
///
/// 它是**投影**不是缓存的业务真值：磁盘一变就重扫，
/// 快照里没有任何领域对象（只有路径字符串与一个布尔）。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceEntries {
    /// 根目录路径（空串 = 未打开工作区）。
    pub root: Option<String>,
    /// 扁平条目 `(路径, 是否目录)`，已按「目录在前、同层按名称」排序。
    pub items: Vec<(String, bool)>,
}

impl WorkspaceEntries {
    /// 以根目录与条目构造。
    pub fn new(root: String, items: Vec<(String, bool)>) -> Self {
        Self {
            root: Some(root),
            items,
        }
    }

    /// 是否已装载（未打开工作区时为假）。
    pub fn is_loaded(&self) -> bool {
        self.root.is_some()
    }
}

/// 一次工作区搜索的结果（分组 + 是否被上限截断）。
///
/// 刻意**不**把「是否截断」藏进 `EditorApp` 的某个字段由搜索顺手改掉：
/// 那样「读结果」就有了隐藏副作用，而截断提示恰恰是**必须被看见**的信息
/// （用户看到 300 条会以为搜全了）。它和结果一起返回，界面不可能漏掉。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceSearch {
    /// 按文件分组的命中。
    pub groups: Vec<SearchGroup>,
    /// 是否因超过总命中上限（300）而被截断。
    pub truncated: bool,
}

impl WorkspaceSearch {
    /// 命中总数（各组之和）。
    pub fn total(&self) -> usize {
        self.groups.iter().map(|g| g.hits.len()).sum()
    }

    /// 是否没有任何命中。
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// 应用根状态：一个 `eframe::App` 实例持有的全部东西。
///
/// # 为什么 UI 状态与应用服务**平铺**在同一结构体里
///
/// 两者需要高频交叉读取（执行动作要知道当前选区，切主题要同步预览缓存）。
/// 再包一层「上下文」只会多一次间接寻址，却不带来任何隔离。
/// 真正的隔离不靠结构体嵌套，而靠 `check-arch` 的依赖禁令——
/// 表现层**拿不到** `domain` 与 `infrastructure` 的类型，
/// 物理上无法把业务真值塞进 UI 状态。
pub struct EditorApp {
    /// 用例服务（组合根装配产物）。
    pub services: Services,
    /// 应用级 UI 状态（含快捷键分发器）。
    pub ui: AppUiState,
    /// 编辑器状态。
    pub editor: EditorUiState,
    /// 预览状态。
    pub preview: PreviewUiState,
    /// 侧栏状态。
    pub sidebar: SidebarUiState,
    /// 浮层状态。
    pub dialog: DialogUiState,
    /// 查找替换状态。
    pub find: FindReplaceUiState,
    /// 导出对话框状态（S6-11）。
    pub export_ui: ExportUiState,
    /// 设置对话框状态（S6-11）。
    ///
    /// 字段名带 `_ui` 后缀是为了与 [`Self::settings`]（读设置 DTO 的方法）区分：
    /// 同名会让 `self.settings` 在一处是字段、另一处是方法，
    /// 编译错误只在用到的那一行出现，排查时看不出是哪个含义。
    pub settings_ui: SettingsUiState,
    /// 工作区文件树的已装载条目（S6-11；只在显式扫描时更新）。
    pub workspace_entries: WorkspaceEntries,
    /// 状态栏状态（S6-11：瞬态消息、编码、写作目标提示）。
    pub status_bar: StatusBarUiState,
    /// 快速打开浮层状态（S6-11f）。
    pub quick_open_ui: QuickOpenUiState,
    /// 快捷键一览浮层状态（S6-11f）。
    pub shortcut_overview_ui: ShortcutOverviewUiState,
    /// 符号 / Emoji 面板状态（S6-11f）。
    pub symbol_ui: SymbolUiState,
    /// 差异对比浮层状态（S6-11f）。
    pub diff_ui: DiffUiState,
    /// 命令面板状态（S6-12）。
    pub command_palette_ui: CommandPaletteUiState,
    /// 系统当前是否偏好深色（「跟随系统」用）。
    system_dark: bool,
    /// 原生对话框的后台执行器（S6-11f）。
    ///
    /// 此前只有一个 `pending_dialog: Option<PendingDialog>`——
    /// 登记了意图但**无人执行**，于是「打开文件」「打开文件夹」
    /// 都停在「记下了，没做」。改成持有执行器后，
    /// `request_dialog` 立刻起线程，`update` 每帧取回结果。
    pub dialogs: DialogRunner,
    /// 当前活动文档 ID（多标签时切换；`None` 表示尚未打开任何文档）。
    ///
    /// 这是**编辑区与领域之间唯一的中介**：领域是内容的单一真相，
    /// `editor_text` 只是它的一个视图副本，每次领域命令后从 `get_content` 回读。
    pub(crate) active_doc: Option<DocumentId>,
    /// 编辑区的可变文本副本（egui `TextEdit` 直接绑定它）。
    ///
    /// 它不是「另一份真值」——`refresh_text()` 每次命令后都用领域内容覆盖它，
    /// 保证二者永不漂移（双写不一致是原型 `editor.ts` 的致命缺陷 B-05）。
    pub(crate) editor_text: String,
    /// 领域编辑后需要把 egui 光标重设到的**字符**位置 `(cursor, anchor)`。
    ///
    /// egui 的 `TextEdit` 在内部记一套光标状态；领域命令改了文本后，
    /// 必须显式把这个位置写回 egui，否则光标会停在错误的偏移（多字节边界尤其明显）。
    pending_cursor: Option<(usize, usize)>,
    /// 上一次领域编辑后需要清空 egui 内置撤销栈。
    ///
    /// 理由：egui 的 `TextEdit` 自带撤销栈并会响应 Ctrl+Z；若不清除，
    /// 就会与**领域**撤销栈（本项目的撤销权威）打架。每次通过领域改了文本后，
    /// 下一帧开头清掉 egui 的撤销栈，让 Ctrl+Z 永远只走领域。
    clear_undoer_next: bool,
    /// 启动时发现、待用户处置的遗留草稿 `(文档 ID, 内容)`（DEF-02 恢复）。
    ///
    /// 非空时每帧弹出一个居中的恢复提示；用户选择「恢复全部」或「丢弃全部」
    /// 后才清空。空列表则完全不渲染，不影响正常编辑。
    pending_recoveries: Vec<(DocumentId, String)>,
    /// 本帧预览区在窗口中的矩形（**逻辑点**；布局未完成时为 `None`）。
    ///
    /// 由 `main_view` 在`CentralPanel` 布局出预览区时写入，
    /// 供图片导出裁剪使用（P2）。存**上一帧**的值不是缺陷：
    /// 截图命令要到下一帧才被后端捕获，届时本帧矩形刚刚写好。
    preview_rect: Option<egui::Rect>,
    /// 已发出截图命令、等待 GPU 回传的任务（P2 图片导出）。
    ///
    /// **必然跨一帧**：egui 的 `ViewportCommand::Screenshot` 在下一帧才通过
    /// `Event::Screenshot` 交回像素（帧 N 发命令、帧 N 绘制时被捕获）。
    /// 故点「导出」时只能先登记，等下一帧再兑现。
    ///
    /// 只有一个在等：连点两次时后一次会覆盖前一次。这是有意的——
    /// 两次截图捕获的是**同一帧画面**（egui 每帧只绘制一次），
    /// 排队等两次毫无意义，用户要的也不是「同一张图存两份」。
    pending_capture: Option<PendingCapture>,
}

/// 一次等待回传的截图任务。
///
/// 只存**裁剪所需的矩形**与落盘目标，不存像素：像素由 egui 在下一帧
/// 通过 `Event::Screenshot` 交回，那是唯一拿到它的时机。
struct PendingCapture {
    /// 导出任务 ID（用于把回传结果关联回领域任务）。
    job_id: JobId,
    /// 预览区矩形（**逻辑点**，兑现时乘 `pixels_per_point` 才是像素）。
    ///
    /// 存逻辑点而非像素：`pixels_per_point` 要到回传那一刻才能从
    /// `ctx` 读到，而矩形是在登记那一帧的布局里确定的。
    clip: egui::Rect,
    /// 输出路径。
    path: String,
    /// 命令是否已经发出去。
    ///
    /// 必须显式区分，否则每帧都会重发一次 `Screenshot`——那不是「多截几帧」，
    /// 而是让后端每帧都做一次全帧回读（GPU 同步等待），
    /// 代价高且用户只要第一张。
    requested: bool,
    /// 长图状态：`None` = 视口截图（一帧完成）；`Some` = 整篇长图（多段滚动拼接）。
    long: Option<LongImageState>,
}

/// 整篇长图的分段捕获状态。
///
/// 每段占用两帧：「设滚动偏移并等待绘制」→「发截图命令」→ 下一帧收像素。
/// 全部段落收齐后在 [`EditorApp::deliver_capture`] 里拼接落盘。
struct LongImageState {
    /// 每段的滚动偏移（逻辑点，内容坐标）。首段恒为 0，末段为 `content_h - 视口高`。
    offsets: Vec<f32>,
    /// 下一要捕获的段下标。
    next: usize,
    /// 已收到的各段像素（未拼接）。
    parts: Vec<image::RgbaImage>,
    /// 预览视口高度（逻辑点）。计算末段与上一段的重叠行数用。
    viewport_h: f32,
    /// 本段的滚动偏移是否已设、并等到了一次绘制。
    ///
    /// 截图命令捕获的是「发命令那一帧」的画面，所以必须先让预览
    /// 以目标偏移绘制一帧，**下一帧**才能发命令——两拍缺一不可。
    armed: bool,
}

/// 整篇长图的**段数上限**。
///
/// 每段至少跨两帧（预备拍 + 截图拍），256 段意味着滚动捕获约需十几秒——
/// 这已经是用户愿意等的上限。超限说明文档大到该走 HTML / PDF 导出，
/// 而不是让应用卡在「逐屏滚动截图」里几十秒甚至几分钟。
const MAX_LONG_IMAGE_SEGMENTS: usize = 256;

/// 计算长图拼接时每段需要丢弃的**顶部像素行数**。
///
/// 段偏移是「首段 0、其余每段推进一个视口高、末段滚到底」，因此只有
/// 末段与上一段重叠：上一段的底边（`offsets[i-1] + viewport_h`）超出
/// 本段顶边（`offsets[i]`）的部分就是要丢弃的重叠行。
///
/// 逻辑点 → 像素要乘 `pixels_per_point`（截图回传的是物理像素）；
/// 结果夹到 `part.height() - 1` 以内，防御舍入与几何量微小漂移
/// 导致 skip ≥ 段高（那会让 [`stitch`](crate::infrastructure::export::viewport_capture::stitch)
/// 直接拒绝整个任务，用户白等全部段落捕获完）。
fn compute_long_image_skip(long: &LongImageState, pixels_per_point: f32) -> Vec<u32> {
    long.parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                return 0;
            }
            let prev_end = long.offsets[i - 1] + long.viewport_h;
            let overlap = ((prev_end - long.offsets[i]).max(0.0) * pixels_per_point).round();
            overlap.min(part.height().saturating_sub(1) as f32) as u32
        })
        .collect()
}

impl EditorApp {
    /// 构造应用根状态。
    ///
    /// `system_dark` 是**系统**偏好（不是当前生效主题）——两者混用会让
    /// 「跟随系统」在用户手动切过主题后永久停在错误的一侧。
    pub fn new(services: Services, system_dark: bool) -> Self {
        // 主题与快捷键都**以持久化配置为权威来源**：用户改过的必须在
        // 启动第一帧就生效，否则会闪一下默认主题，且改过的键位在第一次
        // 按键前「看起来失效」——这是最容易被当成 bug 的表现。
        let settings = services
            .preferences
            .get_settings(GetSettingsQuery)
            .unwrap_or_else(|_| SettingsView::fallback());
        let theme = builtin::resolve(&settings.theme, system_dark);
        let registry = ShortcutRegistry::from_settings(&settings.shortcuts);

        // 启动恢复扫描：列出上次异常退出遗留的草稿，交由恢复提示处置。
        let pending_recoveries = services
            .documents
            .pending_recoveries(&services.drafts_dir)
            .unwrap_or_default();

        let mut app = Self {
            ui: AppUiState::with_shortcuts(theme, registry),
            editor: EditorUiState::new(),
            preview: PreviewUiState::new(),
            sidebar: SidebarUiState::new(),
            dialog: DialogUiState::new(),
            find: FindReplaceUiState::new(),
            export_ui: ExportUiState::default(),
            settings_ui: SettingsUiState::new(),
            workspace_entries: WorkspaceEntries::default(),
            status_bar: StatusBarUiState::new(),
            quick_open_ui: QuickOpenUiState::new(),
            shortcut_overview_ui: ShortcutOverviewUiState::new(),
            symbol_ui: SymbolUiState::new(),
            diff_ui: DiffUiState::new(),
            command_palette_ui: CommandPaletteUiState::new(),
            // 执行器要克隆 `services.dialogs`，故必须在移动 `services` 之前建。
            dialogs: DialogRunner::new(services.dialogs.clone()),
            services,
            system_dark,
            active_doc: None,
            editor_text: String::new(),
            pending_cursor: None,
            clear_undoer_next: false,
            pending_recoveries,
            preview_rect: None,
            pending_capture: None,
        };
        // 分割比例以持久化配置为权威来源：用户拖过的比例重启后仍生效。
        app.ui.split_ratio = settings.editor.split_ratio.clamp(SPLIT_MIN, SPLIT_MAX);
        // 行号栏开关同样以持久化配置为权威来源（默认隐藏，见领域 `EditorSettings`）。
        app.editor.line_numbers = settings.editor.show_line_numbers;
        app
    }

    /// 当前生效的主题。
    pub fn theme(&self) -> &Theme {
        &self.ui.theme
    }

    /// 快捷键分发器（菜单 / 命令面板渲染共用同一份注册表）。
    pub fn dispatcher(&self) -> &ShortcutDispatcher {
        &self.ui.dispatcher
    }

    /// 请求一次阻塞对话框（**在后台线程执行**，见 [`DialogRunner`]）。
    ///
    /// 沿用 [`PendingDialog`] 作为对外签名是为了让侧栏 / 欢迎页
    /// 不必知道「建议文件名」这类只对另存为有意义的细节。
    pub fn request_dialog(&mut self, kind: PendingDialog) {
        let request = match kind {
            PendingDialog::OpenFile => DialogRequest::OpenFile,
            PendingDialog::PickFolder => DialogRequest::PickFolder,
            PendingDialog::SaveFile => DialogRequest::SaveFile {
                suggested: self.suggested_save_name(),
            },
        };
        self.dialogs.spawn(request);
    }

    /// 当前是否有原生对话框在等待用户（状态栏提示用）。
    pub fn dialog_busy(&self) -> bool {
        self.dialogs.is_busy()
    }

    /// 应用一个后台对话框的结果（每帧调一次，无结果立即返回）。
    ///
    /// 公开而非私有：它是「取回结果 → 产生副作用」这条通路的**唯一**实现，
    /// 集成测试必须能驱动它——若测试自己另写一遍「拿到路径就打开」，
    /// 变异测试会立刻证明那是两份真相（S6-08 的教训）。
    ///
    /// # 取消必须**静默**
    ///
    /// 用户按「取消」是正常操作，不是错误。弹一条「打开失败：用户取消」
    /// 会让人以为程序出了故障。判据是错误**种类**
    /// （[`AppError::Cancelled`]）而非文案——按文案判断会在一次
    /// 措辞调整后静默失效。
    pub fn apply_dialog_outcome(&mut self, outcome: DialogOutcome) {
        if outcome.is_cancelled() {
            return;
        }
        match outcome {
            DialogOutcome::OpenFile(Ok(path)) => {
                let _ = self.open_path(path.as_str());
            }
            DialogOutcome::SaveFile(Ok(path)) => match self.save_document_to(path.as_str()) {
                Ok(()) => {
                    let now = self.services.markdown.now().millis();
                    self.status_bar
                        .notice(format!("已保存到 {}", path.as_str()), now);
                }
                Err(e) => self.set_status_error(&format!("另存为失败：{e}")),
            },
            DialogOutcome::PickFolder(Ok(path)) => {
                // 「选择文件夹」有两个用途、共用同一个原生目录对话框：
                // ① 打开工作区（「文件」菜单 / 欢迎页）；
                // ② 选导出输出目录（导出对话框里的「选择文件夹…」）。
                // 二者靠**上下文**区分：导出对话框是模态浮层，用户在它上面点
                // 「选择文件夹」时它必然开着；此时结果应回填输出目录，而不是
                // 把工作区根目录也切过去（那会顺带把文件树换掉）。
                if self.dialog.is_open(Overlay::Export) {
                    self.export_ui.output_dir = path.as_str().to_string();
                    self.export_ui.refresh_resolved_path();
                    return;
                }
                let root = path.as_str().to_string();
                match self.scan_workspace(&root) {
                    Ok(count) => {
                        let msg = format!("已打开工作区：{root}（{count} 项）");
                        self.set_status_notice(msg);
                    }
                    Err(e) => self.set_status_error(&format!("无法打开工作区 {root}：{e}")),
                }
            }
            // 非取消的错误：说明是「对话框系统不可用」这类问题，
            // 必须提示——静默会让用户以为点了没反应。
            DialogOutcome::OpenFile(Err(e)) => self.report_dialog_failure("打开文件", &e),
            DialogOutcome::SaveFile(Err(e)) => self.report_dialog_failure("另存为", &e),
            DialogOutcome::PickFolder(Err(e)) => self.report_dialog_failure("选择文件夹", &e),
        }
    }

    /// 报告「对话框本身不可用」这类失败。
    fn report_dialog_failure(&mut self, what: &str, error: &AppError) {
        self.set_status_error(&format!("{what}对话框不可用：{error}"));
    }

    /// 保存当前文档到指定路径（另存为对话框回填）。
    pub fn save_document_to(&mut self, path: &str) -> AppResult<()> {
        let Some(id) = self.ensure_document() else {
            return Ok(());
        };
        let target = FilePath::new(path).map_err(|e| AppError::Config {
            message: format!("路径非法：{e}"),
        })?;
        self.services
            .documents
            .save_to(SaveToPathCommand { id, path: target })?;
        // 内容没变但「已保存」状态变了，仍要回读让脏标记与标签标题一致。
        self.refresh_text();
        Ok(())
    }

    /// 读取持久化设置；失败时回退到 [`SettingsView::fallback`]。
    pub fn settings(&self) -> SettingsView {
        self.services
            .preferences
            .get_settings(GetSettingsQuery)
            .unwrap_or_else(|_| SettingsView::fallback())
    }

    /// 导出当前文档到 `path`（返回产出路径与字节数）。
    ///
    /// 编排放这里而不是留在视图里，有两个理由：
    ///
    /// 1. **可测**：视图的 `run_export` 是私有函数，集成测试够不到；
    ///    而「四类格式各产出一次真实文件」正是验收标准，必须能被测试断言。
    /// 2. **单一入口**：将来菜单、快捷键、命令面板要触发导出时，
    ///    走的是同一段代码——与 S6-07 「三入口一致」同源。
    ///
    /// 走应用层两段式（`create_job` → `run`）：选项校验在领域，
    /// 这里只负责拼 DTO 与把错误转成提示。
    pub fn export_current_document(
        &mut self,
        request: ExportRequestView,
        path: &str,
    ) -> AppResult<ExportResultView> {
        let destination = FilePath::new(path).map_err(|e| AppError::io(e.to_string()))?;
        // 输出目录可能不存在（用户手填的路径，或上次导出后目录被删）。
        // 导出前确保它存在——否则底层写盘会以「找不到路径」失败，
        // 而那条错误对用户毫无指导意义。`create_dir_all` 对已存在目录幂等，
        // 故重复导出不会报错。
        if let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::io(format!("无法创建输出目录 {}：{e}", parent.display())))?;
        }
        let doc_id = self
            .active_doc
            .ok_or_else(|| AppError::domain("没有可导出的文档"))?;
        let job = self.services.export.create_job(CreateExportJobCommand {
            doc_id,
            destination,
            request,
        })?;
        self.services.export.run(RunExportCommand { job_id: job })
    }

    /// 记录本帧预览区在窗口中的矩形（逻辑点）。
    ///
    /// 由 [`crate::presentation::views::main_view`] 在布局出预览区时调用。
    pub fn set_preview_rect(&mut self, rect: egui::Rect) {
        self.preview_rect = Some(rect);
    }

    /// 本帧记录的预览区矩形（尚未布局过时为 `None`）。
    pub fn preview_rect(&self) -> Option<egui::Rect> {
        self.preview_rect
    }

    /// 发起一次图片导出（视口截图），返回受理结果。
    ///
    /// # 为什么图片不走 `export_current_document`
    ///
    /// 那条路是「拿到源文本 → 同步写盘 → 返回字节数」。图片没有源文本可写——
    /// 像素来自GPU 对**当前帧**的回读，而 egui 的
    /// [`egui::ViewportCommand::Screenshot`] 要到下一帧才通过
    /// `egui::Event::Screenshot`] 交回来。于是这里只做「登记 + 发命令」，
    /// 真正的裁剪与落盘由 [`Self::deliver_capture`] 在下一帧完成。
    ///
    /// # 为什么必须要求预览区可见
    ///
    /// 截图裁的是预览区那一块。预览区被折叠（专注模式 / 关闭预览）时
    /// 根本没有这块像素，与其导出一张空白图，不如当场说清原因。
    pub fn begin_viewport_capture(&mut self, path: &str) -> AppResult<()> {
        let doc_id = self
            .active_doc
            .ok_or_else(|| AppError::domain("没有可导出的文档"))?;
        let clip = self
            .preview_rect
            .filter(|r| r.width() > 0.0 && r.height() > 0.0)
            .ok_or_else(|| {
                AppError::io("预览区当前不可见，无法截图（请关闭专注模式或打开预览面板）")
            })?;

        // 任务仍走应用层创建：选项校验（扩展名匹配等）与状态机是领域的事，
        // 不该因为「像素晚一帧到」就在视图层另起一套。
        let destination = FilePath::new(path).map_err(|e| AppError::io(e.to_string()))?;
        let request = ExportRequestView {
            format: "image".to_string(),
            include_toc: false,
            embed_images: false,
            theme: self.export_ui.theme.clone(),
            page_size: "a4".to_string(),
            paginate: false,
        };
        let job_id = self.services.export.create_job(CreateExportJobCommand {
            doc_id,
            destination,
            request,
        })?;
        self.pending_capture = Some(PendingCapture {
            job_id,
            clip,
            path: path.to_string(),
            requested: false,
            long: None,
        });
        self.export_ui.awaiting_capture = true;
        Ok(())
    }

    /// 发起一次**整篇长图**导出（自动滚动预览区、分段截图、纵向拼接）。
    ///
    /// # 与视口截图的区别
    ///
    /// 视口截图只取「此刻所见」；长图要把整篇文档拼进一张 PNG。
    /// 实现方式不是离屏重建（egui 没有这条公开路径），而是**复用既有
    /// 截图回路**：把预览区滚到每个分段位置、各截一帧、在基础设施层
    /// 纵向拼接（[`crate::infrastructure::export::viewport_capture::stitch`]）。
    ///
    /// # 参数
    ///
    /// `pixels_per_point` 用于在**登记时刻**估算成品高度并提前拦截
    /// 超限请求——否则用户要等整个滚动流程跑完才被告知失败。
    pub fn begin_long_capture(&mut self, path: &str, pixels_per_point: f32) -> AppResult<()> {
        let doc_id = self
            .active_doc
            .ok_or_else(|| AppError::domain("没有可导出的文档"))?;
        let clip = self
            .preview_rect
            .filter(|r| r.width() > 0.0 && r.height() > 0.0)
            .ok_or_else(|| {
                AppError::io("预览区当前不可见，无法截图（请关闭专注模式或打开预览面板）")
            })?;

        // 内容几何量来自上一帧的预览布局（同步状态里存的就是它）。
        let geometry = self.ui.sync.geometry(Side::Preview);
        let viewport_h = geometry.viewport_h;
        let content_h = geometry.content_h;
        if viewport_h <= 0.0 || content_h <= viewport_h {
            // 内容不足一屏：长图与视口截图等价，直接走单帧路径。
            return self.begin_viewport_capture(path);
        }

        // 分段：每段恰好一个视口高（无缝平铺）；末段滚到底，与上一段的重叠
        // 在拼接时丢弃。段数上限防止异常大的文档让应用滚动几十秒。
        let segments =
            usize::try_from((content_h / viewport_h).ceil() as u64).unwrap_or(usize::MAX);
        if segments > MAX_LONG_IMAGE_SEGMENTS {
            return Err(AppError::io(format!(
                "文档过长（约 {segments} 屏，上限 {MAX_LONG_IMAGE_SEGMENTS} 屏），请改用 HTML / PDF 导出"
            )));
        }
        let mut offsets = Vec::with_capacity(segments);
        for i in 0..segments {
            let offset = if i + 1 == segments {
                content_h - viewport_h
            } else {
                i as f32 * viewport_h
            };
            offsets.push(offset.min(content_h - viewport_h).max(0.0));
        }
        // 成品高度提前拦截：估算 = 内容高 × 像素密度。
        let estimated_height = (content_h * pixels_per_point).ceil() as u64;
        if estimated_height
            > u64::from(crate::infrastructure::export::viewport_capture::MAX_LONG_IMAGE_HEIGHT)
        {
            return Err(AppError::io(format!(
                "长图高度约 {estimated_height}px，超过上限 {}px，请改用 HTML / PDF 导出",
                crate::infrastructure::export::viewport_capture::MAX_LONG_IMAGE_HEIGHT
            )));
        }

        let destination = FilePath::new(path).map_err(|e| AppError::io(e.to_string()))?;
        let request = ExportRequestView {
            format: "image".to_string(),
            include_toc: false,
            embed_images: false,
            theme: self.export_ui.theme.clone(),
            page_size: "a4".to_string(),
            paginate: false,
        };
        let job_id = self.services.export.create_job(CreateExportJobCommand {
            doc_id,
            destination,
            request,
        })?;
        self.pending_capture = Some(PendingCapture {
            job_id,
            clip,
            path: path.to_string(),
            requested: false,
            long: Some(LongImageState {
                offsets,
                next: 0,
                parts: Vec::new(),
                viewport_h,
                armed: false,
            }),
        });
        self.export_ui.awaiting_capture = true;
        Ok(())
    }

    /// 兑现一次截图：裁剪 →（长图：继续下一段或拼接落盘）→ 推进领域任务状态。
    ///
    /// 由 [`Self::pump_capture`] 在收到 [`egui::Event::Screenshot`] 时调用。
    fn deliver_capture(
        &mut self,
        mut pending: PendingCapture,
        frame: &std::sync::Arc<egui::epaint::ColorImage>,
        ctx: &egui::Context,
    ) {
        // 任务已兑现，无论成败都不再占用「等待中」——
        // 放在函数开头，保证下面任何 early return 都不会把它漏掉。
        self.export_ui.awaiting_capture = false;
        // 逻辑点 → 物理像素。必须在**回传时刻**换算：`pixels_per_point`
        // 此刻才能从 ctx 读到，而矩形是上一帧布局定的。
        let ppp = ctx.input(|i| i.pixels_per_point);
        let clip = [
            (pending.clip.min.x * ppp).round() as i64,
            (pending.clip.min.y * ppp).round() as i64,
            (pending.clip.max.x * ppp).round() as i64,
            (pending.clip.max.y * ppp).round() as i64,
        ];

        let Some(mut long) = pending.long else {
            // 视口截图：一帧闭环。
            let result = crate::infrastructure::export::viewport_capture::capture_region_to_png(
                frame,
                clip,
                std::path::Path::new(&pending.path),
            )
            .map_err(|e| ExportError::io_unavailable(e.to_string()));

            match self
                .services
                .export
                .complete_deferred(pending.job_id, result)
            {
                Ok(view) => self.export_ui.succeed(format!(
                    "已导出 {}（{} 字节）",
                    view.output_path.as_str(),
                    view.bytes
                )),
                Err(e) => self.export_ui.fail(e.to_string()),
            }
            return;
        };

        // ---- 整篇长图：本帧像素是第 `long.next` 段 ----
        let cropped = crate::infrastructure::export::viewport_capture::crop(frame, clip);
        match cropped {
            Ok(part) => long.parts.push(part),
            Err(e) => {
                self.export_ui
                    .fail(ExportError::io_unavailable(e.to_string()).to_string());
                return;
            }
        }
        long.next += 1;

        let has_more = long.next < long.offsets.len();
        if has_more {
            // 还有段：把任务放回 pending（清两拍标志），awaiting 保持占用，
            // 防止用户在滚动过程中再次提交覆盖本次任务。
            self.export_ui.awaiting_capture = true;
            // `armed` 必须清零：本帧像素属于刚交付的段，**下一段**的滚动
            // 偏移尚未设置（步骤 2 要到下一拍才会设）。若沿用 armed=true，
            // 步骤 2 会在同一帧立即重发截图命令，捕获到的仍是上一段
            // 的画面——长图里出现重复内容。
            long.armed = false;
            // `long` 是从 `pending` 里移出来的字段，放回去之后 `pending`
            // 重新完整，可以整体塞回 `pending_capture`。
            pending.long = Some(long);
            pending.requested = false;
            self.pending_capture = Some(pending);
            return;
        }

        // 全部段到齐：拼接 → 落盘 → 推进领域任务。
        let skip_top = compute_long_image_skip(&long, ppp);
        let result =
            crate::infrastructure::export::viewport_capture::stitch(&long.parts, &skip_top)
                .and_then(|img| {
                    crate::infrastructure::export::viewport_capture::write_png(
                        &img,
                        std::path::Path::new(&pending.path),
                    )
                })
                .map_err(|e| ExportError::io_unavailable(e.to_string()));

        match self
            .services
            .export
            .complete_deferred(pending.job_id, result)
        {
            Ok(view) => self.export_ui.succeed(format!(
                "已导出整篇长图 {}（{} 字节）",
                view.output_path.as_str(),
                view.bytes
            )),
            Err(e) => self.export_ui.fail(e.to_string()),
        }
    }

    /// 处理截图回路：发出待处理的请求，并接收本帧回传的像素。
    ///
    /// 由 [`eframe::App::update`] 每帧在**绘制之后**调用。
    ///
    /// # 为什么必须跨两帧
    ///
    /// egui 在**帧末**才把 `ViewportCommand::Screenshot` 交给后端，
    /// 后端捕获的是**那一帧**的渲染结果，并在下一帧开头以
    /// `Event::Screenshot` 送回。所以「发」与「收」天然分处两帧，
    /// 同一帧内无法闭环——这不是实现取巧，是 API 的时序。
    ///
    /// `requested` 标志保证**只发一次**：否则本帧登记的任务会在之后
    /// 每帧都触发一次全帧 GPU 回读。
    pub fn pump_capture(&mut self, ctx: &egui::Context) {
        // 1) 先收：本帧事件里可能带着上一帧命令的回传结果。
        //
        // 只认 `requested == true` 的任务。否则用户在命令发出前就关掉了对话框、
        // 又恰好收到别的截图回传时，会把像素写到一个已作废的路径上。
        if self.pending_capture.as_ref().is_some_and(|p| p.requested)
            && let Some(frame) = take_screenshot_event(ctx)
            && let Some(pending) = self.pending_capture.take()
        {
            self.deliver_capture(pending, &frame, ctx);
        }

        // 2) 再发：把尚未发过命令的任务登记下去（结果留给下一帧的步骤 1）。
        //
        //    长图多一拍「预备」：命令捕获的是**发命令那一帧**的画面，
        //    所以要先设好滚动偏移、等预览以该偏移绘制一帧，下一帧才发命令。
        if let Some(pending) = self.pending_capture.as_mut()
            && !pending.requested
        {
            let mut scroll_to: Option<f32> = None;
            match pending.long.as_mut() {
                None => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                        pending.job_id.to_string(),
                    )));
                    pending.requested = true;
                }
                Some(long) => {
                    if long.armed && long.next < long.offsets.len() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                            egui::UserData::new(pending.job_id.to_string()),
                        ));
                        pending.requested = true;
                    } else if long.next < long.offsets.len() {
                        // 预备拍：把预览滚到本段位置。`preview_view` 每帧以
                        // `preview.scroll_y` 为初始偏移，下一帧即渲染本段。
                        scroll_to = Some(long.offsets[long.next]);
                        long.armed = true;
                    }
                }
            }
            if let Some(y) = scroll_to {
                self.preview.scroll_y = y;
            }
        }
    }

    /// 当前文档所在目录（导出默认输出目录）。
    ///
    /// 未保存过的新文档没有路径，此时返回 `None`——**不**回退到进程工作目录：
    /// GUI 程序的 CWD 取决于启动方式，用户完全无法预测，
    /// 把文件写到一个他看不见的地方比「要求先保存」糟糕得多。
    pub fn active_document_dir(&self) -> Option<String> {
        let doc = self.active_doc?;
        let view = self
            .services
            .documents
            .get_document(GetDocumentQuery { id: doc })
            .ok()?;
        let path = view.path?;
        path.parent().map(|parent| parent.as_str().to_string())
    }

    /// 当前文档标题去掉 `.md` 后缀（导出 / 另存为的建议文件名用）。
    ///
    /// 刻意**不**用路径的文件名：`/a/b/notes.md` 与 `/a/b/notes.txt`
    /// 去扩展名后同名，两个都导出到同一目录时会互相覆盖。
    /// 故取路径末段再判断扩展名。
    ///
    /// # 另存为的建议名要**带扩展名**
    ///
    /// 此函数服务于两处：导出（补 `.html` 等后缀）与另存为
    /// （交给系统对话框）。后者的建议名若不带 `.md`，
    /// 用户点确认就得到一个无扩展名的文件——双击打不开，
    /// 别的工具也不认。系统对话框**不会**替我们补后缀。
    /// 故另存为走 [`Self::suggested_save_name`]，而不是本函数。
    fn active_document_title_stem(&self) -> Option<String> {
        let doc = self.active_doc?;
        let title = self
            .services
            .documents
            .get_document(GetDocumentQuery { id: doc })
            .ok()?;
        let name = title
            .path
            .as_ref()
            .map_or_else(|| title.title.as_str(), |path| path.file_name())
            .to_string();
        let stem = match name.rfind('.') {
            Some(index) if index > 0 => name[..index].to_string(),
            _ => name,
        };
        let stem = if stem.trim().is_empty() {
            "document".to_string()
        } else {
            stem
        };
        Some(stem)
    }

    /// 另存为对话框的**建议文件名**（带扩展名）。
    ///
    /// 未命名文档给 `未命名.md`：系统对话框不会替我们补后缀，
    /// 建议名不带 `.md` 时用户点确认就得到一个双击打不开的文件。
    /// 「未命名」是领域给的标题（`Document` 的默认 title），
    /// 它本身不是合法文件名之一，但加上扩展名即可用。
    pub fn suggested_save_name(&self) -> String {
        match self.active_document_title_stem() {
            Some(stem) if stem.contains('.') => stem,
            Some(stem) => format!("{stem}.md"),
            // 一个文档都没有时：给一个像样的默认值而不是空串，
            // 空串会让对话框停在「未命名」并要求用户自己敲名字。
            None => "未命名.md".to_string(),
        }
    }

    /// 系统当前是否偏好深色（设置页「跟随系统」提示、状态栏主题文案用）。
    ///
    /// 刻意暴露**系统偏好**而不是「当前生效主题」：后者在
    /// `BuiltinTheme::System` 下已经被解析过一次，拿它判断
    /// 「系统是深是浅」等于拿答案去验证答案。
    pub fn system_prefers_dark(&self) -> bool {
        self.system_dark
    }

    // ————————————————————————————————————————————————————————————
    // 侧栏（S6-11：文件树 / 大纲 / 全局搜索）
    // ————————————————————————————————————————————————————————————
    //
    // 编排放这里而不留在视图，理由与 `export_current_document` 同源：
    // 这些是**验收标准里的行为**（「大纲点击跳转」「搜索结果跳转」
    // 「右键菜单四项可用」），必须能被集成测试断言，而视图的私有函数够不到。

    /// 工作区根目录（未打开文件夹时为空串）。
    pub fn workspace_root(&self) -> String {
        self.workspace_entries.root.clone().unwrap_or_default()
    }

    /// 扫描工作区并把条目装进 UI 状态（返回条目数）。
    ///
    /// 扫描是**磁盘 IO**，绝不能每帧做——故只在「打开文件夹 / 刷新 /
    /// 文件操作之后」显式调用，视图每帧只读已装好的 [`Self::workspace_entries`]。
    pub fn scan_workspace(&mut self, root: &str) -> AppResult<usize> {
        let root_path = FilePath::new(root).map_err(|e| AppError::domain(e.to_string()))?;
        let entries = self.services.workspace.scan_tree(ScanTreeCommand {
            root: root_path.clone(),
            max_depth: TREE_MAX_DEPTH,
        })?;
        self.workspace_entries = WorkspaceEntries::new(
            root_path.as_str().to_string(),
            entries
                .iter()
                .map(|e| (e.path.as_str().to_string(), e.is_directory))
                .collect(),
        );
        // 首次装入时把根目录下的目录全部展开：折叠的全展开会让用户
        // 以为「文件树是空的」，而实际只是没展开。
        if self.sidebar.needs_initial_expand {
            self.sidebar.needs_initial_expand = false;
            self.expand_top_level();
        }
        Ok(self.workspace_entries.items.len())
    }

    /// 展开根目录下的全部目录（仅第一层，深层留给用户自己点）。
    fn expand_top_level(&mut self) {
        let root = self.workspace_root();
        let top: Vec<String> = self
            .workspace_entries
            .items
            .iter()
            .filter(|(path, is_dir)| *is_dir && parent_of(path) == root)
            .map(|(path, _)| path.clone())
            .collect();
        for path in top {
            self.sidebar.expand(&path);
        }
    }

    /// 打开工作区里的某个文件（文件树点击 / 搜索跳转都走这里）。
    pub fn open_workspace_file(&mut self, path: &str) -> AppResult<()> {
        let file_path = FilePath::new(path).map_err(|e| AppError::domain(e.to_string()))?;
        let id = self.services.documents.open(OpenFileCommand {
            path: file_path.clone(),
        })?;
        self.active_doc = Some(id);
        self.find.on_document_switched(id);
        self.editor.cursor = 0;
        self.editor.anchor = None;
        self.ui.sync.reset();
        self.refresh_text();
        self.sidebar.selected_path = Some(path.to_string());
        // 展开文件所在的全部祖先目录，否则文件树里看不到刚打开的文件。
        self.expand_ancestors(path);
        // 记录到最近文件：这条数据同时喂给欢迎页与导出默认目录。
        let _ = self
            .services
            .workspace
            .touch_recent(TouchRecentCommand { path: file_path });
        Ok(())
    }

    /// 展开某路径的所有祖先目录。
    fn expand_ancestors(&mut self, path: &str) {
        let mut current = path.to_string();
        while let Some(cut) = current.rfind('/') {
            current.truncate(cut);
            if current.is_empty() || current == self.workspace_root() {
                break;
            }
            self.sidebar.expand(&current);
        }
    }

    /// 新建文件或目录（右键菜单「新建」提交时调用）。
    pub fn create_entry(&mut self, parent: &str, name: &str, is_directory: bool) -> AppResult<()> {
        let base = if parent.is_empty() {
            FilePath::new(self.workspace_root())
                .map_err(|e: crate::shared::path::PathError| AppError::domain(e.to_string()))?
        } else {
            FilePath::new(parent).map_err(|e| AppError::domain(e.to_string()))?
        };
        let path = base
            .join(name)
            .map_err(|e| AppError::domain(e.to_string()))?;
        self.services
            .workspace
            .create_entry(CreateEntryCommand { path, is_directory })?;
        // 新建文件默认建在**根**或指定目录下；若建在某个未展开的目录里，
        // 用户会看不到刚建出来的东西。
        if !parent.is_empty() {
            self.sidebar.expand(parent);
        }
        // 条目已变，重扫一次让树与磁盘一致（顺序也按名称重排）。
        self.services.workspace.rescan()?;
        self.reload_workspace_entries()
    }

    /// 重命名条目。
    pub fn rename_entry(&mut self, path: &str, new_name: &str) -> AppResult<()> {
        let from = FilePath::new(path).map_err(|e| AppError::domain(e.to_string()))?;
        let parent = from
            .parent()
            .ok_or_else(|| AppError::domain("该条目没有父目录，无法重命名"))?;
        let to = parent
            .join(new_name)
            .map_err(|e| AppError::domain(e.to_string()))?;
        self.services
            .workspace
            .move_entry(MoveEntryCommand { from, to })?;
        self.reload_workspace_entries()
    }

    /// 删除条目。
    ///
    /// **不做二次确认**：删除是不可逆的，而本项目此刻还没有能用的模态确认框
    /// （`DialogPort::confirm` 是同步阻塞的，不能在 `update()` 里调）。
    /// 与其塞一个「假确认」让人以为有保护，不如让删除走**空目录 / 单文件**
    /// 这样的小步子——真正的确认放到S6-11e 的欢迎页接线时一并补。
    pub fn delete_entry(&mut self, path: &str) -> AppResult<()> {
        let target = FilePath::new(path).map_err(|e| AppError::domain(e.to_string()))?;
        self.services
            .workspace
            .delete_entry(DeleteEntryCommand { path: target })?;
        if self.sidebar.selected_path.as_deref() == Some(path) {
            self.sidebar.selected_path = None;
        }
        self.reload_workspace_entries()
    }

    /// 从磁盘重新装载文件树条目（操作变更了树之后调用）。
    fn reload_workspace_entries(&mut self) -> AppResult<()> {
        let root = self.workspace_root();
        if root.is_empty() {
            return Ok(());
        }
        self.scan_workspace(&root)?;
        Ok(())
    }

    /// 「刷新」按钮：重新扫描工作区根目录。
    ///
    /// 树是**快照**（见 `WorkspaceEntries`），只在本应用改动文件后更新。
    /// 但文件也会在本应用之外变——用户用别的编辑器改了文件、git 拉了新
    /// 版本、构建产物被清理——此时树就与磁盘不一致，用户点文件名打开却
    /// 报「文件不存在」，看起来像应用坏了。
    ///
    /// 故需要一个公开入口。放在 `frame` 而非让调用方自己串
    /// `workspace.rescan()` + `scan_workspace()`：那两步必须成对出现，
    /// 漏掉第二步就会「重扫了但界面没变」，而漏掉第一步会导致聚合
    /// 里留着已删除的条目。
    ///
    /// 未打开工作区时是**无效果**（返回 0）而不是报错：用户还没选目录
    /// 就点刷新，不该弹一个无法理解的错误。
    pub fn refresh_workspace(&mut self) -> usize {
        let root = self.workspace_root();
        if root.is_empty() {
            return 0;
        }
        // 聚合与界面快照必须都更新：先重扫领域聚合（它可能还持有
        // 磁盘上已不存在的条目），再重扫 UI 快照。
        if self.services.workspace.rescan().is_err() {
            return 0;
        }
        self.scan_workspace(&root).unwrap_or(0)
    }

    /// 取当前文档的大纲（`None` 表示没有活动文档或解析失败）。
    pub fn current_outline(&self) -> Vec<OutlineItemView> {
        let Some(id) = self.active_doc else {
            return Vec::new();
        };
        self.services
            .markdown
            .rebuild_outline(RebuildOutlineQuery { doc_id: id })
            .map(|view| view.0)
            .unwrap_or_default()
    }

    /// 跳到大纲某一项（把行号换成字节偏移后复用查找替换的跳转通路）。
    ///
    /// 为什么不直接按行号滚：`TextEdit` 与滚动同步全都以**字节偏移**为坐标，
    /// 这里若自己换算一遍就会与 `scroll_to_offset` 各算各的，迟早漂移。
    pub fn goto_outline_line(&mut self, line: usize) {
        let starts = crate::presentation::views::scroll_sync::line_start_offsets(&self.editor_text);
        let Some(&offset) = starts.get(line) else {
            return;
        };
        self.goto_byte_offset(offset);
    }

    /// 跳到某个字节偏移（选中该处并滚进视口）。
    ///
    /// 大纲跳转与搜索跳转共用它——两条路径对「跳过去之后要做什么」
    /// 的理解必须一致（选中 + 滚动），否则用户会发现「大纲跳转不选中、
    /// 搜索跳转选中」，进而怀疑其中一个坏了。
    pub fn goto_byte_offset(&mut self, offset: usize) {
        let at = offset.min(self.editor_text.len());
        self.editor.set_selection(at, at);
        let cursor = editor_ops::byte_to_char(&self.editor_text, at);
        self.pending_cursor = Some((cursor, cursor));
        self.scroll_to_offset(at);
    }

    /// 在工作区里搜索。
    ///
    /// 每次调用都**真的**走一遍磁盘（`WorkspaceAppService::search`），
    /// 绝不在 UI 状态里缓存结果：那会让「文件改了但搜索结果还是旧的」。
    ///
    /// 代价是每帧可能全盘扫描，故视图只在「搜索标签可见」时调用。
    /// 真要优化应加在应用层（那里才知道文件有多大），而不是让表现层
    /// 偷偷少搜几次——那会让「搜到了」与「搜全了」不再等价。
    pub fn search_workspace(&self) -> Option<WorkspaceSearch> {
        if !self.sidebar.should_search() {
            return Some(WorkspaceSearch::default());
        }
        let result = self
            .services
            .workspace
            .search(SearchQuery {
                pattern: self.sidebar.search_query.clone(),
                regex: self.sidebar.search_options.regex,
                case_sensitive: self.sidebar.search_options.case_sensitive,
                whole_word: self.sidebar.search_options.whole_word,
            })
            .ok()?;
        Some(WorkspaceSearch {
            groups: group_search_hits(
                &result,
                &self.sidebar.search_query,
                self.sidebar.search_options.case_sensitive,
            ),
            truncated: result.truncated,
        })
    }

    /// 编辑区 `TextEdit` 的固定 `Id`（用于读写 egui 内部光标 / 撤销栈状态）。
    pub fn editor_id() -> egui::Id {
        egui::Id::new("rustmark.editor.text")
    }

    /// 取得活动文档；没有则新建一个空白未命名文档。
    ///
    /// # 谁在调用
    ///
    /// * `editor_view::show` **每帧**兜底调用 —— 一旦离开欢迎页，
    ///   编辑区必须永远有一页可编辑（否则用户看到的是一块点不动的灰板）；
    /// * `save_document` 在保存前调用，让「没打开任何文档时按 Ctrl+S」
    ///   也能建一份出来而不是静默失败。
    ///
    /// 启动时**不**调用：那一次是欢迎页，见 [`Self::should_show_welcome`]。
    pub fn ensure_document(&mut self) -> Option<DocumentId> {
        if self.active_doc.is_some() {
            return self.active_doc;
        }
        if let Ok(id) = self.services.documents.new_document(NewDocumentCommand) {
            self.active_doc = Some(id);
            // 登记查找状态的「当前文档」：不登记的话，首次跨文档时
            // `on_document_switched` 会把这一次当成「首次」而非「切换」，
            // 于是本该清空的旧命中被保留下来。
            self.find.on_document_switched(id);
            self.refresh_text();
        }
        self.active_doc
    }

    /// 读取当前文档的完整内容（**从领域**，非 `editor_text` 副本）。
    ///
    /// 存在的理由：集成测试需要断言「三入口执行后文档内容字节一致」，
    /// 而 `active_doc` 与 `editor_text` 都是 `pub(crate)`，测试拿不到。
    /// 与其让测试去猜内部可见性，不如把「读当前内容」作为正式能力暴露——
    /// 它本来也是导出、查找替换、状态栏字数统计都要用的东西。
    pub fn current_content(&self) -> Option<String> {
        let id = self.active_doc?;
        self.services
            .documents
            .get_content(GetContentQuery { id, range: None })
            .ok()
    }

    /// 从领域回读当前文档内容到 `editor_text`，并夹紧光标/选区、失效高亮缓存。
    ///
    /// 这是「领域是内容单一真相」的物理保证：每次领域命令后都调用它，
    /// `editor_text` 永远等于 `get_content` 的返回值，二者无法漂移。
    pub fn refresh_text(&mut self) {
        if let Some(id) = self.active_doc
            && let Ok(text) = self
                .services
                .documents
                .get_content(GetContentQuery { id, range: None })
        {
            self.editor_text = text;
            self.editor.clamp_to(self.editor_text.len());
        }
        // 文本被重新拉取 = 内容可能已变 → 重置预览防抖的计时起点。
        //
        // 刻意放在**这里**而不是各个编辑入口：编辑入口有十几处
        // （编辑区输入、插入、替换、整篇变换、查找替换、领域命令…），
        // 漏任何一处都会让防抖退化成「每 150ms 重解析一次」的节流。
        // 全部调用点都是命令 / 事件驱动的（不是每帧），故不会把计时无限推后。
        let now_ms = self.services.markdown.now().millis();
        self.preview.mark_edited(now_ms);
    }

    /// 缩进单位（空格），来自设置 `tab_size`。
    fn tab_unit(&self) -> String {
        " ".repeat(self.settings().editor.tab_size.max(1) as usize)
    }

    /// 当前打开的文档列表（按领域内部顺序）。
    ///
    /// 领域按 `DocumentId` 各存各的内容；UI 的「标签」只是这些文档的视图。
    pub fn open_document_list(&self) -> Vec<DocumentView> {
        self.services
            .documents
            .list_open(ListOpenDocumentsQuery)
            .unwrap_or_default()
    }

    /// 切换到指定文档（多标签）。
    ///
    /// 领域按 `DocumentId` 各自保存内容，`active_doc` 只是 UI 视角的「当前页」；
    /// 切换即换视图，不涉及任何内容搬运——双写不一致在此被结构性排除。
    pub(crate) fn switch_document(&mut self, id: DocumentId) {
        if self.active_doc == Some(id) {
            return;
        }
        self.active_doc = Some(id);
        // 查找状态跨文档策略在此生效（S6-08）：保留查询与选项，
        // 但清空属于旧文档的命中。不调的话「下一个」会按旧文档的
        // 字节偏移跳到新文档的无关位置，甚至越界。
        self.find.on_document_switched(id);
        // 每文档独立的光标 / 选区尚未在 UI State 中建模（S6-12 范畴），
        // 切换时回到文档开头，避免停在另一个文档的偏移上。
        self.editor.cursor = 0;
        self.editor.anchor = None;
        // 两侧滚动偏移都归零，若不重置同步状态，这一帧会被判成「某一侧
        // 发生了巨大位移」并把对侧也拖走——表现为「切标签后预览自己滚了一下」。
        self.ui.sync.reset();
        self.refresh_text();
        // 面板若开着，重扫一次让计数与新文档一致。
        if self.dialog.is_open(Overlay::Find) {
            self.sync_find();
        }
    }

    /// 关闭指定标签页（多标签 × 按钮用）。
    ///
    /// 仅当存在多个打开文档时才真正关闭；最后一份文档关闭后会由
    /// `ensure_document` 在下一帧补建一份空白文档（保证「永远至少有一页可编辑」）。
    pub(crate) fn close_document(&mut self, id: DocumentId) {
        if self.open_document_list().len() <= 1 {
            return;
        }
        let _ = self
            .services
            .documents
            .close(CloseCommand { id, force: false });
        // 仅当文档确实被关闭（不再是「需要保存」而保持打开）才清草稿，
        // 否则脏文档仍在编辑中，草稿仍是它的崩溃保险，不该清掉。
        if self
            .services
            .documents
            .get_document(GetDocumentQuery { id })
            .is_err()
        {
            let _ = self
                .services
                .documents
                .clear_draft(&self.services.drafts_dir, id);
        }
        // 关闭后重新取列表，挑一个相邻文档作为新的活动页（若关掉的是当前页）。
        let remaining = self.open_document_list();
        if remaining.is_empty() {
            self.active_doc = None;
        } else if self.active_doc == Some(id) {
            let idx = remaining.iter().position(|d| d.id == id).unwrap_or(0);
            let next = remaining
                .get(idx.saturating_sub(1))
                .or_else(|| remaining.first());
            if let Some(doc) = next {
                self.active_doc = Some(doc.id);
            }
        }
        self.editor.cursor = 0;
        self.editor.anchor = None;
        self.ui.sync.reset();
        self.refresh_text();
    }

    /// 关闭当前标签页（快捷键 `CloseTab`）。
    pub(crate) fn close_active_tab(&mut self) {
        if let Some(id) = self.active_doc {
            self.close_document(id);
        }
    }

    /// 当前是否应显示**欢迎页**（一个标签都没开）。
    ///
    /// 判据是「有没有活动文档」而不是「文档数是不是 1」：
    /// 启动时那份空白未命名文档**不算**「用户开了个文档」，
    /// 它只是为了让编辑区第一帧可编辑。所以启动时若也显示欢迎页，
    /// 底下那份空白文档就成了「看不见但存在」的多余标签。
    pub fn should_show_welcome(&self) -> bool {
        self.active_doc.is_none()
    }

    /// 关掉**全部**标签，回到欢迎页。
    ///
    /// [`Self::close_document`] 刻意**不**关最后一个标签
    /// （「永远至少有一页可编辑」是硬约束），但欢迎页需要
    /// 「一个标签都不剩」这个状态。二者不冲突：这里走
    /// `force = true` 逐个关掉，不经过那个约束。
    ///
    /// 未命名文档被丢弃是**无损**的（内容为空），
    /// 已保存过的文档则已在磁盘上；未保存的修改会丢失，
    /// 故只在欢迎页的「关闭全部」这类明确动作上调用。
    pub fn close_all_documents(&mut self) {
        for view in self.open_document_list() {
            let _ = self.services.documents.close(CloseCommand {
                id: view.id,
                force: true,
            });
            // 文档已关闭：清掉其草稿，避免下次启动误报「待恢复」。
            let _ = self
                .services
                .documents
                .clear_draft(&self.services.drafts_dir, view.id);
        }
        self.active_doc = None;
        self.editor_text.clear();
        self.editor.reset_for_new_document();
        self.ui.sync.reset();
        self.refresh_text();
    }

    // ================================================================
    // 文件动作（`execute` 的分支实现）
    // ================================================================

    /// 保存 / 另存为。
    ///
    /// 「未命名文档没有路径」时**不弹错误**而是转成「另存为」请求：
    /// 用户按 `Ctrl+S` 的意图是「把这份东西存下来」，
    /// 弹一个「需先指定保存路径」等于把「另存为」这一步的工作推给用户，
    /// 而用户按 `Ctrl+Shift+S` 就能做到同一件事。
    fn run_save(&mut self, save_as: bool) {
        let needs_path = self
            .active_doc
            .and_then(|id| {
                self.services
                    .documents
                    .get_document(GetDocumentQuery { id })
                    .ok()
            })
            .is_none_or(|v| v.path.is_none());
        if needs_path {
            self.request_dialog(PendingDialog::SaveFile);
            return;
        }
        if let Err(e) = self.save_document(save_as) {
            self.set_status_error(&e.to_string());
        }
    }

    /// 新建文档（`Ctrl+N`）。
    fn run_new_document(&mut self) {
        if let Err(e) = self.new_document() {
            self.set_status_error(&e.to_string());
        }
    }

    /// 在状态栏显示一条错误。
    ///
    /// 时钟取自 `services.markdown`（所有应用服务共用同一个 `Clock`），
    /// 不直接调 `SystemClock`：消息的「4 秒后消失」是可测规则，
    /// 用系统时钟就没法写测试验证它。
    pub fn set_status_error(&mut self, text: &str) {
        let now = self.services.markdown.now().millis();
        self.status_bar.error(text, now);
    }

    /// 在状态栏显示一条普通提示。
    pub fn set_status_notice(&mut self, text: impl Into<String>) {
        let now = self.services.markdown.now().millis();
        self.status_bar.notice(text, now);
    }

    /// 当前文档的面包屑路径；无活动文档或无路径时返回「未命名文档」。
    pub fn current_document_path(&self) -> String {
        let Some(id) = self.active_doc else {
            return "未命名文档".to_string();
        };
        match self
            .services
            .documents
            .get_document(GetDocumentQuery { id })
        {
            Ok(view) => view
                .path
                .map(|p| p.as_str().to_string())
                .unwrap_or_else(|| "未命名文档".to_string()),
            Err(_) => "未命名文档".to_string(),
        }
    }

    // ================================================================
    // S6-11f：浮层编排
    // ================================================================

    /// 当前生效主题的显示名（「关于」面板用）。
    pub fn theme_kind_label(&self) -> &'static str {
        match self.ui.theme.kind {
            BuiltinTheme::Dark => "深色",
            BuiltinTheme::Light => "浅色",
            BuiltinTheme::System => "跟随系统",
        }
    }

    /// 在光标处插入一段文本（符号面板 / 工具栏插入结构共用）。
    ///
    /// 走领域 `insert_text` 而非直接改 `editor_text`，理由与
    /// S6-07 的「三入口一致」同源：直接改 UI 文本会绕过一次撤销点，
    /// 让用户「插入一个 emoji 之后按 Ctrl+Z 什么也没撤销」。
    ///
    /// # 插入点必须取 **UI 光标**而非领域光标
    ///
    /// 两者是不同的坐标系：领域记的是**上一次领域命令后**的光标，
    /// UI 记的是 egui `TextEdit` 当前画出来的那个。而大纲跳转
    /// （[`Self::goto_byte_offset`]）、查找命中（`reveal_match`）
    /// 都只改 UI 光标——用户看到光标跳到了第 40 行，
    /// 领域那边还停在第 3 行。此时按「插入符号」若读领域光标，
    /// 内容会插到用户**看不见**的位置。
    /// （`move_line` / `run_format` 同样取 UI 光标，规则一致。）
    ///
    /// 光标停在插入内容**之后**：停在内容里意味着用户接着打字
    /// 会把文字插进代码块标记中间——这是插入类操作最常见的投诉。
    ///
    /// 有选区时**替换选区**：用户选中一段文字再插链接，
    /// 得到「选中文字链接选中文字」是几乎所有编辑器的行为，
    /// 也符合「插入」这个词的含义。
    pub fn insert_at_cursor(&mut self, text: &str) -> AppResult<()> {
        let Some(id) = self.ensure_document() else {
            return Ok(());
        };
        // 选区优先：无选区时 `selection()` 返回 `None`，退回光标。
        // 选区优先：无选区时 `selection()` 返回 `None`，退回光标。
        let (start, end) = self
            .editor
            .selection()
            .unwrap_or((self.editor.cursor, self.editor.cursor));
        let end = end.max(start);
        if start != end {
            self.services.documents.replace_range(ReplaceRangeCommand {
                id,
                range: TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
                text: text.to_string(),
            })?;
        } else {
            self.services.documents.insert_text(InsertTextCommand {
                id,
                offset: start,
                text: text.to_string(),
            })?;
        }
        self.refresh_text();
        let after = start + text.len();
        // 领域光标也要跟上：否则下一次插入仍会用旧位置（见上）。
        self.editor.set_selection(after, after);
        self.pending_cursor = Some((
            editor_ops::byte_to_char(&self.editor_text, after),
            editor_ops::byte_to_char(&self.editor_text, after),
        ));
        self.clear_undoer_next = true;
        Ok(())
    }

    /// 读取一个工作区文件的文本内容（差异对比的右侧）。
    pub fn read_workspace_file(&self, path: &str) -> AppResult<String> {
        let target = FilePath::new(path).map_err(|e| AppError::Config {
            message: format!("路径非法：{e}"),
        })?;
        self.services
            .workspace
            .read_file(ReadFileQuery { path: target })
    }

    // ================================================================
    // 关闭浮层
    // ================================================================

    /// 新建空白文档并切到它。
    ///
    /// 「切换」而不只是「创建」：用户在欢迎页点「新建文件」期待的是
    /// **立刻能打字**，若只是建了文档却仍停在旧标签，他会以为按钮坏了。
    pub fn new_document(&mut self) -> AppResult<()> {
        let id = self.services.documents.new_document(NewDocumentCommand)?;
        self.active_doc = Some(id);
        self.find.on_document_switched(id);
        self.refresh_text();
        Ok(())
    }

    /// 保存当前文档。
    ///
    /// `save_as = true` 时另存副本。**未命名文档没有路径**，
    /// 领域会拒绝并返回「未命名文档需先指定保存路径」——
    /// 那是正确行为：静默存到某个猜测的位置比报错更糟。
    pub fn save_document(&mut self, save_as: bool) -> AppResult<()> {
        let Some(id) = self.ensure_document() else {
            return Ok(());
        };
        self.services.documents.save(SaveCommand { id, save_as })?;
        // 已正常保存：草稿失去意义，清掉以免启动时误报「待恢复」。
        let _ = self
            .services
            .documents
            .clear_draft(&self.services.drafts_dir, id);
        // 内容没变但「已保存」状态变了，仍要回读让脏标记与标签标题一致。
        self.refresh_text();
        Ok(())
    }

    /// 自动保存节拍（DEF-02 修复）。
    ///
    /// 读出当前「自动保存间隔」，询问计时器哪些文档已空闲达间隔，
    /// 为到期文档写入草稿。空转极廉价——未到期时 `due` 立即返回空列表，不落盘。
    ///
    /// 计时器的输入来自 `EventSubscribers` 订阅的 `TextModified` 事件
    /// （在 `bootstrap` 装配阶段接线），故**只有真正编辑过的脏文档**才会进入到期队列。
    fn run_autosave(&mut self) {
        let interval = self
            .services
            .preferences
            .get_settings(GetSettingsQuery)
            .ok()
            .and_then(|s| s.editor.auto_save_interval);
        let Some(secs) = interval else {
            return;
        };
        if secs == 0 {
            return; // 0 表示「关闭自动保存」
        }
        let now = self.services.markdown.now();
        let due = self
            .services
            .auto_save_tracker
            .due(now, (secs as u64) * 1000);
        if due.is_empty() {
            return;
        }
        let drafts_dir = self.services.drafts_dir.clone();
        for id in due {
            let _ = self.services.documents.save_draft(&drafts_dir, id);
        }
    }

    /// 启动恢复提示（DEF-02）：若 `pending_recoveries` 非空，弹出一个居中的
    /// 非阻塞窗口，提供「恢复全部」（按内容新建未命名文档）与「丢弃全部」
    /// （清掉草稿）两个动作。
    ///
    /// 直到用户点击之前，`pending_recoveries` 持续非空、每帧重绘；点击后才清空，
    /// 因此不会因某一帧没点到就丢失待恢复内容。
    fn render_recovery_prompt(&mut self, ctx: &egui::Context) {
        if self.pending_recoveries.is_empty() {
            return;
        }
        let previews: Vec<String> = self
            .pending_recoveries
            .iter()
            .map(|(_, content)| {
                let preview: String = content.chars().take(80).collect();
                if content.chars().count() > 80 {
                    format!("{preview}…")
                } else {
                    preview
                }
            })
            .collect();
        let mut action: Option<bool> = None;
        egui::Window::new("未保存草稿恢复")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("检测到上次异常退出时遗留的未保存草稿，是否恢复？");
                ui.separator();
                for preview in &previews {
                    ui.label(format!("· {preview}"));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("恢复全部").clicked() {
                        action = Some(true);
                    }
                    if ui.button("丢弃全部").clicked() {
                        action = Some(false);
                    }
                });
            });
        match action {
            Some(true) => self.recover_all_drafts(),
            Some(false) => self.discard_all_drafts(),
            None => {}
        }
    }

    /// 恢复全部遗留草稿：按草稿内容新建未命名文档，并删除对应草稿文件。
    ///
    /// 从 [`Self::render_recovery_prompt`] 的「恢复全部」分支抽出，使按钮点击与
    /// 集成测试走**同一条**代码路径——否则测试覆盖的是副本，按钮本身反而是未测代码。
    pub fn recover_all_drafts(&mut self) {
        let drafts_dir = self.services.drafts_dir.clone();
        for (id, content) in std::mem::take(&mut self.pending_recoveries) {
            if let Ok(new_id) = self.services.documents.new_document_with_content(&content) {
                // 仅在恢复成功后清草稿：新建失败时保留草稿，下次启动仍会再提示一次，
                // 而不是让用户的内容随一次失败一起消失。
                let _ = self.services.documents.clear_draft(&drafts_dir, id);
                self.active_doc = Some(new_id);
                self.refresh_text();
            }
        }
    }

    /// 丢弃全部遗留草稿：删除草稿文件并清空待恢复列表。
    pub fn discard_all_drafts(&mut self) {
        let drafts_dir = self.services.drafts_dir.clone();
        for (id, _) in &self.pending_recoveries {
            let _ = self.services.documents.clear_draft(&drafts_dir, *id);
        }
        self.pending_recoveries.clear();
    }

    /// 启动扫描发现的待恢复草稿（只读快照，供状态断言与集成测试）。
    pub fn pending_recoveries(&self) -> &[(DocumentId, String)] {
        &self.pending_recoveries
    }

    /// 当前文档是否有未保存修改。
    pub fn current_document_dirty(&self) -> bool {
        self.active_doc
            .and_then(|id| {
                self.services
                    .documents
                    .get_document(GetDocumentQuery { id })
                    .ok()
            })
            .is_some_and(|v| v.is_dirty)
    }

    /// 当前文档的类型标签（状态栏左端「Markdown」用）。
    ///
    /// 按**扩展名**判定而非写死「Markdown」：编辑器能打开 `.txt`，
    /// 状态栏却显示「Markdown」会让用户怀疑自己打开了错的文件。
    pub fn current_document_kind(&self) -> &'static str {
        let Some(id) = self.active_doc else {
            return "纯文本";
        };
        let ext = self
            .services
            .documents
            .get_document(GetDocumentQuery { id })
            .ok()
            .and_then(|v| v.path)
            .map(|p| {
                let name = p.file_name();
                name.rfind('.')
                    .filter(|i| *i + 1 < name.len())
                    .map(|i| name[i + 1..].to_ascii_lowercase())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        match ext.as_str() {
            "md" | "markdown" | "mdown" | "mkd" => "Markdown",
            "txt" | "text" => "纯文本",
            "rst" => "reStructuredText",
            _ => "Markdown",
        }
    }

    /// 打开最近文件列表（按最近使用降序）。
    ///
    /// 欢迎页与「文件 → 最近」共用。**每次现取**而不是缓存：
    /// 最近文件会被本应用自己的打开操作改写，缓存必然出现
    /// 「刚打开的文件不在最近列表里」这种自相矛盾的状态。
    pub fn recent_files(&self) -> Vec<RecentFileView> {
        self.services
            .workspace
            .list_recent(ListRecentQuery)
            .unwrap_or_default()
    }

    /// 打开一个文件（用于最近文件、文件对话框）。
    ///
    /// 失败时**自己**把原因报到状态栏，不只把 `Err` 往上抛：
    /// 欢迎页点一个已被删除的最近文件时，调用方若忘了展示错误，
    /// 用户看到的就是「点了没反应」——比任何错误提示都更让人困惑。
    /// 报错的职责放在这里而不是各调用方，是为了**按构造**保证可见。
    pub fn open_path(&mut self, path: &str) -> AppResult<()> {
        let result = self.open_workspace_file(path);
        if let Err(e) = &result {
            let name = path.rsplit('/').next().unwrap_or(path);
            self.set_status_error(&format!("无法打开 {name}：{e}"));
        }
        result
    }

    /// 文件拖放打开（P2）。
    ///
    /// egui/winit 后端在「拖放发生的那一帧」把文件填进 `ctx.input().raw.dropped_files`，
    /// 下一帧即清空，因此每帧读一次即可，无需手动消费或去重。
    ///
    /// 行为（推荐方案）：
    /// - 按扩展名分类（`classify_dropped`）——`.md` / `.markdown` / `.txt` 视为可打开文本，
    ///   逐个打开为标签；其余扩展名拒绝并在状态栏提示；
    /// - 拖入无本地路径的「虚拟文件」（如浏览器拖拽）忽略；
    /// - 一次拖入多个文件：全部可识别的都打开（批量），不截断到第一个；
    /// - **目录**在分类前单独拦下：目录也可能带 `.md` 后缀，若放进扩展名分类
    ///   会被当作文本交给 `open_path`，最终以「无法读取文件」报错——
    ///   一次误拖换来一条误导性的红色错误。目录不是异常，提示语用「忽略」不用「拒绝」。
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.as_ref().map(|p| p.to_string_lossy().to_string()))
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        // 目录判定用真实文件系统：扩展名对目录没有任何意义。
        let dir_count = dropped
            .iter()
            .filter(|p| std::path::Path::new(p.as_str()).is_dir())
            .count();
        let files: Vec<String> = dropped
            .into_iter()
            .filter(|p| !std::path::Path::new(p.as_str()).is_dir())
            .collect();
        let (open, reject) = classify_dropped(&files);
        for p in &open {
            let _ = self.open_path(p);
        }
        // 组装「打开了什么、忽略了什么」的提示：两类忽略原因分开说清，
        // 否则用户无法区分「格式不支持」与「拖了个文件夹」。
        let mut skipped: Vec<String> = Vec::new();
        if !reject.is_empty() {
            skipped.push(format!("{} 个非 Markdown / 文本文件", reject.len()));
        }
        if dir_count > 0 {
            skipped.push(format!("{dir_count} 个目录"));
        }
        if !skipped.is_empty() {
            self.set_status_notice(format!(
                "已打开 {} 个文件；{} 已忽略",
                open.len(),
                skipped.join("、")
            ));
        }
    }

    // ================================================================
    // 行列换算（状态栏「行 N, 列 M」用）
    // ================================================================

    /// 当前光标的行列（**1-based**，与原型 `posToLineCol` 一致）。
    ///
    /// # 列为什么是**字符**列而不是字节列
    ///
    /// 一个汉字在 UTF-8 里占 3 字节。按字节算，「你好」的第二个字
    /// 会显示成「列 4」——用户数着自己的光标位置会数出「2」。
    /// 原型用 JS 字符串的 `slice`，天然是 UTF-16 码元口径，
    /// 对 BMP 内的汉字等价于字符列。
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let starts = crate::presentation::views::scroll_sync::line_start_offsets(&self.editor_text);
        // 光标可能落在非字符边界：`clamp_to` 只按**长度**夹紧，
        // 而程序化设值（跳转、测试）能给出任何一个字节偏移。
        // 直接切片会 panic（`panic = "abort"` 下等于终止进程）。
        let cursor = floor_char_boundary(&self.editor_text, self.editor.cursor);
        let line = crate::presentation::views::scroll_sync::line_of_offset(&starts, cursor);
        let line_start = starts.get(line).copied().unwrap_or(0);
        // 行首也可能是非法边界（理论上不会：行首由 `split('\n')` 推出，
        // 必然落在字符边界上）。夹一次是为了不给「理论上」留 panic。
        let line_start = floor_char_boundary(&self.editor_text, line_start);
        let col = self.editor_text[line_start..cursor].chars().count() + 1;
        (line + 1, col)
    }

    /// 选区的行列范围（有选区时状态栏显示 `行 a, 列 b - 行 c, 列 d`）。
    pub fn selection_line_col(&self) -> Option<((usize, usize), (usize, usize))> {
        let (start, end) = self.editor.selection()?;
        if start == end {
            return None;
        }
        let starts = crate::presentation::views::scroll_sync::line_start_offsets(&self.editor_text);
        let at = |offset: usize| {
            let offset = floor_char_boundary(&self.editor_text, offset);
            let line = crate::presentation::views::scroll_sync::line_of_offset(&starts, offset);
            let line_start =
                floor_char_boundary(&self.editor_text, starts.get(line).copied().unwrap_or(0));
            let col = self.editor_text[line_start..offset].chars().count() + 1;
            (line + 1, col)
        };
        Some((at(start), at(end)))
    }

    /// 文档行数（状态栏右端「N 行」用）。
    pub fn current_line_count(&self) -> usize {
        self.editor_text.split('\n').count().max(1)
    }

    /// 当前文档统计（状态栏字数、统计浮层共用）。
    ///
    /// 没有活动文档时返回 `None` 而不是全零：全零会被显示成
    /// 「字数 0」，读起来像「文档是空的」而不是「没有文档」。
    pub fn current_stats(&self) -> Option<StatsView> {
        let id = self.active_doc?;
        self.services.documents.get_stats(GetStatsQuery { id }).ok()
    }

    /// 在打开的文档间循环切换（`NextTab` / `PrevTab`）。
    pub(crate) fn cycle_document(&mut self, delta: isize) {
        let open = self.open_document_list();
        if open.len() <= 1 {
            return;
        }
        let current = self
            .active_doc
            .and_then(|id| open.iter().position(|d| d.id == id));
        let len = open.len() as isize;
        let cur = current.unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len);
        self.switch_document(open[next as usize].id);
    }

    /// 跳转到第 `n` 个标签页（`Alt+1` ~ `Alt+9`，1-based）。
    pub(crate) fn goto_document(&mut self, n: usize) {
        let open = self.open_document_list();
        if n == 0 || n > open.len() {
            return;
        }
        self.switch_document(open[n - 1].id);
    }

    /// 把当前分割比例落盘到设置（拖拽结束时调用，避免每帧写文件）。
    ///
    /// 走 `preferences.set_editor`：它会校验并 `repository.save`。
    /// 读回当前编辑器设置、只改 `split_ratio`、其余原样回写，
    /// 保证不丢用户已设的缩进 / 自动换行 / 自动保存等。
    pub(crate) fn persist_split_ratio(&mut self) {
        let Ok(settings) = self.services.preferences.get_settings(GetSettingsQuery) else {
            return;
        };
        let mut editor = settings.editor;
        editor.split_ratio = self.ui.split_ratio;
        let _ = self
            .services
            .preferences
            .set_editor(SetEditorCommand { editor });
    }

    /// 把一次文本编辑（来自 egui 的 diff 或纯变换）写入领域。
    pub(crate) fn flush_edit(&mut self, edit: editor_ops::TextEdit) {
        let Some(id) = self.active_doc else { return };
        // 用字节区间切片必须合法——`diff_replace` 已保证 `byte_start ≤ byte_end`
        // 且落在旧文本边界内；但若 `editor_text` 已被并发改动则可能越界，
        // 这里用 `saturating` 防御（越界替换会 panic="abort" 终止进程）。
        let start = edit.byte_start.min(self.editor_text.len());
        let end = edit.byte_end.min(self.editor_text.len());
        let range = TextRange::new(ByteOffset::new(start), ByteOffset::new(end));
        let _ = self.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range,
            text: edit.inserted,
        });
        self.refresh_text();
        self.pending_cursor = Some((
            edit.cursor_char,
            edit.anchor_char.unwrap_or(edit.cursor_char),
        ));
        self.clear_undoer_next = true;
    }

    /// 替换源码的一段字节区间（S6-05：预览区点击任务复选框回写）。
    ///
    /// 与 [`Self::flush_edit`] 的区别：本方法**不移动光标也不设 pending**——
    /// 用户此刻的焦点在预览区，编辑区光标应保持原位。
    pub(crate) fn replace_source(&mut self, byte_start: usize, byte_end: usize, text: &str) {
        // 这里直接把 `id` 取出来用，避免后面再 `expect` 一次
        // （规范 §四-15 禁用 `expect()`，V11-12 裁决）。
        let Some(id) = self.active_doc else {
            return;
        };
        let len = self.editor_text.len();
        let start = byte_start.min(len);
        let end = byte_end.min(len).max(start);
        let _ = self.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
            text: text.to_string(),
        });
        self.refresh_text();
        self.clear_undoer_next = true;
    }

    /// 当前文档所在目录（用于解析预览区图片的相对路径）。
    ///
    /// 未保存的草稿没有目录 → 返回 `None`，此时相对路径图片无法解析，
    /// 预览显示占位而非报错。
    pub(crate) fn document_dir(&self) -> Option<std::path::PathBuf> {
        let raw = self.current_document_path();
        if raw.is_empty() || raw == "未命名文档" {
            return None;
        }
        let path = std::path::Path::new(&raw);
        path.parent().map(|p| p.to_path_buf())
    }

    /// 整篇替换（缩进 / 反缩进 / 移动行这类**全文本变换**走这里）。
    fn replace_all(&mut self, new_text: String) {
        let Some(id) = self.active_doc else { return };
        let range = TextRange::new(ByteOffset::new(0), ByteOffset::new(self.editor_text.len()));
        let _ = self.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range,
            text: new_text,
        });
        self.refresh_text();
        self.clear_undoer_next = true;
    }

    /// 撤销一步（领域权威）。
    pub fn undo_edit(&mut self) {
        let Some(id) = self.active_doc else { return };
        let _ = self.services.documents.undo(UndoCommand { id });
        self.sync_cursor_from_domain();
        self.clear_undoer_next = true;
    }

    /// 重做一步（领域权威）。
    pub fn redo_edit(&mut self) {
        let Some(id) = self.active_doc else { return };
        let _ = self.services.documents.redo(RedoCommand { id });
        self.sync_cursor_from_domain();
        self.clear_undoer_next = true;
    }

    /// 撤销/重做后把 egui 光标对齐到领域光标（含选区）。
    fn sync_cursor_from_domain(&mut self) {
        let Some(id) = self.active_doc else { return };
        // 必须先回读文本：撤销 / 重做改的是**领域内容**，
        // 而 `editor_text` 是它的 UI 副本。不同步的话，
        // 撤销后编辑区仍显示撤销前的文本，下一次输入会把它覆盖回去
        // —— 表现为「Ctrl+Z 之后内容又回来了」。
        // （S6-07 之前不容易发现：那时只有输入类编辑，
        //  撤销后紧接着的操作通常就是输入，两者恰好抵消。）
        self.refresh_text();
        if let Ok(view) = self
            .services
            .documents
            .get_document(GetDocumentQuery { id })
        {
            let cursor = editor_ops::byte_to_char(&self.editor_text, view.cursor);
            let anchor = view
                .selection
                .map(|r| editor_ops::byte_to_char(&self.editor_text, r.start))
                .unwrap_or(cursor);
            self.pending_cursor = Some((cursor, anchor));
            self.editor.move_cursor(cursor); // 先把 UI 状态也对齐，避免后续读到的偏移错位
        }
    }

    /// 选区覆盖到的字符区间（无选区为光标处单点）。
    fn selection_chars(&self) -> (usize, usize) {
        match self.editor.selection() {
            Some((s, e)) => (
                editor_ops::byte_to_char(&self.editor_text, s),
                editor_ops::byte_to_char(&self.editor_text, e),
            ),
            None => {
                let c = editor_ops::byte_to_char(&self.editor_text, self.editor.cursor);
                (c, c)
            }
        }
    }

    /// 缩进选区（或光标所在行）。
    ///
    /// 走应用层 `format`（S6-07）而非本地拼接：缩进规则归领域
    /// `EditingService` 所有，这里只负责把「字符选区」翻译成字节区间。
    pub fn indent_selection(&mut self) {
        self.run_format(FormatKind::Indent);
    }

    /// 反缩进选区（或光标所在行）。
    pub fn outdent_selection(&mut self) {
        self.run_format(FormatKind::Outdent);
    }

    // ————————————————————————————————————————————————————————————
    // 查找替换（S6-08）
    // ————————————————————————————————————————————————————————————

    /// 按当前查询 / 选项重扫文档，并把命中写入面板状态。
    ///
    /// 所有会改变「查询 × 选项 × 内容」三者之一的操作 afterwards 都必须调它，
    /// 否则面板显示的 `n/m` 与实际能跳转到的位置不符——这类不一致
    /// 表现为「点下一个跳到了一个奇怪的地方」，极难定位。
    pub fn sync_find(&mut self) {
        let text = self.current_content().unwrap_or_default();
        self.find.rescan(&text);
    }

    /// 跳到下一个 / 上一个命中（`forward = true` 为下一个），并选中它。
    ///
    /// 选中而非只滚动：用户接着多半要按 `Ctrl+H` 替换当前这条，
    /// 选区就是「当前命中」的定义。反之若只滚动光标，用户无法知道
    /// 「当前」是哪一条。
    pub fn find_step(&mut self, forward: bool) {
        if self.find.finder.is_none() {
            // 没扫过就先扫一次：面板刚打开、用户还没动输入框就按 F3
            // 是常见操作（沿用上次的查询继续找）。
            self.sync_find();
        }
        let Some(found) = self.find.step(forward) else {
            return;
        };
        self.reveal_match(found);
    }

    /// 选中某个命中并把它滚进视口。
    fn reveal_match(&mut self, found: crate::presentation::state::find_replace_ui_state::Match) {
        self.editor.set_selection(found.start, found.end);
        // egui 的 `TextEdit` 记的是**字符**位置，不设的话本帧
        // `TextEdit` 读回自己的旧光标，又把我们的选区覆盖掉。
        let (cs, ce) = (
            editor_ops::byte_to_char(&self.editor_text, found.start),
            editor_ops::byte_to_char(&self.editor_text, found.end),
        );
        self.pending_cursor = Some((ce, cs));
        // 滚动到可见：命中在文档很后面时，不滚屏的话「计数显示 8/12」
        // 而光标还在开头，用户完全看不出跳到了哪。
        self.scroll_to_offset(found.start);
    }

    /// 把某个字节偏移滚进编辑区视口（已在视口内则不动）。
    ///
    /// 复用 S6-06 的行顶表（[`mapper::line_tops`]）把字节偏移换成 y，
    /// 再用与滚动同步同一套夹紧，避免「跳转后又被同步逻辑拖回去」。
    pub fn scroll_to_offset(&mut self, byte_offset: usize) {
        let theme = self.theme().clone();
        let Some(tops) = mapper::line_tops(&theme, &self.editor_text) else {
            // 行布局缓存未命中（首帧 / 刚换文档）：不猜行高——
            // 猜了会让跳转停在一个看似合理实则错误的位置。
            return;
        };
        let starts = crate::presentation::views::scroll_sync::line_start_offsets(&self.editor_text);
        let line = crate::presentation::views::scroll_sync::line_of_offset(&starts, byte_offset);
        let Some(&top) = tops.get(line) else {
            return;
        };
        let bottom = tops.get(line + 1).copied().unwrap_or(top);
        let geom = self.ui.sync.geometry(Side::Editor);
        if geom.viewport_h <= 0.0 {
            return;
        }
        let view_top = self.editor.scroll_y;
        let view_bottom = view_top + geom.viewport_h;
        // 已在视口内就**不动**：每次跳转都把命中顶到视口上部会让长文档里
        // 连续「下一个」时画面剧烈跳动，反而更难看清自己在哪。
        // 只有完全看不见（命中是折行成多行的高亮段落时会超出视口）才滚。
        if top >= view_top && bottom <= view_bottom {
            return;
        }
        // 目标：让该行落在视口**上部**（留 1/4 屏余量），
        // 而不是刚好贴边——贴边时用户分不清「跳到这里了」还是「本来就在这」。
        let target = top - geom.viewport_h * 0.25;
        let clamped = crate::presentation::views::scroll_sync::clamp_scroll(target, geom);
        if (clamped - self.editor.scroll_y).abs() > 0.5 {
            self.editor.scroll_y = clamped;
        }
    }

    /// 替换当前命中并前进到下一条（`Ctrl+H` 面板的「替换」按钮）。
    pub fn find_replace_current(&mut self) {
        let Some(found) = self.find.current_match() else {
            return;
        };
        let replacement = self.find.replace_text.clone();
        let len = replacement.len();
        self.replace_match(found, &replacement);
        // 光标落在替换文本之后（与原型 `replaceCurrent` 一致：
        // `at = m.start + replaceText.length`），并选中新插入的内容——
        // 用户连点几次时，每次替换的是「刚插进去的那段」而不是跳过它。
        let at = found.start + len;
        self.editor.set_selection(at, at);
        self.pending_cursor = Some((
            editor_ops::byte_to_char(&self.editor_text, at),
            editor_ops::byte_to_char(&self.editor_text, at),
        ));
        self.sync_find();
        // 替换后内容长度变了，命中列表整体左移；重新选中「原位置之后的
        // 第一条」才能继续替换下一条。不重选的话用户点第二次「替换」
        // 会替换到已经替换过的那条上（或干脆没反应）。
        if let Some(next) = self.find.step(true) {
            self.reveal_match(next);
        }
    }

    /// 全部替换。
    ///
    /// 走**一次** `replace_range(0..len)` 而不是逐条替换：后者会在撤销栈里
    /// 留下 N 个撤销点，用户按一次 Ctrl+Z 只撤掉一处，还得按 N 次——
    /// 而「全部替换」在用户心智里是**一个**动作。
    pub fn find_replace_all(&mut self) {
        let Some(finder) = self.find.finder.clone() else {
            return;
        };
        let text = self.current_content().unwrap_or_default();
        let summary = finder.preview_replace_all(&text, &self.find.replace_text);
        if summary.count == 0 {
            return;
        }
        self.find.last_replace_count = Some(summary.count);
        self.find.last_replace_grows = summary.will_match_again;
        self.replace_all(summary.result);
        // 替换后重扫：命中列表已完全失效（长度都变了）。
        self.sync_find();
    }

    /// 把一条命中替换掉（写领域 + 刷新副本）。
    fn replace_match(&mut self, found: Match, replacement: &str) {
        let Some(id) = self.active_doc else {
            return;
        };
        let range = TextRange::new(ByteOffset::new(found.start), ByteOffset::new(found.end));
        if self
            .services
            .documents
            .replace_range(ReplaceRangeCommand {
                id,
                range,
                text: replacement.to_string(),
            })
            .is_err()
        {
            return;
        }
        self.refresh_text();
        // 查找替换走领域撤销栈 → egui 内置栈必须让位，
        // 否则 Ctrl+Z 会先撤掉 egui 自己的输入历史，替换撤不回来。
        self.clear_undoer_next = true;
    }

    // ---------- 设置（S6-11） ----------

    /// 当前设置快照（「取消」回滚的基准）。
    pub fn settings_snapshot(&self) -> Snapshot {
        let view = self.settings();
        Snapshot {
            theme_kind: view.theme.kind.clone(),
            font_family: view.theme.font_family.clone(),
            font_size: view.theme.font_size,
            line_height: view.theme.line_height,
            tab_size: view.editor.tab_size,
            word_wrap: view.editor.word_wrap,
            auto_save_interval: view.editor.auto_save_interval,
            spell_check: view.editor.spell_check,
            show_line_numbers: view.editor.show_line_numbers,
        }
    }

    /// 打开设置对话框（记录回滚基准）。
    pub(crate) fn open_settings(&mut self) {
        let snapshot = self.settings_snapshot();
        self.settings_ui.open(snapshot);
    }

    /// 写一个主题字段并立即生效（实时预览，S6-11 验收要求）。
    ///
    /// 失败时把错误记进设置状态而不是静默：领域会拒绝非法字号 / 行高，
    /// 静默吞掉的表现是「拖了滑块但字号没变」，用户无从判断是 bug 还是自己操作错了。
    pub fn apply_theme_field(&mut self, mutate: impl FnOnce(&mut ThemeView)) {
        let mut view = self.settings().theme;
        mutate(&mut view);
        match self
            .services
            .preferences
            .set_theme(SetThemeCommand { theme: view })
        {
            Ok(()) => {
                self.settings_ui.error = None;
                self.settings_ui.dirty = true;
                self.ui
                    .set_theme(builtin::resolve(&self.settings().theme, self.system_dark));
            }
            Err(e) => self.settings_ui.set_error(e.to_string()),
        }
    }

    /// 写一个编辑器设置字段并立即生效。
    pub fn apply_editor_field(&mut self, mutate: impl FnOnce(&mut EditorSettingsView)) {
        let mut view = self.settings().editor;
        mutate(&mut view);
        match self
            .services
            .preferences
            .set_editor(SetEditorCommand { editor: view })
        {
            Ok(()) => {
                self.settings_ui.error = None;
                self.settings_ui.dirty = true;
            }
            Err(e) => self.settings_ui.set_error(e.to_string()),
        }
    }

    /// 「恢复本组默认」：只重置**当前分区**（原型缺陷 3.6 根治）。
    pub fn reset_current_settings_section(&mut self) {
        let Some(section) = self.settings_ui.section.resettable_section() else {
            return;
        };
        match self
            .services
            .preferences
            .reset_section(ResetSectionCommand { section })
        {
            Ok(()) => {
                self.settings_ui.error = None;
                self.settings_ui
                    .set_notice(format!("已恢复「{}」默认值", section.label()));
                self.settings_ui.dirty = true;
                // 主题重置会改变生效主题，必须同步到 egui。
                self.ui
                    .set_theme(builtin::resolve(&self.settings().theme, self.system_dark));
                self.ui.set_split_ratio(self.settings().editor.split_ratio);
            }
            Err(e) => self.settings_ui.set_error(e.to_string()),
        }
    }

    /// 「恢复默认快捷键」（快捷键分区的专属按钮）。
    ///
    /// 走分区重置而不是全量：全量会把用户改过的主题与缩进一起打回。
    pub fn reset_all_shortcuts(&mut self) {
        if let Err(e) = self
            .services
            .preferences
            .reset_section(ResetSectionCommand {
                section: SettingsSection::Shortcuts,
            })
        {
            self.settings_ui.set_error(e.to_string());
            return;
        }
        self.settings_ui.shortcut_rollback.clear();
        self.settings_ui.error = None;
        self.settings_ui.set_notice("已恢复默认快捷键");
        self.reload_shortcut_registry();
    }

    /// 「取消」：把**全部**未确认修改回滚到打开时的快照。
    ///
    /// 验收标准原文是「取消回滚全部未确认修改」——所以这里必须覆盖
    /// 主题、编辑器设置与快捷键三类，只回滚其中两类会让用户以为取消失败了。
    pub fn cancel_settings(&mut self) {
        let Some(snapshot) = self.settings_ui.take_snapshot() else {
            return;
        };
        let theme = ThemeView {
            kind: snapshot.theme_kind,
            font_family: snapshot.font_family,
            font_size: snapshot.font_size,
            line_height: snapshot.line_height,
            custom: None,
        };
        let editor = EditorSettingsView {
            tab_size: snapshot.tab_size,
            word_wrap: snapshot.word_wrap,
            auto_save_interval: snapshot.auto_save_interval,
            spell_check: snapshot.spell_check,
            show_line_numbers: snapshot.show_line_numbers,
            split_ratio: self.settings().editor.split_ratio,
            // 设置页尚未提供写作目标输入框，保存时沿用当前值，避免被 0 覆盖。
            writing_goal: self.settings().editor.writing_goal,
        };
        // 主题与编辑器各自独立失败：主题写失败不该让编辑器也回滚不了。
        let mut errors: Vec<String> = Vec::new();
        if let Err(e) = self
            .services
            .preferences
            .set_theme(SetThemeCommand { theme })
        {
            errors.push(e.to_string());
        }
        if let Err(e) = self
            .services
            .preferences
            .set_editor(SetEditorCommand { editor })
        {
            errors.push(e.to_string());
        }
        for (action, combo) in self.settings_ui.take_shortcut_rollback() {
            // 纯 UI 动作（如 `Revert`）没有领域对应项，跳过而不是报错——
            // 它本来就不可能被改键（设置页只列出可持久化动作）。
            let Some(domain) = domain_action(action) else {
                continue;
            };
            if let Err(e) = self.services.preferences.set_shortcut(SetShortcutCommand {
                action: domain,
                combo,
            }) {
                errors.push(e.to_string());
            }
        }
        self.settings_ui.error = (!errors.is_empty()).then(|| errors.join("；"));
        self.settings_ui.close();
        self.ui
            .set_theme(builtin::resolve(&self.settings().theme, self.system_dark));
        self.reload_shortcut_registry();
        // 行号栏的会话开关同样回到回滚基准（与主题/快捷键同理由：取消即还原）。
        self.editor.line_numbers = self.settings().editor.show_line_numbers;
        // **在这里**关浮层，而不是让调用方关：三条关闭路径
        // （取消按钮 / 标题栏 × / Esc）必须得到同样结果。
        // 放在调用方的话，Esc 那条路径（`CloseOverlay` 直接
        // `dialog.close_top`）会跳过回滚——用户改了字号按 Esc，
        // 面板关了但字号没回去，且**没有任何提示**。
        self.dialog.close(Overlay::Settings);
    }

    /// 「保存」：确认当前修改并关闭对话框。
    ///
    /// 刻意**不做任何写盘**：每个字段在改动时就已经落盘（实时预览的要求），
    /// 保存只是「关闭」这一下点击。若在这里再写一次，就出现两份落盘路径，
    /// 将来某条路径漏了字段会以「改了没生效」的形式暴露。
    pub fn confirm_settings(&mut self) {
        self.settings_ui.close();
        self.reload_shortcut_registry();
        self.dialog.close(Overlay::Settings);
    }

    /// 快捷键改绑（设置页录制路径）。
    ///
    /// 冲突由应用层 `set_shortcut` 拒绝并返回 `Err`，这里原样转成可见提示——
    /// 原型缺陷：「静默写入后被注册顺序在前的同名动作吞掉，界面毫无提示」。
    pub fn rebind_shortcut(&mut self, action: UiAction, combo: &str) {
        let Some(domain_action) = domain_action(action) else {
            self.settings_ui
                .set_error(format!("{action:?} 尚未映射到领域动作，无法改键"));
            return;
        };
        // 写之前记下原键位——**必须在这里**，而不是让调用方（视图）先记。
        //
        // 放在视图里的后果：任何绕过视图的调用方（命令面板、将来的导入功能、
        // 集成测试）改了键却没登记回滚，「取消」就静默地不回滚它。
        // 症状是「按了取消，某个键位没回来」——用户无从判断是不是自己记错了。
        let original = self
            .settings()
            .shortcuts
            .iter()
            .find(|s| s.key == domain_action.key())
            .map(|s| s.combo.clone())
            .unwrap_or_default();
        self.settings_ui.remember_shortcut(action, original);
        match self.services.preferences.set_shortcut(SetShortcutCommand {
            action: domain_action,
            combo: combo.to_string(),
        }) {
            Ok(()) => {
                self.settings_ui.error = None;
                self.settings_ui.recording = None;
                self.settings_ui.pending_conflict = None;
                self.settings_ui.dirty = true;
                self.reload_shortcut_registry();
            }
            Err(e) => self.settings_ui.set_error(e.to_string()),
        }
    }

    /// 从持久化配置重建快捷键注册表（改键后调用）。
    ///
    /// 不重建的后果：注册表还是旧的，用户在设置页看到新键位，
    /// 按下去却走旧动作——**界面上写着和实际生效的不一致**。
    pub fn reload_shortcut_registry(&mut self) {
        let settings = self.settings();
        self.ui
            .dispatcher
            .set_registry(ShortcutRegistry::from_settings(&settings.shortcuts));
    }

    /// 执行一个格式动作（S6-07）。
    ///
    /// 这是**三入口的唯一汇聚点**：工具栏按钮、快捷键分发、命令面板
    /// 都以 `UiAction` 为参数进入 [`EditorApp::execute`]，再由它转到此处。
    /// 因此「三入口一致」不是靠三处都写对，而是**按构造**成立。
    pub fn apply_format(&mut self, action: UiAction) {
        let kind = match action {
            UiAction::Bold => FormatKind::Bold,
            UiAction::Italic => FormatKind::Italic,
            UiAction::Strikethrough => FormatKind::Strikethrough,
            UiAction::InlineCode => FormatKind::InlineCode,
            UiAction::InsertLink => FormatKind::InsertLink,
            UiAction::CodeBlock => FormatKind::InsertCodeBlock,
            UiAction::InsertTable => FormatKind::InsertTable,
            UiAction::ToggleQuote => FormatKind::Quote,
            UiAction::ToggleList => FormatKind::BulletList,
            UiAction::ToggleOrderedList => FormatKind::OrderedList,
            UiAction::PromoteHeading => FormatKind::PromoteHeading,
            UiAction::DemoteHeading => FormatKind::DemoteHeading,
            UiAction::ToggleTask => FormatKind::ToggleTask,
            // 缩进已由 `Indent` / `Outdent` 动作直接处理，不走这里。
            _ => return,
        };
        self.run_format(kind);
    }

    /// 调用应用层格式用例并落地结果（新文本 + 选区）。
    fn run_format(&mut self, kind: FormatKind) {
        let Some(id) = self.active_doc else {
            return;
        };
        // UI 用字符索引、应用层用字节偏移，交接处必须换算
        // （`İ`、emoji、组合字符都会让两者不等）。
        let (s, e) = self.selection_chars();
        let range = TextRange::new(
            ByteOffset::new(editor_ops::char_to_byte(&self.editor_text, s)),
            ByteOffset::new(editor_ops::char_to_byte(&self.editor_text, e)),
        );

        let Ok(result) = self.services.documents.format(FormatCommand {
            id,
            kind,
            range,
            indent_unit: self.tab_unit(),
        }) else {
            // 文档已被关闭等异常：静默放弃，不打断当前帧。
            // 格式是增强功能，不该让整个编辑器崩掉。
            return;
        };
        if !result.changed {
            return; // 无效果编辑：不写文本、不重排高亮与预览
        }

        // `format` 已经把改动写进领域，这里只需把内容与选区同步到 UI。
        self.editor_text = result.text;
        let sel = &result.selection;
        let ns = editor_ops::byte_to_char(&self.editor_text, sel.start.min(self.editor_text.len()));
        let ne = editor_ops::byte_to_char(&self.editor_text, sel.end.min(self.editor_text.len()));
        self.editor.cursor = editor_ops::char_to_byte(&self.editor_text, ns);
        self.editor.anchor = Some(editor_ops::char_to_byte(&self.editor_text, ne));
        self.pending_cursor = Some((ns, ne));
        // 格式改动同样进领域撤销栈，故 egui 内置栈必须让位（与普通编辑一致）。
        self.clear_undoer_next = true;
    }

    /// 上移/下移光标所在行（`dir = -1` 上移，`+1` 下移）。
    pub fn move_line(&mut self, dir: i8) {
        let cursor = editor_ops::byte_to_char(&self.editor_text, self.editor.cursor);
        if let Some((new_text, new_cursor)) = editor_ops::move_line(&self.editor_text, cursor, dir)
        {
            self.replace_all(new_text);
            self.pending_cursor = Some((new_cursor, new_cursor));
        }
    }

    /// 把上一帧领域编辑留下的「重设光标 / 清撤销栈」意图落到 egui 内部状态。
    ///
    /// 必须在 `TextEdit::show` **之前**调用（写入的状态会在本帧 show 时被读走）。
    /// 放在编辑区视图顶部，由视图持 `ctx` 调用。
    pub fn apply_pending_editor_state(&mut self, ctx: &egui::Context) {
        if self.clear_undoer_next
            && let Some(mut state) = egui::TextEdit::load_state(ctx, Self::editor_id())
        {
            self.clear_undoer_next = false;
            state.clear_undoer();
            state.store(ctx, Self::editor_id());
        }
        if let Some((cursor, anchor)) = self.pending_cursor.take()
            && let Some(mut state) = egui::TextEdit::load_state(ctx, Self::editor_id())
        {
            let primary = egui::text::CCursor {
                index: cursor,
                prefer_next_row: false,
            };
            let secondary = egui::text::CCursor {
                index: anchor,
                prefer_next_row: false,
            };
            state.cursor.set_char_range(Some(egui::text::CCursorRange {
                primary,
                secondary,
                h_pos: None,
            }));
            state.store(ctx, Self::editor_id());
        }
    }

    /// 主题设置（`None` 表示运行期读取失败，保持当前主题不变）。
    fn theme_view(&self) -> Option<ThemeView> {
        match self.services.preferences.get_settings(GetSettingsQuery) {
            Ok(settings) => Some(settings.theme),
            Err(_) => None,
        }
    }

    /// 刷新「跟随系统」主题。
    ///
    /// 只有设置确实是 `system` 时才重算。若不加这个判断，用户**显式**选定的
    /// 深/浅主题会被系统外观变化悄悄改回去——这是「跟随系统」最常见的实现错误。
    fn refresh_system_theme(&mut self, ctx: &egui::Context) {
        if self.ui.theme.kind != BuiltinTheme::System {
            return;
        }
        // egui 每帧把系统偏好写进 `visuals.dark_mode`（随系统外观变化更新），
        // 直接读它即可，不必引入额外的探测 crate。
        let system_dark = ctx.style().visuals.dark_mode;
        if system_dark == self.system_dark {
            return;
        }
        self.system_dark = system_dark;
        let Some(view) = self.theme_view() else {
            return;
        };
        self.ui.set_theme(builtin::resolve(&view, system_dark));
    }

    /// 按原型顺序轮换主题：`深色 → 浅色 → 跟随系统 → 深色`。
    ///
    /// # 必须是**固定序列**，不能按当前值推导下一态
    ///
    /// 原型 `THEME_CYCLE = ['dark', 'light', 'system']`，即「下一态」只取决于
    /// 当前态在序列中的位置。若改成 `dark→system、light→dark、system→light`
    /// 这类按值推导的映射，用户从「跟随系统」点一下得到的会是**浅色**而不是深色——
    /// 与原型不一致，且用户按第二次时回到的位置也不同。
    ///
    /// 循环而非二元切换：「跟随系统」一旦不可达，用户就再也回不到跟随系统的状态。
    fn cycle_theme(&mut self) {
        /// 轮换序列，**顺序即语义**，不要重排。
        const CYCLE: [&str; 3] = ["dark", "light", "system"];
        let settings = self.settings();
        let current = settings.theme.kind.as_str();
        let index = CYCLE.iter().position(|c| *c == current).unwrap_or(0);
        let mut view = settings.theme;
        view.kind = CYCLE[(index + 1) % CYCLE.len()].to_string();
        // 写盘失败不回滚内存状态：主题是**显示偏好**而非数据，
        // 拦住用户切换只会让界面「看起来没反应」。
        let _ = self.services.preferences.set_theme(SetThemeCommand {
            theme: view.clone(),
        });
        self.ui.set_theme(builtin::resolve(&view, self.system_dark));
    }

    /// 打开某个浮层并做该浮层特有的初始化。
    ///
    /// 「特有初始化」目前只有查找面板需要（`Replace` 要展开替换框、
    /// `Find` 要收起），但把它放在一处而不是散在 `execute` 的分支里，
    /// 是为了让「打开浮层」这件事只有**一个**入口——将来加遮罩、
    /// 动画、焦点陷阱都在这里加，不会漏。
    fn open_overlay(&mut self, overlay: Overlay, action: UiAction) {
        self.dialog.open(overlay);
        // 打开面板即重新扫描：文档内容可能在面板打开期间被改过
        // （面板非模态，用户可以切标签、改内容），沿用旧命中会跳到错误位置。
        if overlay == Overlay::Export {
            // 建议文件名取当前文档标题去扩展名；导出主题取当前生效主题的种类。
            let stem = self
                .active_document_title_stem()
                .unwrap_or_else(|| "document".to_string());
            let theme_kind = match self.ui.theme.kind {
                crate::presentation::theme::builtin::BuiltinTheme::Dark => "dark",
                crate::presentation::theme::builtin::BuiltinTheme::Light => "light",
                crate::presentation::theme::builtin::BuiltinTheme::System => "system",
            };
            self.export_ui.theme = theme_kind.to_string();
            self.export_ui.file_stem = stem;
            // 默认输出目录取文档所在目录；未保存的新文档没有目录，
            // 此时留空并由 `can_submit` 拦下，提示用户先保存或填目录。
            self.export_ui.output_dir = self.active_document_dir().unwrap_or_default();
            self.export_ui.open();
        }
        if overlay == Overlay::Settings {
            // 必须在这里取快照：「取消」要回到的是**打开那一刻**的值，
            // 而设置字段是实时落盘的（实时预览要求），等到视图里再取
            // 就取到的是「用户已经改过」的值，回滚等于没回滚。
            self.open_settings();
        }
        if overlay == Overlay::Find {
            // `Ctrl+F` 在面板已打开时**不**收起替换框：用户的意图是
            // 「回到查找框继续找」，把它变回单行反而是刁难。
            self.find
                .open_panel(self.find.replace_open || action == UiAction::Replace);
            self.sync_find();
        }
        // S6-11f：以下四个浮层有「打开即重置」的状态。
        //
        // 放在**打开那一刻**重置而不是视图里，否则用户第二次打开
        // 会看到上一次的过滤词 / 搜索词 / 选中项——而他从没在
        // 这个面板里做过那个操作。
        if overlay == Overlay::QuickOpen {
            self.quick_open_ui.open();
        }
        if overlay == Overlay::CommandPalette {
            // 命令面板同样「打开即重置」：上一次的查询词对
            // 这次打开毫无意义（面板语境每次都不同）。
            self.command_palette_ui.open();
        }
        if overlay == Overlay::ShortcutOverview {
            self.shortcut_overview_ui.open();
        }
        if overlay == Overlay::Symbol {
            self.symbol_ui.open();
        }
        if overlay == Overlay::Diff {
            // 保留上一次选的对照文件：用户对照两个文件看了半天，
            // 关掉再打开大概率还是想看同一对。**结果**行会重算，
            // 因为左侧内容可能已经变了。
            if self.diff_ui.right_path.is_none() {
                self.diff_ui.error = None;
            }
        }
    }

    /// 执行一个动作。
    ///
    /// 三个入口（菜单 / 快捷键 / 命令面板）**全部**汇聚到这里，
    /// 因此不可能出现「菜单里有的操作，快捷键触发的结果不一样」。
    ///
    /// 未接线动作用 `_ => {}` 兜底而非 `unreachable!()`：
    /// 动作表先于视图实现（动作表有 60 项，视图分批落地），
    /// 若在这里 panic，用户按下任何尚未接线的快捷键都会**直接丢掉窗口**。
    pub fn execute(&mut self, action: UiAction) {
        // 浮层类动作先判：它们占 14 项，逐个写 match 分支会让下面的
        // 布局分支被淹没。判定用两张表的**并集**（`opened_by` 与
        // `also_opened_by`），二者都在 `Overlay` 上，不可能算出不一致的结果。
        if let Some(overlay) = openable_overlay(action) {
            self.open_overlay(overlay, action);
            return;
        }

        match action {
            // —— 文件 ——
            // `Save` / `NewDocument` / `SaveAs` 此前落在 `_ => {}`：动作表里有、
            // 快捷键能触发，但没有任何分支处理，`Ctrl+S` / `Ctrl+N` /
            // `Ctrl+Shift+S` 全部静默失效。状态栏的「未保存 → 点击保存」
            // 与欢迎页的三个主操作也走这里，故**必须**接线，
            // 不能让各视图各写一套保存逻辑。
            UiAction::Save => self.run_save(false),
            UiAction::SaveAs => self.run_save(true),
            UiAction::NewDocument => self.run_new_document(),
            // 「打开文件夹」：请求工作区目录选择（后台对话框），结果由
            // `apply_dialog_outcome` 扫盘装载。入口从侧栏上移到「文件」菜单后，
            // 菜单 / 命令面板仍共用这一条路径（三入口一致）。
            UiAction::OpenFolder => self.request_dialog(PendingDialog::PickFolder),
            // `OpenDocument` **故意不在这里**：`Ctrl+O` 是快速打开浮层的
            // 开启动作（`Overlay::QuickOpen.opened_by`），已经被上面的
            // `openable_overlay` 分支拦走。若在此再写一个 `request_dialog`，
            // 它永远不可达——一段**读起来像接线、实际是死代码**的分支，
            // 比没有分支更容易误导（它会让「Ctrl+O 打不开文件」这件事
            // 看起来已经处理过了）。系统文件对话框的入口在快速打开
            // 面板内的「浏览本机文件…」按钮。
            // —— 布局 ——
            UiAction::ToggleSidebar => self.ui.toggle_sidebar(),
            UiAction::TogglePreview => self.ui.toggle_preview(),
            UiAction::ToggleStatusbar => self.ui.toggle_statusbar(),
            UiAction::ToggleOutline => self.ui.toggle_sidebar_tab(SidebarTab::Outline),
            // `FindInFiles`（Ctrl+Shift+F，全局「在文件中查找」）此前落在 `_ => {}`：
            // 动作表里有、快捷键能触发，但没有任何分支处理，于是按下去毫无反应。
            // 它**不**打开浮层——全局搜索是侧栏的 Search 标签，故直接切到该标签
            // 并展开侧栏，与 `ToggleOutline` 同一思路。
            UiAction::FindInFiles => {
                self.ui.toggle_sidebar_tab(SidebarTab::Search);
            }
            UiAction::ToggleFocusMode => self.ui.toggle_focus_mode(),
            // —— 标签 ——
            UiAction::CloseTab => self.close_active_tab(),
            UiAction::NextTab => self.cycle_document(1),
            UiAction::PrevTab => self.cycle_document(-1),
            UiAction::GotoTab1 => self.goto_document(1),
            UiAction::GotoTab2 => self.goto_document(2),
            UiAction::GotoTab3 => self.goto_document(3),
            UiAction::GotoTab4 => self.goto_document(4),
            UiAction::GotoTab5 => self.goto_document(5),
            UiAction::GotoTab6 => self.goto_document(6),
            UiAction::GotoTab7 => self.goto_document(7),
            UiAction::GotoTab8 => self.goto_document(8),
            UiAction::GotoTab9 => self.goto_document(9),
            // —— 主题 ——
            UiAction::ToggleTheme => self.cycle_theme(),
            // —— 编辑（领域权威：撤销/重做走领域，不走 egui 内置）——
            UiAction::Undo => self.undo_edit(),
            UiAction::Redo => self.redo_edit(),
            UiAction::Indent => self.indent_selection(),
            UiAction::Outdent => self.outdent_selection(),
            UiAction::MoveLineUp => self.move_line(-1),
            UiAction::MoveLineDown => self.move_line(1),
            // —— 格式（S6-07）——
            // 全部汇入 `apply_format`：三入口（工具栏 / 快捷键 / 命令面板）
            // 都经由 `execute` 落到这里，故行为**按构造**一致，
            // 不依赖「记得三处都写对」。
            UiAction::Bold
            | UiAction::Italic
            | UiAction::Strikethrough
            | UiAction::InlineCode
            | UiAction::InsertLink
            | UiAction::CodeBlock
            | UiAction::InsertTable
            | UiAction::ToggleQuote
            | UiAction::ToggleList
            | UiAction::ToggleOrderedList
            | UiAction::PromoteHeading
            | UiAction::DemoteHeading
            | UiAction::ToggleTask => self.apply_format(action),
            // —— 关闭浮层 ——
            // Esc 的仲裁点在浮层状态里：先给浮层一次机会关自己，
            // 没有浮层可关时才退出专注模式。否则 Esc 会「关掉对话框顺便
            // 把专注模式也退掉」——一次按键做两件事几乎总是 bug。
            UiAction::CloseOverlay if self.dialog.is_open(Overlay::Settings) => {
                // Esc 语义 = 取消：与面板上的「取消」按钮、与标题栏 × 一致。
                self.cancel_settings();
            }
            UiAction::CloseOverlay if self.dialog.close_top().is_none() => {
                self.ui.exit_focus_mode();
            }
            // 有浮层被关掉时落到这里：什么都不做。
            UiAction::CloseOverlay => {}
            // —— 其余动作在对应子任务中接线 ——
            _ => {}
        }
    }

    /// 收集本帧触发的动作。
    ///
    /// 用 `i.events` 而非 `i.keys_down`：`keys_down` 是**当前按住**的集合，
    /// 长按会在每一帧都报告一次，用它会导致「按住 Ctrl+S 连存几十次」。
    /// `events` 只含本帧新增的按键事件，且分发器已过滤 `repeat`。
    ///
    /// # 为什么是 `pub`
    ///
    /// 它是「按键事件 → 动作」这条通路的**唯一**实现，`update` 与测试共用。
    /// 曾经它是私有的，于是集成测试只能自己在测试侧重写一遍
    /// 「焦点在输入框时让位」的规则——重写出来的副本与生产代码各改各的，
    /// 变异测试立刻证明了这一点：把生产代码里的让位逻辑删掉，
    /// 测试仍然全绿（它验证的是自己的副本）。
    ///
    /// 公开它，测试就能走**真实**通路，删掉生产逻辑必然变红。
    pub fn collect_actions(&mut self, ctx: &egui::Context) -> Vec<UiAction> {
        self.ui.dispatcher.set_modal(self.dialog.is_modal());
        // 查找/替换输入框有焦点时，全局快捷键必须**让位**。
        //
        // 症状：用户在查找框里输入「Ctrl+B」想找字符串 "b"，
        // 结果文档里的字被加粗了——因为分发器只看按键事件，
        // 不管焦点在哪（编辑区的 `TextEdit` 也要靠快捷键，故不能一律
        // 用 `wants_keyboard_input()` 一刀切）。
        //
        // 只放行 `CloseOverlay`：Esc 必须仍然能关掉面板，
        // 否则用户会被困在查找框里（这与 S6-09「关浮层永远放行」同源）。
        let find_input_focused = ctx.memory(|m| {
            m.has_focus(egui::Id::new(FIND_INPUT_ID))
                || m.has_focus(egui::Id::new(REPLACE_INPUT_ID))
        });
        let mut out = Vec::new();
        for event in &ctx.input(|i| i.events.clone()) {
            if let Dispatch::Triggered(action) = self.ui.dispatcher.handle(event) {
                if find_input_focused && action != UiAction::CloseOverlay {
                    continue;
                }
                out.push(action);
            }
        }
        out
    }
}

/// `UiAction` → 领域 `ActionId`（改键持久化用）。
///
/// # 为什么放在组合根而不是注册表里
///
/// 注册表在 `presentation` 层，而 `ActionId` 在 `domain`——表现层不得依赖领域
/// （`build.rs` 架构守卫会拦）。而这层映射**本来就只在组合根用得上**
/// （`EditorApp::rebind_shortcut` 一处），放这里既合法又不会让表现层沾上领域类型。
///
/// 用 `persisted_key()` 逐字比对而不是写一张对照表：那张表一旦有动作漏登记，
/// 症状是「这个动作改了键却不生效」——零报错。对照表会与 `persisted_key`
/// 一致地漏，把两种机制的错误绑在一起。
fn domain_action(action: UiAction) -> Option<ActionId> {
    let key = action.persisted_key()?;
    ActionId::all()
        .iter()
        .copied()
        .find(|domain| domain.key() == key)
}

/// 从本帧事件里取出**第一张**回传的截图。
///
/// 取第一个而非按`user_data` 精确匹配：应用只有一个视口、一个在等的任务，
/// egui 也只会为我们的那一条命令回传一次。为一个不存在的并发场景
/// 引入匹配逻辑，只会在 `user_data` 序列化失败时静默丢图。
fn take_screenshot_event(ctx: &egui::Context) -> Option<std::sync::Arc<egui::epaint::ColorImage>> {
    ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    })
}

/// 父目录路径（末段之前）；无父目录时返回空串。
fn parent_of(path: &str) -> String {
    path.rfind('/')
        .map(|cut| path[..cut].to_string())
        .unwrap_or_default()
}

/// 把领域命中转成表现层的分组结构，并算出每条命中的高亮区间。
///
/// 高亮区间在**字符**坐标下算（理由同`scroll_sync` 那套换算）：
/// 片段要交给 egui 排版，按字节切中文会错位——「一」在UTF-8 里 3 字节，
/// 字节 7 落在字符 9 的中间，切出来的两半都不是完整字符。
///
/// 查询词与选项由调用方传入而不是从 `result` 里找：`SearchResultView`
/// 本身不携带查询（它是领域 DTO，不该知道 UI 传了什么）。
fn group_search_hits(
    result: &SearchResultView,
    query: &str,
    case_sensitive: bool,
) -> Vec<SearchGroup> {
    let rows: Vec<(String, usize, String)> = result
        .hits
        .iter()
        .map(|hit| (hit.path.as_str().to_string(), hit.line, hit.snippet.clone()))
        .collect();
    let mut groups = group_hits(&rows);
    for group in &mut groups {
        for hit in &mut group.hits {
            hit.highlight =
                locate_in_snippet(&hit.snippet, query, case_sensitive).unwrap_or((0, 0));
        }
    }
    groups
}

/// 动作是否打开某个浮层（`UiAction` → `Overlay` 的反向映射）。
/// 与 [`Overlay::opened_by`] / [`Overlay::also_opened_by`] 互为逆映射，
/// 故用**同一张表**推导：二者不可能不一致，而不一致的后果是
/// 「某个动作落进 `_ => {}` 静默失效」——S6-07 的 `Ctrl+H`（替换）就是这样
/// 丢掉的：它既不是任何浮层的 `opened_by`，也没人接线，按下去毫无反应。
fn openable_overlay(action: UiAction) -> Option<Overlay> {
    Overlay::ALL.iter().copied().find(|overlay| {
        overlay.opened_by() == Some(action) || overlay.also_opened_by() == Some(action)
    })
}

/// 拖放打开（P2）：markdown 编辑器只接受文本类文件。
///
/// 允许 `.md` / `.markdown`（markdown 家族）与 `.txt`（纯文本，渲染为近乎原文）。
/// 其余扩展名（图片、二进制、PDF 等）一律拒绝——避免把非文本塞进编辑缓冲。
fn is_openable_text(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown") || lower.ends_with(".txt")
}

/// 把一批拖入的路径分成「可打开」与「应拒绝」两组（纯函数，便于单测）。
///
/// 不在此处做任何 IO：是否真的能打开由 [`EditorApp::open_path`] 决定，
/// 这里只按扩展名做**意图分类**。
fn classify_dropped(paths: &[String]) -> (Vec<String>, Vec<String>) {
    let mut open = Vec::new();
    let mut reject = Vec::new();
    for p in paths {
        if is_openable_text(p) {
            open.push(p.clone());
        } else {
            reject.push(p.clone());
        }
    }
    (open, reject)
}

/// 浮层关闭后无需显式归还焦点：早期版本曾实现「焦点归还到触发按钮」机制，
/// 但对应的 `overlay_focus_sink` 控件标识从未在任何视图里注册、
/// `take_focus_return` 也从未被调用——是一套空转的半套机制。现已移除，
/// 浮层关闭后焦点按 egui 默认行为处理即可。
impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1) 「跟随系统」在主题落地**之前**刷新：本帧必须用最终主题画，
        //    否则系统外观变化的那一帧会闪一下旧配色。
        self.refresh_system_theme(ctx);
        mapper::apply(ctx, &self.ui.theme);

        // 2) 收后台对话框的结果。
        //
        //    放在快捷键**之前**：用户刚在系统对话框里选了文件，
        //    这一帧就应该把文件打开，而不是先让窗口里残留的按键事件
        //    触发别的动作。
        if let Some(outcome) = self.dialogs.poll() {
            self.apply_dialog_outcome(outcome);
        }

        // 2.5) 文件拖放：把 .md 文件拖入窗口即打开（P2 拖拽打开）。
        //      放在快捷键之前，与对话框结果同级——拖入文件即开，不被残留按键干扰。
        self.handle_dropped_files(ctx);

        // 3) 快捷键 → 动作。
        for action in self.collect_actions(ctx) {
            self.execute(action);
        }

        // 3.5) 自动保存（DEF-02 修复）：周期把脏文档写入草稿，崩溃后可恢复。
        self.run_autosave();

        // 4) 绘制。视图层在 S6-02~S6-11 逐步接入。
        crate::presentation::views::main_view::show(self, ctx);

        // 4.5) 草稿恢复提示（DEF-02）：启动发现遗留草稿时居中提示。
        self.render_recovery_prompt(ctx);

        // 4.6) 图片导出回路（P2）。必须在**绘制之后**：egui 在帧末才把
        //      `ViewportCommand::Screenshot` 交给后端，帧内发命令、
        //      收上一帧的回传，两件事分处两帧才闭环。
        self.pump_capture(ctx);

        // 4.7) 冷启动埋点（S8-03）：首帧画完 = 用户看到窗口。
        // 未开启环境变量时是空操作，不影响正常启动路径。
        crate::app::startup_timing::mark_first_frame_and_report();
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        // 窗口几何与面板开关在 S6-02 用 eframe storage 持久化；
        // 业务配置由 `PreferencesAppService` 自行落盘，不走这里。
    }
}

/// 以给定 `Services` 启动窗口。
pub fn run(services: Services) -> eframe::Result {
    let system_dark = false;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("rustmark"),
        ..Default::default()
    };
    eframe::run_native(
        "rustmark",
        options,
        Box::new(move |cc| {
            // 这个闭包被调用时，事件循环 / 窗口 / 渲染器都已就绪（`cc` 就是凭证）。
            // 因此「装配完成 → 窗口就绪」这一段 = eframe 建窗口 + wgpu 初始化，
            // 「窗口就绪 → 应用构造完成」= `EditorApp::new` 自身的开销。
            // 不拆这一刀的话，冷启动的大头会被误记到 `EditorApp::new` 头上。
            crate::app::startup_timing::mark("窗口就绪");

            // 字体必须在第一帧绘制**之前**装好。
            // 若拖到首帧之后再设，首帧会用默认字体画一遍（中文全为方框）、
            // 第二帧再重排——用户会看到一次明显的字体跳变。
            install_fonts(&cc.egui_ctx);

            let app = EditorApp::new(services, system_dark);
            crate::app::startup_timing::mark("应用构造完成");
            Ok(Box::new(app))
        }),
    )
}

/// 装配界面字体：在 egui 默认字体族上追加系统 CJK 字体。
///
/// # 为什么必须做
///
/// egui 内置字体**不含任何 CJK 字形**，不追加中文字体时界面上的中文
/// 会全部渲染为占位方框。这是必然结果，不是偶发故障。
///
/// # 缺字体时的取舍
///
/// 找不到中文字体时**不让应用启动失败**——界面仍可正常使用，只是中文显示为方框。
/// 但要留下**明确的警告**：否则用户只会看到一屏方框而无从归因，
/// 更想不到是"系统缺中文字体"这种罕见原因。
///
/// 走 `eprintln!` 而非 `log::warn!`：本项目未注册 logger 后端，
/// `log` 宏会被静默丢弃——"以为有日志其实没有"比没有日志更糟。
/// 与 `startup_timing` 的启动期诊断保持同一做法；**仅在失败时出声**，
/// 正常启动不应有任何输出污染 stderr。
fn install_fonts(ctx: &egui::Context) {
    let setup = crate::infrastructure::fonts::font_definitions();
    if setup.cjk_source.is_none() {
        eprintln!(
            "rustmark：未找到可用的中文字体，界面中文将显示为方框。\
             请安装中文字体（Windows 通常自带微软雅黑；\
             Linux 可安装 fonts-noto-cjk；macOS 应自带苹方）"
        );
    }
    ctx.set_fonts(setup.definitions);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拖放分类：markdown 家族与 txt 应打开，其它扩展名应拒绝。
    #[test]
    fn classify_dropped_splits_by_extension() {
        let paths = vec![
            "/d/docs/笔记.md".to_string(),
            "/d/docs/readme.markdown".to_string(),
            "/d/docs/plain.txt".to_string(),
            "/d/docs/photo.png".to_string(),
            "/d/docs/report.pdf".to_string(),
            "/d/docs/archive.zip".to_string(),
        ];
        let (open, reject) = classify_dropped(&paths);
        assert_eq!(
            open,
            vec![
                "/d/docs/笔记.md".to_string(),
                "/d/docs/readme.markdown".to_string(),
                "/d/docs/plain.txt".to_string(),
            ],
            "markdown / markdown / txt 都应被识别为可打开"
        );
        assert_eq!(reject.len(), 3, "png/pdf/zip 应被拒绝");
    }

    /// 大小写不敏感：`.MD` 也应接受。
    #[test]
    fn classify_dropped_is_case_insensitive() {
        let (open, reject) = classify_dropped(&["/d/A.MD".to_string(), "/d/B.TXT".to_string()]);
        assert_eq!(open.len(), 2);
        assert!(reject.is_empty());
    }

    /// 空输入应不产生任何分组。
    #[test]
    fn classify_dropped_empty_is_noop() {
        let (open, reject) = classify_dropped(&[]);
        assert!(open.is_empty() && reject.is_empty());
    }
}
