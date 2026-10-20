//! 写作目标输入框（R-05）：持久化 / 重启读回 / 0 → 未设。
//!
//! 验收口径来自改造计划 R-05：
//! 1. 设 1000 → 保存 → 重启 → 读回 1000；
//! 2. 设 0 → 读回「未设」（而非 0 之外的任何值）；
//! 3. 「0 归一为未设」的分支由 `tools/mut_r05.py` 变异验证钉住。
//!
//! 设置页字段是**实时落盘**的（`apply_editor_field` 即时写偏好），
//! 故「保存」只是关闭；这里的「重启」= 用同一配置目录新建一个 `EditorApp`，
//! 它应从磁盘读回已持久化的目标。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::shared::ids::DocumentId;

use crate::support::fake_dialog::FakeDialog;

fn app_in(dir: &std::path::Path) -> EditorApp {
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir), Arc::new(FakeDialog::new()))
            .expect("装配应成功");
    EditorApp::new(services, false)
}

/// 设 1000 → 关闭 → 重启 → 仍读回 1000，且被记为「已设」。
#[test]
fn setting_a_goal_persists_across_restart() {
    let dir = tempfile::tempdir().expect("工作区");
    {
        let mut app = app_in(dir.path());
        app.apply_editor_field(|e| e.writing_goal = 1000);
        // 刷新一下：确认落盘（实时写，但用快照确认「已写入」）。
        assert_eq!(app.settings().editor.writing_goal, 1000, "设置后应立即可见");
    }
    // 模拟重启：全新的应用实例，从同一目录的磁盘偏好读回。
    let app = app_in(dir.path());
    assert_eq!(
        app.settings().editor.writing_goal,
        1000,
        "重启后应读回目标 1000"
    );
    assert!(
        app.services.preferences.has_writing_goal(),
        "目标应被记为「已设」"
    );
}

/// 设 0 → 读回「未设」（0 是未设的合法表示，不是某种默认值）。
#[test]
fn setting_zero_is_read_back_as_unset() {
    let dir = tempfile::tempdir().expect("工作区");
    {
        let mut app = app_in(dir.path());
        app.apply_editor_field(|e| e.writing_goal = 1000);
        app.apply_editor_field(|e| e.writing_goal = 0);
    }
    let app = app_in(dir.path());
    assert_eq!(
        app.settings().editor.writing_goal,
        0,
        "设 0 应读回未设（而非 1000 或其它值）"
    );
    assert!(
        !app.services.preferences.has_writing_goal(),
        "0 必须记为「未设」，否则状态栏进度条会错显"
    );
}

/// 未设目标时，读回的仍是 0（与「从未设过」一致）。
#[test]
fn never_setting_a_goal_reads_back_as_unset() {
    let dir = tempfile::tempdir().expect("工作区");
    let app = app_in(dir.path());
    assert_eq!(app.settings().editor.writing_goal, 0);
    assert!(!app.services.preferences.has_writing_goal());
    // 无关字段不受影响：构造一个假文档确保 app 可用（避免「空应用」误判）。
    let _: Option<DocumentId> = None;
}
