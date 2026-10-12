//! 动作枚举与菜单分组：菜单 / 工具栏 / 命令面板 / 快捷键共用的唯一动作表。

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

    /// **编辑区工具栏**按钮上的前置图标（单字形）。
    ///
    /// # 只收录「实测有字形」的字符
    ///
    /// 应用在 Windows 下用系统 CJK 字体兜底（见 `infrastructure::fonts`），
    /// egui 自带的西文字体只覆盖一部分符号。图标一旦落在字体覆盖之外，
    /// 界面上就是一个**豆腐块方框**——比没有图标更糟。
    ///
    /// 故这里逐个用 `Fonts::has_glyphs` 实测过（口径见
    /// `tests/presentation/toolbar_visual_test.rs`，它会把这条约束钉住）。
    /// 实测**无字形、因此刻意不用**的候选：
    /// `▾ ◂ ❝ ▦ ▤ ⌫ ⇥ ⇤ ⤒ ⤓ ↹ ❙ ✓ ⁺`（`H⁺`「上标加号」也缺）。
    ///
    /// `None` 表示该动作不配图标——宁可没有，也不放一个语义不明的方块。
    /// 图标与文案之间由渲染层补一个空格（见 `toolbar_view::button_text`），
    /// 所以这里只返回图标字形本身。
    pub const fn icon(self) -> Option<&'static str> {
        match self {
            // 字符样式：用拉丁字母是唯一「在任何字体下都成立」的选择。
            UiAction::Bold => Some("B"),
            UiAction::Italic => Some("I"),
            UiAction::Strikethrough => Some("S"),
            UiAction::InlineCode => Some("<>"),
            UiAction::InsertLink => Some("🔗"),
            // 块级插入
            UiAction::CodeBlock => Some("≡"),
            UiAction::InsertTable => Some("⊞"),
            UiAction::InsertImage => Some("🖼"),
            // 列表 / 结构
            UiAction::ToggleQuote => Some("「"),
            UiAction::ToggleList => Some("•"),
            UiAction::ToggleOrderedList => Some("№"),
            UiAction::ToggleTask => Some("☑"),
            UiAction::PromoteHeading => Some("H↑"),
            UiAction::DemoteHeading => Some("H↓"),
            // 行操作
            UiAction::Indent => Some("→"),
            UiAction::Outdent => Some("←"),
            UiAction::MoveLineUp => Some("↑"),
            UiAction::MoveLineDown => Some("↓"),
            _ => None,
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
