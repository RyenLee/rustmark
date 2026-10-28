//! 导出格式与页面尺寸（导出上下文的**唯一真源**；R-07）。
//!
//! # 为什么放在 shared 而非 domain
//!
//! 这份数据被三方需要：
//!
//! - **领域层**：`ExportJob` 持有格式与纸张，并据此校验「该格式能否带目录 /
//!   能否分页 / 要不要指定纸张」；
//! - **表现层**：导出对话框的单选组、提交给应用层的格式 id、目标文件扩展名；
//! - **基础设施层**：按格式把任务路由到具体导出器（HTML / PDF / 图片）。
//!
//! 而 `docs/spec/architecture-constraints.md` §4 规定：表现层不得依赖 `domain`，
//! 基础设施层不得依赖 `application`。三方共同的依赖点**只有 `shared`**
//! （与 [`crate::shared::path`] / [`crate::shared::text_format`] 同一模式）。
//!
//! # 为什么原先有两套枚举
//!
//! 领域层有 `ExportFormat`，表现层另有 `ExportFormatChoice`——两者方法
//! 一一对应（`label` / `id` / `extension` / `supports_toc` / …），
//! **只靠字符串对齐，没有任何编译期保证**：表现层加一个变体，领域层不会报错，
//! 两边就此漂移（例如表现层的 `id()` 返回 `"image"`，领域侧的
//! `parse_format` 认得 `"image" | "png"`——今天对得上，只是因为两边手写对了）。
//!
//! 本模块是唯一真源：表现层直接用它，领域层 re-export 它以保持
//! `domain::export::value_objects::ExportFormat` 的公开路径不破。
//!
//! # 新增一种导出格式要改哪里
//!
//! 1. 本模块：加变体 → `as_str` / `aliases` 的穷举 `match` 会**编译失败**
//!    （漏加即红，不必等测试），再把它加进 [`ExportFormat::ALL`]；
//! 2. 基础设施层的路由 `match`（`infrastructure::export::routing_exporter`）——
//!    同样靠穷举 `match` 在编译期暴露。
//!
//! 其余地方（对话框单选组、提交 id、扩展名、领域校验）都从本模块派生，
//! **不需要改**。这个不变式由 `tests/app/export_format_guard_test.rs` 守卫。

use serde::{Deserialize, Serialize};

/// 导出格式。
///
/// 持久化 / 提交标识见 [`ExportFormat::as_str`]（`html` / `markdown` / `pdf` / `image`）。
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
    /// 全部格式（单选组的顺序即显示顺序）。
    ///
    /// 这是「有哪些格式」的唯一清单：对话框的单选组、解析的反查、
    /// 测试的遍历都从它派生。新增格式时**必须**登记在这里，
    /// 否则该格式无法被解析（`parse` 遍历本清单）。
    pub const ALL: &'static [Self] = &[Self::Html, Self::Markdown, Self::Pdf, Self::Image];

    /// 提交给应用层的格式标识（`ExportRequestView::format` 的取值）。
    ///
    /// 穷举 `match`：新增变体时**这里编译失败**——格式与其标识的对应关系
    /// 不允许「新增时顺手补」，必须显式表态。
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "markdown",
            Self::Pdf => "pdf",
            Self::Image => "image",
        }
    }

    /// 兼容的历史别名（解析时一并接受）。
    ///
    /// 穷举 `match`：新增变体时**这里编译失败**，理由同 [`Self::as_str`]。
    ///
    /// 这些别名不是「随便多认几个」：它们是历史上真被接受过的写法
    /// （`md` 之于 Markdown、`png` 之于图片），突然不认会让既有调用方
    /// （外部脚本、旧配置）报「未知导出格式」而不是得到预期结果。
    const fn aliases(&self) -> &'static [&'static str] {
        match self {
            Self::Html => &[],
            Self::Markdown => &["md"],
            Self::Pdf => &[],
            Self::Image => &["png"],
        }
    }

    /// 由字符串解析导出格式；无法识别时返回 `None`。
    ///
    /// 识别范围：各自的 [`Self::as_str`] 标识 + [`Self::aliases`] 历史别名，
    /// **不区分大小写**、忽略首尾空白。
    ///
    /// 实现上遍历 [`Self::ALL`] 而非手写字符串 `match`，因此
    /// 「新增格式时忘记加解析分支」这件事**不可能发生**——
    /// 登记进 [`Self::ALL`] 即可被解析。
    ///
    /// 这是 [`std::str::FromStr`] 的特化版本（`Err` 用 `Infallible`），
    /// 故 `ExportFormat::parse("html")` 与 `"html".parse::<ExportFormat>()`
    /// 等价——后者是惯用写法，前者便于在「可能失败」的场景里直接调用。
    pub fn parse(raw: &str) -> Option<Self> {
        let needle = raw.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|format| format.as_str() == needle || format.aliases().contains(&needle.as_str()))
    }

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

impl std::str::FromStr for ExportFormat {
    type Err = String;

    /// trait 版本：无法识别的取值返回 `Err`。
    ///
    /// 之所以**不**把 inherent 的 [`ExportFormat::parse`] 直接
    /// 实现成 trait 方法（`Err = Infallible`），是因为「无法识别」
    /// 是**真实可能**发生的（用户手输、外部脚本传参），
    /// 不该被 `Infallible` 掩盖成「不可能失败」。
    ///
    /// 两者并存：trait 版用于 `str::parse` 的惯用写法，
    /// inherent 版（返回 `Option`）用于「认不出就当没给」的场合。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ExportFormat::parse(s).ok_or_else(|| {
            format!(
                "未知导出格式：{}（应为 html / markdown / pdf / image）",
                s.trim()
            )
        })
    }
}

/// 页面尺寸（仅 PDF 使用）。
///
/// 持久化 / 提交标识见 [`PageSize::as_str`]（`a4` / `letter`）。
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
    /// 全部取值。
    pub const ALL: &'static [Self] = &[Self::A4, Self::Letter];

    /// 提交给应用层的页面尺寸标识（`ExportRequestView::page_size` 的取值）。
    ///
    /// 穷举 `match`：新增变体时**这里编译失败**（理由同 [`ExportFormat::as_str`]）。
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::A4 => "a4",
            Self::Letter => "letter",
        }
    }

    /// 由字符串解析页面尺寸；无法识别时返回 `None`。
    ///
    /// 识别范围：各自的 [`Self::as_str`] 标识，**不区分大小写**、忽略首尾空白。
    pub fn parse(raw: &str) -> Option<Self> {
        let needle = raw.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|size| size.as_str() == needle)
    }

    /// 中文显示名。
    pub const fn label(&self) -> &'static str {
        match self {
            Self::A4 => "A4",
            Self::Letter => "Letter",
        }
    }
}

impl std::str::FromStr for PageSize {
    type Err = String;

    /// trait 版本：无法识别的取值返回 `Err`（理由同 [`ExportFormat`] 的实现）。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PageSize::parse(s).ok_or_else(|| format!("未知页面尺寸：{}（应为 a4 / letter）", s.trim()))
    }
}
