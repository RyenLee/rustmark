//! 单次编辑差异（`diff_replace`）与插入时的自动变换（括号配对 / 列表续行）。

use super::cursor::{byte_to_char, char_to_byte, line_before, matching_close};
use super::line::list_marker;

/// 一次文本编辑的结果：在旧文本的 `[byte_start, byte_end)` 区间替换为 `inserted`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    /// 旧文本中被替换的起点（字节）。
    pub byte_start: usize,
    /// 旧文本中被替换的终点（字节，不含）。
    pub byte_end: usize,
    /// 插入的新文本。
    pub inserted: String,
    /// 编辑后光标应落在的**字符**索引（用于在 egui 中重设光标）。
    pub cursor_char: usize,
    /// 编辑后选区锚点应落在的**字符**索引（`None` 表示无选区）。
    pub anchor_char: Option<usize>,
}

/// 计算从 `old` 到 `new` 的最小替换。
///
/// 在字符层面求最长公共前缀与后缀，二者之间的部分即被替换区间。
/// 返回 `None` 表示无变化（文本相同）。
///
/// 这是编辑区「把 egui 改过的字符串同步回领域」的核心：每次按键后，
/// egui 把 `self.editor_text` 原地改成 `new`，我们用本函数得到一条
/// [`TextEdit`]，再发 [`crate::application::document::commands::ReplaceRangeCommand`]。
pub fn diff_replace(old: &str, new: &str) -> Option<TextEdit> {
    if old == new {
        return None;
    }
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let n = old_chars.len();
    let m = new_chars.len();

    // 最长公共前缀（字符数）。
    let mut prefix = 0;
    while prefix < n && prefix < m && old_chars[prefix] == new_chars[prefix] {
        prefix += 1;
    }
    // 最长公共后缀（字符数，且不与前缀重叠）。
    let mut suffix = 0;
    while suffix < (n - prefix)
        && suffix < (m - prefix)
        && old_chars[n - 1 - suffix] == new_chars[m - 1 - suffix]
    {
        suffix += 1;
    }

    let removed_char_start = prefix;
    let removed_char_end = n - suffix;
    let inserted_char_start = prefix;
    let inserted_char_end = m - suffix;

    let byte_start = char_to_byte(old, removed_char_start);
    let byte_end = char_to_byte(old, removed_char_end);
    let inserted: String = new_chars[inserted_char_start..inserted_char_end]
        .iter()
        .collect();

    // 新文本里光标默认落在插入文本之后。
    let cursor_char = inserted_char_end;
    let anchor_char = if inserted_char_start == inserted_char_end {
        // 纯删除：光标落在删除起点。
        Some(removed_char_start)
    } else {
        None
    };

    Some(TextEdit {
        byte_start,
        byte_end,
        inserted,
        cursor_char,
        anchor_char,
    })
}

/// 在一处「单次插入」编辑上叠加自动变换：括号配对、列表续行。
///
/// 返回是否改动过 `edit`。这些变换只作用于**纯插入**（选区为空），
/// 因为它们假定「用户刚敲了一个键」。
///
/// - **括号配对**：插入内容恰为一个开括号 → 追加对应闭括号，光标停在两者中间。
/// - **列表续行**：插入内容含 `\n` 且前一行为列表项 → 追加下一行标记。
///
/// 注意：`edit.byte_start` / `byte_end` 始终以**旧文本**为基准，变换后不变，
/// 只有 `inserted` 与 `cursor_char` 会变——下游据此发 [`crate::application::document::commands::ReplaceRangeCommand`]。
pub fn transform_insert(old: &str, edit: &mut TextEdit) -> bool {
    let mut changed = false;

    // 括号配对
    if edit.inserted.chars().count() == 1 && edit.byte_start == edit.byte_end {
        // 上一条件已保证恰好一个字符；用 `and_then` 串联而非 `unwrap()`：
        // 取不到或无需配对时跳过，不影响后续的列表续行判断（V11-12）。
        if let Some(close) = edit.inserted.chars().next().and_then(matching_close) {
            edit.inserted.push(close);
            // 光标停在开括号之后、闭括号之前
            edit.cursor_char = byte_to_char(old, edit.byte_start) + 1;
            changed = true;
        }
    }

    // 列表续行：插入内容含换行且前一行是列表项
    if edit.inserted.contains('\n') {
        let line = line_before(old, edit.byte_start);
        if let Some((mlen, marker)) = list_marker(line) {
            // 空列表项（行内只有标记，如 "- " / "  - " / "1. "）——
            // 标准编辑器行为：这次回车应当**退出列表**，而不是再续一个空项。
            // 否则用户永远无法用回车离开列表（每按一次都多一个 "- "）。
            // 做法：把替换区间起点拉到标记起始（即本行行首），连同标记一起删掉，
            // 只插入一个换行；光标落到该空行。仅对「纯插入、无选区」生效，
            // 选区 + 回车（替换选中内容）不触发此分支。
            //
            // 「空项」判定：当前行的标记之后是否还有非空内容。
            // 注意 `list_marker` 返回的是 `(缩进长度, 下一行标记)`——无序时首字段
            // 只是缩进长（故当前标记长 = `mlen + 2`，即「缩进 + "- "」）；
            // 有序时首字段已是当前完整标记长（`mlen`），但「下一行标记」是递增后的
            // （"2. "）。两者都不能直接当「当前标记长」来切尾巴，故据「下一行标记
            // 是否以数字开头」区分：有序 → 当前标记长 = `mlen`；无序 → `mlen + 2`。
            let ordered = marker.chars().next().is_some_and(|c| c.is_ascii_digit());
            let cur_marker_len = if ordered { mlen } else { mlen + 2 };
            let line_start = edit.byte_start.saturating_sub(line.len());
            let after_marker = &line[cur_marker_len.min(line.len())..];
            let is_empty_item = edit.byte_start == edit.byte_end && after_marker.trim().is_empty();
            if is_empty_item {
                edit.byte_start = line_start;
                edit.inserted = "\n".to_string();
                edit.cursor_char = byte_to_char(old, line_start) + 1;
            } else {
                edit.inserted.push_str(&marker);
                edit.cursor_char += marker.chars().count();
            }
            changed = true;
        }
    }

    changed
}
