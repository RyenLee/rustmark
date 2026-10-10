//! 浮层 UI 状态（对话框集合、模态）。
//!
//! # 浮层栈
//!
//! 对话框可叠加（如「设置」上再开「关于」），故用栈而非单值建模。
//! 验收要求「打开浮层期间背景不可交互」（原型 A-28），由 [`Self::is_modal`]
//! 依据浮层是否模态判定——查找面板是非模态工具面板，打开时编辑区仍可编辑。

use crate::presentation::shortcuts::registry::UiAction;

/// 可打开的浮层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    /// 查找 / 替换面板。
    Find,
    /// 设置对话框。
    Settings,
    /// 导出对话框。
    Export,
    /// 快速打开。
    QuickOpen,
    /// 快捷键一览。
    ShortcutOverview,
    /// 关于。
    About,
    /// 文档统计。
    Stats,
    /// 命令面板。
    CommandPalette,
    /// 符号 / Emoji 面板。
    Symbol,
    /// 差异对比。
    Diff,
    // 注意：`InsertLink` / `InsertTable` / `CodeBlock` **不再**对应浮层。
    // 它们曾是 `Overlay::Link` / `Table` / `Code`，但那三个浮层的视图
    // 从未实现（文件只有一行占位），于是动作被 `openable_overlay`
    // 拦截后**什么都不做**——菜单里点「插入链接」毫无反应，
    // 快捷键 Ctrl+K 同样无效。S6-07 起这三个动作直接执行插入
    // （生成可编辑的占位符），对话框形态留待真正需要时再加。
}

impl Overlay {
    /// 全部浮层（遍历用；「关闭全部浮层」与焦点兜底扫描）。
    pub const ALL: &'static [Overlay] = &[
        Overlay::Find,
        Overlay::Settings,
        Overlay::Export,
        Overlay::QuickOpen,
        Overlay::ShortcutOverview,
        Overlay::About,
        Overlay::Stats,
        Overlay::CommandPalette,
        Overlay::Symbol,
        Overlay::Diff,
    ];

    /// 中文标题（对话框顶栏）。
    pub const fn title(self) -> &'static str {
        match self {
            Self::Find => "查找和替换",
            Self::Settings => "设置",
            Self::Export => "导出",
            Self::QuickOpen => "快速打开",
            Self::ShortcutOverview => "快捷键一览",
            Self::About => "关于",
            Self::Stats => "文档统计",
            Self::CommandPalette => "命令面板",
            Self::Symbol => "插入符号",
            Self::Diff => "对比文件差异",
        }
    }

    /// 该浮层由哪个动作打开（用于焦点归还与模态判定）。
    pub const fn opened_by(self) -> Option<UiAction> {
        Some(match self {
            Self::Find => UiAction::Find,
            Self::Settings => UiAction::OpenSettings,
            Self::Export => UiAction::Export,
            Self::ShortcutOverview => UiAction::ShortcutOverview,
            Self::About => UiAction::About,
            Self::Stats => UiAction::DocumentStats,
            Self::CommandPalette => UiAction::CommandPalette,
            Self::Symbol => UiAction::InsertSymbol,
            Self::Diff => UiAction::CompareDiff,
            Self::QuickOpen => UiAction::OpenDocument,
        })
    }

    /// 该浮层的**附加**开启动作（[`Self::opened_by`] 之外的）。
    ///
    /// 查找与替换共用同一个面板（原型 `FindReplacePanel.vue` 也是同一个
    /// 组件，`replaceOpen` 只控制替换框是否展开）。但它们是**两个**动作、
    /// 两个键位（`Ctrl+F` / `Ctrl+H`）、两条菜单项。
    ///
    /// 为什么不把这张表也做成「唯一的反向映射」：因为 S6-07 正是因为
    /// 「`opened_by` 反查得到浮层，却没有第二个动作能打开它」而让
    /// `Ctrl+H` 静默失效（动作落进 `_ => {}`，界面正常、功能无反应）。
    /// 现在 `Replace` 明确登记在这里，并有测试断言
    /// 「每个动作都能打开某个浮层」——**漏登记就红**。
    pub const fn also_opened_by(self) -> Option<UiAction> {
        match self {
            Self::Find => Some(UiAction::Replace),
            _ => None,
        }
    }

    /// 该浮层是否为**模态**（打开时背景不可交互）。
    ///
    /// 查找面板是唯一的**非模态**浮层，理由来自原型：它是浮在编辑区右上角的
    /// 工具面板，编辑区**仍可编辑、仍可滚动**，用户可以一边看命中一边继续改。
    /// 若把它当模态，`is_modal()` 会让分发器拦下所有「打开浮层」的动作，
    /// 且用户会发现「按了 Ctrl+F 之后整个编辑器都不能用了」。
    pub const fn is_modal(self) -> bool {
        !matches!(self, Self::Find)
    }
}

/// 浮层 UI 状态。
#[derive(Clone, Debug, Default)]
pub struct DialogUiState {
    /// 当前打开的浮层栈（栈而非单值：对话框可叠加，如「设置」上再开「关于」）。
    open: Vec<Overlay>,
    /// 需要关闭的浮层（`Esc` 的消费标记）。
    pending_close: Option<Overlay>,
}

impl DialogUiState {
    /// 构造默认状态（无浮层）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 是否有任何浮层打开。
    pub fn any_open(&self) -> bool {
        !self.open.is_empty()
    }

    /// 栈顶浮层（最近打开的）。
    pub fn top(&self) -> Option<Overlay> {
        self.open.last().copied()
    }

    /// 某浮层是否打开。
    pub fn is_open(&self, overlay: Overlay) -> bool {
        self.open.contains(&overlay)
    }

    /// 打开浮层。
    ///
    /// 重复打开同一浮层时**不**重复入栈，否则栈里会出现两项相同浮层，
    /// `Esc` 一次关不掉。
    pub fn open(&mut self, overlay: Overlay) {
        if self.is_open(overlay) {
            // 已打开：提到栈顶（最近使用语义），不重复入栈——
            // 否则 `Esc` 一次关不掉。
            self.close(overlay);
        }
        self.open.push(overlay);
    }

    /// 关闭栈顶浮层。
    ///
    /// 返回被关闭的浮层与它的来源动作（供焦点归还）。
    pub fn close_top(&mut self) -> Option<Overlay> {
        self.open.pop()
    }

    /// 关闭指定浮层（不论它在栈中的位置）。
    pub fn close(&mut self, overlay: Overlay) -> bool {
        let Some(index) = self.open.iter().rposition(|o| *o == overlay) else {
            return false;
        };
        self.open.remove(index);
        true
    }

    /// 关闭全部浮层。
    pub fn close_all(&mut self) {
        self.open.clear();
        self.pending_close = None;
    }

    /// 请求关闭栈顶浮层（`Esc` 走这条路径）。
    ///
    /// 标记而非直接关闭，是为了让分发器与渲染分离：
    /// `Esc` 可能同时被「关闭浮层」和「取消输入」消费，需要仲裁点。
    pub fn request_close(&mut self) {
        self.pending_close = self.top();
    }

    /// 取出并清除待关闭请求。
    pub fn take_pending_close(&mut self) -> Option<Overlay> {
        self.pending_close.take()
    }

    /// 是否处于模态（有**模态**浮层打开时背景不可交互，A-28）。
    ///
    /// 判据是「栈里是否存在模态浮层」而非「栈非空」：查找面板虽然打开，
    /// 但它是非模态工具面板，若被算作模态，用户会发现「按了 Ctrl+F 之后
    /// 编辑区不能用了」——而原型里编辑区始终可编辑。
    pub fn is_modal(&self) -> bool {
        self.open.iter().any(|overlay| overlay.is_modal())
    }

    /// 文档切换后关闭全部浮层（避免对已关闭文档的浮层残留）。
    pub fn reset_for_new_document(&mut self) {
        self.close_all();
    }
}
