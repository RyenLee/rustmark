//! S?-P2 拖拽打开文件：headless 驱动真实 `EditorApp::update` 帧循环，
//! 验证「把 .md 文件拖入窗口 → 自动打开」端到端可用。
//!
//! 真实拖放事件是 OS 级 winit 事件，headless 无法触发；但 egui 把拖放结果
//! 归一为 `RawInput.dropped_files`，我们只需在该字段填上路径即可模拟，
//! 与 `crash_recovery_test.rs` 用 `Frame::_new_kittest()` 驱动真实 `update` 同源。

use std::path::{Path, PathBuf};

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;

use crate::support::fake_dialog::FakeDialog;

use eframe::App;

use std::sync::Arc;

use tempfile::tempdir;

/// egui 0.37 起 `DroppedFile` 是 trait，测试需提供一个实现来模拟拖放结果。
#[derive(Debug)]
struct TestDroppedFile {
    path: PathBuf,
}

impl egui::DroppedFile for TestDroppedFile {
    fn path(&self) -> &Path {
        &self.path
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.path).map_err(|e| e.to_string())
    }
}

fn frame_input_with_drop(paths: &[PathBuf]) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        dropped_files: paths
            .iter()
            .map(|p| -> egui::DroppedFileHandle { Arc::new(TestDroppedFile { path: p.clone() }) })
            .collect(),
        ..Default::default()
    }
}

/// 拖入一个 .md 文件 → 应用打开它（活动文档路径匹配）。
#[test]
fn drag_drop_markdown_opens_document() {
    let dir = tempdir().expect("临时目录应创建成功");
    let md = dir.path().join("拖入的笔记.md");
    std::fs::write(&md, "# 拖入的内容\n\nhello world\n").expect("应能写入临时 md");

    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);

    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    let paths = vec![md.clone()];
    crate::support::run_ui(&ctx, frame_input_with_drop(&paths), |ui| {
        app.ui(ui, &mut frame);
    });

    // 活动文档路径应等于拖入文件（取文件名比对，路径分隔符跨平台一致）。
    let opened = app.current_document_path();
    assert!(
        opened.contains("拖入的笔记.md"),
        "拖入的 .md 应被打开，实际活动文档路径 = {opened:?}"
    );
    assert!(
        !app.open_document_list().is_empty(),
        "打开后应至少有一个标签页"
    );
}

/// 拖入非文本文件（png）→ 不打开，且仍有「拒绝」提示（不 panic）。
#[test]
fn drag_drop_non_text_is_rejected() {
    let dir = tempdir().expect("临时目录应创建成功");
    let png = dir.path().join("photo.png");
    std::fs::write(&png, b"\x89PNG binary junk").expect("应能写入临时 png");

    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);

    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    let paths = vec![png.clone()];
    crate::support::run_ui(&ctx, frame_input_with_drop(&paths), |ui| {
        app.ui(ui, &mut frame);
    });

    // 没有任何文档被打开（本就从空工作区启动，只有未命名文档的概念）。
    assert!(app.open_document_list().is_empty(), "非文本文件不应被打开");
    // 且必须说明原因：静默忽略会让用户以为「拖放功能坏了」。
    // 文案与文件树置灰行的悬停说明同源（问题 045）。
    let notice = app
        .status_bar
        .message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default();
    assert!(
        notice.contains("暂不支持打开该格式"),
        "拖入图片应给出一条提示，实际状态栏 = {notice:?}"
    );
}

/// 一次拖入多个文件：可识别的（md / txt / csv）全部打开，非文本被忽略。
#[test]
fn drag_drop_mixed_batch_opens_only_text() {
    let dir = tempdir().expect("临时目录应创建成功");
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.txt");
    let c = dir.path().join("c.pdf");
    let d = dir.path().join("d.csv");
    std::fs::write(&a, "# A").expect("写 a");
    std::fs::write(&b, "plain").expect("写 b");
    std::fs::write(&c, b"%PDF-1.4 junk").expect("写 c");
    std::fs::write(&d, "x,y\n1,2\n").expect("写 d");

    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);

    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    let paths = vec![a.clone(), b.clone(), c.clone(), d.clone()];
    crate::support::run_ui(&ctx, frame_input_with_drop(&paths), |ui| {
        app.ui(ui, &mut frame);
    });

    // .md / .txt / .csv 都应打开 → 3 个标签；.pdf 被忽略。
    // `.csv` 是问题 045 的回归点：它进了白名单，拖放与文件树必须同时放行。
    assert_eq!(
        app.open_document_list().len(),
        3,
        "批量拖入应打开 .md / .txt / .csv，忽略 .pdf"
    );
}

/// 拖入目录（非文件）→ 不打开、不报错（目录名带 `.md` 也不行）。
///
/// 回归背景：目录曾只按扩展名分类，`notes.md/` 这类目录会被当成文本
/// 交给 `open_path`，最终以「无法读取」报错——一次误拖换一条误导性错误。
#[test]
fn drag_drop_directory_is_ignored_without_error() {
    let dir = tempdir().expect("临时目录应创建成功");
    let notes_dir = dir.path().join("notes.md"); // 目录名刻意带 .md 后缀
    std::fs::create_dir(&notes_dir).expect("应能创建子目录");
    let md = dir.path().join("真文件.md");
    std::fs::write(&md, "# 与目录一起拖入\n").expect("写 md");

    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);

    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    let paths = vec![notes_dir.clone(), md.clone()];
    crate::support::run_ui(&ctx, frame_input_with_drop(&paths), |ui| {
        app.ui(ui, &mut frame);
    });

    // 只有那个真 .md 被打开；目录既不进标签页，也不产生红色错误。
    assert_eq!(app.open_document_list().len(), 1, "目录不应被当作文件打开");
    let opened = app.current_document_path();
    assert!(
        opened.contains("真文件.md"),
        "打开的应是文本文件而非目录，实际 = {opened:?}"
    );
}
