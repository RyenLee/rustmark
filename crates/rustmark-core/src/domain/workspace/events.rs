//! Workspace 上下文的领域事件。
//!
//! 全部收敛到 [`DomainEvent::WorkspaceChanged`]：订阅方只需知道「结构变了」，
//! 具体变化通过重新查询获得，避免事件载荷与聚合结构重复演进。

use crate::ports::event::DomainEvent;
use crate::shared::ids::{DocumentId, WorkspaceId};
use crate::shared::path::FilePath;

/// 工作区结构变化的具体原因（供日志与调试；订阅方按需重新查询）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WorkspaceEvent {
    /// 根目录被切换（或首次设置）。
    RootChanged {
        /// 工作区 ID。
        workspace_id: WorkspaceId,
        /// 新的根目录。
        root: FilePath,
    },
    /// 新增条目。
    EntryAdded {
        /// 工作区 ID。
        workspace_id: WorkspaceId,
        /// 条目路径。
        path: FilePath,
    },
    /// 移除条目（含其子条目）。
    EntryRemoved {
        /// 工作区 ID。
        workspace_id: WorkspaceId,
        /// 条目路径。
        path: FilePath,
    },
    /// 移动 / 重命名条目。
    EntryMoved {
        /// 工作区 ID。
        workspace_id: WorkspaceId,
        /// 原路径。
        from: FilePath,
        /// 新路径。
        to: FilePath,
    },
    /// 打开了一个标签页。
    TabOpened {
        /// 工作区 ID。
        workspace_id: WorkspaceId,
        /// 文档 ID。
        document_id: DocumentId,
    },
    /// 关闭了一个标签页。
    TabClosed {
        /// 工作区 ID。
        workspace_id: WorkspaceId,
        /// 文档 ID。
        document_id: DocumentId,
    },
    /// 最近文件列表发生变化。
    RecentFilesChanged {
        /// 工作区 ID。
        workspace_id: WorkspaceId,
    },
}

impl WorkspaceEvent {
    /// 事件涉及的工作区 ID。
    pub const fn workspace_id(&self) -> WorkspaceId {
        match self {
            Self::RootChanged { workspace_id, .. }
            | Self::EntryAdded { workspace_id, .. }
            | Self::EntryRemoved { workspace_id, .. }
            | Self::EntryMoved { workspace_id, .. }
            | Self::TabOpened { workspace_id, .. }
            | Self::TabClosed { workspace_id, .. }
            | Self::RecentFilesChanged { workspace_id } => *workspace_id,
        }
    }
}

impl From<WorkspaceEvent> for DomainEvent {
    fn from(e: WorkspaceEvent) -> Self {
        Self::WorkspaceChanged(e.workspace_id())
    }
}
