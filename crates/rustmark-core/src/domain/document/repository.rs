//! Document 上下文的仓储 trait（S3-06 支撑）。
//!
//! 领域层**只定义接口**，不做 IO：由 `infrastructure::persistence::FsDocumentRepository`
//! 实现（S5-02）。保存必须是**原子的**（`FR-04`：临时文件 + rename）。

use std::path::Path;

use crate::shared::encoding::TextEncoding;
use crate::shared::error::AppError;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;
use crate::shared::time::Timestamp;

/// 文档在磁盘上的一次完整读写契约。
///
/// 之所以传 `content: String` 而非 `&Document`：仓储位于基础设施层，
/// 不应反向依赖聚合的内部结构（`Box<dyn TextBuffer>` 无法跨层传递）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocumentSnapshot {
    /// 文档 ID（由仓储在首次加载时分配并保持稳定）。
    pub(crate) id: DocumentId,
    /// 文件路径。
    pub path: FilePath,
    /// 全文内容。
    pub content: String,
    /// 该内容在磁盘上的最后保存时间；新文件为 `None`。
    pub saved_at: Option<Timestamp>,
    /// 读取时识别到的编码（V11-03 / DEF-05）。
    ///
    /// `None` 表示「尚未与磁盘发生关系」——新建未保存的文档没有编码可言，
    /// 状态栏此时不应显示任何编码名（显示 `UTF-8` 会是撒谎）。
    ///
    /// 存的是**共享内核的纯标签**（`shared::encoding::TextEncoding`）而非
    /// 基础设施层的 `Encoding`：后者带解码实现，领域层不得依赖它。
    pub(crate) encoding: Option<TextEncoding>,
}

impl DocumentSnapshot {
    /// 构造快照（未知编码）。
    pub fn new(
        id: DocumentId,
        path: FilePath,
        content: impl Into<String>,
        saved_at: Option<Timestamp>,
    ) -> Self {
        Self {
            id,
            path,
            content: content.into(),
            saved_at,
            encoding: None,
        }
    }

    /// 构造快照并带上识别到的编码（磁盘读取路径用）。
    pub fn with_encoding(
        id: DocumentId,
        path: FilePath,
        content: impl Into<String>,
        saved_at: Option<Timestamp>,
        encoding: TextEncoding,
    ) -> Self {
        Self {
            encoding: Some(encoding),
            ..Self::new(id, path, content, saved_at)
        }
    }
}

/// 文档仓储。
///
/// 实现必须 `Send + Sync`；`save` 必须保证**原子性**（`FR-04`）。
pub trait DocumentRepository: Send + Sync {
    /// 按路径读取文档；失败时返回 [`AppError::Io`] 或 [`AppError::Config`]（编码错误）。
    fn load(&self, path: &FilePath) -> Result<DocumentSnapshot, AppError>;

    /// 原子保存：写临时文件后 `rename` 覆盖目标，任何失败都不得破坏原文件。
    fn save(&self, snapshot: &DocumentSnapshot) -> Result<(), AppError>;

    /// 路径是否已存在（用于「保存前是否覆盖」的提示）。
    fn exists(&self, path: &FilePath) -> bool;

    /// 删除文件；不存在时按成功处理（幂等）。
    fn delete(&self, path: &FilePath) -> Result<(), AppError>;

    /// 把未保存草稿写到 `drafts_dir` 下的 `<doc_id>.draft`（异常退出后可恢复，需求 §4.3）。
    ///
    /// `path` 为 `Some` 时**同时**写旁车文件 `<doc_id>.draft.path`：草稿要连
    /// 文档身份一起存，否则恢复出来的文档一律是「未命名」，标签条丢失原始
    /// 文件名（问题 069）。未命名文档（从未保存过）传 `None`，不写旁车。
    ///
    /// 默认实现为空操作：**仅真实文件系统仓储需要持久化草稿**，
    /// 测试替身（`InMemory` / `Failing`）继承此默认即可，不必各自实现。
    /// 真实实现见 `infrastructure::persistence::FsDocumentRepository`。
    fn save_draft(
        &self,
        _drafts_dir: &Path,
        _id: DocumentId,
        _content: &str,
        _path: Option<&FilePath>,
    ) -> Result<(), AppError> {
        Ok(())
    }

    /// 读取草稿；不存在时返回 `None`（默认实现）。
    fn load_draft(&self, _drafts_dir: &Path, _id: DocumentId) -> Result<Option<String>, AppError> {
        Ok(None)
    }

    /// 读取草稿旁车文件里记的文档路径（问题 069）；不存在返回 `None`（默认实现）。
    ///
    /// 与 [`Self::load_draft`] 分开而不是塞进内容文件：草稿正文是用户数据，
    /// 混入路径头会让「读回内容」多一层解析，且内容本身可能含任意字节。
    fn load_draft_path(
        &self,
        _drafts_dir: &Path,
        _id: DocumentId,
    ) -> Result<Option<FilePath>, AppError> {
        Ok(None)
    }

    /// 删除草稿；不存在按成功处理（默认实现）。
    fn clear_draft(&self, _drafts_dir: &Path, _id: DocumentId) -> Result<(), AppError> {
        Ok(())
    }

    /// 列出 `drafts_dir` 下全部遗留草稿对应的文档 ID（默认实现为空列表）。
    fn list_drafts(&self, _drafts_dir: &Path) -> Result<Vec<DocumentId>, AppError> {
        Ok(Vec::new())
    }
}
