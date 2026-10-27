//! 预览图片的字节取用器（`CommandPort::preview_image_fetcher`）。
//!
//! # 为什么这条契约必须有测试
//!
//! R-03 B1 批 2 把 `image_cache::image_entry` 的形参从 `&EditorApp` 换成
//! `&dyn EditorViewContext` 时，唯一挡住它的是三样「ui crate 名不到」的东西：
//! `AssetLoader`（住 infra）、文档目录（组合根 `pub(crate)`）、以及「远程不传
//! 基准目录」这条装配知识。三者被封进
//! [`ImageFetcher`](rustmark::presentation::view_context::ImageFetcher) 返回的闭包。
//!
//! 这段装配**没有类型系统帮忙**：`base_dir` 传错（比如传 `None`、或误传进程
//! cwd）、`remote` 判据写错、闭包不可 `Send`，全都能编译通过。故这里逐条钉住：
//!
//! 1. 绝对路径 → 逐字节读到内容，**且闭包能搬进后台线程**（F-3 的硬要求）；
//! 2. 读不到时返回 `Err` 而不是 panic（`panic = "abort"` 下 panic 等于退进程）；
//! 3. 相对路径按**当前文档所在目录**解析——这条是 root 侧装配知识，
//!    退化成「相对进程 cwd」时第 3 条用例会红；
//! 4. 不支持的协议走错误分支，而不是被当成相对路径去读盘。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::view_context::CommandPort;

use crate::support::fake_dialog::FakeDialog;

fn app_in(dir: &std::path::Path) -> EditorApp {
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir), Arc::new(FakeDialog::new()))
            .expect("装配应成功");
    EditorApp::new(services, false)
}

/// 绝对路径：读到原始字节，并且闭包可以交给后台线程。
///
/// `std::thread::spawn` 要求 `F: Send + 'static`——**这一行本身就是断言**：
/// 哪天有人把 `ImageFetcher` 改成借用上下文（`Box<dyn FnOnce() + '_>`）
/// 或去掉 `+ Send`，本用例直接编译不过。这不是多余的严格：远程图必须在
/// 后台线程里下载（F-3），同步取字节会把 UI 线程按到响应或超时。
#[test]
fn local_bytes_can_be_fetched_from_a_background_thread() {
    let dir = tempfile::tempdir().expect("工作区");
    let app = app_in(dir.path());
    let img = dir.path().join("pic.png");
    std::fs::write(&img, b"\x89PNG-fake-body").expect("写图应成功");

    // 直接把 boxed 闭包交给 spawn（不套一层 `move || fetch()`）：这要求
    // `Box<dyn FnOnce() -> … + Send + 'static>` **自身**满足 `FnOnce + Send +
    // 'static`，比套一层更严格——套一层时是外层空闭包在满足 bound，
    // 内层借用不合法也照样编译通过（clippy 的 redundant_closure 恰好指出这点）。
    let fetch = app.preview_image_fetcher(&img.to_string_lossy());
    let bytes = std::thread::spawn(fetch)
        .join()
        .expect("后台线程不应 panic")
        .expect("应读到字节");

    assert_eq!(
        bytes, b"\x89PNG-fake-body",
        "闭包应返回文件**原始字节**（解码是视图侧的事，不在契约里）"
    );
}

/// 读不到时给出错误，不 panic——占位文案要用这条消息。
#[test]
fn unreadable_file_reports_an_error_instead_of_panicking() {
    let dir = tempfile::tempdir().expect("工作区");
    let app = app_in(dir.path());
    let missing = dir.path().join("nope.png");

    let err = app.preview_image_fetcher(&missing.to_string_lossy())()
        .expect_err("不存在的文件应返回 Err");
    assert!(
        !err.is_empty(),
        "错误信息不应为空——预览区要把它显示成占位原因"
    );
}

/// 相对路径按**当前文档目录**解析，不是按进程 cwd。
///
/// 判别手法：打开文档**之前**同一条相对路径必须失败（错误里说明需要文档目录），
/// 打开之后必须成功。若 `base_dir` 被误连成进程 cwd，前半段就会直接读到文件、
/// `expect_err` 当场变红。
#[test]
fn relative_path_resolves_against_the_document_dir() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    let doc = dir.path().join("note.md");
    std::fs::write(&doc, "# 标题\n").expect("写文档应成功");
    std::fs::create_dir_all(dir.path().join("assets")).expect("建资源目录应成功");
    std::fs::write(dir.path().join("assets").join("pic.png"), b"png-bytes").expect("写图应成功");

    let err =
        app.preview_image_fetcher("assets/pic.png")().expect_err("未打开文档时相对路径无法解析");
    assert!(
        err.contains("相对路径"),
        "错误应说明「相对路径需要先打开文档」，实际：{err}"
    );

    app.open_path(&doc.to_string_lossy()).expect("打开应成功");
    let bytes = app.preview_image_fetcher("assets/pic.png")()
        .expect("打开文档后应能按文档目录读到相对路径");
    assert_eq!(bytes, b"png-bytes");
}

/// 不支持的协议走错误分支，**不得**被当成相对路径去读盘。
///
/// `x://…` 若落进本地分支，`PathBuf::from` 会把它当相对名去 join，
/// 得到一个永远不存在的路径——错误信息会误导成「文件不存在」，
/// 而真实原因是协议不支持。
#[test]
fn unsupported_scheme_is_reported_as_such() {
    let dir = tempfile::tempdir().expect("工作区");
    let app = app_in(dir.path());

    let err =
        app.preview_image_fetcher("ftp://example.com/a.png")().expect_err("不支持的协议应返回 Err");
    assert!(err.contains("协议"), "错误应点名协议不受支持，实际：{err}");
}
