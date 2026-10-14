//! 侧栏「检查」标签的用例（V11-08）。
//!
//! # 这一层要验的是「接线」，不是「规则」
//!
//! 规则的判定正确性已由 `domain::markdown::lint` 的 15 条单测覆盖，
//! 应用层的规则开关过滤由 `markdown_service_test.rs` 的 lint 用例覆盖。
//! 本文件只钉住**最后一段接线**：
//!
//! 1. 当前文档的内容真的被送进了 linter（而不是永远返回空集）；
//! 2. 设置页关掉规则后，侧栏的结果随之消失（开关真的通了）；
//! 3. 点击问题真的会把光标移到对应的行（跳转真的接了）。

use std::sync::Arc;

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::presentation::state::app_ui_state::SidebarTab;
use rustmark::presentation::views::main_view;
use rustmark::shared::geometry::{ByteOffset, TextRange};

// ============================================================================
// 装置
// ============================================================================

fn app_with(content: &str) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("应能开出空白文档");
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: content.to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
    (dir, app)
}

/// 跑若干帧真实渲染，切到「检查」标签。
fn render_lint_tab(app: &mut EditorApp) -> usize {
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Lint;
    app.ui.sidebar_visible = true;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1000.0, 700.0),
        )),
        ..Default::default()
    };
    let mut shapes = 0;
    for _ in 0..2 {
        let out = ctx.run(input.clone(), |ctx| {
            rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
            main_view::show(app, ctx);
        });
        shapes = out.shapes.len();
    }
    shapes
}

/// 一份含多类问题的文档。
const BAD_DOC: &str = "# 一级标题\n\n### 跳到三级 \n\n- 甲\n* 乙\n\n```rust\nfn main() {}\n";

// ============================================================================
// 用例
// ============================================================================

/// 当前文档的内容要真的被送进 linter（否则「检查」标签永远空白）。
#[test]
fn lint_reads_the_active_document() {
    let (_dir, app) = app_with(BAD_DOC);
    let issues = app.lint_current_document();
    assert!(
        !issues.is_empty(),
        "当前文档有明显格式问题，检查结果不该为空"
    );
    assert!(
        issues.iter().any(|i| i.rule == "heading_jump"),
        "应至少报出标题层级跳跃，实际 {issues:?}"
    );
}

/// 没有文档时结果是空集（而不是 panic）。
#[test]
fn lint_is_empty_without_a_document() {
    let dir = tempfile::tempdir().expect("配置目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let app = EditorApp::new(services, false);
    assert!(app.lint_current_document().is_empty());
}

/// 设置页关掉某条规则后，侧栏结果随之中消失——开关真的通了。
#[test]
fn disabling_a_rule_removes_its_issues() {
    let (_dir, mut app) = app_with(BAD_DOC);
    assert!(
        app.lint_current_document()
            .iter()
            .any(|i| i.rule == "list_marker_mixed"),
        "前置条件：默认应报出列表符号混用"
    );
    app.apply_editor_field(|e| e.lint.list_marker_mix = false);
    assert!(
        !app.lint_current_document()
            .iter()
            .any(|i| i.rule == "list_marker_mixed"),
        "关掉规则后不该再报出该类问题"
    );
    // 设置里也确实记下了。
    assert!(!app.settings().editor.lint.list_marker_mix);
}

/// 全部规则关闭 → 空集（应用层短路）。
#[test]
fn disabling_every_rule_yields_no_issues() {
    let (_dir, mut app) = app_with(BAD_DOC);
    app.apply_editor_field(|e| {
        e.lint.heading_jump = false;
        e.lint.list_marker_mix = false;
        e.lint.trailing_whitespace = false;
        e.lint.unclosed_fence = false;
    });
    assert!(app.lint_current_document().is_empty());
}

/// 「检查」标签能在真实帧里渲染（不是只过纯函数）。
#[test]
fn lint_tab_renders_in_a_real_frame() {
    let (_dir, mut app) = app_with(BAD_DOC);
    let shapes = render_lint_tab(&mut app);
    assert!(shapes > 0, "「检查」标签必须画出东西，实际 {shapes} 个形状");
}

/// 切到「检查」后，编辑区必须紧贴侧栏右缘（缺陷 024 回归钉）。
///
/// 缺陷根因：lint 行的说明文字在 `Layout::left_to_right` 里用裸 `label`，
/// **不换行**、溢出面板内容区（内容区右界 232pt，实测溢到 407.9pt）；而
/// egui `SidePanel` 给后续布局保留的是**内容 response.rect 的最大 x**
/// （`panel.rs` 的 `allocate_left_panel`），侧栏因此实际占掉 [0, 415.9]，
/// [240, 415.9] 之间没有任何面板绘制——真实渲染下露出交换链清屏色（黑），
/// 编辑区被整体推右。用户看到的是「侧栏与编辑区之间一条竖向黑框」。
///
/// 断言方式：渲染真实帧，找编辑区自己画的背景大矩形（`bg_primary` 填充、
/// 位于顶栏之下），其左缘必须落在侧栏右缘（240）附近；被推右即回归。
#[test]
fn lint_tab_does_not_push_the_editor_right() {
    let (_dir, mut app) = app_with(BAD_DOC);
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Lint;
    app.ui.sidebar_visible = true;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1000.0, 700.0),
        )),
        ..Default::default()
    };
    let sidebar_right = app.theme().metrics.sidebar_width;
    let mut editor_bg_left: Option<f32> = None;
    for _ in 0..2 {
        let out = ctx.run(input.clone(), |ctx| {
            rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
            main_view::show(&mut app, ctx);
        });
        for clipped in &out.shapes {
            if let egui::Shape::Rect(r) = &clipped.shape {
                // 编辑区背景：大面积 `bg_primary`、顶栏之下、内容区起点附近。
                let area = r.rect.width() * r.rect.height();
                if r.fill == app.theme().bg_primary
                    && area > 20_000.0
                    && r.rect.min.y > 90.0
                    && r.rect.min.x > sidebar_right
                {
                    editor_bg_left = Some(editor_bg_left.map_or(r.rect.min.x, |l| l.min(l)));
                }
            }
        }
    }
    let left = editor_bg_left.expect("编辑区背景矩形应存在于真实帧中");
    assert!(
        left <= sidebar_right + 80.0,
        "编辑区背景左缘 {left} 距侧栏右缘 {sidebar_right} 过远：lint 内容把 \
         侧栏面板撑宽了（缺陷 024 回归），两者之间会出现无人绘制的黑框"
    );
}

/// 跳转：点一条问题后光标应落到该行首。
#[test]
fn jumping_to_an_issue_moves_the_cursor_to_that_line() {
    let (_dir, mut app) = app_with(BAD_DOC);
    let issue = app
        .lint_current_document()
        .into_iter()
        .find(|i| i.rule == "heading_jump")
        .expect("应有标题跳跃问题");
    // 走与点击同一条通路。
    app.goto_outline_line(issue.line);
    // 第三行（0-based 2）是 `### 跳到三级 `，行首字节偏移 = 前缀长度。
    let expected = "# 一级标题\n\n".len();
    assert_eq!(
        app.editor.selection(),
        Some((expected, expected)),
        "光标应落在问题所在行的行首"
    );
}
