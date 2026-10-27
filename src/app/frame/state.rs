//! 应用根状态的**定义**：`EditorApp` 的字段表与构造函数。
//!
//! # 为什么单独成文件
//!
//! 这个结构体有 40 多个字段，每个字段都带「为什么需要它」的注释；与 24 个
//! 子模块声明、帧循环挤在同一个文件里时，要回答「某个字段从哪来」得先滚过
//! 半屏无关内容。拆出来后本文件只回答一件事：**应用根状态长什么样、启动时
//! 怎么装配**。行为一概不在本文件——动作执行在 `super::actions`，帧循环在
//! `super::r#loop`，各视图的写入口在 `super::editor` / `super::sidebar` 等兄弟模块。
//!
//! 可见性零变化：facade 侧 `pub use state::EditorApp;` 让
//! `crate::app::frame::EditorApp` 与 `rustmark::app::frame::EditorApp` 两条路径
//! 保持原样（兄弟模块的 `use super::*` 也照常拿到）。

use super::*;

/// 行索引缓存的存储类型（见 [`EditorApp::line_index_cache`]）。
///
/// 取出成别名以压住 `clippy::type-complexity`：`(键, 值)` 的嵌套组合
/// 达到告警阈值。键是 `(活动文档, 领域修订版本, 文本字节长度)`，
/// 值是一次全文扫描得到的 [`LineIndex`]。
type LineIndexCache =
    std::cell::RefCell<Option<((Option<DocumentId>, u64, usize), std::rc::Rc<LineIndex>)>>;

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
pub(crate) const STATS_DEBOUNCE_MS: u64 = rustmark_core::shared::constants::DEBOUNCE_MS;

/// 应用根状态：一个 `eframe::App` 实例持有的全部东西。
///
/// 一切视图（[`crate::presentation::views`]）都通过 `&mut EditorApp` 读写
/// 各自的子状态与服务，自身不持有业务数据。
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
    /// 可打开的文本格式白名单（问题 045）。
    ///
    /// 全应用**唯一**一份：文件树置灰、双击拦截、拖放分类、原生对话框
    /// 过滤器都由它派生。放在应用状态而非写死常量，是为了未来接设置页
    /// 时只需在此处换成一个「内置 + 用户自定义」的实例，
    /// 各入口不必逐一改动。
    pub(super) text_formats: TextFormats,
    /// 系统当前是否偏好深色（「跟随系统」用）。
    pub(super) system_dark: bool,
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
    pub(super) installed_font_family: Option<String>,
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
    pub(super) line_index_cache: LineIndexCache,
    /// 状态栏字数统计的防抖节拍器（建议一 / 问题 039；见 [`StatsPace`]）。
    pub(super) stats_pace: StatsPace,
    /// 帧内到期点的统一登记与预约出口（T11；见 [`FrameScheduler`]）。
    ///
    /// 自动保存、统计/高亮防抖、状态栏消息 TTL 都只在这里 `arm`，
    /// 由帧末的 `flush` 换算成**一次** `request_repaint_after`。
    /// 散落在各处的直接预约会让「新增定时功能就漏预约」的坑反复出现。
    pub(super) scheduler: FrameScheduler,
    /// 领域编辑后需要把 egui 光标重设到的**字符**位置 `(cursor, anchor)`。
    ///
    /// egui 的 `TextEdit` 在内部记一套光标状态；领域命令改了文本后，
    /// 必须显式把这个位置写回 egui，否则光标会停在错误的偏移（多字节边界尤其明显）。
    pub(super) pending_cursor: Option<(usize, usize)>,
    /// 每份打开文档各自的光标 / 选区（R-06：切标签保留光标）。
    ///
    /// 键 [`DocumentId`]，值 `(cursor, anchor)` **字节偏移**——与
    /// [`EditorUiState::cursor`] / [`EditorUiState::anchor`] 同单位；无选区时
    /// `anchor == cursor`（等价于 `anchor: Option<usize>` 的 `None`，这里用
    /// 相等值省掉一层 `Option`）。
    ///
    /// 为什么不在领域层：光标是**视图状态**（见 `editor_ui_state` 模块注释），
    /// 而「哪份文档的光标配哪份文档」天然随标签生命周期生灭——领域
    /// `Document` 关闭后这条记录即失效，故由 UI 侧持有、随关标签清除
    /// （`close_document` / `close_all_documents`）。
    ///
    /// 切走时由 `stash_active_doc_cursor` 记、切回时由
    /// `restore_active_doc_cursor` 取（见 `frame::tabs`）。
    pub(super) per_doc_cursor: std::collections::HashMap<DocumentId, (usize, usize)>,
    /// 上一次领域编辑后需要清空 egui 内置撤销栈。
    ///
    /// 理由：egui 的 `TextEdit` 自带撤销栈并会响应 Ctrl+Z；若不清除，
    /// 就会与**领域**撤销栈（本项目的撤销权威）打架。每次通过领域改了文本后，
    /// 下一帧开头清掉 egui 的撤销栈，让 Ctrl+Z 永远只走领域。
    pub(super) clear_undoer_next: bool,
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
    pub(super) ime_composing: bool,
    /// 组合期保护窗口的到期时刻（毫秒）。
    ///
    /// 见 [`Self::ime_composing`]：这是「粘性标志」的结构性替代品。
    pub(super) ime_composing_until_ms: u64,
    /// egui 的 `TextEdit` 可能已把 `ime_enabled` 置真（「IME armed」）。
    ///
    /// 这个标志在 egui 内部是 `pub(crate)`，我们既读不到也写不了，只能**镜像**
    /// 一份：见 [`Self::apply_pending_editor_state`] 里「egui 侧」那段。
    /// 它与 [`Self::ime_composing`] 是**两件事**：本字段只关心
    /// 「egui 会不会按 IME 模式处理键盘」，那个模式会剔除部分按键。
    pub(super) egui_ime_armed: bool,
    /// 「armed」保护窗口的到期时刻（毫秒）；到点后补发 `Disabled` 复位 egui。
    pub(super) egui_ime_armed_until_ms: u64,
    /// 启动时发现、待用户处置的遗留草稿 `(文档 ID, 内容, 路径)`（DEF-02 恢复）。
    ///
    /// 路径为崩溃前文档的磁盘身份（问题 069）：恢复时据此还原原始文件名，
    /// 标签条不再一律显示「未命名」。`None` 表示该草稿属于从未保存过的
    /// 未命名文档。
    ///
    /// 非空时每帧弹出一个居中的恢复提示；用户选择「恢复全部」或「丢弃全部」
    /// 后才清空。空列表则完全不渲染，不影响正常编辑。
    pub(super) pending_recoveries: Vec<(DocumentId, String, Option<FilePath>)>,
    /// 本帧预览区在窗口中的矩形（**逻辑点**；布局未完成时为 `None`）。
    ///
    /// 由 `main_view` 在`CentralPanel` 布局出预览区时写入，
    /// 供图片导出裁剪使用（P2）。存**上一帧**的值不是缺陷：
    /// 截图命令要到下一帧才被后端捕获，届时本帧矩形刚刚写好。
    pub(super) preview_rect: Option<egui::Rect>,
    /// 已发出截图命令、等待 GPU 回传的任务（P2 图片导出）。
    ///
    /// **必然跨一帧**：egui 的 `ViewportCommand::Screenshot` 在下一帧才通过
    /// `Event::Screenshot` 交回像素（帧 N 发命令、帧 N 绘制时被捕获）。
    /// 故点「导出」时只能先登记，等下一帧再兑现。
    ///
    /// 只有一个在等：连点两次时后一次会覆盖前一次。这是有意的——
    /// 两次截图捕获的是**同一帧画面**（egui 每帧只绘制一次），
    /// 排队等两次毫无意义，用户要的也不是「同一张图存两份」。
    pub(super) pending_capture: Option<PendingCapture>,
    /// 截图回传收件箱：egui 0.37 起 `request_screenshot` 以回调方式交付像素，
    /// 回调在 `self` 之外触发，故用 `Arc<Mutex<...>>` 共享。
    pub(super) screenshot_sink:
        std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::epaint::ColorImage>>>>,
    /// 已发出确认框、等待用户答复的删除目标（V11-06）。
    ///
    /// 删除是唯一不可逆的工作区操作，而 `DialogPort::confirm` 是**同步阻塞**
    /// 的原生模态框——在 `update()` 里直接调会把帧循环卡死。故走两步：
    /// 侧栏点「删除」时把目标记在这里并起后台确认框，取回答复后再真正删。
    ///
    /// 记**路径**而不是「待执行的闭包」：路径是可打印、可断言的值，
    /// 测试能直接读出「还悬着的是哪一个」，闭包只能靠副作用观察。
    pub(super) pending_delete: Option<String>,
    /// 已发出确认框、等待用户答复的「全部关闭」（问题 038）。
    ///
    /// 与 [`Self::pending_delete`] 同款两步走：全部关闭会丢弃**所有**未保存
    /// 修改，不可逆；而 `DialogPort::confirm` 同步阻塞，不能在 `update()` 里调。
    /// 布尔而非路径：待确认的目标就是「当前打开的全部文档」，
    /// 没有第二个自由度，记路径反而是假精度。
    pub(super) pending_close_all: bool,
    /// 已发出确认框、等待用户答复的「还原修改」目标（R-04a）。
    ///
    /// 与 [`Self::pending_delete`] 同款两步走：还原丢弃全部未保存修改，
    /// 不可逆；确认框同步阻塞，故登记目标 → 后台问 → 每帧取回。
    /// 记 **DocumentId** 而非布尔：目标是「请求还原时的那一个文档」，
    /// 答复回来后按 id 兑现，不按「当时谁是活动文档」——后者会把
    /// 「确认期间用户切了标签」变成还原错文档。
    pub(super) pending_revert: Option<DocumentId>,
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
            Result<Vec<rustmark_core::application::dto::FileEntryView>, String>,
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
    pub(super) sidebar_visible_persisted: bool,
    /// 上一次落盘的侧栏宽度（基准值，供帧末变更检测比对）。
    ///
    /// 与 `sidebar_visible_persisted` 同源：`persist_sidebar_width`
    /// 每帧比对「当前宽度 ≠ 基准」决定要不要写盘。拿 `get_settings()`
    /// 每次去问也行，但那会把整张快捷键表一起克隆一遍——60fps 下为
    /// 一个浮点数付这个代价不值当。
    pub(super) sidebar_width_persisted: f32,
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
            per_doc_cursor: std::collections::HashMap::new(),
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
            pending_revert: None,
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
        // 自动换行 / 拼写检查同理（F-1）：两条写入口都落盘，启动必须回读——
        // 缺了这两行，设置页存下的值重启后又退回领域默认，用户看到的是
        // 「设了没保存」；而状态栏的切换更是每次都丢。
        app.editor.word_wrap = settings.editor.word_wrap;
        app.editor.spell_check = settings.editor.spell_check;
        // 远程图片开关（PRIV-01）：预览区按会话值决定是否外联，启动必须回读，
        // 否则用户关了开关、重启后又开始联网（隐私承诺变成「只管本次会话」）。
        app.editor.load_remote_images = settings.editor.load_remote_images;
        app
    }
}

// ————————————————————————————————————————————————————————————
// 子模块（facade 之外的实现）
// ————————————————————————————————————————————————————————————
