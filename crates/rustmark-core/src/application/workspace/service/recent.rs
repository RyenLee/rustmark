//! 最近文件用例：启动恢复、记录打开、失效清理与列举。
//!
//! 去重与上限（20）由领域层 [`crate::domain::workspace::value_objects::RecentFiles`] 保证，
//! 这里只做编排：聚合变更 → 持久化 → 发布事件。

use crate::shared::path::FilePath;
use crate::shared::result::AppResult;

use crate::application::dto::RecentFileView;

use super::super::commands::*;
use super::super::queries::*;
use super::WorkspaceAppService;

impl WorkspaceAppService {
    /// 启动时从持久化恢复最近文件列表。
    pub fn restore_recent(&self) -> AppResult<()> {
        let items = self.repository.load_recent()?;
        self.lock_write()?.restore_recent(items);
        Ok(())
    }

    /// 记录一次文件打开（更新最近文件并持久化）。
    pub fn touch_recent(&self, cmd: TouchRecentCommand) -> AppResult<()> {
        let now = self.clock.now();
        let items = {
            let mut ws = self.lock_write()?;
            ws.touch_recent(cmd.path.clone(), now);
            ws.recent().list().to_vec()
        };
        self.repository.save_recent(&items)?;
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(())
    }

    /// 从最近文件中移除并持久化。
    pub fn forget_recent(&self, cmd: ForgetRecentCommand) -> AppResult<()> {
        let items = {
            let mut ws = self.lock_write()?;
            ws.forget_recent(&cmd.path);
            ws.recent().list().to_vec()
        };
        self.repository.save_recent(&items)?;
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(())
    }

    /// 列出最近文件（按最近使用降序）。
    pub fn list_recent(&self, _cmd: ListRecentQuery) -> AppResult<Vec<RecentFileView>> {
        let ws = self.lock_read()?;
        Ok(ws
            .recent()
            .list()
            .iter()
            .map(|item| RecentFileView {
                path: item.path().clone(),
                last_opened: item.last_opened().millis(),
            })
            .collect())
    }

    /// 移除一条失效的最近文件记录（失败静默）。
    ///
    /// 这只是清理动作：条目已经不在磁盘上，把它留在最近文件里只会让
    /// 用户点到一个打不开的路径。清理失败（配置目录只读）不值得打断操作。
    pub(super) fn forget_recent_quietly(&self, path: &FilePath) {
        let _ = self.forget_recent(ForgetRecentCommand { path: path.clone() });
    }
}
