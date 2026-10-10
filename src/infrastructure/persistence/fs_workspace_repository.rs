//! 文件系统工作区仓储（S5-05）：目录遍历 + 最近文件持久化。
//!
//! # 分层
//!
//! 本层只做 **IO**：读目录、读文件、读写最近文件列表。
//! 「哪些条目合法」「最近文件如何排序裁剪」等规则由领域层
//! [`crate::domain::workspace::aggregate::Workspace`] 负责——
//! 规则留在领域层才能被单测覆盖，留在 IO 层就只能靠集成测试碰运气。
//!
//! # 遍历的安全边界
//!
//! 桌面编辑器面对的是**任意用户目录**，故必须防御性地设置边界：
//!
//! - **深度限制**：防止深目录树（如误扫了 `node_modules`）拖垮 UI 线程；
//! - **条目上限**：超大目录一次性返回十万条会让文件树卡死；
//! - **符号链接跳过**：跟随链接可能形成环（`a -> b -> a`）导致无限递归，
//!   也可能把扫描引到网络盘或 `/proc` 这类伪文件系统；
//! - **隐藏文件过滤**：`.git`、`.DS_Store`、`node_modules` 等对用户无意义，
//!   却在文件树里占据大量空间。
//!
//! 过滤规则集中在 [`TraversalRules`]，便于测试与后续按偏好调整。

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::workspace::repository::WorkspaceRepository;
use crate::domain::workspace::value_objects::{EntryKind, RecentFile, WorkspaceEntry};
use crate::shared::error::AppError;
use crate::shared::path::FilePath;

use super::recent_files_store::RecentFilesStore;

/// 遍历规则（可按用户偏好覆盖）。
#[derive(Clone, Debug)]
pub struct TraversalRules {
    /// 默认忽略的目录名（小写比较）。
    pub ignored_dirs: Vec<String>,
    /// 是否忽略以 `.` 开头的隐藏文件 / 目录。
    pub skip_hidden: bool,
    /// 条目上限（防止超大目录拖垮 UI）。
    pub max_entries: usize,
}

impl Default for TraversalRules {
    fn default() -> Self {
        Self {
            ignored_dirs: vec![
                ".git".to_string(),
                "node_modules".to_string(),
                "target".to_string(),
                ".svn".to_string(),
                ".hg".to_string(),
                "dist".to_string(),
                "build".to_string(),
                ".idea".to_string(),
                ".vscode".to_string(),
                "__pycache__".to_string(),
            ],
            skip_hidden: true,
            max_entries: 5000,
        }
    }
}

impl TraversalRules {
    /// 该目录名是否应被忽略。
    pub fn is_ignored_dir(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        self.ignored_dirs.iter().any(|d| d.to_lowercase() == lower)
    }

    /// 该文件名是否应被忽略（隐藏文件规则）。
    pub fn is_hidden(&self, name: &str) -> bool {
        self.skip_hidden && name.starts_with('.')
    }
}

/// 文件系统工作区仓储。
#[derive(Debug)]
pub struct FsWorkspaceRepository {
    recent: RecentFilesStore,
    rules: TraversalRules,
}

impl FsWorkspaceRepository {
    /// 构造仓储，最近文件持久化到 `recent_file`。
    pub fn new(recent_file: impl Into<PathBuf>) -> Self {
        Self {
            recent: RecentFilesStore::new(recent_file),
            rules: TraversalRules::default(),
        }
    }

    /// 覆盖遍历规则（测试与用户偏好）。
    pub fn with_rules(mut self, rules: TraversalRules) -> Self {
        self.rules = rules;
        self
    }

    /// 最近文件存储（供组合根单独管理最近文件的写时机）。
    pub fn recent_store(&self) -> &RecentFilesStore {
        &self.recent
    }

    /// `FilePath` → `PathBuf`（内部统一用 `/` 分隔，Windows 上 `fs` 也能吃）。
    fn to_path(path: &FilePath) -> PathBuf {
        PathBuf::from(path.as_str())
    }

    /// 递归收集目录条目。
    ///
    /// 产出顺序：**目录在前、同层按名称排序**——与原型文件树的展示顺序一致，
    /// 也让「重命名后位置不动」这类预期成立（顺序确定而非依赖文件系统返回序）。
    fn collect(&self, root: &Path, max_depth: usize, out: &mut Vec<WorkspaceEntry>) -> bool {
        // 返回 false 表示已达到条目上限，调用方应立即停止递归。
        if out.len() >= self.rules.max_entries {
            return false;
        }
        let Ok(entries) = fs::read_dir(root) else {
            // 单个目录读失败（权限不足、已删除）不应中断整个扫描。
            return true;
        };

        // 先收集再排序：`read_dir` 的返回序在多数平台上是任意的。
        let mut dirs: Vec<(String, PathBuf)> = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            // 符号链接一律跳过：`DirEntry::file_type` 不跟随链接，
            // 因此 `is_symlink` 为真时不会进入任一分支。
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                log::debug!("跳过符号链接：{}", path.display());
                continue;
            }
            if self.rules.is_hidden(&name) || self.rules.is_ignored_dir(&name) {
                continue;
            }
            if file_type.is_dir() {
                dirs.push((name, path));
            } else {
                files.push((name, path));
            }
        }
        dirs.sort_by(|a, b| a.0.cmp(&b.0));
        files.sort_by(|a, b| a.0.cmp(&b.0));

        // 目录与文件必须**分别**标记 `EntryKind`（S6-11 修复）：
        // 二者曾共用 `EntryKind::File`，导致 `is_directory` 恒为 `false`，
        // 文件树无法建出层级——表现层只能把整个目录平铺成文件列表。
        // 探针实测：含 1 个子目录的工作区扫描出 0 个目录。
        for (is_dir, path) in dirs
            .iter()
            .map(|(_, path)| (true, path))
            .chain(files.iter().map(|(_, path)| (false, path)))
        {
            if out.len() >= self.rules.max_entries {
                return false;
            }
            if let Ok(fp) = FilePath::new(path.to_string_lossy().replace('\\', "/")) {
                let kind = if is_dir {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                };
                out.push(WorkspaceEntry::new(fp, kind));
            }
        }

        // `max_depth` 为 0 时只列出本层（不递归）；1 表示含一层子目录。
        if max_depth == 0 {
            return true;
        }
        for (_, path) in dirs {
            if !self.collect(&path, max_depth.saturating_sub(1), out) {
                return false;
            }
        }
        true
    }
}

impl WorkspaceRepository for FsWorkspaceRepository {
    fn scan(&self, root: &FilePath, max_depth: usize) -> Result<Vec<WorkspaceEntry>, AppError> {
        let root_path = PathBuf::from(root.as_str());
        if !root_path.is_dir() {
            return Err(AppError::io(format!(
                "工作区根目录不存在或不是目录：{}",
                root.as_str()
            )));
        }
        let mut out = Vec::new();
        if !self.collect(&root_path, max_depth, &mut out) {
            log::warn!(
                "工作区 {} 条目数达到上限 {}，扫描已截断",
                root.as_str(),
                self.rules.max_entries
            );
        }
        Ok(out)
    }

    fn load_recent(&self) -> Result<Vec<RecentFile>, AppError> {
        self.recent.load()
    }

    fn save_recent(&self, items: &[RecentFile]) -> Result<(), AppError> {
        self.recent.save(items)
    }

    fn read_content(&self, path: &FilePath) -> Result<String, AppError> {
        let bytes = fs::read(PathBuf::from(path.as_str()))
            .map_err(|e| AppError::io(format!("无法读取文件 {}：{e}", path.as_str())))?;
        // 复用文档仓储的解码规则（BOM 剥离 + CRLF 归一），
        // 保证搜索时看到的行与编辑器里看到的一致。
        Ok(super::fs_document_repository::FsDocumentRepository::decode_public(&bytes))
    }

    fn create_file(&self, path: &FilePath) -> Result<(), AppError> {
        // `create_new` 而非 `create`：`create` 会**静默截断**同名文件，
        // 用户「新建文件」时恰好撞上同名文件，就会把他原有的内容清空——
        // 而界面上看不出任何异常。重名应由领域层先拦，这里是第二道防线。
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(Self::to_path(path))
            .map(|_| ())
            .map_err(|e| AppError::io(format!("无法创建文件 {}：{e}", path.as_str())))
    }

    fn create_directory(&self, path: &FilePath) -> Result<(), AppError> {
        fs::create_dir_all(Self::to_path(path))
            .map_err(|e| AppError::io(format!("无法创建目录 {}：{e}", path.as_str())))
    }

    fn rename(&self, from: &FilePath, to: &FilePath) -> Result<(), AppError> {
        // 先查目标是否存在：`fs::rename` 在 POSIX 上会**静默覆盖**目标文件，
        // 用户「重命名为 b.md」而 b.md 已存在时，b.md 的内容就没了。
        // Windows 上会直接失败——两端行为不一致更难排查，故显式统一为拒绝。
        if Self::to_path(to).exists() {
            return Err(AppError::io(format!("已存在同名条目：{}", to.as_str())));
        }
        fs::rename(Self::to_path(from), Self::to_path(to)).map_err(|e| {
            AppError::io(format!(
                "无法移动 {} 到 {}：{e}",
                from.as_str(),
                to.as_str()
            ))
        })
    }

    fn remove(&self, path: &FilePath) -> Result<(), AppError> {
        let target = Self::to_path(path);
        // 目录用 `remove_dir`（非空即失败），文件用 `remove_file`。
        // 不用 `remove_dir_all`：右键「删除」一个目录就抹掉里面全部内容，
        // 是几乎必然招致后悔的行为（见 trait 文档）。
        let result = if target.is_dir() {
            fs::remove_dir(&target)
        } else {
            fs::remove_file(&target)
        };
        result.map_err(|e| {
            if target.is_dir() {
                AppError::io(format!(
                    "无法删除目录 {}（目录非空时请先清空）：{e}",
                    path.as_str()
                ))
            } else {
                AppError::io(format!("无法删除 {}：{e}", path.as_str()))
            }
        })
    }
}
