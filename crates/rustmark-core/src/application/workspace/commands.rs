//! Workspace 用例命令（阶段 04 · S4-07）。
//!
//! 命令是**纯数据**结构，字段对齐工作区聚合的访问器与领域事件。

use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

/// 扫描文件树命令（也会把结果写入聚合的工作区条目）。
#[derive(Clone, Debug)]
pub struct ScanTreeCommand {
    /// 根目录。
    pub root: FilePath,
    /// 递归深度上限。
    pub max_depth: usize,
}

/// 记录一次文件打开（更新最近文件）。
#[derive(Clone, Debug)]
pub struct TouchRecentCommand {
    /// 被打开的文件路径。
    pub path: FilePath,
}

/// 从最近文件中移除（文件被删除或用户手动清除）。
#[derive(Clone, Debug)]
pub struct ForgetRecentCommand {
    /// 待移除的路径。
    pub path: FilePath,
}

/// 打开（或激活）一个标签页。
#[derive(Clone, Debug)]
pub struct OpenTabCommand {
    /// 文档 ID。
    pub doc_id: DocumentId,
}

/// 关闭标签页。
#[derive(Clone, Debug)]
pub struct CloseTabCommand {
    /// 文档 ID。
    pub doc_id: DocumentId,
    /// 该文档是否存在未保存修改；为 `true` 时返回 [`CloseDecisionView::NeedsSave`]。
    pub dirty: bool,
}

/// 强制关闭标签页（用户选择「不保存」）。
#[derive(Clone, Debug)]
pub struct ForceCloseTabCommand {
    /// 文档 ID。
    pub doc_id: DocumentId,
}

// ===========================================================================
// 文件树条目操作（S6-11：右键菜单「新建 / 重命名 / 删除 / 移动」）
// ===========================================================================
//
// 四条命令都只带**纯数据**路径，不带任何 IO 语义：
// 「新建文件叫什么扩展名」「移动到同级还是子目录」这类决定属于表现层，
// 而「重名是否允许」「能不能移进自己的子目录」属于领域——
// 分工与 S3-11 聚合的设计一致。

/// 在工作区中新建文件或目录。
#[derive(Clone, Debug)]
pub struct CreateEntryCommand {
    /// 新条目的完整路径（含文件名）。
    pub path: FilePath,
    /// 是否为目录。
    pub is_directory: bool,
}

/// 重命名或移动条目（同一对路径覆盖两种语义）。
#[derive(Clone, Debug)]
pub struct MoveEntryCommand {
    /// 源路径。
    pub from: FilePath,
    /// 目标完整路径。
    pub to: FilePath,
}

/// 删除条目（文件或空目录）。
#[derive(Clone, Debug)]
pub struct DeleteEntryCommand {
    /// 待删除路径。
    pub path: FilePath,
}
