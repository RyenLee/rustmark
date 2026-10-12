use super::highlight_cache::build_line_job;
use super::*;
use crate::application::dto::{RangeView, SpanView};
use egui::Color32;
use egui::text::{LayoutJob, LayoutSection};

/// 可用于排版的 `Context`（先跑空帧装好字库，否则 `fonts_mut` 会 panic）。
fn test_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

/// 取某段覆盖的文本（egui 0.33 的 `LayoutSection` 用 `byte_range` 索引 `job.text`）。
fn section_text<'a>(job: &'a LayoutJob, s: &LayoutSection) -> &'a str {
    job.text.get(s.byte_range.clone()).unwrap_or("")
}

/// 查找命中段必须叠加背景，非命中段保持透明。
#[test]
fn build_line_job_applies_find_background() {
    let theme = Theme::dark();
    // "hello world" 中命中字节 6..11（"world"）。
    let job = build_line_job(&theme, "hello world", &[], &[(6, 11, false)], 0.0);
    let world = job
        .sections
        .iter()
        .find(|s| section_text(&job, s) == "world")
        .expect("应有 world 段");
    assert_ne!(
        world.format.background,
        Color32::TRANSPARENT,
        "查找命中段必须叠加背景"
    );
    let hello = job
        .sections
        .iter()
        .find(|s| section_text(&job, s) == "hello ")
        .expect("应有 hello 段");
    assert_eq!(
        hello.format.background,
        Color32::TRANSPARENT,
        "非命中段不应有背景"
    );
}

/// 当前命中背景应区别于普通命中（更醒目）。
#[test]
fn current_mark_uses_distinct_background() {
    let theme = Theme::dark();
    let plain = build_line_job(&theme, "hello world", &[], &[(6, 11, false)], 0.0);
    let current = build_line_job(&theme, "hello world", &[], &[(6, 11, true)], 0.0);
    let plain_bg = plain
        .sections
        .iter()
        .find(|s| section_text(&plain, s) == "world")
        .unwrap()
        .format
        .background;
    let current_bg = current
        .sections
        .iter()
        .find(|s| section_text(&current, s) == "world")
        .unwrap()
        .format
        .background;
    assert_ne!(plain_bg, current_bg, "当前命中背景应区别于普通命中");
}

/// 语法前景色与查找背景可共存：命中段若同时是语法 span，颜色不丢。
#[test]
fn find_background_does_not_clobber_syntax_foreground() {
    let theme = Theme::dark();
    let spans = vec![SpanView {
        range: RangeView::new(6, 11),
        style: "strong".to_string(),
    }];
    let job = build_line_job(&theme, "hello world", &spans, &[(6, 11, false)], 0.0);
    let world = job
        .sections
        .iter()
        .find(|s| section_text(&job, s) == "world")
        .expect("应有 world 段");
    assert_ne!(
        world.format.background,
        Color32::TRANSPARENT,
        "命中段应有背景"
    );
    assert_eq!(
        world.format.color, theme.syn_bold,
        "命中段不应丢失语法前景色"
    );
}

/// `find_sig` 必须并入缓存键：不同命中集要失效重建，相同命中集要命中整文档缓存。
#[test]
fn highlight_layout_keys_on_find_signature() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();
    let text = "the cat sat on the mat";
    let marks = vec![FindMark {
        start: 4,
        end: 7,
        current: true,
    }];

    let none = highlight_layout_with_find(&ctx, &theme, text, None, 400.0, None);
    let with = highlight_layout_with_find(&ctx, &theme, text, None, 400.0, Some(&marks));
    assert!(
        !std::sync::Arc::ptr_eq(&none, &with),
        "有无查找命中必须失效重建（否则旧高亮残留）"
    );
    let with_again = highlight_layout_with_find(&ctx, &theme, text, None, 400.0, Some(&marks));
    assert!(
        std::sync::Arc::ptr_eq(&with, &with_again),
        "相同命中集应命中整文档缓存（空闲帧零开销）"
    );
}

/// DEF-04：span 边界落在多字节字符中间时不得 panic（`panic = "abort"`）。
///
/// 生产路径每帧重新高亮，span 与文本天然同源，故这个缺陷从未触发；
/// 但「同源」只是约定不是保证——缓存版本错位、手写区间都会打破它。
/// 表现层是最后一道防线：宁可少上一段色，也不能崩。
#[test]
fn build_line_job_survives_non_char_boundary_spans() {
    let theme = Theme::dark();
    // 中=0..3、文=3..6、a=6、b=7、c=8 → 1 与 5 都在字符内部。
    let line = "中文abc";
    assert!(
        !line.is_char_boundary(1) && !line.is_char_boundary(5),
        "用例前提：这两个偏移必须是非字符边界"
    );
    let spans = vec![SpanView {
        range: RangeView::new(1, 5),
        style: "strong".to_string(),
    }];
    let job = build_line_job(&theme, line, &spans, &[], 0.0);
    // 走到这里即未 panic；再断言文本被完整覆盖（少上色可以，丢字符不行）。
    let covered: String = job.sections.iter().map(|s| section_text(&job, s)).collect();
    assert_eq!(covered, line, "吸附边界后不得丢字符");
}

/// 查找命中区间同样可能被算到字符中间（DEF-04 的同源风险）。
#[test]
fn build_line_job_survives_non_char_boundary_find_marks() {
    let theme = Theme::dark();
    let line = "中文abc";
    let job = build_line_job(&theme, line, &[], &[(1, 5, true)], 0.0);
    let covered: String = job.sections.iter().map(|s| section_text(&job, s)).collect();
    assert_eq!(covered, line, "吸附边界后不得丢字符");
}

/// 端到端：span 来自「文本的上一版」（长度已变）时，整篇排版不得 panic。
#[test]
fn highlight_layout_survives_spans_from_older_text() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();
    let older = "列表项一";
    // 新版更长 → 旧 span 的坐标与文本整体错位。
    let newer = "列表项壹拾贰";
    let spans = vec![SpanView {
        range: RangeView::new(1, older.len()),
        style: "strong".to_string(),
    }];
    let g = highlight_layout(&ctx, &theme, newer, Some(&spans), 900.0);
    assert!(g.rect.height() > 0.0, "排版应产出非空结果");
}

/// DEF-05（回归）：`highlight_layout*` 的产出不得被 `Galley::is_empty()` 判为空。
///
/// `Galley::concat` 只把传入的 `job` 原样搬到合并结果上，而下游两处都以
/// `galley.is_empty()`（≡ `job.sections.is_empty()`）作为「整篇为空、不画」的判据：
/// `egui::Painter::galley` 与 `epaint::Tessellator::tessellate_text`。
/// 于是「逐行 Galley 排得完全正确、编辑区却一个字都不画」是可能的——
/// 只要 `sections` 被留空，`Shape::galley` 根本进不了 `PaintList`。
///
/// 这条用例直接把该前提钉死：文本非空 ⇒ 整篇 Galley 非空。
/// 它不依赖 egui 渲染，因此比 `tests/presentation/editor_view_test.rs`
/// 更早、更准地定位到这一类回归。
#[test]
fn highlight_layout_output_is_not_empty_for_non_empty_text() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let text = "# 标题\n\n正文一行。\n第二行 `code`";
    let spans = vec![SpanView {
        range: RangeView::new(0, 5),
        style: "heading:1".to_string(),
    }];

    // 两个主题都要过：该缺陷与配色无关，任何主题下都是白板。
    for theme in [Theme::dark(), Theme::light()] {
        let g = highlight_layout(&ctx, &theme, text, Some(&spans), 900.0);
        assert!(
            !g.is_empty(),
            "整篇 Galley 被判为空 → Painter::galley 会跳过绘制（编辑区白板）"
        );
        assert_eq!(
            g.text(),
            text,
            "整篇 Galley 必须携带全文（选中/查找依赖它）"
        );
        assert!(g.rect.height() > 0.0, "排版应产出非零高度");
    }

    // 不带 span（防抖期 / 大文件降级）走的也是同一条拼接路径。
    clear_highlight_caches();
    let plain = highlight_layout(&ctx, &Theme::dark(), "纯文本", None, 900.0);
    assert!(!plain.is_empty(), "无 span 的降级路径同样不得被判为空");
    assert_eq!(plain.text(), "纯文本");

    // 带查找命中（编辑区 Ctrl+F 高亮）的路径同理。
    clear_highlight_caches();
    let marks = vec![FindMark {
        start: 0,
        end: 3,
        current: true,
    }];
    let found =
        highlight_layout_with_find(&ctx, &Theme::dark(), "纯文本", None, 900.0, Some(&marks));
    assert!(!found.is_empty(), "带查找命中的路径同样不得被判为空");
}
