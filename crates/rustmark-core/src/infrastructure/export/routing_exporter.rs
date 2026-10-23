//! 按导出格式分发的导出器（S6-01 组合根的装配件）。
//!
//! # 为什么要一个路由而不是让应用层选
//!
//! `Exporter` trait 的 `export(content, job)` 从 `job.format()` 取格式，
//! 应用层 [`ExportAppService`] 只注入了**一个** `Arc<dyn Exporter>`。
//! 若让应用层按格式 `if` 分派再调不同导出器，就得把 `Exporter` 拆成
//! `HtmlExporter + PdfExporter + ImageExporter` 三个依赖——而这会让应用层
//! **新增一种格式时必须改应用层代码**，直接违反 S6-11「导出四类」的开放性要求。
//!
//! 路由放在基础设施层内聚：格式 → 实现 的映射是**基础设施的内部细节**
//! （哪些格式有系统级实现、哪些要退回错误提示），不该泄漏到用例编排里。
//!
//! # 行为约定
//!
//! - `Html` / `Markdown` → [`PulldownHtmlExporter`]
//! - `Pdf` → [`SystemPdfExporter`]（ADR-003 外置；不可用时返回明确 `Err`）
//! - `Image` → 明确 `Err`：图片是**视口截图**，像素来自 GPU 回读而非 `content`，
//!   由组合根直接调 [`crate::infrastructure::export::viewport_capture`] 落盘。

use crate::domain::export::aggregate::ExportJob;
use crate::domain::export::errors::ExportError;
use crate::domain::export::services::Exporter;
use crate::infrastructure::export::pulldown_html_exporter::PulldownHtmlExporter;
use crate::infrastructure::export::system_pdf_exporter::SystemPdfExporter;
use crate::shared::export_format::ExportFormat;

/// 按格式路由到具体导出器的组合器。
///
/// HTML 导出器是无状态的零大小类型；PDF 导出器持有打印调度端口
/// （V11-04，测试要能替换掉「真的调起系统打印」这一步），故整体
/// 保留 `Clone` 而不再 `Copy`。
#[derive(Debug, Clone, Default)]
pub struct RoutingExporter {
    html: PulldownHtmlExporter,
    pdf: SystemPdfExporter,
}

impl RoutingExporter {
    /// 构造路由器（PDF 使用真实系统打印调度）。
    pub fn new() -> Self {
        Self {
            html: PulldownHtmlExporter,
            pdf: SystemPdfExporter::new(),
        }
    }
}

impl Exporter for RoutingExporter {
    fn export(&self, content: &str, job: &ExportJob) -> Result<usize, ExportError> {
        match job.format() {
            ExportFormat::Html | ExportFormat::Markdown => self.html.export(content, job),
            ExportFormat::Pdf => self.pdf.export(content, job),
            // 图片导出**不经过这里**：它的像素来自 GPU 对当前帧的回读
            // （egui `ViewportCommand::Screenshot`），而不是 `content`。
            // 这条路拿不到渲染上下文，只能由组合根在拿到回传像素后
            // 直接调`viewport_capture` 落盘。
            //
            // 这里仍必须返回明确错误而不是静默成功：若将来有代码路径
            // 误把图片交给本路由器，静默成功会让用户以为导出了文件，
            // 而磁盘上什么都没有。
            ExportFormat::Image => Err(ExportError::io_unavailable(
                "图片导出走视口截图通路，不能由内容导出器处理",
            )),
        }
    }
}
