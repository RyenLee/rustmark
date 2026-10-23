//! 应用级UI 状态（布局开关、当前标签、专注模式）。
//!
//! # 硬边界：这里只放**视图状态**
//!
//! 允许：开关、当前选中项、滚动位置、临时输入、分割比例。
//! 禁止：`Document` / `Rope` / 任何领域聚合根或应用层 DTO 的**副本**。
//!
//! 理由：UI 状态与业务状态一旦双向同步，就会出现「两边都以为自己对」
//! 的双写不一致。原型的 `editor.ts` 就踩过——撤销栈做成全局的，
//! 切换文件后 A 的内容被写进 B（缺陷 B-05）。
//! 本项目由`check-arch` +下面的静态断言共同保证这条边界。

use crate::presentation::shortcuts::dispatcher::ShortcutDispatcher;
use crate::presentation::shortcuts::registry::ShortcutRegistry;
use crate::presentation::state::scroll_sync_state::ScrollSync;
use crate::presentation::theme::builtin::Theme;

/// 侧边栏当前标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SidebarTab {
    /// 文件树。
    #[default]
    Files,
    /// 大纲。
    Outline,
    /// 全局搜索。
    Search,
    /// 格式检查（lint，V11-08）。
    Lint,
}

impl SidebarTab {
    /// 全部标签。
    pub const ALL: &'static [SidebarTab] = &[
        SidebarTab::Files,
        SidebarTab::Outline,
        SidebarTab::Search,
        SidebarTab::Lint,
    ];

    /// 中文显示名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Files => "文件",
            Self::Outline => "大纲",
            Self::Search => "搜索",
            Self::Lint => "检查",
        }
    }
}

/// 分割比例的合法区间。
///
/// 0.2 ~ 0.8 是原型的约定：留出20% 给一侧时另一侧仍够看全文。
/// 拖拽时必须夹紧，否则分割条能被拖到「编辑区完全消失」——
/// 那样用户既看不到编辑区也看不到预览区，只剩一条 4px 的条。
pub const SPLIT_MIN: f32 = 0.2;
/// 分割比例上界。
pub const SPLIT_MAX: f32 = 0.8;

/// 侧栏宽度的静态下界（pt）。
///
/// 再窄活动栏（48pt）+ 内容面板就放不下任何东西，文件树一行都显示不全。
pub const SIDEBAR_MIN: f32 = 180.0;
/// 侧栏宽度的静态上界（pt）。
///
/// 再宽侧栏自己就成了主角，编辑区被压到角落——且宽侧栏里并没有更多
/// 信息可放（树 / 大纲 / 搜索都是纵向列表）。
pub const SIDEBAR_MAX: f32 = 560.0;
/// 侧栏宽度占「编辑区 + 预览区」的比例上界。
///
/// 静态上界管不住**窄窗口**：1280pt 窗口下 560pt 侧栏还剩 716pt 主体，
/// 而 800pt 窗口下同样 560pt 只剩 236pt——编辑区被吃到几乎为零。
/// 故渲染时再按主体区宽度夹一次：`sw ≤ 0.6 × body`，
/// 即 `sw ≤ 0.6/(1+0.6) × (total − divider)`。
pub(crate) const SIDEBAR_BODY_SHARE_MAX: f32 = 0.6;

/// 应用级 UI 状态。
#[derive(Debug)]
pub struct AppUiState {
    /// 侧边栏是否可见。
    pub sidebar_visible: bool,
    /// 侧边栏当前标签。
    pub sidebar_tab: SidebarTab,
    /// 预览区是否可见。
    pub preview_visible: bool,
    /// 底部状态栏是否可见（S6-02：侧栏 / 预览 / 状态栏三者可独立开关）。
    pub status_bar_visible: bool,
    /// 编辑区占中间区域的宽度比（0.2 ~ 0.8）。
    pub split_ratio: f32,
    /// 本帧分割条是否正在被拖拽（**瞬态**，不持久化）。
    ///
    /// 用于在「拖拽结束」那一刻把 `split_ratio` 落盘到设置：
    /// 每帧都写配置文件太频繁，只在 `dragging: true → false` 的跳变时写一次。
    pub split_dragging: bool,
    /// 侧栏宽度（pt；用户拖拽定下的值，静态夹紧在 [`SIDEBAR_MIN`]~[`SIDEBAR_MAX`]）。
    ///
    /// 初始值取主题默认宽度（`Metrics::sidebar_width`），启动时被持久化
    /// 配置覆盖（见 `EditorApp::new`）。**渲染用的是
    /// [`Self::effective_sidebar_width`]**——本字段是「用户意图」，
    /// 生效宽还会按窗口宽度做一次动态夹紧，两者在窄窗口下不相等。
    pub sidebar_width: f32,
    /// 本帧侧栏分割条是否正在被拖拽（**瞬态**，不持久化）。
    ///
    /// 作用与 `split_dragging` 对称但目的相反：那里用于「松手即落盘」，
    /// 这里用于「拖拽中**不**落盘」——宽度每帧都在变，60fps 写配置文件
    /// 正是 `persist_split_ratio` 要避免的（见 `persist_sidebar_width`）。
    pub sidebar_dragging: bool,
    /// 专注 / 阅读模式：隐藏侧栏 / 工具栏 / 状态栏。
    ///
    /// 注意原型 A-27 要求「`Esc` 退出，浮动退出按钮」——
    /// 退出**只**通过 `CloseOverlay` 动作触发，不做硬编码（缺陷 B-03）。
    pub focus_mode: bool,
    /// 当前标签页索引。
    pub active_tab: usize,
    /// 打开的标签页数量。
    pub tab_count: usize,
    /// 标签条的横向滚动偏移（**会话级，不落盘**；单位 pt）。
    ///
    /// 为什么必须由我们自己持有，而不直接读 `ScrollArea` 的内部状态
    /// （问题 042 的根因之一）：egui 的滚动增量 `pass_state.scroll_delta`
    /// 是**全局**的、不分滚动区——任何视图调 `ui.scroll_to_rect` 都会写进去，
    /// 由**下一个 begin 的 ScrollArea** 消费。标签条在顶栏、总是最先执行，
    /// 于是把编辑区 / 预览区的滚动请求整个抢走，表现为标签条 offset
    /// 随机漂移、导航箭头「点了没反应」。把偏移提升为应用状态后，
    /// 每帧末尾无条件写回权威值，外部污染即被纠正。
    pub tabstrip_scroll_x: f32,
    /// 上一帧的活动文档 id，用于判定「活动标签是否刚变化」（会话级，不落盘）。
    ///
    /// 见 `draw_tab_strip` 里 `changed` 的注释：这个量**不能**放 egui temp
    /// memory，否则每帧都判成「变化」，reveal 会把用户的手动滚动逐帧撤销。
    pub tabstrip_last_active: Option<u64>,
    /// 当前主题。
    pub theme: Theme,
    /// 快捷键分发器（含唯一动作表）。
    pub dispatcher: ShortcutDispatcher,
    /// 双向滚动同步状态机（S6-06）。
    ///
    /// 放在这里而不是某个视图内部：编辑区与预览区是**两个**视图，
    /// 谁持有都不合适，而同步的判定必须看到两侧的位移。
    pub sync: ScrollSync,
}

impl Default for AppUiState {
    fn default() -> Self {
        Self::new(Theme::dark())
    }
}

impl AppUiState {
    /// 以给定主题构造。
    pub fn new(theme: Theme) -> Self {
        Self {
            // 首次打开默认**收起**侧栏（问题 061 用户诉求）：内容区优先——
            // 阅读与编辑是主任务，文件树按需展开（顶栏右上角的侧栏按钮 /
            // Ctrl+\ / 视图菜单都可展开）。会话级状态，不落盘。
            sidebar_visible: false,
            sidebar_tab: SidebarTab::Files,
            preview_visible: true,
            status_bar_visible: true,
            split_ratio: 0.5,
            split_dragging: false,
            // 主题默认宽度是「没拖过」时的渲染值；持久化配置（若有）
            // 在 `EditorApp::new` 里覆盖它。
            sidebar_width: theme.metrics.sidebar_width,
            sidebar_dragging: false,
            focus_mode: false,
            active_tab: 0,
            tab_count: 0,
            tabstrip_scroll_x: 0.0,
            tabstrip_last_active: None,
            theme,
            dispatcher: ShortcutDispatcher::default(),
            sync: ScrollSync::new(),
        }
    }

    /// 以应用层下发的快捷键构造（权威来源）。
    pub fn with_shortcuts(theme: Theme, registry: ShortcutRegistry) -> Self {
        let mut state = Self::new(theme);
        state.dispatcher = ShortcutDispatcher::new(registry);
        state
    }

    /// 切换侧边栏。
    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    /// 切换到某个侧边栏标签。
    ///
    /// 对齐原型 `toggleSidebar(tab)`：**再次点击同一标签会收起侧边栏**
    /// （而不是「点了没反应」）。这条细节很容易漏，漏了就是「标签栏点不动」。
    pub fn toggle_sidebar_tab(&mut self, tab: SidebarTab) {
        if self.sidebar_visible && self.sidebar_tab == tab {
            self.sidebar_visible = false;
            return;
        }
        self.sidebar_tab = tab;
        self.sidebar_visible = true;
    }

    /// 切换预览区。
    pub fn toggle_preview(&mut self) {
        self.preview_visible = !self.preview_visible;
    }

    /// 切换底部状态栏。
    pub fn toggle_statusbar(&mut self) {
        self.status_bar_visible = !self.status_bar_visible;
    }

    /// 切换专注模式。
    pub fn toggle_focus_mode(&mut self) {
        self.focus_mode = !self.focus_mode;
    }

    /// 退出专注模式（`Esc` / 浮动退出按钮调用）。
    pub fn exit_focus_mode(&mut self) {
        self.focus_mode = false;
    }

    /// 设置分割比例（自动夹紧到合法区间）。
    pub fn set_split_ratio(&mut self, ratio: f32) {
        self.split_ratio = ratio.clamp(SPLIT_MIN, SPLIT_MAX);
    }

    /// 设置侧栏宽度（自动夹紧到静态区间 [`SIDEBAR_MIN`]~[`SIDEBAR_MAX`]）。
    ///
    /// 只做**静态**夹紧：动态上界（不超过主体区的
    /// [`SIDEBAR_BODY_SHARE_MAX`]）依赖当帧窗口宽度，由
    /// [`Self::effective_sidebar_width`] 在渲染时算——存的值因此是
    /// 「用户意图」，窗口放大后能重新显形（例：窄窗里拖到 400，宽屏
    /// 上若存的是 560 就还是 560）。
    pub fn set_sidebar_width(&mut self, width: f32) {
        self.sidebar_width = width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
    }

    /// 侧栏的**生效**宽度：用户值再按窗口宽度做一次动态夹紧。
    ///
    /// `total_width` 是侧栏绘制前整个主体带的宽度（顶栏 / 状态栏已切走），
    /// `divider_width` 是侧栏分割条宽度。约束 `sw ≤ 0.6 × body` 展开即
    /// `sw ≤ 0.6/(1+0.6) × (total − divider)`。
    ///
    /// 极窄窗口下动态上界可能低于静态下界（如 400pt 窗口算出 148pt）：
    /// 此时以下界为准——侧栏被压到放不下内容比「除法给出负宽」好，
    /// 且那种窗口本就不在支持范围内（验收基线是 1280×800）。
    pub fn effective_sidebar_width(&self, total_width: f32, divider_width: f32) -> f32 {
        let dynamic_max =
            SIDEBAR_BODY_SHARE_MAX / (1.0 + SIDEBAR_BODY_SHARE_MAX) * (total_width - divider_width);
        let upper = dynamic_max.max(SIDEBAR_MIN);
        self.sidebar_width.clamp(SIDEBAR_MIN, upper)
    }

    /// 编辑区宽度占比。
    pub fn editor_ratio(&self) -> f32 {
        self.split_ratio
    }

    /// 预览区宽度占比。
    pub fn preview_ratio(&self) -> f32 {
        1.0 - self.split_ratio
    }

    /// 切换标签页（`next_tab` / `prev_tab`）。
    ///
    /// 返回是否真的切换了——在只有一个标签时返回 `false`，
    /// 调用方据此决定要不要提示（而不是静默无效）。
    pub fn cycle_tab(&mut self, delta: isize) -> bool {
        if self.tab_count == 0 {
            return false;
        }
        let len = self.tab_count as isize;
        let current = self.active_tab as isize;
        let next = (current + delta).rem_euclid(len);
        if next == current {
            return false;
        }
        self.active_tab = next as usize;
        true
    }

    /// 跳转到指定序号的标签（`Alt+1` ~ `Alt+9`，1-based）。
    pub fn goto_tab(&mut self, one_based: usize) -> bool {
        if one_based == 0 || one_based > self.tab_count {
            return false;
        }
        self.active_tab = one_based - 1;
        true
    }

    /// 应用主题变更（会同步到 egui 与预览缓存签名）。
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }
}
