//! 领域事件与事件总线抽象（Shared Kernel）。
//!
//! 设计约束：
//! - 事件**只携带 ID 与标量**，不携带聚合引用，避免 `shared` 反向依赖 `domain`；
//! - 订阅者返回 `Result`，总线**收集错误而不中断**派发（S5-07）；
//! - 事件是**通知**而非命令：订阅者只更新派生状态，不得反向修改触发事件的聚合。

use std::sync::Arc;

use crate::shared::error::AppError;
use crate::shared::geometry::TextRange;
use crate::shared::ids::{DocumentId, JobId, WorkspaceId};

/// 领域事件。
///
/// 变体与各上下文的职责对应（需求文档 §6.1）。载荷保持最小化，
/// 订阅方如需详情，应凭 ID 向应用层查询。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DomainEvent {
    /// 文档被打开或新建。
    DocumentOpened(DocumentId),
    /// 文档正文被修改（携带修改次数，供订阅方判断是否需要刷新）。
    TextModified {
        document_id: DocumentId,
        revision: u64,
    },
    /// 光标或选区发生变化（仅通知，订阅方不应据此重解析全文）。
    SelectionChanged {
        document_id: DocumentId,
        selection: Option<TextRange>,
    },
    /// 文档已成功保存到磁盘。
    DocumentSaved(DocumentId),
    /// 文档被关闭。
    DocumentClosed(DocumentId),
    /// 工作区结构（文件树 / 最近文件 / 标签页）发生变化。
    WorkspaceChanged(WorkspaceId),
    /// 偏好设置（主题 / 编辑器 / 快捷键）发生变化，订阅方应失效相关缓存。
    PreferencesChanged,
    /// 导出任务结束。
    ExportFinished(JobId),
}

impl DomainEvent {
    /// 事件涉及的文档 ID（若该事件与文档无关则为 `None`）。
    pub fn document_id(self) -> Option<DocumentId> {
        match self {
            Self::DocumentOpened(id)
            | Self::TextModified {
                document_id: id, ..
            }
            | Self::SelectionChanged {
                document_id: id, ..
            }
            | Self::DocumentSaved(id)
            | Self::DocumentClosed(id) => Some(id),
            _ => None,
        }
    }
}

/// 事件订阅者：接收事件引用，返回是否处理成功。
///
/// 使用 `Arc<dyn Fn>` 而非泛型，便于在容器中存放异构订阅者。
pub type EventHandler = Arc<dyn Fn(&DomainEvent) -> Result<(), AppError> + Send + Sync>;

/// 事件总线抽象（由 `infrastructure::events` 实现，S5-07）。
///
/// 实现必须 `Send + Sync`；`publish` 同步派发，返回订阅者产生的错误列表，
/// **单个订阅者失败不得影响其他订阅者**。
pub trait EventBus: Send + Sync {
    /// 发布事件，按订阅顺序同步派发，返回订阅者错误（按派发顺序）。
    fn publish(&self, event: DomainEvent) -> Vec<AppError>;

    /// 注册订阅者。
    ///
    /// 采用 `&self` 而非 `&mut self`：实现方一律使用内部可变性（`Mutex`）
    /// 承载订阅者列表，因此组合根可以在把总线包成 `Arc<dyn EventBus>` 共享后，
    /// 仍能在任意时刻注册订阅者（否则 `&mut` 会强制「先注册、后共享」，
    /// 与「订阅者又依赖其它服务」的装配顺序冲突）。
    fn subscribe(&self, handler: EventHandler);
}
