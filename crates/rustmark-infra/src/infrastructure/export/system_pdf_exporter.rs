//! 系统打印管线 PDF 导出器（S5-10，ADR-004）。
//!
//! # 为什么**不内嵌** PDF 引擎
//!
//! 内嵌 PDF 引擎（`printpdf` / `genpdf` / `typst` 之类）会带来两笔账：
//! - **体积**：PDF 字体与排版引擎轻松吃掉 2–4 MB，直接威胁 §8.1 的 10 MB 红线；
//! - **正确性**：Markdown → PDF 的排版规则（分页、代码块跨页、表格断行、
//!   中文断字）极难做好，做出来的效果通常还不如浏览器打印。
//!
//! 故采用外置方案：**生成打印就绪的 HTML → 交给系统打印/导出路径**。
//! 三个平台各有一条已验证的路径，且都不需要额外 crate。
//!
//! # 各平台的实际落点
//!
//! - **Windows**：优先用**无头 Chromium 内核**（Edge / Chrome）的
//!   `--print-to-pdf`——静默、直接把 PDF 写到目标路径、大小如实回报；
//!   找不到浏览器才退回系统关联程序的 `printto` 动词（会弹打印对话框）。
//!   退回路径为什么不首选见 `dispatch::shell_print_to` 的注释。
//! - **macOS / Linux**：`lp`（CUPS），仍由系统打印流程产出，大小未知。
//!
//! # 可用性探测
//!
//! [`SystemPdfExporter::detect`] 返回当前平台是否存在可用后端。
//! 探测是**纯函数**（可注入 `Probe`），因此可用性判断能被单测覆盖，
//! 而不必真的在 CI 里装一个打印服务——这也是验收标准
//! 「三平台可用性探测函数有单测（fake 环境）」的实现方式。
//!
//! # 不可用时的行为
//!
//! 返回 [`ExportError::IoUnavailable`]，且错误文案**给出可操作的下一步**
//! （先导出 HTML 再手动打印），而不是干巴巴一句「不支持 PDF」。
//!
//! # 模块结构
//!
//! facade 保留 `SystemPdfExporter`、可用性探测与 `Exporter` 实现；
//! 后端探测模型在 `backend`，系统打印命令调度在 `dispatch`。

mod backend;
mod dispatch;

use std::path::Path;
use std::sync::Arc;

use crate::infrastructure::persistence::recent_files_store::atomic_write;
use rustmark_core::domain::export::aggregate::ExportJob;
use rustmark_core::domain::export::errors::ExportError;
use rustmark_core::domain::export::services::{Exporter, TocBuilder};
use rustmark_core::domain::export::value_objects::TocEntry;
use rustmark_core::shared::export_format::{ExportFormat, PageSize};

use super::html_sanitize::sanitize_html;
use super::pulldown_html_exporter::{collect_toc, render_fragment, render_toc};
pub use backend::{CommandProbe, PathProbe, PdfBackend, PrintOutcome};
pub use dispatch::{PrintDispatcher, SystemPrintDispatcher, file_url};
#[cfg(target_os = "windows")]
pub use dispatch::{ensure_print_source_is_sanitized, headless_print_command, shell_print_command};

#[cfg(target_os = "windows")]
use dispatch::{chromium_candidates, pick_browser};

/// 基于系统打印管线的 PDF 导出器。
#[derive(Clone)]
pub struct SystemPdfExporter {
    dispatcher: Arc<dyn PrintDispatcher>,
    /// 后端探测器（同样可注入：CI 上不装打印服务也要能覆盖「可用」分支）。
    probe: Arc<dyn CommandProbe>,
}

impl Default for SystemPdfExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SystemPdfExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SystemPdfExporter")
    }
}

/// `PathProbe` 的单例，便于在 `#[cfg]` 之外的自由函数里使用。
#[cfg(not(target_os = "windows"))]
const PATH_PROBE: PathProbe = PathProbe;

impl SystemPdfExporter {
    /// 构造导出器（真实 PATH 探测 + 真实系统打印调度）。
    pub fn new() -> Self {
        Self {
            dispatcher: Arc::new(SystemPrintDispatcher),
            probe: Arc::new(PathProbe),
        }
    }

    /// 构造导出器并同时指定探测与调度（测试用：让「后端可用」分支在
    /// **不装任何打印服务的 CI** 上也能被覆盖）。
    pub fn with_probe_and_dispatcher(
        probe: Arc<dyn CommandProbe>,
        dispatcher: Arc<dyn PrintDispatcher>,
    ) -> Self {
        Self { dispatcher, probe }
    }

    /// 探测当前平台可用的打印后端。
    ///
    /// - Windows：优先 **Edge / Chrome**（无头内核，`--print-to-pdf`，
    ///   静默直接落盘），其次 `powershell`（`Start-Process -Verb PrintTo`，
    ///   经打印对话框）。判据与 `SystemPrintDispatcher` 完全一致。
    /// - macOS：`lp`（CUPS 自带）；
    /// - Linux：`lp` 或 `lpstat`（CUPS 客户端）。
    pub fn detect(probe: &dyn CommandProbe) -> PdfBackend {
        #[cfg(target_os = "windows")]
        {
            if let Some((name, _)) = pick_browser(&chromium_candidates(), probe) {
                return PdfBackend::direct(name);
            }
            if probe.exists("powershell") || probe.exists("pwsh") {
                PdfBackend::dialog("powershell")
            } else {
                PdfBackend::unavailable(
                    "powershell",
                    "未找到 Edge / Chrome / PowerShell，无法生成 PDF；请先导出 HTML 后用浏览器打印",
                )
            }
        }
        #[cfg(target_os = "macos")]
        {
            if probe.exists("lp") {
                PdfBackend::dialog("lp")
            } else {
                PdfBackend::unavailable(
                    "lp",
                    "未找到 lp（CUPS 未安装）；请先导出 HTML 后用浏览器打印",
                )
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            if probe.exists("lp") {
                PdfBackend::dialog("lp")
            } else if probe.exists("lpstat") {
                PdfBackend::dialog("lpstat")
            } else {
                PdfBackend::unavailable(
                    "lp",
                    "未找到 lp/lpstat（CUPS 客户端未安装）；请先导出 HTML 后用浏览器打印",
                )
            }
        }
    }

    /// 生成打印就绪的 HTML（含内联样式，供打印使用）。
    ///
    /// # 为什么这里**必须**清洗
    ///
    /// 产物会被无头 Chromium 以 `file:///` 打开并渲染成 PDF——那是一个真实的
    /// 浏览器引擎，文档里的 `<script>` 与事件属性会执行（H-1：此处此前把
    /// 「交给打印引擎」误当成「不在浏览器里打开」，因而漏掉了清洗）。
    /// 这与 HTML 导出（`PulldownHtmlExporter::build_document`）面对的是同一类
    /// 不可信输入，故复用同一套白名单清洗。
    ///
    /// 清洗边界严格取在**正文 + TOC 片段**上：`wrap_document` 生成的
    /// `<meta charset>` / `<style>` 骨架是可信常量，而清洗器按设计会丢弃
    /// `style` / `meta`（二者在 `DROP_TAGS` 中），一并送进清洗会把打印样式
    /// 与字符集声明剥掉，打印出来是无样式的裸文档。
    ///
    /// 公开仅为让单测能直接断言中间产物（同 `PulldownHtmlExporter::build_document`），
    /// 不是给其它模块调的。
    pub fn build_print_html(content: &str, job: &ExportJob) -> String {
        let toc: Vec<TocEntry> = collect_toc(content);
        let _ = TocBuilder::verify(&toc);
        let title = job.destination().file_name().to_string();
        let body = render_fragment(content);
        let with_toc = if job.options().include_toc() && job.needs_toc() {
            format!("{body}\n{}", render_toc(&toc))
        } else {
            body
        };
        // 不可信来源（pulldown 原样透传的内嵌 HTML + TOC）先过白名单清洗。
        let safe_body = sanitize_html(&with_toc);
        // 纸张尺寸写成 `@page`。界面一直承诺「纸张会写入导出件的 `@page` 规则」，
        // 但这条规则此前**根本不存在**（打印样式表里没有任何 `@page`），
        // 于是用户选的纸张从来没生效过。无头内核会按它排版（A4 实测 595×842pt）。
        let page_style = format!(
            "@page{{size:{};margin:18mm}}",
            match job.options().page_size() {
                PageSize::A4 => "A4",
                PageSize::Letter => "letter",
            }
        );
        super::pulldown_html_exporter::build_printable_document(&title, &safe_body, &page_style)
    }
}

impl Exporter for SystemPdfExporter {
    fn export(&self, content: &str, job: &ExportJob) -> Result<usize, ExportError> {
        if job.format() != ExportFormat::Pdf {
            return Err(ExportError::io_unavailable(format!(
                "{} 导出不由 PDF 导出器处理",
                job.format().label()
            )));
        }

        let backend = Self::detect(self.probe.as_ref());
        if !backend.available {
            return Err(ExportError::IoUnavailable {
                message: backend.reason.to_string(),
            });
        }

        // 先落一个打印用 HTML，再交给系统管线。
        // 目标 PDF 路径的扩展名换成 `.html`，避免污染用户的 PDF 文件。
        let html_path = job.destination().with_extension("print.html");
        let disk_path = Path::new(html_path.as_str());
        let html = Self::build_print_html(content, job);
        atomic_write(disk_path, html.as_bytes())
            .map_err(|e| ExportError::io_unavailable(format!("写入打印用 HTML 失败：{e}")))?;

        // 调用系统管线；具体用哪个命令由注入的 dispatcher 按平台决定。
        let outcome = self
            .dispatcher
            .print(disk_path, Path::new(job.destination().as_str()))
            .map_err(|e| ExportError::io_unavailable(format!("调用系统打印失败：{e}")))?;

        match outcome {
            // 无头内核直接把 PDF 写到了目标路径：报**真实大小**。
            PrintOutcome::Wrote(bytes) => Ok(usize::try_from(bytes).unwrap_or(usize::MAX)),
            // 经系统打印对话框产出：用户在对话框里可以改纸张与缩放，
            // 实际落盘大小随之变化，本进程拿不到。返回打印用 HTML 的大小是
            // **已知不对**的数字，宁可返回 0 并在 DTO 文档里说清
            // 「0 表示由系统管线产出、大小未知」，也不要报一个看起来合理却错误的值。
            PrintOutcome::Delegated => Ok(0),
        }
    }
}

/// 非 Windows 平台：探测结果是否为「只有 `lpstat`、没有 `lp`」。
///
/// 拆成独立函数是为了让 `#[cfg]` 只作用于函数体，
/// 而不是让 `backend` 参数在某个平台下变成未使用。
#[cfg(not(target_os = "windows"))]
fn backend_hint_is_stat_only() -> bool {
    PATH_PROBE.exists("lpstat") && !PATH_PROBE.exists("lp")
}
