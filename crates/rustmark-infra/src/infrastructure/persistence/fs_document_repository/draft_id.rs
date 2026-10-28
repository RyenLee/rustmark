//! 草稿文件名 → `DocumentId` 逆向解析（从 `fs_document_repository` 抽出；R-07 Batch 2）。

use rustmark_core::shared::ids::DocumentId;

/// 由草稿文件名 `<DocumentId(N)>` 还原文档 ID。
///
/// 草稿文件名即 `DocumentId` 的 `Display` 输出（`DocumentId(N)`），
/// 此处逆向解析，使「列出遗留草稿」能与「按 ID 清除草稿」对齐。
pub(super) fn parse_draft_id(stem: &str) -> Option<DocumentId> {
    let inner = stem.strip_prefix("DocumentId(")?.strip_suffix(")")?;
    inner.parse::<u64>().ok().map(DocumentId::from_u64)
}
