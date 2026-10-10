//! 按格式路由的导出器用例（S6-01 装配件）。

use std::path::PathBuf;

use rustmark::domain::export::aggregate::ExportJob;
use rustmark::domain::export::services::Exporter;
use rustmark::domain::export::value_objects::{ExportFormat, ExportOptions};
use rustmark::infrastructure::export::routing_exporter::RoutingExporter;
use rustmark::shared::ids::{DocumentId, JobId};
use rustmark::shared::path::FilePath;

/// 造一个指向临时目录的导出任务。
fn job_for(dir: &tempfile::TempDir, name: &str, format: ExportFormat) -> (ExportJob, PathBuf) {
    let target = dir.path().join(name);
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        format,
        ExportOptions::default_options(),
    );
    (job, target)
}

#[test]
fn html_format_produces_real_file() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let (job, target) = job_for(&dir, "out.html", ExportFormat::Html);
    RoutingExporter::new()
        .export("# 标题\n\n正文。", &job)
        .expect("HTML 导出应成功");
    let written = std::fs::read_to_string(&target).expect("应产出文件");
    assert!(
        written.contains("<h1") && written.contains("正文"),
        "导出的 HTML 应含真实内容，实际长度 {}",
        written.len()
    );
}

#[test]
fn markdown_format_copies_verbatim() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let (job, target) = job_for(&dir, "out.md", ExportFormat::Markdown);
    let source = "# 原样保留\n\n`不转义`";
    RoutingExporter::new()
        .export(source, &job)
        .expect("Markdown 导出应成功");
    assert_eq!(
        std::fs::read_to_string(&target).expect("应产出文件"),
        source,
        "Markdown 导出必须是原文拷贝"
    );
}

#[test]
fn image_format_fails_loudly_instead_of_silently_succeeding() {
    // 图片自 P2 起已可用，但走的是**视口截图**通路：像素来自 GPU 回读，
    // 内容导出器拿不到渲染上下文。故本路由器仍必须对它明确报错——
    // 若静默成功，用户会拿到一张内容为空的 .png 却毫无察觉。
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let (job, target) = job_for(&dir, "out.png", ExportFormat::Image);
    let err = RoutingExporter::new()
        .export("正文", &job)
        .expect_err("内容导出器不能处理图片，必须返回错误");
    let text = format!("{err}");
    assert!(
        text.contains("截图"),
        "错误信息应说清图片走截图通路而非内容导出，实际：{text}"
    );
    assert!(
        !target.exists(),
        "返回错误时绝不能留下空文件——用户会以为导出成功了"
    );
}

#[test]
fn routing_never_dispatches_to_wrong_exporter() {
    // 反向断言：HTML 任务不能被 PDF 导出器接走。
    // 两者的产物形态差异极大（HTML 是文本，PDF 是二进制），
    // 若路由写错，症状是「导出的 .html 打不开」，很难联想到路由表。
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let (job, target) = job_for(&dir, "routed.html", ExportFormat::Html);
    RoutingExporter::new().export("# ok", &job).expect("应成功");
    let bytes = std::fs::read(&target).expect("应产出文件");
    assert!(!bytes.starts_with(b"%PDF"), "HTML 任务绝不能产出 PDF 内容");
}
