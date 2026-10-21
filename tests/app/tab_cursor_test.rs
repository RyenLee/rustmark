//! 切标签保留光标（R-06 验收）。
//!
//! # 修的缺陷
//!
//! 旧实现 `switch_document` 切换时硬置 `editor.cursor = 0`：用户在 A 文档
//! 第 3 行编辑，切到 B 再切回 A，光标弹回文档开头——「每切一次标签就要
//! 重新找行」。
//!
//! # 三层判据（从内到外，缺一不可）
//!
//! 1. **应用层**：切走时记下 A 的光标、切回时恢复它（`per_doc_cursor` 表）。
//!    只断言 `editor.cursor` 分不清「表里有值」与「恰好默认 0」，故同时
//!    用 `per_doc_cursor_for_diag` 钉住「真的记了」。
//! 2. **一次性通道**：切回帧必须把恢复位置写进 `pending_cursor`——否则 egui
//!    的 `TextEdit` 内部光标态在切文档时被内容替换重置，只改 `editor.cursor`
//!    不够，下一帧 egui 会把自己的旧态（归零）写回来。本层断言「排进了
//!    通道」且「渲染帧消费掉了它」。
//! 3. **真帧**：跑 `main_view::show`（其内部经 `editor_view` 调
//!    `apply_pending_editor_state` 消费 `pending_cursor` 并把脱字符写进 egui
//!    `TextEditState`），再断言 egui 报回的光标仍是 A 第 3 行。
//!
//! # 为什么文档用中文
//!
//! 第 3 行的**字节**偏移（32）与**字符**偏移（12）不同：`byte_to_char`
//! 必须按**新文档 A** 的文本换算。若实现误拿旧文档 B 的文本算，字符偏移
//! 会错，被第 2 层断言当场抓住。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::{NewDocumentCommand, ReplaceRangeCommand};
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::shared::geometry::{ByteOffset, TextRange};
use rustmark::shared::ids::DocumentId;
use std::sync::Arc;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// 文档 A：中文多行，第 3 行是光标落点。
const DOC_A: &str = "第一行内容\n第二行内容\n第三行内容\n第四行内容\n";
/// 文档 B：ASCII 短文档——与 A 的长度 / 字符结构都不同，用来暴露
/// 「拿旧文本换算字符偏移」的实现错误。
const DOC_B: &str = "second doc\n";

/// 造应用 + 两个文档（A 为活动页、B 备用），返回 `(A id, B id, app, tempdir)`。
fn two_docs(a_text: &str, b_text: &str) -> (DocumentId, DocumentId, EditorApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let a = app.ensure_document().expect("启动即开空白文档");
    set_content(&mut app, a, a_text);
    // `new_document` 只建文档、不改活动页——活动页仍是 A，供后续 GotoTab2 切换。
    let b = app
        .services
        .documents
        .new_document(NewDocumentCommand)
        .expect("应能新建文档");
    set_content(&mut app, b, b_text);
    (a, b, app, dir)
}

/// 直接写领域内容并刷新 UI 副本（绕过 `TextEdit`，与 find_replace_view_test 同法）。
fn set_content(app: &mut EditorApp, id: DocumentId, content: &str) {
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: content.to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
}

/// 复刻真实帧循环的最小驱动：同一 `ctx` 上反复 `main_view::show`。
///
/// `ctx` 必须跨帧复用——`TextEditState`（含脱字符）存在 `ctx.memory` 里，
/// 每帧新建 `ctx` 会把状态丢掉，`pending_cursor` 的消费就无从观测。
struct Frames {
    ctx: egui::Context,
}

impl Frames {
    fn new() -> Self {
        let ctx = egui::Context::default();
        // 首帧空跑装载字库（egui 0.37 `run_logic` 不初始化字体）。
        crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});
        Frames { ctx }
    }

    /// 跑一帧（时间 `t` 秒）。`main_view::show` 内部经 `editor_view` 调
    /// `apply_pending_editor_state`，即真实帧序里 `pending_cursor` 的消费点。
    fn show(&self, app: &mut EditorApp, t: f64) {
        app.ui
            .set_theme(rustmark::presentation::theme::builtin::Theme::dark());
        app.ui.preview_visible = false;
        app.ui.sidebar_visible = false;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(t),
            ..Default::default()
        };
        let _ = crate::support::run_ui(&self.ctx, input, |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        });
    }
}

// ============================================================================
// 判据 1：应用层——切走记下、切回恢复
// ============================================================================

#[test]
fn switching_tabs_restores_per_document_cursor() {
    let (a, b, mut app, _d) = two_docs(DOC_A, DOC_B);
    // 把 A 的光标搬到第 3 行（走 R-11 收敛的入口，模拟编辑区每帧的同步）。
    let byte3 = DOC_A.find("第三行内容").expect("第 3 行应存在");
    app.set_editor_selection(byte3, None);

    // 切到 B：应记下 A 的光标 / 选区。
    app.execute(UiAction::GotoTab2);
    assert_eq!(
        app.per_doc_cursor_for_diag(a),
        Some((byte3, byte3)),
        "切走时应记下 A 的光标 / 选区"
    );
    // B 从未编辑过 → 恢复默认 0，与旧行为一致。
    assert_eq!(app.editor.cursor, 0, "B 无记录，光标应在开头");

    // 切回 A：应恢复 A 的光标，而非弹回 0。
    app.execute(UiAction::GotoTab1);
    assert_ne!(
        byte3, 0,
        "第 3 行偏移不应为 0，否则分不清『恢复』与『归零』"
    );
    assert_eq!(
        app.editor.cursor, byte3,
        "切回应恢复 A 第 3 行的光标，而非弹回文档开头"
    );
    assert_eq!(app.editor.anchor, None, "恢复的光标应无选区");
    let _ = b;
}

// ============================================================================
// 判据 2：一次性通道——排进 pending_cursor，渲染帧消费掉
// ============================================================================

#[test]
fn switching_back_schedules_pending_cursor_then_frame_consumes_it() {
    let (a, b, mut app, _d) = two_docs(DOC_A, DOC_B);
    let byte3 = DOC_A.find("第三行内容").expect("第 3 行");
    let char3 = DOC_A[..byte3].chars().count();
    app.set_editor_selection(byte3, None);
    app.execute(UiAction::GotoTab2);

    // 切回 A：restore 应把恢复位置写进 pending_cursor（按 A 文本换算的字符偏移）。
    app.execute(UiAction::GotoTab1);
    assert_eq!(
        app.pending_cursor_for_diag(),
        Some((char3, char3)),
        "切回帧应把恢复的光标排进 pending_cursor（字符偏移须按 A 文本换算）"
    );

    // 真帧：editor_view::show 内部调 apply_pending_editor_state 消费它。
    // 首帧建 TextEditState、次帧才消费（load_state 首帧可能为 None，
    // 而实现刻意「先取 state 再 take」，故 pending 不被吞）。跑三帧收敛。
    let f = Frames::new();
    f.show(&mut app, 0.0);
    f.show(&mut app, 0.2);
    f.show(&mut app, 0.4);
    assert_eq!(
        app.pending_cursor_for_diag(),
        None,
        "渲染帧应已消费 pending_cursor"
    );
    assert_eq!(
        app.editor.cursor, byte3,
        "egui 重设脱字符后报回的光标应仍是 A 第 3 行"
    );
    let _ = (a, b);
}

// ============================================================================
// 判据 3：真帧·egui 侧闭环——脱字符真的被写进 TextEditState
// ============================================================================

#[test]
fn switching_back_resets_the_egui_caret_to_the_restored_position() {
    // 长文档：光标落在第 120 行。
    let long: String = (1..=150).map(|i| format!("第{i}行文本\n")).collect();
    let (a, b, mut app, _d) = two_docs(&long, DOC_B);
    let byte_deep = long.find("第120行文本").expect("第 120 行");
    let char_deep = long[..byte_deep].chars().count();
    app.set_editor_selection(byte_deep, None);
    app.execute(UiAction::GotoTab2);
    app.execute(UiAction::GotoTab1);

    // 跑三帧让 apply_pending_editor_state 把脱字符写进 egui TextEditState。
    let f = Frames::new();
    f.show(&mut app, 0.0);
    f.show(&mut app, 0.2);
    f.show(&mut app, 0.4);

    // 直接读 egui 内部态：脱字符必须落在恢复位置（第 120 行的字符偏移），
    // 而非被切文档时的内容替换重置回 0。这是 R-06 与 pending_cursor 时序
    // 协同的「egui 侧」闭环——只改 `app.editor.cursor` 到不了这里。
    //
    // 为什么不断言 `scroll_y`：egui「把光标滚进视口」依赖焦点 / 交互，
    // 无头 `run_ui` 下不触发（实测 scroll_y 恒 0），拿它当判据会假红。
    let state =
        egui::TextEdit::load_state(&f.ctx, EditorApp::editor_id()).expect("TextEditState 应已建立");
    let range = state.cursor.char_range().expect("应有脱字符范围");
    assert_eq!(
        range.primary.index.0, char_deep,
        "egui 脱字符应被重设到恢复位置（第 120 行字符偏移 {char_deep}），而非归零"
    );
    let _ = (a, b);
}
