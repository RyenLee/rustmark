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
//!
//! # 2026-10-11 复报的补充根因（本文件后半部分）
//!
//! 用户复报「行号递增但可见内容不换行」。排查锁定两处残余缺陷：
//!
//! 1. **`Commit` 续期冻结窗**：Commit 的语义是「组合结束」，egui 在 Commit
//!    分支里已自行复位 `ime_enabled`，但旧实现把它当「组合进行中」续了
//!    800ms 保护窗——提交后立刻做列表续行回车，`pending_cursor` 被冻结，
//!    egui 光标停在 `"\n"` 之后、`"2. "` 之前，下一次回车把 `\n` 插到标记
//!    前面、`transform_insert` 又续一个标记，逐次堆积：行号递增、光标附近
//!    却不见正常新行。见 `list_enter_right_after_commit_does_not_pile_markers`。
//! 2. **armed 窗口内 `repeat` 回车被 egui 剔除**：Windows 上候选窗/其他窗口
//!    抢焦点会吞掉 keyup，egui 的 `keys_down` 残留使下一次按下被判
//!    `repeat: true`，恰逢 `ime_enabled` 期被剔除。应用既已判定「无组合
//!    在进行」，就应放行。见 `enter_press_without_release_still_inserts_under_ime_enabled`。

use crate::support::clock::TestClock;
use crate::support::fake_dialog::FakeDialog;
use eframe::App as _;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use std::sync::Arc;

/// egui 0.37 起 `ImeEvent::Enabled` 已废弃、egui 不再消费（IME 开关由 egui 自管）。
/// 保留该构造器仅为**复现真实输入法发来的事件序列**以驱动旧 IME 守卫逻辑回归；
/// 用 `#[allow(deprecated)]` 压制告警。
#[allow(deprecated)]
fn ime_enabled_event() -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Enabled)
}

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

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
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {}); // 装载字库
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
        let out = crate::support::run_ui(&self.ctx, input, |ui| {
            self.app.ui(ui, &mut self.frame);
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
    f.frame_with(vec![ime_enabled_event()]);
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
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: word.to_owned(),
            active_range_chars: None,
        })]);
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

    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);
    for pre in ["s", "sa", "sad", "sadaf"] {
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: pre.to_owned(),
            active_range_chars: None,
        })]);
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

    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);
    for pre in ["a", "ai", "aidi"] {
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: pre.to_owned(),
            active_range_chars: None,
        })]);
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

    f.frame_with(vec![ime_enabled_event()]);
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
    f.frame_with(vec![ime_enabled_event()]);
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
    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);
    for pre in ["s", "sa"] {
        f.clock.advance(100);
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: pre.to_owned(),
            active_range_chars: None,
        })]);
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

/// 无 IME：输入一串字符后回车，连续两次各产生一个新行。
#[test]
fn enter_after_typing_without_ime() {
    let mut f = fixture_with("");
    f.focus_at_end();
    f.frame_with(vec![egui::Event::Text("2112".to_owned())]);
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "2112\n", "实验1：无 IME 时输入后回车应正常");
    f.enter();
    assert_eq!(f.content(), "2112\n\n", "实验1：第二次回车应正常");
}

/// 仅 Enabled（无组合）：输入后回车，连续两次各产生一个新行。
#[test]
fn enter_after_typing_with_ime_enabled_only() {
    let mut f = fixture_with("");
    f.focus_at_end();
    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);
    f.frame_with(vec![egui::Event::Text("2112".to_owned())]);
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "2112\n", "实验2：Enabled 后输入回车应正常");
    f.enter();
    assert_eq!(f.content(), "2112\n\n", "实验2：第二次回车应正常");
}

/// 先短文本+回车成功一次，再输入后回车：不得复现「第二次输入后被吞」。
#[test]
fn second_typing_then_enter_works() {
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
fn realistic_ms_pinyin_stream_enter_stays_visible() {
    let mut f = fixture_with("");
    f.focus_at_end();

    // 真实流程：编辑区获得焦点 → IME Enabled。
    f.frame_with(vec![ime_enabled_event()]);
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
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: pre.to_owned(),
            active_range_chars: None,
        })]);
        f.frame_with(vec![]);
    }
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Commit(
        "撒打发第三方".to_owned(),
    ))]);
    f.frame_with(vec![]);
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
        text: String::new(),
        active_range_chars: None,
    })]);
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

// ============================================================================
// 2026-10-11 复报根因的回归（Commit 续期冻结窗 / armed 窗口 repeat 剔除）
// ============================================================================

/// **Commit 不得续期冻结窗**：中文提交后**立刻**在列表项上回车，
/// 连续三次必须得到「续行 → 退出列表 → 空行」，不得出现标记堆积。
///
/// 修复前：Commit 把 800ms 保护窗再续一遍，第一次列表续行写入的
/// `pending_cursor` 被冻结、光标回不去（停在 `"\n"` 之后、`"2. "` 之前），
/// 第二次回车把 `\n` 插到 `"2. "` 前面且 `transform_insert` 又续一个标记，
/// 内容变成 `"1. first译文\n\n2. 2. "`——行号递增但光标附近不见正常新行，
/// 正是用户复报的症状。
///
/// 注意全程**不推进时钟**：只有冻结窗语义错误时本用例才会失败，
/// 时间一推进（>800ms）超时解冻就会把缺陷掩盖掉。
#[test]
fn list_enter_right_after_commit_does_not_pile_markers() {
    let mut f = fixture_with("1. first");
    f.focus_at_end();

    // 一次完整的中文组合：Enabled → Preedit 逐字母 → Commit。
    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);
    for pre in ["y", "yw"] {
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: pre.to_owned(),
            active_range_chars: None,
        })]);
        f.frame_with(vec![]);
    }
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Commit(
        "译文".to_owned(),
    ))]);
    f.frame_with(vec![]);
    assert_eq!(f.content(), "1. first译文", "组合应正常提交");

    // 提交后**同一时刻**（时钟零推进）连按回车。
    f.enter();
    assert_eq!(f.content(), "1. first译文\n2. ", "第 1 次回车应续出序号");
    f.enter();
    assert_eq!(
        f.content(),
        "1. first译文\n\n",
        "第 2 次回车应退出列表（删掉空标记），不得堆积出 \"2. 2. \""
    );
    f.enter();
    assert_eq!(f.content(), "1. first译文\n\n\n", "第 3 次回车只多一个空行");
}

/// **release 丢失时回车不得被吞**：Windows 上候选窗/其他窗口抢焦点会吃掉
/// keyup，egui 的 `keys_down` 残留使下一次按下被判 `repeat: true`；恰逢
/// `ime_enabled` 期（Enabled 后 50ms armed 窗口内），egui 会把它整个剔除。
/// 应用既已判定「无组合在进行」，就必须放行——抹平 repeat 让 egui
/// 当作一次普通按下处理。
///
/// 修复前：第二次按下被 `remove_ime_incompatible_events` 剔除，内容停在
/// `"hello\n"`——正是「行号不动了」的另一种形态（模型层吞键）。
#[test]
fn enter_press_without_release_still_inserts_under_ime_enabled() {
    let mut f = fixture_with("hello");
    f.focus_at_end();

    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);

    // 第 1 次按下：正常插入。release **丢失**（不发 released）。
    f.frame_with(vec![pressed_key(egui::Key::Enter)]);
    f.frame_with(vec![]);
    // 第 2 次按下：egui 判 repeat=true。全程时钟零推进 → armed 窗口未过期。
    f.frame_with(vec![pressed_key(egui::Key::Enter)]);
    f.frame_with(vec![released_key(egui::Key::Enter)]);
    f.frame_with(vec![]);
    assert_eq!(
        f.content(),
        "hello\n\n",
        "release 丢失后的第二次按下也必须换行"
    );

    // 键盘恢复正常（release 到位）后照常工作。
    f.frame_with(vec![pressed_key(egui::Key::Enter)]);
    f.frame_with(vec![released_key(egui::Key::Enter)]);
    f.frame_with(vec![]);
    assert_eq!(f.content(), "hello\n\n\n");
}

// ============================================================================
// 2026-10-12 暴力扫描发现的回归（armed 到期帧 repeat 剔除 / 组合期预编辑串被吃）
// ============================================================================

/// **armed 窗口到期的那一帧，`repeat` 回车不得被吞**。
///
/// egui 的 `remove_ime_incompatible_events` 在 `TextEdit::show` **控件内部**、
/// 按当帧仍为真的 `state.ime_enabled` 剔除全部 `repeat: true` 按键；armed
/// 到期帧补发的 `Disabled` 排在事件流尾部、要等 IME 事件排序（发生在剔除
/// **之后**）才生效，救不回那一帧。旧实现还在该帧先清零 `egui_ime_armed`
/// 再做抹平检查，抹平被跳过——release 丢失后的第二次回车被整个吞掉，
/// 内容停在 `"hello\n"`，正是「连按回车只多出一个新行」。
///
/// 修复：抹平判据改为「armed 为真 **或** 本帧即将补发 Disabled」。
#[test]
fn repeat_enter_on_armed_expiry_frame_still_inserts() {
    let mut f = fixture_with("hello");
    f.focus_at_end();

    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);

    // 第 1 次：正常按下，release **丢失**（Windows 上候选窗/其他窗口抢焦点
    // 吞掉 keyup 的真实场景）——egui 的 `keys_down` 因此残留。
    f.frame_with(vec![pressed_key(egui::Key::Enter)]);
    f.frame_with(vec![]);
    assert_eq!(f.content(), "hello\n");

    // 第 2 次：`keys_down` 残留使 egui 判 `repeat: true`，且时钟推进 100ms
    // 已超过 armed 窗口（50ms）——本帧正是 armed 到期帧。
    f.clock.advance(100);
    f.frame_with(vec![pressed_key(egui::Key::Enter)]);
    f.frame_with(vec![released_key(egui::Key::Enter)]);
    f.frame_with(vec![]);
    assert_eq!(
        f.content(),
        "hello\n\n",
        "armed 到期帧的 repeat 回车也必须换行"
    );
    assert_eq!(f.app.editor_bound_text(), "hello\n\n");

    // 第 3 次：键盘恢复正常（release 到位）后照常工作。
    f.clock.advance(100);
    f.frame_with(vec![pressed_key(egui::Key::Enter)]);
    f.frame_with(vec![released_key(egui::Key::Enter)]);
    f.frame_with(vec![]);
    assert_eq!(f.content(), "hello\n\n\n");
}

/// **组合窗内的裸按键：预编辑串转为普通文本，按键照常生效**。
///
/// egui 0.33 的 `Preedit` 会把预编辑串真实写进文本缓冲区，本应用的 diff
/// 路径随即把它同步进领域——用户看得见、领域也已认账。旧实现在 armed 窗
/// （50ms，比组合窗 800ms 短一个量级）到期后补发 `Disabled` 清零 egui 的
/// `ime_enabled`，**预编辑选区却还挂在缓冲区里**：回车的 `delete_selected`
/// 把 "ni" 整个吃掉，领域内容凭空丢失（实测 `"hello\n"`）。
///
/// 修复：armed 到期时组合窗仍新鲜就只续期、不补 `Disabled`；组合窗内到达
/// 裸按键则补一个空 `Commit("")` 折叠选区（egui 的 Commit 分支对空串的
/// 处理正是「折叠光标到 primary」），预编辑串由此转为普通文本。
#[test]
fn preedit_then_enter_keeps_preedit_as_text() {
    let mut f = fixture_with("hello");
    f.focus_at_end();

    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);
    for pre in ["n", "ni"] {
        f.clock.advance(100);
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: pre.to_owned(),
            active_range_chars: None,
        })]);
        f.frame_with(vec![]);
    }
    assert_eq!(f.content(), "helloni", "预编辑串应经 diff 路径落进领域");

    // 间隔 300ms：armed 窗（50ms）早已到期，但组合窗（800ms）仍新鲜——
    // 此时不得补发 Disabled 拆预编辑选区。
    f.clock.advance(300);
    f.frame_with(vec![]);

    // 用户不确认候选直接回车：预编辑串保留为普通文本，回车落在其后。
    f.enter();
    assert_eq!(f.content(), "helloni\n", "放弃组合的回车不应吃掉预编辑串");
    assert_eq!(f.app.editor_bound_text(), "helloni\n");
    f.enter();
    assert_eq!(f.content(), "helloni\n\n", "第二次回车紧接其后再出一行");
}

/// 组合窗内的**裸文本**同样保留预编辑串（打字不会让已可见的拼音消失）。
#[test]
fn preedit_then_typing_keeps_preedit_as_text() {
    let mut f = fixture_with("hello");
    f.focus_at_end();

    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);
    for pre in ["n", "ni"] {
        f.clock.advance(100);
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: pre.to_owned(),
            active_range_chars: None,
        })]);
        f.frame_with(vec![]);
    }

    // 组合窗内直接打一个可见字符：选区先被折叠，"ni" 保留为普通文本。
    f.clock.advance(200);
    f.frame_with(vec![egui::Event::Text("x".to_owned())]);
    f.frame_with(vec![]);
    assert_eq!(f.content(), "hellonix", "裸文本不得吃掉预编辑串");

    f.enter();
    assert_eq!(f.content(), "hellonix\n");
}

/// **守卫到期那一帧的裸按键，预编辑串也不得被吃**（2026-10-12 细网格扫描发现）。
///
/// 这是「组合期预编辑串保护」的**边界情形**：`Preedit` 之后一直没有
/// `Preedit`/`Commit`/`Disabled`，直到 800ms 守卫窗**恰好到期的那一帧**
/// 才按下一个裸键（回车）。旧实现里该帧的执行顺序是：
///
/// ```text
/// ① 守卫分支：ime_composing && now >= until → 清零 ime_composing = false
/// ② armed 分支：已 false → 补发 Disabled、清零 egui 的 ime_enabled
/// ③ 裸按键兜底：判据 `self.ime_composing` 已为 false → 整段跳过  ← 漏洞
/// ④ TextEdit 处理 Enter：delete_selected 吃掉选区里的预编辑串
/// ```
///
/// 关键在于兜底判据用错了对象：`ime_composing` 只是**我们这侧的观察窗口**，
/// 到期只说明「等够了」，不代表真实组合已结束——egui 的预编辑选区仍在
/// 缓冲区里。修复：判据改用**本帧清算之前**的状态（`composing_before`），
/// 于是「Preedit 之后无论隔多久才按裸键」都能被兜住。
#[test]
fn preedit_then_enter_on_guard_expiry_frame_keeps_text() {
    let mut f = fixture_with("基线");
    f.focus_at_end();

    f.frame_with(vec![ime_enabled_event()]);
    f.frame_with(vec![]);
    f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
        text: "sheng".to_owned(),
        active_range_chars: None,
    })]);
    f.frame_with(vec![]);
    assert_eq!(f.content(), "基线sheng", "预编辑串应落进领域");

    // 恰好跨过 800ms 守卫窗（旧实现在该帧吃掉预编辑串）。
    f.clock.advance(900);
    f.enter();

    assert_eq!(
        f.content(),
        "基线sheng\n",
        "守卫到期帧的回车不得吃掉预编辑串"
    );
    assert_eq!(f.app.editor_bound_text(), "基线sheng\n");
    f.enter();
    assert_eq!(f.content(), "基线sheng\n\n", "后续回车照常");
}

/// 守卫到期的**整段邻域**都要安全：细网格扫描两个时间边界
/// （armed 50ms / 守卫 800ms）前后各档，预编辑串一颗都不能丢。
///
/// 时间边界是此类缺陷的高发区——单点用例只能证明「某个毫秒值成立」，
/// 扫描才能证明「这个边界整体不漏」。用户复报十几次的根源正是
/// 每次只修了一个点、边界另一侧还漏。
#[test]
fn preedit_survives_enter_across_all_timing_boundaries() {
    for gap in [
        0u64, 10, 20, 30, 40, 50, 60, 70, 80, 100, 200, 400, 600, 700, 780, 790, 795, 799, 800,
        801, 805, 810, 850, 900, 1000, 1600, 5000,
    ] {
        let mut f = fixture_with("基线");
        f.focus_at_end();
        f.frame_with(vec![ime_enabled_event()]);
        f.frame_with(vec![]);
        f.frame_with(vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: "sheng".to_owned(),
            active_range_chars: None,
        })]);
        f.frame_with(vec![]);

        let before = f.content();
        f.enter_after(gap);
        let after = f.content();

        assert!(
            before.contains("sheng") && after.contains("sheng"),
            "gap={gap}ms：预编辑串被吃掉（前={before:?} 后={after:?}）"
        );
        assert!(
            after.ends_with('\n'),
            "gap={gap}ms：回车未生效（内容={after:?}）"
        );
        assert_eq!(
            f.app.editor_bound_text(),
            after,
            "gap={gap}ms：领域与绑定串必须逐字节相等"
        );
    }
}

/// **不同光标位置**的回车 + 行号栏与内容行数**严格一致**（验收标准 3/4）。
///
/// 覆盖：行中拆行、行首上插空行、同一光标连续回车、文首插入；每一步都
/// 断言「行号栏行数 == 绑定串行数 == 实际绘制的 galley 行数」。
#[test]
fn enter_at_various_cursor_positions_keeps_gutter_in_sync() {
    /// 行号栏（与绑定串同源）与可见 galley 的行数必须同时等于期望值。
    fn assert_gutter_sync(f: &Fixture, expected_lines: usize) {
        let bound = f.app.editor_bound_text().to_string();
        assert_eq!(
            bound.split('\n').count(),
            expected_lines,
            "行号栏行数（=编辑器绑定串行数）必须与内容行数严格一致"
        );
        assert_eq!(
            f.visible_editor_lines(),
            Some(expected_lines),
            "实际绘制的编辑器 galley 行数必须与行号栏一致"
        );
    }

    let mut f = fixture_with("alpha\nbeta\ngamma");
    f.focus_at_end(); // 光标在文末
    assert_gutter_sync(&f, 3);

    // ① 行中：End 后左移 2 → "gam|ma"，回车应拆成两行。
    for _ in 0..2 {
        f.frame_with(vec![pressed_key(egui::Key::ArrowLeft)]);
        f.frame_with(vec![released_key(egui::Key::ArrowLeft)]);
    }
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(f.content(), "alpha\nbeta\ngam\nma", "① 行中回车应拆行");
    assert_gutter_sync(&f, 4);

    // ② 行首：拆行后光标落在 "ma" 行首，回车应在其上插一个空行。
    f.enter();
    assert_eq!(
        f.content(),
        "alpha\nbeta\ngam\n\nma",
        "② 行首回车应上插空行"
    );
    assert_gutter_sync(&f, 5);

    // ③ 同一光标连续回车：每次恰好 +1 行。
    f.enter();
    assert_eq!(f.content(), "alpha\nbeta\ngam\n\n\nma");
    assert_gutter_sync(&f, 6);

    // ④ 文首：上移 5 行（"ma" → 空行 → 空行 → "gam" → "beta" → "alpha"），
    // 回车顶开首行。
    for _ in 0..5 {
        f.frame_with(vec![pressed_key(egui::Key::ArrowUp)]);
        f.frame_with(vec![released_key(egui::Key::ArrowUp)]);
    }
    f.frame_with(vec![]);
    f.enter();
    assert_eq!(
        f.content(),
        "\nalpha\nbeta\ngam\n\n\nma",
        "④ 文首回车应在最前插入空行"
    );
    assert_gutter_sync(&f, 7);
}
