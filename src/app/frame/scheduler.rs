//! 帧循环定时任务的统一调度（T11 / ADR-009）。
//!
//! # 要解决的问题
//!
//! egui 是**事件驱动重绘**：没有输入、没有动画时根本不产生新帧。任何
//! 「等 X 毫秒后再做一件事」的逻辑只要挂在帧循环里，用户一停手它就永远
//! 等不到下一帧——表现为「定时功能完全不生效，碰巧动一下鼠标才补做一次」。
//!
//! 自动保存曾栽在这里（问题 068），当时的修法是在帧内
//! `request_repaint_after(剩余)`。但那是**逐处手写**的：每新增一个定时功能，
//! 就要重新想一遍「谁算剩余时长」「谁负责预约」「0ms 怎么钳」。结果是
//! 状态栏统计防抖、高亮停手补算、瞬态消息过期三处都**漏了预约**，
//! 只靠编辑区光标闪烁（每 0.5s 一次）兜底——编辑器一旦失焦，字色不再恢复、
//! 字数不再刷新、消息永不消失。
//!
//! # 抽象的形状：登记表 + 唯一预约出口
//!
//! 所有定时任务的真值都在别处（自动保存计时器、`StatsPace`、`HighlightPace`、
//! `StatusMessage::expires_at`）。这里不重造它们，只做两件事：
//!
//! 1. 帧内任意位置 [`arm`](FrameScheduler::arm) 声明「那一刻我需要一帧」；
//! 2. 帧末 [`flush`](FrameScheduler::flush) 取最近的那个到期点，
//!    换算成**一次** `request_repaint_after`。
//!
//! **预约只发生在 `flush` 里**：调用点没有机会「算出了剩余时长却忘了预约」，
//! 因为算剩余时长这件事本身也不再由调用点做。这是本抽象唯一的牙齿，
//! 也是它值得存在的全部理由——若哪天为了「少绕一层」在调用点直接
//! `ctx.request_repaint_after`，T11 就退回了未修状态。
//!
//! # 为什么每帧重算而不是持久化
//!
//! 登记表在 [`begin`](FrameScheduler::begin) 清空：到期点是本帧**依据当前状态
//! 重新算出**的，不是需要跨帧维护的调度状态。持久化会引入「文档关了、消息换了，
//! 旧到期点还在表里」这类陈旧登记，还得再配一套失效逻辑——而每帧重算的成本
//! 只是几次饱和减法。

use std::collections::BTreeMap;
use std::time::Duration;

/// 一帧内可能登记的到期点。
///
/// 用 enum 而不是字符串：拼错编译不过；而且两个任务若共用同一个键会**静默
/// 互相覆盖**，运行时只表现为「某个定时偶发不触发」，几乎无法归因。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Deadline {
    /// 自动保存：脏文档空闲满间隔后写草稿。
    AutoSave,
    /// 状态栏字数的防抖窗口到期。
    StatsDebounce,
    /// 编辑器高亮的防抖窗口到期（停手后补上颜色）。
    HighlightDebounce,
    /// 状态栏瞬态消息的 TTL 到期（该清掉了）。
    StatusMessage,
}

impl Deadline {
    /// 稳定的短名，只用于调试输出与测试断言（不参与调度）。
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::AutoSave => "autosave",
            Self::StatsDebounce => "stats",
            Self::HighlightDebounce => "highlight",
            Self::StatusMessage => "status_message",
        }
    }
}

/// 帧内到期点登记表（见模块文档）。
#[derive(Debug, Default)]
pub(crate) struct FrameScheduler {
    /// 本帧登记的到期点 → 绝对毫秒时刻。
    due: BTreeMap<Deadline, u64>,
    /// 本帧时钟（毫秒），由 [`Self::begin`] 写入。
    now_ms: u64,
    /// 本帧是否已 `begin`——`begin`/`flush` **成对契约**的机械判据。
    ///
    /// 由 [`Self::begin`] 置位、[`Self::flush`] 消费（断言后复位）：
    /// 只置位不复位的话，「第一帧之后的每一帧漏调 `begin`」都会静默通过，
    /// 而那恰恰是定时功能失效的形状。复位后，下一帧漏调才会被断言抓住。
    begun: bool,
}

impl FrameScheduler {
    /// 帧首：清空上一帧的登记，并记下本帧时钟。
    ///
    /// 清空是**幂等**的：某帧没跑 [`Self::flush`]（例如无头测试只画不调度）
    /// 时残留的登记会在这里丢掉，而不是留到下一帧变成幽灵预约。
    pub(crate) fn begin(&mut self, now_ms: u64) {
        self.due.clear();
        self.now_ms = now_ms;
        self.begun = true;
    }

    /// 登记「`deadline` 在 `due_ms` 时刻需要一帧」。
    ///
    /// 同一到期点被登记多次时取**更早**者：本帧多个调用点可能对同一类任务
    /// 各自算出到期点（如多篇文档的高亮），语义是「谁先到期就按谁叫醒」。
    pub(crate) fn arm(&mut self, deadline: Deadline, due_ms: u64) {
        self.due
            .entry(deadline)
            .and_modify(|current| *current = (*current).min(due_ms))
            .or_insert(due_ms);
    }

    /// 帧末：为**最近**的到期点预约一帧，然后清表。
    ///
    /// 这是全应用唯一允许调用 `request_repaint_after` 的地方（定时任务范畴；
    /// egui 自身的动画/IME 请求不在此列）。
    ///
    /// 调用前必须先跑过本帧的 [`Self::begin`]（debug 断言钉住，D-7）：
    /// 漏调 `begin` 时 `now_ms` 停在旧值，全部剩余时长随之算错，
    /// 表现正是「定时功能静默失效」——断言让它在 debug 构建里立刻炸出来。
    pub(crate) fn flush(&mut self, ctx: &egui::Context) {
        debug_assert!(
            self.begun,
            "flush 必须跟在本帧的 begin(now_ms) 之后：漏调 begin 会让时钟停在旧值，\
             定时任务静默失效（ADR-009 的 begin/flush 成对契约）"
        );
        // 消费本帧的 begin：不复位的话，断言只守得住第一帧，
        // 「第二帧起漏调」这种最常见形态反而静默通过。
        self.begun = false;
        let now = self.now_ms;
        let remaining = self.due.values().map(|due| due.saturating_sub(now)).min();
        // 先清表再预约：`request_repaint_after` 不会重入，但保持
        // 「读完即消费」的顺序，万一将来中间插了别的逻辑也不会读到旧值。
        self.due.clear();
        if let Some(ms) = remaining {
            // 已经过期（0ms）也至少等一帧的节奏：同一帧内反复请求「立刻重绘」
            // 会让静止的应用空转成忙循环（问题 068 的钳法，此处继承）。
            ctx.request_repaint_after(Duration::from_millis(ms.max(1)));
        }
    }

    /// 本帧全部登记的 `(短名, 到期时刻)`（测试与调试用；不含时钟）。
    pub(crate) fn entries(&self) -> Vec<(&'static str, u64)> {
        self.due.iter().map(|(d, at)| (d.key(), *at)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 取 `FullOutput` 里最紧的那个重绘延迟。
    ///
    /// egui 0.37 的 `FullOutput` 没有顶层 `repaint_after`：延迟按视口记在
    /// `viewport_output[..].repaint_delay` 里。取 min 与 egui 后端的
    /// 「谁先到就按谁叫醒」语义一致。
    fn repaint_delay(out: &egui::FullOutput) -> Duration {
        out.viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap_or(Duration::MAX)
    }

    /// 驱动一帧，只做调度。返回 egui 换算出的「下一次重绘延迟」。
    ///
    /// # 为什么要跑三帧
    ///
    /// egui 0.37 的前两帧自带「测量 / 字体图集」的重绘请求，`repaint_delay`
    /// 实测恒为 0（与浮层定位测试「先跑 ≥3 帧」同源）。第三帧起才反映
    /// 调用方真正预约的时长——取前两帧会把「有预约」和「没预约」看成一样，
    /// 判据就没有区分力了。
    ///
    /// 每帧都用**同一个** `now_ms` 重新 `begin` + `arm`：模拟「时钟不动的一帧」，
    /// 这样剩余时长不随帧号漂移，三次测量的期望值相同。
    ///
    /// `FullOutput` 的 `TexturesDelta` 必须清空：无头环境没有渲染后端 apply 它，
    /// 其 `Drop` 会 `debug_assert` 非空即 panic（与 `tests/support::run_ui` 同理）。
    fn frame_with(
        sched: &mut FrameScheduler,
        now_ms: u64,
        mut arm: impl FnMut(&mut FrameScheduler),
    ) -> Duration {
        let ctx = egui::Context::default();
        let mut delay = Duration::MAX;
        for _ in 0..3 {
            sched.begin(now_ms);
            arm(sched);
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| sched.flush(ui.ctx()));
            out.textures_delta.clear();
            delay = repaint_delay(&out);
        }
        delay
    }

    #[test]
    fn flush_requests_the_earliest_deadline() {
        let mut sched = FrameScheduler::default();
        let delay = frame_with(&mut sched, 1_000, |s| {
            s.arm(Deadline::StatsDebounce, 1_150);
            s.arm(Deadline::StatusMessage, 5_000);
        });
        assert_eq!(
            delay,
            Duration::from_millis(150),
            "两个到期点并存时必须按更早的那个叫醒（晚的那个会把它吞掉）"
        );
    }

    #[test]
    fn past_deadline_is_clamped_to_one_frame() {
        let mut sched = FrameScheduler::default();
        let delay = frame_with(&mut sched, 2_000, |s| {
            s.arm(Deadline::AutoSave, 1_000);
        });
        assert_eq!(
            delay,
            Duration::from_millis(1),
            "已过期的到期点必须钳到 1ms 而不是 0 —— 0ms 会让静止应用忙循环"
        );
    }

    #[test]
    fn no_deadline_means_no_repaint_request() {
        let mut sched = FrameScheduler::default();
        let delay = frame_with(&mut sched, 1_000, |_| {});
        assert_eq!(
            delay,
            Duration::MAX,
            "没有任何定时任务时不得预约帧（否则静止应用会持续重绘）"
        );
    }

    #[test]
    fn arm_keeps_the_earlier_deadline() {
        let mut sched = FrameScheduler::default();
        sched.begin(0);
        sched.arm(Deadline::HighlightDebounce, 900);
        sched.arm(Deadline::HighlightDebounce, 400);
        assert_eq!(
            sched.entries(),
            vec![(Deadline::HighlightDebounce.key(), 400)],
            "同一到期点重复登记应取更早者（多文档各自算出到期点时靠这条收敛）"
        );
    }

    #[test]
    fn flush_consumes_the_table() {
        let mut sched = FrameScheduler::default();
        let delay = frame_with(&mut sched, 0, |s| s.arm(Deadline::StatsDebounce, 150));
        assert_eq!(delay, Duration::from_millis(150));
        assert!(
            sched.entries().is_empty(),
            "flush 之后登记表必须清空：残留的到期点会在下一帧变成幽灵预约"
        );
    }

    #[test]
    fn begin_discards_a_previous_frames_arm() {
        let mut sched = FrameScheduler::default();
        sched.begin(0);
        sched.arm(Deadline::AutoSave, 10_000);
        // 模拟某帧只画不调度（无头测试的 `settle` 就是这种用法）。
        sched.begin(1_000);
        assert!(
            sched.entries().is_empty(),
            "帧首必须清空上一帧的登记，否则陈旧到期点会一直续命"
        );
    }

    /// D-7：`flush` 在整帧都没 `begin` 时必须断言炸出。
    ///
    /// 漏调 `begin` 的症状是**静默**的（时钟停在旧值 → 全部剩余时长算错，
    /// 定时功能不触发），常规断言看不出来；只有把成对契约做成 debug 断言，
    /// 测试与 debug 构建才会当场红。
    #[test]
    #[should_panic(expected = "begin")]
    fn flush_without_begin_panics_in_debug() {
        let mut sched = FrameScheduler::default();
        let ctx = egui::Context::default();
        sched.flush(&ctx);
    }

    /// D-7 的**消费语义**：`flush` 断言后复位 `begun`。
    ///
    /// 不复位的话，断言只守得住第一帧——「第二帧起漏调 `begin`」恰恰是
    /// 最常见的形态（重构时把帧首那行挪走），反而静默通过。本用例跑完
    /// 一对 begin/flush 再直接 flush，模拟下一帧漏调，必须报出来。
    #[test]
    #[should_panic(expected = "begin")]
    fn a_missed_begin_on_the_next_frame_is_also_caught() {
        let mut sched = FrameScheduler::default();
        let ctx = egui::Context::default();
        sched.begin(0);
        sched.flush(&ctx);
        sched.flush(&ctx);
    }
}
