//! 编辑区纯文本变换逻辑（S6-03）。
//!
//! # 为什么这些是纯函数
//!
//! 编辑区的「缩进 / 反缩进 / 移动行 / 列表续行 / 括号配对」都是**对一段文本的可预测变换**，
//! 不依赖 egui、不依赖领域层。把它们抽成纯函数有三个好处：
//!
//! 1. **可测**：无需 GUI 即可逐条验证多字节边界、选区跨行等易错点（见本文件末尾 `#[cfg(test)]`）。
//! 2. **可移植**：若将来把编辑面从 egui 换成 `cosmic-text`，这些变换一行都不用改
//!    （S6-03 风险预案要求的「输入法/编辑器隔离层」的底层支撑）。
//! 3. **单一真相**：视图只负责「把旧文本变成新文本」的意图，真正的写入永远走领域命令。
//!
//! 所有偏移语义统一为：**选区/光标用字符索引，命令用字节偏移**。
//! 原因见 [`byte_to_char`] / [`char_to_byte`]——`İ`（1 字符 → 2 字节）、
//! emoji、组合字符都会让「字符数 ≠ 字节数」，直接用字节切文本会 panic。
//!
//! # 哪些规则**不该**出现在这里（S6-07 之后的收敛）
//!
//! 本文件曾自带 `indent` / `outdent` 的实现，与领域
//! `EditingService::indent_lines` / `outdent_lines` 形成**双份真相**——
//! 而领域版更完备（处理制表符、缩进不给空行留尾随空白）。
//! S6-07 已把这两处收敛回领域，本文件只保留：
//!
//! - **坐标系换算**（`char_to_byte` / `byte_to_char`）：egui 只认字符索引，
//!   领域只认字节偏移，这个鸿沟只能由表现层填；
//! - **egui 特有逻辑**（`diff_replace` / `transform_insert`）：与 `TextEdit`
//!   的撤销栈、括号配对、列表续行走同一条 `keypress` 通路，搬不进领域。

/// 把字符索引转换为字节偏移。
///
/// `char_idx` 超过文本长度时返回 `text.len()`（夹到末尾，不越界）。
pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    // 第 `char_idx` 个字符的起始字节；越界则夹到文末。
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(byte, _)| byte)
}

/// 把字节偏移转换为字符索引（夹到 `[0, 字符总数]`）。
pub fn byte_to_char(text: &str, byte_idx: usize) -> usize {
    if byte_idx == 0 {
        return 0;
    }
    let clamped = byte_idx.min(text.len());
    text[..clamped].chars().count()
}

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

/// 把光标所在行整体上移/下移一行（`dir = -1` 上移，`+1` 下移）。
///
/// 仅对单光标有意义；有选区时退化为对光标所在行操作。
/// 返回 `None` 表示已在边界（首行上移 / 末行下移），无需改动。
pub fn move_line(text: &str, cursor: usize, dir: i8) -> Option<(String, usize)> {
    if dir == 0 {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    // 找到光标所在行的 [line_start, line_end] 与相邻行的范围。
    let n = chars.len();
    let mut line_start = cursor;
    while line_start > 0 && chars[line_start - 1] != '\n' {
        line_start -= 1;
    }
    let mut line_end = cursor;
    while line_end < n && chars[line_end] != '\n' {
        line_end += 1;
    }
    let has_trailing_newline = line_end < n; // line_end 指向 '\n'
    let cur_start = line_start;
    let cur_line: String = chars[cur_start..line_end].iter().collect();

    let (other_start, other_end) = if dir < 0 {
        if line_start == 0 {
            return None;
        }
        // 上一行
        let mut s = line_start;
        while s > 0 && chars[s - 1] != '\n' {
            s -= 1;
        }
        (s, line_start) // 上一段 [s, line_start) 不含其后的 '\n'
    } else {
        if !has_trailing_newline {
            return None;
        }
        let mut e = line_end + 1;
        while e < n && chars[e] != '\n' {
            e += 1;
        }
        (line_end + 1, e) // 下一行 [line_end+1, e)
    };
    let other_line: String = chars[other_start..other_end].iter().collect();

    // 组装：把两行交换（保持换行符结构不变）。
    let mut out = String::with_capacity(text.len());
    if dir < 0 {
        // 上移：当前行与上一行交换，当前行移动到「上方」。
        out.push_str(&chars[..other_start].iter().collect::<String>());
        out.push_str(&cur_line);
        out.push('\n');
        out.push_str(&other_line);
        if line_end < n {
            out.push('\n');
            out.push_str(&chars[(line_end + 1).min(n)..].iter().collect::<String>());
        }
    } else {
        // 下移：当前行与下一行交换，当前行移动到「下方」。
        out.push_str(&chars[..line_start].iter().collect::<String>());
        out.push_str(&other_line);
        out.push('\n');
        out.push_str(&cur_line);
        if other_end < n {
            out.push('\n');
            out.push_str(&chars[(other_end + 1).min(n)..].iter().collect::<String>());
        }
    }

    // 新光标：保持「当前行」内容上同偏移；交换后当前行起点为 `other_start`。
    let new_cursor = other_start + (cursor - cur_start);
    Some((out, new_cursor))
}

/// 若 `line` 以 Markdown 列表标记开头，返回 `(缩进字节长度, 标记文本)`。
///
/// 支持无序列表 `-` / `*` / `+`（后跟空格）与有序列表 `1.` / `2)` 等形式。
/// 用于「回车续列表」：在前一行是列表项时自动补出下一行标记。
pub fn list_marker(line: &str) -> Option<(usize, String)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= line.len() {
        return None;
    }
    let rest = &line[i..];
    let marker = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "));
    if marker.is_some() {
        return Some((i, line[..i + 2].to_string()));
    }
    // 有序列表：数字 + '.' 或 ')' + 空格
    //
    // 数字段必须**整段**剥出来：只剥一位的话，`10. item` 会变成
    // `0. item`，后续 `. ` 前缀匹配不上，回车就续不出 `11. `——
    // 症状是「列表写到第十条以后，回车不再补序号」。
    let digits_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let after_num = rest[digits_end..]
        .strip_prefix(". ")
        .map(|r| (r, "."))
        .or_else(|| rest[digits_end..].strip_prefix(") ").map(|r| (r, ")")));
    if digits_end > 0
        && let Some((_, sep)) = after_num
    {
        // 分隔符位置：`after_num` 是由 `rest` 剥离得到的，故必然能找到；
        // 用 `?` 处理而非 `unwrap()`——找不到即放弃本次续行（V11-12）。
        let idx = rest.find(sep)?;
        let mlen = i + idx + 2; // 缩进 + 数字 + 分隔 + 空格
        // 递增序号
        let num: String = rest[..idx].chars().collect();
        if let Ok(n) = num.parse::<u32>() {
            return Some((mlen, format!("{}{} ", n + 1, sep)));
        }
    }
    None
}

/// 括号配对：若用户刚输入一个开括号，返回应自动补出的闭括号字符。
pub fn matching_close(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '<' => Some('>'),
        _ => None,
    }
}

/// 返回 `byte` 之前的完整行文本（即上一个 `\n` 之后到 `byte` 为止）。
///
/// 用于「回车续列表」时判断前一行是否为列表项。
pub fn line_before(text: &str, byte: usize) -> &str {
    let clamped = byte.min(text.len());
    let start = text[..clamped].rfind('\n').map(|i| i + 1).unwrap_or(0);
    &text[start..clamped]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_char_roundtrip_multibyte() {
        // 字符索引：你(0) 好(1) a(2) 𝄞(3) b(4)
        // 字节偏移：好=3，a=6，𝄞=7（4 字节），b=11，总长 12
        let t = "你好a𝄞b";
        assert_eq!(char_to_byte(t, 0), 0);
        assert_eq!(byte_to_char(t, 0), 0);
        assert_eq!(char_to_byte(t, 1), 3); // 好
        assert_eq!(byte_to_char(t, 3), 1);
        assert_eq!(char_to_byte(t, 2), 6); // a
        assert_eq!(byte_to_char(t, 6), 2);
        assert_eq!(char_to_byte(t, 3), 7); // 𝄞（4 字节）
        assert_eq!(byte_to_char(t, 7), 3);
        assert_eq!(char_to_byte(t, 4), 11); // b
        assert_eq!(byte_to_char(t, 11), 4);
        assert_eq!(char_to_byte(t, t.chars().count()), t.len());
    }

    #[test]
    fn diff_pure_insert_at_middle() {
        let old = "ab";
        let new = "aXb";
        let e = diff_replace(old, new).unwrap();
        assert_eq!(e.byte_start, 1);
        assert_eq!(e.byte_end, 1);
        assert_eq!(e.inserted, "X");
        assert_eq!(e.cursor_char, 2);
    }

    #[test]
    fn diff_multibyte_insert_keeps_byte_boundaries() {
        // 在「你」与「好」之间插入 X（不能把中文字节切开）
        let old = "你好";
        let new = "你X好";
        let e = diff_replace(old, new).unwrap();
        assert_eq!(e.byte_start, 3);
        assert_eq!(e.byte_end, 3);
        assert_eq!(e.inserted, "X");
        // 用字节区间切片 old 必须合法且恰好是空
        assert_eq!(&old[e.byte_start..e.byte_end], "");
        // 替换后拼回应等于 new
        let rebuilt = format!(
            "{}{}{}",
            &old[..e.byte_start],
            e.inserted,
            &old[e.byte_end..]
        );
        assert_eq!(rebuilt, new);
    }

    #[test]
    fn diff_replace_across_multibyte() {
        let old = "你好世界";
        let new = "你X界"; // 删掉「好世」，插入 X
        let e = diff_replace(old, new).unwrap();
        let rebuilt = format!(
            "{}{}{}",
            &old[..e.byte_start],
            e.inserted,
            &old[e.byte_end..]
        );
        assert_eq!(rebuilt, new);
    }

    #[test]
    fn move_line_down_swaps() {
        let text = "a\nb\nc";
        let (out, cur) = move_line(text, 0, 1).unwrap();
        assert_eq!(out, "b\na\nc");
        assert_eq!(cur, 2); // 原 'a' 现位于第 2 行起点
    }

    #[test]
    fn move_line_up_at_top_is_noop() {
        let text = "a\nb";
        assert!(move_line(text, 0, -1).is_none());
    }

    #[test]
    fn list_marker_unordered() {
        let (indent_len, marker) = list_marker("  - hello").unwrap();
        assert_eq!(indent_len, 2);
        assert_eq!(marker, "  - ");
    }

    #[test]
    fn list_marker_ordered_increments() {
        let (_, marker) = list_marker("1. first").unwrap();
        assert_eq!(marker, "2. ");
    }

    #[test]
    fn list_marker_multi_digit_increments() {
        // 两位数序号：整段数字都要剥出来。只剥一位的话 `10. ` 会被看成
        // `0. `，`. ` 前缀匹配不上 → 回车续不出 `11. `（V11-13 实测）。
        let (mlen, marker) = list_marker("10. 第十条").unwrap();
        assert_eq!(marker, "11. ");
        assert_eq!(mlen, 4); // 缩进 0 + "10" + "." + " "
        let (mlen2, marker2) = list_marker("123) deep").unwrap();
        assert_eq!(marker2, "124) ");
        assert_eq!(mlen2, 5);
    }

    #[test]
    fn matching_close_pairs() {
        assert_eq!(matching_close('('), Some(')'));
        assert_eq!(matching_close('{'), Some('}'));
        assert_eq!(matching_close('"'), Some('"'));
        assert_eq!(matching_close('x'), None);
    }

    #[test]
    fn transform_bracket_pair_inserts_close_and_places_cursor() {
        // 用户在「ab」光标 1 处输入 '('
        let old = "ab";
        let mut edit = TextEdit {
            byte_start: 1,
            byte_end: 1,
            inserted: "(".to_string(),
            cursor_char: 2,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "()");
        // 光标停在 '(' 之后（索引 2，字符层面）
        assert_eq!(edit.cursor_char, 2);
    }

    #[test]
    fn transform_list_continuation_appends_marker() {
        // 在一行无序列表项末尾回车
        let old = "- hello";
        let mut edit = TextEdit {
            byte_start: 7,
            byte_end: 7,
            inserted: "\n".to_string(),
            cursor_char: 8,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "\n- ");
        assert_eq!(edit.cursor_char, 8 + 2);
    }

    #[test]
    fn transform_noop_for_plain_insert() {
        let old = "ab";
        let mut edit = TextEdit {
            byte_start: 1,
            byte_end: 1,
            inserted: "x".to_string(),
            cursor_char: 2,
            anchor_char: None,
        };
        assert!(!transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "x");
    }

    #[test]
    fn transform_empty_list_item_exits_list() {
        // 光标点在一个空无序列表项 "- " 的末尾（字符 2 / 字节 2）。
        // 回车应「退出列表」：把 "- " 整段删掉、只留一个空行，光标落到该空行。
        let old = "- ";
        let mut edit = TextEdit {
            byte_start: 2,
            byte_end: 2,
            inserted: "\n".to_string(),
            cursor_char: 3,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        // 标记被移除、不再续出新的 "- "
        assert_eq!(edit.inserted, "\n");
        // 替换区间起点被拉到行首（标记起始），删掉整段 "- "
        assert_eq!(edit.byte_start, 0);
        // 光标落在空行上：行首字符索引 0 + 换行 1
        assert_eq!(edit.cursor_char, 1);
    }

    #[test]
    fn transform_indented_empty_list_item_exits() {
        // 缩进的空无序列表项 "  - "（2 空格 + "- " = 4 字节），光标在末尾。
        let old = "  - ";
        let mut edit = TextEdit {
            byte_start: 4,
            byte_end: 4,
            inserted: "\n".to_string(),
            cursor_char: 5,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        // 标记被移除、不再续出新的 "- "
        assert_eq!(edit.inserted, "\n");
        // 替换区间起点被拉到行首（标记起始），删掉整段 "  - "
        assert_eq!(edit.byte_start, 0);
        // 光标落在空行上：行首字符索引 0 + 换行 1
        assert_eq!(edit.cursor_char, 1);
    }

    #[test]
    fn transform_empty_ordered_list_item_exits() {
        // 空有序列表项 "1. "（光标在末尾，字节 3），回车退出列表。
        let old = "1. ";
        let mut edit = TextEdit {
            byte_start: 3,
            byte_end: 3,
            inserted: "\n".to_string(),
            cursor_char: 4,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "\n");
        assert_eq!(edit.byte_start, 0);
        assert_eq!(edit.cursor_char, 1);
    }

    #[test]
    fn transform_nonempty_list_item_continues() {
        // 非空列表项仍续行：回车补出下一行标记。
        let old = "- hello";
        let mut edit = TextEdit {
            byte_start: 7,
            byte_end: 7,
            inserted: "\n".to_string(),
            cursor_char: 8,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "\n- ");
        // 起点不变（只续行，不删已有内容）
        assert_eq!(edit.byte_start, 7);
    }
}
