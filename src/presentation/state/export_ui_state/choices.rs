//! 导出对话框的取值模型：格式选择与页面尺寸（含显示名 / 提交标识 / 能力判据）。

/// 导出格式（UI 视角，取值与领域 `ExportFormat` 的 `parse_format` 一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExportFormatChoice {
    /// 单文件 HTML。
    #[default]
    Html,
    /// Markdown 源码。
    Markdown,
    /// PDF。
    Pdf,
    /// 长图（PNG）。
    Image,
}

impl ExportFormatChoice {
    /// 全部格式（单选组的顺序即显示顺序）。
    pub const ALL: &'static [ExportFormatChoice] = &[
        ExportFormatChoice::Html,
        ExportFormatChoice::Markdown,
        ExportFormatChoice::Pdf,
        ExportFormatChoice::Image,
    ];

    /// 中文显示名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Html => "HTML",
            Self::Markdown => "Markdown",
            Self::Pdf => "PDF",
            Self::Image => "图片",
        }
    }

    /// 提交给应用层的格式标识（`ExportRequestView::format` 的取值）。
    pub const fn id(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "markdown",
            Self::Pdf => "pdf",
            Self::Image => "image",
        }
    }

    /// 文件扩展名（不含点）。
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "md",
            Self::Pdf => "pdf",
            Self::Image => "png",
        }
    }

    /// 是否支持目录（TOC）。
    pub const fn supports_toc(self) -> bool {
        matches!(self, Self::Html | Self::Pdf)
    }

    /// 是否支持分页。
    pub const fn supports_pagination(self) -> bool {
        matches!(self, Self::Pdf)
    }

    /// 是否需要指定页面尺寸。
    pub const fn needs_page_size(self) -> bool {
        matches!(self, Self::Pdf)
    }

    /// 该格式在本实现中是否**真的可用**。
    ///
    /// **PDF 已开放（V11-04）**：选择「开放 + 语义诚实」而非继续禁用——
    /// 基础设施（S5-10 打印管线）本就就绪，禁用只是把已能用的能力藏起来。
    /// 产出方式取决于本机有什么：装有 Edge / Chrome 时**直接落盘** `.pdf`，
    /// 否则退回系统打印对话框。这一点由 [`Self::pdf_caveat`] 在选择 PDF 时
    /// 就讲清楚，而不是让用户点了导出才发现。
    pub const fn is_available(self) -> bool {
        true
    }

    /// 不可用时的原因（`None` = 可用）。
    pub const fn unavailable_reason(self) -> Option<&'static str> {
        None
    }

    /// PDF 的**前置声明**：点了会发生什么（V11-04，V11-04a 修订）。
    ///
    /// 之所以必须写在界面上：PDF 与另外三种格式的行为**可能**不同——
    /// 其余格式一定是「选好路径 → 点导出 → 文件就在那」，
    /// PDF 在缺少浏览器内核的机器上会变成
    /// 「生成打印用 HTML → 调起系统对话框 → 用户在对话框里另存」。
    /// 不提前说明，用户会以为导出失败（目标 `.pdf` 没出现）。
    ///
    /// 这里**不**去探测本机装了什么再改文案：那要求表现层查文件系统，
    /// 既跨层又每帧一次。把两种情形各说一句更省事，也不会说错。
    pub const fn pdf_caveat(self) -> Option<&'static str> {
        match self {
            Self::Pdf => Some(
                "PDF 由系统内置排版内核直接生成到目标路径（需本机装有 Edge 或 Chrome）；缺少时改为调起系统打印，请在对话框中选择「另存为 PDF」。",
            ),
            Self::Html | Self::Markdown | Self::Image => None,
        }
    }
}

/// 页面尺寸。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PageSizeChoice {
    /// A4（210×297mm）。
    #[default]
    A4,
    /// Letter（216×279mm）。
    Letter,
}

impl PageSizeChoice {
    /// 全部取值。
    pub const ALL: &'static [PageSizeChoice] = &[PageSizeChoice::A4, PageSizeChoice::Letter];

    /// 中文显示名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::A4 => "A4 210×297mm",
            Self::Letter => "Letter 216×279mm",
        }
    }

    /// 提交给应用层的标识（`ExportRequestView::page_size` 的取值）。
    pub const fn id(self) -> &'static str {
        match self {
            Self::A4 => "a4",
            Self::Letter => "letter",
        }
    }
}
