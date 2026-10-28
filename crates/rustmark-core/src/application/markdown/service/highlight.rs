//! 语法高亮用例：编辑区全文片段（防抖 + 缓存）、片段高亮、缓存统计。

use std::sync::Arc;

use crate::shared::error::AppError;
use crate::shared::result::AppResult;

use crate::application::dto::{RangeView, SpanView};

use super::super::queries::*;
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
    /// 高亮器时才取全文**。
    ///
    /// [`HighlightPace`]: super::HighlightPace
    pub fn highlight(&self, cmd: HighlightQuery) -> AppResult<Arc<Vec<SpanView>>> {
        let (revision, len) = self
            .provider
            .revision_and_len(cmd.doc_id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        if len > TRUNCATE_THRESHOLD_BYTES {
            return Ok(empty_spans());
        }
        let range = (cmd.range.start().value(), cmd.range.end().value());
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
        // 走到这里才真的要解析，此时取全文（每击键一次，不是每帧一次）。
        let (content, _) = self.fetch(cmd.doc_id)?;
        // 优先走增量会话：同一文档连续编辑时只重解变更行（OPT-011 复议）。
        // 不支持增量的实现（含测试替身）回退到 `highlight_range`，行为等价。
        let (raw, pre_clipped) = {
            let mut sessions = lock_or_recover(&self.highlight_sessions);
            if let Some(session) = sessions.get_mut(&cmd.doc_id) {
                (
                    session
                        .advance(&content, cmd.lang.as_deref())
                        .map_err(AppError::from)?,
                    false,
                )
            } else if let Some(mut session) = self.highlighter.start_session() {
                // 会话是空的：首次 advance 即全量，不会重复解析一遍。
                let spans = session
                    .advance(&content, cmd.lang.as_deref())
                    .map_err(AppError::from)?;
                sessions.insert(cmd.doc_id, session);
                (spans, false)
            } else {
                (
                    self.highlighter
                        .highlight_range(&content, cmd.range, cmd.lang.as_deref())
                        .map_err(AppError::from)?,
                    true,
                )
            }
        };
        let spans: Arc<Vec<SpanView>> = Arc::new(
            raw.into_iter()
                // 会话产出的是**全文**片段，这里按 `range` 裁剪，
                // 与 `highlight_range` 的裁剪语义对齐：相交即输出整段。
                .filter(|span| pre_clipped || span.range().intersects(cmd.range))
                .map(|span| SpanView {
                    range: RangeView::new(span.range().start().value(), span.range().end().value()),
                    style: span_style_str(span.style()),
                    url: None,
                })
                .collect(),
        );
        lock_or_recover(&self.highlight_cache).insert(
            cmd.doc_id,
            revision,
            cmd.lang.clone(),
            range,
            Arc::clone(&spans),
        );
        Ok(spans)
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
    pub fn highlight_pace_snapshot(&self) -> Vec<(crate::shared::ids::DocumentId, u64, u64)> {
        lock_or_recover(&self.highlight_pace).snapshot()
    }

    /// 高亮防抖的**下一个到期点**（绝对毫秒）；没有文档在等补算时返回 `None`。
    ///
    /// 供帧循环登记「到期帧」（T11）：[`Self::highlight`] 在防抖窗口内返回空片段，
    /// 必须有人在窗口到期时再跑一帧才补得上颜色。事件驱动重绘下没有输入就没有帧，
    /// 编辑器失焦后连光标闪烁的兜底也没有——缺了这条，语法色会一直停在无色状态。
    pub fn highlight_next_due_in(&self) -> Option<u64> {
        lock_or_recover(&self.highlight_pace).next_deadline(self.policy.interval_ms())
    }

    /// 高亮一段独立文本（预览区的代码块）。
    ///
    /// 走的是与编辑区同一个 `SyntaxHighlighter`，故「预览里的代码配色」
    /// 与「编辑器里的代码配色」天然一致——两套解析器会立刻产生色差。
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
