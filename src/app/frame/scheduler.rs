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
//!
//! # 顺带的职责：帧耗时观测（D-5）
//!
//! 本结构是**唯一**在每帧首尾都会被调到的地方（[`begin`](FrameScheduler::begin)
//! 与 [`flush`](FrameScheduler::flush)），所以帧耗时观测挂在这里不需要新增
//! 任何调用点，也不会漏帧。观测分两层：
//!
//! 1. **超预算告警**：一帧超过 [`FRAME_BUDGET`]（16ms，`NFR-PERF-04` 的一帧口径）
//!    时 [`log::warn!`] 一条，带**分阶段归因**（状态更新 / 绘制 / 未归因）、
//!    **本帧到期的定时任务**与限流信息。此前排障只能靠用户反馈「卡了一下」再
//!    临时插桩，而卡顿往往不可复现。
//! 2. **常态指标**：每 [`STATS_REPORT_EVERY`] 帧 [`log::debug!`] 一条
//!    （平均 / 最大 / 超预算帧数），`RUST_LOG=rustmark=debug` 即可取——
//!    默认 `warn` 级下静默，不占日志。
//!
//! 口径边界，写清楚免得误读：计时区间是 **`begin` → `flush`**，即「状态更新 +
//! 绘制」两段；`flush` 之后的帧末动作（草稿恢复提示、截图回路）与 egui 自身的
//! tessellate / paint **不在其中**。要归因到具体控件仍需
//! `tests/presentation/textedit_cost_attribution_test.rs` 那套测试级手段，
//! 这里给的是**线上常态**的粗粒度告警，两者互补。
//!
//! **只在 release 构建输出**（[`observability_enabled`]）：debug 构建未优化、
//! 断言全开，帧耗时本来就不能代表真实性能，16ms 门槛会**全线超限**——每条都
//! 告警的日志等于没有告警，只会训练人忽略它。判定与统计在任何 profile 下都跑
//! （测试要能断言这些会计），只有**输出**按 profile 关掉。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// 一帧的耗时预算：`NFR-PERF-04` 的「编辑响应 ≤ 16ms（一帧）」。
///
/// 超过它不代表一定卡顿（60fps 下掉一帧几乎无感），但连续超过就是
/// 「用户能感觉到」的起点——告警门槛取这个值，是为了让日志里的每条
/// 记录都对应一个可对照的验收指标，而不是随手拍的数。
pub(crate) const FRAME_BUDGET: Duration = Duration::from_millis(16);

/// 慢帧告警的限流窗口（帧数）：持续慢帧时每约 2s（60fps）只留一条。
///
/// 无限流的话，一台跑不动的机器会以 60 条/秒的速度刷屏，反而把
/// 「第一次变慢」这个关键信号淹掉。被压掉的帧数会累计在下一条里报出。
const OVERRUN_WARN_EVERY: u64 = 120;

/// 常态指标的汇报周期（帧数，约 10s@60fps）。
const STATS_REPORT_EVERY: u64 = 600;

/// 帧内阶段——**归因维度**，不是执行顺序的强制约束。
///
/// 只切两段是刻意的：拆得更细要在视图层到处插桩（每处都是一个长期成本），
/// 而「状态更新 vs 绘制」已经能把「慢在算还是在画」这个最关键的岔口分开。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FramePhase {
    /// 状态更新：主题 / 字体 / 对话框轮询 / 快捷键 / 自动保存 / 到期补算。
    StateUpdate,
    /// 绘制：`main_view::show` 的整棵视图树。
    Draw,
}

/// 阶段个数（数组长度用它，别用 `ALL.len()`——后者在常量泛型位置可读性差）。
const PHASE_COUNT: usize = 2;

impl FramePhase {
    /// 全部阶段（索引即 `as usize`）：新增阶段要同步 [`PHASE_COUNT`] 与
    /// [`Self::label`]，否则归因数组越界或两段名字对不上。
    pub(crate) const ALL: [Self; PHASE_COUNT] = [Self::StateUpdate, Self::Draw];

    /// 归因数组下标。
    const fn index(self) -> usize {
        self as usize
    }

    /// 中文短名（告警文案用，避免排障者再对照英文键翻译一遍）。
    const fn label(self) -> &'static str {
        match self {
            Self::StateUpdate => "状态更新",
            Self::Draw => "绘制",
        }
    }
}

/// 一帧的耗时统计（常态指标的真值）。
#[derive(Debug, Default)]
struct FrameStats {
    /// 已观测帧数。
    frames: u64,
    /// 其中超预算的帧数。
    over_budget: u64,
    /// 累计耗时（算平均用）。
    total: Duration,
    /// 单帧最大耗时。
    max: Duration,
}

/// 一次超预算的完整归因（[`FrameScheduler::note_frame_cost`] 的产物）。
///
/// 返回值而不是直接写日志，是为了让判据可测：门禁里断言的是这份
/// 结构（阈值、归因、限流会计），不是日志字符串。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrameOverrun {
    /// 本帧总耗时。
    pub(crate) total: Duration,
    /// 各阶段耗时，下标与 [`FramePhase::ALL`] 对齐。
    pub(crate) phases: [Duration; PHASE_COUNT],
    /// 总耗时里没被任何阶段解释掉的部分（调用方漏 `lap` 时的自证）。
    pub(crate) unattributed: Duration,
    /// 本帧已到期的定时任务短名（重帧的常见成因）。
    pub(crate) fired: Vec<&'static str>,
    /// 本帧登记、但还没到期的任务个数。
    pub(crate) pending: usize,
    /// 本条之前被限流压掉的超预算帧数。
    pub(crate) suppressed: u64,
}

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
    /// 帧首墙钟（D-5）。[`Self::flush`] 取走；只 `begin` 不 `flush` 的
    /// 无头测试会让它残留，下次 `begin` 覆盖即可（不参与任何调度决策）。
    started: Option<Instant>,
    /// 上一次 [`Self::lap`] 的时刻（帧首也作为起点）。
    ///
    /// 用「分段计时」而不是让调用方各自 `Instant::now()`：调用点只需在
    /// 阶段边界上调一次 `lap`，不必自己管起点，也就没有「忘了重置起点」
    /// 「两个阶段重叠计时」这类错法。
    last_lap: Option<Instant>,
    /// 本帧各阶段累计耗时，下标 = [`FramePhase::index`]。
    phases: [Duration; PHASE_COUNT],
    /// 跨帧统计（常态指标）。
    stats: FrameStats,
    /// 上一次慢帧告警时的帧号（限流用）；`None` = 还没告警过。
    last_warn_frame: Option<u64>,
    /// 自上次告警以来被限流压掉的超预算帧数。
    suppressed_since_warn: u64,
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
        // D-5：帧首同时是计时起点。只取一次 `Instant::now()`，
        // 「总耗时」与「第一阶段」共用一个起点，两者才不会差出几微秒。
        let start = Instant::now();
        self.started = Some(start);
        self.last_lap = Some(start);
        self.phases = [Duration::ZERO; PHASE_COUNT];
    }

    /// 记下「自上次 `lap`（或帧首）到此刻」的耗时，归到 `phase` 名下（D-5）。
    ///
    /// 调用点在阶段**结束处**各调一次（状态更新末尾、`main_view::show` 之后）。
    /// 漏调不会让总耗时失真——[`Self::flush`] 用的是帧首到帧末的墙钟，
    /// 漏掉的部分会落进 `unattributed` 而不是被吞掉。
    pub(crate) fn lap(&mut self, phase: FramePhase) {
        let now = Instant::now();
        if let Some(since) = self.last_lap {
            self.phases[phase.index()] += now.saturating_duration_since(since);
        }
        self.last_lap = Some(now);
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
    ///
    /// D-5 起顺带结掉本帧的耗时观测：取走帧首时刻算总耗时，交给
    /// [`Self::note_frame_cost`] 判定是否告警。
    ///
    /// 返回值是本帧的归因（**仅在真的写了一条告警时**为 `Some`）：生产调用点
    /// 丢弃即可，测试靠它钉住「归因取的是清表**之前**的到期点」这条顺序。
    pub(crate) fn flush(&mut self, ctx: &egui::Context) -> Option<FrameOverrun> {
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
        // D-5：耗时观测必须在清表**之前**结账——归因要读 `due`（本帧到期的
        // 定时任务），清表后读到的永远是空表。计时与预约彼此无关，顺序不影响
        // 调度语义；放在前面只是为了让归因拿得到数据。
        let overrun = self
            .started
            .take()
            .map(|start| start.elapsed())
            .and_then(|cost| self.note_frame_cost(cost));
        // 先清表再预约：`request_repaint_after` 不会重入，但保持
        // 「读完即消费」的顺序，万一将来中间插了别的逻辑也不会读到旧值。
        self.due.clear();
        if let Some(ms) = remaining {
            // 已经过期（0ms）也至少等一帧的节奏：同一帧内反复请求「立刻重绘」
            // 会让静止的应用空转成忙循环（问题 068 的钳法，此处继承）。
            ctx.request_repaint_after(Duration::from_millis(ms.max(1)));
        }
        overrun
    }

    /// 记一帧的耗时：更新统计 → 超预算时产出归因并（限流地）告警。
    ///
    /// 独立成函数有两个理由：`flush` 里只管「取时间」，判定逻辑可以脱离
    /// egui 上下文被测试直接喂数（真等 16ms 才能测超预算的话，判据会慢且抖）；
    /// 限流与统计的会计也只有一个落点。
    ///
    /// 返回 `Some` 表示**这一帧真的写了一条告警**；被限流压掉时返回 `None`
    /// （帧数仍计入统计，压掉多少会累计到下一条告警里）。
    pub(crate) fn note_frame_cost(&mut self, cost: Duration) -> Option<FrameOverrun> {
        self.stats.frames += 1;
        self.stats.total += cost;
        if cost > self.stats.max {
            self.stats.max = cost;
        }
        let report = if cost > FRAME_BUDGET {
            self.stats.over_budget += 1;
            let mut report = self.overrun(cost);
            if self.should_warn() {
                self.last_warn_frame = Some(self.stats.frames);
                report.suppressed = std::mem::take(&mut self.suppressed_since_warn);
                if observability_enabled() {
                    log::warn!(
                        "帧耗时 {:.1}ms 超预算 {:.0}ms：{} / 未归因 {:.1}ms；本帧到期任务 [{}]，\
                     待命 {} 个；此前另有 {} 帧超预算已限流（每 {} 帧最多一条）",
                        millis(cost),
                        millis(FRAME_BUDGET),
                        phase_summary(&report.phases),
                        millis(report.unattributed),
                        if report.fired.is_empty() {
                            "无".to_string()
                        } else {
                            report.fired.join(", ")
                        },
                        report.pending,
                        report.suppressed,
                        OVERRUN_WARN_EVERY,
                    );
                }
                Some(report)
            } else {
                self.suppressed_since_warn += 1;
                None
            }
        } else {
            None
        };
        self.report_stats_periodically();
        report
    }

    /// 把本帧的原始数据整理成归因结构（不含限流会计）。
    fn overrun(&self, cost: Duration) -> FrameOverrun {
        self.overrun_with(cost, self.phases)
    }

    /// [`Self::overrun`] 的可注入形态：阶段耗时由参数给。
    ///
    /// 分开是为了让「未归因 = 总耗时 − 各阶段之和」这条算术能脱离墙钟被断言
    /// ——否则测试要么真等 16ms（慢且抖），要么只能自设阶段再断言同值（同义反复）。
    fn overrun_with(&self, cost: Duration, phases: [Duration; PHASE_COUNT]) -> FrameOverrun {
        // 归因线索：本帧**已到期**的定时任务。重帧与「自动保存 + 高亮补算」
        // 撞在同一帧是常见成因，日志里带上这一条，排障者不必再去猜。
        let fired: Vec<&'static str> = self
            .due
            .iter()
            .filter(|(_, due)| **due <= self.now_ms)
            .map(|(deadline, _)| deadline.key())
            .collect();
        let attributed: Duration = phases.iter().sum();
        FrameOverrun {
            total: cost,
            phases,
            unattributed: cost.saturating_sub(attributed),
            pending: self.due.len().saturating_sub(fired.len()),
            fired,
            suppressed: 0,
        }
    }

    /// 限流判定：首次超预算立即报，之后每 [`OVERRUN_WARN_EVERY`] 帧最多一条。
    fn should_warn(&self) -> bool {
        match self.last_warn_frame {
            None => true,
            Some(last) => self.stats.frames.saturating_sub(last) >= OVERRUN_WARN_EVERY,
        }
    }

    /// 每 [`STATS_REPORT_EVERY`] 帧吐一条常态指标（默认 `warn` 级下不可见）。
    ///
    /// 这条是给「想主动看趋势」的人用的：把 `RUST_LOG` 调到 debug，
    /// 十分钟后就能拿到平均 / 最大 / 超预算占比，而不用先复现卡顿。
    fn report_stats_periodically(&self) {
        if !self.stats.frames.is_multiple_of(STATS_REPORT_EVERY) {
            return;
        }
        if !observability_enabled() {
            return;
        }
        let frames = self.stats.frames;
        let avg = self.stats.total / frames.max(1) as u32;
        log::debug!(
            "帧耗时常态指标（累计 {frames} 帧）：平均 {:.2}ms / 最大 {:.1}ms / 超预算 {} 帧（{:.1}%）",
            millis(avg),
            millis(self.stats.max),
            self.stats.over_budget,
            self.stats.over_budget as f64 * 100.0 / frames as f64,
        );
    }

    /// 本帧全部登记的 `(短名, 到期时刻)`（测试与调试用；不含时钟）。
    pub(crate) fn entries(&self) -> Vec<(&'static str, u64)> {
        self.due.iter().map(|(d, at)| (d.key(), *at)).collect()
    }
}

/// 是否输出帧耗时观测日志：仅 release 构建。
///
/// 判定逻辑与统计**不**受它影响（debug 下照样记账，测试靠返回值断言）；
/// 只有 `log::warn!` / `log::debug!` 两处输出用它把关。理由见模块文档末段。
const fn observability_enabled() -> bool {
    !cfg!(debug_assertions)
}

/// `Duration` → 毫秒（浮点，日志里保留一位小数比 `{:?}` 好读）。
fn millis(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

/// 「状态更新 3.1ms / 绘制 21.4ms」——告警文案里的分阶段归因。
fn phase_summary(phases: &[Duration; PHASE_COUNT]) -> String {
    FramePhase::ALL
        .iter()
        .map(|phase| format!("{} {:.1}ms", phase.label(), millis(phases[phase.index()])))
        .collect::<Vec<_>>()
        .join(" / ")
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
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                sched.flush(ui.ctx());
            });
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
    ///
    /// ⚠ 只在 debug 构建运行：panic 来自 `debug_assert!`（第 264 行），
    /// release 下被编译器优化掉，不会 panic。
    #[test]
    #[cfg(debug_assertions)]
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
    ///
    /// ⚠ 只在 debug 构建运行：同 `flush_without_begin_panics_in_debug`，
    /// panic 来自 `debug_assert!`，release 下不会触发。
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "begin")]
    fn a_missed_begin_on_the_next_frame_is_also_caught() {
        let mut sched = FrameScheduler::default();
        let ctx = egui::Context::default();
        sched.begin(0);
        sched.flush(&ctx);
        sched.flush(&ctx);
    }

    // ---- D-5：帧耗时观测 ----

    /// 未超预算的帧只进统计，不产生告警。
    #[test]
    fn fast_frames_are_counted_but_silent() {
        let mut sched = FrameScheduler::default();
        assert!(
            sched.note_frame_cost(Duration::from_millis(8)).is_none(),
            "8ms 未超 16ms 预算，不该告警"
        );
        assert_eq!(sched.stats.frames, 1, "没告警也必须计入常态统计");
        assert_eq!(sched.stats.over_budget, 0);
        assert_eq!(sched.stats.max, Duration::from_millis(8));
    }

    /// 预算边界是**严格大于**：恰好 16ms 不算超预算。
    ///
    /// 卡在边界上的采样若判成超预算，日志里会出现大量「刚好一帧」的假阳性。
    #[test]
    fn budget_boundary_is_exclusive() {
        let mut sched = FrameScheduler::default();
        assert!(sched.note_frame_cost(FRAME_BUDGET).is_none());
        assert!(
            sched
                .note_frame_cost(FRAME_BUDGET + Duration::from_micros(1))
                .is_some(),
            "超出一微秒就该报——边界取严，宁可多报不可漏报"
        );
    }

    /// 归因算术：未归因 = 总耗时 − 各阶段之和；到期任务与待命任务分开列。
    #[test]
    fn overrun_attributes_phases_and_deadlines() {
        let mut sched = FrameScheduler::default();
        sched.begin(1_000);
        sched.arm(Deadline::HighlightDebounce, 900); // 已到期
        sched.arm(Deadline::StatusMessage, 5_000); // 还没到
        let report = sched.overrun_with(
            Duration::from_millis(25),
            [Duration::from_millis(3), Duration::from_millis(20)],
        );
        assert_eq!(
            report.unattributed,
            Duration::from_millis(2),
            "25 − (3 + 20) = 2ms 的尾巴必须显式报出，不能被悄悄吞掉"
        );
        assert_eq!(
            report.fired,
            vec!["highlight"],
            "只有到期点 ≤ 本帧时钟的任务算「本帧触发」"
        );
        assert_eq!(report.pending, 1, "未到期的那个应计入待命");
    }

    /// 阶段耗时打点对真实时钟有效（只断言下界，避免机器快慢导致抖动）。
    #[test]
    fn lap_records_elapsed_against_the_wall_clock() {
        let mut sched = FrameScheduler::default();
        sched.begin(0);
        std::thread::sleep(Duration::from_millis(2));
        sched.lap(FramePhase::StateUpdate);
        let recorded = sched.phases[FramePhase::StateUpdate.index()];
        assert!(
            recorded >= Duration::from_millis(1),
            "睡了 2ms，状态更新阶段至少要记到 1ms，实际 {recorded:?}"
        );
        assert!(
            recorded < Duration::from_secs(5),
            "不能把整段测试耗时都记进来（说明起点没在 lap 后重置）：{recorded:?}"
        );
    }

    /// 限流：首次慢帧立刻报，其后每 `OVERRUN_WARN_EVERY` 帧才报一条，
    /// 被压掉的帧数累计到下一条里——持续慢的机器不能把日志刷爆。
    #[test]
    fn repeated_overruns_are_rate_limited() {
        let mut sched = FrameScheduler::default();
        let first = sched
            .note_frame_cost(Duration::from_millis(30))
            .expect("首帧超预算必须立刻报");
        assert_eq!(first.suppressed, 0);

        for _ in 1..OVERRUN_WARN_EVERY {
            assert!(
                sched.note_frame_cost(Duration::from_millis(30)).is_none(),
                "限流窗口内的慢帧不逐条告警"
            );
        }
        let next = sched
            .note_frame_cost(Duration::from_millis(30))
            .expect("窗口到期后应恢复告警");
        assert_eq!(
            next.suppressed,
            OVERRUN_WARN_EVERY - 1,
            "被压掉的 {} 帧要在这条里交代清楚",
            OVERRUN_WARN_EVERY - 1
        );
        assert_eq!(
            sched.stats.over_budget,
            OVERRUN_WARN_EVERY + 1,
            "限流只影响日志，不影响统计口径"
        );
    }

    /// 输出开关只关日志，不关会计。
    ///
    /// 这条钉住「别把整个 `note_frame_cost` 一起 cfg 掉」：
    /// 一旦那样做，debug 下的统计与归因全空，测试会先红。
    ///
    /// debug / release 构建 observability 状态相反（见
    /// [`observability_enabled`]），但**会计逻辑在两边都必须跑**——
    /// 本断言按构建分支，两边各自验证自己应有的开关状态，再共同验证
    /// `note_frame_cost` 的产出行为不受开关影响。
    #[test]
    fn output_gate_does_not_affect_accounting() {
        #[cfg(debug_assertions)]
        assert!(
            !observability_enabled(),
            "debug 构建观测输出开关应为关闭（只关日志，不关会计）"
        );
        #[cfg(not(debug_assertions))]
        assert!(observability_enabled(), "release 构建观测输出开关应为开启");
        let mut sched = FrameScheduler::default();
        assert!(
            sched.note_frame_cost(Duration::from_millis(25)).is_some(),
            "开关关的是日志输出，不是归因产出"
        );
        assert_eq!(sched.stats.frames, 1);
        assert_eq!(sched.stats.over_budget, 1);
    }

    /// `flush` 真的把每帧耗时记进去了（wiring 检查：漏接时本用例面对 0 帧）。
    #[test]
    fn flush_feeds_every_frame_into_the_stats() {
        let mut sched = FrameScheduler::default();
        let _ = frame_with(&mut sched, 1_000, |_| {});
        assert_eq!(
            sched.stats.frames, 3,
            "`frame_with` 跑三帧，统计必须正好记三帧（flush 里漏取耗时就会是 0）"
        );
    }

    /// 归因取的是清表**之前**的到期点。
    ///
    /// 先清表再结账的话 `fired` 会恒为空——生产里不报错，只表现为
    /// 「每条慢帧日志都说本帧到期任务 [无]」，归因线索静默失效。
    /// 这里把帧首时刻往前挪 50ms 来模拟慢帧（不睡真时间，判据不抖）。
    #[test]
    fn flush_attributes_deadlines_before_clearing_the_table() {
        let mut sched = FrameScheduler::default();
        sched.begin(1_000);
        sched.arm(Deadline::AutoSave, 900); // 已到期
        sched.started = Some(Instant::now() - Duration::from_millis(50));
        let report = sched
            .flush(&egui::Context::default())
            .expect("50ms 的帧必须触发告警");
        assert_eq!(
            report.fired,
            vec!["autosave"],
            "到期任务必须在清表之前被读到，否则归因永远为空"
        );
        assert!(
            sched.entries().is_empty(),
            "flush 之后登记表仍须清空（原契约不变）"
        );
    }
}
