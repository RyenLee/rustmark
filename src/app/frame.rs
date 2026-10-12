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
    ExportRequestView, ExportResultView, LintIssueView, OutlineItemView, SearchResultView,
};
use crate::application::export::commands::{CreateExportJobCommand, RunExportCommand};
use crate::application::markdown::queries::{LintQuery, RebuildOutlineQuery};
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
    /// 中文输入法（IME）组合是否正在进行（V11-13 回车缺陷的隐形同伙）。
    ///
    /// egui 0.33 的 `Preedit` 会把预编辑串**真实写进文本缓冲区**，并靠
    /// `TextEdit` 内部状态里的一个选区（组合锚点 ↔ 预编辑串尾）在下一次
    /// `Preedit` / `Commit` 时删旧换新。[`Self::apply_pending_editor_state`]
    /// 在帧间回设的折叠光标会打掉这个选区——实测预编辑串逐帧累积
    /// （"s"→"ssa"→"ssasadsadaf"）、候选词确认后中文被整个忽略。
    /// 组合期间必须**冻结**「回设光标 / 清撤销栈」两个动作，见该方法的守卫。
    ///
    /// # 它为什么必须带终止保证（V11-14 回车失效的真正根因）
    ///
    /// 这个标志**绝不能**做成「只有 `Commit` / `Disabled` 才清」的粘性状态机。
    /// 真实 Windows 输入法在编辑区获得焦点时就会发一次 `Ime::Enabled`，
    /// 而 winit 的 `Disabled` 只在**失焦**时才发（见 `egui-winit` 的
    /// `ime_event_enable` / `ime_event_disable`：前者幂等、后者由 `Commit`
    /// 配对，但切换输入法、候选窗直接关闭、进程被杀等路径都可能收不到）。
    /// 一旦标志永久为 `true`，守卫会**冻结 `pending_cursor` 的消费**，
    /// 而 `flush_edit` 每次按键又覆写它——于是光标永远停在第一次回车后的
    /// 旧位置，之后每一次回车都被插到同一处。实测症状与用户报告逐字吻合：
    /// 「连按回车只多出一个新行，必须再输入字符才会真正换行」。
    ///
    /// 故组合期改用 [`Self::ime_composing_until_ms`] 这样的**有界**判定：
    /// 组合事件开启一个短窗口，窗口内不消费 `pending_cursor`；超时即自动
    /// 恢复，并给 egui 补发 `Disabled` 复位它自己的 `ime_enabled`。
    /// 这既保住了 IME 组合期的正确性，又让「标志卡死」在结构上不可能。
    ime_composing: bool,
    /// 组合期保护窗口的到期时刻（毫秒）。
    ///
    /// 见 [`Self::ime_composing`]：这是「粘性标志」的结构性替代品。
    ime_composing_until_ms: u64,
    /// egui 的 `TextEdit` 可能已把 `ime_enabled` 置真（「IME armed」）。
    ///
    /// 这个标志在 egui 内部是 `pub(crate)`，我们既读不到也写不了，只能**镜像**
    /// 一份：见 [`Self::apply_pending_editor_state`] 里「egui 侧」那段。
    /// 它与 [`Self::ime_composing`] 是**两件事**：本字段只关心
    /// 「egui 会不会按 IME 模式处理键盘」，那个模式会剔除部分按键。
    egui_ime_armed: bool,
    /// 「armed」保护窗口的到期时刻（毫秒）；到点后补发 `Disabled` 复位 egui。
    egui_ime_armed_until_ms: u64,
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
    /// 已发出确认框、等待用户答复的删除目标（V11-06）。
    ///
    /// 删除是唯一不可逆的工作区操作，而 `DialogPort::confirm` 是**同步阻塞**
    /// 的原生模态框——在 `update()` 里直接调会把帧循环卡死。故走两步：
    /// 侧栏点「删除」时把目标记在这里并起后台确认框，取回答复后再真正删。
    ///
    /// 记**路径**而不是「待执行的闭包」：路径是可打印、可断言的值，
    /// 测试能直接读出「还悬着的是哪一个」，闭包只能靠副作用观察。
    pending_delete: Option<String>,
}

impl EditorApp {
    /// 请求删除一个工作区条目（**先弹二次确认，答复为「是」才真删**）。
    ///
    /// # 为什么不能在这里直接删
    ///
    /// 原生确认框是模态同步调用（见 [`crate::shared::dialog::DialogPort::confirm`]），
    /// 在 `update()` 里调用会让窗口在用户思考的那几秒里完全失去响应。
    /// 故与「打开 / 另存为」同一套路：登记意图 → 后台线程问 → 每帧取回。
    ///
    /// 已有确认在飞时**忽略**本次请求（`DialogRunner::spawn` 的语义）：
    /// 连点两下「删除」不该弹出两个系统窗口，更不该在第一个答复之后
    /// 把第二个目标也顺手删掉。
    pub fn request_delete(&mut self, path: &str) {
        if self.pending_delete.is_some() {
            return;
        }
        let message = self.delete_confirm_message(path);
        self.pending_delete = Some(path.to_string());
        self.dialogs.spawn(DialogRequest::Confirm { message });
    }

    /// 当前是否有删除在等用户确认（状态栏 / 测试观察用）。
    pub fn delete_pending(&self) -> Option<&str> {
        self.pending_delete.as_deref()
    }

    /// 构造删除确认的提示文案。
    ///
    /// 目录要**明说会连内容一起删**：`remove_dir_all` 一旦执行不可恢复，
    /// 而用户右键点到的是目录本身，很可能没意识到里面还有东西。
    /// 只写「确定删除 X？」属于把风险藏起来。
    fn delete_confirm_message(&self, path: &str) -> String {
        let name = path.rsplit('/').next().unwrap_or(path);
        let is_directory = self
            .workspace_entries
            .items
            .iter()
            .any(|(p, is_dir)| *is_dir && p == path);
        if !is_directory {
            return format!("确定删除「{name}」？删除后不可恢复。");
        }
        // 目录：明说会连内容一起删。`remove_dir_all` 不可恢复，
        // 而用户右键点到的是目录自身，很可能没意识到里面还有东西。
        let prefix = format!("{path}/");
        let inside = self
            .workspace_entries
            .items
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix))
            .count();
        format!("确定删除目录「{name}」？\n其中包含约 {inside} 项内容，删除后不可恢复。")
    }
}

pub(crate) use capture::PendingCapture;

/// IME 组合期保护窗口（毫秒）。
///
/// # 取值理由
///
/// 它必须同时满足两个约束：
///
/// 1. **够长**：真实中文输入从敲下第一个字母到按下候选确认，人可以慢到
///    几百毫秒。每个字母都是一次 `Preedit`，会把窗口**续上**（不是从首次
///    `Enabled` 起一次性计时），故单次只需覆盖「两次击键的间隔」。
///    取 800ms：远大于普通击键间隔（~100–300ms）。
/// 2. **够短**：一旦 `Commit`/`Disabled` 丢失（切输入法、直接关候选窗、焦点
///    落到浮层），冻结必须在可感知的短时间内自行解除，否则用户的回车会被吞
///    ——那正是本常量要根治的缺陷。见 [`EditorApp::apply_pending_editor_state`]。
pub(super) const IME_GUARD_MS: u64 = 800;

/// 「egui 已 armed IME」的保护窗口（毫秒）。
///
/// 与 [`IME_GUARD_MS`] 分开取值，因为两者要防的故障相反：
///
/// * [`IME_GUARD_MS`] 管**我们自己的守卫**——宁长勿短，冻结久一点最多让
///   光标晚一帧回设，绝不能中途解冻（中途解冻会打掉 egui 的预编辑选区）。
/// * 本常量管**复位 egui 的 `ime_enabled`**——宁短勿长。窗口过长就等于
///   「冻结回车」，而回车正是用户最高频的按键。
///
/// 取 50ms：真实组合里 `Enabled` 与首个 `Preedit` 之间的间隔不到一帧
/// （~16ms），50ms 留了三帧余量；连按回车的人手间隔 ≥80ms，
/// 故第一次回车就已出窗、立即恢复。
pub(super) const IME_ARM_MS: u64 = 50;

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
            ime_composing: false,
            ime_composing_until_ms: 0,
            egui_ime_armed: false,
            egui_ime_armed_until_ms: 0,
            pending_recoveries,
            preview_rect: None,
            pending_capture: None,
            pending_delete: None,
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

    /// 读取持久化设置；失败时回退到 [`SettingsView::fallback`]。
    pub fn settings(&self) -> SettingsView {
        self.services
            .preferences
            .get_settings(GetSettingsQuery)
            .unwrap_or_else(|_| SettingsView::fallback())
    }

    /// 系统当前是否偏好深色（设置页「跟随系统」提示、状态栏主题文案用）。
    ///
    /// 刻意暴露**系统偏好**而不是「当前生效主题」：后者在
    /// `BuiltinTheme::System` 下已经被解析过一次，拿它判断
    /// 「系统是深是浅」等于拿答案去验证答案。
    pub fn system_prefers_dark(&self) -> bool {
        self.system_dark
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

    /// 读取编辑区当前绑定串（`TextEdit` 直接绑定的那个 `String`）。
    /// 与 [`Self::current_content`]（从领域读）配套：两者**必须逐字节相等**，
    /// 这是「领域是内容单一真相」在编辑区的可观测不变量。
    /// 漂移意味着用户敲了键界面却没反应——下一帧才被某条路径悄悄补上。
    pub fn editor_bound_text(&self) -> &str {
        &self.editor_text
    }

    /// IME 组合期标志。
    ///
    /// 暴露给测试断言「守卫是否生效」——它是回车失效缺陷的核心状态，
    /// 又是 `pub(crate)` 字段，测试没有它就只能在行为层面绕圈子。
    pub fn ime_composing(&self) -> bool {
        self.ime_composing
    }

    /// 待回设到 egui 的光标位置。
    ///
    /// 暴露给测试断言「普通输入不该产生光标回设」：回设即多一次强制重算，
    /// 是界面闪烁的上游原因（见 [`Self::flush_edit`]）。
    pub fn pending_cursor_for_diag(&self) -> Option<(usize, usize)> {
        self.pending_cursor
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
    pub(super) fn tab_unit(&self) -> String {
        " ".repeat(self.settings().editor.tab_size.max(1) as usize)
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
}

// ————————————————————————————————————————————————————————————
// 子模块（facade 之外的实现）
// ————————————————————————————————————————————————————————————

mod actions;
mod capture;
mod dialog;
mod drag_drop;
mod editor;
mod files;
mod find;
mod outline;
mod recovery;
mod settings;
mod tabs;
mod workspace;
pub(crate) use drag_drop::classify_dropped;

// ————————————————————————————————————————————————————————————
// eframe::App 主循环与启动入口（留在 facade，跨多个子文件方法调用）
// ————————————————————————————————————————————————————————————

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
mod tests;
