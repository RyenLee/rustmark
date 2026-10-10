//! Workspace 上下文的仓储 trait。
//!
//! **目录遍历与磁盘 IO 只在这里定义、由基础设施层实现**（S5-03 / S5-06）；
//! 领域层的 [`crate::domain::workspace::aggregate::Workspace`] 只消费结果并校验规则。

use crate::shared::error::AppError;
use crate::shared::path::FilePath;

use super::value_objects::{RecentFile, WorkspaceEntry};

/// 工作区仓储。
pub trait WorkspaceRepository: Send + Sync {
    /// 扫描根目录，返回 `max_depth` 层以内的条目（目录在前、同层按名称排序）。
    fn scan(&self, root: &FilePath, max_depth: usize) -> Result<Vec<WorkspaceEntry>, AppError>;

    /// 读取最近文件列表（持久化在配置目录）。
    fn load_recent(&self) -> Result<Vec<RecentFile>, AppError>;

    /// 保存最近文件列表。
    fn save_recent(&self, items: &[RecentFile]) -> Result<(), AppError>;

    /// 读取某一路径的文本内容（用于跨文件搜索与预览）。
    ///
    /// 不存在或不可读时返回 [`AppError::Io`]。应用层据此对磁盘文件做内容检索，
    /// 不由应用层直接调用 `std::fs`。
    fn read_content(&self, path: &FilePath) -> Result<String, AppError>;

    // ---------- 文件操作（S6-11：文件树右键菜单） ----------
    //
    // 这四个方法是**唯一**允许写工作区文件系统的出口。「哪些操作合法」
    // （重名、移动到自身子目录、越界根目录）由 [`Workspace`](super::aggregate::Workspace)
    // 判定，仓储只负责执行——规则留在领域层才能被单测覆盖。

    /// 创建空文件；父目录必须已存在，同名时**不得**覆盖已有文件。
    fn create_file(&self, path: &FilePath) -> Result<(), AppError>;

    /// 创建目录（含缺失的父目录）。
    fn create_directory(&self, path: &FilePath) -> Result<(), AppError>;

    /// 重命名 / 移动（同一对路径覆盖两种语义）。
    fn rename(&self, from: &FilePath, to: &FilePath) -> Result<(), AppError>;

    /// 删除文件或**空**目录。
    ///
    /// 刻意**不**递归删除：右键删掉一个目录若连带删掉里面 200 个文件，
    /// 用户几乎一定后悔。递归删除应该是另一个**显式**动作（且带确认），
    /// 不能藏在「删除」背后。
    fn remove(&self, path: &FilePath) -> Result<(), AppError>;
}
