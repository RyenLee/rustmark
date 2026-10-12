//! 动作枚举与菜单分组：菜单 / 工具栏 / 命令面板 / 快捷键共用的唯一动作表。
//!
//! 固有 impl 块拆至兄弟模块：
//! - [`action::impls`] — `UiAction` 的常量注册表 / 元信息 / 分组 / 快捷键
//! - [`action::menu_group`] — `MenuGroup` 的分组常量 / 显示名

mod impls;
mod menu_group;

// ============================================================================
// 动作枚举
// ============================================================================

/// 可被界面触发的全部动作（菜单 / 工具栏 / 命令面板 / 快捷键共用同一份）。
///
/// 与领域 `ActionId` 的对齐契约是 [`UiAction::persisted_key`]：
/// 新增**可持久化**动作时必须同时在领域 `ActionId` 加同名列，
/// 否则用户改键无法落盘，重启后丢失。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum UiAction {
    // —— 文件 ——
    NewDocument,
    OpenDocument,
    OpenFolder,
    Save,
    SaveAs,
    SaveAll,
    CloseTab,
    Export,
    // —— 编辑 ——
    Undo,
    Redo,
    Find,
    Replace,
    FindInFiles,
    Revert,
    // —— 格式（需编辑区焦点）——
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    InsertLink,
    CodeBlock,
    InsertTable,
    InsertImage,
    ToggleQuote,
    ToggleList,
    ToggleOrderedList,
    PromoteHeading,
    DemoteHeading,
    Indent,
    Outdent,
    ToggleTask,
    MoveLineUp,
    MoveLineDown,
    // —— 视图 ——
    ToggleSidebar,
    TogglePreview,
    ToggleOutline,
    ToggleFocusMode,
    ToggleStatusbar,
    CloseOverlay,
    DocumentStats,
    CommandPalette,
    InsertSymbol,
    CompareDiff,
    NextTab,
    PrevTab,
    GotoTab1,
    GotoTab2,
    GotoTab3,
    GotoTab4,
    GotoTab5,
    GotoTab6,
    GotoTab7,
    GotoTab8,
    GotoTab9,
    // —— 设置 / 帮助 ——
    OpenSettings,
    ShortcutOverview,
    About,
    ToggleTheme,
}

/// 菜单分组。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum MenuGroup {
    /// 文件
    File,
    /// 编辑
    Edit,
    /// 格式
    Format,
    /// 视图
    View,
    /// 设置 / 帮助
    Help,
}
