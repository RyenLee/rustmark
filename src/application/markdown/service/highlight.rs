//! 语法高亮用例：编辑区全文片段（防抖 + 缓存）、片段高亮、缓存统计。

use crate::shared::error::AppError;
use crate::shared::result::AppResult;

use crate::application::dto::{RangeView, SpanView};

use super::super::queries::*;
use super::view_mapper::span_style_str;
use super::{MarkdownAppService, TRUNCATE_THRESHOLD_BYTES, lock_or_recover};

impl MarkdownAppService {
    /// 语法高亮：返回片段 DTO 视图。
    ///
    /// 编辑区**每帧**都会请求全文 span（FR-03），而 syntect 解析是 O(全文)——
    /// 实测 10KB 文档一次 **56.5ms**（`docs/metrics.md`）。文档未变的帧
    /// （滚动、空闲、切换面板）本不该付这份钱，故按 `(文档, 版本, 语言, 区间)`
    /// 缓存最近一次结果：版本一变即失效，语义与「重算」完全一致。
    ///
    /// 超过 [`TRUNCATE_THRESHOLD_BYTES`] 的源直接返回空片段：表现层的 mapper
    /// 在同等阈值上已降级为纯文本排版（FR-06），此处若仍解析全文，
    /// 等于每帧付一份**无人使用**的成本（1MB 文档会直接卡死）。
    ///
    /// **防抖（OPT-011 ①）**：文档版本刚变的那 `policy.interval_ms` 毫秒内
    /// **直接返回空片段**，不进高亮器。理由与代价见 [`HighlightPace`]：
    /// 10KB 文档一次 syntect 全文解析 56–58ms（归因见 `docs/metrics.md` §3.2.1，
    /// 97.4% 在解析本身），每击一次键就付一次会把编辑帧推到 67.9ms；
    /// 改为「打字期不上色、停手后一次性上色」后，打字期这一项降到 ≈0，
    /// 代价是连续打字时语法色暂时消失（停手 150ms 后立刻恢复）。
    ///
    /// [`HighlightPace`]: super::HighlightPace
    pub fn highlight(&self, cmd: HighlightQuery) -> AppResult<Vec<SpanView>> {
        let (content, revision) = self.fetch(cmd.doc_id)?;
        if content.len() > TRUNCATE_THRESHOLD_BYTES {
            return Ok(Vec::new());
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
            return Ok(Vec::new());
        }
        if let Some(spans) = lock_or_recover(&self.highlight_cache).get(
            cmd.doc_id,
            revision,
            cmd.lang.as_deref(),
            range,
        ) {
            return Ok(spans.clone());
        }
        let spans = self
            .highlighter
            .highlight_range(&content, cmd.range, cmd.lang.as_deref())
            .map_err(AppError::from)?;
        let spans: Vec<SpanView> = spans
            .iter()
            .map(|span| SpanView {
                range: RangeView::new(span.range().start().value(), span.range().end().value()),
                style: span_style_str(span.style()),
            })
            .collect();
        lock_or_recover(&self.highlight_cache).insert(
            cmd.doc_id,
            revision,
            cmd.lang.clone(),
            range,
            spans.clone(),
        );
        Ok(spans)
    }

    /// 高亮缓存的命中 / 未命中计数（测试与调试面板用）。
    pub fn highlight_cache_stats(&self) -> (u64, u64) {
        let c = lock_or_recover(&self.highlight_cache);
        (c.hits, c.misses)
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
            })
            .collect())
    }
}
