//! 服务内部原语：聚合读写锁、副本验规则、锁外事件发布。

use crate::domain::workspace::aggregate::Workspace;
use crate::domain::workspace::errors::WorkspaceError;
use crate::shared::error::AppError;
use crate::shared::result::AppResult;

use super::WorkspaceAppService;

impl WorkspaceAppService {
    /// 在聚合的**副本**上试跑一条规则；不通过则直接返回错误，真聚合不动。
    pub(super) fn check_on_clone(
        &self,
        rule: impl FnOnce(&mut Workspace) -> Result<(), WorkspaceError>,
    ) -> AppResult<()> {
        let mut probe = self.lock_read()?.clone();
        rule(&mut probe).map_err(|e: WorkspaceError| AppError::domain(e.to_string()))
    }

    /// 发布工作区变更事件。
    pub(super) fn publish_changed(&self) {
        self.bus
            .publish(crate::ports::event::DomainEvent::WorkspaceChanged(self.id));
    }

    pub(super) fn lock_read(&self) -> AppResult<std::sync::RwLockReadGuard<'_, Workspace>> {
        self.workspace
            .read()
            .map_err(|_| AppError::io("工作区读锁已损坏"))
    }

    pub(super) fn lock_write(&self) -> AppResult<std::sync::RwLockWriteGuard<'_, Workspace>> {
        self.workspace
            .write()
            .map_err(|_| AppError::io("工作区写锁已损坏"))
    }
}
