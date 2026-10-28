//! Export 用例服务测试（S4-09）。
//!
//! 覆盖：创建任务（HTML 成功 / PDF 已开放可创建（V11-04）/ 未知格式 Err / 非法组合被 validate 拒绝且不调 Exporter）、
//! 执行（成功广播 ExportFinished、失败标记 Failed 且不误报成功）、查询状态、未知任务错误路径。

use std::sync::Arc;

use rustmark::application::dto::{ExportRequestView, ExportResultView};
use rustmark::application::export::commands::*;
use rustmark::application::export::queries::*;
use rustmark::application::export::service::ExportAppService;
use rustmark::ports::event::{DomainEvent, EventBus};
use rustmark::shared::ids::{DocumentId, JobId};
use rustmark::shared::path::FilePath;

use super::fakes::*;

fn fp(s: &str) -> FilePath {
    FilePath::new(s).unwrap()
}

struct ExFixture {
    service: ExportAppService,
    exporter: Arc<FakeExporter>,
    bus: Arc<InMemoryEventBus>,
    doc_id: DocumentId,
}

fn build(exporter_should_fail: bool) -> ExFixture {
    let provider = Arc::new(FakeDocumentContentProvider::new());
    let doc_id = DocumentId::new();
    provider.insert(doc_id, "# Hello", 0, Some(fp("/d/a.md")));
    let exporter = Arc::new(FakeExporter::new(exporter_should_fail));
    let bus: Arc<InMemoryEventBus> = Arc::new(InMemoryEventBus::new());
    let bus_dyn: Arc<dyn EventBus> = bus.clone();
    let service = ExportAppService::new(provider.clone(), exporter.clone(), bus_dyn.clone());
    ExFixture {
        service,
        exporter,
        bus,
        doc_id,
    }
}

fn html_request(_destination: &str) -> ExportRequestView {
    ExportRequestView {
        format: "html".to_string(),
        include_toc: true,
        embed_images: false,
        theme: "light".to_string(),
        page_size: "a4".to_string(),
        paginate: false,
    }
}

#[test]
fn create_job_html_succeeds() {
    let f = build(false);
    let job_id = f
        .service
        .create_job(CreateExportJobCommand {
            doc_id: f.doc_id,
            destination: fp("/out/a.html"),
            request: html_request("/out/a.html"),
        })
        .unwrap();
    assert!(job_id.as_u64() != 0, "任务 ID 必须非零");
}

#[test]
fn run_html_export_succeeds_and_broadcasts() {
    let f = build(false);
    let job_id = f
        .service
        .create_job(CreateExportJobCommand {
            doc_id: f.doc_id,
            destination: fp("/out/a.html"),
            request: html_request("/out/a.html"),
        })
        .unwrap();

    let result: ExportResultView = f.service.run(RunExportCommand { job_id }).unwrap();
    assert_eq!(result.format, "HTML");
    assert_eq!(result.bytes, "# Hello".len());
    assert_eq!(f.exporter.calls().len(), 1);
    assert!(
        f.exporter.calls()[0].include_toc,
        "导出选项必须下传给 Exporter"
    );
    assert_eq!(
        f.service.job_status(JobStatusQuery { job_id }).unwrap(),
        "succeeded"
    );
    assert!(
        f.bus
            .published()
            .iter()
            .any(|e| matches!(e, DomainEvent::ExportFinished(j) if j.as_u64() == job_id.as_u64())),
        "成功后必须广播 ExportFinished"
    );
}

/// PDF **已开放**（V11-04）：任务可创建、可进入执行阶段。
///
/// 这里原先断言「PDF 必须返回 Err」——那是「界面未开放」时期的行为。
/// 开放后 PDF 经由系统打印管线产出，**可用性由基础设施在执行期探测**：
/// 后端可用 → 落打印用 HTML 并调起打印；不可用 → 明确 `Err`。
/// 本用例的 `FakeExporter` 代表「可用后端」，故应成功；
/// 「后端不可用」的分支由 `tests/infrastructure/pdf_exporter_test.rs` 覆盖。
#[test]
fn create_job_pdf_is_accepted_and_runs() {
    let f = build(false);
    let job_id = f
        .service
        .create_job(CreateExportJobCommand {
            doc_id: f.doc_id,
            destination: fp("/out/a.pdf"),
            request: ExportRequestView {
                format: "pdf".to_string(),
                include_toc: false,
                embed_images: false,
                theme: "light".to_string(),
                page_size: "a4".to_string(),
                paginate: false,
            },
        })
        .expect("PDF 已开放：任务应可创建（V11-04）");

    let result: ExportResultView = f.service.run(RunExportCommand { job_id }).unwrap();
    assert_eq!(result.format, "PDF");
    assert_eq!(
        f.service.job_status(JobStatusQuery { job_id }).unwrap(),
        "succeeded"
    );
}

#[test]
fn create_job_unknown_format_fails() {
    let f = build(false);
    let res = f.service.create_job(CreateExportJobCommand {
        doc_id: f.doc_id,
        destination: fp("/out/a.docx"),
        request: ExportRequestView {
            format: "docx".to_string(),
            include_toc: false,
            embed_images: false,
            theme: "light".to_string(),
            page_size: "a4".to_string(),
            paginate: false,
        },
    });
    assert!(res.is_err());
}

#[test]
fn create_job_invalid_combination_rejected_without_calling_exporter() {
    let f = build(false);
    let res = f.service.create_job(CreateExportJobCommand {
        doc_id: f.doc_id,
        destination: fp("/out/a.png"),
        request: ExportRequestView {
            format: "image".to_string(),
            include_toc: true, // 图片不支持目录 → validate 拒绝
            embed_images: false,
            theme: "light".to_string(),
            page_size: "a4".to_string(),
            paginate: false,
        },
    });
    assert!(res.is_err(), "非法选项组合必须被拒绝");
    assert_eq!(f.exporter.calls().len(), 0, "被拒绝的创建不得调用 Exporter");
}

#[test]
fn run_exporter_failure_marks_failed_and_does_not_report_success() {
    let f = build(true);
    let job_id = f
        .service
        .create_job(CreateExportJobCommand {
            doc_id: f.doc_id,
            destination: fp("/out/a.html"),
            request: html_request("/out/a.html"),
        })
        .unwrap();

    let res = f.service.run(RunExportCommand { job_id });
    assert!(res.is_err(), "Exporter 失败必须返回 Err");
    let status = f.service.job_status(JobStatusQuery { job_id }).unwrap();
    assert!(
        status.starts_with("failed:"),
        "失败必须标记 Failed，绝不误报成功（实际：{status}）"
    );
    assert!(
        status.contains("模拟导出失败"),
        "失败原因应保留底层错误信息（实际：{status}）"
    );
    assert_eq!(f.exporter.calls().len(), 1, "即使失败也应实际尝试导出");
    assert!(
        !f.bus
            .published()
            .iter()
            .any(|e| matches!(e, DomainEvent::ExportFinished(_))),
        "失败的导入不得广播 ExportFinished"
    );
}

#[test]
fn run_unknown_job_fails() {
    let f = build(false);
    let res = f.service.run(RunExportCommand {
        job_id: JobId::new(),
    });
    assert!(res.is_err());
}

#[test]
fn job_status_unknown_fails() {
    let f = build(false);
    let res = f.service.job_status(JobStatusQuery {
        job_id: JobId::new(),
    });
    assert!(res.is_err());
}
