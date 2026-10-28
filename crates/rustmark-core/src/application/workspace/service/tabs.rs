//! 标签页用例：开关与列举（脏文档关闭需先保存）。

use crate::domain::workspace::errors::WorkspaceError;
use crate::domain::workspace::value_objects::CloseDecision;
use crate::shared::error::AppError;
use crate::shared::result::AppResult;

use crate::application::dto::{CloseDecisionView, TabView};

use super::super::commands::*;
use super::super::queries::*;
use super::WorkspaceAppService;

impl WorkspaceAppService {
    /// 打开（或激活）一个标签页。
    pub fn open_tab(&self, cmd: OpenTabCommand) -> AppResult<()> {
        self.lock_write()?.open_tab(cmd.doc_id);
        self.bus
            .publish(crate::ports::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(())
    }

    /// 关闭标签页；脏文档返回 [`CloseDecisionView::NeedsSave`]，保持打开。
    pub fn close_tab(&self, cmd: CloseTabCommand) -> AppResult<CloseDecisionView> {
        let decision = self
            .lock_write()?
            .close_tab(cmd.doc_id, cmd.dirty)
            .map_err(|e: WorkspaceError| AppError::domain(e.to_string()))?;
        self.bus
            .publish(crate::ports::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(match decision {
            CloseDecision::Closed => CloseDecisionView::Closed,
            CloseDecision::NeedsSave => CloseDecisionView::NeedsSave,
        })
    }

    /// 强制关闭标签页（不保存）。
    pub fn force_close_tab(&self, cmd: ForceCloseTabCommand) -> AppResult<()> {
        self.lock_write()?.force_close_tab(cmd.doc_id);
        self.bus
            .publish(crate::ports::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(())
    }

    /// 列出全部标签页。
    pub fn list_tabs(&self, _cmd: ListTabsQuery) -> AppResult<Vec<TabView>> {
        let ws = self.lock_read()?;
        Ok(ws
            .tabs()
            .iter()
            .map(|tab| TabView {
                document_id: tab.document_id(),
            })
            .collect())
    }
}
