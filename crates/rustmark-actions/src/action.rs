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
    /// 新建空白文档（开一个未命名标签页）。
    NewDocument,
    /// 通过系统对话框打开 Markdown 文件为标签页。
    OpenDocument,
    /// 打开文件夹作为工作区，侧边栏显示文件树（纯 UI，不参与改键持久化）。
    OpenFolder,
    /// 保存当前标签页到其磁盘路径；未命名文档转为「另存为」。
    Save,
    /// 另存为：弹路径选择，落盘后标签页指向新路径。
    SaveAs,
    /// 保存全部含未落盘改动的标签页。
    SaveAll,
    /// 关闭当前标签页；有未保存改动时先走确认流程。
    CloseTab,
    /// 导出当前文档（PDF / HTML / 图片等），打开导出对话框。
    Export,
    // —— 编辑 ——
    /// 撤销最近一次编辑（作用于编辑区undo栈）。
    Undo,
    /// 重做被撤销的编辑。
    Redo,
    /// 在当前文档内查找（Ctrl+F，打开查找浮层）。
    Find,
    /// 在当前文档内查找并替换（Ctrl+H）。
    Replace,
    /// 跨工作区全部文件全局查找（打开「在所有文件中查找」面板）。
    FindInFiles,
    /// 丢弃当前文档的未保存改动，还原为磁盘内容（纯 UI，不参与改键持久化）。
    Revert,
    // —— 格式（需编辑区焦点）——
    /// 切换选区的加粗包裹（`**…**`）。
    Bold,
    /// 切换选区的斜体包裹（`*…*`）。
    Italic,
    /// 切换选区的删除线包裹（`~~…~~`）。
    Strikethrough,
    /// 切换选区的行内代码包裹（`` `…` ``）。
    InlineCode,
    /// 插入链接（包裹选区或插入空链接骨架，弹 URL 输入）。
    InsertLink,
    /// 把选区包进围栏代码块（可选语言标注）。
    CodeBlock,
    /// 在光标处插入表格骨架。
    InsertTable,
    /// 插入图片语法（纯 UI，不参与改键持久化）。
    InsertImage,
    /// 切换当前段落的块引用前缀（`> `）。
    ToggleQuote,
    /// 切换当前段落为无序列表项（`- `；纯 UI，不参与改键持久化）。
    ToggleList,
    /// 切换当前段落为有序列表项（`1. `；纯 UI，不参与改键持久化）。
    ToggleOrderedList,
    /// 提升标题级别（`##` → `#`；已在 h1 时无操作）。
    PromoteHeading,
    /// 降低标题级别（`#` → `##`；非标题行无操作）。
    DemoteHeading,
    /// 增加缩进（列表嵌套加深或行首插入缩进）。
    Indent,
    /// 减少一级缩进（列表嵌套变浅；已无缩进时无操作）。
    Outdent,
    /// 切换光标处任务项的勾选状态（`- [ ]` ↔ `- [x]`）。
    ToggleTask,
    /// 当前行整体上移（纯 UI，不参与改键持久化）。
    MoveLineUp,
    /// 当前行整体下移（纯 UI，不参与改键持久化）。
    MoveLineDown,
    // —— 视图 ——
    /// 显示 / 隐藏左侧边栏（文件树等）。
    ToggleSidebar,
    /// 显示 / 隐藏右侧预览区。
    TogglePreview,
    /// 侧边栏切到大纲视图（再次触发切回原视图）。
    ToggleOutline,
    /// 专注 / 阅读模式：隐藏周边 chrome，只留正文。
    ToggleFocusMode,
    /// 显示 / 隐藏底部状态栏。
    ToggleStatusbar,
    /// 关闭当前浮层（对话框 / 查找面板 / 命令面板的统一 Esc 出口）。
    CloseOverlay,
    /// 打开文档统计（字数、字符、段落等计数）。
    DocumentStats,
    /// 打开命令面板（全动作模糊检索入口）。
    CommandPalette,
    /// 打开符号 / Emoji 插入选择器。
    InsertSymbol,
    /// 打开文件差异对比视图。
    CompareDiff,
    /// 激活下一个标签页（末尾回绕到第一个）。
    NextTab,
    /// 激活上一个标签页（开头回绕到最后一个）。
    PrevTab,
    /// 跳转到第 1 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab1,
    /// 跳转到第 2 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab2,
    /// 跳转到第 3 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab3,
    /// 跳转到第 4 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab4,
    /// 跳转到第 5 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab5,
    /// 跳转到第 6 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab6,
    /// 跳转到第 7 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab7,
    /// 跳转到第 8 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab8,
    /// 跳转到第 9 个标签页（序号越界时无操作；纯 UI，不参与改键持久化）。
    GotoTab9,
    // —— 设置 / 帮助 ——
    /// 打开设置窗口。
    OpenSettings,
    /// 打开快捷键一览（纯 UI，不参与改键持久化）。
    ShortcutOverview,
    /// 打开「关于」对话框（版本、许可；纯 UI，不参与改键持久化）。
    About,
    /// 在深色 / 浅色主题间切换（纯 UI，不参与改键持久化）。
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
