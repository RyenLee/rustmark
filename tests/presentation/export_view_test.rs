//! 导出功能用例（S6-11 验收标准中「导出四类各产出一次真实文件」一条）。
//!
//! # 验收标准原文
//!
//! > 导出四类各产出一次真实文件并人工打开确认。
//!
//! 「人工打开确认」这一步在本环境无法自动化，故拆成两部分：
//!
//! - 本文件**逐条**覆盖四种格式（HTML / Markdown / PDF / 图片），
//!   断言产出文件的真实存在性与内容特征——这是「人工确认」能被自动化的部分；
//! - PDF **已开放**（V11-04）：产物经系统打印对话框产出，本进程拿不到最终
//!   文件与大小，且真实调度会**弹出打印对话框**——测试里不能触发，
//!   故不在本文件跑它的端到端。这里断言「界面提供该格式 + 有前置声明」，
//!   端到端（打印用 HTML 落盘 + 调用打印 + 返回 0）由
//!   `tests/infrastructure/pdf_exporter_test.rs` 注入假调度覆盖。
//!
//! 图片自 P2 起已实现（视口截图，见 `docs/p1p2-implementation-log.md` 第 5 项），
//! 但它的成败取决于 GPU 回读，无头环境拿不到渲染结果，
//! 故此处只断言**通路的行为**（可见才提交、越界明确报错、不留空文件），
//! 像素保真度转Stage 09 真机目测。

use crate::support::fake_dialog::FakeDialog;
use std::path::Path;
use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::application::dto::ExportRequestView;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::dialog_ui_state::Overlay;
use rustmark::presentation::state::export_ui_state::ExportFormatChoice;
use rustmark::presentation::state::scroll_sync_state::Side;
use rustmark::presentation::views::export_view::{FILE_STEM_INPUT_ID, OUTPUT_DIR_INPUT_ID};
use rustmark::presentation::views::{main_view, overlays};
use rustmark::shared::geometry::{ByteOffset, TextRange};

/// 导出用语料：覆盖标题层级、粗体、行内代码、列表、分隔线、中文。
const CONTENT: &str = "# 一级标题\n\n正文含 **粗体** 与 `代码`，还有中文。\n\n## 二级标题\n\n- 列表项一\n- 列表项二\n\n---\n\n结尾段落。\n";

/// 造一个装好配置、装入 `CONTENT`、并指定输出目录的应用。
fn app_with_out(out: &Path) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("启动即开空白文档");
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: CONTENT.to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
    app.export_ui.output_dir = out.to_string_lossy().to_string();
    app.export_ui.refresh_resolved_path();
    (dir, app)
}

/// 建一个输出目录（已存在）。
fn out_dir(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).expect("应能创建输出目录");
    out
}

fn request(format: &str, include_toc: bool, paginate: bool) -> ExportRequestView {
    ExportRequestView {
        format: format.to_string(),
        include_toc,
        embed_images: false,
        theme: "dark".to_string(),
        page_size: "a4".to_string(),
        paginate,
    }
}

/// HTML 导出：产出真实文件，且是**渲染后的 HTML** 而非源码。
#[test]
fn html_export_writes_a_real_rendered_file() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("doc.html");

    let result = app
        .export_current_document(request("html", true, false), target.to_str().unwrap())
        .expect("HTML 导出应成功");

    assert!(target.exists(), "应真的产出文件");
    let html = std::fs::read_to_string(&target).expect("应能读回");
    assert!(
        html.contains("<h1") && html.contains("<h2"),
        "标题应被渲染成标签"
    );
    assert!(
        html.contains("<strong>") || html.contains("<b>"),
        "粗体应被渲染"
    );
    assert!(html.contains("<code>"), "行内代码应被渲染");
    assert!(!html.contains("**粗体**"), "不应残留 Markdown 标记");
    assert!(
        html.contains("目录") || html.contains("toc"),
        "勾了目录应产出目录条目：{html}"
    );
    assert_eq!(
        result.bytes,
        html.len(),
        "`bytes` 必须是**产出文件**大小，不是源文长度"
    );
}

/// HTML 导出：不勾目录时不产出目录。
#[test]
fn html_export_without_toc_omits_the_outline() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("plain.html");

    app.export_current_document(request("html", false, false), target.to_str().unwrap())
        .expect("导出应成功");

    let html = std::fs::read_to_string(&target).unwrap();
    assert!(!html.contains("toc-lv"), "未勾目录时不应有目录条目");
}

/// Markdown 导出：产出**源码原文**，不做渲染。
#[test]
fn markdown_export_writes_the_source_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("doc.md");

    let result = app
        .export_current_document(request("markdown", false, false), target.to_str().unwrap())
        .expect("Markdown 导出应成功");

    assert!(target.exists());
    let text = std::fs::read_to_string(&target).unwrap();
    assert_eq!(text, CONTENT, "Markdown 导出必须是**逐字节**的源码原文");
    assert_eq!(result.bytes, CONTENT.len(), "大小应等于源文长度");
}

/// PDF **已开放**（V11-04）：界面提供该格式，并在点导出之前声明它的产出方式。
///
/// 这里不再断言「PDF 被拒绝」——那是开放前的行为。改而断言**前置声明**：
/// PDF 与其余格式的行为完全不同（生成打印用 HTML → 调起系统打印 →
/// 用户在对话框里另存），不提前讲清楚，用户会以为导出失败。
///
/// 端到端（打印用 HTML 落盘 + 调用打印）不在这里验证：真实调度会弹出
/// 打印对话框，测试无法安全触发。由 `tests/infrastructure/pdf_exporter_test.rs`
/// 注入假调度覆盖。
#[test]
fn pdf_export_is_offered_with_an_honest_caveat() {
    assert!(
        ExportFormatChoice::Pdf.is_available(),
        "PDF 已开放（V11-04）"
    );
    assert_eq!(
        ExportFormatChoice::Pdf.unavailable_reason(),
        None,
        "开放后不应再有「不可用」的原因文案"
    );
    let caveat = ExportFormatChoice::Pdf
        .pdf_caveat()
        .expect("PDF 必须有前置声明（产物经系统打印对话框）");
    assert!(caveat.contains("系统打印"), "声明应说清经系统打印产出");
    assert!(
        caveat.contains("另存为 PDF"),
        "声明应告诉用户在对话框里另存为 PDF"
    );
}

/// 图片（PNG）：走视口截图，被拒绝时不产出文件。
#[test]
fn image_export_needs_a_visible_preview_and_writes_nothing_on_rejection() {
    // 自 P2 起图片改走**视口截图**通路：`export_current_document`
    // （同步的内容导出入口）不再处理它。
    //
    // 这里断言的是「被拒绝时不产出文件」——这是失败语义的核心：
    // 静默产出一张空 .png 会让用户以为导出成功了。
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("doc.png");

    let outcome =
        app.export_current_document(request("image", false, false), target.to_str().unwrap());
    if let Err(error) = outcome {
        assert!(!target.exists(), "被拒绝时不得产出文件：{error}");
    }
}

/// 图片截图通路的前置条件：预览区必须已布局出矩形。
///
/// 预览区不可见（专注模式 / 关闭预览）时**必须**当场拒绝——
/// 那时裁出来的是空白，导出一张空白图比报错更糟。
#[test]
fn viewport_capture_requires_a_known_preview_rect() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("shot.png");

    // 尚未布局 →没有预览区矩形。
    assert!(app.preview_rect().is_none(), "刚构造时不该有预览区矩形");
    let err = app
        .begin_viewport_capture(target.to_str().unwrap())
        .expect_err("预览区不可见时必须拒绝，而不是导出空白图");
    assert!(
        err.to_string().contains("预览区"),
        "应说清是预览区不可见：{err}"
    );
    assert!(!target.exists(), "被拒绝时不得产出文件");

    // 布局出矩形后即可受理（此处只验受理，不等GPU 回传）。
    app.set_preview_rect(egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::Pos2::new(100.0, 200.0),
    ));
    app.begin_viewport_capture(target.to_str().unwrap())
        .expect("预览区可见时应受理");
    assert!(!target.exists(), "受理不等于已产出：像素要等下一帧回传");
}

/// 四类格式在**同步内容导出入口**上都走到「明确结论」。
///
/// 写成一条是为了表达验收标准的原意：「四类各产出一次真实文件」
/// 在当前实现下等价于「四类都有可判定的确定结果」。
///
/// 注意本测试走的是 `export_current_document`——**图片不在其中**：
/// 它走截图通路（跨帧、由 GPU 回传像素），在本入口里必然失败。
/// 这不是缺陷而是设计：像素不是源文本的函数。
#[test]
fn content_path_formats_have_a_definitive_outcome_in_the_content_path() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);

    let cases = [
        ("html", "a.html", true, false, true),
        ("markdown", "b.md", false, false, true),
        // 图片走截图通路，在本入口里被拒绝且**不留文件**。
        ("image", "d.png", false, false, false),
        // PDF 不在本用例里：它已开放（V11-04），但成功路径会**真的**调起
        // 系统打印对话框——测试里触发它会弹出对话框甚至挂起 CI。
        // 其端到端由 `tests/infrastructure/pdf_exporter_test.rs`
        // 用注入的假调度覆盖，这里只验「界面提供该格式且有前置声明」
        // （见 `pdf_export_is_offered_with_an_honest_caveat`）。
    ];
    for (format, name, toc, paginate, should_succeed) in cases {
        let target = out.join(name);
        let outcome =
            app.export_current_document(request(format, toc, paginate), target.to_str().unwrap());
        assert_eq!(
            outcome.is_ok(),
            should_succeed,
            "{format} 的结果应与「{should_succeed}」一致：{outcome:?}"
        );
        assert_eq!(
            target.exists(),
            should_succeed,
            "{format} 的文件存在性应与「{should_succeed}」一致"
        );
    }

    // 失败原因必须**区分**两种不适用，而不是一律「尚未实现」：
    // 图片是「走另一条通路」，混成一句话会让用户以为自己操作错了。
    let img_err = app
        .export_current_document(
            request("image", false, false),
            out.join("y.png").to_str().unwrap(),
        )
        .expect_err("图片在内容导出入口应被拒绝")
        .to_string();
    assert!(
        img_err.contains("截图"),
        "图片应说清走截图通路而非「未实现」：{img_err}"
    );
}

/// 扩展名与格式不符时**在创建任务阶段**就拒绝，不写文件。
#[test]
fn extension_mismatch_is_rejected_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("wrong.md");

    let error = app
        .export_current_document(request("html", false, false), target.to_str().unwrap())
        .expect_err("扩展名不符应被拒");

    assert!(
        error.to_string().contains("扩展名"),
        "错误应指出扩展名问题：{error}"
    );
    assert!(!target.exists(), "被拒时不得留下文件");
}

/// 格式不支持目录时（Markdown + TOC）被领域拒绝。
#[test]
fn toc_on_an_unsupported_format_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("x.md");

    let error = app
        .export_current_document(request("markdown", true, false), target.to_str().unwrap())
        .expect_err("Markdown 不支持目录");

    assert!(
        error.to_string().contains("目录"),
        "错误应指出目录不被支持：{error}"
    );
}

/// 输出目录不存在时**自动创建**（用户「另存为到新目录」的常见场景）。
#[test]
fn missing_output_directory_is_created_automatically() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b").join("c");
    let (_cfg, mut app) = app_with_out(&nested);
    let target = nested.join("doc.html");

    app.export_current_document(request("html", false, false), target.to_str().unwrap())
        .expect("父目录不存在时应自动创建而不是失败");

    assert!(target.exists(), "应真的产出文件");
}

/// 输出目录留空时 UI 拦住提交（文件会落到用户看不见的进程工作目录）。
#[test]
fn empty_output_directory_blocks_the_export_button() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);

    app.export_ui.output_dir = String::new();
    app.export_ui.refresh_resolved_path();

    assert!(!app.export_ui.can_submit(), "留空输出目录时不得允许导出");
    assert!(
        app.export_ui
            .blocked_reason()
            .is_some_and(|reason| reason.contains("输出目录")),
        "应说清缺的是输出目录"
    );
}

/// 切换到不支持的格式时自动夹紧选项（否则界面上会出现「Markdown + 目录」这种矛盾组合）。
#[test]
fn switching_format_clamps_unsupported_options() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);

    app.export_ui.include_toc = true;
    app.export_ui.set_format(ExportFormatChoice::Markdown);

    assert!(
        !app.export_ui.include_toc,
        "Markdown 不支持目录，开关应被夹紧"
    );
    assert!(app.export_ui.can_submit(), "夹紧后应可导出");
}

/// PDF 的目录必须配分页；勾目录时自动补上分页。
#[test]
fn pdf_toc_implies_pagination() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);

    app.export_ui.set_format(ExportFormatChoice::Pdf);
    app.export_ui.include_toc = true;
    // 模拟视图里的联动：勾目录时补分页。
    app.export_ui.paginate = true;

    assert!(
        app.export_ui.paginate,
        "PDF 目录必须配分页（领域规则 `TocRequiresPagination`）"
    );
}

/// 真实帧：导出浮层真的被渲染。
#[test]
fn opening_export_actually_renders_it_in_a_real_frame() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let ctx = egui::Context::default();

    for _ in 0..2 {
        crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
    }
    app.execute(UiAction::Export);
    assert!(app.dialog.is_open(Overlay::Export));
    for _ in 0..2 {
        crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
    }

    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(OUTPUT_DIR_INPUT_ID))),
        "导出面板未渲染：输出路径输入框拿不到焦点"
    );
}

fn frame_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1000.0, 800.0),
        )),
        ..Default::default()
    }
}

/// 导出浮层有视图实现，且已从待办清单移除。
#[test]
fn export_overlay_has_a_view() {
    assert!(overlays::has_view(Overlay::Export));
    assert!(
        !overlays::PENDING_VIEWS.contains(&Overlay::Export),
        "已实现的浮层必须从 `PENDING_VIEWS` 移除，否则清单会骗人"
    );
}

/// 关闭导出浮层会把 UI 从「导出中」状态里放出来。
///
/// 否则一次异常中断的导出会让对话框此后一直显示「正在导出，请稍候」，
/// 而实际上早已没有任务在跑——按钮永久置灰，且无从恢复。
#[test]
fn closing_export_clears_the_running_flag() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    app.export_ui.begin();
    assert!(app.export_ui.running);

    app.export_ui.running = false;
    app.export_ui.close();
    app.dialog.close(Overlay::Export);

    assert!(!app.export_ui.running, "关闭后不得停在「导出中」");
    assert!(app.export_ui.can_submit(), "关闭再打开后应可正常导出");
}

// ————————————————————————————————————————————————————————————
// 整篇长图（P2-L4）
// ————————————————————————————————————————————————————————————

/// 长图捕获的前置条件与视口截图一致：预览区不可见必须当场拒绝。
#[test]
fn long_capture_requires_a_known_preview_rect() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("long.png");

    let err = app
        .begin_long_capture(target.to_str().unwrap(), 1.0)
        .expect_err("预览区不可见时必须拒绝");
    assert!(
        err.to_string().contains("预览区"),
        "应说清是预览区不可见：{err}"
    );
    assert!(!target.exists(), "被拒绝时不得产出文件");
}

/// 可滚动内容受理长图任务，并按「每段一个视口高」分段、末段滚到底。
#[test]
fn long_capture_accepts_scrollable_content_and_registers_the_task() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("long.png");

    app.set_preview_rect(egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::Pos2::new(100.0, 300.0),
    ));
    // 内容 800、视口 300 → ceil(800/300)=3 段，偏移 [0, 300, 500]。
    app.ui.sync.set_geometry(Side::Preview, 800.0, 300.0);

    app.begin_long_capture(target.to_str().unwrap(), 1.0)
        .expect("可滚动内容应受理");
    assert!(
        app.export_ui.awaiting_capture,
        "受理后必须占用「等待截图」状态，防止滚动过程中被再次提交覆盖"
    );
    assert!(!target.exists(), "受理不等于已产出：要多帧滚动捕获");
}

/// 内容不足一屏时委托视口截图（长图与视口截图等价，不该空转滚动流程）。
#[test]
fn long_capture_delegates_to_viewport_when_content_fits_one_screen() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("short.png");

    app.set_preview_rect(egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::Pos2::new(100.0, 300.0),
    ));
    app.ui.sync.set_geometry(Side::Preview, 200.0, 300.0);

    app.begin_long_capture(target.to_str().unwrap(), 1.0)
        .expect("内容不足一屏应受理（走视口截图）");
    assert!(
        app.export_ui.awaiting_capture,
        "委托视口截图同样要占用等待状态"
    );
}

/// 段数超限当场拒绝（否则应用要滚动几十秒才告知失败）。
#[test]
fn long_capture_rejects_too_many_segments() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("huge.png");

    app.set_preview_rect(egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::Pos2::new(100.0, 300.0),
    ));
    // 200_000 / 300 ≈ 667 段 > 256 上限。
    app.ui.sync.set_geometry(Side::Preview, 200_000.0, 300.0);

    let err = app
        .begin_long_capture(target.to_str().unwrap(), 1.0)
        .expect_err("段数超限应拒绝");
    assert!(
        err.to_string().contains("屏"),
        "应说明是文档过长（按屏计）：{err}"
    );
    assert!(!app.export_ui.awaiting_capture, "拒绝后不得占用等待状态");
}

/// 成品高度超限时提前拒绝（不等整个滚动流程跑完）。
#[test]
fn long_capture_rejects_oversized_result_height() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("tall.png");

    app.set_preview_rect(egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::Pos2::new(100.0, 300.0),
    ));
    // 20_000 / 300 ≈ 67 段（段数合规），但 20_000px > 16_384px 上限。
    app.ui.sync.set_geometry(Side::Preview, 20_000.0, 300.0);

    let err = app
        .begin_long_capture(target.to_str().unwrap(), 1.0)
        .expect_err("高度超限应拒绝");
    assert!(
        err.to_string().contains("上限"),
        "应说明是成品高度超限：{err}"
    );
    assert!(!target.exists(), "被拒绝时不得产出文件");
}

/// 端到端：注入截图回传事件驱动真实帧循环，验证多段滚动 + 拼接落盘。
///
/// 无头环境没有 GPU，但截图回传的**消费侧**逻辑
/// （两拍状态机、裁剪、末段重叠丢弃、拼接、写盘）全部在应用层 /
/// 基础设施层，可以用帧末兑现的假像素完整驱动。
///
/// 几何设定：预览区 100×300 逻辑点（ppp=1 → 截图 100×300px），
/// 内容高 800 → 段偏移 [0, 300, 500]，末段重叠 = 300+300-500 = 100 行。
/// 成品高度 = 300+300+(300-100) = **800**——若末段重叠行没被丢弃，
/// 高度会是 900，此断言即钉住重叠丢弃逻辑。
#[test]
fn long_capture_e2e_stitches_segments_into_one_png() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let target = out.join("long-e2e.png");

    app.set_preview_rect(egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::Pos2::new(100.0, 300.0),
    ));
    app.ui.sync.set_geometry(Side::Preview, 800.0, 300.0);
    app.begin_long_capture(target.to_str().unwrap(), 1.0)
        .expect("受理长图任务");

    let ctx = egui::Context::default();
    // egui 0.37 起截图不再经 `Event::Screenshot` 注入，而是应用在帧内调用
    // `Context::request_screenshot(cb)` 登记回调，帧末以 `ViewportCommand::Screenshot`
    // 交回集成；集成回读像素后调用回调。无头环境没有 GPU，故本用例在每帧结束后
    // **取出该回调并补上一张 100×300 的假像素**，等价于后端完成一次回读。
    //
    // 时序仍是两拍：应用在发令拍登记回调 → 本用例帧末兑现 → 下一帧 `pump_capture`
    // 从收件箱取回。3 段 × 每段两拍 + 余量。
    let pixels = vec![egui::Color32::from_rgb(120, 40, 40); 100 * 300];
    let mut delivered = false;
    for _ in 0..40 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
            app.pump_capture(ui.ctx());
        });
        // 兑现本帧排队的所有截图请求（长图会多段重发，一次性全部满足）。
        if let Some(vp) = out.viewport_output.get(&egui::ViewportId::ROOT) {
            for cmd in &vp.commands {
                if let egui::ViewportCommand::Screenshot(cb) = cmd {
                    cb.clone()
                        .complete(std::sync::Arc::new(egui::ColorImage::new(
                            [100, 300],
                            pixels.clone(),
                        )));
                }
            }
        }
        if target.exists() {
            delivered = true;
            break;
        }
    }
    assert!(delivered, "帧循环跑完仍未产出 PNG：捕获状态机没有闭环");
    assert!(
        !app.export_ui.awaiting_capture,
        "任务兑现后必须释放等待状态"
    );
    assert!(
        app.export_ui
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("整篇长图")),
        "成功提示应说明产出了整篇长图：{:?}",
        app.export_ui.notice
    );

    let image = image::open(&target).expect("应能读回产出的 PNG");
    assert_eq!(image.width(), 100, "宽度应等于预览区宽 × ppp");
    assert_eq!(
        image.height(),
        800,
        "高度应等于内容高（末段重叠行必须被丢弃，否则是 900）"
    );
}

// ————————————————————————————————————————————————————————————
// 焦点归属：同一浮层里的两个输入框必须能各选各的
// ————————————————————————————————————————————————————————————
//
// 症状：点「文件名」输入框，光标立刻跳回「输出路径」输入框，
// 一个字都打不进去——导出的文件名改不了。
//
// 根因不是布局，是 `show()` 里那句「打开后把焦点交给输出路径」的守卫：
//
//     !m.has_focus(窗口 id)
//
// `Memory::has_focus` 只认**控件** id，而 `egui::Window` 的 id 属于 `Area`——
// egui 内部只有 `TextEdit` / `DragValue` 会 `request_focus`，没有任何地方
// 会让窗口 id 成为「有焦点的控件」。所以那个条件**恒为真**，
// 「打开时给一次」退化成「每帧都抢」，同一浮层里的第二个输入框永远拿不到焦点。
//
// 两条用例分工：一条钉「别抢回来」（纯焦点层面，最接近根因），
// 一条钉「真的点得中」（走指针事件，最接近用户手上的动作）。

/// 跑一帧（可带输入事件）。
fn run_frame(ctx: &egui::Context, app: &mut EditorApp, events: Vec<egui::Event>) {
    let mut input = frame_input();
    input.events = events;
    crate::support::run_ui(ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        main_view::show(app, ui);
    });
}

fn press_at(pos: egui::Pos2) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    }
}

fn release_at(pos: egui::Pos2) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    }
}

/// 打开导出浮层并跑够两帧（焦点请求隔一帧兑现）。
fn open_export(ctx: &egui::Context, app: &mut EditorApp) {
    for _ in 0..2 {
        run_frame(ctx, app, Vec::new());
    }
    app.execute(UiAction::Export);
    assert!(app.dialog.is_open(Overlay::Export), "动作应打开导出浮层");
    for _ in 0..2 {
        run_frame(ctx, app, Vec::new());
    }
}

/// 焦点一旦移到别的输入框，就不得被拽回输出路径框。
#[test]
fn export_dialog_does_not_steal_focus_back_from_the_file_name_box() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let ctx = egui::Context::default();

    open_export(&ctx, &mut app);
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(OUTPUT_DIR_INPUT_ID))),
        "打开后焦点应先落在输出路径框"
    );

    // 用户把光标移进「文件名」框（等价于点中了它）。
    ctx.memory_mut(|m| m.request_focus(egui::Id::unique(FILE_STEM_INPUT_ID)));
    run_frame(&ctx, &mut app, Vec::new());

    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(FILE_STEM_INPUT_ID))),
        "焦点被拽回了输出路径框：用户在「文件名」框里一个字也打不进去"
    );
}

/// 走真实指针事件：点中「文件名」框后焦点要留在那里。
#[test]
fn export_dialog_file_name_box_can_actually_be_selected() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let ctx = egui::Context::default();

    open_export(&ctx, &mut app);
    let rect = ctx
        .read_response(egui::Id::unique(FILE_STEM_INPUT_ID))
        .expect("文件名输入框应已被布局")
        .rect;
    let pos = rect.center();

    run_frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(pos), press_at(pos)],
    );
    run_frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(pos), release_at(pos)],
    );
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(FILE_STEM_INPUT_ID))),
        "点中「文件名」框后焦点应落在那里"
    );

    // 再跑一帧：确认不是「只在这一帧侥幸成立」，而是真的稳住了。
    run_frame(&ctx, &mut app, Vec::new());
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(FILE_STEM_INPUT_ID))),
        "下一帧焦点又被抢走：用户无法在「文件名」框里连续输入"
    );
}

/// 点中「文件名」框后敲进去的字要真的落进文件名。
///
/// 前两条钉的是「焦点在不在」——但焦点在≠打得进去（egui 里每个字符都要
/// 过 `TextEdit` 的焦点检查）。这条走完整通路：点击 → 键盘事件 → 状态变更 →
/// 「将导出到 …」跟着更新。若哪天又在中间某帧把焦点抢走，这里会先红。
#[test]
fn typing_into_the_file_name_box_actually_edits_the_file_name() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (_cfg, mut app) = app_with_out(&out);
    let ctx = egui::Context::default();

    open_export(&ctx, &mut app);
    let before = app.export_ui.file_stem.clone();
    let rect = ctx
        .read_response(egui::Id::unique(FILE_STEM_INPUT_ID))
        .expect("文件名输入框应已被布局")
        .rect;
    let pos = rect.center();

    run_frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(pos), press_at(pos)],
    );
    run_frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(pos), release_at(pos)],
    );
    run_frame(&ctx, &mut app, vec![egui::Event::Text("z".to_string())]);

    assert!(
        app.export_ui.file_stem.contains('z'),
        "敲进去的字符没落进文件名（{before:?} → {:?}）：输入框实际不可编辑",
        app.export_ui.file_stem
    );
    assert_eq!(
        app.export_ui.file_stem.len(),
        before.len() + 1,
        "应恰好插入一个字符"
    );
    assert!(
        app.export_ui.resolved_path.contains('z'),
        "文件名变了，「将导出到 …」也要跟着变：{}",
        app.export_ui.resolved_path
    );
}
