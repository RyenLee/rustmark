//! 增量高亮的纯函数：变更区间计算与片段平移（从 `incremental.rs` 抽出；R-07 Batch 1）。

use rustmark_core::domain::markdown::value_objects::SyntaxSpan;
use rustmark_core::shared::geometry::{ByteOffset, TextRange};

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
pub(crate) fn changed_range(old: &str, new: &str) -> TextRange {
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
pub(crate) fn push_shifted(spans: &mut Vec<SyntaxSpan>, span: SyntaxSpan, delta: i64) {
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

/// 一次编辑在「旧/新」双坐标下的跨度，供增量高亮在多处编辑下算出每个旧检查点
/// 在新文本里的正确位置（累计偏移），从而安全复用未变内部孤岛。
#[derive(Clone, Copy, Debug)]
pub(crate) struct EditRun {
    pub(crate) old_start: usize,
    pub(crate) old_end: usize,
    pub(crate) new_start: usize,
    pub(crate) new_end: usize,
}

/// 累计偏移：所有「完全落在 `old_offset` 之前」的编辑带来的新增字节数之和。
///
/// 正值 = 插入/加长，负值 = 删除/缩短。某旧检查点 `C` 在新文本里的正确位置 =
/// `C.offset + cumulative_delta(runs, C.offset)`。多处散布编辑下偏移非均匀，必须
/// 用编辑区间逐点累计，不能用全局 `delta` 近似。
pub(crate) fn cumulative_delta(runs: &[EditRun], old_offset: usize) -> i64 {
    let mut delta: i64 = 0;
    for r in runs {
        if r.old_end <= old_offset {
            delta += (r.new_end - r.new_start) as i64 - (r.old_end - r.old_start) as i64;
        }
    }
    delta
}

/// 把新文本坐标 `pos` 映射回旧文本坐标。
///
/// 编辑段（run）内部的新坐标没有唯一的旧坐标（旧内容已被替换/删除），此时返回 `None`
/// —— 收敛判定应跳过、照常逐行解析。干净段（run 之间的区域）内新旧逐字节一致、偏移恒定，
/// 直接减该段累计偏移即可。
///
/// 用于增量高亮在多处编辑下，把「当前新行首」正确对应到「旧检查点表」里的旧坐标，从而安全复用
/// 未变内部孤岛。runs 必须按 `old_start` / `new_start` 升序（[`edit_runs`] 保证）。
pub(crate) fn old_offset_for_new(runs: &[EditRun], pos: usize) -> Option<usize> {
    let mut prev_new_end: i64 = 0;
    let mut prev_old_end: i64 = 0;
    for r in runs {
        if pos < r.new_start {
            let shift = prev_new_end - prev_old_end;
            return Some((pos as i64 - shift) as usize);
        }
        if pos < r.new_end {
            return None;
        }
        prev_new_end = r.new_end as i64;
        prev_old_end = r.old_end as i64;
    }
    let shift = prev_new_end - prev_old_end;
    Some((pos as i64 - shift) as usize)
}

/// 旧文本坐标 `old_offset` 是否严格落在某编辑段内部（两端边界算干净）。
///
/// 检查点若夹在编辑段内部，其旧内容已不存在于新文本，不可作为复用锚点；返回 `true` 表示
/// 「在编辑段内、不可复用」。
pub(crate) fn old_in_edit(runs: &[EditRun], old_offset: usize) -> bool {
    runs.iter()
        .any(|r| old_offset > r.old_start && old_offset < r.old_end)
}

/// 计算两次文本间的「编辑区间」列表（旧/新双坐标）。
///
/// 与 [`changed_range`] 的区别：[`changed_range`] 只给最小包围区间（区间内未变片段被
/// 忽略，增量引擎只能整体重解）；本函数把内部也拆成「变更段」与「未变段」，让未变内部
/// 孤岛在多处编辑（如 replace-all、重排版）下仍可被安全复用。
///
/// # 安全性
///
/// 返回的编辑区间**覆盖所有变更字节**（可能过覆盖，但绝不会漏覆盖）。未变段是否真的可复用，
/// 由增量引擎在复用前再用 memcmp + 解析状态双重校验——即便本函数的对齐不完美，也不会产生
/// 错误的高亮，最多只是少复用一些（退化为重解）。
///
/// # 大小守卫
///
/// 内部（剔除公共前后缀后）超过 `128KiB` 时退化为单条覆盖区间：此时不再尝试拆内部孤岛，
/// 增量引擎只复用「头部未变前缀」与「尾部未变后缀」（与 [`changed_range`] 行为一致），避免对
/// 超大差异做行级 diff。增量仅在 ≤ `MAX_INCREMENTAL_BYTES`（500KiB）文档上启用，故内部有上限。
pub(crate) fn edit_runs(old: &str, new: &str) -> Vec<EditRun> {
    let ob = old.as_bytes();
    let nb = new.as_bytes();

    // 公共前缀（收缩到 char boundary）。
    let mut p = 0usize;
    let pl = ob.len().min(nb.len());
    while p < pl && ob[p] == nb[p] {
        p += 1;
    }
    while p > 0 && !new.is_char_boundary(p) {
        p -= 1;
    }
    // 公共后缀（在剔除前缀后的剩余区间内）。
    let mut s = 0usize;
    let sl = (ob.len() - p).min(nb.len() - p);
    while s < sl && ob[ob.len() - 1 - s] == nb[nb.len() - 1 - s] {
        s += 1;
    }
    while s > 0 && !new.is_char_boundary(nb.len() - s) {
        s -= 1;
    }

    let o_lo = p;
    let o_hi = ob.len() - s;
    let n_lo = p;
    let n_hi = nb.len() - s;
    if o_lo >= o_hi || n_lo >= n_hi {
        return Vec::new(); // 内部为空：无编辑
    }

    let interior_old = &ob[o_lo..o_hi];
    let interior_new = &nb[n_lo..n_hi];
    const GUARD: usize = 128 * 1024;
    if interior_old.len() > GUARD || interior_new.len() > GUARD {
        // 退化：单条覆盖区间（旧坐标 [o_lo, o_hi) → 新坐标 [n_lo, n_hi)）。
        return vec![EditRun {
            old_start: o_lo,
            old_end: o_hi,
            new_start: n_lo,
            new_end: n_hi,
        }];
    }

    line_diff(interior_old, interior_new, o_lo, n_lo)
}

/// 把字节切片按行拆分（含行尾 `\n`），返回每行的 (相对起始, 长度)。
fn split_lines(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let end = match bytes[i..].iter().position(|&b| b == b'\n') {
            Some(pos) => i + pos + 1,
            None => bytes.len(),
        };
        lines.push((i, end - i));
        i = end;
    }
    lines
}

/// 行级 diff：在「公共前后缀已剔除」的内部上，找出「未变行段」的补集（编辑段）。
///
/// 对齐用内容哈希 + 贪心首匹配；任何对齐偏差都会在增量引擎复用前的 memcmp 校验中被拦下
/// （最多少复用，绝不产生错误高亮）。覆盖性由「未匹配行即变更」保证。
fn line_diff(a: &[u8], b: &[u8], a_base: usize, b_base: usize) -> Vec<EditRun> {
    let a_lines = split_lines(a);
    let b_lines = split_lines(b);
    if a_lines.is_empty() || b_lines.is_empty() {
        return vec![EditRun {
            old_start: a_base,
            old_end: a_base + a.len(),
            new_start: b_base,
            new_end: b_base + b.len(),
        }];
    }

    // a 的行内容 → 可匹配的剩余位置池。
    let mut pool: std::collections::HashMap<&[u8], Vec<usize>> = std::collections::HashMap::new();
    for (idx, (s, l)) in a_lines.iter().enumerate() {
        pool.entry(&a[*s..*s + *l]).or_default().push(idx);
    }

    // 贪心：b 的每一行在 a 的剩余池中找第一个未用同内容行匹配。
    let mut a_used = vec![false; a_lines.len()];
    let mut b_match: Vec<Option<usize>> = vec![None; b_lines.len()];
    for (bi, (bs, bl)) in b_lines.iter().enumerate() {
        let content = &b[*bs..*bs + *bl];
        if let Some(slots) = pool.get_mut(content)
            && let Some(pos) = slots.iter().position(|&ai| !a_used[ai])
        {
            let ai = slots[pos];
            a_used[ai] = true;
            b_match[bi] = Some(ai);
        }
    }

    // 找干净连续段：b 的连续若干行，逐行匹配到 a 的连续若干行（步长均为 1）。
    // 干净段之间的区域即编辑段（覆盖所有未匹配字节）。
    let mut runs: Vec<EditRun> = Vec::new();
    let mut i = 0usize;
    while i < b_lines.len() {
        if let Some(a0) = b_match[i] {
            let mut k = 0usize;
            while i + k < b_lines.len() && b_match[i + k] == Some(a0 + k) {
                k += 1;
            }
            i += k; // 跳过干净段（未变，不计编辑）
        } else {
            let j = {
                let mut j = i;
                while j < b_lines.len() && b_match[j].is_none() {
                    j += 1;
                }
                j
            };
            let new_start = b_lines[i].0 + b_base;
            let new_end = if j < b_lines.len() {
                b_lines[j].0 + b_base
            } else {
                b_base + b.len()
            };
            let old_start = if i == 0 {
                a_base
            } else {
                // 前一行必已匹配（i>0 时循环保证）；万一为 None，退化为整段边界（过覆盖，最多少复用）。
                match b_match[i - 1] {
                    Some(prev_a) => a_lines[prev_a].0 + a_lines[prev_a].1 + a_base,
                    None => a_base + a.len(),
                }
            };
            let old_end = if j < b_lines.len() {
                // 后一行必已匹配（j<b.len 时循环保证）；万一为 None，退化为整段边界。
                match b_match[j] {
                    Some(next_a) => a_lines[next_a].0 + a_base,
                    None => a_base + a.len(),
                }
            } else {
                a_base + a.len()
            };
            runs.push(EditRun {
                old_start,
                old_end,
                new_start,
                new_end,
            });
            i = j;
        }
    }
    runs
}
