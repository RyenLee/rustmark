//! 浮层 UI 状态（对话框集合、模态）。
//!
//! # 浮层栈
//!
//! 对话框可叠加（如「设置」上再开「关于」），故用栈而非单值建模。
//! 验收要求「打开浮层期间背景不可交互」（原型 A-28），由 [`Self::is_modal`]
//! 依据浮层是否模态判定——查找面板是非模态工具面板，打开时编辑区仍可编辑。

use rustmark_core::presentation::shortcuts::registry::UiAction;

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
    /// 已经申请过「打开即聚焦」的浮层（一次性闩锁）。
    ///
    /// # 为什么不写成「当前没有焦点就每帧请求」
    ///
    /// 那种写法看着更稳（焦点丢了会自己补回来），实际**恒为真**：
    /// `Memory::has_focus(id)` 只认**控件** id，而 `egui::Window` 的 id
    /// 属于 `Area`——egui 全库只有 `TextEdit` / `DragValue` 会
    /// `request_focus`，没有任何地方会让窗口 id 成为「有焦点的控件」。
    /// 于是 `!has_focus(窗口 id)` 永远成立，「打开时给一次」退化成
    /// 「每帧都抢」：用户点进同一浮层里的**第二个**输入框会被立刻拽回第一个。
    /// 导出浮层的「文件名」框就因此一个字也打不进去（而单输入框的浮层
    /// 看起来正常，所以这个坑很晚才暴露）。
    ///
    /// 故改为**边沿触发**：浮层打开时登记一次，视图取走后不再重复申请。
    /// 重新打开会重新登记（见 [`Self::open`]）。
    focus_granted: Vec<Overlay>,
}

impl DialogUiState {
    /// 构造默认状态（无浮层）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 浮层是否**刚打开且还没申请过聚焦**；返回 `true` 的同时记下已申请。
    ///
    /// 视图应在画完浮层之后调用，用来决定「本帧要不要把焦点交给输入框」。
    /// 每帧调用是安全的：除打开后的第一次外都返回 `false`。
    ///
    /// 刻意不做「取不到焦点就再试一次」的补偿：那需要区分「控件还没画出来」
    /// 与「用户已经自己点了别处」，而两者在 egui 里无法区分——
    /// 猜错的代价正是本方法存在的原因（每帧抢焦点）。
    pub fn take_open_focus(&mut self, overlay: Overlay) -> bool {
        if self.focus_granted.contains(&overlay) {
            return false;
        }
        self.focus_granted.push(overlay);
        true
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
    ///
    /// 同时**重新登记**「打开即聚焦」（见 [`Self::take_open_focus`]）：
    /// 已打开的浮层被再次打开（如 `Ctrl+F` 时查找面板本就在）属于
    /// 「用户又唤起了它」，此时把光标送回输入框正是他想要的。
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
        // 顺手撤掉「打开即聚焦」的登记：下次打开要能重新给一次焦点。
        self.focus_granted.retain(|o| *o != overlay);
        true
    }

    /// 关闭全部浮层。
    pub fn close_all(&mut self) {
        self.open.clear();
        self.pending_close = None;
        self.focus_granted.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 「打开即聚焦」是**边沿触发**：只给一次，之后每帧都返回 `false`。
    ///
    /// 这条是导出浮层「点不进文件名框」的回归防线：若哪天有人把视图里的
    /// 判断改回 `!m.has_focus(...)`，本方法就不会再被当成一次性闩锁用，
    /// 而 `focus_granted` 的语义会在这里先红。
    #[test]
    fn open_focus_is_granted_once_per_open() {
        let mut state = DialogUiState::new();
        state.open(Overlay::Export);

        assert!(state.take_open_focus(Overlay::Export), "打开后应给一次");
        for _ in 0..3 {
            assert!(
                !state.take_open_focus(Overlay::Export),
                "同一轮打开里只能给一次，否则每帧抢焦点"
            );
        }
    }

    /// 关掉再打开要能重新给一次焦点。
    #[test]
    fn closing_and_reopening_grants_focus_again() {
        let mut state = DialogUiState::new();
        state.open(Overlay::Export);
        assert!(state.take_open_focus(Overlay::Export));

        state.close(Overlay::Export);
        state.open(Overlay::Export);
        assert!(
            state.take_open_focus(Overlay::Export),
            "重新打开应重新给一次焦点"
        );
    }

    /// 重复打开**同一**浮层（已打开时再次触发动作）也要重新给焦点。
    #[test]
    fn reopening_an_already_open_overlay_grants_focus_again() {
        let mut state = DialogUiState::new();
        state.open(Overlay::Find);
        assert!(state.take_open_focus(Overlay::Find));

        // 面板已打开时再按一次 `Ctrl+F`：属于「又唤起了它」。
        state.open(Overlay::Find);
        assert!(state.take_open_focus(Overlay::Find));
    }

    /// 关掉全部浮层会撤掉登记（切文档后不得把焦点留给旧浮层）。
    #[test]
    fn close_all_clears_the_focus_latch() {
        let mut state = DialogUiState::new();
        state.open(Overlay::Settings);
        assert!(state.take_open_focus(Overlay::Settings));

        state.close_all();

        state.open(Overlay::Settings);
        assert!(
            state.take_open_focus(Overlay::Settings),
            "`close_all` 之后重新打开应重新给一次"
        );
    }

    /// 各浮层的登记互不干扰。
    #[test]
    fn the_latch_is_per_overlay() {
        let mut state = DialogUiState::new();
        state.open(Overlay::Settings);
        state.open(Overlay::About);

        assert!(state.take_open_focus(Overlay::About));
        assert!(!state.take_open_focus(Overlay::About));
        assert!(
            state.take_open_focus(Overlay::Settings),
            "另一个浮层的登记不该被牵连"
        );
    }
}
