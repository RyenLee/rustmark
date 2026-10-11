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

    /// 由字符串解析（配置持久化用）。
    ///
    /// 未知值退回「文件」而不是报错：侧栏标签名可能随版本增删，
    /// 旧配置里的废弃名不该让整个配置加载失败。
    pub fn parse(raw: &str) -> Self {
        match raw {
            "outline" => Self::Outline,
            "search" => Self::Search,
            "lint" => Self::Lint,
            _ => Self::Files,
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
    /// 专注 / 阅读模式：隐藏侧栏 / 工具栏 / 状态栏。
    ///
    /// 注意原型 A-27 要求「`Esc` 退出，浮动退出按钮」——
    /// 退出**只**通过 `CloseOverlay` 动作触发，不做硬编码（缺陷 B-03）。
    pub focus_mode: bool,
    /// 当前标签页索引。
    pub active_tab: usize,
    /// 打开的标签页数量。
    pub tab_count: usize,
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
            sidebar_visible: true,
            sidebar_tab: SidebarTab::Files,
            preview_visible: true,
            status_bar_visible: true,
            split_ratio: 0.5,
            split_dragging: false,
            focus_mode: false,
            active_tab: 0,
            tab_count: 0,
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

    /// 编辑区宽度占比。
    pub fn editor_ratio(&self) -> f32 {
        self.split_ratio
    }

    /// 预览区宽度占比。
    pub fn preview_ratio(&self) -> f32 {
        1.0 - self.split_ratio
    }

    /// 是否处于「编辑区独占」状态（预览关闭）。
    ///
    /// 专注模式下仍保留编辑区与预览区，只隐藏**外围** UI（原型 A-27）。
    pub fn is_editor_only(&self) -> bool {
        !self.preview_visible
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
