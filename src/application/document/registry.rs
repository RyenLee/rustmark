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

    /// 只取「版本号 + 字节长度」，**不做全文快照**（问题 046）。
    ///
    /// # 为什么必须有这个方法（而不是继续用 `content_and_revision`）
    ///
    /// 编辑区每帧都要问这两个数，判断的全是**廉价决策**：
    /// 「是不是超过大文件阈值」（只看长度）、「文档静止够久了吗」（只看版本号）、
    /// 「缓存命不命中」（键里只有版本号，不含文本）。而 `content_and_revision`
    /// 为了回答它们要把整篇文本从 rope 快照成一个新 `String`——
    /// 1MB 文档就是每帧一次 MB 级分配 + 拷贝，纯属为两个整数付 O(全文)。
    ///
    /// 与问题 043（预览区只取版本号）同一个根因，只是当时的修法只覆盖了预览。
    ///
    /// # 默认实现
    ///
    /// 退回 [`Self::content_and_revision`] 再取长度：语义等价但**是慢路径**。
    /// 生产实现（[`DocumentRegistry`]）必须覆写为 O(1)。
    fn revision_and_len(&self, id: DocumentId) -> Option<(u64, usize)> {
        self.content_and_revision(id)
            .map(|(text, revision)| (revision, text.len()))
    }

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

    /// 只取文档版本号，**不做全文快照**（问题 043）。
    ///
    /// 预览区每帧要拿版本号构建缓存键，此前走 `content_and_revision`——
    /// 为了一个 `u64` 把整篇文本从 rope 快照出来。版本号本身是聚合上的
    /// 计数器，读取 O(1)。
    pub fn revision_of(&self, id: DocumentId) -> Option<u64> {
        self.docs.get(&id).map(|doc| doc.revision())
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

    /// O(1)：版本号是聚合上的计数器，字节长度由 rope 直接给出，都不触发快照。
    ///
    /// 覆写默认实现的原因见 trait 上的文档（每帧路径不得快照全文）。
    fn revision_and_len(&self, id: DocumentId) -> Option<(u64, usize)> {
        self.docs
            .get(&id)
            .map(|doc| (doc.revision(), doc.len_bytes()))
    }

    fn path_of(&self, id: DocumentId) -> Option<FilePath> {
        self.docs.get(&id).and_then(|doc| doc.path().cloned())
    }

    fn open_documents(&self) -> Vec<(DocumentId, Option<FilePath>, String)> {
        // 按**插入顺序**（= 标签栏顺序）而不是 `HashMap` 迭代序：
        // 搜索结果的分组顺序直接由这里决定，随机序会让「同一个查询
        // 两次搜索结果分组顺序不同」，也让学生以为分组逻辑坏了。
        self.ids()
            .into_iter()
            .filter_map(|id| {
                self.docs
                    .get(&id)
                    .map(|doc| (doc.id(), doc.path().cloned(), doc.text()))
            })
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

    /// O(1)：只借注册表的读锁取两个整数，不做快照（理由见 trait 文档）。
    fn revision_and_len(&self, id: DocumentId) -> Option<(u64, usize)> {
        let guard = self.registry.read().unwrap_or_else(|poisoned| {
            log::warn!(
                "RegistryProvider::revision_and_len: registry RwLock poisoned; recovering guard"
            );
            poisoned.into_inner()
        });
        guard.revision_and_len(id)
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
