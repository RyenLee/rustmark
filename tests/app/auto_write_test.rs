//! 「自动保存写回原文件」的端到端回归（`frame::auto_write`）。
//!
//! 走**真实** `Services` 装配 + 真实临时文件，不替身化仓储：本题的核心
//! 是「有没有覆盖磁盘、有没有覆盖别人的改动」，用内存仓储测等于没测。
//!
//! 覆盖五件事：
//! 1. 开关默认关闭、且经 DTO → 领域 → DTO 往返不丢值；
//! 2. 开启后到期时把当前活动文档写回原文件、清脏标记与草稿；
//! 3. 只对活动文档写回，其余到期文档仍写草稿；
//! 4. 磁盘被外部改动时跳过写回、提示一次、仅存草稿；手动保存后解锁；
//! 5. 自动保存关闭时，开关不产生任何写盘。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::InsertTextCommand;
use rustmark::application::document::queries::GetDocumentQuery;
use rustmark::application::dto::EditorSettingsView;
use rustmark::shared::ids::DocumentId;

use crate::support::clock::TestClock;
use crate::support::fake_dialog::FakeDialog;

/// 可控时钟的应用：时钟不前进，到期与否就完全由用例决定。
fn app_with_clock() -> (tempfile::TempDir, EditorApp, Arc<TestClock>) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let clock = Arc::new(TestClock::default());
    let services = Services::assemble_with_clock_and_dialogs(
        ConfigPaths::in_dir(dir.path()),
        clock.clone(),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false), clock)
}

/// 打开一份磁盘文件，返回 (文档 ID, 磁盘路径)。
fn open_file(app: &mut EditorApp, dir: &std::path::Path, name: &str, body: &str) -> DocumentId {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("写文件应成功");
    app.open_path(&path.to_string_lossy()).expect("打开应成功");
    app.ensure_document().expect("打开后应有活动文档")
}

/// 配置自动保存（间隔 + 写回开关）。
///
/// 走设置页的真实写入口 `apply_editor_field`，而不是直接调仓储：
/// 开关从关→开时补播种磁盘基线就挂在那条路径上，绕过去会让「外部改动」
/// 用例退化成「没有基线」，测不出保护是否有效。
fn configure(app: &mut EditorApp, interval: Option<u32>, write_back: bool) {
    app.apply_editor_field(|editor: &mut EditorSettingsView| {
        editor.auto_save_interval = interval;
        editor.auto_save_to_source = write_back;
    });
}

/// 改脏文档：走领域命令，与编辑器按键同一条事件路径（`TextModified`）。
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

fn is_dirty(app: &EditorApp, id: DocumentId) -> bool {
    app.services
        .documents
        .get_document(GetDocumentQuery { id })
        .expect("文档应仍在")
        .is_dirty
}

fn disk_text(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).expect("磁盘文件应可读")
}

fn draft_path(app: &EditorApp, id: DocumentId) -> std::path::PathBuf {
    app.services.drafts_dir.join(format!("{id}.draft"))
}

fn status_text(app: &EditorApp) -> String {
    app.status_bar
        .message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

/// 1 秒间隔 + 让时钟推进到「停手满间隔」，随后驱动一次真实节拍。
fn tick(app: &mut EditorApp, clock: &TestClock) {
    clock.advance(1_001);
    app.run_autosave();
}

/// 开关默认关闭；开启值要能经 `set_editor` → 领域 → `get_settings` 往返。
#[test]
fn write_back_is_off_by_default_and_round_trips_through_preferences() {
    let (_dir, mut app, _clock) = app_with_clock();
    assert!(
        !app.settings().editor.auto_save_to_source,
        "默认必须关闭：升级不该静默改变「自动保存只写草稿」的行为"
    );

    configure(&mut app, Some(30), true);
    assert!(
        app.settings().editor.auto_save_to_source,
        "开启值必须经 DTO → 领域 → DTO 往返（漏一半就是「勾了没生效」）"
    );

    configure(&mut app, Some(30), false);
    assert!(!app.settings().editor.auto_save_to_source);
}

/// 开启后：到期节拍把活动文档写回原文件，清掉脏标记与草稿。
#[test]
fn write_back_persists_active_document_and_clears_dirty_flag() {
    let (dir, mut app, clock) = app_with_clock();
    let path = dir.path().join("a.md");
    let id = open_file(&mut app, dir.path(), "a.md", "alpha\n");
    configure(&mut app, Some(1), true);

    dirty(&mut app, id, "EDIT ");
    tick(&mut app, &clock);

    assert_eq!(
        disk_text(&path),
        "EDIT alpha\n",
        "到期后原文件应被写回（自动保存从草稿升级为落盘）"
    );
    assert!(!is_dirty(&app, id), "写回成功后脏标记应清除");
    assert!(
        !draft_path(&app, id).exists(),
        "写回成功后草稿必须清掉，否则下次启动误报「待恢复」"
    );
}

/// 只对**当前活动文档**写回；另一个到期文档仍按 ADR-003 写草稿。
#[test]
fn write_back_only_touches_the_active_tab() {
    let (dir, mut app, clock) = app_with_clock();
    let path_a = dir.path().join("a.md");
    let path_b = dir.path().join("b.md");
    let a = open_file(&mut app, dir.path(), "a.md", "alpha\n");
    let b = open_file(&mut app, dir.path(), "b.md", "beta\n");
    configure(&mut app, Some(1), true);

    dirty(&mut app, a, "A ");
    dirty(&mut app, b, "B ");
    tick(&mut app, &clock);

    assert_eq!(disk_text(&path_b), "B beta\n", "活动文档应被写回");
    assert_eq!(
        disk_text(&path_a),
        "alpha\n",
        "非活动文档的原文件不应被无人值守地覆盖"
    );
    assert!(
        draft_path(&app, a).exists(),
        "非活动文档的修改必须落到草稿，不能凭空丢失"
    );
}

/// 磁盘被外部改动时：跳过写回、状态栏提示一次、仅存草稿；
/// 用户手动 Ctrl+S 显式处置后，自动写回应恢复工作。
#[test]
fn external_change_blocks_write_back_and_manual_save_unlocks_it() {
    let (dir, mut app, clock) = app_with_clock();
    let path = dir.path().join("a.md");
    let id = open_file(&mut app, dir.path(), "a.md", "alpha\n");
    configure(&mut app, Some(1), true);

    dirty(&mut app, id, "EDIT ");
    // 模拟 Git 切分支 / 另一编辑器 / 同步盘在用户停手期间改了同一个文件。
    std::fs::write(&path, "externally changed\n").expect("外部改写应成功");
    tick(&mut app, &clock);

    assert_eq!(
        disk_text(&path),
        "externally changed\n",
        "检测到外部改动后绝不能覆盖磁盘——这是不可逆的数据丢失"
    );
    assert!(
        status_text(&app).contains("已被其他程序改动"),
        "冲突必须明确告知用户，不能静默跳过"
    );
    assert!(
        draft_path(&app, id).exists(),
        "跳过写回时本次修改仍要落到草稿"
    );

    // 用户显式决定覆盖磁盘（Ctrl+S）：解锁并刷新基线。
    app.save_document(false).expect("手动保存应成功");
    dirty(&mut app, id, "AGAIN ");
    tick(&mut app, &clock);
    assert!(
        disk_text(&path).contains("AGAIN"),
        "手动处置后自动写回必须恢复，不能永久停摆"
    );
}

/// 自动保存整体关闭时，写回开关不应产生任何写盘。
#[test]
fn write_back_does_nothing_when_autosave_is_disabled() {
    let (dir, mut app, clock) = app_with_clock();
    let path = dir.path().join("a.md");
    let id = open_file(&mut app, dir.path(), "a.md", "alpha\n");
    configure(&mut app, None, true);

    dirty(&mut app, id, "EDIT ");
    tick(&mut app, &clock);

    assert_eq!(disk_text(&path), "alpha\n", "没有节拍就没有写回");
    assert!(!draft_path(&app, id).exists(), "自动保存关闭时不写草稿");
}
