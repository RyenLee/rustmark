//! Workspace 上下文的值对象（S3-11 配套）。
//!
//! 路径一律使用 [`crate::shared::path::FilePath`]，不使用 `std::path::PathBuf`
//! （`.agent/conventions.md` 第 12 条）。

use serde::{Deserialize, Serialize};

use crate::ports::time::Timestamp;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

/// 文件树条目的类型。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum EntryKind {
    /// 普通文件。
    File,
    /// 目录。
    Directory,
}

impl EntryKind {
    /// 是否为目录。
    pub(crate) const fn is_directory(&self) -> bool {
        matches!(self, Self::Directory)
    }
}

/// 文件树中的一个条目。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    path: FilePath,
    kind: EntryKind,
}

impl WorkspaceEntry {
    /// 构造条目：路径 + 类型。
    pub fn new(path: FilePath, kind: EntryKind) -> Self {
        Self { path, kind }
    }

    /// 构造文件条目。
    pub fn file(path: FilePath) -> Self {
        Self::new(path, EntryKind::File)
    }

    /// 构造目录条目。
    pub fn directory(path: FilePath) -> Self {
        Self::new(path, EntryKind::Directory)
    }

    /// 条目路径。
    pub const fn path(&self) -> &FilePath {
        &self.path
    }

    /// 条目类型。
    pub const fn kind(&self) -> EntryKind {
        self.kind
    }
}

/// 最近打开文件列表。
///
/// 规则：按 `last_opened` **降序**（最近使用的在前）、同路径去重、条数上限 20
/// （需求文档 §3.1「最近文件」P1，上限取常见编辑器惯例）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RecentFiles {
    items: Vec<RecentFile>,
    limit: usize,
}

/// 默认条数上限。
pub const RECENT_FILES_LIMIT: usize = 20;

impl RecentFiles {
    /// 用默认上限（20）构造空列表。
    pub fn new() -> Self {
        Self::with_limit(RECENT_FILES_LIMIT)
    }

    /// 用指定上限构造空列表；`limit` 为 0 时按 1 处理。
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            items: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// 由已持久化的记录重建列表（保留原始时间戳，不做时间重置）。
    ///
    /// 用于启动时从配置目录恢复「最近文件」；重建后会重新排序并裁剪到上限。
    pub(crate) fn with_items(items: Vec<RecentFile>, limit: usize) -> Self {
        let mut recent = Self {
            items,
            limit: limit.max(1),
        };
        recent.normalize();
        recent
    }

    /// 记录一次打开：已存在则**更新时间并移到最前**，否则插入最前。
    pub(crate) fn touch(&mut self, path: FilePath, at: Timestamp) {
        if let Some(existing) = self.items.iter_mut().find(|item| item.path == path) {
            existing.last_opened = at;
        } else {
            self.items.push(RecentFile::new(path, at));
        }
        self.normalize();
    }

    /// 移除某路径（文件被删除或用户手动清除）。
    pub(crate) fn remove(&mut self, path: &FilePath) {
        self.items.retain(|item| &item.path != path);
    }

    /// 按最近使用降序返回全部条目。
    pub fn list(&self) -> &[RecentFile] {
        &self.items
    }

    /// 当前条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 排序（最近在前）并裁剪到上限。
    fn normalize(&mut self) {
        self.items
            .sort_by_key(|item| std::cmp::Reverse(item.last_opened()));
        if self.items.len() > self.limit {
            self.items.truncate(self.limit);
        }
    }
}

impl Default for RecentFiles {
    fn default() -> Self {
        Self::new()
    }
}

/// 一条最近打开记录。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RecentFile {
    path: FilePath,
    last_opened: Timestamp,
}

impl RecentFile {
    /// 构造记录。
    pub fn new(path: FilePath, last_opened: Timestamp) -> Self {
        Self { path, last_opened }
    }

    /// 记录对应的文件路径。
    pub const fn path(&self) -> &FilePath {
        &self.path
    }

    /// 最近打开时间。
    pub(crate) const fn last_opened(&self) -> Timestamp {
        self.last_opened
    }
}

/// 关闭文档时的决策。
///
/// 领域层**不做 IO**：只给出决策，由应用层弹出保存对话框并执行保存。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseDecision {
    /// 文档已干净，可以直接关闭（标签页已从工作区移除）。
    Closed,
    /// 文档有未保存修改，需要先询问用户保存；标签页**保持打开**。
    NeedsSave,
}

impl CloseDecision {
    /// 是否允许关闭。
    pub const fn can_close(&self) -> bool {
        matches!(self, Self::Closed)
    }
}

/// 标签页（对文档的引用，不持有文档本体）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Tab {
    document_id: DocumentId,
}

impl Tab {
    /// 构造标签页。
    pub(crate) const fn new(document_id: DocumentId) -> Self {
        Self { document_id }
    }

    /// 文档 ID。
    pub(crate) const fn document_id(&self) -> DocumentId {
        self.document_id
    }
}
