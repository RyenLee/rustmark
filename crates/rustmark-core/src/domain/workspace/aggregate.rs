//! Workspace 聚合根（S3-11）。
//!
//! # 边界
//!
//! 聚合**不做磁盘遍历**：条目由基础设施层扫描后经 [`Workspace::replace_entries`]
//! 整体装入。聚合只负责结构与规则：
//! - 同名条目不得重复；
//! - 目录不得移动到自己的子目录；
//! - 最近文件去重、按最近使用排序、上限 20；
//! - 关闭脏文档返回 [`CloseDecision::NeedsSave`]，由应用层决定是否弹保存对话框。

use crate::ports::time::Timestamp;
use crate::shared::ids::{DocumentId, WorkspaceId};
use crate::shared::path::FilePath;

use super::errors::WorkspaceError;
use super::events::WorkspaceEvent;
use super::value_objects::{
    CloseDecision, RECENT_FILES_LIMIT, RecentFile, RecentFiles, Tab, WorkspaceEntry,
};

/// 工作区聚合根。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Workspace {
    id: WorkspaceId,
    root: Option<FilePath>,
    entries: Vec<WorkspaceEntry>,
    tabs: Vec<Tab>,
    recent: RecentFiles,
    events: Vec<WorkspaceEvent>,
}

impl Workspace {
    /// 新建一个尚未设置根目录的工作区。
    pub fn new(id: WorkspaceId) -> Self {
        Self {
            id,
            root: None,
            entries: Vec::new(),
            tabs: Vec::new(),
            recent: RecentFiles::new(),
            events: Vec::new(),
        }
    }

    /// 新建并设置根目录。
    pub fn with_root(id: WorkspaceId, root: FilePath) -> Self {
        let mut workspace = Self::new(id);
        workspace.root = Some(root);
        workspace
    }

    // ---------- 访问器 ----------

    /// 工作区 ID。
    pub const fn id(&self) -> WorkspaceId {
        self.id
    }

    /// 根目录；未设置（未打开文件夹）时为 `None`。
    pub const fn root(&self) -> Option<&FilePath> {
        self.root.as_ref()
    }

    /// 文件树条目。
    pub fn entries(&self) -> &[WorkspaceEntry] {
        &self.entries
    }

    /// 已打开的标签页（按打开顺序）。
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// 最近文件列表。
    pub const fn recent(&self) -> &RecentFiles {
        &self.recent
    }

    /// 取走并清空事件队列。
    pub fn take_events(&mut self) -> Vec<WorkspaceEvent> {
        std::mem::take(&mut self.events)
    }

    // ---------- 根目录 ----------

    /// 切换根目录；已有条目与标签页保留（切换视图不应丢失正在编辑的内容）。
    pub fn set_root(&mut self, root: FilePath) {
        self.root = Some(root.clone());
        self.emit(WorkspaceEvent::RootChanged {
            workspace_id: self.id,
            root,
        });
    }

    /// 整体替换文件树（由基础设施层扫描后调用）。
    pub fn replace_entries(&mut self, entries: Vec<WorkspaceEntry>) {
        self.entries = entries;
    }

    // ---------- 条目规则 ----------

    /// 新增条目；同名（同路径）已存在时拒绝。
    pub fn add_entry(&mut self, entry: WorkspaceEntry) -> Result<(), WorkspaceError> {
        self.ensure_inside_root(entry.path())?;
        if self.entries.iter().any(|e| e.path() == entry.path()) {
            return Err(WorkspaceError::DuplicateName {
                path: entry.path().clone(),
            });
        }
        let path = entry.path().clone();
        self.entries.push(entry);
        self.emit(WorkspaceEvent::EntryAdded {
            workspace_id: self.id,
            path,
        });
        Ok(())
    }

    /// 移除条目及其全部子条目；不存在时拒绝。
    pub fn remove_entry(&mut self, path: &FilePath) -> Result<(), WorkspaceError> {
        if !self.entries.iter().any(|e| e.path() == path) {
            return Err(WorkspaceError::NotFound { path: path.clone() });
        }
        self.entries
            .retain(|e| e.path() != path && !e.path().is_under(path));
        self.emit(WorkspaceEvent::EntryRemoved {
            workspace_id: self.id,
            path: path.clone(),
        });
        Ok(())
    }

    /// 移动或重命名条目。
    ///
    /// 拒绝规则：
    /// - 源条目不存在；
    /// - 目标是源自身的后代（会产生环）；
    /// - 目标路径已存在。
    pub fn move_entry(&mut self, from: &FilePath, to: &FilePath) -> Result<(), WorkspaceError> {
        if !self.entries.iter().any(|e| e.path() == from) {
            return Err(WorkspaceError::NotFound { path: from.clone() });
        }
        if to.is_under(from) || to == from {
            return Err(WorkspaceError::MoveIntoDescendant {
                from: from.clone(),
                to: to.clone(),
            });
        }
        if self.entries.iter().any(|e| e.path() == to) {
            return Err(WorkspaceError::DuplicateName { path: to.clone() });
        }

        let from_len = from.as_str().len();
        for entry in self.entries.iter_mut() {
            if entry.path() == from {
                *entry = WorkspaceEntry::new(to.clone(), entry.kind());
            } else if entry.path().is_under(from) {
                let suffix = entry.path().as_str().get(from_len..).unwrap_or("");
                let moved = FilePath::new(format!("{to}{suffix}")).map_err(|_| {
                    WorkspaceError::OutsideRoot {
                        path: to.clone(),
                        root: to.clone(),
                    }
                })?;
                *entry = WorkspaceEntry::new(moved, entry.kind());
            }
        }
        self.emit(WorkspaceEvent::EntryMoved {
            workspace_id: self.id,
            from: from.clone(),
            to: to.clone(),
        });
        Ok(())
    }

    /// 判断某路径是否为目录（未收录的目录也按扩展名推断不出来，故一律返回 `false`）。
    pub fn is_directory(&self, path: &FilePath) -> bool {
        self.entries
            .iter()
            .any(|e| e.path() == path && e.kind().is_directory())
    }

    // ---------- 标签页 ----------

    /// 打开（或激活）一个标签页；已存在时不重复添加，仅发出事件。
    pub fn open_tab(&mut self, document_id: DocumentId) {
        if !self.tabs.iter().any(|tab| tab.document_id() == document_id) {
            self.tabs.push(Tab::new(document_id));
        }
        self.emit(WorkspaceEvent::TabOpened {
            workspace_id: self.id,
            document_id,
        });
    }

    /// 尝试关闭标签页。
    ///
    /// - `dirty == true`：返回 [`CloseDecision::NeedsSave`]，标签页**保持打开**；
    /// - `dirty == false`：移除标签页并返回 [`CloseDecision::Closed`]；
    /// - 标签页不存在：返回 [`WorkspaceError::TabNotFound`]。
    pub fn close_tab(
        &mut self,
        document_id: DocumentId,
        dirty: bool,
    ) -> Result<CloseDecision, WorkspaceError> {
        if !self.tabs.iter().any(|tab| tab.document_id() == document_id) {
            return Err(WorkspaceError::TabNotFound(document_id));
        }
        if dirty {
            return Ok(CloseDecision::NeedsSave);
        }
        self.tabs.retain(|tab| tab.document_id() != document_id);
        self.emit(WorkspaceEvent::TabClosed {
            workspace_id: self.id,
            document_id,
        });
        Ok(CloseDecision::Closed)
    }

    /// 强制关闭标签页（用户选择「不保存」时由应用层调用）。
    pub fn force_close_tab(&mut self, document_id: DocumentId) {
        self.tabs.retain(|tab| tab.document_id() != document_id);
        self.emit(WorkspaceEvent::TabClosed {
            workspace_id: self.id,
            document_id,
        });
    }

    // ---------- 最近文件 ----------

    /// 记录一次文件打开。
    pub fn touch_recent(&mut self, path: FilePath, at: Timestamp) {
        self.recent.touch(path, at);
        self.emit(WorkspaceEvent::RecentFilesChanged {
            workspace_id: self.id,
        });
    }

    /// 用持久化的记录恢复「最近文件」（启动时调用，保留原始时间戳）。
    pub(crate) fn restore_recent(&mut self, items: Vec<RecentFile>) {
        self.recent = RecentFiles::with_items(items, RECENT_FILES_LIMIT);
    }

    /// 从最近文件中移除（文件已被删除时）。
    pub fn forget_recent(&mut self, path: &FilePath) {
        self.recent.remove(path);
        self.emit(WorkspaceEvent::RecentFilesChanged {
            workspace_id: self.id,
        });
    }

    // ---------- 内部 ----------

    /// 确保路径位于根目录之内（未设置根目录时不做限制）。
    fn ensure_inside_root(&self, path: &FilePath) -> Result<(), WorkspaceError> {
        match &self.root {
            Some(root) if !path.starts_with_path(root) => Err(WorkspaceError::OutsideRoot {
                path: path.clone(),
                root: root.clone(),
            }),
            _ => Ok(()),
        }
    }

    /// 入队事件。
    fn emit(&mut self, event: WorkspaceEvent) {
        self.events.push(event);
    }
}
