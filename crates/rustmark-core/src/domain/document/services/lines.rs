//! [`EditingService`] 的行级编辑能力：
//! 前缀切换、缩进、整行移动 / 复制 / 删除，以及内部逐行变换原语。

use crate::shared::geometry::{ByteOffset, TextRange};

use super::super::value_objects::{Edit, EditPlan};
use super::EditingService;

impl EditingService {
    /// 对区间覆盖的每一行**切换**行首前缀（有则删、无则加）。
    ///
    /// 典型用法：`> `（引用）、`# ` / `## `（标题）、`- `（列表）。
    pub fn toggle_line_prefix(text: &str, range: TextRange, prefix: &str) -> EditPlan {
        Self::transform_lines(text, range, |line| {
            let stripped = line.strip_prefix(prefix);
            match stripped {
                Some(rest) => rest.to_string(),
                None => format!("{prefix}{line}"),
            }
        })
    }

    /// 对区间覆盖的每一行增加一级缩进。
    ///
    /// `indent_unit` 由调用方从偏好设置的 `tabSize` 推导（`"  "` / `"    "` /
    /// `"        "` / `"\t"`），**不在此处硬编码**，以根治原型缺陷 2.2。
    pub fn indent_lines(text: &str, range: TextRange, indent_unit: &str) -> EditPlan {
        Self::transform_lines(text, range, |line| {
            // 空行不产生尾随空白（验收标准明确要求）。
            if line.is_empty() {
                return String::new();
            }
            format!("{indent_unit}{line}")
        })
    }

    /// 对区间覆盖的每一行减少一级缩进。
    ///
    /// 删除规则：优先删除**恰好一个** `indent_unit`；若行首是制表符则删除一个 `\t`；
    /// 否则删除行首至多 `indent_unit.len()` 个空格。
    pub fn outdent_lines(text: &str, range: TextRange, indent_unit: &str) -> EditPlan {
        Self::transform_lines(text, range, |line| {
            if let Some(rest) = line.strip_prefix(indent_unit) {
                return rest.to_string();
            }
            let mut idx = 0;
            let mut spaces_removed = 0;
            while idx < line.len() && spaces_removed < indent_unit.len().max(1) {
                match line[idx..].chars().next() {
                    // 行首是制表符：无论 `indent_unit` 多宽，都只吃掉一个 `\t`。
                    Some('\t') => {
                        idx += 1;
                        break;
                    }
                    Some(' ') => {
                        idx += 1;
                        spaces_removed += 1;
                    }
                    _ => break,
                }
            }
            line[idx..].to_string()
        })
    }

    /// 把区间覆盖的整块行向上移动一行；已在首行时返回 `None`。
    pub fn move_line_up(text: &str, range: TextRange) -> Option<EditPlan> {
        let block = Self::expand_to_full_lines(text, range);
        if block.start().value() == 0 {
            return None;
        }
        let prev_start = Self::line_start(text, block.start().value().saturating_sub(1));
        let region = TextRange::new(ByteOffset::new(prev_start), block.end());
        let region_text = text.get(region.as_usize_range())?;
        let mut lines: Vec<&str> = region_text.split('\n').collect();
        if lines.len() < 2 {
            return None;
        }
        let block_count = block_line_count(text, &block);
        if block_count >= lines.len() {
            return None;
        }
        // 原顺序：[上一行] [块…] [其余] → [块…] [上一行] [其余]
        let mut reordered: Vec<&str> = Vec::with_capacity(lines.len());
        reordered.extend(lines.iter().take(1 + block_count).skip(1).copied());
        reordered.push(lines[0]);
        reordered.extend(lines.iter().skip(1 + block_count).copied());
        lines = reordered;

        let replacement = lines.join("\n");
        let prev_len = lines[block_count].len() + 1;
        let new_start = block.start().value().saturating_sub(prev_len);
        let selection = TextRange::new(
            ByteOffset::new(new_start),
            ByteOffset::new(new_start + block.len()),
        );
        Some(EditPlan::new(
            Edit::new(region, replacement),
            Some(selection),
        ))
    }

    /// 把区间覆盖的整块行向下移动一行；已在末行时返回 `None`。
    pub fn move_line_down(text: &str, range: TextRange) -> Option<EditPlan> {
        let block = Self::expand_to_full_lines(text, range);
        let next_line_start = block.end().value() + 1; // 跳过本块的 '\n'
        if next_line_start > text.len() {
            return None;
        }
        let next_line_end = Self::line_end(text, next_line_start.min(text.len()));
        let region = TextRange::new(block.start(), ByteOffset::new(next_line_end));
        let region_text = text.get(region.as_usize_range())?;
        let mut lines: Vec<&str> = region_text.split('\n').collect();
        let block_count = block_line_count(text, &block);
        if block_count + 1 > lines.len() {
            return None;
        }
        // 原顺序：[块…] [下一行] → [下一行] [块…]
        let moved = lines[block_count];
        let mut reordered: Vec<&str> = Vec::with_capacity(lines.len());
        reordered.push(moved);
        reordered.extend(lines.iter().take(block_count).copied());
        reordered.extend(lines.iter().skip(block_count + 1).copied());
        lines = reordered;

        let replacement = lines.join("\n");
        let new_start = block.start().value() + moved.len() + 1;
        let selection = TextRange::new(
            ByteOffset::new(new_start),
            ByteOffset::new(new_start + block.len()),
        );
        Some(EditPlan::new(
            Edit::new(region, replacement),
            Some(selection),
        ))
    }

    /// 在光标所在行下方复制该行（光标停留在新行同一列）。
    pub fn duplicate_line(text: &str, cursor: ByteOffset) -> EditPlan {
        let line = Self::line_range(text, cursor.value());
        let raw = text.get(line.as_usize_range()).unwrap_or("");
        let line_text = raw.strip_suffix('\r').unwrap_or(raw);
        let insert_at = line.end();
        let insertion = format!("\n{line_text}");
        EditPlan::plain(Edit::insertion(insert_at, insertion))
    }

    /// 删除光标所在的整行（含其换行符；末行则只删行内容）。
    pub fn delete_line(text: &str, cursor: ByteOffset) -> EditPlan {
        let line = Self::line_range(text, cursor.value());
        // 非末行：连同行尾换行符一起删除；
        // 末行：改为删除其前一个换行符（与上一行合并）；
        // 全文仅一行：删除该行内容。
        let (start, end) = if line.end().value() < text.len() {
            (line.start().value(), line.end().value() + 1)
        } else if line.start().value() > 0 {
            (line.start().value() - 1, line.end().value())
        } else {
            (line.start().value(), line.end().value())
        };
        EditPlan::plain(Edit::deletion(TextRange::new(
            ByteOffset::new(start),
            ByteOffset::new(end),
        )))
    }

    /// 逐行变换区间覆盖的所有行（内部处理 `CRLF`），返回替换编辑。
    ///
    /// `transform` 收到的是**已剥离行尾 `\r`** 的行内容。
    pub(super) fn transform_lines(
        text: &str,
        range: TextRange,
        transform: impl Fn(&str) -> String,
    ) -> EditPlan {
        let region = Self::expand_to_full_lines(text, range);
        let region_text = text.get(region.as_usize_range()).unwrap_or("");
        let mut out = String::with_capacity(region_text.len() + 16);
        for (idx, raw) in region_text.split('\n').enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            let (body, carriage) = match raw.strip_suffix('\r') {
                Some(b) => (b, true),
                None => (raw, false),
            };
            out.push_str(&transform(body));
            if carriage {
                out.push('\r');
            }
        }
        EditPlan::plain(Edit::new(region, out))
    }
}

/// 计算 `block`（已扩展到整行）覆盖的行数，至少为 1。
fn block_line_count(text: &str, block: &TextRange) -> usize {
    let slice = text.get(block.as_usize_range()).unwrap_or("");
    slice.split('\n').count().max(1)
}
