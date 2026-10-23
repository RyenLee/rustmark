//! Export 聚合根 `ExportJob`（S3-13）。
//!
//! 领域层**只校验选项与维护状态机**，真正的渲染与写盘由基础设施层完成
//! （`infrastructure/export`），PDF 一律走系统打印管线（ADR-004）。

use crate::shared::ids::{DocumentId, JobId};
use crate::shared::path::FilePath;

use super::errors::ExportError;
use super::services::TocBuilder;
use super::value_objects::{ExportFormat, ExportOptions, ExportStatus, TocEntry};

/// 一次导出任务。
#[derive(Clone, PartialEq, Debug)]
pub struct ExportJob {
    id: JobId,
    source: DocumentId,
    destination: FilePath,
    format: ExportFormat,
    options: ExportOptions,
    status: ExportStatus,
}

impl ExportJob {
    /// 构造导出任务（**不校验**；校验见 [`ExportJob::validate`]）。
    pub fn new(
        id: JobId,
        source: DocumentId,
        destination: FilePath,
        format: ExportFormat,
        options: ExportOptions,
    ) -> Self {
        Self {
            id,
            source,
            destination,
            format,
            options,
            status: ExportStatus::Pending,
        }
    }

    /// 任务 ID。
    pub(crate) const fn id(&self) -> JobId {
        self.id
    }

    /// 源文档 ID。
    pub(crate) const fn source(&self) -> DocumentId {
        self.source
    }

    /// 目标文件路径。
    pub(crate) const fn destination(&self) -> &FilePath {
        &self.destination
    }

    /// 导出格式。
    pub const fn format(&self) -> ExportFormat {
        self.format
    }

    /// 导出选项。
    pub const fn options(&self) -> &ExportOptions {
        &self.options
    }

    /// 当前状态。
    pub const fn status(&self) -> &ExportStatus {
        &self.status
    }

    /// 校验选项组合是否合法。
    ///
    /// 判定表（顺序即优先级）：
    /// 1. 主题名为空 → [`ExportError::EmptyTheme`]；
    /// 2. 目标扩展名与格式不符 → [`ExportError::ExtensionMismatch`]；
    /// 3. 格式不支持目录但要求目录 → [`ExportError::TocNotSupported`]；
    /// 4. 格式不支持分页但要求分页 → [`ExportError::PaginationNotSupported`]；
    /// 5. PDF 关闭分页却要求目录 → [`ExportError::TocRequiresPagination`]。
    pub fn validate(&self) -> Result<(), ExportError> {
        if self.options.theme().trim().is_empty() {
            return Err(ExportError::EmptyTheme);
        }

        let expected = self.format.extension();
        match self.destination.extension() {
            Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
            other => {
                return Err(ExportError::ExtensionMismatch {
                    expected,
                    actual: other.unwrap_or("").to_string(),
                });
            }
        }

        if self.options.include_toc() && !self.format.supports_toc() {
            return Err(ExportError::TocNotSupported {
                format: self.format,
            });
        }

        if self.options.paginate() && !self.format.supports_pagination() {
            return Err(ExportError::PaginationNotSupported {
                format: self.format,
            });
        }

        if self.format == ExportFormat::Pdf
            && self.options.include_toc()
            && !self.options.paginate()
        {
            return Err(ExportError::TocRequiresPagination);
        }

        Ok(())
    }

    /// 校验并附带目录转义检查（完整校验入口）。
    pub fn validate_with_toc(&self, entries: &[TocEntry]) -> Result<(), ExportError> {
        self.validate()?;
        TocBuilder::verify(entries)
    }

    /// 是否真的需要生成目录（格式支持 **且** 用户勾选）。
    pub const fn needs_toc(&self) -> bool {
        self.options.include_toc() && self.format.supports_toc()
    }

    /// 标记为开始执行；非 `Pending` 状态返回当前状态不变（幂等）。
    pub fn start(&mut self) {
        if matches!(self.status, ExportStatus::Pending) {
            self.status = ExportStatus::Running;
        }
    }

    /// 标记成功。
    pub fn succeed(&mut self) {
        self.status = ExportStatus::Succeeded;
    }

    /// 标记失败（携带可呈现给用户的原因）。
    pub fn fail(&mut self, reason: impl Into<String>) {
        self.status = ExportStatus::Failed {
            reason: reason.into(),
        };
    }
}
