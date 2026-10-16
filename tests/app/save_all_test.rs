//! 「保存全部」（问题 038）。
//!
//! # 缺陷形态
//!
//! `UiAction::SaveAll` 在动作表、「文件」菜单、快捷键（`Ctrl+Alt+S`）
//! 三处都有，但 `EditorApp::execute` 里**没有对应分支**——点「保存全部」
//! 落进 `_ => {}` 兜底，毫无反应。与当年 `Ctrl+S` / `Ctrl+N` 静默失效
//! 是同一条根因：三入口（菜单 / 快捷键 / 命令面板）汇聚点漏接线。
//!
//! 本文件钉住四件事：
//! 1. 经 `execute` 触发（= 三个入口的共同路径）后，**所有**脏文档落盘；
//! 2. 未命名文档不失败、不弹框，单独计数提示；
//! 3. 没有脏文档时明确提示，而不是静默；
//! 4. 某个文档写盘失败不中断其余文档（逐个捕获、汇总报告）。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::InsertTextCommand;
use rustmark::application::document::queries::GetDocumentQuery;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::shared::ids::DocumentId;

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

/// 打开一份磁盘文件并返回其文档 ID。
fn open_file(app: &mut EditorApp, dir: &std::path::Path, name: &str, body: &str) -> DocumentId {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("写文件应成功");
    app.open_path(&path.to_string_lossy()).expect("打开应成功");
    app.ensure_document().expect("打开后应有活动文档")
}

/// 把某个文档改脏（走领域命令，与编辑器按键同一条路径）。
fn dirty(app: &mut EditorApp, id: DocumentId, text: &str) {
    app.services
        .documents
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: text.to_string(),
        })
        .expect("插入应成功");
}

/// 某个文档当前是否脏。
fn is_dirty(app: &EditorApp, id: DocumentId) -> bool {
    app.services
        .documents
        .get_document(GetDocumentQuery { id })
        .expect("文档应仍在")
        .is_dirty
}

/// 状态栏当前文案（无消息时空串）。
fn status_text(app: &EditorApp) -> String {
    app.status_bar
        .message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

/// 主路径：两个脏文档经 `execute(SaveAll)` 全部落盘、脏标记清除。
#[test]
fn save_all_persists_every_dirty_document() {
    let (dir, mut app) = app();
    let a = open_file(&mut app, dir.path(), "a.md", "alpha\n");
    let b = open_file(&mut app, dir.path(), "b.md", "beta\n");
    dirty(&mut app, a, "A");
    dirty(&mut app, b, "B");

    app.execute(UiAction::SaveAll);

    assert!(!is_dirty(&app, a), "a.md 保存后应清掉脏标记");
    assert!(!is_dirty(&app, b), "b.md 保存后应清掉脏标记");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).expect("应可读"),
        "Aalpha\n",
        "改动必须真的写到磁盘"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.md")).expect("应可读"),
        "Bbeta\n"
    );
    assert!(
        status_text(&app).contains("已保存 2 个文档"),
        "应报告保存数量，实际 {:?}",
        status_text(&app)
    );
}

/// 未命名文档：不失败、不弹框，单独计数提示。
///
/// 批量保存不可能连着弹 N 个「另存为」对话框；用户缺的是路径，
/// 提示里说清「需先另存为」即可，不该以「保存失败」的面貌出现。
#[test]
fn save_all_counts_unnamed_documents_without_failing() {
    let (dir, mut app) = app();
    let named = open_file(&mut app, dir.path(), "named.md", "x\n");
    app.execute(UiAction::NewDocument);
    let unnamed = app.ensure_document().expect("新建后应有活动文档");
    dirty(&mut app, named, "N");
    dirty(&mut app, unnamed, "U");

    app.execute(UiAction::SaveAll);

    assert!(!is_dirty(&app, named), "有路径的文档应被保存");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("named.md")).expect("应可读"),
        "Nx\n"
    );
    assert!(
        is_dirty(&app, unnamed),
        "未命名文档存不下来，脏标记必须保留（否则用户以为存过了）"
    );
    let text = status_text(&app);
    assert!(
        text.contains("已保存 1 个文档") && text.contains("1 个未命名文档需先另存为"),
        "提示应同时报出已保存数与未命名数，实际 {text:?}"
    );
}

/// 没有脏文档时明确提示——静默会让用户怀疑「菜单是不是坏了」。
#[test]
fn save_all_with_nothing_dirty_says_so() {
    let (dir, mut app) = app();
    open_file(&mut app, dir.path(), "clean.md", "nothing to do\n");

    app.execute(UiAction::SaveAll);

    assert!(
        status_text(&app).contains("没有未保存的文档"),
        "无脏文档应给明确提示，实际 {:?}",
        status_text(&app)
    );
    assert!(
        !app.status_bar.message.as_ref().is_some_and(|m| m.is_error),
        "无脏文档不是错误"
    );
}

/// 某个文档写盘失败不中断其余文档，且失败被汇总报告。
///
/// 失败路径的构造：把 `bad.md` **换成同名目录**——原子写的最后一步是
/// rename（临时文件 → 目标路径），「目标是目录」时必然失败。
/// （不能用「删掉父目录」构造：`atomic_write` 会先把父目录建回来，
/// 实测那样得到的是成功保存。）
#[test]
fn save_all_keeps_going_when_one_document_fails() {
    let (dir, mut app) = app();
    let good = open_file(&mut app, dir.path(), "good.md", "g\n");
    let bad_dir = dir.path().join("doomed");
    std::fs::create_dir(&bad_dir).expect("应可建目录");
    let bad = open_file(&mut app, &bad_dir, "bad.md", "b\n");
    dirty(&mut app, good, "G");
    dirty(&mut app, bad, "B");
    // 让 bad.md 的路径变成一个目录：rename 覆盖目录必然失败。
    std::fs::remove_file(bad_dir.join("bad.md")).expect("删除文件应成功");
    std::fs::create_dir(bad_dir.join("bad.md")).expect("建同名目录应成功");

    app.execute(UiAction::SaveAll);

    assert!(!is_dirty(&app, good), "好文档必须照常保存（失败不中断）");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("good.md")).expect("应可读"),
        "Gg\n"
    );
    assert!(is_dirty(&app, bad), "写盘失败的文档必须保持脏标记");
    let text = status_text(&app);
    assert!(
        text.contains("bad.md") && text.contains("保存失败"),
        "失败必须汇总报告，实际 {text:?}"
    );
}

/// 单个保存过的文档不会在「保存全部」里充数（幂等，且提示说清现状）。
#[test]
fn save_all_after_single_save_reports_nothing_left() {
    let (dir, mut app) = app();
    let a = open_file(&mut app, dir.path(), "a.md", "alpha\n");
    dirty(&mut app, a, "A");

    app.execute(UiAction::Save); // 先单独存当前文档
    assert!(!is_dirty(&app, a));

    app.execute(UiAction::SaveAll);
    assert!(
        status_text(&app).contains("没有未保存的文档"),
        "单个保存后不应再有脏文档，实际 {:?}",
        status_text(&app)
    );
}
