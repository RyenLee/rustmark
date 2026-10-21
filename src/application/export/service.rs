//! Export 用例编排服务（阶段 04 · S4-09）。
//!
//! 编排导出任务的生命周期：创建（`validate` 校验选项）→ 执行（取内容 → 调基础设施
//! [`Exporter`] → 推进状态机 → 广播 [`DomainEvent::ExportFinished`]）。
//!
//! **失败语义优先**（原型 2.1 反面教材）：
//! - 非法选项组合在创建/执行时被领域 [`ExportJob::validate`] 拒绝，**绝不调用 Exporter**；
//! - PDF 界面暂未开放（基础设施为系统打印管线，v1.1 决策），返回**明确**的 `Err`，不静默；
//! - Exporter 失败时返回 `Err`，且任务状态被标记为 `Failed`，绝不误报成功。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::domain::export::aggregate::ExportJob;
use crate::domain::export::errors::ExportError;
use crate::domain::export::services::Exporter;
use crate::domain::export::value_objects::{ExportFormat, ExportOptions, ExportStatus, PageSize};

use crate::shared::error::AppError;
use crate::shared::event::{DomainEvent, EventBus};
use crate::shared::ids::JobId;
use crate::shared::result::AppResult;

use crate::application::document::DocumentContentProvider;
use crate::application::dto::{ExportRequestView, ExportResultView};

use super::commands::*;
use super::queries::*;

/// Export 用例服务。
pub struct ExportAppService {
    provider: Arc<dyn DocumentContentProvider>,
    exporter: Arc<dyn Exporter>,
    bus: Arc<dyn EventBus>,
    jobs: Arc<Mutex<HashMap<JobId, ExportJob>>>,
}

impl ExportAppService {
    /// 构造服务（依赖全部注入）。
    pub fn new(
        provider: Arc<dyn DocumentContentProvider>,
        exporter: Arc<dyn Exporter>,
        bus: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            provider,
            exporter,
            bus,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 创建导出任务：校验选项（含扩展名匹配、TOC/分页可行性），失败返回 `Err` 且不存储。
    ///
    /// **PDF 已开放（V11-04 决策）**：它的产物经由**系统打印管线**产出——
    /// 先落一份打印用 HTML，再调起系统打印对话框，由用户在对话框中
    /// 选择「另存为 PDF」。因此本进程**无法预先验证**「能否打印」：
    /// 可用性由 `SystemPdfExporter` 在**执行期**探测，不可用时返回明确错误
    /// （如「未找到 lp/lpstat（CUPS 客户端未安装）」），而不是静默失败。
    ///
    /// 同样因为产物不由本进程写盘，导出结果里的 `bytes` 恒为 0——
    /// 表现层据此不显示「0 字节」，而是提示去对话框里另存（见 `export_view`）。
    pub fn create_job(&self, cmd: CreateExportJobCommand) -> AppResult<JobId> {
        let format = parse_format(&cmd.request.format)?;
        let options = to_options(&cmd.request)?;
        let job = ExportJob::new(
            JobId::new(),
            cmd.doc_id,
            cmd.destination.clone(),
            format,
            options,
        );

        // 领域层校验选项组合：非法（扩展名不符、TOC/分页不可行等）直接拒绝，**不**调用 Exporter。
        job.validate().map_err(AppError::from)?;

        let id = job.id();
        self.jobs
            .lock()
            .map_err(|_| AppError::io("导出任务锁已损坏"))?
            .insert(id, job);
        Ok(id)
    }

    /// 执行导出任务：取内容 → 调 Exporter → 推进状态机 → 广播 `ExportFinished`。
    ///
    /// 任何失败（内容缺失 / Exporter 错误）都返回 `Err`，并把任务标记为 `Failed`，
    /// 绝不误报成功。
    pub fn run(&self, cmd: RunExportCommand) -> AppResult<ExportResultView> {
        let (job, content) = {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| AppError::io("导出任务锁已损坏"))?;
            let job = jobs
                .get_mut(&cmd.job_id)
                .ok_or_else(|| AppError::domain("导出任务不存在"))?;
            let doc_id = job.source();
            let content = self
                .provider
                .content_and_revision(doc_id)
                .map(|(text, _)| text)
                .ok_or_else(|| AppError::domain("源文档未打开"))?;
            job.start();
            (job.clone(), content)
        };

        let result = self.exporter.export(&content, &job);
        match result {
            Ok(written) => {
                {
                    let mut jobs = self
                        .jobs
                        .lock()
                        .map_err(|_| AppError::io("导出任务锁已损坏"))?;
                    if let Some(job) = jobs.get_mut(&cmd.job_id) {
                        job.succeed();
                    }
                }
                self.bus.publish(DomainEvent::ExportFinished(cmd.job_id));
                Ok(ExportResultView {
                    job_id: cmd.job_id,
                    output_path: job.destination().clone(),
                    // 产出大小由导出器给出（真正写盘的那一层），
                    // **不是** `content.len()`——后者是源文长度，
                    // 与磁盘上的文件对不上（详见 `Exporter::export` 的文档）。
                    bytes: written,
                    format: job.format().label().to_string(),
                })
            }
            Err(e) => {
                {
                    let mut jobs = self
                        .jobs
                        .lock()
                        .map_err(|_| AppError::io("导出任务锁已损坏"))?;
                    if let Some(job) = jobs.get_mut(&cmd.job_id) {
                        job.fail(e.to_string());
                    }
                }
                Err(AppError::from(e))
            }
        }
    }

    /// 标记一个**已经 start 过**的任务完成（成功或失败），并广播结果事件。
    ///
    /// # 为什么需要它：图片导出天然跨帧
    ///
    /// 其余三种格式都是「拿到源文本 → 同步写盘 → 返回字节数」。
    /// 图片不同——像素来自 GPU 回读，而 egui 的
    /// `ViewportCommand::Screenshot` 要到**下一帧**才通过
    /// `Event::Screenshot` 交回来（帧 N 发命令、帧 N 绘制时被捕获）。
    ///
    /// 于是图片导出必然是「本帧登记 + 下一帧交付」。若让 [`Self::run`]
    /// 直接返回，就得给[`ExportResultView`]加一个「尚未完成」的状态——
    /// 那会让**所有**格式的调用方都得处理这个第三种结局，
    /// 而它只对1/4 的格式有意义。改为把「执行」这一段挪到调用方，
    /// 状态机、事件广播、失败语义全部复用既有实现，不新增中间态。
    ///
    /// 刻意**不**在这里取内容、也不校验文档是否仍打开：像素在登记时
    /// 已经拿到了，此刻再查一次只会引入「导出中途关了文档」这类
    /// 与截图无关的失败。
    pub fn complete_deferred(
        &self,
        job_id: JobId,
        outcome: Result<usize, ExportError>,
    ) -> AppResult<ExportResultView> {
        // 先推进状态机再消费 `outcome`：状态推进必须在锁内完成
        // （与 `run` 同构），而 `outcome` 的值在锁外用。
        let job = {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| AppError::io("导出任务锁已损坏"))?;
            let job = jobs
                .get_mut(&job_id)
                .ok_or_else(|| AppError::domain("导出任务不存在"))?;
            match &outcome {
                Ok(_) => job.succeed(),
                Err(e) => job.fail(e.to_string()),
            }
            job.clone()
        };

        match outcome {
            Ok(written) => {
                self.bus.publish(DomainEvent::ExportFinished(job_id));
                Ok(ExportResultView {
                    job_id,
                    output_path: job.destination().clone(),
                    bytes: written,
                    format: job.format().label().to_string(),
                })
            }
            Err(e) => {
                // 失败**不**广播 `ExportFinished`：该事件在别处被当作
                // 「导出成功完成」使用（见 `run` 的同名分支）。
                // 复用它广播失败，等于让订阅者收到一个自相矛盾的信号。
                Err(AppError::from(e))
            }
        }
    }

    /// 查询任务状态（用于失败诊断与进度展示）。
    pub fn job_status(&self, query: JobStatusQuery) -> AppResult<String> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| AppError::io("导出任务锁已损坏"))?;
        let job = jobs
            .get(&query.job_id)
            .ok_or_else(|| AppError::domain("导出任务不存在"))?;
        Ok(match job.status() {
            ExportStatus::Pending => "pending".to_string(),
            ExportStatus::Running => "running".to_string(),
            ExportStatus::Succeeded => "succeeded".to_string(),
            ExportStatus::Failed { reason } => format!("failed: {reason}"),
        })
    }
}

/// 由字符串解析导出格式；未知格式返回 `Err`。
///
/// 解析规则（可识别的取值、大小写、空白、历史别名）全在
/// [`ExportFormat::parse`] 这一处——本函数只负责把「不认识的」
/// 翻译成用户看得懂的错误文案。
fn parse_format(raw: &str) -> AppResult<ExportFormat> {
    ExportFormat::parse(raw).ok_or_else(|| {
        AppError::domain(format!(
            "未知导出格式：{}（应为 html / markdown / pdf / image）",
            raw.trim()
        ))
    })
}

/// 由 DTO 构造导出选项。
fn to_options(request: &ExportRequestView) -> AppResult<ExportOptions> {
    let page_size = PageSize::parse(&request.page_size).ok_or_else(|| {
        AppError::domain(format!(
            "未知页面尺寸：{}（应为 a4 / letter）",
            request.page_size.trim()
        ))
    })?;
    Ok(ExportOptions::new(
        request.include_toc,
        request.embed_images,
        request.theme.clone(),
        page_size,
        request.paginate,
    ))
}

/// 编译期断言：`ExportAppService` 可跨线程共享。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_EXPORT_SERVICE: () = _assert_send_sync::<ExportAppService>();
