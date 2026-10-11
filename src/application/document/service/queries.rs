//! 查询用例：文档视图 / 内容 / 统计 / 当前选区 / 已打开文档列表。

use crate::shared::error::AppError;
use crate::shared::result::AppResult;

use crate::application::dto::{DocumentView, RangeView, StatsView};

use super::super::queries::*;
use super::DocumentAppService;
use super::mapping::to_document_view;
use super::stats::compute_stats;

impl DocumentAppService {
    /// 查询单个文档视图。
    pub fn get_document(&self, cmd: GetDocumentQuery) -> AppResult<DocumentView> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        Ok(to_document_view(doc))
    }

    /// 查询文档内容；`range = None` 取全文。
    pub fn get_content(&self, cmd: GetContentQuery) -> AppResult<String> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        match cmd.range {
            None => Ok(doc.text()),
            Some(range) => Ok(doc.slice(range)?),
        }
    }

    /// 查询字数统计。
    pub fn get_stats(&self, cmd: GetStatsQuery) -> AppResult<StatsView> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        Ok(compute_stats(doc.text().as_str(), doc.meta().title()))
    }

    /// 查询当前选区（无选区返回 `None`）。
    pub fn get_selection(&self, cmd: GetSelectionQuery) -> AppResult<Option<RangeView>> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        Ok(doc
            .cursor()
            .selection()
            .map(|r| RangeView::new(r.start().value(), r.end().value())))
    }

    /// 列出全部已打开文档。
    pub fn list_open(&self, _cmd: ListOpenDocumentsQuery) -> AppResult<Vec<DocumentView>> {
        let reg = self.lock_read()?;
        Ok(reg
            .ids()
            .iter()
            .filter_map(|id| reg.get(*id).map(to_document_view))
            .collect())
    }
}
