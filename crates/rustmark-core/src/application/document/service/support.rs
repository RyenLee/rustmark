//! 服务内部并发与事件原语：注册表读写锁、聚合事件抽取、锁外事件发布。

use crate::domain::document::aggregate::Document;
use crate::shared::error::AppError;
use crate::shared::event::DomainEvent;
use crate::shared::result::AppResult;

use super::super::registry::DocumentRegistry;
use super::DocumentAppService;

impl DocumentAppService {
    pub(super) fn lock_read(&self) -> AppResult<std::sync::RwLockReadGuard<'_, DocumentRegistry>> {
        self.registry
            .read()
            .map_err(|_| AppError::io("文档注册表读锁已损坏"))
    }

    pub(super) fn lock_write(
        &self,
    ) -> AppResult<std::sync::RwLockWriteGuard<'_, DocumentRegistry>> {
        self.registry
            .write()
            .map_err(|_| AppError::io("文档注册表写锁已损坏"))
    }

    /// 把聚合事件队列转换为总线事件（`take_events` 必须在锁内调用）。
    pub(super) fn drain_doc_events(doc: &mut Document) -> Vec<DomainEvent> {
        doc.take_events().into_iter().map(Into::into).collect()
    }

    /// 在**不持有注册表锁**的前提下发布事件。
    ///
    /// 事件发布是同步调用订阅者回调，而订阅者（如 `EventSubscribers` 的
    /// `DocumentSaved` 处理）会通过 [`DocumentContentProvider`] 反向读取同一份注册表。
    /// 若持锁发布，同一线程重入 `RwLock` 将直接死锁——故所有事件必须在锁释放后才发出。
    ///
    /// [`DocumentContentProvider`]: super::super::DocumentContentProvider
    pub(super) fn publish_all(&self, events: Vec<DomainEvent>) {
        for event in events {
            self.bus.publish(event);
        }
    }
}
