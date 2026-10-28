//! 预览区外链打开：经端口而非直调进程（R-03）。
//!
//! # 这条用例守什么
//!
//! R-03 把 `std::process` 移出 `shared` 后，预览区点击外链改走
//! `Services::opener`（端口在 `shared`、实现在 `infrastructure`）。
//! **改造前这个行为无法测**——`open_external` 真起一个 `cmd` 进程，
//! 既不幂等也没法观察；改造后可注入假实现，断言三件事：
//!
//! 1. 点了 `link` span 会调 `open`；
//! 2. 参数是**那个链接的地址**（不是别的 span、不是空串）；
//! 3. **不安全的地址不调**（`javascript:` 等被 `is_safe_to_open` 拦在
//!    端口之前）——这一条是 XSS 防护的回归，端口化时最容易顺手丢掉。
//!
//! 另有一条守「端口真的被用」：若哪天有人把调用改回直连
//! `ports::system::open_external`，编译会先失败（函数已不存在），
//! 而这条用例保证**即便将来又把自由函数加回来**，行为仍走端口。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rustmark::app::bootstrap::Services;
use rustmark::app::frame::EditorApp;
use rustmark::application::dto::{RangeView, SpanView};
use rustmark::ports::system::{ExternalOpener, is_safe_to_open};
use rustmark::ports::time::SystemClock;
use rustmark::presentation::views::preview_view::test_only::handle_span_click;

use super::support::fake_dialog::FakeDialog;

/// 记录调用的假打开器。
///
/// 同时充当「可观测的 `ExternalOpener`」与「可注入的端口」——
/// 这正是 R-03 把执行端抽成 trait 的理由。
struct RecordingOpener {
    calls: Mutex<Vec<String>>,
}

impl RecordingOpener {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl ExternalOpener for RecordingOpener {
    fn open(&self, target: &str) -> std::io::Result<()> {
        self.calls.lock().unwrap().push(target.to_string());
        Ok(())
    }
}

/// 从不被调用的打开器（用于「不该调」的断言）。
///
/// 用计数器而非 `panic!`：断言失败时 `panic` 会带出调用栈，
/// 但这里要的是「调用次数 = 0」这个**事实**，且不希望真的 panic
/// 把测试变成「崩了才知道错」。
#[derive(Default)]
struct CountingOpener {
    count: AtomicUsize,
}

impl ExternalOpener for CountingOpener {
    fn open(&self, _target: &str) -> std::io::Result<()> {
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// 造一个装着假打开器的应用，返回 recorder 便于断言。
fn app_with_recorder() -> (tempfile::TempDir, EditorApp, Arc<RecordingOpener>) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let recorder = RecordingOpener::new();
    let services = Services::assemble_with_opener(
        rustmark::app::bootstrap::ConfigPaths::in_dir(dir.path()),
        Arc::new(SystemClock),
        Arc::new(FakeDialog::new()),
        recorder.clone() as Arc<dyn ExternalOpener>,
    )
    .expect("装配应成功");
    let app = EditorApp::new(services, false);
    (dir, app, recorder)
}

/// 造一个装着「只计数」打开器的应用。
fn app_with_counter() -> (tempfile::TempDir, EditorApp, Arc<CountingOpener>) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let counter = Arc::new(CountingOpener::default());
    let services = Services::assemble_with_opener(
        rustmark::app::bootstrap::ConfigPaths::in_dir(dir.path()),
        Arc::new(SystemClock),
        Arc::new(FakeDialog::new()),
        counter.clone() as Arc<dyn ExternalOpener>,
    )
    .expect("装配应成功");
    let app = EditorApp::new(services, false);
    (dir, app, counter)
}

/// 造一个 `link` span。
fn link_span(start: usize, end: usize, url: &str) -> SpanView {
    SpanView {
        range: RangeView::new(start, end),
        style: "link".to_string(),
        url: Some(url.to_string()),
    }
}

/// 点击安全外链 → 调 `open` 且参数正确。
#[test]
fn clicking_a_safe_link_calls_the_opener_with_that_url() {
    let (_dir, mut app, recorder) = app_with_recorder();

    let content = "见 [官网](https://example.com) 详情";
    let sv = link_span(3, 5, "https://example.com");
    handle_span_click(&mut app, &sv, content, &[]);

    assert_eq!(
        recorder.calls(),
        vec!["https://example.com".to_string()],
        "点击安全外链应调用一次 open，且参数就是那个地址"
    );
}

/// 不安全地址（XSS 向量）**不调** `open`。
///
/// # 为什么这条最关键
///
/// 端口化把「校验」和「执行」拆到两处，重构时最容易顺手把校验那一步
/// 丢掉（「反正 opener 自己会处理」——它不会，它只是起进程）。
/// 这条守住「`is_safe_to_open` 仍在调用路径上」。
#[test]
fn an_unsafe_link_never_reaches_the_opener() {
    let (_dir, mut app, counter) = app_with_counter();

    for evil in [
        "javascript:alert(1)",
        "JavaScript://evil.com",
        "vbscript:msgbox",
        "data:text/html,<script>",
    ] {
        assert!(
            !is_safe_to_open(evil),
            "用例前提：{evil} 必须被 is_safe_to_open 判为不安全"
        );
        let content = format!("[x]({evil})");
        let sv = link_span(0, 1, evil);
        handle_span_click(&mut app, &sv, &content, &[]);
    }

    assert_eq!(
        counter.count.load(Ordering::SeqCst),
        0,
        "不安全地址必须在到达端口之前就被拦下，一次都不该调 open"
    );
}

/// `#锚点` 走内部跳转，**不**打开外部程序。
///
/// 锚点是文档内跳转（`sync.request_click`），若误把它交给 opener，
/// 会弹出一个浏览器——那是很显眼的用户可见错误。
#[test]
fn an_anchor_link_does_not_open_anything() {
    let (_dir, mut app, counter) = app_with_counter();

    let content = "见 [第二章](#第二章) 的说明";
    let sv = link_span(3, 6, "#第二章");
    handle_span_click(&mut app, &sv, content, &[]);

    assert_eq!(
        counter.count.load(Ordering::SeqCst),
        0,
        "内部锚点应走 sync.request_click，不得惊动外部打开器"
    );
}

/// 无 `url` 的 link span 安全返回（不 panic、不调 open）。
#[test]
fn a_link_without_url_is_a_no_op() {
    let (_dir, mut app, counter) = app_with_counter();

    let content = "[裸链接]";
    let sv = SpanView {
        range: RangeView::new(0, 5),
        style: "link".to_string(),
        url: None,
    };
    handle_span_click(&mut app, &sv, content, &[]);

    assert_eq!(
        counter.count.load(Ordering::SeqCst),
        0,
        "没有地址的 link span 不该调用打开器"
    );
}

/// `file://` 与相对路径是本地文件，允许打开。
///
/// 与 XSS 那条互为对照：白名单不能收得过紧，否则用户文档里的
/// 相对路径图片 / 附件点不开。
#[test]
fn local_targets_are_opened() {
    let (_dir, mut app, recorder) = app_with_recorder();

    for target in ["file:///C:/x.png", "./foo.png", "relative/path.md"] {
        assert!(is_safe_to_open(target), "用例前提：{target} 应判为安全");
        let content = format!("[a]({target})");
        let sv = link_span(0, 1, target);
        handle_span_click(&mut app, &sv, &content, &[]);
    }

    assert_eq!(
        recorder.calls(),
        vec![
            "file:///C:/x.png".to_string(),
            "./foo.png".to_string(),
            "relative/path.md".to_string(),
        ],
        "本地文件与相对路径应逐一交给打开器"
    );
}
