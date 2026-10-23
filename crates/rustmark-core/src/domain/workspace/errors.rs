//! Workspace 上下文的错误类型。

use thiserror::Error;

use crate::shared::error::AppError;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

/// 工作区规则拒绝了该操作。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    /// 同一父目录下已存在同名条目。
    #[error("目录下已存在同名条目：{path}")]
    DuplicateName {
        /// 撞名的目标条目路径。
        path: FilePath,
    },

    /// 试图把目录移动到**它自己的子目录**中（会产生环）。
    #[error("不能把 {from} 移动到它自己的子目录 {to} 中")]
    MoveIntoDescendant {
        /// 待移动的源目录路径。
        from: FilePath,
        /// 目标路径（位于 `from` 之内）。
        to: FilePath,
    },

    /// 待操作的条目不存在。
    #[error("工作区中不存在：{path}")]
    NotFound {
        /// 查无此物的路径。
        path: FilePath,
    },

    /// 该路径不是目录（例如把文件当作目录展开）。
    #[error("{path} 不是目录")]
    NotADirectory {
        /// 实为普通文件的路径。
        path: FilePath,
    },

    /// 尚未设置工作区根目录。
    #[error("尚未打开工作区目录")]
    RootNotSet,

    /// 待关闭的标签页不存在。
    #[error("标签页中不存在 ID 为 {0} 的文档")]
    TabNotFound(DocumentId),

    /// 路径不在工作区根目录之内（防止越界操作）。
    #[error("{path} 不在工作区根目录 {root} 之内")]
    OutsideRoot {
        /// 试图访问的越界路径。
        path: FilePath,
        /// 工作区根目录，越界的判定基准。
        root: FilePath,
    },
}

impl From<WorkspaceError> for AppError {
    fn from(e: WorkspaceError) -> Self {
        Self::Domain {
            message: e.to_string(),
        }
    }
}
