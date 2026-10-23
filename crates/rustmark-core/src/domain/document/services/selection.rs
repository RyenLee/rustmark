//! [`EditingService`] 的选区包裹能力：标记符包裹 / 解除包裹。

use crate::shared::geometry::{ByteOffset, TextRange};

use super::super::value_objects::{Edit, EditPlan};
use super::EditingService;

impl EditingService {
    /// 用 `prefix` / `suffix` 包裹选区；若已被包裹则**解除**包裹。
    ///
    /// - 空选区：插入 `prefix + suffix`，光标落在两者之间；
    /// - 非空选区：包裹选中文本，返回的选区仍覆盖原文本（不含标记）；
    /// - 已包裹：删除两侧标记，返回的选区覆盖内部文本。
    ///
    /// 越界区间会先被裁剪到文本范围内（不 panic）。
    pub fn wrap_selection(text: &str, range: TextRange, prefix: &str, suffix: &str) -> EditPlan {
        let range = Self::align_range(text, range);
        let start = range.start().value().min(text.len());
        let end = range.end().value().min(text.len()).max(start);
        let selected = text.get(start..end).unwrap_or("");

        let inner_start = start.saturating_sub(prefix.len());
        let already_wrapped = start >= prefix.len()
            && end + suffix.len() <= text.len()
            && text.get(inner_start..start) == Some(prefix)
            && text.get(end..end + suffix.len()) == Some(suffix);

        if already_wrapped {
            let edit = Edit::new(
                TextRange::new(
                    ByteOffset::new(inner_start),
                    ByteOffset::new(end + suffix.len()),
                ),
                selected,
            );
            let selection = TextRange::new(
                ByteOffset::new(inner_start),
                ByteOffset::new(inner_start + selected.len()),
            );
            return EditPlan::new(edit, Some(selection));
        }

        let mut replacement = String::with_capacity(prefix.len() + selected.len() + suffix.len());
        replacement.push_str(prefix);
        replacement.push_str(selected);
        replacement.push_str(suffix);
        let edit = Edit::new(
            TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
            replacement,
        );
        let selection = TextRange::new(
            ByteOffset::new(start + prefix.len()),
            ByteOffset::new(start + prefix.len() + selected.len()),
        );
        EditPlan::new(edit, Some(selection))
    }
}
