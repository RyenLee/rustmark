//! 模块外置测试。
use super::*;

/// 字体清晰度：每一份交给 egui 的字体都必须**显式**开启平滑 hinting 与
/// 亚像素定位，不依赖全局 `TextOptions` 默认值（主题切换会重建 `Visuals`）。
/// 换字体时若这两项随默认值漂移，`Fonts::begin_pass` 会二次重建图集，
/// 跨帧 Galley 的 `uv_rect` 就会采样到错位的字形——表现为锯齿 / 发虚。
#[test]
fn egui_fonts_always_use_explicit_antialiasing_options() {
    let setup = crate::infrastructure::fonts::font_definitions(None);
    let defs = to_egui_font_definitions(setup.definitions);
    assert!(!defs.font_data.is_empty(), "至少应有内置基础字体");
    for (name, font) in &defs.font_data {
        assert_eq!(
            font.tweak.hinting,
            Some(true),
            "{name} 必须显式开启平滑 hinting"
        );
        assert_eq!(
            font.tweak.subpixel_binning,
            Some(true),
            "{name} 必须显式开启亚像素定位"
        );
    }
}

/// 明暗两套 `Visuals` 的字体渲染选项都要保持恒定：任一字段在两套主题间
/// 不同，切换主题（或重装字体）都会触发字体图集重建。
#[test]
fn both_visuals_keep_font_rendering_options_constant() {
    let ctx = egui::Context::default();
    crate::presentation::theme::mapper::apply(
        &ctx,
        &crate::presentation::theme::builtin::Theme::dark(),
    );
    let dark = ctx.style_of(egui::Theme::Dark);
    assert!(dark.visuals.text_options.font_hinting);
    assert!(dark.visuals.text_options.subpixel_binning);

    crate::presentation::theme::mapper::apply(
        &ctx,
        &crate::presentation::theme::builtin::Theme::light(),
    );
    let light = ctx.style_of(egui::Theme::Light);
    assert!(light.visuals.text_options.font_hinting);
    assert!(light.visuals.text_options.subpixel_binning);
}

/// 拖放分类：白名单内的格式（markdown 家族 / txt / csv）应打开，其余应拒绝。
#[test]
fn classify_dropped_splits_by_extension() {
    let paths = vec![
        "/d/docs/笔记.md".to_string(),
        "/d/docs/readme.markdown".to_string(),
        "/d/docs/plain.txt".to_string(),
        "/d/docs/table.csv".to_string(),
        "/d/docs/photo.png".to_string(),
        "/d/docs/report.pdf".to_string(),
        "/d/docs/archive.zip".to_string(),
    ];
    let (open, reject) = classify_dropped(&TextFormats::builtin(), &paths);
    assert_eq!(
        open,
        vec![
            "/d/docs/笔记.md".to_string(),
            "/d/docs/readme.markdown".to_string(),
            "/d/docs/plain.txt".to_string(),
            "/d/docs/table.csv".to_string(),
        ],
        "markdown / markdown / txt / csv 都应被识别为可打开（与文件树守卫同一份白名单）"
    );
    assert_eq!(reject.len(), 3, "png/pdf/zip 应被拒绝");
}

/// 大小写不敏感：`.MD` 也应接受。
#[test]
fn classify_dropped_is_case_insensitive() {
    let (open, reject) = classify_dropped(
        &TextFormats::builtin(),
        &["/d/A.MD".to_string(), "/d/B.TXT".to_string()],
    );
    assert_eq!(open.len(), 2);
    assert!(reject.is_empty());
}

/// 空输入应不产生任何分组。
#[test]
fn classify_dropped_empty_is_noop() {
    let (open, reject) = classify_dropped(&TextFormats::builtin(), &[]);
    assert!(open.is_empty() && reject.is_empty());
}

/// 分类用的是**传入的**白名单，不是内置常量——拖放与守卫同源。
#[test]
fn classify_dropped_follows_the_injected_whitelist() {
    let custom = TextFormats::builtin().extended_with(["log"]);
    let paths = vec!["/d/app.log".to_string(), "/d/a.md".to_string()];
    let (open, _) = classify_dropped(&custom, &paths);
    assert_eq!(open.len(), 2, "追加的 .log 应随白名单一起放行");
    let (open, reject) = classify_dropped(&TextFormats::builtin(), &paths);
    assert_eq!(open.len(), 1);
    assert_eq!(reject.len(), 1);
}
