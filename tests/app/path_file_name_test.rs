//! 路径末段提取的收口（V11-54a / R-15）。
//!
//! # 背景：这不是「代码洁癖」，是用户可见的 bug
//!
//! 全库曾有 **9 处** `path.rsplit('/').next()` 取文件名，其中 **3 处**
//! （app 层的对话框标题、删除确认、打开失败提示）**只认 `/`**。
//! Windows 的原生对话框 / 拖放事件给的是**反斜杠**路径
//! （`C:\Users\me\a.md`）→ 那些提示里显示的是**整条路径**：
//!
//! ```text
//! 确定删除「C:\Users\me\文档\重要.md」？删除后不可恢复。
//! ```
//!
//! 用户看到的是一串路径噪音，而不是「哪个文件」。
//!
//! # 收口方式
//!
//! 唯一实现 = [`rustmark::shared::path::file_name_of`]（同时认 `/` 与 `\`），
//! 9 处调用点全部改走它。
//!
//! # 为什么不直接用 `FilePath::file_name`
//!
//! `FilePath` 是**值对象**：构造即校验（拒绝空串、拒绝 `..`）并归一化。
//! 而这里要处理的是**原始路径字符串**——含 `..` 的合法路径会被
//! `FilePath::new` 判成 `Err`，若写成 `FilePath::new(path).ok()` 就会
//! **退化回"显示整条路径"**，比现在更糟。故 `file_name_of` 只做取末段
//! 这一件事，不做任何校验。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::dialog_runner::DialogOutcome;
use rustmark::app::frame::EditorApp;
use rustmark::shared::path::FilePath;
use rustmark::shared::path::file_name_of;

use crate::support::fake_dialog::FakeDialog;

fn app() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

fn status_text(app: &EditorApp) -> String {
    app.status_bar
        .message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

// ============================================================================
// 纯函数层
// ============================================================================

/// 反斜杠路径必须取到文件名，而不是整条路径。
///
/// 这条是**核心判据**：改坏它（退回只认 `/`）→ 用例立刻红。
#[test]
fn file_name_extraction_handles_windows_backslashes() {
    assert_eq!(file_name_of("C:\\a\\b.md"), "b.md", "反斜杠也是分隔符");
    assert_eq!(file_name_of("C:\\a\\b.md"), "b.md");
}

/// 两种分隔符混用（Windows 路径里偶尔会出现）。
#[test]
fn file_name_extraction_handles_mixed_separators() {
    assert_eq!(file_name_of("C:/a\\b/c.md"), "c.md");
}

/// 无分隔符时原样返回。
#[test]
fn file_name_extraction_returns_the_input_when_there_is_no_separator() {
    assert_eq!(file_name_of("b.md"), "b.md");
    assert_eq!(file_name_of(""), "");
}

/// 末尾是分隔符时取到空串（调用方按需处理，不在这里编造）。
#[test]
fn file_name_extraction_yields_empty_for_a_trailing_separator() {
    assert_eq!(file_name_of("a/b/"), "");
}

/// `FilePath::file_name` 与 `file_name_of` 对同一输入必须一致
/// （两处规则不许分家）。
#[test]
fn the_value_object_and_the_free_function_agree() {
    for raw in ["a/b.md", "C:/x/y.md", "/root/z.md", "single.md"] {
        let object = FilePath::new(raw).expect("应可构造");
        assert_eq!(
            object.file_name(),
            file_name_of(raw),
            "两处取末段的规则必须一致：{raw}"
        );
    }
}

// ============================================================================
// 可见效果层（走真实 DialogOutcome 路径）
// ============================================================================

/// 另存为成功后的提示只显示**文件名**，不显示整条路径（V11-54a / R-15）。
///
/// 这是**用户可见**的那一处：原先 Windows 上会显示
/// 「已保存到 C:\Users\me\文档\笔记.md」。
///
/// 目标路径必须落在**真实可写**的临时目录里——否则保存会失败，
/// 走的是错误分支，测的就不是「成功后提示显示什么」了。
#[test]
fn dialog_title_shows_only_the_file_name_on_windows_paths() {
    let (dir, mut app) = app();

    // 先造一个可保存的文档。
    app.new_document().expect("新建应成功");
    app.insert_at_cursor("内容").expect("插入应成功");

    // 走**真实的** `DialogOutcome` 路径。目标名里带**目录段**，
    // 是本用例要验的关键：旧实现只认 `/`，会把整条路径显示出来。
    let target_path = dir.path().join("文档").join("笔记.md");
    std::fs::create_dir_all(target_path.parent().unwrap()).expect("应可建目录");
    let target = FilePath::new(target_path.to_string_lossy().as_ref()).expect("应可构造");

    app.apply_dialog_outcome(DialogOutcome::SaveFile(Ok(target.clone())));

    let text = status_text(&app);
    assert!(
        text.contains("已保存到 笔记.md"),
        "提示应只显示文件名，实际：{text:?}"
    );
    assert!(!text.contains("文档"), "提示不应包含目录段，实际：{text:?}");
    assert!(
        !text.contains('\\'),
        "提示不应包含反斜杠路径片段，实际：{text:?}"
    );

    // 落盘目标确实用了整条路径（提示简化不影响保存位置）。
    assert!(
        target.as_str().ends_with("笔记.md"),
        "保存目标应保留完整路径"
    );
    assert!(
        target_path.exists(),
        "文件应真的落盘在完整路径上：{}",
        target_path.display()
    );
}

/// 「无法打开」的错误提示只显示文件名。
///
/// 注意断言范围：只查**外层**提示（`无法打开 <文件名>：…`）。
/// 内层是 `AppError` 的 IO 消息（含完整路径），那是错误值自身的职责，
/// 不在本项范围内——把两件事混在一起断言会让本用例因无关原因变红。
#[test]
fn the_open_failure_notice_shows_only_the_file_name() {
    let (_dir, mut app) = app();
    // 一个不存在的 Windows 路径。
    let missing = "C:\\Users\\me\\文档\\缺失.md";
    let result = app.open_path(missing);
    assert!(result.is_err(), "打开不存在的文件应报错");

    let text = status_text(&app);
    assert!(
        text.starts_with("无法打开 缺失.md："),
        "错误提示应以「无法打开 <文件名>：」开头，实际：{text:?}"
    );
    // 外层提示本身不得含路径分隔符（内层 IO 消息允许含，那是错误详情）。
    let outer = text.split("：").next().unwrap_or_default();
    assert!(
        !outer.contains('/') && !outer.contains('\\'),
        "提示的外层部分不应含路径分隔符，实际：{outer:?}"
    );
}

/// 「暂不支持打开该格式」的提示只显示文件名。
#[test]
fn the_unsupported_format_notice_shows_only_the_file_name() {
    let notice = EditorApp::unsupported_format_notice("C:\\Users\\me\\图片\\照片.png");
    assert_eq!(
        notice, "暂不支持打开该格式：照片.png",
        "提示语应只说文件末段，不含整条路径（状态栏放不下）"
    );
}
