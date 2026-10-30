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
use rustmark::domain::export::value_objects::{ExportFormat, ExportOptions, PageSize};
use rustmark::infrastructure::export::system_pdf_exporter::{
    CommandProbe, PathProbe, PrintDispatcher, PrintOutcome, SystemPdfExporter,
};
use rustmark::shared::ids::{DocumentId, JobId};
use rustmark::shared::path::FilePath;

/// 只认白名单里命令 / 路径的假探测器。
struct FakeProbe {
    available: Vec<&'static str>,
    /// 「磁盘上存在」的绝对路径。
    ///
    /// 单独一份而不是复用 `available`：Chromium 系浏览器装在固定位置、
    /// **不进 PATH**，探测它的方式与探测 `powershell` / `lp` 不同。
    files: Vec<&'static str>,
}

impl FakeProbe {
    fn with(commands: &[&'static str]) -> Self {
        Self {
            available: commands.to_vec(),
            files: Vec::new(),
        }
    }

    /// 假机器上装了一个 Chromium 系浏览器（路径与真机候选一致）。
    #[cfg(target_os = "windows")]
    fn with_browser(browser: &'static str) -> Self {
        Self {
            available: Vec::new(),
            files: vec![browser],
        }
    }

    /// 什么都没装的机器（用于验证「不可用」分支）。
    fn empty() -> Self {
        Self {
            available: Vec::new(),
            files: Vec::new(),
        }
    }
}

impl CommandProbe for FakeProbe {
    fn exists(&self, command: &str) -> bool {
        self.available.contains(&command)
    }

    fn path_exists(&self, path: &str) -> bool {
        self.files.contains(&path)
    }
}

/// 真机上 Edge 的默认安装路径（候选列表里的第一条）。
// 两个路径常量与 `with_browser` 都只服务于「无头 Chromium」这条 Windows-only
// 路径（见 `SystemPdfExporter::detect` 的平台分支），非 Windows 上没有引用点，
// 不门控会在 `-D warnings` 下报死代码。
#[cfg(target_os = "windows")]
const EDGE_PATH: &str = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
/// 真机上 Chrome 的默认安装路径。
#[cfg(target_os = "windows")]
const CHROME_PATH: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

/// 记录型打印调度：不碰系统，只记下「被要求打印什么、目标在哪」（V11-04）。
///
/// 真实调度会真的调起浏览器 / 弹打印对话框——测试里必须替换掉这一步，
/// 否则 CI 会被弹窗挂住。
struct FakeDispatcher {
    calls: Mutex<Vec<(PathBuf, PathBuf)>>,
    /// 调度的结果：`Ok` 里是要回报的产物形态。
    outcome: Result<PrintOutcome, String>,
}

impl FakeDispatcher {
    /// 直接落盘 PDF 的调度（模拟「本机有 Edge / Chrome」）。
    fn writes(bytes: u64) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            outcome: Ok(PrintOutcome::Wrote(bytes)),
        }
    }

    /// 交给系统对话框的调度（模拟「只有 PowerShell / lp」）。
    fn delegates() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            outcome: Ok(PrintOutcome::Delegated),
        }
    }

    /// 调度失败。
    fn failing() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            outcome: Err("模拟：系统打印命令失败".to_string()),
        }
    }

    fn printed(&self) -> Vec<(PathBuf, PathBuf)> {
        self.calls.lock().expect("调度记录锁").clone()
    }
}

impl PrintDispatcher for FakeDispatcher {
    fn print(
        &self,
        html_path: &std::path::Path,
        pdf_path: &std::path::Path,
    ) -> Result<PrintOutcome, String> {
        self.calls
            .lock()
            .expect("调度记录锁")
            .push((html_path.to_path_buf(), pdf_path.to_path_buf()));
        self.outcome.clone()
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

/// Windows：**优先**无头 Chromium（Edge → Chrome），它才直接落盘 PDF。
///
/// 原实现优先 PowerShell，理由是「系统自带、不依赖浏览器」；但
/// `Start-Process -Verb PrintTo` 走的是注册表里 `htmlfile\shell\printto`
/// 指向的 `rundll32 mshtml.dll,PrintHTML`——已废弃的 MSHTML 引擎，
/// 且必须经打印对话框。本机实测 Edge 的 `--print-to-pdf` 静默产出真 PDF，
/// 故改为浏览器优先、PowerShell 兜底。
#[test]
#[cfg(target_os = "windows")]
fn windows_prefers_headless_browser_and_reports_direct_writes() {
    let edge = SystemPdfExporter::detect(&FakeProbe::with_browser(EDGE_PATH));
    assert!(edge.available);
    assert_eq!(edge.command, "msedge");
    assert!(edge.writes_pdf, "无头内核会直接产出 PDF 文件");

    let chrome = SystemPdfExporter::detect(&FakeProbe::with_browser(CHROME_PATH));
    assert_eq!(chrome.command, "chrome");
    assert!(chrome.writes_pdf);

    // Edge 与 Chrome 都在时选 Edge（系统预装，覆盖率更高）。
    let both = FakeProbe {
        available: Vec::new(),
        files: vec![EDGE_PATH, CHROME_PATH],
    };
    assert_eq!(SystemPdfExporter::detect(&both).command, "msedge");
}

/// 没有浏览器时退回 PowerShell：仍可用，但要经打印对话框。
#[test]
#[cfg(target_os = "windows")]
fn windows_falls_back_to_powershell_dialog_when_no_browser() {
    let backend = SystemPdfExporter::detect(&FakeProbe::with(&["powershell"]));
    assert!(backend.available);
    assert_eq!(backend.command, "powershell");
    assert!(
        !backend.writes_pdf,
        "`printto` 动词要经系统打印对话框，不能承诺直接落盘"
    );

    // pwsh（PowerShell 7）也应被接受。
    let backend = SystemPdfExporter::detect(&FakeProbe::with(&["pwsh"]));
    assert!(backend.available);
}

/// `file:///` URL 必须对空格与非 ASCII 做百分号编码。
///
/// 不编码的后果不是「报错」而是**静默找错文件**：命令行里的地址在第一个
/// 空格处就被截断，浏览器打开/打印的是另一个路径（或压根不存在）。
#[test]
fn file_url_percent_encodes_spaces_and_non_ascii() {
    // 纯 ASCII 路径保持原样，只把 `\` 换成 `/`。
    assert_eq!(
        rustmark::infrastructure::export::system_pdf_exporter::file_url(std::path::Path::new(
            r"D:\downloads\Mdown\1.print.html"
        )),
        "file:///D:/downloads/Mdown/1.print.html"
    );

    // 空格 → %20；中文按 UTF-8 逐字节编码。
    let encoded = rustmark::infrastructure::export::system_pdf_exporter::file_url(
        std::path::Path::new("D:/我的 文档/1.print.html"),
    );
    assert!(
        encoded.starts_with("file:///D:/"),
        "盘符与冒号不该被编码：{encoded}"
    );
    assert!(encoded.contains("%20"), "空格必须编码：{encoded}");
    assert!(!encoded.contains(' '), "编码后不该再有裸空格：{encoded}");
    assert!(encoded.is_ascii(), "编码后应全是 ASCII：{encoded}");
    assert!(
        encoded.ends_with("1.print.html"),
        "末尾的文件名该保持可读：{encoded}"
    );
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
    let dispatcher = Arc::new(FakeDispatcher::writes(1));
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

/// PDF **两种产出方式**的端到端：打印用 HTML 落盘 → 调起调度 → 回报大小。
///
/// 用假探测器（假装后端都在）+ 假调度（不碰系统）跑真实链路，
/// 于是 CI 上既不装打印服务、也不弹对话框，仍能验证整条路径。
#[test]
fn pdf_export_writes_print_html_and_dispatches_to_the_target_path() {
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
    let dispatcher = Arc::new(FakeDispatcher::delegates());
    let exporter = SystemPdfExporter::with_probe_and_dispatcher(
        Arc::new(FakeProbe::with(&["powershell", "pwsh", "lp", "lpstat"])),
        dispatcher.clone(),
    );

    let bytes = exporter
        .export("# 标题\n\n正文\n", &job)
        .expect("后端可用 + 调度成功时应成功");

    // 交给对话框那条路：产物由用户在对话框里另存，本进程拿不到大小 → 约定返回 0。
    assert_eq!(bytes, 0, "交对话框时不落盘，bytes 约定为 0");

    // 落盘的是**打印用 HTML**（扩展名换成 .print.html），不是 .pdf。
    let html = dir.path().join("doc.print.html");
    assert!(html.exists(), "必须生成打印用 HTML：{}", html.display());
    assert!(
        !target.exists(),
        "不得凭空产出一个空的 .pdf——那会让用户以为导出成功了"
    );
    let text = std::fs::read_to_string(&html).expect("打印用 HTML 应可读");
    assert!(text.contains("标题"), "打印用 HTML 应含渲染后的正文");

    // 打印确实被调起一次，且同时收到「渲染源」与「目标 PDF」两条信息：
    // 无头内核需要目标路径才能直接落盘，只给 HTML 路径就退化成只能弹对话框。
    let calls = dispatcher.printed();
    assert_eq!(calls.len(), 1, "应调起一次系统打印");
    assert_eq!(calls[0].0, html, "交给打印的必须是打印用 HTML");
    assert_eq!(calls[0].1, target, "调度端还要知道 PDF 该写到哪");
}

/// 无头内核直接落盘时，必须回报**真实字节数**而不是 0。
///
/// 回报 0 会让界面把一次成功的直接导出说成「请在对话框里另存为 PDF」，
/// 用户会跑去找一个并不存在的对话框。
// 仅 Windows：**只有** Windows 的 `PdfBackend::detect` 会走「无头 Chromium →
// `PdfBackend::direct`」这条能直接落盘的路径；macOS / Linux 分支只能返回
// `dialog`（CUPS 从源 HTML 打印，目标由 CUPS 决定），永远拿不到 `direct`，
// 于是 `export()` 只会报 `IoUnavailable`、本用例必红（run #11/#12 实测）。
// 给 CI 装 CUPS 也救不了——装了仍只是 `dialog`，`writes_pdf` 依旧为 false。
#[test]
#[cfg(target_os = "windows")]
fn pdf_export_reports_real_size_when_written_directly() {
    let dir = tempfile::tempdir().expect("临时目录");
    let target = dir.path().join("direct.pdf");
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Pdf,
        ExportOptions::default_options(),
    );
    let exporter = SystemPdfExporter::with_probe_and_dispatcher(
        Arc::new(FakeProbe::with_browser(EDGE_PATH)),
        Arc::new(FakeDispatcher::writes(51_534)),
    );

    let bytes = exporter.export("# 标题\n", &job).expect("直接落盘应成功");
    assert_eq!(bytes, 51_534, "应回报内核写出的真实字节数");
}

/// 调度失败（系统打印命令报错）必须明确报错，不能静默算成功。
#[test]
fn pdf_export_reports_dispatch_failure() {
    let dispatcher = Arc::new(FakeDispatcher::failing());
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

/// 打印用 HTML 必须带上用户选的纸张尺寸（`@page`）。
///
/// 界面一直写着「纸张…会写入导出件的 `@page` 规则」，但打印样式表里
/// 此前**一条 `@page` 都没有**，纸张选项从未生效。无头内核会按 `@page`
/// 排版（A4 实测 595×842pt、A5 420×595pt），所以这条规则是真的有用的。
#[test]
fn print_html_carries_the_chosen_page_size() {
    let dir = tempfile::tempdir().expect("临时目录");
    let dest = FilePath::new(
        dir.path()
            .join("paper.pdf")
            .to_string_lossy()
            .replace('\\', "/"),
    )
    .expect("路径应合法");

    let job_for = |size| {
        ExportJob::new(
            JobId::new(),
            DocumentId::new(),
            dest.clone(),
            ExportFormat::Pdf,
            ExportOptions::new(true, true, "default", size, true),
        )
    };

    let a4 = SystemPdfExporter::build_print_html("# 标题\n", &job_for(PageSize::A4));
    assert!(
        a4.contains("@page{size:A4;"),
        "A4 的 `@page` 规则缺失：{}",
        a4.lines()
            .find(|l| l.contains("@page"))
            .unwrap_or("<无 @page>")
    );

    let letter = SystemPdfExporter::build_print_html("# 标题\n", &job_for(PageSize::Letter));
    assert!(
        letter.contains("@page{size:letter;"),
        "Letter 的 `@page` 规则缺失"
    );
    assert!(!letter.contains("size:A4"), "换了纸张就不该还留着 A4 规则");
}

// ---------------------------------------------------------------------------
// H-1：打印路径必须复用 HTML 白名单清洗
// ---------------------------------------------------------------------------

/// 打印用 HTML 会由无头 Chromium 以 `file:///` 渲染成 PDF——那是真实浏览器
/// 引擎，用户注入的 `<script>` 与事件属性会执行。故打印正文必须先过
/// `sanitize_html`（与 HTML 导出同一道门）。
#[test]
fn print_html_drops_scripts_and_event_handlers() {
    let job = pdf_job("sanitize.pdf");
    let html = SystemPdfExporter::build_print_html(
        "正文\n\n<script>alert(1)</script>\n\n<img src=x onerror=alert(2)>\n",
        &job,
    );

    assert!(!html.contains("<script"), "用户脚本必须被丢弃：{html}");
    assert!(!html.contains("onerror"), "事件属性必须被剥除：{html}");
    assert!(
        !html.to_ascii_lowercase().contains("javascript:"),
        "javascript: URL 必须被剥除：{html}"
    );
    assert!(html.contains("正文"), "正文必须保留：{html}");
}

/// 链接里的 `javascript:` 协议同样不得进入打印产物。
#[test]
fn print_html_drops_javascript_urls() {
    let job = pdf_job("jsurl.pdf");
    let html = SystemPdfExporter::build_print_html("[x](javascript:alert(1))", &job);

    assert!(
        !html.to_ascii_lowercase().contains("javascript:"),
        "javascript: 链接必须被剥除：{html}"
    );
}

/// 清洗不能误伤公式 / 图表 / 图片，也不能误伤导出器自己注入的 CDN 资源。
#[test]
fn print_html_keeps_math_mermaid_and_images_but_drops_user_scripts() {
    let job = pdf_job("keep.pdf");
    let content = "公式 $x^2$\n\n```mermaid\ngraph TD;\n  A-->B;\n```\n\n\
                   ![图](./a.png)\n\n<script>alert(1)</script>\n";
    let html = SystemPdfExporter::build_print_html(content, &job);

    assert!(html.contains("class=\"math"), "公式 span 必须保留：{html}");
    assert!(
        html.contains("language-mermaid"),
        "mermaid 代码块必须保留：{html}"
    );
    assert!(html.contains("katex.min.js"), "公式资源仍应注入：{html}");
    assert!(
        html.contains("<img src=\"./a.png\""),
        "图片必须保留（`./` 相对路径在清洗白名单内）：{html}"
    );
    // 导出器自己的 CDN <script> 允许存在，但用户注入的必须消失。
    assert!(
        !html.contains("<script>alert"),
        "用户 script 必须消失：{html}"
    );
    assert!(!html.contains("onerror"), "事件属性必须消失：{html}");
}

/// 无头打印命令的参数形状：PDF 输出参数在前、源 URL 在后。
///
/// 这里刻意断言**不含** `--blink-settings=scriptEnabled=false`：该参数在本机
/// Edge 154 实测会让 `--print-to-pdf` 完全不产出文件（打印整体失败），
/// 因此第二道防线改在 `ensure_print_source_is_sanitized`（见下一个用例）。
#[cfg(target_os = "windows")]
#[test]
fn headless_print_command_args_carry_pdf_output_and_source() {
    use std::path::Path;

    let dir = tempfile::tempdir().expect("临时目录");
    let html = dir.path().join("a.print.html");
    std::fs::write(&html, "<html><body>纯文本</body></html>").expect("应能写入");
    let pdf = dir.path().join("a.pdf");

    let command = rustmark::infrastructure::export::system_pdf_exporter::headless_print_command(
        Path::new("msedge.exe"),
        &html,
        &pdf,
    );
    let args: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    let print_at = args
        .iter()
        .position(|a| a.starts_with("--print-to-pdf="))
        .expect("缺少 PDF 输出参数");
    let url_at = args
        .iter()
        .position(|a| a.starts_with("file:///"))
        .expect("缺少打印源 URL");
    assert!(
        print_at < url_at,
        "PDF 输出参数必须排在源 URL 之前：{args:?}"
    );
    assert!(
        !args.iter().any(|a| a.contains("scriptEnabled=false")),
        "不得使用实测会让打印失败的 blink 开关：{args:?}"
    );
}

/// 第二道防线：打印源里只允许导出器自己注入的脚本存在。
///
/// 静态文档应为 0 条；含公式 / 图表时恰好是注入的 CDN + 渲染脚本条数。
/// 多出来的任何 `<script` 都说明清洗漏了，必须拒绝打印（fail-closed）。
#[cfg(target_os = "windows")]
#[test]
fn print_source_guard_rejects_uncleaned_scripts() {
    use rustmark::infrastructure::export::system_pdf_exporter::ensure_print_source_is_sanitized;

    // 静态文档：没有任何脚本。
    assert!(
        ensure_print_source_is_sanitized("<html><body><p>纯文本</p></body></html>").is_ok(),
        "静态文档不应被拒"
    );

    // 静态文档混入用户脚本：必须拒绝。
    let error = ensure_print_source_is_sanitized(
        "<html><body><p>纯文本</p><script>alert(1)</script></body></html>",
    )
    .expect_err("用户脚本必须被拦下");
    assert!(error.contains("未清洗"), "错误文案应指出来源：{error}");

    // 含公式：KaTeX 脚本 + 内联渲染脚本 = 2 条，属于预期。
    assert!(
        ensure_print_source_is_sanitized(
            "<html><body><span class=\"math math-inline\">x</span>\
             <script defer src=\"katex.min.js\"></script><script defer>render()</script>\
             </body></html>"
        )
        .is_ok(),
        "公式文档的注入脚本不应被拒"
    );

    // 公式文档里再多一条用户脚本：超出预期，拒绝。
    let error = ensure_print_source_is_sanitized(
        "<html><body><span class=\"math math-inline\">x</span>\
         <script defer src=\"katex.min.js\"></script><script defer>render()</script>\
         <script>alert(1)</script></body></html>",
    )
    .expect_err("多出的脚本必须被拦下");
    assert!(error.contains("未清洗"), "错误文案应指出来源：{error}");
}

/// Windows 兜底命令：**路径只能走环境变量**，绝不能作为尾随参数。
///
/// 这条钉的是用户实际遇到的那个报错。`powershell -Command <脚本> <参数…>`
/// 的尾随参数**不会**绑进 `$args`——实测
/// `powershell -NoProfile -Command 'Write-Output "[$args]"' hello world`
/// 输出 `[]`，且 `hello` / `world` 被当成脚本正文另起语句执行。
/// 原实现写 `-FilePath $args[0]`，于是 `$args[0]` 恒为 `null`，
/// `Start-Process` 直接报 `Cannot validate argument on parameter 'FilePath'`。
///
/// 这里不真的执行命令（那会弹出打印对话框），只检查命令的构造：
/// 环境变量里有路径，命令行里没有路径。
#[test]
#[cfg(target_os = "windows")]
fn powershell_fallback_passes_the_path_via_env_not_argv() {
    let html = std::path::Path::new(r"D:\我的 文档\1.print.html");
    let command = rustmark::infrastructure::export::system_pdf_exporter::shell_print_command(html);

    let envs: Vec<(String, Option<String>)> = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert!(
        envs.iter()
            .any(|(key, value)| key == "RUSTMARK_PRINT_SOURCE"
                && value.as_deref() == Some(r"D:\我的 文档\1.print.html")),
        "路径必须经环境变量传入，实际 {envs:?}"
    );

    let args: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(
        !args
            .iter()
            .any(|a| a.contains("print.html") || a.contains("$args")),
        "路径不能出现在命令行里，脚本也不能引用 `$args`（它拿不到尾随参数）：{args:?}"
    );
}

/// 真实链路（需要本机装了 Edge / Chrome，否则跳过）：**走完整的 `export()`**，
/// 检验磁盘上真的出现一个可用的 PDF。
///
/// 这是唯一能挡住「命令拼错、参数被吞、产物没落盘」这类问题的用例——
/// 用户报的那个 bug 恰恰是**没人跑过真实命令**：单测全程用假调度，
/// 命令能编译、探测也能通过，直到用户点导出才炸。
#[test]
#[cfg(target_os = "windows")]
fn real_export_writes_a_real_pdf_to_the_chosen_path() {
    let backend = SystemPdfExporter::detect(&PathProbe);
    if !backend.writes_pdf {
        eprintln!(
            "跳过：本机未探测到 Edge / Chrome（探测结果 {}），真实链路无法验证",
            backend.command
        );
        return;
    }

    let dir = tempfile::tempdir().expect("临时目录");
    let target = dir.path().join("real.pdf");
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Pdf,
        ExportOptions::default_options(),
    );

    let bytes = SystemPdfExporter::new()
        .export("# 中文标题\n\n正文探针\n", &job)
        .expect(
            "无头内核应能产出 PDF\
             （若在受限沙箱里运行，子进程可能被静默拦截——换普通终端重跑）",
        );

    // 1) 用户选的路径上出现了真 PDF，且回报的字节数与磁盘一致。
    let raw = std::fs::read(&target).expect("PDF 应落在用户选的路径上");
    assert_eq!(
        &raw[..5],
        b"%PDF-",
        "产物不是 PDF（头部为 {:?}）",
        String::from_utf8_lossy(&raw[..5.min(raw.len())])
    );
    assert_eq!(
        bytes as u64,
        raw.len() as u64,
        "回报的字节数必须与落盘文件一致（界面会把它显示给用户）"
    );
    assert!(
        raw.len() > 1000,
        "PDF 太小，八成是空文档：{} 字节",
        raw.len()
    );

    // 2) `@page{size:A4}` 必须被内核尊重：A4 ≈ 595×842pt（Letter 是 612×792）。
    //    MediaBox 在 PDF 里是明文，直接找它并取出四个数，避免引入 PDF 解析依赖。
    let text = String::from_utf8_lossy(&raw);
    let at = text.find("/MediaBox").expect("PDF 里应有 /MediaBox");
    // PDF 是二进制，`at + 80` 可能落在字符中间；取不到就退回尾部切片。
    let window = text
        .get(at..(at + 80).min(text.len()))
        .unwrap_or(&text[at..]);
    let media_box: Vec<f32> = window
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect();
    assert!(media_box.len() >= 4, "MediaBox 应有四个数：{media_box:?}");
    let (width, height) = (media_box[2], media_box[3]);
    // 容差 2pt：A4 的精确值是 595.276×841.89pt，内核给的是四舍五入后的点数。
    assert!(
        (width - 595.0).abs() < 2.0 && (height - 842.0).abs() < 2.0,
        "纸张没按 `@page` 走（A4 应约 595×842pt，Letter 是 612×792）：{width}×{height}pt"
    );
}
