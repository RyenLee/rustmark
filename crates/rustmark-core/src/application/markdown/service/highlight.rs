//! 语法高亮用例：编辑区全文片段（防抖 + 缓存 + 后台补算）、片段高亮、缓存统计。

use std::sync::Arc;

use crate::shared::error::AppError;
use crate::shared::geometry::TextRange;
use crate::shared::ids::DocumentId;
use crate::shared::result::AppResult;

use crate::application::dto::{RangeView, SpanView};

use super::super::queries::*;
use super::highlight_worker::{HighlightRequest, RESULT_POLL_INTERVAL_MS, to_span_views};
use super::span_cache::empty_spans;
use super::view_mapper::span_style_str;
use super::{MarkdownAppService, TRUNCATE_THRESHOLD_BYTES, lock_or_recover};

impl MarkdownAppService {
    /// 语法高亮：返回片段 DTO 视图（**共享快照**，见下）。
    ///
    /// 编辑区**每帧**都会请求全文 span（FR-03），而 syntect 解析是 O(全文)——
    /// 实测 10KB 文档一次 **56.5ms**（`docs/spec/metrics.md`）。文档未变的帧
    /// （滚动、空闲、切换面板）本不该付这份钱，故按 `(文档, 版本, 语言, 区间)`
    /// 缓存最近一次结果：版本一变即失效，语义与「重算」完全一致。
    ///
    /// 超过 [`TRUNCATE_THRESHOLD_BYTES`] 的源直接返回空片段：表现层的 mapper
    /// 在同等阈值上已降级为纯文本排版（FR-06），此处若仍解析全文，
    /// 等于每帧付一份**无人使用**的成本（1MB 文档会直接卡死）。
    ///
    /// **防抖（OPT-011 ①）**：文档版本刚变的那 `policy.interval_ms` 毫秒内
    /// **直接返回空片段**，不进高亮器。理由与代价见 [`HighlightPace`]：
    /// 10KB 文档一次 syntect 全文解析 56–58ms（归因见 `docs/spec/metrics.md` §3.2.1，
    /// 97.4% 在解析本身），每击一次键就付一次会把编辑帧推到 67.9ms；
    /// 改为「打字期不上色、停手后一次性上色」后，打字期这一项降到 ≈0，
    /// 代价是连续打字时语法色暂时消失（停手 150ms 后立刻恢复）。
    ///
    /// **停手补算走后台（P1）**：防抖窗口到期、缓存又未命中时，本方法
    /// **不再自己解析**——它把请求投给后台 worker（`highlight_worker` 模块）
    /// 后立刻返回空片段（见 `dispatch_highlight`）。停手那一帧因此
    /// 不再付 ~64ms 的解析钱，颜色在结果回送的下一两帧里出现。
    ///
    /// 本方法每次调用都会先收一次后台结果（[`Self::poll_highlight_results`]）：
    /// 帧循环另有轮询点，但无头驱动（测试直接调服务）时没有帧循环，
    /// 「查询前先收结果」是唯一能让调用方在本帧看到颜色收敛的时机。
    ///
    /// # 为什么返回 `Arc<Vec<SpanView>>` 而不是 `Vec<SpanView>`（问题 046）
    ///
    /// 调用方每帧要一次，而它拿到的东西是**不可变快照**。按值返回意味着每帧
    /// 克隆整个片段表——100KB 文档 3080 个 `SpanView`（每个含一个 `String`），
    /// 实测每帧 **0.108ms** 全部花在克隆上，且随文档线性增长。共享 `Arc`
    /// 后这一项降到 O(1)，语义完全不变（下游只读）。
    ///
    /// # 帧路径上只碰 O(1) 元数据（问题 046）
    ///
    /// 上面三条决策（大文件降级 / 防抖 / 缓存命中）需要的只有**版本号与字节长度**。
    /// 此前实现先 `fetch` 全文再判断，等于每帧把整篇文本从 rope 快照成一个
    /// `String`——1MB 文档每帧一次 MB 级分配。现在改用
    /// [`DocumentContentProvider::revision_and_len`]（O(1)），**只有真的要走
    /// 高亮器时才取全文**（P1 起是「真的要分发一次后台补算时」）。
    ///
    /// [`HighlightPace`]: super::HighlightPace
    /// [`DocumentContentProvider::revision_and_len`]: crate::application::document::DocumentContentProvider::revision_and_len
    pub fn highlight(&self, cmd: HighlightQuery) -> AppResult<Arc<Vec<SpanView>>> {
        let (revision, len) = self
            .provider
            .revision_and_len(cmd.doc_id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        if len > TRUNCATE_THRESHOLD_BYTES {
            return Ok(empty_spans());
        }
        let range = (cmd.range.start().value(), cmd.range.end().value());
        // 先把后台已经算完的结果落库：算完的帧就该上色，不必再等下一帧。
        self.poll_highlight_results();
        // 防抖：未静止则不查缓存也不重算（注意缓存键带版本号，跳过它不会读到旧 span）。
        let settled = lock_or_recover(&self.highlight_pace).settled(
            cmd.doc_id,
            revision,
            self.clock.now(),
            self.policy.interval_ms(),
        );
        if !settled {
            return Ok(empty_spans());
        }
        if let Some(spans) = lock_or_recover(&self.highlight_cache).get(
            cmd.doc_id,
            revision,
            cmd.lang.as_deref(),
            range,
        ) {
            return Ok(spans);
        }
        self.dispatch_highlight(cmd.doc_id, revision, cmd.lang, cmd.range)
    }

    /// 把一次「停手补算」投给后台线程；本帧不解析。
    ///
    /// # 三种结果
    ///
    /// 1. **已投递**：返回空片段（本帧不上色，颜色由结果回送的帧补上）；
    /// 2. **该文档已有在途请求**：同样返回空片段，**不排队**——排队会在
    ///    连续停手时堆出一串过期解析，而它们的结果都会被 `revision` 丢弃；
    /// 3. **线程不可用**（spawn 失败 / 解析 panic 后通道断开）：回退为
    ///    本帧同步解析。宁可这一帧慢 64ms，也不能让语法色永远不出现。
    fn dispatch_highlight(
        &self,
        doc_id: DocumentId,
        revision: u64,
        lang: Option<String>,
        range: TextRange,
    ) -> AppResult<Arc<Vec<SpanView>>> {
        // 线程不可用：不投递（投了也没人收），直接走同步回退。
        if !self.highlight_worker.is_available() {
            return self.highlight_inline(doc_id, revision, lang, range);
        }
        if !self.highlight_worker.try_begin(doc_id, revision) {
            return Ok(empty_spans());
        }
        // 走到这里才真的取全文（每次停手一次，不是每帧一次）。
        let content = match self.fetch(doc_id) {
            Ok((content, _)) => content,
            Err(e) => {
                // 收尾在途登记：否则这篇文档再也分发不出去，颜色永久缺失。
                self.highlight_worker.abort(doc_id);
                return Err(e);
            }
        };
        let request = HighlightRequest {
            doc_id,
            revision,
            lang: lang.clone(),
            range,
            content,
        };
        if self.highlight_worker.dispatch(request) {
            return Ok(empty_spans());
        }
        // 通道在中途断开（线程刚退出）：清登记后走同步回退。
        self.highlight_worker.abort(doc_id);
        self.highlight_inline(doc_id, revision, lang, range)
    }

    /// 同步解析一次并落库（后台线程不可用时的回退路径）。
    ///
    /// **不建增量会话**：会话表在 worker 线程里（那是它独占的状态），
    /// 这里拿不到，因此退化为区间全量解析——慢，但结果正确。
    /// 这条路径只在「后台线程起不来」或「解析 panic 后线程已死」时走到。
    fn highlight_inline(
        &self,
        doc_id: DocumentId,
        revision: u64,
        lang: Option<String>,
        range: TextRange,
    ) -> AppResult<Arc<Vec<SpanView>>> {
        let (content, _) = self.fetch(doc_id)?;
        let raw = self
            .highlighter
            .highlight_range(&content, range, lang.as_deref());
        match raw {
            Ok(raw) => {
                // `highlight_range` 已按区间裁剪（`pre_clipped = true`）。
                let spans = to_span_views(raw, range, true);
                lock_or_recover(&self.highlight_cache).insert(
                    doc_id,
                    revision,
                    lang,
                    (range.start().value(), range.end().value()),
                    Arc::clone(&spans),
                );
                Ok(spans)
            }
            Err(e) => {
                // 帧路径上没有能弹错的位置（高亮是每帧的隐式查询），
                // 与后台路径一致：记一条告警并降级为无片段。
                log::warn!("语法高亮失败（文档已降级为纯文本）：{e}");
                Ok(empty_spans())
            }
        }
    }

    /// 收后台补算结果并落库，返回本轮**真正写进缓存**的条数。
    ///
    /// # 陈旧结果一律丢弃
    ///
    /// 结果带 `revision`。解析期间用户又编辑了（版本不等）就丢弃——
    /// 片段的字节区间与新文本错位，排版层按错误边界切片会 panic。
    /// 丢弃后不必做别的：下一帧的 [`Self::highlight`] 仍是缓存未命中，
    /// 会按新版本重新分发。
    ///
    /// # 为什么单独一个公开方法
    ///
    /// 帧循环（`app/frame/loop.rs`）每帧调它，让「后台算完了」这件事在
    /// **状态更新阶段**就落库，本帧绘制即可看到新颜色；[`Self::highlight`]
    /// 也调它，覆盖没有帧循环的无头驱动（测试 / 基准）。
    pub fn poll_highlight_results(&self) -> usize {
        let mut applied = 0usize;
        for result in self.highlight_worker.take_results() {
            // 先结在途：无论结果是否采用，这次解析都已经结束。
            self.highlight_worker
                .finish(result.doc_id, result.elapsed_ms);
            let spans = match result.outcome {
                Ok(spans) => spans,
                Err(e) => {
                    log::warn!("语法高亮失败（文档已降级为纯文本）：{e}");
                    continue;
                }
            };
            let current = self
                .provider
                .revision_and_len(result.doc_id)
                .map(|(revision, _)| revision);
            if current != Some(result.revision) {
                continue;
            }
            lock_or_recover(&self.highlight_cache).insert(
                result.doc_id,
                result.revision,
                result.lang,
                (result.range.start().value(), result.range.end().value()),
                spans,
            );
            applied += 1;
        }
        applied
    }

    /// 是否有补算请求在途（测试与调试面板用）。
    pub fn highlight_pending(&self) -> usize {
        self.highlight_worker.in_flight_count()
    }

    /// 后台已完成的解析次数（含结果被丢弃的那些——成本确实付了）。
    pub fn highlight_parse_count(&self) -> u64 {
        self.highlight_worker.parse_count()
    }

    /// 最近一次后台解析的真实耗时（毫秒）；从未解析过时为 `None`。
    ///
    /// 「帧变快了」不代表「成本消失了」，这份数字是成本**转移到哪**的证据：
    /// `tests/integration/main.rs::measure_editor_frame_highlight_cost`
    /// 用它登记真实解析基线（10KB ≈ 64ms）。
    pub fn last_highlight_parse_ms(&self) -> Option<f64> {
        self.highlight_worker.last_parse_ms()
    }

    /// 高亮缓存的命中 / 未命中计数（测试与调试面板用）。
    pub fn highlight_cache_stats(&self) -> (u64, u64) {
        let c = lock_or_recover(&self.highlight_cache);
        (c.hits, c.misses)
    }

    /// 高亮防抖状态快照（测试与调试面板用）。
    ///
    /// 每项为 `(文档 ID, 上次见到的版本, 上次见到的时刻毫秒)`；空表示该文档
    /// 尚未被高亮查询见过（首帧会立刻上色，见 [`HighlightPace`]）。
    ///
    /// [`HighlightPace`]: super::HighlightPace
    pub fn highlight_pace_snapshot(&self) -> Vec<(DocumentId, u64, u64)> {
        lock_or_recover(&self.highlight_pace).snapshot()
    }

    /// 高亮的**下一个到期点**（绝对毫秒）；没有待办时返回 `None`。
    ///
    /// 供帧循环登记「到期帧」（T11）。两个来源，取更早者：
    ///
    /// 1. **防抖窗口到期**：窗口内 [`Self::highlight`] 返回空片段，必须有人在
    ///    窗口到期时再跑一帧才补得上颜色；
    /// 2. **后台结果在途**：解析要几十毫秒，期间没人叫醒帧循环的话，颜色要等
    ///    用户下一次输入才出现——失焦时是永久无色。故在途期间按
    ///    `RESULT_POLL_INTERVAL_MS`（4ms）预约轮询帧；结果收完即返回 `None`。
    ///
    /// 事件驱动重绘下没有输入就没有帧，缺了这条，语法色会一直停在无色状态。
    pub fn highlight_next_due_in(&self) -> Option<u64> {
        let pace = lock_or_recover(&self.highlight_pace).next_deadline(self.policy.interval_ms());
        let polling = (self.highlight_worker.in_flight_count() > 0).then(|| {
            self.clock
                .now()
                .millis()
                .saturating_add(RESULT_POLL_INTERVAL_MS)
        });
        match (pace, polling) {
            (Some(pace_at), Some(poll_at)) => Some(pace_at.min(poll_at)),
            (pace_at, poll_at) => pace_at.or(poll_at),
        }
    }

    /// 高亮一段独立文本（预览区的代码块）。
    ///
    /// 走的是与编辑区同一个 `SyntaxHighlighter`，故「预览里的代码配色」
    /// 与「编辑器里的代码配色」天然一致——两套解析器会立刻产生色差。
    ///
    /// 保持**同步**：片段是预览里可见的一小段代码，且有独立缓存，与编辑区
    /// 的全文补算不是一回事（P1 只搬全文那条）。
    pub fn highlight_snippet(&self, cmd: HighlightSnippetQuery) -> AppResult<Vec<SpanView>> {
        let spans = self
            .highlighter
            .highlight(&cmd.text, cmd.lang.as_deref())
            .map_err(AppError::from)?;
        Ok(spans
            .iter()
            .map(|span| SpanView {
                range: RangeView::new(span.range().start().value(), span.range().end().value()),
                style: span_style_str(span.style()),
                url: None,
            })
            .collect())
    }
}
