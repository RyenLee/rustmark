//! Export 用例命令（阶段 04 · S4-09）。
//!
//! 命令携带 DTO，由服务转换为领域 [`crate::domain::export::aggregate::ExportJob`] 与
//! [`crate::domain::export::value_objects::ExportOptions`]，确保 AppService 签名不暴露聚合根。

use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

use crate::application::dto::ExportRequestView;

/// 创建导出任务命令。
#[derive(Clone, Debug)]
pub struct CreateExportJobCommand {
    /// 源文档 ID。
    pub doc_id: DocumentId,
    /// 输出目标路径。
    pub destination: FilePath,
    /// 导出参数（DTO）。
    pub request: ExportRequestView,
}

/// 执行导出任务命令。
#[derive(Clone, Debug)]
pub struct RunExportCommand {
    /// 任务 ID。
    pub job_id: crate::shared::ids::JobId,
}
