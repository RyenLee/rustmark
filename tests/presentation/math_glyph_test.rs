//! 数学符号的**字形覆盖**回归（问题 3：公式里的符号显示为方框）。
//!
//! # 为什么必须自动化核对
//!
//! 符号表把 `\alpha` 映射到 `α` 之类的 Unicode 字形，但**映射过去不等于
//! 渲染得出来**：字形还要落在已装载字体（比例族 + CJK 兜底）的覆盖范围内。
//! 落在范围外的表现是一个方框——不报错、不崩溃，在几十个符号里靠肉眼
//! 几乎不可能发现（工具栏图标踩过同样的坑）。
//!
//! 本文件按**真实字体配置**（与运行一致的 `font_definitions(None)` +
//! 系统 CJK 兜底）逐个符号核对：`math::paint` 用比例族画文本，
//! 故以 `FontId::proportional` 判定；等宽族一并核对，因为源码卡回退
//! 与将来的行内公式可能走它。

use rustmark::presentation::views::math;

/// 装与真实应用同一套字体的上下文。
fn ctx_with_app_fonts() -> egui::Context {
    let ctx = egui::Context::default();
    let setup = rustmark::infrastructure::fonts::font_definitions(None);
    ctx.set_fonts(setup.definitions);
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});
    ctx
}

#[test]
fn every_math_symbol_has_a_glyph_under_the_real_font_setup() {
    let ctx = ctx_with_app_fonts();
    let proportional = egui::FontId::proportional(16.0);
    let monospace = egui::FontId::monospace(16.0);

    let mut missing_prop = Vec::new();
    let mut missing_mono = Vec::new();
    let mut checked = 0usize;
    for (cmd, glyph) in math::symbol_table() {
        // 只核对「命令名是纯字母」的条目：`{` / `%` / ` ` 这类键不是
        // `\命令`，而是解析器直接吃掉的字面量，走不到符号表查找。
        if cmd.is_empty() || !cmd.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        if glyph.trim().is_empty() {
            continue; // `\!` 之类负间距，本就不画字形。
        }
        checked += 1;
        if !ctx.fonts_mut(|f| f.has_glyphs(&proportional, glyph)) {
            missing_prop.push(format!("\\{cmd} → {glyph}"));
        }
        if !ctx.fonts_mut(|f| f.has_glyphs(&monospace, glyph)) {
            missing_mono.push(format!("\\{cmd} → {glyph}"));
        }
    }
    assert!(checked > 50, "前提：应核对到足够多的符号，实际 {checked}");
    assert!(
        missing_prop.is_empty(),
        "比例族缺字形（预览里会是方框）：{missing_prop:?}"
    );
    assert!(
        missing_mono.is_empty(),
        "等宽族缺字形（源码卡回退里会是方框）：{missing_mono:?}"
    );
}

/// 每个符号命令都要能**独立排版**（`\命令` 单独出现时不报 Unsupported）。
///
/// 这条守的是另一半：「表里有、排版器不认」——那种符号同样会整块回退到
/// 源码卡，用户看到的是公式消失而不是方框。
#[test]
fn every_symbol_command_layouts_on_its_own() {
    let measure = |text: &str, size: f32| text.chars().count() as f32 * size * 0.5;
    let mut unsupported = Vec::new();
    let mut checked = 0usize;
    for (cmd, _) in math::symbol_table() {
        if cmd.is_empty() || !cmd.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        checked += 1;
        let latex = format!("\\{cmd}");
        if let Err(e) = math::layout(&latex, 16.0, false, &measure) {
            unsupported.push(format!("{latex}: {e}"));
        }
    }
    assert!(checked > 50, "前提：应核对到足够多的符号，实际 {checked}");
    assert!(
        unsupported.is_empty(),
        "符号无法独立排版（会整块回退到源码卡）：{unsupported:?}"
    );
}
