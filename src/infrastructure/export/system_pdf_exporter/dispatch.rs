//! 真实系统打印调度：无头 Chromium `--print-to-pdf`、Windows `printto` 兜底、
//! 非 Windows 的 `lp`（CUPS）。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::backend::{CommandProbe, PathProbe, PrintOutcome};

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
            if super::backend_hint_is_stat_only() {
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
pub(super) fn chromium_candidates() -> Vec<(&'static str, PathBuf)> {
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
/// 与 `SystemPdfExporter::detect` 共用同一份候选与判据，
/// 所以「界面说会直接落盘」与「实际真用浏览器落盘」不会分叉。
#[cfg(target_os = "windows")]
pub(super) fn pick_browser(
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
