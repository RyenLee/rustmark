//! 增量语法高亮 PoC（对应 T4 预研报告、OPT-011 复议）。
//!
//! # 要验证什么
//!
//! 「行边界检查点 → 恢复 `ParseState`/`ScopeStack` → 从变更行续解 →
//! 状态 `Eq` 收敛即截断」这条链路，能否在**不改 syntect、不动领域接口**
//! 的前提下，把「改一行重解全文」的成本降到 1/N，且结果与全量**逐片段相同**。
//!
//! # 为什么先 PoC 而不是直接改接口
//!
//! `ParseState` 是 syntect 的类型。若把它放进 domain 的
//! [`SyntaxHighlighter`](crate::domain::markdown::services::SyntaxHighlighter)
//! 就违反「基础设施类型不外泄」的分层约定（OPT-011 当年否决的正是这一点）。
//! 因此 PoC 把 [`SyntectSession`] 完全关在 infrastructure 内部：先用
//! 「正确性对拍 + 实测收益」回答**值不值得**为它设计领域接口
//! （预研报告方案 A：`HighlightSession`）。
//!
//! # 调用方必须保证的前提
//!
//! 1. **单次连续编辑**：`changed` 给出的是**新文本**中的变更区间，
//!    且 `[0, changed.start)` 与上次文本逐字节相同；
//! 2. `changed = None` 表示「不确定改了什么」→ 退化为全量重建。
//!
//! 前缀一致性在 [`SyntectSession::highlight`] 内部用 memcmp 校验：不满足
//! 就静默退化为全量。**宁可慢，不可错。**
//!
//! # 正确性依据
//!
//! syntect 5.3 的 `ParseState` 与 `ScopeStack` 都实现了 `Eq`，且
//! `parse_line` 是 `(state, line, syntaxes) → ops` 的确定性函数。因此
//! 「新旧状态相等」⇒「后续每一行的 ops 都相等」⇒ 后续片段可整体复用。
//!
//! # 一个不显然的坑：收敛点为什么按**字节偏移**查而不按行号
//!
//! 插入/删除整行会让新旧行号错位。若按「每 N 行一个检查点」的行号相位
//! 判收敛，插一行后相位永久错位，收敛点一次都命中不了，增量退化为全量。
//! 故收敛判定在**新文本的每一个行首**做，把 `line_start - delta` 映射回
//! 旧文本坐标，再用二分查旧检查点表——行数怎么变都不影响映射的正确性。

use syntect::easy::ScopeRangeIterator;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::domain::markdown::errors::HighlightError;
use crate::domain::markdown::services::HighlightSession;
use crate::domain::markdown::value_objects::{SpanStyle, SyntaxSpan};
use crate::shared::geometry::{ByteOffset, TextRange};

use super::syntect_highlighter::{classify, default_syntax_set, push_span, resolve_language};

/// 检查点间隔（行）。
///
/// 间隔越小，恢复点离变更行越近、但检查点数量（及其 `ParseState` /
/// `ScopeStack` 克隆的内存）越大。25 行在 10KB（约 300 行）量级下
/// 约 12 个检查点、几十 KB 内存，平均恢复距离 12.5 行。
const CHECKPOINT_INTERVAL_LINES: usize = 25;

/// 超过该字节数的文档不做增量（与现有 >500KB 降级为空片段的策略对齐）。
const MAX_INCREMENTAL_BYTES: usize = 512 * 1024;

/// 检查点：某一**行首**处的完整解析状态。
///
/// `offset` 始终是「当前文本」坐标（增量后旧检查点按 `delta` 平移），
/// `state` / `stack` 在「该行之前的文本逐字节未变」时保持有效。
#[derive(Clone)]
struct Checkpoint {
    offset: usize,
    state: ParseState,
    stack: ScopeStack,
}

/// 一次高亮的路径与工作量统计（PoC 观测用；接线时可下沉为 metrics）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct IncrementalStats {
    /// 是否走了增量路径。
    pub incremental: bool,
    /// 本次实际 `parse_line` 的行数。
    pub parsed_lines: usize,
    /// 新文本总行数。
    pub total_lines: usize,
    /// 收敛点（新文本字节偏移）；`None` 表示解析到文末或走了全量。
    pub converged_at: Option<usize>,
}

impl IncrementalStats {
    /// 复用的行数（未重新解析的行）。
    pub(crate) fn reused_lines(&self) -> usize {
        self.total_lines.saturating_sub(self.parsed_lines)
    }

    /// 重新解析的行占比（0.0 ~ 1.0）。
    pub(crate) fn parsed_ratio(&self) -> f64 {
        if self.total_lines == 0 {
            return 0.0;
        }
        self.parsed_lines as f64 / self.total_lines as f64
    }
}

/// 有状态的增量高亮会话：持有上次的文本、检查点与片段。
///
/// 生命周期与「一个代码块的高亮缓存」对齐——编辑器里即
/// 「同一文档同一语言」的那份缓存。
pub(crate) struct SyntectSession {
    syntaxes: &'static SyntaxSet,
    /// 上次高亮的语言键（`trim + lowercase` 后）。
    lang: Option<String>,
    /// 上次高亮的完整文本（前缀校验与 `delta` 计算的基准）。
    text: String,
    /// 上次的检查点，按 `offset` 升序。
    checkpoints: Vec<Checkpoint>,
    /// 上次的完整片段。
    spans: Vec<SyntaxSpan>,
}

/// 解析起点（全量时是文首 + 空状态，增量时是某个检查点）。
struct ParseStart {
    offset: usize,
    state: ParseState,
    stack: ScopeStack,
}

/// 收敛判定所需的旧侧信息。
struct ConvergeCtx<'a> {
    /// 旧检查点表（按 offset 升序）。
    old: &'a [Checkpoint],
    /// 新文本长度 − 旧文本长度（单次连续编辑下全文档恒定）。
    delta: i64,
    /// 新文本中变更区间的结束：收敛点必须不早于它，否则会把
    /// 「状态碰巧相同」的变更区间内部误判为可复用。
    changed_end: usize,
}

/// 一段连续解析的产物。
struct ParseOutcome {
    spans: Vec<SyntaxSpan>,
    checkpoints: Vec<Checkpoint>,
    converged_at: Option<usize>,
    parsed_lines: usize,
}

impl SyntectSession {
    /// 新建空会话。
    pub(crate) fn new() -> Self {
        Self {
            syntaxes: default_syntax_set(),
            lang: None,
            text: String::new(),
            checkpoints: Vec::new(),
            spans: Vec::new(),
        }
    }

    /// 高亮并更新会话缓存；语义与
    /// [`SyntaxHighlighter::highlight`](crate::domain::markdown::services::SyntaxHighlighter::highlight)
    /// 一致。
    pub(crate) fn highlight(
        &mut self,
        text: &str,
        lang: Option<&str>,
        changed: Option<TextRange>,
    ) -> Result<Vec<SyntaxSpan>, HighlightError> {
        self.highlight_with_stats(text, lang, changed)
            .map(|(spans, _)| spans)
    }

    /// 同 [`Self::highlight`]，额外返回本次的路径与工作量。
    pub(crate) fn highlight_with_stats(
        &mut self,
        text: &str,
        lang: Option<&str>,
        changed: Option<TextRange>,
    ) -> Result<(Vec<SyntaxSpan>, IncrementalStats), HighlightError> {
        let mut stats = IncrementalStats::default();

        // --- 1. 语言 / 空文本快速路径：与 SyntectHighlighter 的降级语义一致 ---
        let Some(lang_raw) = lang else {
            self.reset();
            return Ok((Vec::new(), stats));
        };
        let lang_key = lang_raw.trim().to_ascii_lowercase();
        let Some(syntax) = resolve_language(self.syntaxes, &lang_key) else {
            log::warn!("未知语言「{lang_raw}」，按纯文本处理");
            self.reset();
            return Ok((Vec::new(), stats));
        };
        if text.is_empty() {
            self.reset();
            return Ok((Vec::new(), stats));
        }

        stats.total_lines = text.split_inclusive('\n').count();

        // --- 2. 判定能否走增量 ---
        let reusable = changed.filter(|range| self.can_reuse(text, &lang_key, *range));
        let delta = text.len() as i64 - self.text.len() as i64;

        let outcome = match reusable {
            Some(range) => {
                // 恢复点：变更点之前最近的检查点。它之前的文本逐字节未变。
                let anchor = self
                    .checkpoints
                    .partition_point(|c| c.offset <= range.start().value())
                    .checked_sub(1)
                    .map(|index| self.checkpoints[index].clone());
                match anchor {
                    Some(anchor) => {
                        let anchor_offset = anchor.offset;
                        let mut outcome = self.run(
                            text,
                            ParseStart {
                                offset: anchor.offset,
                                state: anchor.state,
                                stack: anchor.stack,
                            },
                            Some(ConvergeCtx {
                                old: &self.checkpoints,
                                delta,
                                changed_end: range.end().value(),
                            }),
                        )?;
                        // 复用恢复点之前的旧片段：它们完整落在未变前缀里，
                        // 偏移不变。漏掉这段会静默丢掉文档前半部分的高亮。
                        let head = self
                            .spans
                            .partition_point(|span| span.range().end().value() <= anchor_offset);
                        if head > 0 {
                            let mut merged = Vec::with_capacity(head + outcome.spans.len());
                            merged.extend_from_slice(&self.spans[..head]);
                            merged.append(&mut outcome.spans);
                            outcome.spans = merged;
                        }
                        if let Some(converge_offset) = outcome.converged_at {
                            self.append_reused_tail(&mut outcome, converge_offset, delta);
                        }
                        outcome
                    }
                    // 变更发生在首个检查点之前（文档头部）：没有可恢复的状态。
                    None => self.run_full(text, syntax)?,
                }
            }
            None => self.run_full(text, syntax)?,
        };

        let ParseOutcome {
            spans,
            checkpoints,
            converged_at,
            parsed_lines,
        } = outcome;

        stats.incremental = reusable.is_some();
        stats.parsed_lines = parsed_lines;
        stats.converged_at = converged_at;

        // --- 3. 更新缓存 ---
        self.lang = Some(lang_key);
        self.text.clear();
        self.text.push_str(text);
        self.checkpoints = checkpoints;
        self.spans = spans.clone();

        Ok((spans, stats))
    }

    /// 上一次高亮的片段（便于调用方零拷贝取用）。
    pub(crate) fn last_spans(&self) -> &[SyntaxSpan] {
        &self.spans
    }

    /// 能否复用上一次的状态。
    fn can_reuse(&self, text: &str, lang_key: &str, changed: TextRange) -> bool {
        if self.lang.as_deref() != Some(lang_key) || self.text.is_empty() {
            return false;
        }
        if self.text.len() > MAX_INCREMENTAL_BYTES || text.len() > MAX_INCREMENTAL_BYTES {
            return false;
        }
        let start = changed.start().value();
        let end = changed.end().value();
        if end < start || end > text.len() || start > self.text.len() {
            return false;
        }
        // 前缀必须逐字节相同——这是「从 anchor 恢复状态」的合法性基础。
        // memcmp 是 O(n)，但 10KB 约 1µs，相对 58ms 的解析成本可忽略。
        text.as_bytes()[..start] == self.text.as_bytes()[..start]
    }

    /// 全量重建。
    fn run_full(
        &self,
        text: &str,
        syntax: &SyntaxReference,
    ) -> Result<ParseOutcome, HighlightError> {
        self.run(
            text,
            ParseStart {
                offset: 0,
                state: ParseState::new(syntax),
                stack: ScopeStack::new(),
            },
            None,
        )
    }

    /// 把收敛点之后的旧片段平移 `delta` 后接上，并平移旧检查点。
    fn append_reused_tail(&self, outcome: &mut ParseOutcome, converge_offset: usize, delta: i64) {
        // 收敛点在旧文本中的坐标。delta 恒定 ⇒ 该处之后的文本逐字节相同。
        let old_signed = converge_offset as i64 - delta;
        if old_signed < 0 {
            return;
        }
        let old_offset = old_signed as usize;

        // `push_span` 保证片段不跨行，而 old_offset 是旧检查点（行首），
        // 故不存在被收敛点切断的片段，按 start 二分即可整体复用。
        let tail = self
            .spans
            .partition_point(|span| span.range().start().value() < old_offset);
        for span in &self.spans[tail..] {
            push_shifted(&mut outcome.spans, *span, delta);
        }

        // 旧检查点本身也继续有效：它们之后的文本未变，只是偏移平移。
        for checkpoint in self
            .checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.offset >= old_offset)
        {
            let shifted = checkpoint.offset as i64 + delta;
            if shifted < 0 {
                continue;
            }
            let offset = shifted as usize;
            // 收敛点自身可能同时被新解析段与旧检查点表产出，去重。
            if outcome.checkpoints.last().map(|last| last.offset) == Some(offset) {
                continue;
            }
            outcome.checkpoints.push(Checkpoint {
                offset,
                state: checkpoint.state.clone(),
                stack: checkpoint.stack.clone(),
            });
        }
    }

    /// 从 `start` 起顺序解析，收集片段与检查点；给了 `converge` 时提前截断。
    fn run(
        &self,
        text: &str,
        start: ParseStart,
        converge: Option<ConvergeCtx<'_>>,
    ) -> Result<ParseOutcome, HighlightError> {
        let mut state = start.state;
        let mut stack = start.stack;
        let mut spans: Vec<SyntaxSpan> = Vec::new();
        let mut checkpoints: Vec<Checkpoint> = Vec::new();
        let mut converged_at = None;
        let mut parsed_lines = 0usize;

        let mut line_start = start.offset;
        let mut is_first_line = true;
        // 相邻区间的 scope 栈常完全相同，用快照做记忆化（与生产实现同款）。
        let mut cached_scopes: Vec<Scope> = Vec::new();
        let mut cached_style = SpanStyle::Plain;

        // 起点自身登记为检查点：否则「改文档开头」时找不到可恢复的锚点，
        // 每次都会退化成全量。相位与后续每 N 行的检查点一致。
        checkpoints.push(Checkpoint {
            offset: start.offset,
            state: state.clone(),
            stack: stack.clone(),
        });

        for line in text[start.offset..].split_inclusive('\n') {
            // 收敛判定必须在**进入本行之前**：此刻的 (state, stack) 正是
            // 旧检查点记录的形状（行首状态）。
            if let Some(ctx) = &converge
                && !is_first_line
                && line_start >= ctx.changed_end
            {
                let old_offset = line_start as i64 - ctx.delta;
                if old_offset >= 0
                    && let Ok(index) = ctx
                        .old
                        .binary_search_by_key(&(old_offset as usize), |c| c.offset)
                    && ctx.old[index].state == state
                    && ctx.old[index].stack == stack
                {
                    converged_at = Some(line_start);
                    break;
                }
            }
            is_first_line = false;

            let line_len = line.len();
            let ops = state
                .parse_line(line, self.syntaxes)
                .map_err(|e| HighlightError::new(format!("解析语法失败：{e}")))?;
            parsed_lines += 1;

            for (range_in_line, op) in ScopeRangeIterator::new(&ops, line) {
                if !matches!(op, syntect::parsing::ScopeStackOp::Noop) && stack.apply(op).is_err() {
                    // 栈失衡（语法定义异常）时停止，已产出的片段仍然有效。
                    log::warn!("scope 栈失衡，提前结束高亮");
                    return Ok(ParseOutcome {
                        spans,
                        checkpoints,
                        converged_at,
                        parsed_lines,
                    });
                }
                if range_in_line.is_empty() {
                    continue;
                }
                let span_start = line_start + range_in_line.start;
                let span_end = line_start + range_in_line.end;
                let style = if cached_scopes.as_slice() == stack.as_slice() {
                    cached_style
                } else {
                    let resolved = classify(&stack);
                    cached_scopes.clear();
                    cached_scopes.extend_from_slice(stack.as_slice());
                    cached_style = resolved;
                    resolved
                };
                if style == SpanStyle::Plain {
                    continue;
                }
                push_span(&mut spans, span_start, span_end, style, line_start);
            }

            line_start += line_len;
            // 行首检查点：解析完这一行后，下一行行首的状态。
            if parsed_lines.is_multiple_of(CHECKPOINT_INTERVAL_LINES) {
                checkpoints.push(Checkpoint {
                    offset: line_start,
                    state: state.clone(),
                    stack: stack.clone(),
                });
            }
        }

        // 文末补一个检查点：否则「在文档最后 25 行内编辑」解析到文末后
        // 重建的检查点表会退化到只剩起点，下一轮直接失去可恢复锚点。
        if converged_at.is_none()
            && line_start > start.offset
            && checkpoints.last().map(|last| last.offset) != Some(line_start)
        {
            checkpoints.push(Checkpoint {
                offset: line_start,
                state: state.clone(),
                stack: stack.clone(),
            });
        }

        Ok(ParseOutcome {
            spans,
            checkpoints,
            converged_at,
            parsed_lines,
        })
    }

    /// 清空全部缓存（语言未知 / 文本为空时）。
    fn reset(&mut self) {
        self.lang = None;
        self.text.clear();
        self.checkpoints.clear();
        self.spans.clear();
    }
}

impl HighlightSession for SyntectSession {
    fn advance(
        &mut self,
        text: &str,
        lang: Option<&str>,
    ) -> Result<Vec<SyntaxSpan>, HighlightError> {
        // 文本与语言都没变：同一 revision 下换 clip 的重复查询，直接复用。
        let lang_key = lang.map(|value| value.trim().to_ascii_lowercase());
        if text == self.text && self.lang == lang_key {
            return Ok(self.spans.clone());
        }
        // 文本变了才值得比对；语言变了但文本没变时 changed 为 None（全量重建）。
        let changed = (text != self.text).then(|| changed_range(&self.text, text));
        self.highlight(text, lang, changed)
    }
}

/// 用「公共前缀 + 公共后缀」求新文本坐标下的变更区间。
///
/// # 为什么不用真正的 diff
///
/// 高亮只需要「哪一段可能影响语法状态」，不需要最小编辑脚本。公共前后缀
/// 给出的是**包含所有变更的最小连续区间**：区间外逐字节相同，正是增量
/// 恢复所需的全部前提；区间内哪怕有未变的片段，也只是多解几行。
///
/// 多光标 / 撤销这类「两处都变」的编辑会得到一个横跨两处的区间——最坏
/// 退化为全量，正确性不受影响。
///
/// # UTF-8 边界
///
/// 前缀/后缀是按**字节**比对的，可能落在多字节字符中间。此时向前/向后
/// 收缩到最近的 char boundary，把这个字符整体算进变更区（区间只能变大，
/// 不会切在字符中间导致后续切片 panic）。
fn changed_range(old: &str, new: &str) -> TextRange {
    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();

    let mut prefix = 0usize;
    let prefix_limit = old_bytes.len().min(new_bytes.len());
    while prefix < prefix_limit && old_bytes[prefix] == new_bytes[prefix] {
        prefix += 1;
    }
    while prefix > 0 && !new.is_char_boundary(prefix) {
        prefix -= 1;
    }

    let mut suffix = 0usize;
    let suffix_limit = (old_bytes.len() - prefix).min(new_bytes.len() - prefix);
    while suffix < suffix_limit
        && old_bytes[old_bytes.len() - 1 - suffix] == new_bytes[new_bytes.len() - 1 - suffix]
    {
        suffix += 1;
    }
    while suffix > 0 && !new.is_char_boundary(new_bytes.len() - suffix) {
        suffix -= 1;
    }

    TextRange::new(
        ByteOffset::new(prefix),
        ByteOffset::new(new_bytes.len() - suffix),
    )
}

/// 追加一个整体平移 `delta` 的旧片段。
///
/// 不复用 [`push_span`] 的合并逻辑：收敛点是行首，新旧片段分属不同行，
/// 跨收敛边界的合并会破坏「增量与全量逐片段一致」的验收标准。
fn push_shifted(spans: &mut Vec<SyntaxSpan>, span: SyntaxSpan, delta: i64) {
    let start = span.range().start().value() as i64 + delta;
    let end = span.range().end().value() as i64 + delta;
    if start < 0 || end < start {
        return;
    }
    spans.push(SyntaxSpan::new(
        TextRange::new(
            ByteOffset::new(start as usize),
            ByteOffset::new(end as usize),
        ),
        span.style(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::markdown::services::SyntaxHighlighter;
    use crate::infrastructure::highlighting::syntect_highlighter::SyntectHighlighter;
    use std::time::Instant;

    /// 生成一段约 `target_bytes` 的 Rust 样本，含跨行块注释、raw string、
    /// 字符字面量等**有状态**结构（这些才是增量高亮最容易出错的地方）。
    fn sample_rust(target_bytes: usize) -> String {
        let mut text = String::from("//! 生成的增量高亮样本。\n\n");
        let mut index = 0usize;
        while text.len() < target_bytes {
            text.push_str(&format!(
                "/// 第 {index} 个函数。
fn generated_{index}(input: &str) -> Result<usize, String> {{
    let mut total = 0usize;
    /* 块注释开始
       跨行延续，含 \"引号\" 与转义 */
    for ch in input.chars() {{
        if ch == 'λ' {{ total += 2; }} else {{ total += 1; }}
    }}
    let raw = r#\"raw string {index}\"#;
    Ok(total + raw.len())
}}

"
            ));
            index += 1;
        }
        text
    }

    fn range(start: usize, end: usize) -> TextRange {
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end))
    }

    /// 用替换区间模拟一次编辑，返回新文本与新文本坐标下的变更区间。
    fn replace(text: &str, start: usize, end: usize, replacement: &str) -> (String, TextRange) {
        let mut out = String::with_capacity(text.len() + replacement.len());
        out.push_str(&text[..start]);
        out.push_str(replacement);
        out.push_str(&text[end..]);
        (out, range(start, start + replacement.len()))
    }

    /// 全量基线：走生产实现，确保对比的是「用户真正看到的结果」。
    fn full(text: &str, lang: &str) -> Vec<SyntaxSpan> {
        SyntectHighlighter::new()
            .highlight(text, Some(lang))
            .expect("全量高亮不应失败")
    }

    fn assert_same(actual: &[SyntaxSpan], expected: &[SyntaxSpan], context: &str) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{context}：片段数量不一致\n实际: {actual:?}\n期望: {expected:?}"
        );
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(a, e, "{context}：第 {i} 个片段不一致");
        }
    }

    /// 找第 `occurrence` 次出现 `needle` 的字节偏移（超出则取模，避免
    /// 测试与样本行数耦合）。
    fn find_nth(text: &str, needle: &str, occurrence: usize) -> usize {
        let hits: Vec<usize> = text.match_indices(needle).map(|(at, _)| at).collect();
        assert!(!hits.is_empty(), "样本里找不到 `{needle}`");
        hits[occurrence % hits.len()]
    }

    #[test]
    fn first_call_equals_full_highlight() {
        let text = sample_rust(10 * 1024);
        let mut session = SyntectSession::new();
        let (spans, stats) = session
            .highlight_with_stats(&text, Some("rust"), None)
            .expect("首帧不应失败");
        assert!(!stats.incremental, "首帧没有历史状态，必须走全量");
        assert_eq!(stats.parsed_lines, stats.total_lines);
        assert_same(&spans, &full(&text, "rust"), "首帧");
        assert!(!session.checkpoints.is_empty(), "全量后应留下检查点");
    }

    #[test]
    fn equal_length_edit_converges_and_matches_full() {
        let text = sample_rust(10 * 1024);
        let mut session = SyntectSession::new();
        session.highlight(&text, Some("rust"), None).unwrap();

        // 改一个标识符：`total += 2` → `total += 3`，等长替换。
        let at = find_nth(&text, "total += 2", 40);
        let (next, changed) = replace(&text, at, at + "total += 2".len(), "total += 3");

        let (spans, stats) = session
            .highlight_with_stats(&next, Some("rust"), Some(changed))
            .expect("增量不应失败");
        assert!(stats.incremental, "应走增量路径");
        assert!(
            stats.parsed_lines < stats.total_lines / 5,
            "重解行数应远少于全量：{}/{}",
            stats.parsed_lines,
            stats.total_lines
        );
        assert_same(&spans, &full(&next, "rust"), "等长替换");
    }

    #[test]
    fn insert_and_delete_line_match_full() {
        let text = sample_rust(10 * 1024);
        let mut session = SyntectSession::new();
        session.highlight(&text, Some("rust"), None).unwrap();

        // 插入一整行（delta > 0，且行数 +1，检验按字节映射而非行号）。
        let at = find_nth(&text, "    let mut total = 0usize;", 20);
        let (inserted, changed) = replace(&text, at, at, "    // 新增的一行注释\n");
        let (spans, stats) = session
            .highlight_with_stats(&inserted, Some("rust"), Some(changed))
            .unwrap();
        assert!(stats.incremental);
        assert_same(&spans, &full(&inserted, "rust"), "插入一行后");

        // 再删掉一整行（delta < 0）。
        let at = find_nth(&inserted, "    // 新增的一行注释\n", 0);
        let (deleted, changed) = replace(&inserted, at, at + "    // 新增的一行注释\n".len(), "");
        let (spans, stats) = session
            .highlight_with_stats(&deleted, Some("rust"), Some(changed))
            .unwrap();
        assert!(stats.incremental);
        assert_same(&spans, &full(&deleted, "rust"), "删除一行后");
    }

    #[test]
    fn breaking_block_comment_state_still_matches_full() {
        let text = sample_rust(10 * 1024);
        let mut session = SyntectSession::new();
        session.highlight(&text, Some("rust"), None).unwrap();

        // 在块注释内部提前闭合，把后文的语法状态整体打乱——收敛点必须
        // 一路后移，但结果仍要与全量逐片段一致。
        let at = find_nth(&text, "跨行延续，含", 10);
        let (next, changed) = replace(&text, at, at, "*/ 提前闭合 ");
        let (spans, stats) = session
            .highlight_with_stats(&next, Some("rust"), Some(changed))
            .unwrap();
        assert!(stats.incremental);
        assert_same(&spans, &full(&next, "rust"), "块注释状态被打断");
    }

    #[test]
    fn random_edit_sequence_matches_full_every_step() {
        let mut text = sample_rust(10 * 1024);
        let mut session = SyntectSession::new();
        session.highlight(&text, Some("rust"), None).unwrap();

        // 固定的确定性序列（不引入随机源，保证可复现）：等长替换、插入、删除交替。
        for step in 0..12 {
            let (next, changed) = match step % 3 {
                0 => {
                    let at = find_nth(&text, "generated_", step * 7 + 5);
                    replace(&text, at, at + "generated_".len(), "produced__")
                }
                1 => {
                    let at = find_nth(&text, "    Ok(total + raw.len())", step * 3 + 2);
                    replace(&text, at, at, "    // 插入\n")
                }
                _ => {
                    let at = find_nth(&text, "    let mut total = 0usize;\n", step * 2 + 1);
                    replace(&text, at, at + "    let mut total = 0usize;\n".len(), "")
                }
            };
            let (spans, stats) = session
                .highlight_with_stats(&next, Some("rust"), Some(changed))
                .unwrap();
            assert!(stats.incremental, "第 {step} 步应走增量");
            assert_same(&spans, &full(&next, "rust"), &format!("第 {step} 步"));
            text = next;
        }
    }

    #[test]
    fn prefix_mismatch_falls_back_to_full() {
        let text = sample_rust(10 * 1024);
        let mut session = SyntectSession::new();
        session.highlight(&text, Some("rust"), None).unwrap();

        // 声称只改了后面，实际改了开头：前缀校验必须拦下并退化为全量。
        let at = find_nth(&text, "generated_", 1);
        let (mut next, changed) = replace(&text, at, at + "generated_".len(), "produced__");
        // 插入点必须落在 `changed.start` **之前**，否则并不违反「前缀未变」，
        // 属于调用方把变更区间报小了——那不是本实现的契约责任。
        next.insert_str(0, "// 偷偷在前面插入\n");
        let (spans, stats) = session
            .highlight_with_stats(&next, Some("rust"), Some(changed))
            .unwrap();
        assert!(!stats.incremental, "前缀不一致必须退化为全量");
        assert_same(&spans, &full(&next, "rust"), "前缀不一致退化");
    }

    #[test]
    fn language_switch_rebuilds() {
        let text = sample_rust(4 * 1024);
        let mut session = SyntectSession::new();
        session.highlight(&text, Some("rust"), None).unwrap();

        let changed = range(0, 0);
        let (spans, stats) = session
            .highlight_with_stats(&text, Some("javascript"), Some(changed))
            .unwrap();
        assert!(!stats.incremental, "换语言必须全量");
        assert_same(&spans, &full(&text, "javascript"), "换语言");
    }

    /// PoC 收益基准。默认 `#[ignore]`，因为 debug 下 syntect 慢数倍、
    /// 数字没有参考价值。用 release 显式运行：
    ///
    /// ```text
    /// cargo test --release --lib incremental::tests::bench -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "收益基准：需 release 下显式运行"]
    fn bench_incremental_vs_full_on_16kb() {
        const ROUNDS: usize = 30;
        // 16KB 而非 10KB：30 轮连续编辑每轮消耗一个编辑点，样本太小时
        // 后段会「无点可改」而测不到稳态。
        let text = sample_rust(16 * 1024);
        let highlighter = SyntectHighlighter::new();

        // 预热：syntect 的 LazyContexts 首次加载约 24ms/语言，不能计入稳态。
        let _ = highlighter.highlight(&text, Some("rust")).unwrap();

        // 全量：每次都完整重解。
        let mut full_samples = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let started = Instant::now();
            let spans = highlighter.highlight(&text, Some("rust")).unwrap();
            std::hint::black_box(&spans);
            full_samples.push(started.elapsed());
        }

        // 增量：模拟连续编辑，每轮改一个不同的行（长度不变）。
        let mut session = SyntectSession::new();
        let mut current = text.clone();
        session.highlight(&current, Some("rust"), None).unwrap();
        let mut incremental_samples = Vec::with_capacity(ROUNDS);
        for round in 0..ROUNDS {
            let needle = "total += 2";
            // 实时取编辑点：已改过的位置自然消失，不会重复。
            // 去掉末尾 3 个——文末不足 25 行时没有旧检查点可收敛，
            // 那是设计内的退化路径，不该混进「稳态改一行」的基准。
            let hits: Vec<usize> = current.match_indices(needle).map(|(at, _)| at).collect();
            assert!(hits.len() > 4, "第 {round} 轮：样本的编辑点已耗尽");
            let usable = &hits[..hits.len() - 3];
            let at = usable[round % usable.len()];
            let (next, changed) = replace(&current, at, at + needle.len(), "total += 3");
            let started = Instant::now();
            let (spans, stats) = session
                .highlight_with_stats(&next, Some("rust"), Some(changed))
                .unwrap();
            let elapsed = started.elapsed();
            std::hint::black_box(&spans);
            assert!(
                stats.incremental,
                "第 {round} 轮应走增量（at={at}，changed={}..{}）",
                changed.start().value(),
                changed.end().value()
            );
            incremental_samples.push((elapsed, stats));
            current = next;
        }

        let median = |mut values: Vec<f64>| -> f64 {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap());
            values[values.len() / 2]
        };
        let full_ms = median(full_samples.iter().map(|d| d.as_secs_f64() * 1e3).collect());
        let incremental_ms = median(
            incremental_samples
                .iter()
                .map(|(d, _)| d.as_secs_f64() * 1e3)
                .collect(),
        );
        let parsed: Vec<usize> = incremental_samples
            .iter()
            .map(|(_, s)| s.parsed_lines)
            .collect();
        let total_lines = incremental_samples[0].1.total_lines;
        let ratio = full_ms / incremental_ms;

        println!(
            "[metrics] 16KB 全量 {full_ms:.3} ms · 增量 {incremental_ms:.3} ms · \
             加速 {ratio:.2}× · 重解行数 {}/{total_lines}（中位）",
            median(parsed.iter().map(|n| *n as f64).collect())
        );

        assert!(ratio >= 5.0, "PoC 门槛：加速比应 ≥ 5×，实测 {ratio:.2}×");
    }
}
