//! 打开文档的注册表与内容提供者（阶段 04 · S4-04 支撑）。
//!
//! 注册表是 `Document` 聚合的运行时容器。Markdown 预览与导出服务通过
//! [`DocumentContentProvider`] 只读访问文档内容，**不持有聚合根**。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::domain::document::aggregate::Document;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

/// 文档内容只读提供者（依赖倒置点）。
///
/// Markdown 预览与导出需要读取文档内容，但不应持有聚合根；
/// 由 [`DocumentRegistry`] 或测试用的 fake 实现本 trait。
pub trait DocumentContentProvider: Send + Sync {
    /// 返回文档内容与版本号；文档未打开时返回 `None`。
    fn content_and_revision(&self, id: DocumentId) -> Option<(String, u64)>;

    /// 返回文档保存路径；未保存过或文档未打开时返回 `None`。
    fn path_of(&self, id: DocumentId) -> Option<FilePath>;

    /// 返回全部打开文档的 `(id, path, content)`；未保存文档的 `path` 为 `None`。
    ///
    /// 由工作区跨文件搜索使用，以保证「未保存草稿」也参与检索（而非退回磁盘旧内容）。
    fn open_documents(&self) -> Vec<(DocumentId, Option<FilePath>, String)>;
}

/// 打开文档的注册表（持有 `Document` 聚合，按 ID 索引）。
///
/// 由于 UI 线程与后台预览线程会并发访问，注册表本身不含锁——
/// 由 [`crate::application::document::service::DocumentAppService`] 以
/// `Arc<RwLock<DocumentRegistry>>` 包裹果实现共享。
#[derive(Default)]
pub struct DocumentRegistry {
    docs: HashMap<DocumentId, Document>,
    /// 文档 ID 的**插入顺序**。
    ///
    /// 必须显式维护：`HashMap` 的迭代顺序是**不确定**的（每次进程启动、
    /// 每次 rehash 都可能不同），而 UI 的标签条按 `ids()` 的顺序显示，
    /// `Alt+2`（`GotoTab2`）也按这个下标取文档。用 `HashMap` 直接迭代的后果：
    ///
    /// - 标签条顺序每次启动都可能不同；
    /// - `GotoTab2` 可能切到「当前已打开的那一个」，表现为**快捷键没反应**；
    /// - 「下一个标签」在两个文档间来回跳。
    ///
    /// 这些都不报错，只是行为随机——比崩溃更难排查。改成 `Vec` 维护顺序后，
    /// 「顺序 = 打开顺序」成为结构性保证。
    order: Vec<DocumentId>,
}

impl DocumentRegistry {
    /// 新建空注册表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 插入（或替换）一个文档。
    ///
    /// 重复插入**不**改变顺序：那是「内容更新」而非「新开标签」，
    /// 标签位置理应保持不动（否则用户正在编辑的标签会突然跳到末尾）。
    pub fn insert(&mut self, document: Document) {
        let id = document.id();
        if !self.docs.contains_key(&id) {
            self.order.push(id);
        }
        self.docs.insert(id, document);
    }

    /// 取文档（只读）。
    pub fn get(&self, id: DocumentId) -> Option<&Document> {
        self.docs.get(&id)
    }

    /// 取文档（可变）。
    pub fn get_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        self.docs.get_mut(&id)
    }

    /// 移除文档，返回被移除的文档。
    pub fn remove(&mut self, id: DocumentId) -> Option<Document> {
        self.order.retain(|open| *open != id);
        self.docs.remove(&id)
    }

    /// 是否含有某文档。
    pub fn contains(&self, id: DocumentId) -> bool {
        self.docs.contains_key(&id)
    }

    /// 全部打开的文档 ID，**按打开顺序**。
    ///
    /// 顺序即 UI 标签条的显示顺序，也是 `Alt+N` 的下标依据。
    /// 过滤掉已不在 `docs` 里的 ID（理论上不会发生，`remove` 会同步维护，
    /// 但多一层防御比「信任内部不变量」便宜）。
    pub fn ids(&self) -> Vec<DocumentId> {
        self.order
            .iter()
            .copied()
            .filter(|id| self.docs.contains_key(id))
            .collect()
    }

    /// 打开文档数。
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

impl DocumentContentProvider for DocumentRegistry {
    fn content_and_revision(&self, id: DocumentId) -> Option<(String, u64)> {
        self.docs.get(&id).map(|doc| (doc.text(), doc.revision()))
    }

    fn path_of(&self, id: DocumentId) -> Option<FilePath> {
        self.docs.get(&id).and_then(|doc| doc.path().cloned())
    }

    fn open_documents(&self) -> Vec<(DocumentId, Option<FilePath>, String)> {
        self.docs
            .values()
            .map(|doc| (doc.id(), doc.path().cloned(), doc.text()))
            .collect()
    }
}

/// 把 `Arc<RwLock<DocumentRegistry>>` 适配为只读内容提供者，供 Markdown / 导出等
/// 服务注入（依赖倒置：它们只认 trait，不认注册表实现），实现跨服务共享而不暴露聚合。
#[derive(Clone)]
pub struct RegistryProvider {
    registry: Arc<RwLock<DocumentRegistry>>,
}

impl RegistryProvider {
    /// 由共享注册表构造提供者。
    pub fn new(registry: Arc<RwLock<DocumentRegistry>>) -> Self {
        Self { registry }
    }

    /// 取出底层的共享注册表（供 `DocumentAppService` 复用同一份状态）。
    pub fn registry(&self) -> Arc<RwLock<DocumentRegistry>> {
        self.registry.clone()
    }
}

impl DocumentContentProvider for RegistryProvider {
    fn content_and_revision(&self, id: DocumentId) -> Option<(String, u64)> {
        let guard = self.registry.read().unwrap_or_else(|poisoned| {
            log::warn!(
                "RegistryProvider::content_and_revision: registry RwLock poisoned; recovering guard"
            );
            poisoned.into_inner()
        });
        guard.content_and_revision(id)
    }

    fn path_of(&self, id: DocumentId) -> Option<FilePath> {
        let guard = self.registry.read().unwrap_or_else(|poisoned| {
            log::warn!("RegistryProvider::path_of: registry RwLock poisoned; recovering guard");
            poisoned.into_inner()
        });
        guard.path_of(id)
    }

    fn open_documents(&self) -> Vec<(DocumentId, Option<FilePath>, String)> {
        let guard = self.registry.read().unwrap_or_else(|poisoned| {
            log::warn!(
                "RegistryProvider::open_documents: registry RwLock poisoned; recovering guard"
            );
            poisoned.into_inner()
        });
        guard.open_documents()
    }
}
