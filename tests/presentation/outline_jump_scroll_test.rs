//! 大纲点击跳转的**滚动定位**回归（用户反馈：点目录项后编辑区没定位到
//! 标题所在位置）。
//!
//! # 钉住的不变量
//!
//! 点击大纲 / 检查 / 搜索结果条目后，编辑区必须把目标行滚到**视口顶部**
//! （贴顶留 `TARGET_MARGIN`）。两个反模式各有一条用例钉死：
//!
//! 1. **「目标行已在视口内就不动」**——查找「下一个」的防抖策略被套到
//!    离散跳转上：用户点目录，章节就落在眼前最下方，纹丝不动，看起来
//!    「点了没反应」；
//! 2. **「落到视口上部 1/4 处」**——同为查找策略：跳过去了，但标题停在
//!    视口四分之一处，与「标题顶部」差半屏，用户仍要自己找一眼。
//!
//! # 判定链（真帧，不读源码推断）
//!
//! 点击走真实 UI（从本帧形状取条目中心 → press/release 两帧），跳转后
//! 再渲染一帧，判据分两层：
//!
//! - **状态层**：`editor.scroll_y == clamp(line_tops[行] - TARGET_MARGIN)`；
//! - **渲染层**：编辑区文档 Galley（文本与全文一致的那个 `TextShape`）里
//!   该行的 `PlacedRow::rect` 落在视口顶 + `TARGET_MARGIN` 处。
//!   两层共用同一个 y 但走的是不同代码路径（状态是跳转写的，矩形是
//!   `ScrollArea` 渲染产物），同时过才算「真的定位到了」。
//!
//! # 附带钉住
//!
//! - `line_tops[行] == Galley 行 pos.y`（行顶表与渲染产物同源，含折行）；
//! - 目标靠近文档末尾时**夹紧**生效：滚不到顶也要把该行完整滚进视口；
//! - 光标仍落在标题行（只滚屏不错行没有意义）。

use std::sync::Arc;

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::presentation::state::app_ui_state::SidebarTab;
use rustmark::presentation::state::scroll_sync_state::Side;
use rustmark::presentation::views::main_view;
use rustmark::presentation::views::scroll_sync::{TARGET_MARGIN, clamp_scroll};
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

/// 10 章 × 6 段的长文档。
///
/// 两个硬约束：① 总高必须远超视口（否则「跳转」无从谈起）；② 每行都短
/// （不折行）——`Galley` 的 `PlacedRow` 与源码行号一一对应是本文件渲染层
/// 判据的前提，折行会让一行占多个 row。
fn jump_doc() -> String {
    let mut s = String::new();
    for chapter in 1..=10u32 {
        s.push_str(&format!("# 第{chapter}章标题\n\n"));
        for para in 1..=6u32 {
            s.push_str(&format!("第{chapter}章第{para}段，正文若干字。\n\n"));
        }
    }
    s
}

fn frame_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    }
}

/// 渲染一帧并回收形状（首帧是测量 pass，取画面一律先跑过它）。
fn render_frame(ctx: &egui::Context, app: &mut EditorApp) -> Vec<egui::epaint::ClippedShape> {
    crate::support::run_ui(ctx, frame_input(), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        main_view::show(app, ui);
    })
    .shapes
}

/// 在 `pos` 上模拟一次完整点击（移动 → 按下 → 释放在同一点）。
///
/// egui 的 `clicked()` 判定「按下与释落在同一控件」，只发 pressed 不触发；
/// 交互判定依赖上一帧已建立的布局，故每帧都重跑 `main_view::show`。
fn click_at(ctx: &egui::Context, app: &mut EditorApp, pos: egui::Pos2) {
    for pressed in [true, false] {
        let mut input = frame_input();
        input.events.push(egui::Event::PointerMoved(pos));
        input.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
        crate::support::run_ui(ctx, input, |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(app, ui);
        });
    }
}

/// 递归遍历形状里的全部文本。
///
/// 生命周期与输入绑定（而非 HRTB）：调用方要把 `&TextShape` 存起来
/// 跨闭包使用（如「找文档 Galley」），HRTB 版本会以 E0521 拒绝。
fn walk_texts<'a>(
    shapes: &'a [egui::epaint::ClippedShape],
    f: &mut impl FnMut(&'a egui::epaint::TextShape),
) {
    fn one<'a>(shape: &'a egui::epaint::Shape, f: &mut impl FnMut(&'a egui::epaint::TextShape)) {
        match shape {
            egui::epaint::Shape::Text(t) => f(t),
            egui::epaint::Shape::Vec(v) => {
                for s in v {
                    one(s, f);
                }
            }
            _ => {}
        }
    }
    for cs in shapes {
        one(&cs.shape, f);
    }
}

/// 侧栏区域内某段文字的中心点（点击落点）。
///
/// 只接受裁剪矩形贴在窗左、宽度不超过面板宽的形状，从而排除编辑区 /
/// 预览区里同名的正文（如大纲条目「第6章标题」与正文重名）。
fn sidebar_text_center(shapes: &[egui::epaint::ClippedShape], needle: &str) -> Option<egui::Pos2> {
    let mut found = None;
    for cs in shapes {
        let clip = cs.clip_rect;
        if clip.left() > 1.0 || clip.width() > 320.0 {
            continue;
        }
        walk_texts(
            std::slice::from_ref(cs),
            &mut |t: &egui::epaint::TextShape| {
                if found.is_none() && t.galley.text() == needle {
                    found = Some(egui::pos2(
                        t.pos.x + t.galley.size().x / 2.0,
                        t.pos.y + t.galley.size().y / 2.0,
                    ));
                }
            },
        );
    }
    found
}

/// 编辑区文档 Galley：文本与全文一致的那个 `TextShape`（自定义 layouter
/// 对整篇只排一次，故全文档只有一个这样的形状），连同它的裁剪矩形
/// （= 编辑区视口）。
fn editor_doc_galley<'a>(
    shapes: &'a [egui::epaint::ClippedShape],
    content: &str,
) -> (&'a egui::epaint::TextShape, egui::Rect) {
    for cs in shapes {
        let mut found = None;
        walk_texts(
            std::slice::from_ref(cs),
            &mut |t: &egui::epaint::TextShape| {
                if t.galley.text() == content {
                    found = Some(t);
                }
            },
        );
        if let Some(t) = found {
            return (t, cs.clip_rect);
        }
    }
    panic!("应渲染出与全文一致的编辑区 Galley（自定义 layouter 整篇只排一次）");
}

/// 源码第 `line` 行在**屏幕**上的矩形（Galley 行坐标 + 形状原点）。
///
/// 前提：该行不折行（本文件的文档每行都短，`PlacedRow` 与源码行一一对应）。
fn source_row_rect(text: &egui::epaint::TextShape, line: usize) -> egui::Rect {
    let origin = text.pos.to_vec2() + egui::vec2(text.galley.rect.left(), 0.0);
    text.galley.rows[line].rect().translate(origin)
}

/// 行顶表（稳定帧后必须可用）。
fn line_tops(app: &EditorApp, content: &str) -> Vec<f32> {
    rustmark::presentation::theme::mapper::line_tops(&app.theme().clone(), content)
        .expect("稳定帧后行顶表应可用（layouter 已跑过）")
}

// ============================================================================
// 用例
// ============================================================================

/// 主回归：点击大纲条目 → 目标标题被送到视口顶部。
#[test]
fn outline_jump_scrolls_the_heading_to_the_viewport_top() {
    let (_dir, mut app) = app_with(&jump_doc());
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Outline;
    app.ui.sidebar_visible = true;

    let mut shapes = Vec::new();
    for _ in 0..2 {
        shapes = render_frame(&ctx, &mut app);
    }

    let content = app.current_content().expect("应能读到内容");
    let items = app.current_outline();
    assert_eq!(items.len(), 10, "10 章应解析出 10 条大纲");
    // 第 6 章：初始位置远在首屏之下（跳转必然发生），且距文档末尾还有
    // 40+ 行（`clamp_scroll` 不会掺和进来，判据才是「贴顶」本身）。
    let target = items[5].clone();
    assert_eq!(target.text, "第6章标题");
    let tops = line_tops(&app, &content);

    // 行顶表与渲染产物同源：行号 ↔ `PlacedRow` 一一对应。
    let (doc_text, _clip) = editor_doc_galley(&shapes, &content);
    assert!(
        (tops[target.line] - doc_text.galley.rows[target.line].pos.y).abs() <= 0.5,
        "line_tops[{line}]={} 应与 Galley 行 pos.y={} 一致",
        tops[target.line],
        doc_text.galley.rows[target.line].pos.y,
        line = target.line,
    );
    assert!(
        doc_text.galley.rows[target.line]
            .text()
            .starts_with("# 第6章标题"),
        "第 {line} 行应就是「# 第6章标题」，实际 {:?}",
        doc_text.galley.rows[target.line].text(),
        line = target.line,
    );

    let pos = sidebar_text_center(&shapes, &target.text).expect("侧栏应渲染出「第6章标题」条目");
    click_at(&ctx, &mut app, pos);
    shapes = render_frame(&ctx, &mut app);

    // 判据一（状态层）：偏移 = 行顶 + 文本顶端偏移 − TARGET_MARGIN（夹紧后）。
    let geom = app.ui.sync.geometry(Side::Editor);
    let inset = app.ui.sync.editor_text_top();
    assert!(
        (inset - 4.0).abs() <= 0.5,
        "实测文本顶端偏移应为 TextEdit atom frame 的竖向内边距（space_xs=4pt），实际 {inset:.2}",
    );
    let expected = clamp_scroll(tops[target.line] + inset - TARGET_MARGIN, geom);
    assert!(
        (app.editor.scroll_y - expected).abs() <= 2.0,
        "点击大纲后 scroll_y 应等于 行顶+顶端偏移−TARGET_MARGIN：实际 {actual:.2}，期望 {expected:.2}\
         （行顶 {top:.2}，偏移 {inset:.2}，margin {TARGET_MARGIN}）",
        actual = app.editor.scroll_y,
        top = tops[target.line],
    );

    // 判据二（渲染层）：标题行矩形落在视口顶 + TARGET_MARGIN。
    let (doc_text, clip) = editor_doc_galley(&shapes, &content);
    let row = source_row_rect(doc_text, target.line);
    let off = row.min.y - clip.min.y;
    assert!(
        (off - TARGET_MARGIN).abs() <= 2.0,
        "标题行应贴在视口顶部（留 {TARGET_MARGIN}pt）：实际距视口顶 {off:.2}pt",
    );

    // 光标仍落在标题行：定位不能只滚屏不错行。
    let starts = rustmark::presentation::views::scroll_sync::line_start_offsets(&content);
    assert_eq!(
        app.editor.cursor, starts[target.line],
        "光标应仍落在「# 第6章标题」行首",
    );
}

/// 「标题已在视口内」也必须重排到顶部。
///
/// 这是用户反馈的主症状：`scroll_to_offset` 的「已在视口内就不动」分支
/// 源自查找「下一个」的防抖需求，被大纲跳转继承后，点击一个就在眼前
/// （但不在顶部）的章节毫无反应——用户视角就是「点了没定位」。
#[test]
fn outline_jump_realigns_a_heading_that_is_already_visible() {
    let (_dir, mut app) = app_with(&jump_doc());
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Outline;
    app.ui.sidebar_visible = true;

    // 本条只用稳定布局，不取稳定帧的画面（取的是预滚之后的）。
    for _ in 0..2 {
        render_frame(&ctx, &mut app);
    }

    let content = app.current_content().expect("应能读到内容");
    let items = app.current_outline();
    let target = items[4].clone();
    assert_eq!(target.text, "第5章标题");
    let tops = line_tops(&app, &content);

    // 预滚：目标行顶落在视口顶之下 80pt——可见、但明显不在顶部。
    // 注意 scroll_y 是**内容坐标**：要把内容里 tops+inset 处的行放到
    // 视口顶下 80pt，偏移应取 `tops + inset - 80`。
    app.editor.scroll_y = tops[target.line] + app.ui.sync.editor_text_top() - 80.0;
    let mut shapes = render_frame(&ctx, &mut app);

    // 前提自校验：此刻目标行确实「可见但不在顶部」，否则本条用例
    // 测的就不是「已可见」分支了。
    let (doc_text, clip) = editor_doc_galley(&shapes, &content);
    let row_y = source_row_rect(doc_text, target.line).min.y;
    assert!(
        row_y > clip.min.y + 30.0 && row_y < clip.max.y - 20.0,
        "前提：预滚后「第5章标题」应可见但不在顶部，实际 row_y={row_y:.2}，视口 [{:.2}, {:.2}]",
        clip.min.y,
        clip.max.y,
    );

    let pos = sidebar_text_center(&shapes, &target.text).expect("侧栏应渲染出「第5章标题」条目");
    click_at(&ctx, &mut app, pos);
    shapes = render_frame(&ctx, &mut app);

    let geom = app.ui.sync.geometry(Side::Editor);
    let expected = clamp_scroll(
        tops[target.line] + app.ui.sync.editor_text_top() - TARGET_MARGIN,
        geom,
    );
    assert!(
        (app.editor.scroll_y - expected).abs() <= 2.0,
        "已可见的标题被点击后仍应送到视口顶部：实际 scroll_y={actual:.2}，期望 {expected:.2}\
         （预滚位 {pre:.2}，若相等说明「已可见就不动」分支又回来了）",
        actual = app.editor.scroll_y,
        pre = tops[target.line] + app.ui.sync.editor_text_top() - 80.0,
    );

    let (doc_text, clip) = editor_doc_galley(&shapes, &content);
    let off = source_row_rect(doc_text, target.line).min.y - clip.min.y;
    assert!(
        (off - TARGET_MARGIN).abs() <= 2.0,
        "已可见的标题被点击后应贴在视口顶部：实际距视口顶 {off:.2}pt",
    );
}

/// 点最后一章：贴不到顶（文档到底了）也要把它完整滚进视口。
///
/// `clamp_scroll` 会把目标压到 `max_offset`——此时标题落在视口顶之下
/// 若干 pt 是几何必然，但「整行可见」必须仍成立（用户点目录最频繁的
/// 就是最后几章）。
#[test]
fn outline_jump_to_the_last_heading_keeps_it_fully_visible() {
    let (_dir, mut app) = app_with(&jump_doc());
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Outline;
    app.ui.sidebar_visible = true;

    let mut shapes = Vec::new();
    for _ in 0..2 {
        shapes = render_frame(&ctx, &mut app);
    }

    let content = app.current_content().expect("应能读到内容");
    let items = app.current_outline();
    let target = items.last().expect("应有大纲条目").clone();
    assert_eq!(target.text, "第10章标题");
    let tops = line_tops(&app, &content);

    let pos = sidebar_text_center(&shapes, &target.text).expect("侧栏应渲染出「第10章标题」条目");
    click_at(&ctx, &mut app, pos);
    shapes = render_frame(&ctx, &mut app);

    // 夹紧后的期望值（= max_offset，因为行顶−margin 超出可滚范围）。
    let geom = app.ui.sync.geometry(Side::Editor);
    let expected = clamp_scroll(
        tops[target.line] + app.ui.sync.editor_text_top() - TARGET_MARGIN,
        geom,
    );
    assert!(
        (app.editor.scroll_y - expected).abs() <= 2.0,
        "末章跳转应按夹紧值落位：实际 {actual:.2}，期望 {expected:.2}",
        actual = app.editor.scroll_y,
    );

    // 几何必然：末章贴不到顶，但整行必须落在视口内。
    let (doc_text, clip) = editor_doc_galley(&shapes, &content);
    let row = source_row_rect(doc_text, target.line);
    assert!(
        row.min.y >= clip.min.y - 0.5 && row.max.y <= clip.max.y + 0.5,
        "末章标题应完整可见：行 [{:.2}, {:.2}]，视口 [{:.2}, {:.2}]",
        row.min.y,
        row.max.y,
        clip.min.y,
        clip.max.y,
    );
}
