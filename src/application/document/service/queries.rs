//! 查询用例：文档视图 / 内容 / 统计 / 当前选区 / 已打开文档列表。

use std::sync::atomic::Ordering;

use crate::shared::error::AppError;
use crate::shared::result::AppResult;

use crate::application::dto::{DocumentView, RangeView, StatsView, TabSummary};

use super::super::queries::*;
use super::DocumentAppService;
use super::mapping::{to_document_view, to_tab_summary};
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

    /// 查询单个文档的**轻量摘要**（不做全文快照，见 [`TabSummary`]）。
    ///
    /// 状态栏每帧查脏标记 / 类型 / 路径、面包屑每帧查路径都走这里，
    /// 不再为拿一个布尔值付整篇文本的克隆。
    pub fn get_tab_summary(&self, cmd: GetDocumentQuery) -> AppResult<TabSummary> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        Ok(to_tab_summary(doc))
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
    ///
    /// 结果按 `(文档 ID, 版本)` 缓存（见 `DocumentAppService::stats_cache`）：
    /// 状态栏每帧查两次，文本未变的帧直接命中，不再全文克隆 + 全文统计。
    pub fn get_stats(&self, cmd: GetStatsQuery) -> AppResult<StatsView> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        let revision = doc.revision();
        {
            let cache = self.stats_cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((id, rev, stats)) = cache.as_ref()
                && *id == cmd.id
                && *rev == revision
            {
                self.stats_cache_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(stats.clone());
            }
        }
        let stats = compute_stats(doc.text().as_str(), doc.meta().title());
        *self.stats_cache.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((cmd.id, revision, stats.clone()));
        Ok(stats)
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

    /// 列出全部已打开文档的**轻量摘要**（标签条用，见 [`TabSummary`]）。
    ///
    /// [`Self::list_open`] 会为每个文档做一次全文快照；标签条每帧都要
    /// 这份列表，只需要标题 / 脏标记 / 路径——多文档场景下那是每帧
    /// N 次 O(全文) 的克隆（问题 043 卡顿主因），一律走这里。
    pub fn list_open_tabs(&self, _cmd: ListOpenDocumentsQuery) -> AppResult<Vec<TabSummary>> {
        let reg = self.lock_read()?;
        Ok(reg
            .ids()
            .iter()
            .filter_map(|id| reg.get(*id).map(to_tab_summary))
            .collect())
    }
}
