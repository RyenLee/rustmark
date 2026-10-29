//! 语法高亮的**后台补算**（P1：把「停手后一次性全文重算」搬出帧循环）。
//!
//! # 为什么要搬
//!
//! 防抖（OPT-011 ①）把「打字期逐字解析」消掉了，但停手后那次**全文**解析
//! 仍然留在帧里：10KB 文档一次 syntect 解析实测 ~64ms（`docs/spec/metrics.md`
//! §3.2.11），那一帧必然掉帧。代价并不会因为「只发生一次」而消失——
//! 它只是从「每击键一次」变成「每次停手一次」。
//!
//! 本模块把这次解析交给一个常驻后台线程：帧内只构造请求、投递、立刻返回
//! 缓存里的旧片段（通常为空），颜色在结果回送后的那一两帧里出现。
//!
//! # 为什么是 `std::thread` + `mpsc`（而不是运行时）
//!
//! 与 [`DialogRunner`](../../../../../../src/app/dialog_runner.rs) 同一个理由：
//! 项目没有 async 运行时（ADR-001 只选 egui/wgpu），为一个 64ms 的纯计算
//! 引入 tokio / async-std 不划算。`std::thread` + `mpsc` 已经够了。
//!
//! # 为什么是**常驻**线程而不是「一次请求一个线程」
//!
//! 增量会话 [`HighlightSession`] 要跨版本复用（同一文档连续编辑只重解变更行），
//! 它持有解析状态与整篇检查点，**必须活在线程里**：每次新建线程就每次全量，
//! 增量收益直接归零。常驻线程顺便让「一次请求一个线程」的 spawn 成本消失。
//!
//! # 为什么会话归线程独占而不是加锁共享
//!
//! `MarkdownAppService::clear_cache`（切主题 / 关文档）也要动会话表。若会话表
//! 放在 `Mutex` 里、解析期持锁，`clear_cache` 最坏要等一次解析（64ms）——
//! 正好是本模块要消灭的那类卡顿，只不过换了个入口。故会话表**只属于** worker
//! 线程，外部清缓存走 [`Command::Clear`]，投递即返回（微秒级）。
//!
//! 代价是清缓存变成**异步**的：`clear_cache` 返回时会话可能还没释放。
//! 语义上无碍——会话只是解析状态，之后的重建是全量（`advance` 首次即全量），
//! 不会读到陈旧片段；命中的缓存条目本来就已同步失效。
//!
//! # 陈旧结果：靠 `revision` 丢弃，不靠时序
//!
//! 请求与结果都带文档版本号。用户在解析期间又改了文档时：
//! 1. 回送的结果 `revision` 与文档当前版本不等 → 丢弃（片段区间会错位，
//!    排版层按错误边界切片会 panic）；
//! 2. 同一文档**在途**期间不再分发新请求（[`HighlightWorker::try_begin`]）——
//!    宁可晚一帧，不排队堆解析；被丢弃后下一帧自然会重新分发。
//!
//! # 失败降级
//!
//! 两种失败都不允许「永久无色」：
//! - **线程起不来 / 已死**（spawn 失败、解析 panic 后通道断开）：[`HighlightWorker::dispatch`]
//!   返回 `false`，调用方回退到同步解析（慢，但正确）；
//! - **解析报错**：结果里带 `Err`，帧内只记 `log::warn!` 并保持无片段。
//!   帧循环没有能把错误弹给用户的位置（高亮是每帧的隐式查询），
//!   而这里报错基本等价于「这套语法不可用」，降级为纯文本是唯一有意义的处理。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::application::dto::{RangeView, SpanView};
use crate::domain::markdown::services::{HighlightSession, SyntaxHighlighter};
use crate::shared::error::AppError;
use crate::shared::geometry::TextRange;
use crate::shared::ids::DocumentId;

use super::lock_or_recover;
use super::span_cache::empty_spans;
use super::view_mapper::span_style_str;

/// 有在途请求时，帧循环轮询结果的间隔（毫秒）。
///
/// # 取值理由
///
/// 解析要几十毫秒，期间必须有人叫醒帧循环去取结果，否则颜色要等下一次
/// 用户输入才出现（失焦时是永久无色）。但**不能**取 1ms：那会让一次 64ms
/// 解析变成 64 次空转唤醒。取 4ms：
/// - 颜色延迟 ≤4ms，远小于一帧（16ms），观感上「停手就上色」；
/// - 64ms 解析只多出约 16 次唤醒，每次成本是一次 `try_recv`（未就绪即返回）。
pub(super) const RESULT_POLL_INTERVAL_MS: u64 = 4;

/// 一次后台补算请求。
///
/// `content` 是**快照**：帧内取一次全文（10KB 拷贝 ~µs），之后解析不再碰领域。
/// 不在 worker 里回调 `provider` 取文本——那会把领域锁带进后台线程，
/// 并与「本帧看到的版本」脱钩。
pub(super) struct HighlightRequest {
    pub(super) doc_id: DocumentId,
    pub(super) revision: u64,
    pub(super) lang: Option<String>,
    pub(super) range: TextRange,
    pub(super) content: String,
}

/// 一次后台补算的结果。
pub(super) struct HighlightResult {
    pub(super) doc_id: DocumentId,
    pub(super) revision: u64,
    pub(super) lang: Option<String>,
    pub(super) range: TextRange,
    /// 成功时是**未裁剪**好的片段视图（已按 `range` 过滤，见 [`to_span_views`]）。
    pub(super) outcome: Result<Arc<Vec<SpanView>>, AppError>,
    /// 本次解析的真实耗时（毫秒）。
    ///
    /// 帧耗时不再包含它，这份数字就是「成本去哪了」的唯一证据：
    /// `measure_editor_frame_highlight_cost` 用它登记真实解析基线，
    /// 而不是把「帧变快了」当成「成本消失了」。
    pub(super) elapsed_ms: f64,
}

/// worker 线程收到的命令。
enum Command {
    /// 解析一篇文档（携带全文快照）。
    Parse(Box<HighlightRequest>),
    /// 释放会话：`Some` 精确到文档（关文档），`None` 全清（切主题）。
    Clear(Option<DocumentId>),
}

/// 后台补算线程的**句柄**（线程本体与它持有的会话在 [`run`] 里）。
///
/// 由 [`MarkdownAppService`](super::MarkdownAppService) 持有；所有方法都是
/// `&self`（内部用 `Mutex`），故 `MarkdownAppService` 仍满足 `Send + Sync`。
pub(super) struct HighlightWorker {
    /// 两条通道（`mpsc` 的 `Sender` / `Receiver` 都不是 `Sync`，故合起来上锁）。
    ///
    /// 锁本身无争用：只有 UI 线程会 `send` / `try_recv`，后台线程持有的是
    /// 另一端。加锁只是为了让 `MarkdownAppService: Sync` 这条编译期契约成立。
    channels: Mutex<Channels>,
    /// 在途请求：文档 → 已分发但结果未落库的版本号。
    ///
    /// 除了「不重复分发」之外，`in_flight_count()` 还是帧循环预约下一次
    /// 结果的唯一依据（见 [`RESULT_POLL_INTERVAL_MS`]）。
    in_flight: Mutex<HashMap<DocumentId, u64>>,
    /// 解析统计（测试与调试面板用；也是「真实成本仍被测量」的出口）。
    stats: Mutex<ParseStats>,
    /// 线程已死的标记（结果通道断开时置位）。
    ///
    /// 没有它，解析 panic 之后 [`Self::is_available`] 仍为真：每帧都会白取一次
    /// 全文再投递，而投递必然失败；更糟的是在途登记永远等不到结果，
    /// 帧循环会以 4ms 周期空转到世界尽头。置位后直接走同步回退。
    dead: AtomicBool,
}

/// 两条单向通道的持有者（见 [`HighlightWorker::channels`]）。
struct Channels {
    /// 命令发送端；`None` = 后台线程起不来（已死），调用方须回退同步解析。
    tx: Option<Sender<Command>>,
    /// 结果接收端（每帧 `try_recv`）。
    rx: Receiver<HighlightResult>,
}

/// 后台解析的累计统计。
#[derive(Default, Clone, Copy)]
struct ParseStats {
    /// 完成的解析次数（含被丢弃结果的解析——成本确实付了）。
    count: u64,
    /// 最近一次解析耗时（毫秒）。
    last_ms: Option<f64>,
}

impl HighlightWorker {
    /// 起一个后台线程，并把它要用的高亮器交给它。
    ///
    /// # 为什么在构造时起线程（而不是首次请求时懒起）
    ///
    /// 懒起要在 `highlight()` 里维护「有没有初始化过」的额外状态，而
    /// `MarkdownAppService::new` 本来就在启动路径上——一次 `spawn` 的开销
    /// （几十 µs）淹没在窗口初始化里，不值得为它引入一个状态位。
    pub(super) fn spawn(highlighter: Arc<dyn SyntaxHighlighter>) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (res_tx, res_rx) = channel::<HighlightResult>();
        let tx = match std::thread::Builder::new()
            .name("rustmark-highlight".to_string())
            .spawn(move || run(highlighter, cmd_rx, res_tx))
        {
            Ok(_handle) => Some(cmd_tx),
            Err(e) => {
                // 起不来就降级同步解析（`dispatch` 返回 false）。宁可停手那帧
                // 慢 64ms，也不能让语法色永远不出现。
                log::warn!("高亮后台线程启动失败，已回退为帧内同步解析：{e}");
                None
            }
        };
        Self {
            channels: Mutex::new(Channels { tx, rx: res_rx }),
            in_flight: Mutex::new(HashMap::new()),
            stats: Mutex::new(ParseStats::default()),
            dead: AtomicBool::new(false),
        }
    }

    /// 是否还能接活（线程活着）。`false` 时调用方必须回退同步解析。
    pub(super) fn is_available(&self) -> bool {
        !self.dead.load(Ordering::Relaxed) && lock_or_recover(&self.channels).tx.is_some()
    }

    /// 标记后台线程已死，并清掉全部在途登记（它们的结果不会再来了）。
    fn mark_dead(&self) {
        self.dead.store(true, Ordering::Relaxed);
        lock_or_recover(&self.in_flight).clear();
    }

    /// 登记一次在途请求；该文档已有在途请求时返回 `false`（不重复分发）。
    ///
    /// 与 [`Self::dispatch`] 分开是为了让「检查 + 登记」在同一把锁里完成：
    /// 两帧连着重算时（解析未回、新版本已到）不会双发。
    pub(super) fn try_begin(&self, doc_id: DocumentId, revision: u64) -> bool {
        let mut in_flight = lock_or_recover(&self.in_flight);
        if in_flight.contains_key(&doc_id) {
            return false;
        }
        in_flight.insert(doc_id, revision);
        true
    }

    /// 投递请求；线程不可用（起不来 / 已死）时返回 `false` 并清掉在途登记。
    ///
    /// 失败时**不**把请求还回去：调用方要走的是同步回退，它自己会重新取一次
    /// 全文。让签名保持 `bool` 比为了省一次（只会在后台线程已死时发生的）
    /// 拷贝而把消息从 `SendError` 里拆出来更简单。
    pub(super) fn dispatch(&self, request: HighlightRequest) -> bool {
        let channels = lock_or_recover(&self.channels);
        let Some(tx) = channels.tx.as_ref() else {
            return false;
        };
        if tx.send(Command::Parse(Box::new(request))).is_err() {
            // 通道断开 = 线程已退出（解析 panic）。标记死亡并让调用方回退。
            self.mark_dead();
            return false;
        }
        true
    }

    /// 撤销在途登记（取全文失败 / 投递失败时收尾，避免该文档再也分发不出去）。
    pub(super) fn abort(&self, doc_id: DocumentId) {
        lock_or_recover(&self.in_flight).remove(&doc_id);
    }

    /// 请求释放会话（异步；投递即返回）。
    pub(super) fn request_clear(&self, doc_id: Option<DocumentId>) {
        let channels = lock_or_recover(&self.channels);
        if let Some(tx) = channels.tx.as_ref() {
            // 通道断开只说明线程已退出，会话随线程一起没了——无需处理。
            let _ = tx.send(Command::Clear(doc_id));
        }
    }

    /// 取走全部已完成的结果（非阻塞）。
    ///
    /// 发现结果通道断开（后台线程 panic 后退出）时顺手标记死亡并清空在途登记：
    /// 那些请求的结果永远不会来了，留着只会让帧循环按 4ms 周期空转，
    /// 并让该文档再也分发不出去。
    pub(super) fn take_results(&self) -> Vec<HighlightResult> {
        let channels = lock_or_recover(&self.channels);
        let mut out = Vec::new();
        let mut disconnected = false;
        loop {
            match channels.rx.try_recv() {
                Ok(result) => out.push(result),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        drop(channels);
        if disconnected {
            self.mark_dead();
        }
        out
    }

    /// 结掉一次在途请求并记账。
    pub(super) fn finish(&self, doc_id: DocumentId, elapsed_ms: f64) {
        lock_or_recover(&self.in_flight).remove(&doc_id);
        let mut stats = lock_or_recover(&self.stats);
        stats.count += 1;
        stats.last_ms = Some(elapsed_ms);
    }

    /// 在途请求数（帧循环据此预约下一次轮询）。
    pub(super) fn in_flight_count(&self) -> usize {
        lock_or_recover(&self.in_flight).len()
    }

    /// 已完成的解析次数。
    pub(super) fn parse_count(&self) -> u64 {
        lock_or_recover(&self.stats).count
    }

    /// 最近一次解析的真实耗时（毫秒）；还没解析过时为 `None`。
    pub(super) fn last_parse_ms(&self) -> Option<f64> {
        lock_or_recover(&self.stats).last_ms
    }
}

/// worker 线程主体：按序处理命令，退出条件是命令通道断开（服务被丢弃）。
fn run(
    highlighter: Arc<dyn SyntaxHighlighter>,
    commands: Receiver<Command>,
    results: Sender<HighlightResult>,
) {
    // 会话表**只属于本线程**：解析期不需要任何锁，`clear_cache` 也不会被解析阻塞。
    let mut sessions: HashMap<DocumentId, Box<dyn HighlightSession>> = HashMap::new();
    while let Ok(command) = commands.recv() {
        match command {
            Command::Clear(Some(doc_id)) => {
                sessions.remove(&doc_id);
            }
            Command::Clear(None) => sessions.clear(),
            Command::Parse(request) => {
                let started = Instant::now();
                let outcome = parse(&*highlighter, &mut sessions, &request);
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                let result = HighlightResult {
                    doc_id: request.doc_id,
                    revision: request.revision,
                    lang: request.lang,
                    range: request.range,
                    outcome,
                    elapsed_ms,
                };
                // 接收端已丢弃（应用退出）时 send 只是失败，不 panic。
                if results.send(result).is_err() {
                    break;
                }
            }
        }
    }
}

/// 解析一次请求：优先复用该文档的增量会话，不支持增量时回退区间高亮。
///
/// 与改造前 `MarkdownAppService::highlight` 里的解析段**逐条等价**——
/// 只是搬到了后台线程，语义（含「首次 advance 即全量」）没有变化。
fn parse(
    highlighter: &dyn SyntaxHighlighter,
    sessions: &mut HashMap<DocumentId, Box<dyn HighlightSession>>,
    request: &HighlightRequest,
) -> Result<Arc<Vec<SpanView>>, AppError> {
    let lang = request.lang.as_deref();
    let (raw, pre_clipped) = if let Some(session) = sessions.get_mut(&request.doc_id) {
        (
            session
                .advance(&request.content, lang)
                .map_err(AppError::from)?,
            false,
        )
    } else if let Some(mut session) = highlighter.start_session() {
        // 会话是空的：首次 advance 即全量，不会重复解析一遍。
        let spans = session
            .advance(&request.content, lang)
            .map_err(AppError::from)?;
        sessions.insert(request.doc_id, session);
        (spans, false)
    } else {
        (
            highlighter
                .highlight_range(&request.content, request.range, lang)
                .map_err(AppError::from)?,
            true,
        )
    };
    Ok(to_span_views(raw, request.range, pre_clipped))
}

/// 领域片段 → DTO 片段视图，并按 `range` 裁剪。
///
/// `pre_clipped` 为真表示上游（`highlight_range`）**已经**裁过，
/// 再裁一次只会白跑一遍过滤。
pub(super) fn to_span_views(
    raw: Vec<crate::domain::markdown::value_objects::SyntaxSpan>,
    range: TextRange,
    pre_clipped: bool,
) -> Arc<Vec<SpanView>> {
    if raw.is_empty() {
        return empty_spans();
    }
    Arc::new(
        raw.into_iter()
            .filter(|span| pre_clipped || span.range().intersects(range))
            .map(|span| SpanView {
                range: RangeView::new(span.range().start().value(), span.range().end().value()),
                style: span_style_str(span.style()),
                url: None,
            })
            .collect(),
    )
}
