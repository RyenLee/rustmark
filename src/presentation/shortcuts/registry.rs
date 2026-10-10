//! 动作表与快捷键注册表（阶段 06 · S6-09）。
//!
//! # 这个文件存在的唯一理由：消灭「四处真相」
//!
//! 原型的快捷键有**四个**互不相干的来源：菜单项上写的文字、
//! `mock/shortcuts.ts` 的默认表、`useHotkeys.ts` 的匹配器、
//! 设置弹窗里的录制列表。改一个键要改四处，漏一处就静默失效
//! （原型缺陷 B-02：`Ctrl+Shift+S` 声明了但从没注册过）。
//!
//! 本模块把四合一：
//!
//! ```text
//!                ShortcutRegistry（唯一动作表+ 绑定）
//!                     │
//!   ┌─────────────────┼─────────────────┐
//!   ↓                 ↓                 ↓
//! 菜单显示         命令面板显示     keydown 分发
//! ```
//!
//! 三处**都**只能问 [`ShortcutRegistry`]，谁都不许自己写一份键位。
//! 注册遗漏在结构上不可能发生——因为「注册」本身就是「遍历这张表」。
//!
//! # 与领域层的关系
//!
//! 表现层**不得**依赖 `domain`（`check-arch` 会拦），所以这里：
//! - 用 [`UiAction`] 这套自有枚举表达动作，与领域 `ActionId` 通过
//!   [`UiAction::persisted_key`] 的**字符串契约**对齐，而���是类型依赖；
//! - 归一化 / 显示规则**逐位复刻**领域 `KeyCombo`（见 [`normalize`]），
//!   并由 `tests/presentation/shortcut_registry_test.rs` 做**差分测试**锁定两者一致。
//!
//! 绝不能「顺手在表现层 import 领域类型」——那会让整个分层失效，
//! 也会让「改键要改两处」的问题从四处变成两处。

use std::collections::HashMap;

use crate::application::dto::ShortcutView;

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

impl UiAction {
    /// 全部动作（遍历用；「快捷键一览」与命令面板的全集）。
    pub const ALL: &'static [UiAction] = &[
        UiAction::NewDocument,
        UiAction::OpenDocument,
        UiAction::OpenFolder,
        UiAction::Save,
        UiAction::SaveAs,
        UiAction::SaveAll,
        UiAction::CloseTab,
        UiAction::Export,
        UiAction::Undo,
        UiAction::Redo,
        UiAction::Find,
        UiAction::Replace,
        UiAction::FindInFiles,
        UiAction::Revert,
        UiAction::Bold,
        UiAction::Italic,
        UiAction::Strikethrough,
        UiAction::InlineCode,
        UiAction::InsertLink,
        UiAction::CodeBlock,
        UiAction::InsertTable,
        UiAction::InsertImage,
        UiAction::ToggleQuote,
        UiAction::ToggleList,
        UiAction::ToggleOrderedList,
        UiAction::PromoteHeading,
        UiAction::DemoteHeading,
        UiAction::Indent,
        UiAction::Outdent,
        UiAction::ToggleTask,
        UiAction::MoveLineUp,
        UiAction::MoveLineDown,
        UiAction::ToggleSidebar,
        UiAction::TogglePreview,
        UiAction::ToggleOutline,
        UiAction::ToggleFocusMode,
        UiAction::ToggleStatusbar,
        UiAction::CloseOverlay,
        UiAction::DocumentStats,
        UiAction::CommandPalette,
        UiAction::InsertSymbol,
        UiAction::CompareDiff,
        UiAction::NextTab,
        UiAction::PrevTab,
        UiAction::GotoTab1,
        UiAction::GotoTab2,
        UiAction::GotoTab3,
        UiAction::GotoTab4,
        UiAction::GotoTab5,
        UiAction::GotoTab6,
        UiAction::GotoTab7,
        UiAction::GotoTab8,
        UiAction::GotoTab9,
        UiAction::OpenSettings,
        UiAction::ShortcutOverview,
        UiAction::About,
        UiAction::ToggleTheme,
    ];

    /// 稳定 `snake_case` 键，与领域 `ActionId::key()` **逐字一致**。
    ///
    /// 只有领域里存在的动作才返回 `Some`——其余（`Revert` / `ToggleList` /
    /// `InsertImage` / `GotoTabN` / `ShortcutOverview` / `About`）是纯 UI 动作，
    /// 没有配置项，不参与改键持久化。
    pub const fn persisted_key(self) -> Option<&'static str> {
        Some(match self {
            UiAction::NewDocument => "new_document",
            UiAction::OpenDocument => "open_document",
            UiAction::Save => "save",
            UiAction::SaveAs => "save_as",
            UiAction::SaveAll => "save_all",
            UiAction::CloseTab => "close_tab",
            UiAction::Export => "export",
            UiAction::Undo => "undo",
            UiAction::Redo => "redo",
            UiAction::Find => "find",
            UiAction::Replace => "replace",
            UiAction::FindInFiles => "find_in_files",
            UiAction::Bold => "bold",
            UiAction::Italic => "italic",
            UiAction::Strikethrough => "strikethrough",
            UiAction::InlineCode => "inline_code",
            UiAction::InsertLink => "insert_link",
            UiAction::CodeBlock => "code_block",
            UiAction::InsertTable => "insert_table",
            UiAction::ToggleQuote => "toggle_quote",
            UiAction::PromoteHeading => "promote_heading",
            UiAction::DemoteHeading => "demote_heading",
            UiAction::Indent => "indent",
            UiAction::Outdent => "outdent",
            UiAction::ToggleTask => "toggle_task",
            UiAction::TogglePreview => "toggle_preview",
            UiAction::ToggleOutline => "toggle_outline",
            UiAction::ToggleSidebar => "toggle_sidebar",
            UiAction::ToggleStatusbar => "toggle_statusbar",
            UiAction::CommandPalette => "command_palette",
            UiAction::OpenSettings => "open_settings",
            UiAction::ToggleFocusMode => "focus_mode",
            UiAction::CloseOverlay => "close_overlay",
            UiAction::DocumentStats => "document_stats",
            UiAction::InsertSymbol => "insert_symbol",
            UiAction::CompareDiff => "compare_diff",
            UiAction::NextTab => "next_tab",
            UiAction::PrevTab => "prev_tab",
            _ => return None,
        })
    }

    /// 由持久化键反查动作。
    pub fn from_persisted_key(key: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.persisted_key() == Some(key))
    }

    /// 中文显示名（菜单 / 命令面板 / 一览三处共用此**唯一**来源）。
    pub const fn label(self) -> &'static str {
        match self {
            UiAction::NewDocument => "新建文档",
            UiAction::OpenDocument => "打开文档",
            UiAction::OpenFolder => "打开文件夹",
            UiAction::Save => "保存",
            UiAction::SaveAs => "另存为",
            UiAction::SaveAll => "保存全部",
            UiAction::CloseTab => "关闭标签页",
            UiAction::Export => "导出…",
            UiAction::Undo => "撤销",
            UiAction::Redo => "重做",
            UiAction::Find => "查找",
            UiAction::Replace => "替换",
            UiAction::FindInFiles => "在所有文件中查找",
            UiAction::Revert => "还原修改",
            UiAction::Bold => "加粗",
            UiAction::Italic => "斜体",
            UiAction::Strikethrough => "删除线",
            UiAction::InlineCode => "行内代码",
            UiAction::InsertLink => "插入链接…",
            UiAction::CodeBlock => "插入代码块…",
            UiAction::InsertTable => "插入表格…",
            UiAction::InsertImage => "插入图片",
            UiAction::ToggleQuote => "引用",
            UiAction::ToggleList => "无序列表",
            UiAction::ToggleOrderedList => "有序列表",
            UiAction::PromoteHeading => "提升标题级别",
            UiAction::DemoteHeading => "降低标题级别",
            UiAction::Indent => "缩进",
            UiAction::Outdent => "反缩进",
            UiAction::ToggleTask => "切换任务勾选",
            UiAction::MoveLineUp => "上移当前行",
            UiAction::MoveLineDown => "下移当前行",
            UiAction::ToggleSidebar => "显示 / 隐藏侧边栏",
            UiAction::ToggleStatusbar => "显示 / 隐藏状态栏",
            UiAction::TogglePreview => "显示 / 隐藏预览区",
            UiAction::ToggleOutline => "侧边栏：大纲",
            UiAction::ToggleFocusMode => "专注 / 阅读模式",
            UiAction::CloseOverlay => "关闭浮层",
            UiAction::DocumentStats => "文档统计",
            UiAction::CommandPalette => "命令面板",
            UiAction::InsertSymbol => "插入符号 / Emoji",
            UiAction::CompareDiff => "对比文件差异",
            UiAction::NextTab => "下一个标签页",
            UiAction::PrevTab => "上一个标签页",
            UiAction::GotoTab1 => "跳转到第 1 个标签页",
            UiAction::GotoTab2 => "跳转到第 2 个标签页",
            UiAction::GotoTab3 => "跳转到第 3 个标签页",
            UiAction::GotoTab4 => "跳转到第 4 个标签页",
            UiAction::GotoTab5 => "跳转到第 5 个标签页",
            UiAction::GotoTab6 => "跳转到第 6 个标签页",
            UiAction::GotoTab7 => "跳转到第 7 个标签页",
            UiAction::GotoTab8 => "跳转到第 8 个标签页",
            UiAction::GotoTab9 => "跳转到第 9 个标签页",
            UiAction::OpenSettings => "设置",
            UiAction::ShortcutOverview => "快捷键一览",
            UiAction::About => "关于",
            UiAction::ToggleTheme => "切换主题（深 / 浅 / 跟随系统）",
        }
    }

    /// **编辑区工具栏**按钮上的文案（比 [`UiAction::label`] 短）。
    ///
    /// # 为什么工具栏需要一份更短的文案
    ///
    /// `label()` 是「菜单 / 命令面板里的完整说法」（「提升标题级别」），
    /// 直接铺到一行 18 个按钮上会横向溢出——实测完整文案合计约 1270pt，
    /// 已超过 1280 窗口扣除面板边距后的可用宽度，最右侧的按钮会被裁掉，
    /// 而「被裁掉」等于「这个功能不存在」。
    ///
    /// 因此工具栏只用 2–3 字，完整说法与快捷键通过**悬停提示**补足
    /// （见 `toolbar_view::format_button`），可发现性由悬停保证，
    /// 不靠按钮文案硬扛。
    ///
    /// 未列出的动作直接复用 `label()`：它们的完整说法本来就够短
    /// （「加粗」「引用」「缩进」），多维护一份表只会多一处要同步的地方。
    pub const fn toolbar_label(self) -> &'static str {
        match self {
            UiAction::InlineCode => "行内",
            UiAction::InsertLink => "链接",
            UiAction::CodeBlock => "代码块",
            UiAction::InsertTable => "表格",
            UiAction::InsertImage => "图片",
            UiAction::ToggleList => "无序",
            UiAction::ToggleOrderedList => "有序",
            UiAction::PromoteHeading => "升标题",
            UiAction::DemoteHeading => "降标题",
            UiAction::ToggleTask => "任务项",
            UiAction::MoveLineUp => "上移",
            UiAction::MoveLineDown => "下移",
            // 其余（加粗 / 斜体 / 删除线 / 引用 / 缩进 / 反缩进）本来就只有 2–3 字。
            other => other.label(),
        }
    }

    /// 所属菜单分组（对齐原型 `commands.ts` 的 `group`）。
    pub const fn group(self) -> MenuGroup {
        match self {
            UiAction::NewDocument
            | UiAction::OpenDocument
            | UiAction::OpenFolder
            | UiAction::Save
            | UiAction::SaveAs
            | UiAction::SaveAll
            | UiAction::CloseTab
            | UiAction::Export => MenuGroup::File,
            UiAction::Undo
            | UiAction::Redo
            | UiAction::Find
            | UiAction::Replace
            | UiAction::FindInFiles
            | UiAction::Revert => MenuGroup::Edit,
            UiAction::Bold
            | UiAction::Italic
            | UiAction::Strikethrough
            | UiAction::InlineCode
            | UiAction::InsertLink
            | UiAction::CodeBlock
            | UiAction::InsertTable
            | UiAction::InsertImage
            | UiAction::ToggleQuote
            | UiAction::ToggleList
            | UiAction::ToggleOrderedList
            | UiAction::PromoteHeading
            | UiAction::DemoteHeading
            | UiAction::Indent
            | UiAction::Outdent
            | UiAction::ToggleTask
            | UiAction::MoveLineUp
            | UiAction::MoveLineDown => MenuGroup::Format,
            UiAction::ToggleSidebar
            | UiAction::TogglePreview
            | UiAction::ToggleOutline
            | UiAction::ToggleFocusMode
            | UiAction::ToggleStatusbar
            | UiAction::CloseOverlay
            | UiAction::DocumentStats
            | UiAction::CommandPalette
            | UiAction::InsertSymbol
            | UiAction::CompareDiff
            | UiAction::NextTab
            | UiAction::PrevTab
            | UiAction::GotoTab1
            | UiAction::GotoTab2
            | UiAction::GotoTab3
            | UiAction::GotoTab4
            | UiAction::GotoTab5
            | UiAction::GotoTab6
            | UiAction::GotoTab7
            | UiAction::GotoTab8
            | UiAction::GotoTab9 => MenuGroup::View,
            UiAction::ToggleTheme => MenuGroup::View,
            UiAction::OpenSettings | UiAction::ShortcutOverview | UiAction::About => {
                MenuGroup::Help
            }
        }
    }

    /// 是否为「格式类」动作（需要编辑区焦点；命令面板触发后应先聚焦编辑区）。
    pub const fn needs_editor_focus(self) -> bool {
        matches!(self.group(), MenuGroup::Format)
    }

    /// 命令面板模糊检索用的额外关键词（不参与显示）。
    pub const fn keywords(self) -> &'static str {
        match self {
            UiAction::NewDocument => "new create file",
            UiAction::OpenDocument => "open quick",
            UiAction::OpenFolder => "open folder workspace directory 文件夹 目录",
            UiAction::Save => "save",
            UiAction::SaveAs => "save as",
            UiAction::SaveAll => "save all",
            UiAction::CloseTab => "close tab",
            UiAction::Export => "export pdf html image",
            UiAction::Undo => "undo",
            UiAction::Redo => "redo",
            UiAction::Find => "find search",
            UiAction::Replace => "replace",
            UiAction::FindInFiles => "find in files global",
            UiAction::Revert => "revert discard",
            UiAction::Bold => "bold",
            UiAction::Italic => "italic",
            UiAction::Strikethrough => "strike del",
            UiAction::InlineCode => "code inline",
            UiAction::InsertLink => "link url",
            UiAction::CodeBlock => "code block fence",
            UiAction::InsertTable => "table",
            UiAction::InsertImage => "image picture",
            UiAction::ToggleQuote => "quote",
            UiAction::ToggleList => "list bullet",
            UiAction::ToggleOrderedList => "ordered list number",
            UiAction::PromoteHeading => "heading h up",
            UiAction::DemoteHeading => "heading h down",
            UiAction::Indent => "indent tab",
            UiAction::Outdent => "outdent unindent",
            UiAction::ToggleTask => "task checkbox",
            UiAction::MoveLineUp => "move line up",
            UiAction::MoveLineDown => "move line down",
            UiAction::ToggleSidebar => "sidebar",
            UiAction::ToggleStatusbar => "statusbar 状态栏",
            UiAction::TogglePreview => "preview",
            UiAction::ToggleOutline => "outline",
            UiAction::ToggleFocusMode => "focus zen",
            UiAction::CloseOverlay => "escape close overlay",
            UiAction::DocumentStats => "stats count words",
            UiAction::CommandPalette => "command palette",
            UiAction::InsertSymbol => "symbol emoji",
            UiAction::CompareDiff => "diff compare",
            UiAction::NextTab => "next tab",
            UiAction::PrevTab => "prev tab",
            UiAction::GotoTab1 => "tab 1 goto",
            UiAction::GotoTab2 => "tab 2 goto",
            UiAction::GotoTab3 => "tab 3 goto",
            UiAction::GotoTab4 => "tab 4 goto",
            UiAction::GotoTab5 => "tab 5 goto",
            UiAction::GotoTab6 => "tab 6 goto",
            UiAction::GotoTab7 => "tab 7 goto",
            UiAction::GotoTab8 => "tab 8 goto",
            UiAction::GotoTab9 => "tab 9 goto",
            UiAction::OpenSettings => "settings pref",
            UiAction::ShortcutOverview => "shortcuts keys",
            UiAction::About => "about",
            UiAction::ToggleTheme => "theme dark light 主题 深色 浅色",
        }
    }

    /// 默认键位（仅当应用层未提供覆盖时使用）。
    ///
    /// 与领域 `ShortcutTable::default_bindings()` 保持一致；应用层一旦可用，
    /// 就以应用层为准（用户可能已改过键），本表只作兜底。
    pub const fn default_combo(self) -> Option<&'static str> {
        Some(match self {
            UiAction::NewDocument => "Ctrl+N",
            UiAction::OpenDocument => "Ctrl+O",
            UiAction::Save => "Ctrl+S",
            UiAction::SaveAll => "Ctrl+Alt+S",
            UiAction::SaveAs => "Ctrl+Alt+Shift+S",
            UiAction::CloseTab => "Ctrl+Alt+W",
            UiAction::Export => "Ctrl+E",
            UiAction::Undo => "Ctrl+Z",
            UiAction::Redo => "Ctrl+Shift+Z",
            UiAction::Find => "Ctrl+F",
            UiAction::Replace => "Ctrl+H",
            UiAction::FindInFiles => "Ctrl+Shift+F",
            UiAction::Bold => "Ctrl+B",
            UiAction::Italic => "Ctrl+I",
            UiAction::Strikethrough => "Ctrl+Shift+X",
            UiAction::InlineCode => "Ctrl+Shift+C",
            UiAction::InsertLink => "Ctrl+K",
            UiAction::CodeBlock => "Ctrl+Shift+K",
            UiAction::InsertTable => "Ctrl+Shift+T",
            UiAction::ToggleQuote => "Ctrl+Shift+Q",
            // 列表键位对齐主流编辑器（VS Code / Obsidian：Ctrl+Shift+8 / 7）。
            // 此前**漏绑**了这两个动作：菜单里有「无序列表 / 有序列表」，
            // 但按快捷键毫无反应——这正是原型缺陷 B-02 的形态
            // （命令存在但键位没注册），故 S6-07 一并补齐。
            UiAction::ToggleList => "Ctrl+Shift+8",
            UiAction::ToggleOrderedList => "Ctrl+Shift+7",
            UiAction::PromoteHeading => "Ctrl+=",
            UiAction::DemoteHeading => "Ctrl+-",
            UiAction::Indent => "Ctrl+]",
            UiAction::Outdent => "Ctrl+[",
            UiAction::ToggleTask => "Ctrl+Shift+Enter",
            UiAction::TogglePreview => "Ctrl+P",
            UiAction::ToggleOutline => "Ctrl+Shift+O",
            UiAction::ToggleSidebar => "Ctrl+\\",
            UiAction::ToggleStatusbar => "",
            UiAction::DocumentStats => "Ctrl+Shift+S",
            UiAction::CommandPalette => "Ctrl+Shift+P",
            UiAction::InsertSymbol => "Ctrl+Shift+E",
            UiAction::CompareDiff => "Ctrl+Alt+D",
            UiAction::NextTab => "Ctrl+Shift+]",
            UiAction::PrevTab => "Ctrl+Shift+[",
            UiAction::OpenSettings => "Ctrl+,",
            UiAction::ToggleFocusMode => "F8",
            UiAction::CloseOverlay => "Esc",
            UiAction::MoveLineUp => "Alt+Up",
            UiAction::MoveLineDown => "Alt+Down",
            UiAction::GotoTab1 => "Alt+1",
            UiAction::GotoTab2 => "Alt+2",
            UiAction::GotoTab3 => "Alt+3",
            UiAction::GotoTab4 => "Alt+4",
            UiAction::GotoTab5 => "Alt+5",
            UiAction::GotoTab6 => "Alt+6",
            UiAction::GotoTab7 => "Alt+7",
            UiAction::GotoTab8 => "Alt+8",
            UiAction::GotoTab9 => "Alt+9",
            UiAction::Revert => "",
            // 「打开文件夹」跟随「打开文档」的惯例：菜单 / 命令面板可达，
            // 但不占用快捷键（无默认键位）。
            UiAction::OpenFolder => "",
            UiAction::InsertImage => "",
            UiAction::ShortcutOverview => "",
            UiAction::About => "",
            // 原型 `view.theme` 同样未绑定快捷键（keys: ''），
            // 只在「视图」菜单与命令面板中出现。
            UiAction::ToggleTheme => "",
        })
    }
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

impl MenuGroup {
    /// 全部分组（按显示顺序）。
    ///
    /// 用于**需要覆盖全部动作**的分组视图：命令面板、快捷键一览。
    /// 顶部菜单栏不用这一份——它只放应用级分组，见 [`MenuGroup::APP_LEVEL`]。
    pub const ALL: &'static [MenuGroup] = &[
        MenuGroup::File,
        MenuGroup::Edit,
        MenuGroup::Format,
        MenuGroup::View,
        MenuGroup::Help,
    ];

    /// **应用级**分组（顶部菜单栏第一层，按显示顺序）。
    ///
    /// # 为什么把 [`MenuGroup::Format`] 排除在外
    ///
    /// 这一层是「应用级菜单」：打开 / 保存 / 撤销 / 视图开关 / 设置，
    /// 作用域是**整个应用**。而格式编排（加粗、列表、缩进……）作用域是
    /// **当前编辑区的内容**，它属于编辑区工具栏，不属于应用级菜单。
    ///
    /// 两者混在同一行时，「文件」下拉与「加粗」按钮看起来同级，
    /// 职责边界就消失了——用户分不清哪些命令会改变全局状态、
    /// 哪些只动光标处的文本。因此这里显式按作用域切开：
    /// - 本常量：顶部**第一层**，全部是下拉菜单；
    /// - [`MenuGroup::Format`]：顶部**第二层**（编辑区工具栏），全部是按钮。
    ///
    /// [`MenuGroup::ALL`] 保持不变：命令面板与快捷键一览仍需要「格式」分组。
    pub const APP_LEVEL: &'static [MenuGroup] = &[
        MenuGroup::File,
        MenuGroup::Edit,
        MenuGroup::View,
        MenuGroup::Help,
    ];

    /// 分组显示名。
    pub const fn label(self) -> &'static str {
        match self {
            MenuGroup::File => "文件",
            MenuGroup::Edit => "编辑",
            MenuGroup::Format => "格式",
            MenuGroup::View => "视图",
            MenuGroup::Help => "设置",
        }
    }
}

// ============================================================================
// 组合键
// ============================================================================

/// 修饰键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Modifiers {
    /// `Ctrl`（Windows/Linux）或 `Command`（macOS）。
    pub ctrl: bool,
    /// `Alt`（macOS 为 `Option`）。
    pub alt: bool,
    /// `Shift`。
    pub shift: bool,
    /// `Meta`（macOS `Command` / Windows `Win`）。
    pub meta: bool,
}

impl Modifiers {
    /// 无修饰键。
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    /// 按固定顺序 `Ctrl → Alt → Shift → Meta` 拼出显示前缀。
    ///
    /// 顺序固定是**归一化**的关键：只有顺序一致，
    /// `Ctrl+Shift+S` 与 `Shift+Ctrl+S` 才判等（原型缺陷 B-02/C）。
    fn prefix(self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.meta {
            out.push_str("Meta+");
        }
        out
    }
}

/// 主键：字符键或功能键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    /// 字符键（ASCII 字母已归一化为大写，故 `S` 与 `s` 判等）。
    Char(char),
    /// 功能键 `F1`–`F12`。
    Function(u8),
    /// 命名键。
    Named(NamedKey),
}

/// 命名键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NamedKey {
    /// `Esc`
    Escape,
    /// `Enter`
    Enter,
    /// `Tab`
    Tab,
    /// `Space`
    Space,
    /// `Backspace`
    Backspace,
    /// `Delete`
    Delete,
    /// `↑`
    Up,
    /// `↓`
    Down,
    /// `←`
    Left,
    /// `→`
    Right,
    /// `Home`
    Home,
    /// `End`
    End,
    /// `PageUp`
    PageUp,
    /// `PageDown`
    PageDown,
    /// `Insert`
    Insert,
}

impl NamedKey {
    /// 由别名解析（大小写不敏感），别名集与领域 `NamedKey::parse` 一致。
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "esc" | "escape" => Self::Escape,
            "enter" | "return" | "cr" => Self::Enter,
            "tab" => Self::Tab,
            "space" | " " => Self::Space,
            "backspace" => Self::Backspace,
            "delete" | "del" => Self::Delete,
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" | "pgup" => Self::PageUp,
            "pagedown" | "pgdn" => Self::PageDown,
            "insert" | "ins" => Self::Insert,
            _ => return None,
        })
    }

    /// 规范显示名（与领域 `NamedKey::display` 逐字一致）。
    pub const fn display(self) -> &'static str {
        match self {
            Self::Escape => "Esc",
            Self::Enter => "Enter",
            Self::Tab => "Tab",
            Self::Space => "Space",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "PageUp",
            Self::PageDown => "PageDown",
            Self::Insert => "Insert",
        }
    }
}

impl Key {
    /// 解析主键字符串；失败返回 `None`。
    ///
    /// **大小写不敏感**：调用方（[`normalize`]）已把片段转成小写，
    /// 若这里只认大写 `F`，`F8` 会静默解析失败 → 功能键永远绑不上
    /// （这正是原型 B-01「`F8` 永不触发」的同款缺陷）。
    pub fn parse(raw: &str) -> Option<Self> {
        let lower = raw.to_ascii_lowercase();
        if lower.len() >= 2
            && lower.starts_with('f')
            && let Ok(n) = lower[1..].parse::<u8>()
            && (1..=12).contains(&n)
        {
            return Some(Self::Function(n));
        }
        if let Some(named) = NamedKey::parse(raw) {
            return Some(Self::Named(named));
        }
        let mut chars = raw.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        Some(Self::Char(normalize_char(first)))
    }
}

/// 字符键归一化：ASCII 字母一律大写。
fn normalize_char(c: char) -> char {
    if c.is_ascii_alphabetic() {
        c.to_ascii_uppercase()
    } else {
        c
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Char(c) => write!(f, "{c}"),
            Self::Function(n) => write!(f, "F{n}"),
            Self::Named(named) => f.write_str(named.display()),
        }
    }
}

/// 一个归一化后的快捷键组合。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct KeyCombo {
    modifiers: Modifiers,
    key: Key,
}

impl KeyCombo {
    /// 构造组合（**不做**归一化，请用 [`normalize`]）。
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// 修饰键。
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// 主键。
    pub const fn key(&self) -> Key {
        self.key
    }

    /// **唯一**的显示形式。
    ///
    /// 菜单右侧文字、命令面板右侧文字、设置里的绑定框、
    /// 「快捷键一览」表格——四处全部调本函数，不得各写各的格式化。
    pub fn display(&self) -> String {
        format!("{}{}", self.modifiers.prefix(), self.key)
    }
}

impl std::fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display())
    }
}

/// 解析失败的原因。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ParseComboError {
    /// 空串或只有修饰键（缺主键）。
    MissingKey,
    /// 出现多个主键。
    MultipleKeys,
    /// 主键名无法识别。
    UnknownKey(String),
}

impl std::fmt::Display for ParseComboError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey => f.write_str("缺少主键"),
            Self::MultipleKeys => f.write_str("出现多个主键"),
            Self::UnknownKey(raw) => write!(f, "无法识别的主键：{raw}"),
        }
    }
}

/// 把任意写法的组合串归一化。
///
/// 接受 `Ctrl+Shift+S`、`ctrl + shift + s`、`Shift+Ctrl+s`，全部归一为同一值。
/// 修饰键顺序按 `Ctrl → Alt → Shift → Meta` 固定重排。
pub fn normalize(raw: &str) -> Result<KeyCombo, ParseComboError> {
    let mut modifiers = Modifiers::NONE;
    let mut key: Option<Key> = None;
    for token in raw.split('+') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers.ctrl = true,
            "alt" | "option" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            // `cmd`/`command` 必须归**Meta** 而非 Ctrl：macOS 上 Cmd 就是 ⌘，
            // 归到 Ctrl 会让 `Cmd+K` 与 `Ctrl+K` 判等——两者在 mac 上是不同的键。
            // （差分测试 `normalization_matches_domain_key_combo` 锁定了这条。）
            "meta" | "cmd" | "command" | "super" | "win" => modifiers.meta = true,
            other => {
                let parsed = Key::parse(other)
                    .ok_or_else(|| ParseComboError::UnknownKey(other.to_string()))?;
                if key.is_some() {
                    return Err(ParseComboError::MultipleKeys);
                }
                key = Some(parsed);
            }
        }
    }
    key.map(|k| KeyCombo::new(modifiers, k))
        .ok_or(ParseComboError::MissingKey)
}

// ============================================================================
// 注册表
// ============================================================================

/// 快捷键注册表：**唯一**动作表 + 当前生效绑定。
///
/// # 不变式
///
/// 1. 同一 [`KeyCombo`] 至多绑定一个动作（`HashMap` 键唯一，结构上保证）；
/// 2. [`Self::display_for`] 是唯一的键位显示来源；
/// 3. [`Self::resolve`] **不因修饰键缺失而拒绝**——`F8` / `Esc` 这类
///    无修饰键单键必须能触发（原型缺陷 B-01 的根治点）。
#[derive(Clone, Debug, Default)]
pub struct ShortcutRegistry {
    /// 归一化后的绑定：`KeyCombo` → 动作。
    bindings: HashMap<KeyCombo, UiAction>,
}

impl ShortcutRegistry {
    /// 以默认键位构造（应用层尚不可用时的兜底）。
    pub fn with_defaults() -> Self {
        Self::build(|_, _| None)
    }

    /// 以应用层下发的绑定构造（**权威来源**，用户可能已改键）。
    ///
    /// 未出现在 `views` 中的动作退回 [`UiAction::default_combo`]，
    /// 这样新增动作在旧配置下也立即可用，而不被静默禁用。
    pub fn from_settings(views: &[ShortcutView]) -> Self {
        Self::build(|action, default| {
            views
                .iter()
                .find(|view| view.key == action.persisted_key().unwrap_or("~none~"))
                .and_then(|view| normalize(&view.combo).ok())
                .filter(|user| *user != default)
                .map(|user| (default, user))
        })
    }

    /// 两处构造共用的骨架：先铺默认，再按覆盖项替换。
    ///
    /// `pick_override(action, default)` 返回 `Some((default, user))` 表示该动作
    /// 被用户改键，应以 `user` 为准。
    ///
    /// **必须两轮**：若在同一个循环里「边铺默认边应用覆盖」，那么当用户把
    /// `Save` 改到 `Ctrl+Alt+S`（`SaveAll` 的默认键）时，后遍历到的
    /// `SaveAll` 会把 `Ctrl+Alt+S` 又写回去，把用户的覆盖悄悄吃掉。
    /// 先铺满默认、再统一应用覆盖，覆盖永远是最后写入的一方。
    fn build<F>(mut pick_override: F) -> Self
    where
        F: FnMut(UiAction, KeyCombo) -> Option<(KeyCombo, KeyCombo)>,
    {
        let mut defaults: HashMap<KeyCombo, UiAction> = HashMap::new();
        for action in UiAction::ALL {
            let Some(raw) = action.default_combo() else {
                continue;
            };
            if raw.is_empty() {
                continue;
            }
            let Ok(default) = normalize(raw) else {
                continue;
            };
            defaults.insert(default, *action);
        }

        // 第二轮：应用用户覆盖。被覆盖动作的旧键位要让位，
        // 否则旧绑定会残留成「幽灵快捷键」。
        let mut bindings = defaults.clone();
        for action in UiAction::ALL {
            let Some(raw) = action.default_combo() else {
                continue;
            };
            if raw.is_empty() {
                continue;
            }
            let Ok(default) = normalize(raw) else {
                continue;
            };
            if let Some(user) = pick_override(*action, default).map(|(_, user)| user) {
                bindings.remove(&default);
                bindings.insert(user, *action);
            }
        }
        Self { bindings }
    }

    /// 查按键触发的动作。
    ///
    /// 精确匹配 `KeyCombo`，**不做**前缀匹配，也不按修饰键子集匹配——
    /// 否则只按 `S` 时会误触发 `Ctrl+S`。
    pub fn resolve(&self, combo: KeyCombo) -> Option<UiAction> {
        self.bindings.get(&combo).copied()
    }

    /// 查动作当前绑定的组合。
    pub fn combo_for(&self, action: UiAction) -> Option<KeyCombo> {
        self.bindings
            .iter()
            .find(|(_, a)| **a == action)
            .map(|(combo, _)| *combo)
    }

    /// 动作的显示用键位文本（无绑定时返回空串）。
    ///
    /// 菜单 / 命令面板 / 一览**三处都调本函数**——这是「三处同步」的实现点。
    pub fn display_for(&self, action: UiAction) -> String {
        self.combo_for(action)
            .map(|combo| combo.display())
            .unwrap_or_default()
    }

    /// 重绑定（录制新键后调用）。
    ///
    /// 返回被**挤掉**的动作：若 `combo` 已被别的动作占用，
    /// 原动作将变为「无绑定」，调用方必须把它一并展示给用户
    /// （原型缺陷 B-04：先注册者静默吞掉后者）。
    pub fn rebind(&mut self, action: UiAction, combo: KeyCombo) -> Option<UiAction> {
        // 同动作换键：先删旧的，再看新键是否撞了别人。
        if let Some(old) = self.combo_for(action)
            && old != combo
        {
            self.bindings.remove(&old);
        }
        let displaced = self.bindings.insert(combo, action);
        // 若撞的正是自己（改回原键），不算挤掉任何动作。
        (displaced != Some(action)).then_some(displaced).flatten()
    }

    /// 解除某动作的绑定（返回被解除的组合）。
    pub fn unbind(&mut self, action: UiAction) -> Option<KeyCombo> {
        let previous = self.combo_for(action)?;
        self.bindings.remove(&previous);
        Some(previous)
    }

    /// 当前所有绑定，按动作枚举序——保证「一览」表格顺序稳定。
    pub fn entries(&self) -> Vec<(UiAction, KeyCombo)> {
        let mut out: Vec<(UiAction, KeyCombo)> = self
            .bindings
            .iter()
            .map(|(combo, action)| (*action, *combo))
            .collect();
        out.sort_by_key(|(action, _)| *action);
        out
    }

    /// 动作表（菜单与命令面板渲染的唯一依据）。
    pub fn table(&self) -> ActionTable<'_> {
        ActionTable { registry: self }
    }
}

// ============================================================================
// 动作表（菜单 / 命令面板渲染用）
// ============================================================================

/// 动作表视图：菜单与命令面板渲染的**唯一**依据。
///
/// 刻意做成借用注册表的轻量包装（而非快照）：改键后无需重建表，
/// 下一次渲染自然看到新键位——这正是「三处同步」想要的语义。
pub struct ActionTable<'a> {
    registry: &'a ShortcutRegistry,
}

/// 命令面板 / 菜单的一行。
#[derive(Clone, Debug, PartialEq)]
pub struct ActionEntry {
    /// 动作。
    pub action: UiAction,
    /// 中文显示名。
    pub label: &'static str,
    /// 分组。
    pub group: MenuGroup,
    /// 显示用键位（可能为空串）。
    pub combo: String,
}

impl<'a> ActionTable<'a> {
    /// 某分组下的全部动作（按枚举序）。
    pub fn group(&self, group: MenuGroup) -> Vec<ActionEntry> {
        UiAction::ALL
            .iter()
            .filter(|action| action.group() == group)
            .map(|action| self.entry(*action))
            .collect()
    }

    /// 全部动作（命令面板用）。
    pub fn all(&self) -> Vec<ActionEntry> {
        UiAction::ALL.iter().map(|a| self.entry(*a)).collect()
    }

    /// 单个动作的条目。
    pub fn entry(&self, action: UiAction) -> ActionEntry {
        ActionEntry {
            action,
            label: action.label(),
            group: action.group(),
            combo: self.registry.display_for(action),
        }
    }

    /// 命令面板的模糊检索。
    ///
    /// 子序列匹配（对齐原型 `fuzzyScore`）：`bt` 能命中「加粗bold」。
    /// 检索范围含中文显示名与英文关键词，故中英文都能搜到。
    pub fn search(&self, query: &str) -> Vec<ScoredAction> {
        let mut out = Vec::new();
        for action in UiAction::ALL {
            let entry = self.entry(*action);
            // 标签命中权重高于关键词命中：搜「保存」应先出「保存」而非「保存全部」。
            //
            // 关键词命中时 `hits` 必须是**空**而非关键词内的坐标：
            // [`ScoredAction::hits`] 的语义是「在 `label` 中的字符下标」，
            // 而关键词（`bold` / `insert_link`）与中文标签（「加粗」）
            // 长度与内容都对不上。把关键词坐标塞进来，UI 按它切标签就会
            // 越界——「加粗」只有 2 个字却被告知命中下标 0..4。
            let best = fuzzy_match(action.label(), query)
                .map(|hits| (100, hits))
                .or_else(|| fuzzy_match(action.keywords(), query).map(|_| (50, Vec::new())));
            if let Some((score, hits)) = best {
                out.push(ScoredAction { entry, score, hits });
            }
        }
        // 分数降序；同分保持动作枚举序（`sort_by` 稳定），结果因此可预测。
        out.sort_by_key(|a| std::cmp::Reverse(a.score));
        out
    }
}

/// 一条检索命中。
#[derive(Clone, Debug, PartialEq)]
pub struct ScoredAction {
    /// 动作条目。
    pub entry: ActionEntry,
    /// 匹配得分（越高越靠前）。
    pub score: i32,
    /// 命中字符在 `label` 中的**字符**下标（供 UI 高亮）。
    ///
    /// **空 = 未命中 `label`**：只命中英文关键词时也是空的。
    /// 关键词里的坐标对 `label` 无效（`bold` 的 `[0..4]` 落在
    /// 「加粗」之外），UI 必须按空处理——不画高亮，而不是越界切片。
    pub hits: Vec<usize>,
}

/// 子序列模糊匹配；命中时返回**字符**索引序列。
///
/// 索引以 `char` 为单位而非字节——中文标签按字节切会把字符劈开，
/// 后续高亮就会切在字符中间。
fn fuzzy_match(text: &str, query: &str) -> Option<Vec<usize>> {
    if query.trim().is_empty() {
        return Some(Vec::new());
    }
    let haystack: Vec<char> = text.to_lowercase().chars().collect();
    let needle: Vec<char> = query.to_lowercase().chars().collect();
    let mut cursor = 0usize;
    let mut hits = Vec::with_capacity(needle.len());
    for ch in needle {
        let found = haystack.iter().skip(cursor).position(|c| *c == ch)?;
        hits.push(cursor + found);
        cursor = hits.last().copied().unwrap_or(0) + 1;
    }
    Some(hits)
}
