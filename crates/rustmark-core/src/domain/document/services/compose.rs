//! [`EditingService`] 的内容编排能力：
//! 列表回车延续、任务勾选、链接 / 代码块 / 表格骨架插入。

use crate::shared::geometry::{ByteOffset, TextRange};

use super::super::value_objects::{Edit, EditPlan};
use super::EditingService;

impl EditingService {
    /// 在 `cursor` 处按回车时，是否应自动延续当前行的列表标记。
    ///
    /// 返回 `None` 表示当前行不是列表项（调用方按普通回车处理）。
    ///
    /// 边界语义：
    /// - 行内容为 `- `（标记后无内容）→ 返回**删除标记**的编辑（退出列表），而非新增一行；
    /// - 有序列表 `3. ` → 下一行为 `4. `；
    /// - 任务列表 `- [x] ` → 下一行为 `- [ ] `（新任务默认未完成）。
    pub fn continue_list_on_newline(text: &str, cursor: ByteOffset) -> Option<EditPlan> {
        let offset = cursor.value().min(text.len());
        let line = Self::line_range(text, offset);
        let raw = text.get(line.start().value()..line.end().value())?;
        let body = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed_start = body.len() - body.trim_start().len();
        let indent = body.get(..trimmed_start).unwrap_or("");
        let content = body.get(trimmed_start..).unwrap_or("");

        let marker = list_marker_of(content)?;
        if content.len() == marker.len() {
            // 只有标记、没有内容：退出列表而不是继续。
            let start = line.start().value() + trimmed_start;
            let edit = Edit::deletion(TextRange::new(
                ByteOffset::new(start),
                ByteOffset::new(start + marker.len()),
            ));
            return Some(EditPlan::plain(edit));
        }

        let next_marker = next_list_marker(marker);
        let insertion = format!("\n{indent}{next_marker}");
        Some(EditPlan::plain(Edit::insertion(cursor, insertion)))
    }

    /// 切换光标所在行的任务列表勾选状态。
    ///
    /// 语义：`- [ ] ` ⇄ `- [x] `；普通列表项 `- ` → `- [ ] `；非列表行 → `- [ ] `。
    pub fn toggle_task(text: &str, cursor: ByteOffset) -> EditPlan {
        let line = Self::line_range(text, cursor.value());
        let raw = text.get(line.as_usize_range()).unwrap_or("");
        let body = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed_start = body.len() - body.trim_start().len();
        let indent = body.get(..trimmed_start).unwrap_or("");
        let content = body.get(trimmed_start..).unwrap_or("");

        let new_content = if let Some(rest) = content.strip_prefix("- [ ] ") {
            format!("- [x] {rest}")
        } else if let Some(rest) = content.strip_prefix("- [x] ") {
            format!("- [ ] {rest}")
        } else if let Some(rest) = content.strip_prefix("- ") {
            format!("- [ ] {rest}")
        } else {
            format!("- [ ] {content}")
        };
        let replacement = format!("{indent}{new_content}");
        EditPlan::plain(Edit::new(line, replacement))
    }

    /// 把选区（或占位文本）转为 Markdown 链接，并把选区落在 URL 上。
    pub fn insert_link(text: &str, range: TextRange) -> EditPlan {
        let range = Self::align_range(text, range);
        let start = range.start().value().min(text.len());
        let end = range.end().value().min(text.len()).max(start);
        let selected = text.get(start..end).unwrap_or("");
        let label = if selected.is_empty() {
            "链接文本"
        } else {
            selected
        };
        let replacement = format!("[{label}](url)");
        let url_start = start + 1 + label.len() + 2;
        let selection = TextRange::new(ByteOffset::new(url_start), ByteOffset::new(url_start + 3));
        EditPlan::new(
            Edit::new(
                TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
                replacement,
            ),
            Some(selection),
        )
    }

    /// 把选区包进围栏代码块，并把选区落在代码内容上。
    pub fn insert_code_block(text: &str, range: TextRange) -> EditPlan {
        let range = Self::align_range(text, range);
        let start = range.start().value().min(text.len());
        let end = range.end().value().min(text.len()).max(start);
        let selected = text.get(start..end).unwrap_or("");
        let replacement = format!("```\n{selected}\n```");
        let selection = TextRange::new(
            ByteOffset::new(start + 4),
            ByteOffset::new(start + 4 + selected.len()),
        );
        EditPlan::new(
            Edit::new(
                TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
                replacement,
            ),
            Some(selection),
        )
    }

    /// 在 `at` 处插入一个 GFM 表格骨架（不含表体内容）。
    pub fn insert_table(at: ByteOffset, rows: usize, cols: usize) -> EditPlan {
        let rows = rows.clamp(1, 100);
        let cols = cols.clamp(1, 32);
        let mut header = String::from("|");
        let mut divider = String::from("|");
        for c in 0..cols {
            header.push_str(&format!(" 列 {} |", c + 1));
            divider.push_str(" --- |");
        }
        let mut out = header;
        out.push('\n');
        out.push_str(&divider);
        for _ in 0..rows {
            out.push('\n');
            out.push('|');
            for _ in 0..cols {
                out.push_str("  |");
            }
        }
        EditPlan::plain(Edit::insertion(at, out))
    }
}

/// 识别行内容的列表标记（含其后空格）；不是列表行时返回 `None`。
fn list_marker_of(content: &str) -> Option<&str> {
    for marker in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "] {
        if let Some(rest) = content.get(..marker.len())
            && rest.eq_ignore_ascii_case(marker)
        {
            return content.get(..marker.len());
        }
    }
    // 有序列表：`1. ` / `10) `
    let bytes = content.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == 0 || idx >= bytes.len() {
        return None;
    }
    // `1. ` 这类带尾随空格的标记长度为「数字 + 分隔符 + 空格」，故分隔符部分计 2。
    let delim_len = match bytes[idx] {
        b'.' | b')' if bytes.len() > idx + 1 && bytes[idx + 1] == b' ' => 2,
        b'.' | b')' => 1,
        _ => return None,
    };
    let end = idx + delim_len;
    content.get(..end).filter(|marker| !marker.is_empty())
}

/// 生成下一行的列表标记（有序列表递增，任务列表重置为未完成）。
fn next_list_marker(marker: &str) -> String {
    if marker.eq_ignore_ascii_case("- [x] ") {
        return "- [ ] ".to_string();
    }
    let trimmed = marker.trim_end_matches(['.', ')', ' ']);
    if let Ok(number) = trimmed.parse::<u64>() {
        let delim = marker.chars().nth(trimmed.len()).unwrap_or('.');
        return format!("{}{} ", number.saturating_add(1), delim);
    }
    marker.to_string()
}
