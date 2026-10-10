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

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::domain::export::aggregate::ExportJob;
use crate::domain::export::errors::ExportError;
use crate::domain::export::services::{Exporter, TocBuilder};
use crate::domain::export::value_objects::{ExportFormat, TocEntry};
use crate::infrastructure::persistence::recent_files_store::atomic_write;

use super::pulldown_html_exporter::{collect_toc, render_fragment, render_toc};

/// 打印后端的探测结果。
///
/// 刻意做成**数据**而非布尔：不同平台的失败提示不同（Windows 缺浏览器、
/// macOS 缺打印服务、Linux 缺 `lp`），UI 需要据此给出对应文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfBackend {
    /// 探测到的后端标识（如 `lp` / `lpstat` / `open`）。
    pub command: &'static str,
    /// 是否可用。
    pub available: bool,
    /// 不可用时的原因（可用时为空串）。
    pub reason: &'static str,
}

impl PdfBackend {
    /// 构造「可用」结果。
    const fn available(command: &'static str) -> Self {
        Self {
            command,
            available: true,
            reason: "",
        }
    }

    /// 构造「不可用」结果。
    const fn unavailable(command: &'static str, reason: &'static str) -> Self {
        Self {
            command,
            available: false,
            reason,
        }
    }
}

/// 可注入的探测函数，抽象掉「到 PATH 里找可执行文件」这件事。
///
///抽象成 trait 而不是直接调 `which`，是为了让三平台的可用性判断
/// 能在单测里被完整覆盖（验收标准要求「fake 环境」）。
pub trait CommandProbe: Send + Sync {
    /// 探测 `command` 是否存在于 PATH 中。
    fn exists(&self, command: &str) -> bool;
}

/// 基于真实 PATH 的探测器。
#[derive(Debug, Clone, Copy, Default)]
pub struct PathProbe;

impl CommandProbe for PathProbe {
    fn exists(&self, command: &str) -> bool {
        // Windows 的可执行文件没有后缀，直接查 `lp` 会永远失败。
        if cfg!(windows) {
            ["", ".exe", ".bat", ".cmd"]
                .iter()
                .any(|ext| which(Path::new(&format!("{command}{ext}"))))
        } else {
            which(Path::new(command))
        }
    }
}

/// 最小化的 `which` 实现：逐个询问 `PATH` 中的每个目录。
///
/// 不用 `std::process::Command::new(command)` 试跑一次，因为那会**真的执行**目标
/// 程序（`lp --version` 之类的副作用不可控，且在某些系统上会阻塞）。
fn which(target: &Path) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(target).is_file())
}

/// 打印调度端口（V11-04）。
///
/// # 为什么要有这个端口
///
/// PDF 导出会**真的**调起系统打印（`Start-Process -Verb PrintTo` / `lp`）。
/// PDF 一旦在界面开放，这条副作用就会在测试里被触发——CI 上会弹出打印
/// 对话框甚至挂起。副作用无法在测试中安全执行，因此把「调用系统命令」
/// 抽成端口：生产用 [`SystemPrintDispatcher`]，测试注入记录型实现，
/// 从而在不打扰桌面的前提下验证整条导出链路。
pub trait PrintDispatcher: Send + Sync {
    /// 调起系统打印；`html_path` 是已落盘的打印用 HTML。
    fn print(&self, html_path: &Path) -> Result<(), String>;
}

/// 真实调度：按平台调用系统打印命令。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemPrintDispatcher;

impl PrintDispatcher for SystemPrintDispatcher {
    fn print(&self, html: &Path) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            let output = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "Start-Process -FilePath $args[0] -Verb PrintTo",
                ])
                .arg(html)
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).to_string());
            }
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        {
            // `lpstat` 只探测、不打印，探测到它说明 CUPS 客户端不完整。
            if backend_hint_is_stat_only() {
                return Err("系统仅安装了 lpstat（无 lp），请用浏览器打印该 HTML".to_string());
            }
            let output = Command::new("lp")
                .arg(html)
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).to_string());
            }
            Ok(())
        }
    }
}

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

    /// 构造导出器并指定打印调度（V11-04：测试注入记录型实现，避免真的弹打印）。
    pub fn with_dispatcher(dispatcher: Arc<dyn PrintDispatcher>) -> Self {
        Self {
            dispatcher,
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
    /// 三个平台各判一个代表性命令：
    /// - Windows：优先 `powershell`（`Start-Process -Verb PrintTo`），
    ///   它是系统自带的，不依赖用户装了 Chrome；
    /// - macOS：`lp`（CUPS 自带）；
    /// - Linux：`lp` 或 `lpstat`（CUPS 客户端）。
    pub fn detect(probe: &dyn CommandProbe) -> PdfBackend {
        #[cfg(target_os = "windows")]
        {
            if probe.exists("powershell") || probe.exists("pwsh") {
                PdfBackend::available("powershell")
            } else {
                PdfBackend::unavailable(
                    "powershell",
                    "未找到 PowerShell，无法调用系统打印；请先导出 HTML 后用浏览器打印",
                )
            }
        }
        #[cfg(target_os = "macos")]
        {
            if probe.exists("lp") {
                PdfBackend::available("lp")
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
                PdfBackend::available("lp")
            } else if probe.exists("lpstat") {
                PdfBackend::available("lpstat")
            } else {
                PdfBackend::unavailable(
                    "lp",
                    "未找到 lp/lpstat（CUPS 客户端未安装）；请先导出 HTML 后用浏览器打印",
                )
            }
        }
    }

    /// 当前环境是否可用（便捷方法，使用真实 PATH 探测）。
    pub fn is_available() -> bool {
        Self::detect(&PathProbe).available
    }

    /// 生成打印就绪的 HTML（含内联样式，供打印使用）。
    ///
    /// 与 [`super::pulldown_html_exporter`] 的区别：这里**不做白名单清洗**，
    /// 因为产物是给打印引擎消费的本地临时文件，不会在浏览器里打开；
    /// 但 Markdown 中的原始 HTML 仍会被 pulldown 原样透传，故这里
    /// 复用 `render_fragment` 的既有行为，不额外扩大攻击面。
    fn build_print_html(content: &str, job: &ExportJob) -> String {
        let toc: Vec<TocEntry> = collect_toc(content);
        let _ = TocBuilder::verify(&toc);
        let title = job.destination().file_name().to_string();
        let body = render_fragment(content);
        let with_toc = if job.options().include_toc() && job.needs_toc() {
            format!("{body}\n{}", render_toc(&toc))
        } else {
            body
        };
        super::pulldown_html_exporter::build_printable_document(&title, &with_toc)
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
        self.dispatcher
            .print(disk_path)
            .map_err(|e| ExportError::io_unavailable(format!("调用系统打印失败：{e}")))?;

        // PDF 由系统打印对话框产出，本进程**拿不到最终文件大小**
        // （用户在对话框里可以改纸张与缩放，实际落盘大小随之变化）。
        // 返回打印用 HTML 的大小是**已知不对**的数字，宁可返回 0 并在
        // DTO 文档里说清「0 表示由系统管线产出、大小未知」，
        // 也不要报一个看起来合理却错误的值。
        let _ = html.len();
        Ok(0)
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
