//! `SystemPdfExporter` 单测（S5-10）。
//!
//! 重点是**可用性探测**：把 `CommandProbe` 换成 fake，
//! 就能在没有任何打印服务的 CI 机器上覆盖三个平台的全部分支。
//! 真实调用系统打印会弹窗，因此**不在单测中触发**（阶段 07 手工验证）。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rustmark::domain::export::aggregate::ExportJob;
use rustmark::domain::export::errors::ExportError;
use rustmark::domain::export::services::Exporter;
use rustmark::domain::export::value_objects::{ExportFormat, ExportOptions};
use rustmark::infrastructure::export::system_pdf_exporter::{
    CommandProbe, PathProbe, PrintDispatcher, SystemPdfExporter,
};
use rustmark::shared::ids::{DocumentId, JobId};
use rustmark::shared::path::FilePath;

/// 只认白名单里命令的假探测器。
struct FakeProbe {
    available: Vec<&'static str>,
}

impl FakeProbe {
    fn with(commands: &[&'static str]) -> Self {
        Self {
            available: commands.to_vec(),
        }
    }

    /// 什么都没装的机器（用于验证「不可用」分支）。
    fn empty() -> Self {
        Self {
            available: Vec::new(),
        }
    }
}

impl CommandProbe for FakeProbe {
    fn exists(&self, command: &str) -> bool {
        self.available.contains(&command)
    }
}

/// 记录型打印调度：不碰系统，只记下「被要求打印哪个文件」（V11-04）。
///
/// PDF 开放后，真实调度会弹出打印对话框——测试里必须替换掉这一步，
/// 否则 CI 会被弹窗挂住。
struct FakeDispatcher {
    calls: Mutex<Vec<PathBuf>>,
    fail: bool,
}

impl FakeDispatcher {
    fn ok() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail: false,
        }
    }

    fn printed(&self) -> Vec<PathBuf> {
        self.calls.lock().expect("调度记录锁").clone()
    }
}

impl PrintDispatcher for FakeDispatcher {
    fn print(&self, html_path: &std::path::Path) -> Result<(), String> {
        self.calls
            .lock()
            .expect("调度记录锁")
            .push(html_path.to_path_buf());
        if self.fail {
            return Err("模拟：系统打印命令失败".to_string());
        }
        Ok(())
    }
}

/// 构造一个 PDF 导出任务（目标写到临时目录，不会真的落盘）。
fn pdf_job(name: &str) -> ExportJob {
    let target = std::env::temp_dir().join("rustmark-pdf-test").join(name);
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Pdf,
        ExportOptions::default_options(),
    )
}

#[test]
fn empty_probe_reports_unavailable_with_actionable_message() {
    let backend = SystemPdfExporter::detect(&FakeProbe::empty());

    assert!(!backend.available, "没有任何打印后端时必须报告不可用");
    assert!(
        !backend.reason.is_empty(),
        "不可用时必须给出原因，不能是空串"
    );
    // 验收标准：错误文案必须「可操作」——引导用户先导出 HTML。
    assert!(
        backend.reason.contains("HTML"),
        "错误文案应引导用户先导出 HTML，实际：{}",
        backend.reason
    );
}

#[test]
fn current_platform_backend_is_detected_when_available() {
    // 只放行当前平台真正会探测的那个命令。
    let expected: &[&str] = if cfg!(target_os = "windows") {
        &["powershell"]
    } else {
        &["lp"]
    };
    let backend = SystemPdfExporter::detect(&FakeProbe::with(expected));
    assert!(
        backend.available,
        "提供 {} 时应判定为可用，实际 reason={}",
        expected[0], backend.reason
    );
    assert!(backend.reason.is_empty(), "可用时 reason 应为空串");
}

/// Windows 优先用 PowerShell（系统自带，不依赖用户装了浏览器）。
#[test]
#[cfg(target_os = "windows")]
fn windows_prefers_powershell_over_browser() {
    let backend = SystemPdfExporter::detect(&FakeProbe::with(&["powershell"]));
    assert!(backend.available);
    assert_eq!(backend.command, "powershell");

    // pwsh（PowerShell 7）也应被接受。
    let backend = SystemPdfExporter::detect(&FakeProbe::with(&["pwsh"]));
    assert!(backend.available);
}

/// 非 Windows 平台：`lp` 优先于 `lpstat`。
#[test]
#[cfg(not(target_os = "windows"))]
fn unix_prefers_lp_over_lpstat() {
    let both = SystemPdfExporter::detect(&FakeProbe::with(&["lp", "lpstat"]));
    assert_eq!(both.command, "lp", "两者都有时应选 lp");

    // 只有 lpstat（CUPS 客户端不完整）时仍算「有后端」，
    // 但真正打印时会给出更具体的提示，由 `dispatch_print` 负责。
    let stat_only = SystemPdfExporter::detect(&FakeProbe::with(&["lpstat"]));
    assert!(
        stat_only.available,
        "lpstat 存在说明 CUPS 至少装了一半，探测层不应直接判死"
    );
}

#[test]
fn html_format_is_rejected_by_pdf_exporter() {
    let html_job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        FilePath::new("/tmp/x.html").expect("路径应合法"),
        ExportFormat::Html,
        ExportOptions::default_options(),
    );

    let error = SystemPdfExporter::new()
        .export("# x", &html_job)
        .expect_err("HTML 不应由 PDF 导出器处理");
    assert!(
        matches!(error, ExportError::IoUnavailable { .. }),
        "应返回资源不可用错误，实际：{error:?}"
    );
}

#[test]
fn markdown_format_is_rejected_by_pdf_exporter() {
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        FilePath::new("/tmp/y.md").expect("路径应合法"),
        ExportFormat::Markdown,
        ExportOptions::default_options(),
    );
    assert!(
        SystemPdfExporter::new().export("# x", &job).is_err(),
        "Markdown 不应由 PDF 导出器处理"
    );
}

/// PDF 任务在目标目录不存在时也不应 panic——
/// 真实打印路径不可用时会在写盘前就返回错误。
#[test]
fn pdf_export_without_backend_fails_cleanly() {
    // 探测已可注入（V11-04），故这条用例在任何机器上都**确定**地走
    // 「无后端」分支：不再依赖本机有没有装打印服务，也不会误弹打印对话框。
    let dispatcher = Arc::new(FakeDispatcher::ok());
    let exporter = SystemPdfExporter::with_probe_and_dispatcher(
        Arc::new(FakeProbe::empty()),
        dispatcher.clone(),
    );
    let job = pdf_job("unavailable.pdf");
    let result = exporter.export("# 标题", &job);

    match result {
        Err(ExportError::IoUnavailable { message }) => {
            assert!(
                message.contains("HTML"),
                "不可用时的文案应引导导出 HTML，实际：{message}"
            );
        }
        Err(other) => panic!("应是资源不可用错误，实际：{other:?}"),
        Ok(_) => panic!("无打印后端时不应报告成功"),
    }
    assert!(
        dispatcher.printed().is_empty(),
        "后端不可用就绝不能调起打印"
    );
}

#[test]
fn real_probe_does_not_panic_and_returns_a_decision() {
    // 真实 PATH 探测：无论本机有没有打印服务，都必须返回一个明确结论，
    // 不能 panic（CI 环境通常两者都没有）。
    let backend = SystemPdfExporter::detect(&PathProbe);
    if !backend.available {
        assert!(!backend.reason.is_empty(), "不可用时必须给出原因");
    }
}

/// PDF **开放后**的端到端（V11-04）：打印用 HTML 落盘 → 调起打印 → 返回 0。
///
/// 用假探测器（假装后端都在）+ 假调度（不碰系统）跑真实链路，
/// 于是 CI 上既不装打印服务、也不弹对话框，仍能验证整条路径。
#[test]
fn pdf_export_writes_print_html_dispatches_print_and_reports_zero_bytes() {
    let dir = tempfile::tempdir().expect("临时目录");
    let target = dir.path().join("doc.pdf");
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Pdf,
        ExportOptions::default_options(),
    );
    let dispatcher = Arc::new(FakeDispatcher::ok());
    let exporter = SystemPdfExporter::with_probe_and_dispatcher(
        Arc::new(FakeProbe::with(&["powershell", "pwsh", "lp", "lpstat"])),
        dispatcher.clone(),
    );

    let bytes = exporter
        .export("# 标题\n\n正文\n", &job)
        .expect("后端可用 + 调度成功时应成功");

    // 产物由用户在系统对话框里另存，本进程拿不到大小 → 约定返回 0。
    assert_eq!(bytes, 0, "PDF 不经本进程落盘，bytes 约定为 0");

    // 落盘的是**打印用 HTML**（扩展名换成 .print.html），不是 .pdf。
    let html = dir.path().join("doc.print.html");
    assert!(html.exists(), "必须生成打印用 HTML：{}", html.display());
    assert!(
        !target.exists(),
        "不得凭空产出一个空的 .pdf——那会让用户以为导出成功了"
    );
    let text = std::fs::read_to_string(&html).expect("打印用 HTML 应可读");
    assert!(text.contains("标题"), "打印用 HTML 应含渲染后的正文");

    // 打印确实被调起一次，且交给它的是打印用 HTML。
    let calls = dispatcher.printed();
    assert_eq!(calls.len(), 1, "应调起一次系统打印");
    assert_eq!(calls[0], html, "交给打印的必须是打印用 HTML");
}

/// 调度失败（系统打印命令报错）必须明确报错，不能静默算成功。
#[test]
fn pdf_export_reports_dispatch_failure() {
    let dispatcher = Arc::new(FakeDispatcher {
        calls: Mutex::new(Vec::new()),
        fail: true,
    });
    let exporter = SystemPdfExporter::with_probe_and_dispatcher(
        Arc::new(FakeProbe::with(&["powershell", "pwsh", "lp", "lpstat"])),
        dispatcher,
    );
    let job = pdf_job("dispatch-fail.pdf");

    let error = exporter
        .export("# 标题", &job)
        .expect_err("系统打印失败必须报错");
    assert!(
        error.to_string().contains("调用系统打印失败"),
        "错误应指向打印环节，实际：{error}"
    );
}
