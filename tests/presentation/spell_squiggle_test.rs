//! 拼写波浪线几何用例（P1 · 零依赖路线的落地验证）。
//!
//! 波浪线是「错词字节区间 ×排版结果」的乘积，最容易出的错是**静默错位**：
//! 编译不报、运行不崩，只是波浪线画在别的词下面。因此这里逐条钉住：
//! 越界区间被丢弃、跨行错词被拆段、相位错开、坐标随origin 平移。

use rustmark::presentation::theme::builtin::Theme;
use rustmark::presentation::theme::mapper::{SpellMark, highlight_layout, squiggles_for};

fn test_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

fn galley_of(ctx: &egui::Context, text: &str, wrap: f32) -> std::sync::Arc<egui::Galley> {
    highlight_layout(ctx, &Theme::dark(), text, None, wrap)
}

fn mark(start: usize, end: usize) -> SpellMark {
    SpellMark { start, end }
}

// ============================================================================
// 基本几何
// ============================================================================

#[test]
fn squiggle_is_produced_for_a_valid_range() {
    let ctx = test_ctx();
    let text = "teh quick";
    let g = galley_of(&ctx, text, 400.0);
    let sq = squiggles_for(&g, text, &[mark(0, 3)], egui::Vec2::ZERO);
    assert_eq!(sq.len(), 1, "一个错词应产出一个片段");
    assert!(sq[0].rect.width() > 0.0, "片段应有实际宽度");
    assert!(sq[0].rect.height() > 0.0, "片段应有实际高度");
}

#[test]
fn squiggle_position_tracks_the_marked_word() {
    // 核心不变量：标记越靠后，波浪线越靠右。
    // 字节↔像素映射写错时，这里会立刻暴露（两者会重叠或倒置）。
    let ctx = test_ctx();
    let text = "teh quick brown";
    let g = galley_of(&ctx, text, 400.0);
    let first = squiggles_for(&g, text, &[mark(0, 3)], egui::Vec2::ZERO);
    let second = squiggles_for(&g, text, &[mark(10, 15)], egui::Vec2::ZERO);
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert!(
        second[0].rect.left() > first[0].rect.left(),
        "后一个词的波浪线应更靠右：{:?} vs {:?}",
        second[0].rect,
        first[0].rect
    );
}

#[test]
fn origin_translation_is_applied() {
    let ctx = test_ctx();
    let text = "teh quick";
    let g = galley_of(&ctx, text, 400.0);
    let base = squiggles_for(&g, text, &[mark(0, 3)], egui::Vec2::ZERO);
    let moved = squiggles_for(&g, text, &[mark(0, 3)], egui::vec2(100.0, 50.0));
    assert_eq!(base.len(), 1);
    assert_eq!(moved.len(), 1);
    assert!((moved[0].rect.left() - base[0].rect.left() - 100.0).abs() < 0.01);
    assert!((moved[0].rect.top() - base[0].rect.top() - 50.0).abs() < 0.01);
}

// ============================================================================
// 防御性：非法区间不得 panic（`panic = "abort"` 下会直接崩进程）
// ============================================================================

#[test]
fn invalid_ranges_are_skipped_not_panicked() {
    let ctx = test_ctx();
    let text = "teh quick";
    let g = galley_of(&ctx, text, 400.0);
    // 起点 >= 终点
    assert!(squiggles_for(&g, text, &[mark(3, 3)], egui::Vec2::ZERO).is_empty());
    assert!(squiggles_for(&g, text, &[mark(5, 2)], egui::Vec2::ZERO).is_empty());
    // 越界
    assert!(squiggles_for(&g, text, &[mark(0, 999)], egui::Vec2::ZERO).is_empty());
    // 空标记集
    assert!(squiggles_for(&g, text, &[], egui::Vec2::ZERO).is_empty());
}

#[test]
fn range_landing_inside_a_multibyte_char_is_skipped() {
    // 中文 3 字节；区间若落在字符中间，切片会 panic。
    // `panic = "abort"` 下没有 unwind 兜底，纯函数必须自己挡住。
    let ctx = test_ctx();
    let text = "中文 teh";
    let g = galley_of(&ctx, text, 400.0);
    // 字节 1 是「文」的中间
    let sq = squiggles_for(&g, text, &[mark(1, 4)], egui::Vec2::ZERO);
    assert!(sq.is_empty(), "落在多字节字符中间的区间应被跳过");
}

#[test]
fn multibyte_prefix_keeps_correct_geometry() {
    // 中文在前、错词在后：字节→字符换算错了波浪线就会落在中文上。
    let ctx = test_ctx();
    let text = "中文前缀 teh 后缀";
    let g = galley_of(&ctx, text, 400.0);
    // "teh" 起始于"中文前缀 " 之后
    let start = text.find("teh").expect("应能定位");
    let sq = squiggles_for(&g, text, &[mark(start, start + 3)], egui::Vec2::ZERO);
    assert_eq!(sq.len(), 1, "多字节前缀下仍应产出片段");
}

// ============================================================================
// 跨行与相位
// ============================================================================

#[test]
fn wrapped_word_is_split_into_multiple_segments() {
    // 窄宽度强制软换行后，一个跨行的错词必须拆成多段，
    // 否则波浪线会横穿两行变成一条长下划线。
    let ctx = test_ctx();
    let text = "aaaaaaaaaa tttttttt bbbbbbbbbb cccccccccc";
    // 宽度取得很窄，保证 `tttttttt` 落在行尾而`bbbbbbbbbb` 在下一行。
    let g = galley_of(&ctx, text, 90.0);
    let start = text.find("tttttttt").expect("应能定位");
    let end = start + "tttttttt".len();
    let sq = squiggles_for(&g, text, &[mark(start, end)], egui::Vec2::ZERO);
    assert!(
        !sq.is_empty(),
        "跨行错词至少应产出一个片段（wrap 宽度 {}）",
        90.0
    );
    // 每个片段的宽度都应小于整行的宽度——若等于，说明没拆开。
    let line_w = g.size().x;
    for s in &sq {
        assert!(
            s.rect.width() <= line_w + 1.0,
            "片段宽度不应超过行宽，说明跨行未被拆分：{:?} / {line_w}",
            s.rect
        );
    }
}

#[test]
fn distinct_words_get_distinct_phases() {
    // 相邻错词的波形必须错开，否则视觉上连成一条实线。
    let ctx = test_ctx();
    let text = "teh recieve seperate";
    let g = galley_of(&ctx, text, 400.0);
    let sq = squiggles_for(&g, text, &[mark(0, 3), mark(4, 11)], egui::Vec2::ZERO);
    assert_eq!(sq.len(), 2);
    assert!(
        (sq[0].phase - sq[1].phase).abs() > f32::EPSILON,
        "相邻错词应错开相位，否则波浪会连成实线：{} vs {}",
        sq[0].phase,
        sq[1].phase
    );
}
