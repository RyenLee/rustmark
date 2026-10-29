//! 打印后端的探测模型：后端描述、可注入探测器、PATH 查找与调度结果。

use std::path::Path;

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
    /// 也由同一判据决定（见 `SystemPdfExporter::detect` 与 `SystemPrintDispatcher`），
    /// 所以两者不会分叉。
    pub writes_pdf: bool,
}

impl PdfBackend {
    /// 构造「可用、且直接落盘 PDF」的结果。
    ///
    /// 仅 Windows 存在「直接落盘」的后端——只有无头 Chromium 能把 PDF 静默写到
    /// 目标路径；macOS / Linux 的 `lp` 一律走 CUPS 队列、必须经打印对话框。
    /// 因此本构造子在非 Windows 上不可达，随变体一起门控，避免 `-D warnings`
    /// 下的 dead-code 误报（run #12 的 ubuntu / macos clippy 即红在此）。
    #[cfg(target_os = "windows")]
    pub(super) const fn direct(command: &'static str) -> Self {
        Self {
            command,
            available: true,
            reason: "",
            writes_pdf: true,
        }
    }

    /// 构造「可用，但要经系统打印对话框」的结果。
    pub(super) const fn dialog(command: &'static str) -> Self {
        Self {
            command,
            available: true,
            reason: "",
            writes_pdf: false,
        }
    }

    /// 构造「不可用」结果。
    pub(super) const fn unavailable(command: &'static str, reason: &'static str) -> Self {
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
