//! 编辑区回车换行：IME 组合期不得吞掉回车（V11-14 回归）。
//!
//! # 缺陷背景
//!
//! 用户报告：**已输入文本后连按多次回车，界面只多出一个新行，必须再输入
//! 一个字符才会真正换行**，且过程有界面闪烁。
//!
//! 根因是 IME 组合期的两处状态管理：
//!
//! 1. `Ime::Enabled` 会让 egui 的 `TextEdit` 把内部 `state.ime_enabled` 置真。
//!    此后它按「候选窗开着」的假设处理键盘——`remove_ime_incompatible_events`
//!    会剔除它认为「IME 期间会打架」的按键。该标志**只能**由
//!    `Preedit`/`Commit`/`Disabled`/焦点变化复位，而真实 Windows 输入法的
//!    `Disabled` 只在**失焦**时才发（切输入法、直接关候选窗、焦点落到浮层
//!    都收不到）。
//! 2. 应用自己也有一层「组合期守卫」，旧实现是**粘性标志**——只有
//!    `Commit`/`Disabled` 能清。它一旦卡住，`pending_cursor` 就永远不被消费，
//!    而 `flush_edit` 每次按键又覆写它，光标从此失联。
//!
//! 两者叠加的结果就是：第一次回车生效，之后每一次都被插到同一处，
//! 界面只多出一个新行；直到用户敲了别的字符才「把攒下的换行带出来」。
//!
//! # 本文件的用例形态
//!
//! 全部用**可控时钟**装配（`Services::assemble_with_clock_and_dialogs`），
//! 断言「保护窗口内」与「窗口已超时」两种状态下的回车行为——真实
//! `SystemClock` 下整个用例跑不完 800ms，超时分支永远测不到。

use crate::support::fake_dialog::FakeDialog;
use eframe::App as _;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::shared::time::{Clock, Timestamp};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

// ============================================================================
// 可控时钟
// ============================================================================

/// 测试时钟：毫秒数由用例显式推进。
///
/// 初值取一个**非零**的 UNIX 纪元量级——与生产 `SystemClock` 同量级，
/// 免得「时间从 0 开始」这种现实中不存在的状态影响被测逻辑
/// （如滚动同步的抑制窗口、撤销合并窗口等都以绝对毫秒做差）。
#[derive(Debug)]
struct TestClock(AtomicU64);

impl Default for TestClock {
    fn default() -> Self {
        TestClock(AtomicU64::new(1_700_000_000_000))
    }
}

impl TestClock {
    fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
}

// ============================================================================
// 夹具
// ============================================================================

struct Fixture {
    _dir: tempfile::TempDir,
    app: EditorApp,
    ctx: egui::Context,
    frame: eframe::Frame,
    clock: Arc<TestClock>,
    /// 上一帧实际绘制到屏幕的文本（`Shape::Text` 的 galley 内容）。
    /// 「可见性」的唯一可信证据：编辑区渲染的 galley 必须携带全文。
    painted: Vec<String>,
}

fn fixture_with(initial: &str) -> Fixture {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let clock = Arc::new(TestClock::default());
    let services = Services::assemble_with_clock_and_dialogs(
        ConfigPaths::in_dir(dir.path()),
        clock.clone(),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let file = dir.path().join("note.md");
    std::fs::write(&file, initial).expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {}); // 装载字库
    Fixture {
        _dir: dir,
        app,
        ctx,
        frame: eframe::Frame::_new_kittest(),
        clock,
        painted: Vec::new(),
    }
}

impl Fixture {
    fn frame_with(&mut self, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            events,
            ..Default::default()
        };
        let out = self.ctx.run(input, |ctx| {
            self.app.update(ctx, &mut self.frame);
        });
        self.painted = out
            .shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Text(g) => Some(g.galley.text().to_string()),
                _ => None,
            })
            .collect();
    }

    /// 编辑区的可见行数：从本帧绘制的文本形状里找携带全文的编辑器 galley，
    /// 按 `\n` 数行。找不到 = 编辑区渲染的 galley 与文本模型不一致。
    fn visible_editor_lines(&self) -> Option<usize> {
        let bound = self.app.editor_bound_text();
        self.painted
            .iter()
            .find(|t| t.as_str() == bound)
            .map(|t| t.split('\n').count())
    }

    /// 聚焦编辑区并把光标放到文末（用户「已输入文本后按回车」的起点）。
    fn focus_at_end(&mut self) {
        self.frame_with(vec![]);
        self.ctx
            .memory_mut(|m| m.request_focus(EditorApp::editor_id()));
        self.frame_with(vec![]);
        self.frame_with(vec![pressed_key(egui::Key::End)]);
        self.frame_with(vec![]);
    }

    /// 按一次回车，再空跑一帧（真实帧节奏：事件帧 + 绘制帧）。
    ///
    /// `advance_ms` 用来模拟两次击键之间的真实时间流逝。**这是必需的**：
    /// 回车连击的人手间隔是几十毫秒，而 IME 保护窗是 800ms（见
    /// [`IME_GUARD_MS`]），两者的关系决定了「保护窗是否已到期」。
    /// 真实运行时 `SystemClock` 自然推进，无头测试必须显式推进才能覆盖
    /// 「窗口已到期」这条分支——否则用例只在窗口内断言，等于没测。
    ///
    /// [`IME_GUARD_MS`]: rustmark::app::frame::IME_GUARD_MS
    fn enter(&mut self) {
        self.enter_after(0);
    }

    /// 先推进 `advance_ms` 毫秒，再按一次回车。
    ///
    /// **必须模拟真实键盘的按下-松开两帧**：egui 跨帧跟踪 `keys_down`，
    /// 只发 pressed 不发 released 时，第二次按下会被 egui 判为
    /// `repeat: true`——而 `ime_enabled` 期间 egui 会剔除全部 repeat 按键，
    /// 那样测出来的「回车被吞」是测试伪影，不是真实键盘行为。
    fn enter_after(&mut self, advance_ms: u64) {
        self.clock.advance(advance_ms);
        self.frame_with(vec![pressed_key(egui::Key::Enter)]);
        self.frame_with(vec![released_key(egui::Key::Enter)]);
    }

    /// 推进时间并空跑一帧，让「有界窗口」到期逻辑真正被触发。
    fn idle_past_guard(&mut self) {
        self.clock.advance(1_000);
        self.frame_with(vec![]);
        self.frame_with(vec![]);
    }

    fn content(&self) -> String {
        self.app.current_content().unwrap_or_default()
    }
}

fn pressed_key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
        physical_key: None,
    }
}

fn released_key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::default(),
        physical_key: None,
    }
}

// ============================================================================
// 回归用例
// ============================================================================

/// 核心回归：输入法开启后（`Enabled` 到达、之后不再有任何 IME 事件），
/// **连按 4 次回车必须产生 4 个新行**。
///
/// 修复前：只有第 1 次生效，领域内容停在 `"hello\n"` 不动——正是用户报告的
/// 「只生成一个新行，必须再输入字符才会真正换行」。
#[test]
fn repeated_enter_works_after_ime_enabled_without_disabled() {
    let mut f = fixture_with("hello");
    f.focus_at_end();

    // 真实 Windows 输入法在编辑区获得焦点时发一次 `Enabled`，
    // 此后若用户没再输入中文（或候选窗被直接关掉），`Disabled` 不会来。
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    f.frame_with(vec![]);

    // `Enabled` 之后保护窗开始计时。真实用户不会在**同一毫秒**里连按 4 次
    // 回车——人手连击间隔 ≥80ms，故这里每按一次推进 100ms，覆盖
    // 「armed 窗口（50ms）已到期 → 补发 Disabled 复位 egui」这条分支。
    for i in 1..=4 {
        f.enter_after(100);
        assert_eq!(
            f.content(),
            format!("hello{}", "\n".repeat(i)),
            "第 {i} 次回车后应多出一个新行"
        );
    }
}

/// 保护窗口的**语义**：`Preedit` 不断续命时不得提前解冻（中文输入不能被打断）。
#[test]
fn preedit_keeps_guard_alive_across_windows() {
    let mut f = fixture_with("");
    f.focus_at_end();

    // 连续三轮组合，每轮都隔「超过半个保护窗」的时长——只要每轮都有
    // `Preedit` 续命，守卫就应始终有效（这里验证的是不崩、不吞字）。
    for word in ["ni", "nihao", "nihaojie"] {
        f.clock.advance(400);
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit(
            word.to_owned(),
        ))]);
        f.frame_with(vec![]);
    }
    f.clock.advance(400);
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Commit(
        "你好".to_owned(),
    ))]);
    f.frame_with(vec![]);

    assert_eq!(f.content(), "你好", "组合完成后应只剩提交的中文");
}

/// 组合**进行中**的那一帧必须冻结光标回设，否则 egui 的预编辑选区被打掉、
/// 提交被静默忽略（V11-13 的原始缺陷，不能被本次修复带回来）。
#[test]
fn commit_after_preedit_is_not_swallowed() {
    let mut f = fixture_with("");
    f.focus_at_end();

    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    f.frame_with(vec![]);
    for pre in ["s", "sa", "sad", "sadaf"] {
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit(
            pre.to_owned(),
        ))]);
        f.frame_with(vec![]);
    }
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Commit(
        "撒打发第三方".to_owned(),
    ))]);
    f.frame_with(vec![]);
    assert_eq!(
        f.content(),
        "撒打发第三方",
        "候选确认后应只留下中文，预编辑串不得残留"
    );

    // 提交后立刻连按回车：必须立即全部生效。
    for i in 1..=3 {
        f.enter();
        assert_eq!(
            f.content(),
            format!("撒打发第三方{}", "\n".repeat(i)),
            "提交后第 {i} 次回车应换行"
        );
    }
}

/// 组合结束后接普通输入 + 连按回车：不得出现「攒着换行」的现象。
#[test]
fn enter_after_ime_commit_and_char_keeps_up() {
    let mut f = fixture_with("");
    f.focus_at_end();

    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    f.frame_with(vec![]);
    for pre in ["a", "ai", "aidi"] {
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit(
            pre.to_owned(),
        ))]);
        f.frame_with(vec![]);
    }
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Commit(
        "爱迪生".to_owned(),
    ))]);
    f.frame_with(vec![]);

    f.frame_with(vec![egui::Event::Text("x".to_owned())]);
    f.frame_with(vec![]);
    assert_eq!(f.content(), "爱迪生x");

    for i in 1..=3 {
        f.enter();
        assert_eq!(f.content(), format!("爱迪生x{}", "\n".repeat(i)));
    }
}

/// 列表续行的回车（`transform_insert` 真的改写了文本）必须照常续序号，
/// 且不得受 IME 守卫影响——这是 `flush_edit` 新增 `transformed` 参数的对照面。
#[test]
fn list_continuation_enter_unaffected_by_ime() {
    let mut f = fixture_with("1. first");
    f.focus_at_end();

    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    f.frame_with(vec![]);

    // 用 `enter_after(100)`：推进时间让 armed 窗口出窗（与真实人手节奏一致），
    // 见 `repeated_enter_works_after_ime_enabled_without_disabled` 的说明。
    f.enter_after(100);
    assert_eq!(f.content(), "1. first\n2. ", "列表续行应续出下一序号");
    f.frame_with(vec![egui::Event::Text("item".to_owned())]);
    f.frame_with(vec![]);
    f.enter_after(100);
    assert_eq!(f.content(), "1. first\n2. item\n3. ", "序号应继续自增");
}

/// 绑定串与领域内容**永不漂移**：这是「敲了键界面必须有反应」的可观测不变量。
/// 漂移过一次就意味着存在「下一次输入才补上」的滞后。
#[test]
fn bound_text_never_drifts_from_domain() {
    let mut f = fixture_with("hello");
    f.focus_at_end();
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    f.frame_with(vec![]);

    for i in 0..4 {
        f.enter();
        assert_eq!(
            f.app.editor_bound_text(),
            f.content(),
            "第 {i} 次回车后绑定串与领域必须逐字节相等"
        );
    }
}

/// 界面**不闪烁**的可观测代理：一次普通按键不得留下「待回设光标」。
///
/// 闪烁的机制是「每敲一键都强制 egui 重算光标/选区/撤销栈 + 触发一次
/// 额外的光标滚入视口动画」。此处断言的是它的**上游**——普通输入不产生
/// 待回设的光标（`pending_cursor` 为空），也就是「无事可做、无需重算」。
#[test]
fn plain_typing_needs_no_cursor_writeback() {
    let mut f = fixture_with("hello");
    f.focus_at_end();

    f.frame_with(vec![egui::Event::Text("a".to_owned())]);
    assert_eq!(
        f.app.pending_cursor_for_diag(),
        None,
        "普通输入后不应有待回设光标（回设即多一次强制重算 = 闪烁源）"
    );
    f.frame_with(vec![]);
    assert_eq!(f.content(), "helloa");

    f.enter();
    assert_eq!(
        f.app.pending_cursor_for_diag(),
        None,
        "普通回车同样不应触发光标回设"
    );
}

/// 守卫必须靠**超时**解除，而不是只靠 `Commit`/`Disabled`。
///
/// 构造「有 `Preedit`（进了守卫）但永远没有 `Commit`/`Disabled`」的场景：
/// 真实输入法里这很常见——用户敲了几个拼音后直接点 elsewhere 或关掉候选窗。
/// 此时若守卫是粘性的，光标回设就永久冻结，后续回车全部失效。
///
/// 故此处**不**发送任何收尾事件，只让时间流逝，然后要求回车照常工作。
#[test]
fn guard_releases_on_timeout_without_commit() {
    let mut f = fixture_with("hello");
    f.focus_at_end();

    // 有 Preedit → 进入守卫（ime_composing = true）。
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    f.frame_with(vec![]);
    for pre in ["s", "sa"] {
        f.clock.advance(100);
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit(
            pre.to_owned(),
        ))]);
        f.frame_with(vec![]);
    }
    assert!(
        f.app.ime_composing(),
        "有 Preedit 且刚续过窗口，守卫应处于生效状态"
    );

    // 用户没确认候选就走了：没有 Commit，也没有 Disabled。
    f.idle_past_guard();
    assert!(!f.app.ime_composing(), "超时后守卫必须自行解除，不能粘住");

    // 守卫解除后，回车必须立即恢复。
    f.enter();
    assert_eq!(f.content(), "hello\n");
    f.enter();
    assert_eq!(f.content(), "hello\n\n");
}

/// 对照组：完全没有 IME 事件时连按回车（基线行为，修复前后都应成立）。
#[test]
fn baseline_repeated_enter_without_any_ime_event() {
    let mut f = fixture_with("hello");
    f.focus_at_end();
    for i in 1..=4 {
        f.enter();
        assert_eq!(f.content(), format!("hello{}", "\n".repeat(i)));
    }
}

// ============================================================================
// 可见性取证（2026-10-11 用户复报：行号递增但可见内容不换行）
// ============================================================================

/// 实验 1：完全无 IME——「输入一串字符后立即回车」是否正常。
#[test]
fn diag_x1_type_then_enter_no_ime() {
    let mut f = fixture_with("");
    f.focus_at_end();
    f.frame_with(vec![egui::Event::Text("2112".to_owned())]);
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "2112\n", "实验1：无 IME 时输入后回车应正常");
    f.enter();
    assert_eq!(f.content(), "2112\n\n", "实验1：第二次回车应正常");
}

/// 实验 2：仅 Enabled——「输入一串字符后立即回车」是否正常。
#[test]
fn diag_x2_type_then_enter_after_ime_enabled() {
    let mut f = fixture_with("");
    f.focus_at_end();
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    f.frame_with(vec![]);
    f.frame_with(vec![egui::Event::Text("2112".to_owned())]);
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "2112\n", "实验2：Enabled 后输入回车应正常");
    f.enter();
    assert_eq!(f.content(), "2112\n\n", "实验2：第二次回车应正常");
}

/// 实验 3：先短文本+回车成功一次，再输入后回车（逼近 diag 失败序列）。
#[test]
fn diag_x3_second_type_then_enter_fails() {
    let mut f = fixture_with("");
    f.focus_at_end();
    f.frame_with(vec![egui::Event::Text("1".to_owned())]);
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "1\n");
    f.frame_with(vec![egui::Event::Text("2112".to_owned())]);
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "1\n2112\n", "实验3：第二次输入后的回车应正常");
}

/// 模拟**真实 Windows MS-Pinyin** 的事件流：输入文本（含一次中文组合提交、
/// Commit 后 winit 必发一条空 `Preedit` 清候选窗），随后连按回车。
/// 每一步都断言：领域内容、绑定串、**实际绘制的编辑器 galley** 三者一致。
#[test]
fn diag_realistic_ime_stream_enter_stays_visible() {
    let mut f = fixture_with("");
    f.focus_at_end();

    // 真实流程：编辑区获得焦点 → IME Enabled。
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
    f.frame_with(vec![]);

    // 数字直接上屏。
    f.frame_with(vec![egui::Event::Text("1".to_owned())]);
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "1\n");

    f.frame_with(vec![egui::Event::Text("2112".to_owned())]);
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "1\n2112\n");

    // 中文组合：Preedit 逐字母 → Commit → **winit 还会发一条空 Preedit 清候选窗**。
    for pre in ["s", "sd", "sdf"] {
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit(
            pre.to_owned(),
        ))]);
        f.frame_with(vec![]);
    }
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Commit(
        "撒打发第三方".to_owned(),
    ))]);
    f.frame_with(vec![]);
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit(String::new()))]);
    f.frame_with(vec![]);
    assert_eq!(f.content(), "1\n2112\n撒打发第三方");

    // 连按 3 次回车：每按一次，领域、绑定串、可见 galley 三方必须同步 +1 行。
    for i in 1..=3 {
        f.enter();
        let expected = format!("1\n2112\n撒打发第三方{}", "\n".repeat(i));
        let bound = f.app.editor_bound_text().to_string();
        assert_eq!(f.content(), expected, "领域内容：第 {i} 次回车后");
        assert_eq!(bound, expected, "绑定串：第 {i} 次回车后");
        assert_eq!(
            f.visible_editor_lines(),
            Some(expected.split('\n').count()),
            "可见渲染：第 {i} 次回车后编辑器 galley 必须携带全文"
        );
    }
}
