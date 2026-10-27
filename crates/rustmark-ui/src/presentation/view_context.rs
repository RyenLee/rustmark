//! 视图窄接口 [`EditorViewContext`]（R-03 B1，2026-10-27 开工）。
//!
//! # 它解决的是什么问题
//!
//! R-03 B2 之前，`src/presentation/views/` 下 36 个视图文件一律用
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
//! # 为什么放在 `rustmark-ui` 而不是 root
//!
//! 视图最终要整体迁进本 crate（B1 步骤 3）。trait 若留在 root，视图搬过来时
//! 必须反向依赖 root——依赖方向立刻成环。放这里则**搬迁时视图签名零改动**，
//! 只需把 `crate::app::frame::EditorApp` 的 `impl` 留在 root 即可。
//!
//! # 为什么端口里没有 `UiAction`
//!
//! 工具栏 / 菜单 / 命令面板走的是 `execute(UiAction)`，但 `UiAction` 属
//! `rustmark-actions`，而本 crate 当前只依赖 `egui` + `rustmark-core`
//! （依赖面由 `tests/workspace_topology_test.rs` 断言）。把 `UiAction`
//! 塞进来就要新增一条 `ui → actions` 的 crate 边，为一批视图付一笔全局依赖账。
//! 故端口只收**语义命令**（`insert_at_cursor` 这种），`UiAction` 入口留给
//! 仍在 root 的聚合视图，待其迁移时再议。
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
use crate::presentation::state::dialog_ui_state::DialogUiState;
use crate::presentation::state::shortcut_overview_ui_state::ShortcutOverviewUiState;
use crate::presentation::state::symbol_ui_state::SymbolUiState;
use crate::presentation::theme::builtin::Theme;
use rustmark_core::application::dto::{StatsView, TabSummary};
use rustmark_core::shared::result::AppResult;

/// 视图可用的窄接口：只暴露「这个视图真正需要」的状态与命令。
///
/// 实现方是 root 的 `EditorApp`（见 `src/app/frame/view_context.rs`）。
/// 视图侧一律以 `&mut dyn EditorViewContext` 接收，**不得**再出现
/// `EditorApp`——由 `tools/arch_check.rs::check_view_decoupling` 机械拦截。
pub trait EditorViewContext {
    // ------------------------------------------------------------------
    // 状态访问
    // ------------------------------------------------------------------

    /// 浮层栈状态（打开 / 关闭 / 栈顶 / 首次打开取焦）。
    fn dialog_state(&mut self) -> &mut DialogUiState;

    /// 符号面板状态（分组 / 查询 / 选中项）。
    fn symbol_ui_state(&mut self) -> &mut SymbolUiState;

    /// 快捷键一览的过滤词与分组缓存。
    ///
    /// 只读：改过滤词走 [`Self::set_shortcut_overview_query`] 这条命令，
    /// 不在视图里直接写字段——重置逻辑归命令所有，视图只负责画。
    fn shortcut_overview_ui_state(&self) -> &ShortcutOverviewUiState;

    /// 当前主题。
    fn theme_state(&self) -> &Theme;

    /// 生效的快捷键注册表（菜单 / 命令面板 / 一览三处同源）。
    fn dispatcher_state(&self) -> &ShortcutDispatcher;

    // ------------------------------------------------------------------
    // 只读查询
    // ------------------------------------------------------------------

    /// 当前主题的显示名（「浅色」「深色」「跟随系统」…）。
    fn theme_kind_label(&self) -> &'static str;

    /// 已打开文档的摘要列表（标签栏顺序）。
    fn open_document_list(&self) -> Vec<TabSummary>;

    /// 工作区根路径；未打开工作区时为空串。
    fn workspace_root(&self) -> String;

    /// 当前文档的统计；无活动文档时为 `None`。
    ///
    /// 口径唯一：与状态栏取自同一个 `compute_stats`，视图**不得**自行重算。
    fn current_stats(&self) -> Option<StatsView>;

    // ------------------------------------------------------------------
    // 语义命令
    // ------------------------------------------------------------------

    /// 在光标处插入文本（进领域 → 成为一个撤销点 → 刷新预览）。
    fn insert_at_cursor(&mut self, text: &str) -> AppResult<()>;

    /// 设置快捷键一览的过滤词（含重置语义，见实现方）。
    fn set_shortcut_overview_query(&mut self, query: String);
}
