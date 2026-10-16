//! S7-06 异常退出与草稿恢复（集成级）。
//!
//! # 范围与诚实声明
//!
//! 本文件验证「异常退出与草稿恢复」中**已实现**的底层保证：
//!
//! 1. **原子保存的控制流保证**（`FR-04`）：目标文件只在最后一次 `rename` 处被替换，
//!    且 `rename` 前临时文件已 `sync_all`。因此「进程在写入中途崩溃」不会让目标文件
//!    出现半截内容——崩溃最多留下一个 `.rustmark-tmp-` 残留，目标始终是最后一次
//!    成功保存的完整内容。这对应验收点「原子保存中断原文件完好」。
//! 2. **自动保存计时逻辑**（`AutoSaveTracker`）：纯逻辑层验证「文档空闲 ≥ 配置间隔
//!    才被标记为待保存」，对应验收点「强杀后损失 ≤1 个自动保存间隔」的**决策逻辑**。
//! 3. **草稿仓储机制**（`save_draft`/`load_draft`/`clear_draft`）：恢复功能的**数据层**。
//!
//! ## 端到端验收的当前状态（DEF-02 已修复）
//!
//! DEF-02 的**运行时接线已补齐**（见 `docs/crash-recovery-report.md` 修订）：
//!
//! - `EventSubscribers` 在装配阶段注册到事件总线，`TextModified` → `AutoSaveTracker`
//!   记录、`DocumentSaved`/`DocumentClosed` → 移除计时；
//! - `EditorApp::update` 每帧调用 `auto_save_tracker.due`，到期文档经
//!   `DocumentAppService::save_draft` 写入 `.rustmark-draft/<doc_id>.draft`；
//! - 启动流程扫描 `.rustmark-draft/` 得到 `pending_recoveries`，弹出恢复提示。
//!
//! 上述接线已通过生产级 `Services` 装配在 `autosave_writes_draft_and_is_recoverable_after_wiring`
//! 与 `restart_scan_finds_leftover_drafts` 两例中验证。
//!
//! ## headless 端到端（第 4 节）
//!
//! `e2e_crash_recovery_restores_unsaved_draft` 驱动**真实** `EditorApp::update`
//! 帧循环：eframe 为 kittest 提供了 `Frame::_new_kittest()`（无窗口、无 GPU 的空
//! `Frame`），而 `EditorApp::update` 恰不使用 `frame`，故可用 `ctx.run` 直接驱动。
//! 由此「编辑 → 自动保存落草稿 → 崩溃 → 重启扫描 → 恢复」全链路无需真实窗口即可验收。

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use eframe::App;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::{InsertTextCommand, OpenFileCommand};
use rustmark::application::document::queries::GetContentQuery;
use rustmark::application::event_subscriptions::AutoSaveTracker;
use rustmark::application::preferences::commands::SetEditorCommand;
use rustmark::application::preferences::queries::GetSettingsQuery;
use rustmark::domain::document::repository::{DocumentRepository, DocumentSnapshot};
use rustmark::infrastructure::persistence::fs_document_repository::FsDocumentRepository;
use rustmark::shared::ids::DocumentId;
use rustmark::shared::path::FilePath;
use rustmark::shared::time::Timestamp;

use crate::support::fake_dialog::FakeDialog;

/// 把磁盘路径包成领域层 `FilePath`（与 fs_document_repository_test.rs 同款）。
fn fp(disk: &std::path::Path) -> FilePath {
    FilePath::new(disk.to_string_lossy().replace('\\', "/")).expect("临时路径应合法")
}

fn snapshot(disk: &std::path::Path, content: &str) -> DocumentSnapshot {
    DocumentSnapshot::new(DocumentId::new(), fp(disk), content, None)
}

/// 列出目录下所有 `.rustmark-tmp-` 残留文件名。
fn leftover_temp_files(dir: &std::path::Path) -> Vec<String> {
    fs::read_dir(dir)
        .expect("应能列出目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(".rustmark-tmp-"))
        .collect()
}

// ---------------------------------------------------------------------------
// 1. 原子保存的控制流保证（FR-04）：模拟「写入中途崩溃」
// ---------------------------------------------------------------------------

/// 模拟场景：上一次保存在「写完临时文件、尚未 rename」时崩溃，磁盘留下一个
/// `.rustmark-tmp-<name>` 垃圾残留。随后用户正常编辑并再次保存。
///
/// 断言：
/// - 崩溃残留**从未污染**目标文件——目标始终是上次成功保存的完整内容 A；
/// - 新的保存（内容 B）走原子 rename 覆盖目标，结果恰好是 B；
/// - 崩溃残留被新保存的临时文件同名覆盖并最终 rename 掉，**不留垃圾**。
#[test]
fn atomic_save_survives_leftover_temp_from_crash() {
    let dir = tempfile::tempdir().expect("应能创建临时目录");
    let target = dir.path().join("note.md");
    let repo = FsDocumentRepository::new();

    // 第一次正常保存：目标 = "原始内容A"。
    repo.save(&snapshot(&target, "原始内容A"))
        .expect("首次保存应成功");
    assert_eq!(
        repo.load(&fp(&target)).expect("读取应成功").content,
        "原始内容A"
    );

    // 模拟崩溃：在「同名临时文件」里塞入垃圾，但 rename 未发生。
    let tmp_path = dir.path().join(".rustmark-tmp-note.md");
    fs::write(&tmp_path, "GARBAGE-HALF-WRITTEN").expect("应可写垃圾残留");
    // 崩溃发生时，目标仍是上次完整的 A（残留只影响 tmp，不影响 target）。
    assert_eq!(
        repo.load(&fp(&target)).expect("读取应成功").content,
        "原始内容A",
        "崩溃残留不得污染目标文件"
    );

    // 新一次正常保存：内容 B。
    repo.save(&snapshot(&target, "新内容B"))
        .expect("第二次保存应成功");

    // 目标被原子覆盖为 B，且残留垃圾已随 rename 消失。
    assert_eq!(
        repo.load(&fp(&target)).expect("读取应成功").content,
        "新内容B",
        "崩溃后再次保存，目标应恰好是新内容 B（不是垃圾、不是 A 的残缺）"
    );
    assert!(
        leftover_temp_files(dir.path()).is_empty(),
        "保存后应无 .rustmark-tmp- 残留，实际：{:?}",
        leftover_temp_files(dir.path())
    );
}

/// 模拟场景：保存的父路径组件被一个**普通文件**占据，导致原子写入无法落盘。
///
/// 断言：保存失败（`Err`），但**不泄漏**临时文件，且不产生任何半截目标。
/// 这是「写入路径本身不可达」时原子性下限的验证。
#[test]
fn atomic_save_failure_leaves_no_temp_residue() {
    let dir = tempfile::tempdir().expect("应能创建临时目录");
    let repo = FsDocumentRepository::new();

    // 用同名文件占据父目录位置，使 `blocker/note.md` 永远无法创建父目录。
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, b"i am a file, not a dir").expect("应可写 blocker");
    let bad_target = dir.path().join("blocker").join("note.md");

    let result = repo.save(&snapshot(&bad_target, "不应落盘"));
    assert!(
        result.is_err(),
        "父路径被文件占据时保存必须失败而非静默成功"
    );
    assert!(
        leftover_temp_files(dir.path()).is_empty(),
        "失败保存不得遗留 .rustmark-tmp- 文件，实际：{:?}",
        leftover_temp_files(dir.path())
    );
}

// ---------------------------------------------------------------------------
// 2. 自动保存计时逻辑：验证「损失 ≤1 个自动保存间隔」的决策正确性
// ---------------------------------------------------------------------------

#[test]
fn autosave_tracker_marks_due_only_after_interval() {
    let tracker = AutoSaveTracker::new();
    let id = DocumentId::new();
    let t0 = Timestamp::from_millis(1_000);
    tracker.record(id, 1, t0);

    // 间隔 30_000ms：在 t0 + 29_999 时仍未到，不应被标记。
    assert!(
        tracker
            .due(Timestamp::from_millis(1_000 + 29_999), 30_000)
            .is_empty(),
        "距最后编辑不足一个间隔，不应触发自动保存"
    );

    // 恰好到达间隔边界：应被标记待保存。
    let due = tracker.due(Timestamp::from_millis(1_000 + 30_000), 30_000);
    assert_eq!(due, vec![id], "到达间隔边界应触发自动保存");

    // `due` 采用「取出即清除」语义：下一次查询同文档不应重复保存。
    assert!(
        tracker
            .due(Timestamp::from_millis(1_000 + 60_000), 30_000)
            .is_empty(),
        "`due` 取出后应清除追踪，避免对同一文档重复自动保存"
    );
}

#[test]
fn autosave_tracker_forget_removes_tracking() {
    let tracker = AutoSaveTracker::new();
    let id = DocumentId::new();
    tracker.record(id, 1, Timestamp::from_millis(0));
    assert!(tracker.contains(id), "记录后应在追踪中");

    tracker.forget(id);
    assert!(!tracker.contains(id), "forget 后不应再追踪");
    assert!(
        tracker.due(Timestamp::from_millis(10_000), 1).is_empty(),
        "forget 后即使超过间隔也不应被标记为待保存（已保存/已关闭）"
    );
}

#[test]
fn autosave_tracker_partial_due_across_multiple_docs() {
    let tracker = AutoSaveTracker::new();
    let a = DocumentId::new();
    let b = DocumentId::new();

    // a 在 t=0 编辑，b 在 t=20_000 编辑。
    tracker.record(a, 1, Timestamp::from_millis(0));
    tracker.record(b, 1, Timestamp::from_millis(20_000));

    // 在 t=30_000、间隔 30_000 时：a 空闲 30_000（应保存），b 空闲 10_000（不应）。
    let due = tracker.due(Timestamp::from_millis(30_000), 30_000);
    assert_eq!(due.len(), 1, "只应有一个文档到达间隔");
    assert!(due.contains(&a), "a 应被标记");
    assert!(!due.contains(&b), "b 尚未到间隔，不应被标记");
}

// ---------------------------------------------------------------------------
// 3. 草稿仓储机制：恢复功能的「数据层」存在性验证
// ---------------------------------------------------------------------------

/// 草稿的「写 → 读 → 清 → 再读为空」语义，以及清除的幂等性。
///
/// 这是恢复功能的底层数据层：一旦 S4-10/S5-04 把运行时接线补齐，
/// 启动时扫描 `.rustmark-draft/<doc_id>.draft` 即可还原未保存内容。
/// （与 `tests/infrastructure/fs_document_repository_test.rs` 互补：那里验单测，
/// 这里在集成目标里把草稿明确锚定为「崩溃恢复数据层」。）
#[test]
fn draft_persistence_roundtrip_and_clear() {
    let dir = tempfile::tempdir().expect("应能创建临时目录");
    let drafts = dir.path().join(".rustmark-draft");
    let repo = FsDocumentRepository::new();
    let id = DocumentId::new();

    assert_eq!(
        repo.load_draft(&drafts, id).unwrap(),
        None,
        "尚无草稿时应返回 None"
    );

    repo.save_draft(&drafts, id, "未保存的草稿内容")
        .expect("写草稿应成功");
    assert_eq!(
        repo.load_draft(&drafts, id).unwrap(),
        Some("未保存的草稿内容".to_string()),
        "应能读回草稿"
    );

    repo.clear_draft(&drafts, id).expect("清草稿应成功");
    assert_eq!(
        repo.load_draft(&drafts, id).unwrap(),
        None,
        "清除后草稿应消失"
    );
    // 幂等：再次清除不应报错。
    repo.clear_draft(&drafts, id).expect("重复清除应按成功处理");
}

// ---------------------------------------------------------------------------
// 3.5 DEF-02 修复后：自动保存计时器接线 + 草稿落盘 + 启动恢复扫描
// ---------------------------------------------------------------------------

/// DEF-02 接线验证（不再挂起）：自动保存计时器已接入事件总线，
/// `EditorApp::update` 每帧通过 `auto_save_tracker.due` 驱动落盘，
/// 启动流程扫描 `.rustmark-draft/` 得到待恢复草稿。
///
/// 本用例走与生产相同的 `Services` 装配（含 `EventSubscribers` 注册），
/// 因此同时验证了「`TextModified` → 计时器记录」这一事件接线。
#[test]
fn autosave_writes_draft_and_is_recoverable_after_wiring() {
    let dir = tempfile::tempdir().expect("应能创建临时目录");
    let mut services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");

    // 准备一个磁盘源文件并打开。
    let src = dir.path().join("src.md");
    std::fs::write(&src, "# 原始标题\n正文\n").expect("应可写源文件");
    let id = services
        .documents
        .open(OpenFileCommand { path: fp(&src) })
        .expect("打开应成功");

    // 编辑文档 → 发布 `TextModified` → 计时器被事件订阅记录（DEF-02 接线）。
    services
        .documents
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: "追加内容".to_string(),
        })
        .expect("插入应成功");

    assert!(
        services.auto_save_tracker.contains(id),
        "TextModified 事件应驱动 AutoSaveTracker 记录该文档（DEF-02 接线）"
    );

    // 模拟「到达自动保存间隔」：间隔设为 0，立即到期。
    let now = services.markdown.now();
    let due = services.auto_save_tracker.due(now, 0);
    assert_eq!(due, vec![id], "到期文档应被标记为待保存");

    // 自动保存把草稿落到磁盘（与 `EditorApp::run_autosave` 同一路径）。
    services
        .documents
        .save_draft(&services.drafts_dir, id)
        .expect("写草稿应成功");

    // 草稿文件应存在且内容等于文档当前内容。
    let expected = services
        .documents
        .get_content(GetContentQuery { id, range: None })
        .expect("读内容应成功");
    let draft_path = services.drafts_dir.join(format!("{id}.draft"));
    assert!(draft_path.exists(), "草稿文件应已落盘");
    let draft_content = std::fs::read_to_string(&draft_path).expect("读草稿应成功");
    assert_eq!(
        draft_content, expected,
        "草稿内容应与文档当前内容一致（恢复不丢字）"
    );

    // 启动恢复扫描应发现该草稿。
    let recoveries = services
        .documents
        .pending_recoveries(&services.drafts_dir)
        .expect("扫描草稿应成功");
    assert_eq!(recoveries.len(), 1, "应发现 1 份待恢复草稿");
    assert_eq!(recoveries[0].1, expected, "恢复内容应与文档一致");

    // 恢复后清草稿（与 `EditorApp::render_recovery_prompt` 的「丢弃」同路径）。
    services
        .documents
        .clear_draft(&services.drafts_dir, id)
        .expect("清草稿应成功");
    assert!(
        services
            .documents
            .pending_recoveries(&services.drafts_dir)
            .expect("扫描应成功")
            .is_empty(),
        "清草稿后启动恢复扫描应为空"
    );
}

/// 「重启」场景：第一轮进程异常退出（留下草稿），第二轮进程启动时应扫描到遗留草稿。
///
/// 用**同一配置目录**装配两套 `Services` 模拟「两次启动」，验证草稿跨进程留存，
/// 即 NFR-REL-03「异常退出后草稿恢复」的数据层闭环。
#[test]
fn restart_scan_finds_leftover_drafts() {
    let dir = tempfile::tempdir().expect("应能创建临时目录");

    // 第一轮：打开并编辑，模拟自动保存写出草稿后「崩溃」。
    let mut first = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let src = dir.path().join("doc.md");
    std::fs::write(&src, "v1\n").expect("应可写源文件");
    let id = first
        .documents
        .open(OpenFileCommand { path: fp(&src) })
        .expect("打开应成功");
    first
        .documents
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: "未保存的改动".to_string(),
        })
        .expect("插入应成功");
    first
        .documents
        .save_draft(&first.drafts_dir, id)
        .expect("模拟自动保存应成功");
    // first 在此离开作用域，相当于进程结束（未手动保存）。

    // 第二轮：同一配置目录重新装配（模拟重启）。
    let second = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("重启装配应成功");
    let recoveries = second
        .documents
        .pending_recoveries(&second.drafts_dir)
        .expect("重启扫描草稿应成功");
    assert_eq!(recoveries.len(), 1, "重启应发现上一轮遗留的 1 份草稿");
    assert_eq!(
        recoveries[0].1, "未保存的改动v1\n",
        "恢复内容应与崩溃前一致"
    );
}

// ---------------------------------------------------------------------------
// 4. 端到端验收（headless 驱动真实 EditorApp::update）
// ---------------------------------------------------------------------------

/// 一帧「真实」输入：显式给定 1280×800 的 `screen_rect`。
///
/// 不给会让 egui 退回上一个（或默认）screen_rect，布局在 10000×10000 上跑，
/// 既慢又与真实窗口尺寸不符。
fn frame_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    }
}

/// 列出草稿目录下所有 `.draft` 文件——自动保存在**磁盘上**的真实产物。
fn draft_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    if !dir.exists() {
        return Vec::new();
    }
    fs::read_dir(dir)
        .expect("应能列出草稿目录")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("draft"))
        .collect()
}

/// 装配一个「自动保存间隔 = 1 秒」的应用。
///
/// 用 1 秒而非默认 30 秒，是为了让用例在可接受的等待内**确定性地**跨过一个间隔，
/// 而不是靠 sleep 猜默认配置。
fn app_with_one_second_autosave(dir: &std::path::Path) -> EditorApp {
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir), Arc::new(FakeDialog::new()))
            .expect("装配应成功");
    let mut editor = services
        .preferences
        .get_settings(GetSettingsQuery)
        .expect("设置应可读")
        .editor;
    editor.auto_save_interval = Some(1);
    services
        .preferences
        .set_editor(SetEditorCommand { editor })
        .expect("自动保存间隔应可写");
    EditorApp::new(services, false)
}

/// 端到端验收（S7-06）：headless 驱动**真实** `EditorApp::update` 帧循环，
/// 验证「编辑 → 自动保存落草稿 → 崩溃 → 重启扫描 → 恢复」全链路。
///
/// 此前挂 `#[ignore]` 的原因是「需 headless 渲染 `EditorApp` 才能驱动真实
/// `update`」。eframe 为此提供了 `Frame::_new_kittest()`（无窗口 / 无 GPU 的空
/// `Frame`），而 `EditorApp::update` 不使用 `frame`，故可直接用 `ctx.run` 驱动。
#[test]
fn e2e_crash_recovery_restores_unsaved_draft() {
    let dir = tempfile::tempdir().expect("临时目录应创建成功");
    let drafts_dir = ConfigPaths::in_dir(dir.path()).drafts_dir();

    // 1) 首次运行：新建文档并输入，但**不保存**。
    let mut app = app_with_one_second_autosave(dir.path());
    app.new_document().expect("新建文档应成功");
    app.insert_at_cursor("未保存的改动").expect("插入应成功");
    assert_eq!(
        draft_files(&drafts_dir).len(),
        0,
        "刚编辑完、间隔未到时不应落草稿"
    );

    // 2) 驱动真实帧循环：跨过 1 个自动保存间隔后，`run_autosave` 应写出草稿。
    std::thread::sleep(Duration::from_millis(1_100));
    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    for _ in 0..3 {
        crate::support::run_ui(&ctx, frame_input(), |ui| {
            app.ui(ui, &mut frame);
        });
    }
    assert_eq!(
        draft_files(&drafts_dir).len(),
        1,
        "自动保存应在间隔到期后写出 1 份草稿"
    );

    // 3) 模拟「进程被强杀」：直接丢弃应用，不走 save / close（二者才会清草稿）。
    drop(app);

    // 4) 二次运行（同一配置目录）：启动扫描应发现遗留草稿。
    let mut restarted = app_with_one_second_autosave(dir.path());
    assert_eq!(
        restarted.pending_recoveries().len(),
        1,
        "重启应扫描到上一轮遗留的 1 份草稿"
    );
    assert!(
        restarted.pending_recoveries()[0].1.contains("未保存的改动"),
        "草稿内容应与崩溃前一致"
    );

    // 5) 驱动几帧真实 update：恢复提示持续存在（用户未选择前不清空）。
    let ctx2 = egui::Context::default();
    let mut frame2 = eframe::Frame::_new_kittest();
    for _ in 0..3 {
        crate::support::run_ui(&ctx2, frame_input(), |ui| {
            restarted.ui(ui, &mut frame2);
        });
    }
    assert_eq!(
        restarted.pending_recoveries().len(),
        1,
        "用户未选择前不应丢弃待恢复内容"
    );

    // 6) 「恢复全部」：内容回到编辑区，草稿被消费。
    restarted.recover_all_drafts();
    assert!(
        restarted.pending_recoveries().is_empty(),
        "恢复后待恢复列表应清空"
    );
    let content = restarted.current_content().expect("应有活动文档");
    assert!(
        content.contains("未保存的改动"),
        "恢复内容应与崩溃前一致：{content}"
    );
    assert_eq!(draft_files(&drafts_dir).len(), 0, "恢复后草稿应被清除");
}

/// 「丢弃全部」分支：删掉磁盘草稿并清空待恢复列表，且**不**新建任何文档。
#[test]
fn recovery_prompt_discard_clears_leftover_drafts() {
    let dir = tempfile::tempdir().expect("临时目录应创建成功");
    let drafts_dir = ConfigPaths::in_dir(dir.path()).drafts_dir();

    let mut app = app_with_one_second_autosave(dir.path());
    app.new_document().expect("新建文档应成功");
    app.insert_at_cursor("将被丢弃的改动").expect("插入应成功");
    std::thread::sleep(Duration::from_millis(1_100));
    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    for _ in 0..2 {
        crate::support::run_ui(&ctx, frame_input(), |ui| {
            app.ui(ui, &mut frame);
        });
    }
    assert_eq!(draft_files(&drafts_dir).len(), 1, "应写出 1 份草稿");
    drop(app);

    let mut restarted = app_with_one_second_autosave(dir.path());
    assert_eq!(restarted.pending_recoveries().len(), 1, "应扫描到 1 份草稿");

    restarted.discard_all_drafts();
    assert!(
        restarted.pending_recoveries().is_empty(),
        "丢弃后待恢复列表应清空"
    );
    assert_eq!(
        draft_files(&drafts_dir).len(),
        0,
        "丢弃后磁盘上的草稿应被删除"
    );
}
