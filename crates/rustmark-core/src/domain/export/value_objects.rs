//! Export 上下文的值对象（S3-13 配套）。
//!
//! # 导出格式与页面尺寸为什么不在这里定义
//!
//! [`ExportFormat`] 与 [`PageSize`] 的**唯一真源**在
//! [`crate::shared::export_format`]：它们同时被领域层（任务校验）、
//! 表现层（导出对话框）与基础设施层（导出器路由）需要，而
//! `docs/architecture-constraints.md` §4 规定表现层不得依赖 `domain`——
//! 真源留在本层，表现层就**用不到**它，于是只能再抄一份
//! （这就是 R-07 之前 `ExportFormatChoice` 的由来：两份同构枚举靠字符串对齐，
//! 没有编译期保证）。
//!
//! 本模块**转出**它们，是为了让领域层的公开路径
//! （`domain::export::value_objects::ExportFormat`）保持不变——
//! 换的是归属，不是可达性。

use serde::{Deserialize, Serialize};

/// 导出格式（唯一真源见 [`crate::shared::export_format`]）。
pub use crate::shared::export_format::ExportFormat;
/// 页面尺寸（唯一真源见 [`crate::shared::export_format`]）。
pub use crate::shared::export_format::PageSize;

/// 导出选项。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ExportOptions {
    include_toc: bool,
    embed_images: bool,
    theme: String,
    page_size: PageSize,
    paginate: bool,
}

impl ExportOptions {
    /// 默认选项：含 TOC、内联图片、默认主题、A4、分页。
    pub fn default_options() -> Self {
        Self {
            include_toc: true,
            embed_images: true,
            theme: "default".to_string(),
            page_size: PageSize::A4,
            paginate: true,
        }
    }

    /// 构造选项。
    pub fn new(
        include_toc: bool,
        embed_images: bool,
        theme: impl Into<String>,
        page_size: PageSize,
        paginate: bool,
    ) -> Self {
        Self {
            include_toc,
            embed_images,
            theme: theme.into(),
            page_size,
            paginate,
        }
    }

    /// 是否生成目录。
    pub const fn include_toc(&self) -> bool {
        self.include_toc
    }

    /// 主题名。
    pub(crate) fn theme(&self) -> &str {
        &self.theme
    }

    /// 页面尺寸。
    pub(crate) const fn page_size(&self) -> PageSize {
        self.page_size
    }

    /// 是否分页。
    pub(crate) const fn paginate(&self) -> bool {
        self.paginate
    }
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self::default_options()
    }
}

/// 目录条目。
///
/// `needs_escape` 是原型缺陷 2.4 的根治点：**转义要求由领域层在构造时判定并标记**，
/// 导出器（基础设施层）必须消费该标记；若渲染时才想起来补转义，就会出现「TOC 与正文
/// 双重转义 / 漏转义」的不一致。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TocEntry {
    level: u8,
    title: String,
    anchor: String,
    needs_escape: bool,
}

impl TocEntry {
    /// 构造目录条目（自动计算锚点与转义需求）。
    pub fn new(level: u8, title: impl Into<String>) -> Self {
        let title = title.into();
        let anchor = slugify(&title);
        let needs_escape = needs_html_escape(&title);
        Self {
            level,
            title,
            anchor,
            needs_escape,
        }
    }

    /// 标题级别（1–6）。
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// 标题原文（**未**转义）。
    pub fn title(&self) -> &str {
        &self.title
    }

    /// 锚点（已 slug 化，可直接写入 `href="#..."`）。
    pub fn anchor(&self) -> &str {
        &self.anchor
    }

    /// 渲染前是否需要做 HTML 转义。
    ///
    /// 导出器必须据此决定是否转义；标题含 `<` `>` `&` `"` `'` 时为 `true`。
    pub const fn needs_escape(&self) -> bool {
        self.needs_escape
    }
}

/// 导出任务状态。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum ExportStatus {
    /// 已创建，尚未开始。
    #[default]
    Pending,
    /// 正在执行。
    Running,
    /// 已完成。
    Succeeded,
    /// 失败（携带可直接呈现给用户的原因）。
    Failed {
        /// 失败原因（`Display` 后可直接呈现给用户）。
        reason: String,
    },
}

impl ExportStatus {
    /// 是否为终态。
    pub const fn is_finished(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed { .. })
    }
}

/// 需要 HTML 转义的字符集合（[`TocEntry::needs_escape`] 的判定依据）。
///
/// 对外暴露，供 [`crate::domain::export::services::TocBuilder::verify`]
/// 与基础设施层导出器共用**同一份**判定，避免两处漂移。
pub(crate) const NEEDS_ESCAPE_CHARS: [char; 5] = ['<', '>', '&', '"', '\''];

/// 标题是否含有需要 HTML 转义的字符。
fn needs_html_escape(title: &str) -> bool {
    title.chars().any(|c| NEEDS_ESCAPE_CHARS.contains(&c))
}

/// 生成锚点：小写、空白转 `-`、丢弃其余非 `alnum / - / _` 的字符。
fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_dash = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else if ch == '-' || ch == '_' {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch);
        } else if ch.is_whitespace() {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        out.push_str("section");
    }
    out
}
