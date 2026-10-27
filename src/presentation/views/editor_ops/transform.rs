//! 单次编辑差异（`diff_replace`）与插入时的自动变换（括号配对 / 列表续行）。

use super::cursor::{byte_to_char, line_before, matching_close};
use super::line::list_marker;

/// 一次文本编辑的结果：在旧文本的 `[byte_start, byte_end)` 区间替换为 `inserted`。
///
/// # 它住在 `rustmark-ui` 而不是这里
///
/// 编辑区的 trait 方法 `CommandPort::flush_edit` 要拿它做参数，而 trait 在
/// `rustmark-ui`——**契约的参数类型必须与契约同侧**，否则 trait 就得反向
/// 依赖组合根（root 的 `views/`）。故定义搬进
/// `rustmark_ui::presentation::view_context::TextEdit`，此处 `pub use` 转发，
/// 公开路径 `rustmark::presentation::views::editor_ops::TextEdit` **逐字不变**。
pub use rustmark_ui::presentation::view_context::TextEdit;

/// 计算从 `old` 到 `new` 的最小替换。
///
/// 求最长公共前缀与后缀，二者之间的部分即被替换区间。
/// 返回 `None` 表示无变化（文本相同）。
///
/// 这是编辑区「把 egui 改过的字符串同步回领域」的核心：每次按键后，
/// egui 把 `self.editor_text` 原地改成 `new`，我们用本函数得到一条
/// [`TextEdit`]，再发 [`rustmark_core::application::document::commands::ReplaceRangeCommand`]。
///
/// # 为什么在**字节层**求前后缀（问题 049 的修复）
///
/// 本函数挂在「每击键一次」的路径上（`editor_view::show` 的 `changed()` 分支），
/// 1MB 文档下必须避免任何 O(全文) 的**分配**与**解码**。旧实现在字符层比较：
/// 两侧各 `chars().collect::<Vec<char>>()`（35 万个 char，约 2 × 1.4MB 分配 + 两遍
/// UTF-8 解码），再用两次 `char_to_byte`（内部 `char_indices().nth(k)`，O(k)）
/// 把字符索引换回字节——1MB 上实测 **6.8ms**，仅次于状态栏的字数统计。
///
/// 字节层只需逐字节比较：**不分配、不解码**，且 UTF-8 的续字节（`0x80..=0xBF`）
/// 与 ASCII 都参与等值比较，故得到的公共前后缀与字符层结论一致（中文字符的
/// 首字节不同 → 字符层也在同一处停下）。
///
/// 唯一的坑是**字节前缀可能落在字符中间**（如 `é` ↔ `è` 只差末字节，公共前缀
/// 停在 2 字节字符的第二字节上）。UTF-8 是自同步编码：某位置是否为字符边界
/// 只取决于**该位置自身的字节**是否为续字节（加上文末），而 `0..prefix` 两侧
/// 逐字节相同，故边界判定在两侧一致——直接拿 `old.is_char_boundary` 把前缀
/// 回退、把后缀收拢到字符边界即可，无需构造任何字符级缓冲。
pub fn diff_replace(old: &str, new: &str) -> Option<TextEdit> {
    if old == new {
        return None;
    }
    let (ob, nb) = (old.as_bytes(), new.as_bytes());
    let (n, m) = (ob.len(), nb.len());

    // 最长公共前缀（字节）。
    let mut prefix = 0;
    while prefix < n && prefix < m && ob[prefix] == nb[prefix] {
        prefix += 1;
    }
    // 回退到字符边界（至多 3 步）。
    let mut byte_start = prefix;
    while !old.is_char_boundary(byte_start) {
        byte_start -= 1;
    }

    // 最长公共后缀（字节，且不与前缀重叠）。
    let mut suffix = 0;
    while suffix < n - prefix && suffix < m - prefix && ob[n - 1 - suffix] == nb[m - 1 - suffix] {
        suffix += 1;
    }
    // 收拢到字符边界（至多 3 步；只会让公共后缀变短）。
    let mut byte_end = n - suffix;
    while !old.is_char_boundary(byte_end) {
        byte_end += 1;
    }

    // `new` 中的插入区间终点：公共后缀在两串里占**同样多的字节**。
    let inserted_byte_end = m - (n - byte_end);
    let inserted = new[byte_start..inserted_byte_end].to_string();

    // 光标 / 锚点是**字符**索引（egui 口径）。前缀在两侧逐字节相同，
    // 故「前缀的字符数」在 old / new 上是同一个值，只需算一次；
    // 插入文本之后的字符索引 = 该值 + 插入内容的字符数。
    let prefix_chars = byte_to_char(old, byte_start);
    let cursor_char = prefix_chars + inserted.chars().count();
    // 纯删除（没有插入任何内容）时光标落在删除起点；否则由 egui 自己算。
    let anchor_char = inserted.is_empty().then_some(prefix_chars);

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
/// 只有 `inserted` 与 `cursor_char` 会变——下游据此发 [`rustmark_core::application::document::commands::ReplaceRangeCommand`]。
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
