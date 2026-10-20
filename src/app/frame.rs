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
    EditorSettingsView, RecentFileView, SettingsView, StatsView, TabSummary, ThemeView,
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
use crate::shared::export_format::{ExportFormat, PageSize};
use crate::shared::fonts::EditorFontChoice;
use crate::shared::path::{FilePath, file_name_of};
use crate::shared::result::AppResult;
use crate::shared::text_format::{self, TextFormats};
use crate::shared::time::Timestamp;

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
use crate::presentation::state::sidebar_ui_state::{
    SearchGroup, SearchScope, group_hits, locate_in_snippet,
};
use crate::presentation::state::status_bar_ui_state::StatusBarUiState;
use crate::presentation::state::symbol_ui_state::SymbolUiState;
use crate::presentation::theme::builtin::{self, BuiltinTheme, Theme};
use crate::presentation::theme::mapper;
use crate::presentation::views::editor_ops;
use crate::presentation::views::scroll_sync::{LineIndex, RevealIntent, TARGET_MARGIN};
// `floor_char_boundary` 的语义与「为何只下取整」见 `shared::geometry`；
// 程序化设值（大纲跳转、测试、`TextEdit` 回调）能给任意字节偏移，
// 而 `&str[a..b]` 遇到非字符边界会 panic，`panic = "abort"` 下即窗口消失。
use crate::shared::geometry::{ByteOffset, TextRange, floor_char_boundary};
use crate::shared::ids::{DocumentId, JobId};

pub use dialog::PendingDialog;
pub use workspace::{WorkspaceEntries, WorkspaceSearch};

/// 应用根状态：一个 `eframe::App` 实例持有的全部东西。
///
/// # 为什么 UI 状态与应用服务**平铺**在同一结构体里
///
/// 行索引缓存的存储类型（见 [`EditorApp::line_index_cache`]）。
///
/// 取出成别名以压住 `clippy::type-complexity`：`(键, 值)` 的嵌套组合
/// 达到告警阈值。键是 `(活动文档, 领域修订版本, 文本字节长度)`，
/// 值是一次全文扫描得到的 [`LineIndex`]。
type LineIndexCache =
    std::cell::RefCell<Option<((Option<DocumentId>, u64, usize), std::rc::Rc<LineIndex>)>>;

/// 两者需要高频交叉读取（执行动作要知道当前选区，切主题要同步预览缓存）。
/// 再包一层「上下文」只会多一次间接寻址，却不带来任何隔离。
/// 真正的隔离不靠结构体嵌套，而靠 `check-arch` 的依赖禁令——
/// 表现层**拿不到** `domain` 与 `infrastructure` 的类型，
/// 物理上无法把业务真值塞进 UI 状态。
/// 状态栏字数统计的**防抖**节拍器（建议一 / 问题 039）。
///
/// 与编辑器高亮的 `HighlightPace` 同源：打字期不每击键整篇重算
/// （1MB 上 `compute_stats` ≈ 9.5ms，见 `docs/metrics.md` §3.2.5），
/// 而是沿用「上一次静止时算出的结果」，直到文档**静止满 [`STATS_DEBOUNCE_MS`]** 才刷新。
///
/// 与高亮的区别：高亮首次见某篇要立刻上色；这里**首次见某篇也要立刻算**
/// （否则打开文档后字数先显 0、150ms 后才跳真值，同样 flicker）。
/// 故 `doc` 改变（或首算）时直接重算，不走防抖。
///
/// 统计浮层（`stats_view`）走 [`EditorApp::current_stats`]（点击时才算、即时），
/// 本节拍器只服务每帧的状态栏。
pub(crate) struct StatsPace {
    /// 缓存所属文档；`None` 表示尚未计算过任何一次。
    pub(crate) doc: Option<DocumentId>,
    /// 上次**见到**的修订版本（用于检测「本帧相对上次是否变了」）。
    pub(crate) last_seen_revision: u64,
    /// `cached` 所对应的修订版本（即「已算到哪一版」）。
    pub(crate) computed_revision: u64,
    /// 上次见到修订变化的时刻（用于判定是否静止满窗口）。
    pub(crate) last_change_at: Timestamp,
    /// 最近一次（可能滞后的）统计结果。
    pub(crate) cached: Option<StatsView>,
}

impl Default for StatsPace {
    fn default() -> Self {
        Self {
            doc: None,
            last_seen_revision: 0,
            computed_revision: 0,
            last_change_at: Timestamp::from_millis(0),
            cached: None,
        }
    }
}

/// 状态栏统计防抖窗口（毫秒）。
///
/// 取 150ms：与编辑器高亮 `HighlightPace` 的静止窗口一致——用户停手约一帧半后
/// 刷新字数，打字期完全不重算。数字来自 `docs/metrics.md` §3.2.5：`compute_stats`
/// 在 1MB 上 ≈ 9.5ms，与其每击键付一次，不如等静止后付一次。
pub(crate) const STATS_DEBOUNCE_MS: u64 = 150;

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
    /// 可打开的文本格式白名单（问题 045）。
    ///
    /// 全应用**唯一**一份：文件树置灰、双击拦截、拖放分类、原生对话框
    /// 过滤器都由它派生。放在应用状态而非写死常量，是为了未来接设置页
    /// 时只需在此处换成一个「内置 + 用户自定义」的实例，
    /// 各入口不必逐一改动。
    text_formats: TextFormats,
    /// 系统当前是否偏好深色（「跟随系统」用）。
    system_dark: bool,
    /// 设置页「字体」下拉的可选项（启动时发现一次：项目 `fonts/` 目录
    /// + 常见系统等宽字体 + 默认哨兵）。
    ///
    /// 由基础设施层产出（`EditorFontChoice` 是 `shared` 契约，表现层可持）；
    /// 发现只做存在性检查，成本在一启动时一次性付出。
    pub editor_fonts: Vec<EditorFontChoice>,
    /// 当前已装进 egui 的编辑区字体键；`None` = 尚未装配。
    ///
    /// 与持久化配置里的 `font_family` 比对即可判断「要不要重装字体」——
    /// 重装会触发 egui 整体重建字体图集（一次可见卡顿），故只在
    /// **真的变了**时做。首帧后由 [`Self::ensure_editor_fonts`] 补齐。
    installed_font_family: Option<String>,
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
    /// 当前文本的**行索引缓存**：`(键, 索引)`（问题 053）。
    ///
    /// 键是 `(活动文档, 领域修订版本, 文本字节长度)`，理由见
    /// [`EditorApp::line_index`]。放在这里而不是每帧现算，是因为状态栏
    /// （光标的行列 / 选区的行列 / 「N 行」）与编辑区（`desired_rows` /
    /// 行号栏宽度）一帧要问四五次**同一份**信息，1MB 上实测每题一次
    /// 全文扫描、每帧恒定 1.1ms（`docs/metrics.md` §3.2.5）。
    line_index_cache: LineIndexCache,
    /// 状态栏字数统计的防抖节拍器（建议一 / 问题 039；见 [`StatsPace`]）。
    stats_pace: StatsPace,
    /// 帧内到期点的统一登记与预约出口（T11；见 [`FrameScheduler`]）。
    ///
    /// 自动保存、统计/高亮防抖、状态栏消息 TTL 都只在这里 `arm`，
    /// 由帧末的 `flush` 换算成**一次** `request_repaint_after`。
    /// 散落在各处的直接预约会让「新增定时功能就漏预约」的坑反复出现。
    scheduler: FrameScheduler,
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
    /// 启动时发现、待用户处置的遗留草稿 `(文档 ID, 内容, 路径)`（DEF-02 恢复）。
    ///
    /// 路径为崩溃前文档的磁盘身份（问题 069）：恢复时据此还原原始文件名，
    /// 标签条不再一律显示「未命名」。`None` 表示该草稿属于从未保存过的
    /// 未命名文档。
    ///
    /// 非空时每帧弹出一个居中的恢复提示；用户选择「恢复全部」或「丢弃全部」
    /// 后才清空。空列表则完全不渲染，不影响正常编辑。
    pending_recoveries: Vec<(DocumentId, String, Option<FilePath>)>,
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
    /// 截图回传收件箱：egui 0.37 起 `request_screenshot` 以回调方式交付像素，
    /// 回调在 `self` 之外触发，故用 `Arc<Mutex<...>>` 共享。
    screenshot_sink:
        std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::epaint::ColorImage>>>>,
    /// 已发出确认框、等待用户答复的删除目标（V11-06）。
    ///
    /// 删除是唯一不可逆的工作区操作，而 `DialogPort::confirm` 是**同步阻塞**
    /// 的原生模态框——在 `update()` 里直接调会把帧循环卡死。故走两步：
    /// 侧栏点「删除」时把目标记在这里并起后台确认框，取回答复后再真正删。
    ///
    /// 记**路径**而不是「待执行的闭包」：路径是可打印、可断言的值，
    /// 测试能直接读出「还悬着的是哪一个」，闭包只能靠副作用观察。
    pending_delete: Option<String>,
    /// 已发出确认框、等待用户答复的「全部关闭」（问题 038）。
    ///
    /// 与 [`Self::pending_delete`] 同款两步走：全部关闭会丢弃**所有**未保存
    /// 修改，不可逆；而 `DialogPort::confirm` 同步阻塞，不能在 `update()` 里调。
    /// 布尔而非路径：待确认的目标就是「当前打开的全部文档」，
    /// 没有第二个自由度，记路径反而是假精度。
    pending_close_all: bool,
    /// 后台工作区扫描的待处理结果（PickFolder 首次扫描路径异步化）。
    ///
    /// channel 发回 `(root, Result<Vec<FileEntryView>, String>)` ——
    /// 扫描条目由后台线程完整执行 `scan_tree`（领域层写入 + 事件发布），
    /// 主线程收到后再构造 `WorkspaceEntries` + 展开顶层目录 + 更新状态栏。
    /// 其他扫描路径（reload / refresh / create_entry 后）**保持同步**——
    /// 那些路径的目录规模已知且通常较小，后台线程化反而增加首帧延迟。
    #[allow(clippy::type_complexity)]
    // Receiver<(String, Result<...>, String)> 嵌套 3 层；抽别名收益极低
    pub(crate) scan_pending: Option<
        std::sync::mpsc::Receiver<(
            String,
            Result<Vec<crate::application::dto::FileEntryView>, String>,
        )>,
    >,
    /// 侧栏扫描 loading 态（显示"正在扫描..."提示）。
    pub(crate) scan_loading: bool,
    /// 侧栏开合**上次落盘**的值（会话级，不参与别的判定）。
    ///
    /// 存在的理由：开合状态要「变了就写盘」（问题 062），而写入动作在
    /// 每帧末尾发生，判断「变没变」必须有一个上一次的值可比。拿
    /// `get_settings()` 每次去问也行，但那会把整张快捷键表一起克隆一遍
    /// （`SettingsView` 含 `Vec<ShortcutView>`）——60fps 下为两个布尔付这个
    /// 代价不值当，与 `split_dragging`「只在跳变时写一次」同源。
    sidebar_visible_persisted: bool,
    /// 上一次落盘的侧栏宽度（基准值，供帧末变更检测比对）。
    ///
    /// 与 `sidebar_visible_persisted` 同源：`persist_sidebar_width`
    /// 每帧比对「当前宽度 ≠ 基准」决定要不要写盘。拿 `get_settings()`
    /// 每次去问也行，但那会把整张快捷键表一起克隆一遍——60fps 下为
    /// 一个浮点数付这个代价不值当。
    sidebar_width_persisted: f32,
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
///
/// 出窗那一帧由 [`EditorApp::apply_pending_editor_state`] 补发 `Disabled`
/// 复位 egui，并**同时抹平本帧 Enter 的 `repeat`**——两者缺一不可：
/// `remove_ime_incompatible_events` 在 `TextEdit::show` 内部、按当帧仍为真
/// 的 `ime_enabled` 剔除 repeat 按键，晚一步的 `Disabled` 救不回它。
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
            // 白名单以内置清单起步（唯一出处见 `shared::text_format`）。
            text_formats: TextFormats::builtin(),
            // 执行器要克隆 `services.dialogs`，故必须在移动 `services` 之前建。
            dialogs: DialogRunner::new(services.dialogs.clone()),
            services,
            system_dark,
            // 字体清单只做存在性发现（不读文件内容），启动成本可忽略；
            // 真正的字节读取与装配发生在 `ensure_editor_fonts`。
            editor_fonts: crate::infrastructure::fonts::editor_font_choices(),
            installed_font_family: None,
            active_doc: None,
            editor_text: String::new(),
            line_index_cache: std::cell::RefCell::new(None),
            // 首次见某篇文档要立刻算（避免开文先显 0），故默认空、惰性填充。
            stats_pace: StatsPace::default(),
            scheduler: FrameScheduler::default(),
            pending_cursor: None,
            clear_undoer_next: false,
            ime_composing: false,
            ime_composing_until_ms: 0,
            egui_ime_armed: false,
            egui_ime_armed_until_ms: 0,
            pending_recoveries,
            preview_rect: None,
            pending_capture: None,
            screenshot_sink: std::sync::Arc::new(std::sync::Mutex::new(None)),
            pending_delete: None,
            pending_close_all: false,
            scan_pending: None,
            scan_loading: false,
            // 真实值在下面按持久化设置覆盖（与 `split_ratio` 同一手法）。
            sidebar_visible_persisted: false,
            // 基准在下面与 `ui.sidebar_width` 一并确定（持久化值或主题默认）。
            sidebar_width_persisted: 0.0,
        };
        // 分割比例以持久化配置为权威来源：用户拖过的比例重启后仍生效。
        app.ui.split_ratio = settings.editor.split_ratio.clamp(SPLIT_MIN, SPLIT_MAX);
        // 侧栏宽度同理（问题 067）：拖过的宽度重启后仍生效；**没拖过**
        // （`None`，首次启动或旧配置）保持 `AppUiState::new` 里的主题
        // 默认宽度。夹紧在 `set_sidebar_width` 内做（手改配置写出 5000
        // 也不会把侧栏撑出窗口）。
        if let Some(width) = settings.editor.sidebar_width {
            app.ui.set_sidebar_width(width);
        }
        // 基准取**生效值**（持久化值或主题默认）：与磁盘一致就不该触发
        // 写盘，否则每次启动都会白写一次配置文件。
        app.sidebar_width_persisted = app.ui.sidebar_width;
        // 侧栏开合同理（问题 062 / 064）：上次明确决定过展开的就还展开、
        // 收起的就还收起；**没决定过**（`None`，首次启动或旧配置）按默认
        // 收起——之后打开文件夹时再自动展开（`reveal_workspace_sidebar`）。
        // 同时把有效值记为「已落盘值」，供每帧的变更检测做基准（见
        // [`Self::persist_sidebar_visibility`]）——不然启动第一帧就会被判成
        // 「变了」，白写一次配置文件。注意基准取**有效值**而非决定本身：
        // `None` + 界面收起 = 与磁盘一致，不该触发写盘。
        let sidebar_effective = settings.editor.sidebar_visible.unwrap_or(false);
        app.ui.sidebar_visible = sidebar_effective;
        app.sidebar_visible_persisted = sidebar_effective;
        // 行号栏开关同样以持久化配置为权威来源（默认隐藏，见领域 `EditorSettings`）。
        app.editor.line_numbers = settings.editor.show_line_numbers;
        app
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
mod scheduler;
mod settings;
mod tabs;
mod workspace;
pub(crate) use drag_drop::classify_dropped;
pub(crate) use scheduler::{Deadline, FrameScheduler};

// ————————————————————————————————————————————————————————————
// eframe::App 主循环与启动入口（留在 facade，跨多个子文件方法调用）
// ————————————————————————————————————————————————————————————

/// 浮层关闭后无需显式归还焦点：早期版本曾实现「焦点归还到触发按钮」机制，
/// 但对应的 `overlay_focus_sink` 控件标识从未在任何视图里注册、
/// `take_focus_return` 也从未被调用——是一套空转的半套机制。现已移除，
/// 浮层关闭后焦点按 egui 默认行为处理即可。
impl eframe::App for EditorApp {
    // egui 0.37 把旧的 `update` 拆成 `logic`（窗口隐藏时也要跑的状态更新）
    // 与 `ui`（仅绘制）。本项目所有状态更新都依赖绘制结果（如拖放命中、快捷键
    // 触发、截图回传），且旧 `update` 就是「每帧当要重绘时调用」，与 `ui`
    // 的语义一致，故全部逻辑统一收进 `ui`，不实现空的 `logic`。
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 状态更新阶段：需要 `ctx`。这段借用必须**在 `main_view::show` 之前结束**，
        // 因为绘制阶段要的可变 `ui` 与这里的不可变 `ctx` 借用冲突。
        {
            let ctx = ui.ctx();

            // 0) 帧首清空到期点登记表（T11）。必须在任何 `arm` 之前：
            //    到期点是本帧依据当前状态重新算出的，不是跨帧状态。
            self.scheduler.begin(self.services.markdown.now().millis());

            // 1) 「跟随系统」在主题落地**之前**刷新：本帧必须用最终主题画，
            //    否则系统外观变化的那一帧会闪一下旧配色。
            self.refresh_system_theme(ctx);
            mapper::apply(ctx, &self.ui.theme);

            // 1.5) 编辑区字体：设置页改过（或首帧）时重装。
            //      放在任何控件布局之前——`set_fonts` 要赶在本帧排版之前生效。
            //      内部按「键没变就跳过」短路，未改字体时每帧只多一次字符串比对。
            self.ensure_editor_fonts(ctx);

            // 2) 收后台对话框的结果。
            //
            //    放在快捷键**之前**：用户刚在系统对话框里选了文件，
            //    这一帧就应该把文件打开，而不是先让窗口里残留的按键事件
            //    触发别的动作。
            if let Some(outcome) = self.dialogs.poll() {
                self.apply_dialog_outcome(outcome);
            }

            // 2.1) 工作区后台扫描结果轮询（PickFolder 首次扫描路径异步化）。
            //
            //     扫描是磁盘 IO，大目录会阻塞 UI 帧循环——用户选完文件夹后窗口
            //     会「冻住」几百毫秒到几秒（视目录规模而定）。改成后台线程后，
            //     用户选完文件夹立刻回到 UI 继续操作，侧栏短暂显示 loading 态。
            //     领域层 scan_tree（RwLock 写入 + EventBus publish）线程安全，
            //     放后台无副作用。其他扫描路径（reload / refresh）保持同步。
            self.poll_scan_result();

            // 2.5) 文件拖放：把 .md 文件拖入窗口即打开（P2 拖拽打开）。
            //      放在快捷键之前，与对话框结果同级——拖入文件即开，不被残留按键干扰。
            self.handle_dropped_files(ctx);

            // 3) 快捷键 → 动作。
            for action in self.collect_actions(ctx) {
                self.execute(action);
            }

            // 3.5) 自动保存（DEF-02 修复；问题 068 补「到期帧」调度）：
            //      周期把脏文档写入草稿，崩溃后可恢复；没有输入事件时由
            //      `request_repaint_after` 保证到期时刻仍有一帧来执行。
            self.run_autosave();

            // 3.6) 高亮的「到期帧」（T11）：`highlight` 在防抖窗口内返回空片段，
            //      必须有人在窗口到期时再跑一帧才补得上颜色。编辑器失焦后没有
            //      光标闪烁兜底，缺这行语法色会一直停在无色状态。
            //      到期点由 `HighlightPace` 给出（只统计「还没算完」的文档）。
            if let Some(due_ms) = self.services.markdown.highlight_next_due_in() {
                self.scheduler.arm(Deadline::HighlightDebounce, due_ms);
            }
        }

        // 4) 绘制。视图层在 S6-02~S6-11 逐步接入。
        //    egui 0.37 起根 `Ui` 已自带全窗口可用区，Panel 全部从 `ui` 切分。
        // 侧栏开合的落盘在主视图帧末完成（见 `main_view::show`）——
        // 那里才是「本帧所有点击都处理完」的位置。
        crate::presentation::views::main_view::show(self, ui);

        // 4.1) 帧末统一预约（T11）：本帧所有 `arm` 过的到期点在此换算成
        //      **一次** `request_repaint_after`。位置必须在 `main_view::show`
        //      之后——状态栏的统计防抖与消息 TTL 是在绘制阶段登记的。
        self.scheduler.flush(ui.ctx());

        // 4.5) 草稿恢复提示（DEF-02）：启动发现遗留草稿时居中提示。
        {
            let ctx = ui.ctx();
            self.render_recovery_prompt(ctx);

            // 4.6) 图片导出回路（P2）。必须在**绘制之后**：egui 在帧末才把
            //      `ViewportCommand::Screenshot` 交给后端，帧内发命令、
            //      收上一帧的回传，两件事分处两帧才闭环。
            self.pump_capture(ctx);
        }

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

            // 应用先构造（它持有持久化配置，字体选择要以配置为权威来源），
            // 再装字体——但**仍然在第一帧绘制之前**：eframe 绘制首帧前会先
            // 走完创建闭包，故用户上次选的字体会从第一帧就生效，不会出现
            // 「先用默认字体画一帧（中文全为方框）、第二帧再跳变」。
            let mut app = EditorApp::new(services, system_dark);
            crate::app::startup_timing::mark("应用构造完成");
            app.ensure_editor_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

/// 装配界面字体：在 egui 默认字体族上追加系统 CJK 字体，并把用户选中的
/// 等宽字体置顶进内置 `Monospace` 族。
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
/// 走 `log::warn!`：F-2 起 `main` 注册了 env_logger 后端（默认级别 warn），
/// 该告警默认可见；`RUST_LOG` 可调级别。**仅在失败时出声**——
/// 正常启动（找得到中文字体时）没有任何输出。
impl EditorApp {
    /// 登记状态栏消息的 TTL 到期点（T11）。
    ///
    /// 由状态栏视图在**取出未过期消息之后**调用——那一刻它才知道消息还剩多久。
    /// 做成方法而不是把 `scheduler` 字段开给视图：`arm` 是帧内可以开放的动作，
    /// `flush` 不是，字段私有才能保证「预约只有一个出口」。
    ///
    /// 消息槽为空时是空操作：没有消息就没有要叫醒的时刻。
    pub(crate) fn arm_status_message_deadline(&mut self) {
        if let Some(expires_at) = self.status_bar.message.as_ref().map(|m| m.expires_at) {
            self.scheduler.arm(Deadline::StatusMessage, expires_at);
        }
    }

    /// 本帧已登记的到期点 `(短名, 到期毫秒)`（测试与调试用）。
    ///
    /// 存在理由与 `highlight_pace_snapshot` 同类：回答「这个定时功能到底有没有
    /// 接进帧循环」。没有它，只能靠真机等 150ms 看字色有没有恢复来间接推断。
    pub fn scheduled_deadlines(&self) -> Vec<(&'static str, u64)> {
        self.scheduler.entries()
    }

    /// 确保 egui 里装的是当前选中的编辑区字体；**没变就不动**。
    ///
    /// 每帧调用（`update` 开头与启动各一次）：内部只做一次字符串比对，
    /// 命中即返回。重装有真实代价——`set_fonts` 会触发 egui 整体重建
    /// 字体图集（一次可见卡顿），故绝不在「没变」时触发。
    fn ensure_editor_fonts(&mut self, ctx: &egui::Context) {
        let selected = self.settings().theme.font_family.clone();
        if self.installed_font_family.as_deref() == Some(selected.as_str()) {
            return;
        }
        self.install_editor_fonts(ctx, &selected);
    }

    /// 真正重装字体（选中项变化 / 首帧时）。
    fn install_editor_fonts(&mut self, ctx: &egui::Context, selected: &str) {
        let setup = crate::infrastructure::fonts::font_definitions(Some(selected));
        if setup.cjk_source.is_none() {
            // 系统没中文字体不等于中文变方框了——内置 Noto Sans SC 子集
            // （GB2312）会兜底。仍要出声：用户该知道观感来自内置子集，
            // 且 GB2312 之外的生僻字在此环境下仍会是方框。
            log::warn!(
                "未找到系统中文字体，已改用内置 Noto Sans SC 子集（GB2312）渲染中文。\
                 常用汉字不受影响；生僻字在此环境下仍可能显示为方框。\
                 装一副中文字体即可获得完整覆盖（Windows 通常自带微软雅黑；\
                 Linux 可安装 fonts-noto-cjk；macOS 应自带苹方）"
            );
        }
        ctx.set_fonts(setup.definitions);
        // 清单同步刷新：用户可能往项目 fonts/ 目录放了新字体后重启，
        // 那次重启会走这里，下拉里立刻能选到。
        self.editor_fonts = setup.editor_fonts;
        self.installed_font_family = Some(selected.to_string());
    }
}

#[cfg(test)]
mod tests;
