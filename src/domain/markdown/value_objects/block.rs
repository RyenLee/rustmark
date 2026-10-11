//! 渲染块标识与块类型：[`BlockId`] / [`BlockKind`]。
//!
//! [`BlockKind`] 覆盖 Markdown 全部块级语义；列表 / 表格 / 定义列表的结构化载荷
//! 定义在 `table.rs`，由本枚举持有。

use super::table::{DefinitionEntry, ListItem, TableData};

/// 渲染块的稳定标识（单次解析结果内唯一）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct BlockId(u64);

impl BlockId {
    /// 由序号构造。
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// 取出序号。
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "block#{}", self.0)
    }
}

/// 渲染块的类型。
///
/// 覆盖需求文档 §3.1 的 P0 特性；P1（数学公式、Mermaid）已落地为
/// [`BlockKind::Math`] / [`BlockKind::Diagram`]，其余未支持的扩展块仍走
/// [`BlockKind::Raw`]。
///
/// # 为什么列表 / 表格要把结构留在 `BlockKind` 里
///
/// S6-05 的预览要把表格画成网格、把任务列表画成可点击的复选框，
/// 而一个扁平字符串（`"列A列B1234"`）做不到——但结构是**解析期**才知道的，
/// 到表现层已无法还原。故结构性载荷必须由本层持有。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BlockKind {
    /// 标题（1–6 级）。
    Heading { level: u8 },
    /// 段落。
    Paragraph,
    /// 围栏或缩进代码块。
    CodeBlock { lang: Option<String> },
    /// GFM 表格（含表头 / 表体 / 列对齐）。
    Table { data: TableData },
    /// 有序 / 无序列表。
    List {
        /// 是否为有序列表。
        ordered: bool,
        /// 列表项（保序）。
        items: Vec<ListItem>,
    },
    /// GFM 任务列表。
    ///
    /// 与 [`BlockKind::List`] 分开设：任务项的 `checked` 需要参与渲染与回写，
    /// 用一个 `Vec<ListItem>` 承载可以让「勾选状态」成为结构而非文本解析的结果。
    TaskList { items: Vec<ListItem> },
    /// 引用块。
    Quote,
    /// 图片（本地 / 远程 / data URI）。
    Image { alt: String, url: String },
    /// 分割线。
    Rule,
    /// 脚注定义。
    ///
    /// `label` 是源码里 `[^label]:` 的那个 `label`——渲染脚注条目时必须显示出来
    /// （否则用户不知道正文里的 `[^1]` 对应哪一条），而它**不在** `content` 里
    /// （`content` 只有定义正文），故必须单独承载。
    Footnote {
        /// 脚注标识（`[^1]` 里的 `1`）。
        label: String,
    },
    /// 定义列表（`术语` + 换行缩进的 `: 定义`）。
    ///
    /// Pulldown 的 `Tag::DefinitionList` 结构是
    /// `DefinitionList` → `DefinitionListTitle`（术语）→ 一个或多个
    /// `DefinitionListDefinition`（定义），故这里收成「条目 + 该条目的多条定义」。
    DefinitionList {
        /// 条目（每项为一条术语及其定义）。
        entries: Vec<DefinitionEntry>,
    },
    /// 数学公式（行内或块级）。
    ///
    /// `display` 为 `true` 表示块级公式（`$$...$$`），`false` 表示行内（`$...$`）。
    /// 编辑器内以「结构化源码卡」呈现（零依赖，不内嵌 KaTeX/MathJax 的像素级渲染）；
    /// 导出 HTML 时由 pulldown 输出 `<span class="math">`，交浏览器 / MathJax 渲染。
    Math { display: bool },
    /// 图表（如 Mermaid）。`kind` 标记图表引擎，当前仅 `"mermaid"`。
    ///
    /// ` ```mermaid ` 围栏经解析器识别后落到此处，而非当作普通代码块。
    Diagram { kind: String },
    /// 目录（`@[TOC](标题)` / `[TOC]`）。
    ///
    /// 标题由源码里的括号内容给出（CSDN 的 `@[TOC](欢迎)` 写作 `欢迎`）；
    /// 目录**条目**不在这里存——它们就是文档里的标题块，表现层按块序列
    /// 现场收集（标题会随编辑增删，存一份副本必然过期）。
    Toc { title: String },
    /// 原始 HTML 等尚未支持的扩展块。
    Raw { marker: String },
}

impl BlockKind {
    /// 是否为可在大纲中显示的标题。
    pub const fn is_heading(&self) -> bool {
        matches!(self, Self::Heading { .. })
    }

    /// 是否为列表（含任务列表）。
    pub const fn is_list(&self) -> bool {
        matches!(self, Self::List { .. } | Self::TaskList { .. })
    }

    /// 标题级别（非标题为 `None`）。
    pub const fn heading_level(&self) -> Option<u8> {
        match self {
            Self::Heading { level } => Some(*level),
            _ => None,
        }
    }

    /// 是否为需要语法高亮的块（代码块）。
    pub const fn is_code(&self) -> bool {
        matches!(self, Self::CodeBlock { .. })
    }

    /// 代码块语言（非代码块为 `None`）。
    pub fn language(&self) -> Option<&str> {
        match self {
            Self::CodeBlock { lang: Some(lang) } => Some(lang.as_str()),
            _ => None,
        }
    }

    /// 列表项（非列表为 `None`）。
    ///
    /// 任务列表与普通列表共用同一份 [`ListItem`]，故两者在这里表现一致。
    pub fn list_items(&self) -> Option<&[ListItem]> {
        match self {
            Self::List { items, .. } | Self::TaskList { items } => Some(items),
            _ => None,
        }
    }

    /// 是否为任务列表。
    pub const fn is_task_list(&self) -> bool {
        matches!(self, Self::TaskList { .. })
    }

    /// 表格数据（`kind` 为表格时有值）。
    pub const fn table(&self) -> Option<&TableData> {
        match self {
            Self::Table { data } => Some(data),
            _ => None,
        }
    }

    /// 定义列表条目（`kind` 为定义列表时有值）。
    pub fn definitions(&self) -> Option<&[DefinitionEntry]> {
        match self {
            Self::DefinitionList { entries } => Some(entries),
            _ => None,
        }
    }

    /// 脚注标识（`kind` 为脚注时有值）。
    pub fn footnote_label(&self) -> Option<&str> {
        match self {
            Self::Footnote { label } => Some(label.as_str()),
            _ => None,
        }
    }
}
