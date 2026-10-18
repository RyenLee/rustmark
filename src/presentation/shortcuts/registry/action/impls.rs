//! [`UiAction`] 的固有 impl 块：常量注册表 / 元信息 / 分组 / 快捷键。

use super::*;

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
            UiAction::ToggleTheme => "切换主题（深 / 浅）",
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
