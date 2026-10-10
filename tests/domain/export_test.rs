//! Export 上下文单测（S3-13）。
//!
//! 判定表覆盖 4 种格式 × 关键选项组合（≥12 条），并验证 TOC 的转义标记
//! （原型缺陷 2.4 的根治点）。

use rustmark::domain::export::aggregate::ExportJob;
use rustmark::domain::export::errors::ExportError;
use rustmark::domain::export::services::TocBuilder;
use rustmark::domain::export::value_objects::{
    ExportFormat, ExportOptions, ExportStatus, TocEntry,
};
use rustmark::shared::ids::{DocumentId, JobId};
use rustmark::shared::path::FilePath;

fn path(raw: &str) -> FilePath {
    FilePath::new(raw).expect("测试路径应合法")
}

fn job(format: ExportFormat, destination: &str, options: ExportOptions) -> ExportJob {
    ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path(destination),
        format,
        options,
    )
}

fn options(include_toc: bool, paginate: bool) -> ExportOptions {
    ExportOptions::new(include_toc, true, "default", Default::default(), paginate)
}

// ---------- 格式 × 选项判定表 ----------

#[test]
fn html_with_toc_is_valid() {
    let job = job(ExportFormat::Html, "/tmp/a.html", options(true, false));
    assert!(job.validate().is_ok());
}

#[test]
fn html_without_toc_is_valid() {
    let job = job(ExportFormat::Html, "/tmp/a.html", options(false, false));
    assert!(job.validate().is_ok());
}

#[test]
fn html_with_pagination_is_rejected() {
    let job = job(ExportFormat::Html, "/tmp/a.html", options(false, true));
    assert!(matches!(
        job.validate(),
        Err(ExportError::PaginationNotSupported { .. })
    ));
}

#[test]
fn html_with_wrong_extension_is_rejected() {
    let job = job(ExportFormat::Html, "/tmp/a.pdf", options(false, false));
    assert!(matches!(
        job.validate(),
        Err(ExportError::ExtensionMismatch { .. })
    ));
}

#[test]
fn html_extension_match_is_case_insensitive() {
    let job = job(ExportFormat::Html, "/tmp/a.HTML", options(false, false));
    assert!(job.validate().is_ok());
}

#[test]
fn markdown_plain_is_valid() {
    let job = job(ExportFormat::Markdown, "/tmp/a.md", options(false, false));
    assert!(job.validate().is_ok());
}

#[test]
fn markdown_with_toc_is_rejected() {
    let job = job(ExportFormat::Markdown, "/tmp/a.md", options(true, false));
    assert!(matches!(
        job.validate(),
        Err(ExportError::TocNotSupported { .. })
    ));
}

#[test]
fn pdf_with_toc_and_pagination_is_valid() {
    let job = job(ExportFormat::Pdf, "/tmp/a.pdf", options(true, true));
    assert!(job.validate().is_ok());
}

#[test]
fn pdf_with_toc_without_pagination_is_rejected() {
    let job = job(ExportFormat::Pdf, "/tmp/a.pdf", options(true, false));
    assert!(matches!(
        job.validate(),
        Err(ExportError::TocRequiresPagination)
    ));
}

#[test]
fn pdf_without_toc_without_pagination_is_valid() {
    // 长图式 PDF：不带页码，故也不带目录。
    let job = job(ExportFormat::Pdf, "/tmp/a.pdf", options(false, false));
    assert!(job.validate().is_ok());
}

#[test]
fn image_without_options_is_valid() {
    let job = job(ExportFormat::Image, "/tmp/a.png", options(false, false));
    assert!(job.validate().is_ok());
}

#[test]
fn image_with_toc_is_rejected() {
    let job = job(ExportFormat::Image, "/tmp/a.png", options(true, false));
    assert!(matches!(
        job.validate(),
        Err(ExportError::TocNotSupported { .. })
    ));
}

#[test]
fn image_with_pagination_is_rejected() {
    let job = job(ExportFormat::Image, "/tmp/a.png", options(false, true));
    assert!(matches!(
        job.validate(),
        Err(ExportError::PaginationNotSupported { .. })
    ));
}

#[test]
fn empty_theme_is_rejected() {
    let job = job(
        ExportFormat::Html,
        "/tmp/a.html",
        ExportOptions::new(false, true, "  ", Default::default(), false),
    );
    assert!(matches!(job.validate(), Err(ExportError::EmptyTheme)));
}

#[test]
fn destination_without_extension_is_rejected() {
    let job = job(ExportFormat::Html, "/tmp/a", options(false, false));
    assert!(matches!(
        job.validate(),
        Err(ExportError::ExtensionMismatch { .. })
    ));
}

#[test]
fn needs_toc_reflects_format_support() {
    let html = job(ExportFormat::Html, "/tmp/a.html", options(true, false));
    let markdown = job(ExportFormat::Markdown, "/tmp/a.md", options(true, false));
    assert!(html.needs_toc());
    assert!(!markdown.needs_toc(), "不支持 TOC 的格式即使勾选也不生成");
}

// ---------- TOC 转义（原型缺陷 2.4） ----------

#[test]
fn toc_entry_marks_escape_for_html_sensitive_title() {
    let entry = TocEntry::new(2, "a<b & c\"d'e");
    assert!(entry.needs_escape());
    assert_eq!(entry.level(), 2);
}

#[test]
fn toc_entry_plain_title_needs_no_escape() {
    let entry = TocEntry::new(1, "普通标题");
    assert!(!entry.needs_escape());
}

#[test]
fn toc_entry_generates_slug_anchor() {
    let entry = TocEntry::new(1, "Hello World!");
    assert_eq!(entry.anchor(), "hello-world");
}

#[test]
fn toc_entry_anchor_falls_back_for_symbols_only() {
    let entry = TocEntry::new(1, "!!!");
    assert_eq!(entry.anchor(), "section");
}

#[test]
fn toc_builder_builds_entries() {
    let entries = TocBuilder::build(&[(1, "A"), (2, "B")]);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].level(), 2);
    assert_eq!(entries[1].title(), "B");
}

#[test]
fn toc_builder_verify_passes_for_constructed_entries() {
    let entries = TocBuilder::build(&[(1, "a<b"), (2, "plain")]);
    assert!(TocBuilder::verify(&entries).is_ok());
}

#[test]
fn toc_entry_marker_matches_content_for_every_sensitive_char() {
    // 五个敏感字符逐一验证：只要出现就必须标记 `needs_escape`。
    for sensitive in ['<', '>', '&', '"', '\''] {
        let title = format!("标题{sensitive}x");
        let entry = TocEntry::new(1, title.clone());
        assert!(entry.needs_escape(), "{title} 应标记为需要转义");
        assert!(TocBuilder::verify(&[entry]).is_ok());
    }
    assert!(TocBuilder::verify(&[TocEntry::new(1, "plain")]).is_ok());
}

#[test]
fn escape_title_produces_html_entities() {
    let entry = TocEntry::new(1, "a<b & c\"d'e");
    assert_eq!(
        TocBuilder::escape_title(&entry),
        "a&lt;b &amp; c&quot;d&#39;e"
    );
}

#[test]
fn job_with_toc_validation_runs_both_checks() {
    let valid = job(ExportFormat::Html, "/tmp/a.html", options(true, false));
    let entries = TocBuilder::build(&[(1, "标题"), (2, "a<b")]);
    assert!(valid.validate_with_toc(&entries).is_ok());

    let bad_job = job(ExportFormat::Html, "/tmp/a.md", options(true, false));
    assert!(bad_job.validate_with_toc(&entries).is_err());
}

// ---------- 状态机 ----------

#[test]
fn job_status_transitions() {
    let mut successful = job(ExportFormat::Html, "/tmp/a.html", options(false, false));
    assert_eq!(successful.status(), &ExportStatus::Pending);
    assert!(!successful.status().is_finished());

    successful.start();
    assert_eq!(successful.status(), &ExportStatus::Running);
    successful.start();
    assert_eq!(
        successful.status(),
        &ExportStatus::Running,
        "重复 start 应幂等"
    );

    successful.succeed();
    assert!(successful.status().is_finished());

    let mut failing = job(ExportFormat::Html, "/tmp/b.html", options(false, false));
    failing.fail("磁盘不可写");
    assert!(matches!(failing.status(), ExportStatus::Failed { .. }));
}

#[test]
fn export_error_mapping_keeps_io_separate() {
    use rustmark::shared::error::AppError;
    let config_error: AppError = ExportError::TocRequiresPagination.into();
    assert!(matches!(config_error, AppError::Export { .. }));
    assert!(ExportError::TocRequiresPagination.is_configuration_error());

    let io_error: AppError = ExportError::io_unavailable("no printer").into();
    assert!(matches!(io_error, AppError::Io { .. }));
    assert!(!ExportError::io_unavailable("x").is_configuration_error());
}
