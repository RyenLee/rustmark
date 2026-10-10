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
//!   退回路径为什么不首选见 [`shell_print_to`] 的注释。
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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::domain::export::aggregate::ExportJob;
use crate::domain::export::errors::ExportError;
use crate::domain::export::services::{Exporter, TocBuilder};
use crate::domain::export::value_objects::{ExportFormat, PageSize, TocEntry};
use crate::infrastructure::persistence::recent_files_store::atomic_write;

use super::pulldown_html_exporter::{collect_toc, render_fragment, render_toc};

/// 打印后端的探测结果。
///
/// 刻意做成**数据**而非布尔：不同平台的失败提示不同（Windows 缺浏览器、
/// macOS 缺打印服务、Linux 缺 `lp`），UI 需要据此给出对应文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfBackend {
    /// 探测到的后端标识（如 `msedge` / `lp` / `lpstat`）。
    pub command: &'static str,
    /// 是否可用。
    pub available: bool,
    /// 不可用时的原因（可用时为空串）。
    pub reason: &'static str,
    /// 是否**直接把 PDF 写到目标路径**（无需用户在对话框里另存）。
    ///
    /// 「点导出后是直接拿到文件，还是弹个打印对话框让你自己另存」对用户是
    /// 两回事，而这条差别只靠探测命令就能判出来；`export()` 的实际行为
    /// 也由同一判据决定（见 [`Self::detect`] 与 [`SystemPrintDispatcher`]），
    /// 所以两者不会分叉。
    pub writes_pdf: bool,
}

impl PdfBackend {
    /// 构造「可用、且直接落盘 PDF」的结果。
    const fn direct(command: &'static str) -> Self {
        Self {
            command,
            available: true,
            reason: "",
            writes_pdf: true,
        }
    }

    /// 构造「可用，但要经系统打印对话框」的结果。
    const fn dialog(command: &'static str) -> Self {
        Self {
            command,
            available: true,
            reason: "",
            writes_pdf: false,
        }
    }

    /// 构造「不可用」结果。
    const fn unavailable(command: &'static str, reason: &'static str) -> Self {
        Self {
            command,
            available: false,
            reason,
            writes_pdf: false,
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

    /// 探测一个**绝对路径**是否存在。
    ///
    /// Chromium 系浏览器装在固定位置、**不进 PATH**，只能按路径找；
    /// 这条与 `exists` 分开，是为了让「本机装了哪个浏览器」也能被测试注入
    /// （否则 CI 上有没有 Edge 会让探测结果不可预期）。
    fn path_exists(&self, path: &str) -> bool;
}

/// 基于真实文件系统的探测器。
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

    fn path_exists(&self, path: &str) -> bool {
        Path::new(path).is_file()
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

/// 打印调度的结果：这一次到底有没有**直接产出 PDF 文件**。
///
/// 两个变体的差别对用户是可见的（一个直接拿到文件，一个要去对话框里另存），
/// 所以不能让调度端悄悄吞掉这个区别——`export()` 要据此回报真实字节数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintOutcome {
    /// 已把 PDF 写到目标路径（实际字节数）。
    Wrote(u64),
    /// 已交给系统打印流程，最终文件由用户在对话框里另存（本进程拿不到大小）。
    Delegated,
}

/// 打印调度端口（V11-04）。
///
/// # 为什么要有这个端口
///
/// PDF 导出会**真的**调起系统打印（无头浏览器 / `Start-Process -Verb PrintTo` / `lp`）。
/// PDF 一旦在界面开放，这条副作用就会在测试里被触发——CI 上会弹出打印
/// 对话框甚至挂起。副作用无法在测试中安全执行，因此把「调用系统命令」
/// 抽成端口：生产用 [`SystemPrintDispatcher`]，测试注入记录型实现，
/// 从而在不打扰桌面的前提下验证整条导出链路。
pub trait PrintDispatcher: Send + Sync {
    /// 调起系统打印/导出。
    ///
    /// - `html_path`：已落盘的打印用 HTML（渲染源）；
    /// - `pdf_path`：用户选定的 PDF 目标路径（无头内核会直接写到这里）。
    fn print(&self, html_path: &Path, pdf_path: &Path) -> Result<PrintOutcome, String>;
}

/// 真实调度：按平台调用系统打印命令。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemPrintDispatcher;

impl PrintDispatcher for SystemPrintDispatcher {
    fn print(&self, html: &Path, pdf: &Path) -> Result<PrintOutcome, String> {
        #[cfg(target_os = "windows")]
        {
            // 首选无头 Chromium：静默、直接落盘、能报真实大小。
            if let Some((_, browser)) = pick_browser(&chromium_candidates(), &PathProbe) {
                return headless_print(&browser, html, pdf).map(PrintOutcome::Wrote);
            }
            // 兜底：系统关联程序的 `printto` 动词，会弹打印对话框。
            shell_print_to(html)?;
            Ok(PrintOutcome::Delegated)
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
            Ok(PrintOutcome::Delegated)
        }
    }
}

/// Windows 上可用的 Chromium 系浏览器候选：`(标识, 可执行文件路径)`。
///
/// 顺序即优先级：**Edge 优先**（Windows 10/11 随系统预装，覆盖率最高），
/// 其次 Chrome。**不进 PATH 查**——这些浏览器安装时不把自己挂到 PATH 上。
#[cfg(target_os = "windows")]
fn chromium_candidates() -> Vec<(&'static str, PathBuf)> {
    let mut out: Vec<(&'static str, PathBuf)> = vec![
        (
            "msedge",
            PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
        ),
        (
            "msedge",
            PathBuf::from(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"),
        ),
    ];
    // 按用户安装（免管理员）的落点在 `%LOCALAPPDATA%` 下。
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        out.push((
            "msedge",
            local.join(r"Microsoft\Edge\Application\msedge.exe"),
        ));
        out.push((
            "chrome",
            local.join(r"Google\Chrome\Application\chrome.exe"),
        ));
    }
    out.push((
        "chrome",
        PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
    ));
    out.push((
        "chrome",
        PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
    ));
    out
}

/// 从候选里挑出第一个真实存在的浏览器（纯函数：只看 `probe`）。
///
/// 与 [`SystemPdfExporter::detect`] 共用同一份候选与判据，
/// 所以「界面说会直接落盘」与「实际真用浏览器落盘」不会分叉。
#[cfg(target_os = "windows")]
fn pick_browser(
    candidates: &[(&'static str, PathBuf)],
    probe: &dyn CommandProbe,
) -> Option<(&'static str, PathBuf)> {
    candidates
        .iter()
        .find(|(_, path)| probe.path_exists(&path.to_string_lossy()))
        .cloned()
}

/// 把本地路径转成 `file:///` URL。
///
/// 空格、中文等非 URL 安全字符必须百分号编码：命令行里塞原始路径时，
/// 地址含空格会让浏览器只取到空格前半截（`D:/我的 文档/x.html` → 找不到文件）。
/// 该函数与平台无关，故放在 `#[cfg]` 之外，三平台都能单测。
///
/// 公开仅为让单测直接断言编码结果，不是给其它模块调的。
pub fn file_url(path: &Path) -> String {
    let mut out = String::from("file:///");
    for byte in path.to_string_lossy().replace('\\', "/").bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 用无头 Chromium 内核把 HTML 渲染成 PDF，返回落盘字节数。
///
/// # 为什么不用 `Start-Process -Verb PrintTo`
///
/// `.html` 的 `printto` 动词在注册表里指向
/// `rundll32.exe mshtml.dll,PrintHTML "%1" "%2"`——**已废弃**的 MSHTML 打印引擎，
/// 且必须经系统打印对话框（用户还得自己在对话框里「另存为 PDF」）。
/// 无头 Chromium 是**静默**的，直接写出用户要的那个 `.pdf`，大小也能如实回报。
///
/// # 参数都是实测结论，别凭印象改
///
/// - `--headless=new`：无窗口。实测**与已运行的 Edge 实例不冲突**，
///   不需要额外的 `--user-data-dir`（也就不必往磁盘上丢一个浏览器 profile）。
/// - `--no-pdf-header-footer`：去掉页眉页脚。**实测 `--print-to-pdf-no-header`
///   在本机 Edge 154 上已失效**（带与不带产物字节数完全一致），别改回旧名。
/// - `@page` 的纸张尺寸会被尊重（A4 → 594.96×841.92pt、A5 → 420×594.96pt，实测）。
#[cfg(target_os = "windows")]
fn headless_print(browser: &Path, html: &Path, pdf: &Path) -> Result<u64, String> {
    let output = Command::new(browser)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            "--no-pdf-header-footer",
        ])
        .arg(format!("--print-to-pdf={}", pdf.display()))
        .arg(file_url(html))
        .output()
        .map_err(|e| format!("调起 {} 失败：{e}", browser.display()))?;

    // 判据是**产物是否存在**而不是退出码：实测无头内核在写盘失败时
    // 也可能以 0 退出（只把原因写到 stderr），只看退出码会报「导出成功」
    // 而磁盘上什么都没有。
    let size = std::fs::metadata(pdf).map(|meta| meta.len()).map_err(|e| {
        format!(
            "{} 未产出 PDF（{}）：{}",
            browser.display(),
            e,
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })?;
    Ok(size)
}

/// 构造调用系统 `printto` 动词的 PowerShell 命令。
///
/// **路径必须走环境变量，不能当尾随参数。** 这是本函数存在的全部理由：
/// `powershell -Command <脚本字符串> <参数…>` 的尾随参数**不会**绑进 `$args`——
/// 实测 `powershell -Command 'Write-Output "[$args]"' hello world` 得到空，
/// 且 `hello` / `world` 会被当成**脚本正文**另起语句执行。
/// 原实现写的是 `-FilePath $args[0]`，于是 `$args[0]` 恒为 `null`，
/// `Start-Process` 直接报 `Cannot validate argument on parameter 'FilePath'`。
///
/// 单独成函数（而不是写在 `shell_print_to` 里）是为了让上面这条不变式
/// 能被单测直接检查：命令里**不许出现路径**，路径只许出现在环境变量里。
#[cfg(target_os = "windows")]
pub fn shell_print_command(html: &Path) -> Command {
    let mut command = Command::new("powershell");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Process -FilePath $env:RUSTMARK_PRINT_SOURCE -Verb PrintTo",
        ])
        .env("RUSTMARK_PRINT_SOURCE", html);
    command
}

/// Windows 兜底：交给系统关联程序的 `printto` 动词（**会弹打印对话框**）。
#[cfg(target_os = "windows")]
fn shell_print_to(html: &Path) -> Result<(), String> {
    let output = shell_print_command(html)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
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
    /// - Windows：优先 **Edge / Chrome**（无头内核，`--print-to-pdf`，
    ///   静默直接落盘），其次 `powershell`（`Start-Process -Verb PrintTo`，
    ///   经打印对话框）。判据与 [`SystemPrintDispatcher`] 完全一致。
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
        super::pulldown_html_exporter::build_printable_document(&title, &with_toc, &page_style)
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
