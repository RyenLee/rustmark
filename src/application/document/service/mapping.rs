//! 领域聚合 / EditPlan → DTO 的投影，编辑施加与选区折算，另存为路径派生。

use crate::domain::document::aggregate::Document;
use crate::shared::geometry::{ByteOffset, TextRange};
use crate::shared::path::FilePath;

use crate::application::dto::{DocumentView, RangeView, TabSummary};

/// 把一次编辑施加到文本上，返回新文本。
///
/// 切片全部走 `get().unwrap_or("")`：偏移若落在非字符边界（例如调用方
/// 用字符数当字节数传进来），`&text[a..b]` 会 panic，
/// 而本项目 `panic = "abort"` 下等于终止进程——宁可少替换一点也不能崩。
pub(super) fn apply_edit(text: &str, range: TextRange, replacement: &str) -> String {
    let start = range.start().value().min(text.len());
    let end = range.end().value().min(text.len()).max(start);
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(text.get(..start).unwrap_or(""));
    out.push_str(replacement);
    out.push_str(text.get(end..).unwrap_or(""));
    out
}

/// 把领域给出的选区（**新文本**坐标系）折算成可返回的区间。
///
/// `EditPlan::selection` 已经是「编辑完成后在新内容里」的位置，
/// 所以这里**不需要**平移——只需夹紧到新文本长度内。
/// `None` 表示领域未指定选区，此时把光标放在**替换文本末尾**，
/// 与 `TextEdit` 插入后的默认行为一致，用户可立刻继续输入。
pub(super) fn shift_selection(
    _old: &str,
    new_text: &str,
    selection: Option<TextRange>,
    edit_range: TextRange,
) -> RangeView {
    let len = new_text.len();
    let sel = selection.unwrap_or_else(|| {
        let at = edit_range.start().value().saturating_add(
            edit_range
                .end()
                .value()
                .saturating_sub(edit_range.start().value()),
        );
        TextRange::new(ByteOffset::new(at), ByteOffset::new(at))
    });
    let start = sel.start().value().min(len);
    let end = sel.end().value().min(len).max(start);
    RangeView::new(start, end)
}

pub fn to_document_view(doc: &Document) -> DocumentView {
    let text = doc.text();
    let preview: String = text.chars().take(120).collect();
    DocumentView {
        id: doc.id(),
        title: doc.meta().title().to_string(),
        path: doc.path().cloned(),
        content_preview: preview,
        is_dirty: doc.is_dirty(),
        cursor: doc.cursor().position().value(),
        selection: doc
            .cursor()
            .selection()
            .map(|r| RangeView::new(r.start().value(), r.end().value())),
        line_count: crate::shared::text::line_count(&text),
        byte_len: doc.len_bytes(),
    }
}

/// 轻量摘要投影：**不做全文快照**（见 [`TabSummary`] 的文档）。
pub fn to_tab_summary(doc: &Document) -> TabSummary {
    TabSummary {
        id: doc.id(),
        title: doc.meta().title().to_string(),
        path: doc.path().cloned(),
        is_dirty: doc.is_dirty(),
        byte_len: doc.len_bytes(),
    }
}

/// 由已有路径派生「另存为」路径（同目录、文件名插入 `-copy`）。
pub(super) fn derive_save_as_path(original: &FilePath) -> FilePath {
    let name = original.file_name();
    let new_name = match name.rfind('.') {
        Some(idx) => format!("{}-copy{}", &name[..idx], &name[idx..]),
        None => format!("{}-copy", name),
    };
    match original.parent() {
        Some(parent) => parent.join(&new_name).unwrap_or_else(|_| original.clone()),
        None => original.clone(),
    }
}
