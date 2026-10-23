//! Export 用例查询（阶段 04 · S4-09）。
//!
//! 查询与命令命名不冲突（动名词区分）。

/// 查询某导出任务的状态（用于进度展示与失败诊断）。
#[derive(Clone, Debug)]
pub struct JobStatusQuery {
    /// 任务 ID。
    pub job_id: crate::shared::ids::JobId,
}
