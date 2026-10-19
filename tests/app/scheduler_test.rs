//! 帧循环定时任务的**接线**（T11 / ADR-009）。
//!
//! # 这一层测什么，不测什么
//!
//! - 测：某个定时功能到底有没有把到期点交给 `FrameScheduler`，以及真实帧里
//!   egui 最终拿到的重绘延迟是不是「最早的那个」；
//! - 不测：剩余时长怎么算、过期怎么钳、同键多次登记取谁——那些由
//!   `src/app/frame/scheduler.rs` 的内联单测负责（它直接 `arm`，不经过业务路径）。
//!
//! 分开的理由：内联单测**看不出接线漏了**（它自己造登记），而业务路径又无法
//! 在小规模用例里等到 4 秒的消息过期。两者合起来才盖住「`arm` 没被调用」
//! 这个历史上真实发生过的缺陷形态。
//!
//! # 为什么能驱动完整帧
//!
//! `eframe::Frame::_new_kittest()` 提供无窗口、无 GPU 的空 `Frame`，
//! 而 `EditorApp::ui` 不使用 `frame`，因此可以像生产一样逐帧调用——
//! 包括帧末的 `flush`。没有这条通路，「预约真的发给了 egui」就只能靠真机观察。

use std::sync::Arc;
use std::time::Duration;

use eframe::App as _;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::preferences::commands::SetEditorCommand;
use rustmark::application::preferences::queries::GetSettingsQuery;
use rustmark::presentation::state::status_bar_ui_state::MESSAGE_TTL_MS;
use rustmark::shared::time::Clock;

use crate::support::clock::TestClock;
use crate::support::fake_dialog::FakeDialog;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

fn frame_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        ..Default::default()
    }
}

/// 驱动 `frames` 帧真实 `EditorApp::ui`，返回**最后一帧**的 `FullOutput`。
fn run_frames(app: &mut EditorApp, clock: &TestClock, frames: usize) -> egui::FullOutput {
    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    let mut last = None;
    for index in 0..frames {
        // 显式给时钟：无头环境默认 `time` 恒为 0，egui 会把每帧都当成首帧，
        // 前两帧的重绘延迟恒为 0（与浮层定位测试「先跑 ≥3 帧」同源）。
        let mut input = frame_input();
        input.time = Some(index as f64 * 0.016);
        let mut out = crate::support::run_ui(&ctx, input, |ui| {
            app.ui(ui, &mut frame);
        });
        out.textures_delta.clear();
        last = Some(out);
        // 让测试时钟与帧号同步推进：`flush` 的剩余时长按本帧时钟换算，
        // 若时钟不动而 egui 时间在走，断言值会随帧号漂移。
        clock.advance(16);
    }
    last.expect("至少跑一帧")
}

/// 取 `FullOutput` 里最紧的重绘延迟（egui 0.37 把它按视口存放）。
fn repaint_delay(out: &egui::FullOutput) -> Duration {
    out.viewport_output
        .values()
        .map(|viewport| viewport.repaint_delay)
        .min()
        .unwrap_or(Duration::MAX)
}

/// 造一个带可控时钟的应用（不打开任何文档）。
fn app_with_clock() -> (tempfile::TempDir, EditorApp, Arc<TestClock>) {
    let cfg = tempfile::tempdir().expect("配置目录");
    let clock = Arc::new(TestClock::default());
    let services = Services::assemble_with_clock_and_dialogs(
        ConfigPaths::in_dir(cfg.path()),
        clock.clone(),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let app = EditorApp::new(services, false);
    (cfg, app, clock)
}

/// 在一个新工作区里打开指定文件。
fn open_file(app: &mut EditorApp, dir: &std::path::Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("目录应可建");
    }
    std::fs::write(&path, content).expect("文件应可写");
    app.open_workspace_file(&path.to_string_lossy().replace('\\', "/"))
        .expect("打开应成功");
}

/// 在 `scheduled_deadlines` 里找某个到期点。
fn deadline_of(app: &EditorApp, key: &str) -> Option<u64> {
    app.scheduled_deadlines()
        .into_iter()
        .find(|(name, _)| *name == key)
        .map(|(_, due)| due)
}

// ===========================================================================
// 三个定时功能各自「有没有登记」
// ===========================================================================

/// 自动保存：脏文档在间隔内必须登记到期点。
///
/// 直接驱动 `run_autosave` 而不是跑整帧：统计防抖也会登记 150ms 的到期点，
/// 整帧的 `repaint_delay` 会被它抢先，从而**看不出**自动保存有没有接线。
/// 这里要看的是「自动保存这条路径有没有 produce 到期点」，所以只跑它。
#[test]
fn autosave_arms_a_deadline_for_a_dirty_document() {
    let (cfg, mut app, clock) = app_with_clock();
    // 1 秒间隔：短到「刚编辑完一定在窗口内」，又长到不会当场到期。
    let mut editor = app
        .services
        .preferences
        .get_settings(GetSettingsQuery)
        .expect("设置应可读")
        .editor;
    editor.auto_save_interval = Some(1);
    app.services
        .preferences
        .set_editor(SetEditorCommand { editor })
        .expect("间隔应可写");

    app.new_document().expect("新建应成功");
    app.insert_at_cursor("dirty").expect("插入应成功");

    app.run_autosave();
    let due = deadline_of(&app, "autosave").expect(
        "编辑过的文档必须登记自动保存到期点——缺了它，用户停手后不会再有帧，\
         自动保存表现为完全不生效（问题 068）",
    );
    let now = clock.now().millis();
    let remaining = due.saturating_sub(now);
    assert!(
        remaining > 0 && remaining <= 1_000,
        "到期点应落在 (0, 1000] 毫秒内，实际剩余 {remaining}ms（due={due} now={now}）"
    );
    drop(cfg);
}

/// 统计防抖：版本刚变、窗口未满时必须登记到期点。
#[test]
fn stats_debounce_arms_a_deadline_after_an_edit() {
    let (cfg, mut app, clock) = app_with_clock();
    let docs = tempfile::tempdir().expect("文档目录");
    open_file(&mut app, docs.path(), "d.md", "hello");

    // 首帧：首次见文档立刻算，不登记（没有等待中的窗口）。
    let _ = app.current_stats_for_status_bar();

    let id = app.active_doc_for_probe();
    app.services
        .documents
        .insert_text(
            rustmark::application::document::commands::InsertTextCommand {
                id,
                offset: 5,
                text: " world".to_string(),
            },
        )
        .expect("插入应成功");

    let _ = app.current_stats_for_status_bar();
    let due = deadline_of(&app, "stats")
        .expect("版本刚变、窗口未满时必须登记 —— 缺了它，停手后字数永远停在旧值");
    let now = clock.now().millis();
    let remaining = due.saturating_sub(now);
    assert!(
        remaining > 0 && remaining <= 150,
        "到期点应为「变化时刻 + 150ms」，实际剩余 {remaining}ms"
    );
    drop((cfg, docs));
}

/// 状态栏消息：TTL 未到时必须让 egui 预约一帧。
#[test]
fn status_message_reserves_a_frame_for_its_ttl() {
    let (cfg, mut app, clock) = app_with_clock();
    app.set_status_notice("已保存");

    let out = run_frames(&mut app, &clock, 4);
    let delay = repaint_delay(&out);
    assert!(
        delay > Duration::from_millis(3_800) && delay <= Duration::from_millis(MESSAGE_TTL_MS),
        "重绘延迟 {delay:?} 不在 4 秒 TTL 窗口内——\
         说明消息没有登记到期点，一条该消失的提示会一直挂在状态栏上"
    );
    drop(cfg);
}

// ===========================================================================
// 真实帧的预约：取最早的到期点
// ===========================================================================

/// 多个到期点并存时，egui 拿到的重绘延迟是**最早**那个。
///
/// 构造：自动保存 1s + 统计防抖 150ms + 消息 TTL 4s。
/// 若 `flush` 取 max、或干脆不预约、或钳成 0，这条都会红——
/// 它是「统一出口真的把最早到期点交给了 egui」的唯一端到端证据。
#[test]
fn a_real_frame_reserves_the_earliest_deadline() {
    let (cfg, mut app, clock) = app_with_clock();
    let docs = tempfile::tempdir().expect("文档目录");
    open_file(&mut app, docs.path(), "d.md", "hello");
    app.set_status_notice("已保存");

    let mut editor = app
        .services
        .preferences
        .get_settings(GetSettingsQuery)
        .expect("设置应可读")
        .editor;
    editor.auto_save_interval = Some(1);
    app.services
        .preferences
        .set_editor(SetEditorCommand { editor })
        .expect("间隔应可写");

    // 制造「统计窗口未满」的状态。
    let _ = app.current_stats_for_status_bar();
    let id = app.active_doc_for_probe();
    app.services
        .documents
        .insert_text(
            rustmark::application::document::commands::InsertTextCommand {
                id,
                offset: 5,
                text: " world".to_string(),
            },
        )
        .expect("插入应成功");

    // 前三帧是 egui 的测量 / 字体图集帧（delay 恒为 0），第四帧才反映预约。
    let out = run_frames(&mut app, &clock, 4);
    let delay = repaint_delay(&out);
    assert!(
        delay > Duration::from_millis(1),
        "重绘延迟塌到 {delay:?}：说明没有任何人预约，或预约被钳成 0（忙循环）"
    );
    assert!(
        delay <= Duration::from_millis(150),
        "重绘延迟 {delay:?} 超过了最早的到期点（统计防抖 150ms）——\
         说明 flush 没有取「最早」那个，晚到的消息 TTL / 自动保存把叫醒推迟了"
    );
    drop((cfg, docs));
}
