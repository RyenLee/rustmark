//! 可绑定动作目录 [`ActionId`]：命令面板与快捷键共享同一份枚举。
//!
//! 动作的稳定标识（[`ActionId::key`]）是持久化契约，显示文案
//! （[`ActionId::label`]）可随产品打磨调整。

use serde::{Deserialize, Serialize};

/// 可绑定的动作（命令面板与快捷键共享同一份枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionId {
    /// 新建文档
    NewDocument,
    /// 打开文档
    OpenDocument,
    /// 保存
    Save,
    /// 另存为
    SaveAs,
    /// 关闭标签页
    CloseTab,
    /// 撤销
    Undo,
    /// 重做
    Redo,
    /// 查找
    Find,
    /// 替换
    Replace,
    /// 加粗
    Bold,
    /// 斜体
    Italic,
    /// 删除线
    Strikethrough,
    /// 行内代码
    InlineCode,
    /// 插入链接
    InsertLink,
    /// 代码块
    CodeBlock,
    /// 插入表格
    InsertTable,
    /// 切换引用
    ToggleQuote,
    /// 提升标题级别
    PromoteHeading,
    /// 降低标题级别
    DemoteHeading,
    /// 缩进
    Indent,
    /// 反缩进
    Outdent,
    /// 切换任务勾选
    ToggleTask,
    /// 切换预览
    TogglePreview,
    /// 切换大纲
    ToggleOutline,
    /// 打开命令面板
    CommandPalette,
    /// 打开设置
    OpenSettings,
    /// 导出
    Export,
    /// 专注 / 阅读模式（默认 `F8`，`Esc` 退出）
    ///
    /// 之所以必须可绑定：原型缺陷 B-01 的根因是「匹配层拒绝纯单键」，
    /// 于是 `F8` 这类声明在设置里却永远不触发。此处入枚举即代表
    /// 「功能键与 `Esc` 是合法组合」，匹配层不得再拒绝。
    FocusMode,
    /// 关闭浮层 / 对话框（默认 `Esc`）
    ///
    /// 原型缺陷 B-03：该项可录制但被硬编码分支绕过。此处无任何特判，
    /// 改绑后 `Esc` 即失效（验收用例 ③）。
    CloseOverlay,
    /// 文档统计
    DocumentStats,
    /// 折叠 / 展开侧边栏
    ToggleSidebar,
    /// 显示 / 隐藏底部状态栏
    ToggleStatusbar,
    /// 在所有文件中查找
    FindInFiles,
    /// 保存全部已修改文档
    SaveAll,
    /// 插入符号 / Emoji
    InsertSymbol,
    /// 对比文件差异
    CompareDiff,
    /// 下一个标签页
    NextTab,
    /// 上一个标签页
    PrevTab,
}

impl ActionId {
    /// 全部动作（**可持久化**动作全集）。
    ///
    /// # 它与 `default_bindings()` 不是一回事
    ///
    /// 本清单 = 「**能被改键持久化**」的动作全集；
    /// [`ShortcutTable::default_bindings`] = 「**有默认键位**」的动作集。
    ///
    /// 「可持久化但无默认键位」是**合法**状态：「显示 / 隐藏状态栏」
    /// 用户可在设置页自行录制，但产品上**不占用快捷键**，
    /// 故它在本清单里、却不在默认键位表里。
    ///
    /// # 这份清单必须与枚举变体**同步**
    ///
    /// 它是**手写清单**，编译器管不到：加变体忘了加进这里 →
    /// 该动作**不出现在命令面板**、**不可改键**，而**零报错**。
    /// 表现层的 `UiAction::persisted_key` 认它、领域这里不认 →
    /// 改键时报「尚未映射到领域动作」。
    ///
    /// 2026-10-20 实测：本清单**真的漏登记过** `ToggleStatusbar`
    /// （枚举 38 / 清单 37），症状是「显示/隐藏状态栏」改键时弹
    /// 「尚未映射到领域动作，无法改键」。由
    /// `tests/app/shortcut_action_guard_test.rs` 的
    /// `the_domain_action_list_matches_the_enum` 钉住。
    pub const fn all() -> &'static [ActionId] {
        &[
            Self::NewDocument,
            Self::OpenDocument,
            Self::Save,
            Self::SaveAs,
            Self::CloseTab,
            Self::Undo,
            Self::Redo,
            Self::Find,
            Self::Replace,
            Self::Bold,
            Self::Italic,
            Self::Strikethrough,
            Self::InlineCode,
            Self::InsertLink,
            Self::CodeBlock,
            Self::InsertTable,
            Self::ToggleQuote,
            Self::PromoteHeading,
            Self::DemoteHeading,
            Self::Indent,
            Self::Outdent,
            Self::ToggleTask,
            Self::TogglePreview,
            Self::ToggleOutline,
            Self::CommandPalette,
            Self::OpenSettings,
            Self::Export,
            Self::FocusMode,
            Self::CloseOverlay,
            Self::DocumentStats,
            Self::ToggleSidebar,
            Self::ToggleStatusbar,
            Self::FindInFiles,
            Self::SaveAll,
            Self::InsertSymbol,
            Self::CompareDiff,
            Self::NextTab,
            Self::PrevTab,
        ]
    }

    /// 稳定 `snake_case` 键（跨层传递用）。
    ///
    /// 与 [`Self::label`] 的区别：label 是**文案**（会随产品打磨调整），
    /// key 是**契约**（改了要同步改配置里已存盘的绑定）。
    /// 表现层必须按 key 匹配动作，用 label 当键会让「改个菜单措辞」静默改掉快捷键归属。
    pub const fn key(&self) -> &'static str {
        match self {
            Self::NewDocument => "new_document",
            Self::OpenDocument => "open_document",
            Self::Save => "save",
            Self::SaveAs => "save_as",
            Self::SaveAll => "save_all",
            Self::CloseTab => "close_tab",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Find => "find",
            Self::Replace => "replace",
            Self::FindInFiles => "find_in_files",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Strikethrough => "strikethrough",
            Self::InlineCode => "inline_code",
            Self::InsertLink => "insert_link",
            Self::CodeBlock => "code_block",
            Self::InsertTable => "insert_table",
            Self::ToggleQuote => "toggle_quote",
            Self::PromoteHeading => "promote_heading",
            Self::DemoteHeading => "demote_heading",
            Self::Indent => "indent",
            Self::Outdent => "outdent",
            Self::ToggleTask => "toggle_task",
            Self::TogglePreview => "toggle_preview",
            Self::ToggleOutline => "toggle_outline",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::ToggleStatusbar => "toggle_statusbar",
            Self::CommandPalette => "command_palette",
            Self::OpenSettings => "open_settings",
            Self::Export => "export",
            Self::FocusMode => "focus_mode",
            Self::CloseOverlay => "close_overlay",
            Self::DocumentStats => "document_stats",
            Self::InsertSymbol => "insert_symbol",
            Self::CompareDiff => "compare_diff",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
        }
    }

    /// 中文显示名（供菜单与命令面板共用）。
    pub(crate) const fn label(&self) -> &'static str {
        match self {
            Self::NewDocument => "新建文档",
            Self::OpenDocument => "打开文档",
            Self::Save => "保存",
            Self::SaveAs => "另存为",
            Self::CloseTab => "关闭标签页",
            Self::Undo => "撤销",
            Self::Redo => "重做",
            Self::Find => "查找",
            Self::Replace => "替换",
            Self::Bold => "加粗",
            Self::Italic => "斜体",
            Self::Strikethrough => "删除线",
            Self::InlineCode => "行内代码",
            Self::InsertLink => "插入链接",
            Self::CodeBlock => "代码块",
            Self::InsertTable => "插入表格",
            Self::ToggleQuote => "切换引用",
            Self::PromoteHeading => "提升标题级别",
            Self::DemoteHeading => "降低标题级别",
            Self::Indent => "缩进",
            Self::Outdent => "反缩进",
            Self::ToggleTask => "切换任务勾选",
            Self::TogglePreview => "切换预览",
            Self::ToggleOutline => "切换大纲",
            Self::CommandPalette => "命令面板",
            Self::OpenSettings => "设置",
            Self::Export => "导出",
            Self::FocusMode => "专注模式",
            Self::CloseOverlay => "关闭浮层",
            Self::DocumentStats => "文档统计",
            Self::ToggleSidebar => "折叠 / 展开侧边栏",
            Self::ToggleStatusbar => "显示 / 隐藏状态栏",
            Self::FindInFiles => "在所有文件中查找",
            Self::SaveAll => "保存全部",
            Self::InsertSymbol => "插入符号",
            Self::CompareDiff => "对比文件差异",
            Self::NextTab => "下一个标签页",
            Self::PrevTab => "上一个标签页",
        }
    }
}
