//! Export 上下文的值对象（S3-13 配套）。

use serde::{Deserialize, Serialize};

use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

/// 导出格式。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// 独立 HTML（默认）。
    #[default]
    Html,
    /// Markdown 原文（用于「另存副本」）。
    Markdown,
    /// PDF（交由系统打印管线，见 ADR-004）。
    Pdf,
    /// 长图（PNG）。
    Image,
}

impl ExportFormat {
    /// 该格式对应的文件扩展名（不含点号）。
    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "md",
            Self::Pdf => "pdf",
            Self::Image => "png",
        }
    }

    /// 是否支持目录（TOC）。
    ///
    /// HTML 与 PDF 支持；Markdown 与长图不支持（长图是单张图片，没有页码）。
    pub const fn supports_toc(&self) -> bool {
        matches!(self, Self::Html | Self::Pdf)
    }

    /// 是否支持分页。
    pub const fn supports_pagination(&self) -> bool {
        matches!(self, Self::Pdf)
    }

    /// 是否需要指定页面尺寸。
    pub const fn needs_page_size(&self) -> bool {
        matches!(self, Self::Pdf)
    }

    /// 中文显示名。
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Html => "HTML",
            Self::Markdown => "Markdown",
            Self::Pdf => "PDF",
            Self::Image => "图片",
        }
    }
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// 页面尺寸（仅 PDF 使用）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageSize {
    /// A4（默认）。
    #[default]
    A4,
    /// Letter。
    Letter,
}

impl PageSize {
    /// 中文显示名。
    pub const fn label(&self) -> &'static str {
        match self {
            Self::A4 => "A4",
            Self::Letter => "Letter",
        }
    }
}

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

    /// 是否把图片内联为 data URI。
    pub const fn embed_images(&self) -> bool {
        self.embed_images
    }

    /// 主题名。
    pub fn theme(&self) -> &str {
        &self.theme
    }

    /// 页面尺寸。
    pub const fn page_size(&self) -> PageSize {
        self.page_size
    }

    /// 是否分页。
    pub const fn paginate(&self) -> bool {
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
    Failed { reason: String },
}

impl ExportStatus {
    /// 是否为终态。
    pub const fn is_finished(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed { .. })
    }
}

/// 导出任务的来源文档引用。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ExportSource {
    document_id: DocumentId,
}

impl ExportSource {
    /// 构造来源引用。
    pub const fn new(document_id: DocumentId) -> Self {
        Self { document_id }
    }

    /// 文档 ID。
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }
}

/// 目标路径（复用共享内核的 `FilePath`，避免领域层依赖 `std::path`）。
pub type ExportDestination = FilePath;

/// 需要 HTML 转义的字符集合（[`TocEntry::needs_escape`] 的判定依据）。
///
/// 对外暴露，供 [`crate::domain::export::services::TocBuilder::verify`]
/// 与基础设施层导出器共用**同一份**判定，避免两处漂移。
pub const NEEDS_ESCAPE_CHARS: [char; 5] = ['<', '>', '&', '"', '\''];

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
