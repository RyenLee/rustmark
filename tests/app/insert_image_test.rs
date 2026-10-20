//! 「插入图片」（R-04b）：选图 → 复制进资源目录 → 光标处插相对链接。
//!
//! 契约（2026-10-20 与用户确认）：选图后**复制**到 `<文档目录>/assets/`
//! （重名去重、不覆盖），Markdown 里插**相对**链接。本文件钉住：
//! 1. 快乐路径：复制发生、字节一致、链接形如 `![](assets/photo.png)`；
//! 2. 取消 = 什么都不做（不插、不复制、不报错）；
//! 3. 重名去重：第二次插入同一个名字得到 `-1` 后缀；
//! 4. 未命名文档没有资源目录 → 插绝对路径、不复制；
//! 5. 导入失败 → 报错且不插入（静默是最坏结果）。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::queries::GetContentQuery;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::shared::error::AppError;
use rustmark::shared::ids::DocumentId;
use rustmark::shared::image_asset::{ImageAssetStore, ImportedImage};
use rustmark::shared::path::FilePath;
use rustmark::shared::result::AppResult;
use rustmark::shared::system::ExternalOpener;
use rustmark::shared::time::SystemClock;

use crate::support::fake_dialog::FakeDialog;

/// 总是失败的假导入器（第 5 条：导入失败必须可见）。
struct FailingStore;

impl ImageAssetStore for FailingStore {
    fn import_image(
        &self,
        _source: &FilePath,
        _doc_dir: &std::path::Path,
    ) -> AppResult<ImportedImage> {
        Err(AppError::io("模拟磁盘已满"))
    }
}

/// 收我一个后台对话框结果并应用（等价于 `update` 里的那两步）。
fn drain(app: &mut EditorApp) -> bool {
    for _ in 0..200 {
        if let Some(outcome) = app.dialogs.poll() {
            app.apply_dialog_outcome(outcome);
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

fn content_of(app: &EditorApp, id: DocumentId) -> String {
    app.services
        .documents
        .get_content(GetContentQuery { id, range: None })
        .expect("应可读取内容")
}

/// 打开一份磁盘文档（有路径 → 有资源目录），返回文档 id。
fn open_disk_document(
    app: &mut EditorApp,
    ws: &std::path::Path,
    name: &str,
    body: &str,
) -> DocumentId {
    let path = ws.join(name);
    std::fs::write(&path, body).expect("写文件应成功");
    app.open_path(&path.to_string_lossy()).expect("打开应成功");
    app.ensure_document().expect("应有活动文档")
}

/// 快乐路径：复制进 `assets/`、字节一致、插相对链接。
#[test]
fn insert_image_copies_into_assets_and_links_relatively() {
    let ws = tempfile::tempdir().expect("文档工作区");
    let src_dir = tempfile::tempdir().expect("图片来源目录");
    let src = src_dir.path().join("photo.png");
    std::fs::write(&src, b"\x89PNG-fake-bytes").expect("写源图");

    let fake = FakeDialog::picking_image(&src.to_string_lossy());
    let dir = tempfile::tempdir().expect("配置目录");
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake.clone()))
            .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "# 标题\n");

    app.execute(UiAction::InsertImage);
    assert!(drain(&mut app), "选图对话框应有答复");
    assert_eq!(fake.image_calls(), 1, "应弹一次选图对话框");

    // 链接：相对、正斜杠、空 alt。
    assert_eq!(
        content_of(&app, id),
        "![](assets/photo.png)# 标题\n",
        "图片应插在光标处（无选区时光标在文档头）"
    );
    // 复制：落点存在且字节一致；原文件不动。
    let copied = ws.path().join("assets/photo.png");
    assert_eq!(
        std::fs::read(&copied).expect("资源应已复制"),
        b"\x89PNG-fake-bytes",
        "复制进资源目录的字节必须与源一致"
    );
    assert!(src.exists(), "源文件不该被移动");
}

/// 取消 = 什么都不做。
#[test]
fn cancelling_the_picker_inserts_nothing() {
    let ws = tempfile::tempdir().expect("文档工作区");
    let fake = FakeDialog::new(); // 不预置回应 = 取消
    let dir = tempfile::tempdir().expect("配置目录");
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake.clone()))
            .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "原文\n");

    app.execute(UiAction::InsertImage);
    assert!(drain(&mut app), "对话框应有答复（取消也是答复）");

    assert_eq!(content_of(&app, id), "原文\n", "取消后内容必须原样");
    assert!(!ws.path().join("assets").exists(), "取消不该创建资源目录");
    assert_eq!(fake.image_calls(), 1, "应弹了一次选图对话框");
}

/// 重名去重：第二次插入同一个名字得到 `-1` 后缀，且不覆盖。
#[test]
fn second_insert_of_same_name_dedupes() {
    let ws = tempfile::tempdir().expect("文档工作区");
    let src_dir = tempfile::tempdir().expect("图片来源目录");
    let src = src_dir.path().join("photo.png");
    std::fs::write(&src, b"second-bytes").expect("写源图");
    std::fs::create_dir_all(ws.path().join("assets")).expect("建资源目录");
    std::fs::write(ws.path().join("assets/photo.png"), b"first-bytes").expect("预置同名文件");

    let fake = FakeDialog::picking_image(&src.to_string_lossy());
    let dir = tempfile::tempdir().expect("配置目录");
    let services = Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake))
        .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "正文\n");

    app.execute(UiAction::InsertImage);
    assert!(drain(&mut app));

    assert_eq!(content_of(&app, id), "![](assets/photo-1.png)正文\n");
    assert_eq!(
        std::fs::read(ws.path().join("assets/photo.png")).expect("读旧文件"),
        b"first-bytes",
        "已存在的同名文件绝不被覆盖"
    );
    assert_eq!(
        std::fs::read(ws.path().join("assets/photo-1.png")).expect("读新文件"),
        b"second-bytes"
    );
}

/// 未命名文档没有文档目录：插**绝对**路径、不复制。
#[test]
fn insert_into_unsaved_document_uses_absolute_path() {
    let src_dir = tempfile::tempdir().expect("图片来源目录");
    let src = src_dir.path().join("photo.png");
    std::fs::write(&src, b"bytes").expect("写源图");

    let fake = FakeDialog::picking_image(&src.to_string_lossy());
    let dir = tempfile::tempdir().expect("配置目录");
    let services = Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake))
        .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    app.new_document().expect("新建应成功");
    let id = app.ensure_document().expect("应有活动文档");

    app.execute(UiAction::InsertImage);
    assert!(drain(&mut app));

    // 绝对路径（`FilePath` 归一化为 `/` 分隔，可直接嵌进 Markdown）。
    let expected = FilePath::new(src.to_string_lossy().as_ref()).expect("合法路径");
    assert_eq!(
        content_of(&app, id),
        format!("![]({})", expected.as_str()),
        "未命名文档应插绝对路径"
    );
    assert!(src.exists(), "源文件不该被复制或移动");
    assert!(
        !src_dir.path().join("assets").exists(),
        "没有文档目录就不该建资源目录"
    );
}

/// 导入失败：报错且不插入（静默会让用户以为插成功了）。
#[test]
fn import_failure_is_reported_and_nothing_is_inserted() {
    let ws = tempfile::tempdir().expect("文档工作区");
    let src_dir = tempfile::tempdir().expect("图片来源目录");
    let src = src_dir.path().join("photo.png");
    std::fs::write(&src, b"bytes").expect("写源图");

    let fake = FakeDialog::picking_image(&src.to_string_lossy());
    let dir = tempfile::tempdir().expect("配置目录");
    let services = Services::assemble_with_image_store(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(SystemClock),
        Arc::new(fake),
        Arc::new(NoopOpener),
        Arc::new(FailingStore),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "正文\n");

    app.execute(UiAction::InsertImage);
    assert!(drain(&mut app));

    assert_eq!(content_of(&app, id), "正文\n", "导入失败不该留下半截链接");
    assert!(!ws.path().join("assets").exists(), "导入失败不该建资源目录");
    let message = app.status_bar.message.as_ref().expect("应有一条错误");
    assert!(message.is_error, "导入失败必须报错，不能静默");
    assert!(
        message.text.contains("导入图片失败"),
        "错误应说明是导入失败，实际 {}",
        message.text
    );
}

/// 从不被调用的打开器（本测试不触发外链打开）。
struct NoopOpener;

impl ExternalOpener for NoopOpener {
    fn open(&self, _target: &str) -> std::io::Result<()> {
        Ok(())
    }
}
