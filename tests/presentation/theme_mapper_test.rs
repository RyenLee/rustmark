//! 主题系统用例（S6-10 验收标准）。
//!
//! 验收标准：切换主题后编辑区高亮、预览区排版、侧边栏 / 状态栏配色同步更新
//! 且无需重启；跟随系统响应系统主题变化；主题变更触发预览缓存失效。
//!
//! 用例里包含一条**回归锁定**：原型的 `rgba(r, g, b, a)` 里 `a` 是
//! **不透明度**，而 `Color32` 的第 4 通道是 **alpha（透明度）**。
//! 两者差一个 `1 - a`，写反会让深色主题整体发灰——肉眼能看出，
//! 但不会有任何测试失败，故必须钉死。

use egui::Color32;

use rustmark::application::dto::ThemeView;
use rustmark::presentation::theme::builtin::{
    Metrics, Theme, palette_signature, resolve, theme_kind_label,
};
use rustmark::presentation::theme::mapper::{StyleMapper, apply};
use rustmark::shared::theme::ThemeKind;

// ============================================================================
// 配色对齐原型 tokens.css
// ============================================================================

/// 深色主题的关键色值必须与原型 `[data-theme='dark']` 逐值一致。
///
/// 2026-10：原型 `tokens.css` 做了视觉精修（更深的画布 / 分明的层次 /
/// 单一冷蓝强调色），本用例的期望值随之重抄一遍。
#[test]
fn dark_palette_matches_prototype_tokens() {
    let theme = Theme::dark();
    let cases: &[(Color32, Color32, &str)] = &[
        (
            theme.bg_primary,
            Color32::from_rgb(0x14, 0x16, 0x1B),
            "bg_primary",
        ),
        (
            theme.bg_secondary,
            Color32::from_rgb(0x1A, 0x1D, 0x24),
            "bg_secondary",
        ),
        (
            theme.bg_tertiary,
            Color32::from_rgb(0x23, 0x28, 0x33),
            "bg_tertiary",
        ),
        (theme.border, Color32::from_rgb(0x2B, 0x32, 0x3D), "border"),
        (
            theme.text_primary,
            Color32::from_rgb(0xE8, 0xEA, 0xF0),
            "text_primary",
        ),
        (
            theme.text_secondary,
            Color32::from_rgb(0x9A, 0xA4, 0xB4),
            "text_secondary",
        ),
        (theme.accent, Color32::from_rgb(0x6A, 0xA9, 0xFF), "accent"),
        (
            theme.success,
            Color32::from_rgb(0x4E, 0xCF, 0xA4),
            "success",
        ),
        (
            theme.warning,
            Color32::from_rgb(0xE0, 0xB0, 0x54),
            "warning",
        ),
        (theme.error, Color32::from_rgb(0xF2, 0x72, 0x7A), "error"),
        (
            theme.syn_heading,
            Color32::from_rgb(0x6A, 0xA9, 0xFF),
            "syn_heading",
        ),
        (
            theme.syn_bold,
            Color32::from_rgb(0xE0, 0x98, 0x7A),
            "syn_bold",
        ),
        (
            theme.syn_italic,
            Color32::from_rgb(0xC5, 0x8A, 0xF9),
            "syn_italic",
        ),
        (
            theme.syn_code,
            Color32::from_rgb(0x4E, 0xD8, 0xB4),
            "syn_code",
        ),
        (
            theme.syn_quote,
            Color32::from_rgb(0x7D, 0xA3, 0x5F),
            "syn_quote",
        ),
        (
            theme.syn_marker,
            Color32::from_rgb(0xE2, 0xC0, 0x7D),
            "syn_marker",
        ),
        (
            theme.syn_table,
            Color32::from_rgb(0x9C, 0xD4, 0xFE),
            "syn_table",
        ),
    ];
    for (actual, expected, name) in cases {
        assert_eq!(*actual, *expected, "{name} 与原型 tokens.css 不一致");
    }
}

/// 浅色主题同理。
#[test]
fn light_palette_matches_prototype_tokens() {
    let theme = Theme::light();
    let cases: &[(Color32, Color32, &str)] = &[
        (
            theme.bg_primary,
            Color32::from_rgb(0xFF, 0xFF, 0xFF),
            "bg_primary",
        ),
        (
            theme.bg_secondary,
            Color32::from_rgb(0xF7, 0xF8, 0xFA),
            "bg_secondary",
        ),
        (
            theme.bg_tertiary,
            Color32::from_rgb(0xEE, 0xF1, 0xF5),
            "bg_tertiary",
        ),
        (theme.border, Color32::from_rgb(0xE3, 0xE6, 0xEC), "border"),
        (
            theme.text_primary,
            Color32::from_rgb(0x1A, 0x1D, 0x23),
            "text_primary",
        ),
        (
            theme.text_secondary,
            Color32::from_rgb(0x5C, 0x65, 0x75),
            "text_secondary",
        ),
        (theme.accent, Color32::from_rgb(0x25, 0x63, 0xEB), "accent"),
        (
            theme.success,
            Color32::from_rgb(0x0F, 0x8A, 0x63),
            "success",
        ),
        (
            theme.warning,
            Color32::from_rgb(0x96, 0x69, 0x0A),
            "warning",
        ),
        (theme.error, Color32::from_rgb(0xD1, 0x34, 0x38), "error"),
        (
            theme.syn_heading,
            Color32::from_rgb(0x1F, 0x4F, 0xB8),
            "syn_heading",
        ),
        (
            theme.syn_bold,
            Color32::from_rgb(0xB3, 0x26, 0x1E),
            "syn_bold",
        ),
        (
            theme.syn_italic,
            Color32::from_rgb(0x8B, 0x2F, 0x6B),
            "syn_italic",
        ),
        (
            theme.syn_code,
            Color32::from_rgb(0x0F, 0x7A, 0x63),
            "syn_code",
        ),
        (
            theme.syn_quote,
            Color32::from_rgb(0x2F, 0x7D, 0x32),
            "syn_quote",
        ),
        (
            theme.syn_table,
            Color32::from_rgb(0x16, 0x55, 0xA8),
            "syn_table",
        ),
    ];
    for (actual, expected, name) in cases {
        assert_eq!(*actual, *expected, "{name} 与原型 tokens.css 不一致");
    }
}

/// 半透明色的 alpha 必须按「不透明度」换算成 `Color32` 的 alpha。
///
/// 原型 `rgba(106, 169, 255, 0.14)` = 14% **不透明**，
/// 对应 `Color32` 的 alpha 通道应为 `0.14 * 255 ≈ 36`。
/// 若误写成 `alpha = 0.14`（即 14% 透明），深色主题底色会被大幅冲淡。
#[test]
fn prototype_opacity_is_converted_to_alpha_correctly() {
    let theme = Theme::dark();
    // accent_soft = rgba(106, 169, 255, 0.14) → 约 36
    let alpha = theme.accent_soft.a();
    assert!(
        (33..=38).contains(&alpha),
        "accent_soft 的 alpha 应约 36（14% 不透明），实际 {alpha}"
    );

    // current_line = rgba(255, 255, 255, 0.045) → 约 11
    let current = theme.current_line.a();
    assert!(
        (9..=13).contains(&current),
        "current_line 的 alpha 应约 11（4.5% 不透明），实际 {current}"
    );

    // 反向自查：把 alpha 当不透明度会得到 255-36=219，两者差 5 倍以上。
    assert!(
        theme.accent_soft.a() < 60,
        "若 alpha 看起来像 200+，说明把不透明度当成了透明度"
    );
}

/// 深浅主题的正文色必须相反（否则其中一个主题不可读）。
#[test]
fn light_and_dark_have_opposite_foreground() {
    let dark = Theme::dark();
    let light = Theme::light();

    // 深色主题：浅字；浅色主题：深字。
    assert!(dark.text_primary.to_array()[0] > 128, "深色主题应为浅色字");
    assert!(light.text_primary.to_array()[0] < 128, "浅色主题应为深色字");

    // 背景亦相反。
    assert!(dark.bg_primary.to_array()[0] < 128);
    assert!(light.bg_primary.to_array()[0] > 128);
}

// ============================================================================
// 尺寸token（布局不破版的前提）
// ============================================================================

/// 尺寸必须与原型栅格一致——验收要求两档窗口下布局不破版。
#[test]
fn metrics_match_prototype_grid() {
    let m = Metrics::default();
    assert_eq!(m.topbar_height, 40.0, "--topbar-h");
    assert_eq!(
        m.editor_toolbar_height, 34.0,
        "编辑区工具栏行高（原型把工具栏与应用菜单放在同一行，本项目按作用域拆成两层后新增的一段）"
    );
    assert_eq!(m.statusbar_height, 24.0, "--statusbar-h");
    assert_eq!(m.tabbar_height, 28.0, "--tabbar-h");
    assert_eq!(m.tabstrip_height, 32.0, "--tabstrip-h");
    assert_eq!(m.sidebar_width, 308.0, "--sidebar-w");
    assert_eq!(m.divider_width, 4.0, "--divider-w");
    assert_eq!(m.radius_button, 6.0, "--radius-btn");
    assert_eq!(m.radius_card, 8.0, "--radius-card");
    assert_eq!(m.radius_dialog, 14.0, "--radius-dialog");
    assert_eq!(m.ui_font_size, 13.0, "--ui-font-size");
    assert_eq!(m.preview_font_size, 15.0, "--preview-font-size");
    assert_eq!(m.preview_line_height, 1.7, "--preview-line-height");
}

/// 两档验收窗口下，各区域高度之和不得超过窗口高度（否则状态栏被挤出屏幕）。
///
/// 顶部固定区域现共**三段**（应用级菜单栏 / 标签条 / 面包屑）——
/// 编辑区工具栏已移到**编辑区内部顶部**（见 `editor_view::show`），
/// 它占的是编辑区自己的高度，而非不可压缩的顶栏 chrome，故此处不再计入。
/// 底部固定区为状态栏。三者之外才是可伸缩的编辑/预览正文。
#[test]
fn vertical_budget_fits_both_acceptance_window_sizes() {
    let m = Metrics::default();
    // 不可压缩的 chrome：顶栏三段 + 底部状态栏。
    let chrome = m.topbar_height + m.tabstrip_height + m.tabbar_height + m.statusbar_height;
    for (w, h) in [(1280.0_f32, 800.0_f32), (1440.0, 900.0)] {
        // 编辑区（含其顶部工具栏）至少还要留这么高，正文才不至于完全消失。
        let editor_min = m.editor_toolbar_height + 200.0;
        assert!(
            chrome + editor_min <= h,
            "窗口 {w}×{h} 下固定区域（{chrome}）+ 编辑区最小高度（{editor_min}）超预算"
        );
    }
}

// ============================================================================
// 跟随系统
// ============================================================================

/// 「跟随系统」按系统偏好解析为具体配色。
#[test]
fn system_theme_resolves_by_system_preference() {
    let view = theme_view("system");

    let dark = resolve(&view, true);
    assert!(dark.is_dark(), "系统为深色时应解析出深色主题");
    assert_eq!(dark.bg_primary, Theme::dark().bg_primary);

    let light = resolve(&view, false);
    assert!(!light.is_dark(), "系统为浅色时应解析出浅色主题");
    assert_eq!(light.bg_primary, Theme::light().bg_primary);
}

/// 解析后的 `kind` 必须是**实际生效**的那套，否则 `is_dark()` 会误报
/// （`kind`仍是 `System` 时 `is_dark()` 返回 false，深色系统下就错了）。
#[test]
fn resolved_kind_reflects_effective_palette() {
    let view = theme_view("system");
    let dark = resolve(&view, true);
    assert_eq!(dark.kind, ThemeKind::Dark);
    assert!(dark.is_dark());

    let light = resolve(&view, false);
    assert_eq!(light.kind, ThemeKind::Light);
    assert!(!light.is_dark());
}

/// 显式深 / 浅不受系统偏好影响。
#[test]
fn explicit_theme_ignores_system_preference() {
    let view = theme_view("dark");
    assert!(
        resolve(&view, false).is_dark(),
        "显式深色不应被系统偏好覆盖"
    );
    assert!(resolve(&view, true).is_dark());

    let view = theme_view("light");
    assert!(!resolve(&view, true).is_dark());
    assert!(!resolve(&view, false).is_dark());
}

/// 未知取值退回「跟随系统」而非报错——旧配置里出现新取值时程序应照常启动。
///
/// # 真源与表现层的容错点不同
///
/// 表现层私有的 `BuiltinTheme::parse`（R-09 已删除）自己带容错：
/// 未知值**直接退回** `System`。
/// 真源 [`ThemeKind::parse`] 则**不**做兜底——它返回 `Option`，
/// 未知值是 `None`。容错的责任落在**调用点**：
/// `builtin::resolve` 用 `unwrap_or(ThemeKind::System)` 兜底，
/// 应用层 `to_theme` 则把未知值**当错误**报出。
///
/// 两种职责划分各有道理：表现层要「照常启动」，应用层要「告诉用户
/// 配置写错了」。本用例守住真源的**分工**：
/// 真源只负责「认得 / 不认得」，兜底由调用点决定。
#[test]
fn unknown_theme_kind_falls_back_to_system() {
    // 真源：认得的照常解析。
    assert_eq!(ThemeKind::parse("LIGHT"), Some(ThemeKind::Light));
    assert_eq!(ThemeKind::parse("  dark  "), Some(ThemeKind::Dark));
    assert_eq!(ThemeKind::parse("system"), Some(ThemeKind::System));
    assert_eq!(ThemeKind::parse("custom"), Some(ThemeKind::Custom));
    // 真源：不认得的**如实**返回 `None`（不擅自兜底）。
    assert_eq!(ThemeKind::parse("sepia"), None);
    assert_eq!(ThemeKind::parse(""), None);
}

/// 表现层的兜底责任：未知取值按「跟随系统」渲染，程序照常启动。
///
/// 这条是上一条的**调用点**版本——真源返回 `None` 后，
/// `builtin::resolve` 必须把它兜成 `System` 再按系统偏好解析。
#[test]
fn resolving_an_unknown_theme_kind_still_yields_a_theme() {
    let view = theme_view("sepia");
    // 未知取值 → 退回跟随系统 → 按系统偏好解析成具体明暗。
    assert!(!resolve(&view, false).is_dark());
    assert!(resolve(&view, true).is_dark());
}

/// 主题字符串与显示名。
#[test]
fn theme_labels_and_strings() {
    assert_eq!(ThemeKind::Dark.as_str(), "dark");
    assert_eq!(theme_kind_label(ThemeKind::Light), "浅色");
    assert_eq!(theme_kind_label(ThemeKind::System), "跟随系统");
    assert_eq!(theme_kind_label(ThemeKind::Custom), "自定义");
}

// ============================================================================
// 字体设置
// ============================================================================

/// 字体设置只影响字体，不影响配色（否则「改字号」会把主题整个换掉）。
#[test]
fn font_settings_do_not_change_palette() {
    let view = ThemeView {
        kind: "dark".to_string(),
        font_family: "Consolas".to_string(),
        font_size: 18.0,
        line_height: 2.0,
        custom: None,
    };
    let themed = resolve(&view, false);
    let base = Theme::dark();

    assert_eq!(themed.bg_primary, base.bg_primary);
    assert_eq!(themed.accent, base.accent);
    assert_eq!(themed.editor_font_size, 18.0);
    assert_eq!(themed.editor_line_height, 2.0);
    assert_eq!(themed.editor_font_family, "Consolas");
}

/// 空字体族不应把排版搞坏（退回内置默认）。
#[test]
fn blank_font_family_falls_back_to_default() {
    let view = ThemeView {
        kind: "dark".to_string(),
        font_family: "   ".to_string(),
        font_size: 14.0,
        line_height: 1.6,
        custom: None,
    };
    let themed = resolve(&view, false);
    assert!(
        !themed.editor_font_family.trim().is_empty(),
        "空字体族应退回默认，而不是产生无法渲染的字体名"
    );
}

// ============================================================================
// 样式映射
// ============================================================================

/// 每种语义样式都必须映射到某个颜色，且不落到 egui 默认灰。
#[test]
fn every_semantic_style_maps_to_a_color() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);

    // 与 `MarkdownAppService::span_style_str` 的取值对应。
    let styles = [
        "plain",
        "heading",
        "heading:1",
        "heading:6",
        "emphasis",
        "strong",
        "strikethrough",
        "code",
        "link",
        "image",
        "quote",
        "list_marker",
        "table_border",
        "task_marker:true",
        "task_marker:false",
        "keyword",
        "string",
        "comment",
        "number",
        "function",
        "type",
        "variable",
        "operator",
    ];
    for style in styles {
        let format = mapper.format(style);
        assert_ne!(
            format.color,
            Color32::GRAY,
            "样式 {style} 未被映射，落到了 egui 默认灰"
        );
    }
}

/// 未知样式必须退回正文色，**不得** panic 或变黑。
#[test]
fn unknown_style_falls_back_to_body_color() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);
    for style in ["totally_unknown", "keyword:99", "", ";;;"] {
        assert_eq!(
            mapper.color(style),
            theme.text_primary,
            "未知样式 {style:?} 应退回正文色"
        );
    }
}

/// **编辑区**所有样式必须共用同一字号与同一行高。
///
/// 这是「行号与文字不再重叠」的**结构性**前提：行号栏按「行高 × 行序号」落点，
/// 只有每一行都真的等高，落点才对得上。
///
/// 早期实现让标题行按 `editor_font_size × heading_scale(level)` 放大，
/// 于是标题行比正文行高出一截——行号每隔几行就与文字错位一次，
/// 图片 / 代码块亦然（它们各按自己的字体度量算高）。现在行的差异只用**颜色**
/// 表达，字号与行高严格相等；标题的放大移到**预览区**（排版视图，
/// 见 [`StyleMapper::heading_scale`]）。
#[test]
fn editor_styles_share_one_font_size_and_line_height() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);
    let body = mapper.format("plain");

    for style in [
        "plain",
        "",
        "heading",
        "heading:1",
        "heading:3",
        "heading:6",
        "strong",
        "emphasis",
        "strikethrough",
        "code",
        "link",
        "image",
        "quote",
        "list_marker",
        "table_border",
        "keyword",
        "string",
        "comment",
    ] {
        let format = mapper.format(style);
        assert_eq!(
            format.font_id.size, body.font_id.size,
            "{style:?} 的字号必须与正文一致，否则该行会高于其他行"
        );
        assert_eq!(
            format.line_height, body.line_height,
            "{style:?} 的行高必须与正文一致，否则行号会与文字错位"
        );
    }

    // 行高确实来自外观设置（字号 × 「行高」），而不是字体自身度量——
    // 后者会随系统字体回退而变（CJK 字体行高天然高于等宽数字）。
    let expected = theme.editor_font_size * theme.editor_line_height;
    assert_eq!(
        body.line_height,
        Some(expected),
        "编辑区行高应等于 editor_font_size × editor_line_height"
    );
}

/// 标题在编辑区**只换颜色**，级别信息不再改变字号。
#[test]
fn heading_style_in_editor_changes_color_only() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);
    let body = mapper.format("plain");
    let h1 = mapper.format("heading:1");

    assert_ne!(h1.color, body.color, "标题应有区别于正文的颜色");
    assert_eq!(h1.font_id, body.font_id, "标题字号不应随级别变化");
}

/// 标题取色一致（所有级别同色，与原型一致）。
#[test]
fn all_heading_levels_share_one_color() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);
    let color = mapper.color("heading:1");
    for level in 2..=6 {
        assert_eq!(
            mapper.color(&format!("heading:{level}")),
            color,
            "h{level} 的颜色应与 h1 一致"
        );
    }
}

/// 斜体 / 删除线 / 链接的装饰属性必须真的生效。
#[test]
fn decorative_attributes_are_applied() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);

    assert!(mapper.format("emphasis").italics, "斜体应设置 italics");
    assert!(!mapper.format("strong").italics, "粗体不应被当成斜体");
    assert!(
        !mapper.format("strikethrough").strikethrough.is_empty(),
        "删除线应设置描边"
    );
    assert!(
        !mapper.format("link").underline.is_empty(),
        "链接应设置下划线"
    );
    assert!(
        mapper.format("plain").underline.is_empty(),
        "普通文本不应有下划线"
    );
}

/// 行内代码应有独立底色（与原型 `--code-bg` 一致）。
#[test]
fn inline_code_has_background() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);
    let code = mapper.format("code");
    assert_eq!(code.background, theme.code_bg);
    assert_eq!(
        mapper.format("plain").background,
        Color32::TRANSPARENT,
        "普通文本不应有底色"
    );
}

/// 深浅主题下同一样式必须取到**不同**颜色（否则切主题后代码仍不可读）。
#[test]
fn styles_differ_between_themes() {
    let dark_theme = Theme::dark();
    let light_theme = Theme::light();
    let dark = StyleMapper::new(&dark_theme);
    let light = StyleMapper::new(&light_theme);

    for style in [
        "plain",
        "heading:1",
        "strong",
        "emphasis",
        "code",
        "link",
        "quote",
        "string",
        "comment",
        "number",
    ] {
        assert_ne!(
            dark.color(style),
            light.color(style),
            "样式 {style} 在深浅主题下取色相同，切主题后该样式不会更新"
        );
    }
}

/// 块类型取色。
#[test]
fn block_colors_depend_on_kind() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);
    assert_eq!(mapper.block_color("heading"), theme.syn_heading);
    assert_eq!(mapper.block_color("quote"), theme.syn_quote);
    assert_eq!(mapper.block_color("paragraph"), theme.text_primary);
    // 带后缀的 raw 类型按前缀匹配，不应 panic。
    assert_eq!(
        mapper.block_color("raw:html"),
        theme.text_primary,
        "raw:html 不应崩溃"
    );
}

/// 标题级别解析。
#[test]
fn heading_level_parsing() {
    assert_eq!(StyleMapper::heading_level_of("heading"), Some(1));
    assert_eq!(StyleMapper::heading_level_of("heading:3"), Some(3));
    assert_eq!(StyleMapper::heading_level_of("heading:x"), None);
    assert_eq!(StyleMapper::heading_level_of("strong"), None);
}

// ============================================================================
// 主题变更 → 预览缓存失效
// ============================================================================

/// 配色变了必须换签名（预览据此重建 Galley）。
#[test]
fn palette_change_invalidates_preview_cache() {
    let dark = Theme::dark();
    let light = Theme::light();
    assert_ne!(
        palette_signature(&dark),
        palette_signature(&light),
        "深浅主题的配色签名必须不同，否则切换主题后预览不会重排"
    );
}

/// 同一主题的签名必须稳定（否则每帧都判定「变了」，缓存永久失效）。
#[test]
fn palette_signature_is_stable_for_same_theme() {
    let a = Theme::dark();
    let b = Theme::dark();
    assert_eq!(palette_signature(&a), palette_signature(&b));
    // 连续调用也一致。
    assert_eq!(palette_signature(&a), palette_signature(&a));
}

/// 改字号不应让配色签名变化——那是纯浪费（预览会无谓重排）。
#[test]
fn font_change_does_not_invalidate_palette_signature() {
    let mut theme = Theme::dark();
    let before = palette_signature(&theme);
    theme.editor_font_size = 20.0;
    theme.editor_line_height = 2.0;
    assert_eq!(
        palette_signature(&theme),
        before,
        "字号变化不应触发预览重排"
    );
}

// ============================================================================
// egui 全局样式应用
// ============================================================================

/// 主题必须能真正写进 egui 的 `Context`（这是「无需重启」的落点）。
#[test]
fn apply_writes_theme_into_egui_context() {
    let ctx = egui::Context::default();

    let dark = Theme::dark();
    apply(&ctx, &dark);
    assert!(
        ctx.style_of(ctx.theme()).visuals.dark_mode,
        "深色主题应设置 dark_mode"
    );
    assert_eq!(
        ctx.style_of(ctx.theme()).visuals.panel_fill,
        dark.bg_secondary,
        "面板底色应来自主题"
    );
    assert_eq!(
        ctx.style_of(ctx.theme()).visuals.code_bg_color,
        dark.code_bg,
        "代码块底色应来自主题"
    );

    // 切到浅色，同一个 Context 必须立刻变——不重建窗口。
    let light = Theme::light();
    apply(&ctx, &light);
    assert!(
        !ctx.style_of(ctx.theme()).visuals.dark_mode,
        "浅色主题应清除 dark_mode"
    );
    assert_eq!(
        ctx.style_of(ctx.theme()).visuals.panel_fill,
        light.bg_secondary
    );

    // 编辑区字号也应随之改变。
    let size = ctx.style_of(ctx.theme()).text_styles[&egui::TextStyle::Monospace].size;
    assert!(
        (size - light.editor_font_size).abs() < 0.01,
        "编辑区字号应来自主题，实际 {size}"
    );
}

/// 编辑区可读性由 egui 的三个 visual 决定，切换主题时必须一并重算：
///
/// - `selection.stroke.color` 是**被选中文字**的颜色（egui 在
///   `paint_text_selection` 里会强制改写选中文字的顶点色）；
/// - `text_cursor.stroke` 是聚焦后的光标色；
/// - `weak_text_color` 是次要文字色。
///
/// 三者若沿用 egui 的深色默认值，浅色主题下就会出现「看不见」的问题。
#[test]
fn apply_sets_readable_caret_and_selection_for_both_themes() {
    let ctx = egui::Context::default();

    for theme in [Theme::dark(), Theme::light()] {
        apply(&ctx, &theme);
        let visuals = ctx.style_of(ctx.theme()).visuals.clone();

        assert_eq!(
            visuals.selection.stroke.color, theme.text_primary,
            "选区文字色应为正文色（压在选择背景上仍可读）"
        );
        assert_eq!(
            visuals.text_cursor.stroke.color, theme.accent,
            "光标色应来自主题，而非 egui 默认值"
        );
        assert_eq!(
            visuals.weak_text_color,
            Some(theme.text_secondary),
            "次要文字色应显式来自主题"
        );
    }
}

/// 浅色主题**不得**继承 egui 的「深色模式专用」光标色。
///
/// egui 的 `TextCursorStyle::default()` 是浅蓝 `rgb(192, 222, 255)`，
/// 在白色画布上几乎不可见。早先的 `apply` 只覆盖部分字段，这个值会被带进
/// 浅色主题——正是「聚焦后看不见光标」的成因。
#[test]
fn light_theme_does_not_inherit_egui_dark_cursor_default() {
    let ctx = egui::Context::default();

    // 先应用深色，再切浅色：浅色必须把光标色重算一遍。
    apply(&ctx, &Theme::dark());
    let dark_caret = ctx.style_of(ctx.theme()).visuals.text_cursor.stroke.color;

    apply(&ctx, &Theme::light());
    let light_caret = ctx.style_of(ctx.theme()).visuals.text_cursor.stroke.color;

    assert_ne!(
        light_caret,
        Color32::from_rgb(192, 222, 255),
        "浅色主题不得沿用 egui 的深色默认光标色"
    );
    assert_ne!(light_caret, dark_caret, "切换主题后光标色必须重算");
}

/// 编辑区正文色与画布背景的对比度必须达到 WCAG AA（正文 4.5:1）。
///
/// 这是「输入文字不可见」这类问题的兜底回归锁：只要有人把正文色或画布色
/// 调到接近，对比度就会跌破阈值，测试立刻失败。
#[test]
fn editor_foreground_has_aa_contrast_in_both_themes() {
    for theme in [Theme::dark(), Theme::light()] {
        let ratio = contrast_ratio(theme.text_primary, theme.bg_primary);
        assert!(
            ratio >= 4.5,
            "正文色与画布色的对比度仅 {ratio:.2}:1，低于 WCAG AA 的 4.5:1"
        );
    }
}

/// 两色对比度（WCAG 2.x：`(L_max + 0.05) / (L_min + 0.05)`）。
fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// 单色相对亮度（WCAG 2.x 的 sRGB 线性化公式）。
fn relative_luminance(color: Color32) -> f32 {
    let [r, g, b, _] = color.to_array();
    let channel = |v: u8| {
        let s = f32::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// 辅助：构造主题设置视图。
fn theme_view(kind: &str) -> ThemeView {
    ThemeView {
        kind: kind.to_string(),
        font_family: String::new(),
        font_size: 14.0,
        line_height: 1.6,
        custom: None,
    }
}

// ============================================================================
// P2 自定义主题（docs/p1p2-remediation-plan.md · 自定义主题）
// ============================================================================

/// 自定义主题：调色板覆盖 8 个核心语义色，语法高亮等其余颜色沿用内置深色值。
#[test]
fn resolve_custom_palette_overrides_core_colors() {
    let view = ThemeView {
        kind: "custom".to_string(),
        font_family: String::new(),
        font_size: 14.0,
        line_height: 1.6,
        custom: Some(rustmark::application::dto::CustomThemePaletteView {
            bg_primary: "#000000".to_string(),
            bg_secondary: "#111111".to_string(),
            text_primary: "#222222".to_string(),
            text_secondary: "#333333".to_string(),
            accent: "#4455FF".to_string(),
            code_bg: "#444444".to_string(),
            border: "#555555".to_string(),
            selection_bg: "#666666".to_string(),
            ..Default::default()
        }),
    };
    let theme = resolve(&view, false);
    assert_eq!(theme.bg_primary, Color32::from_rgb(0, 0, 0));
    assert_eq!(theme.text_primary, Color32::from_rgb(0x22, 0x22, 0x22));
    assert_eq!(theme.accent, Color32::from_rgb(0x44, 0x55, 0xFF));
    // 未开放的颜色（语法高亮）仍沿用内置深色值，而非被改黑。
    assert_ne!(theme.syn_heading, Color32::from_rgb(0, 0, 0));
    // 深色背景 → 解析为 Dark，驱动 egui 明暗模式。
    assert!(theme.is_dark());
}

/// 自定义主题：调色板缺失（旧配置 / 异常）时退回默认深色，不崩溃、不丢配色。
#[test]
fn resolve_custom_without_palette_falls_back_to_dark() {
    let view = ThemeView {
        kind: "custom".to_string(),
        font_family: String::new(),
        font_size: 14.0,
        line_height: 1.6,
        custom: None,
    };
    let theme = resolve(&view, false);
    assert!(theme.is_dark());
    assert_eq!(theme.bg_primary, Theme::dark().bg_primary);
}

/// 自定义主题：浅色背景应解析为 Light（由主背景亮度决定，而非锁死 Dark）。
#[test]
fn resolve_custom_light_background_is_light_mode() {
    let view = ThemeView {
        kind: "custom".to_string(),
        font_family: String::new(),
        font_size: 14.0,
        line_height: 1.6,
        custom: Some(rustmark::application::dto::CustomThemePaletteView {
            bg_primary: "#FFFFFF".to_string(),
            bg_secondary: "#F0F0F0".to_string(),
            text_primary: "#111111".to_string(),
            text_secondary: "#666666".to_string(),
            accent: "#007ACC".to_string(),
            code_bg: "#ECECEC".to_string(),
            border: "#D4D4D4".to_string(),
            selection_bg: "#007ACC".to_string(),
            ..Default::default()
        }),
    };
    let theme = resolve(&view, false);
    assert!(!theme.is_dark(), "浅色背景应解析为 Light 模式");
}

// ----------------------------------------------------------------------------
// V11-09 · 语法高亮细粒度配色
// ----------------------------------------------------------------------------

/// 构造「深色背景 + 指定语法覆盖」的自定义主题视图。
fn custom_view(syn_heading: Option<&str>) -> ThemeView {
    ThemeView {
        kind: "custom".to_string(),
        font_family: String::new(),
        font_size: 14.0,
        line_height: 1.6,
        custom: Some(rustmark::application::dto::CustomThemePaletteView {
            syn_heading: syn_heading.map(str::to_string),
            ..Default::default()
        }),
    }
}

/// 显式给出的语法色要覆盖基线。
#[test]
fn custom_syntax_override_takes_effect() {
    let theme = resolve(&custom_view(Some("#FF0000")), false);
    assert_eq!(
        theme.syn_heading,
        Color32::from_rgb(0xFF, 0x00, 0x00),
        "显式覆盖的语法色必须生效"
    );
    // 其余未覆盖项仍是内置深色主题的值（默认调色板背景是深色）。
    assert_eq!(theme.syn_link, Theme::dark().syn_link);
}

/// 未覆盖的语法色跟随**深色**基线（默认调色板背景为深色）。
#[test]
fn unspecified_syntax_colors_follow_dark_base() {
    let theme = resolve(&custom_view(None), false);
    assert_eq!(theme.syn_heading, Theme::dark().syn_heading);
    assert_eq!(theme.syn_code, Theme::dark().syn_code);
}

/// 浅色背景的自定义主题，未覆盖的语法色必须跟随**浅色**基线——
/// 否则浅底上会残留深色主题的浅色语法色，糊成一片（V11-09 要修的正是这条）。
#[test]
fn light_custom_background_uses_light_syntax_colors() {
    let mut view = custom_view(None);
    if let Some(p) = view.custom.as_mut() {
        p.bg_primary = "#FFFFFF".to_string();
    }
    let theme = resolve(&view, false);
    assert!(!theme.is_dark());
    assert_eq!(
        theme.syn_heading,
        Theme::light().syn_heading,
        "浅色背景应取浅色主题的语法色"
    );
    assert_eq!(theme.syn_link, Theme::light().syn_link);
    // 与深色主题确实不同，证明「跟随」不是巧合命中。
    assert_ne!(Theme::light().syn_heading, Theme::dark().syn_heading);
}

/// 非法 `#RRGGBB` 应退回**基线**（而不是黑色），避免语法色突然变黑看不见。
#[test]
fn invalid_syntax_hex_falls_back_to_base() {
    let theme = resolve(&custom_view(Some("not-a-color")), false);
    assert_eq!(
        theme.syn_heading,
        Theme::dark().syn_heading,
        "非法色值应退回基线而非黑色"
    );
}

/// 任一语法色变化都必须换签名，否则预览缓存不会失效（改了色但预览不变）。
#[test]
fn changing_a_syntax_color_invalidates_the_signature() {
    let base = palette_signature(&Theme::dark());
    let mut theme = Theme::dark();
    theme.syn_marker = Color32::from_rgb(0x01, 0x02, 0x03);
    assert_ne!(
        palette_signature(&theme),
        base,
        "语法色变化必须让配色签名改变"
    );
}

// ============================================================================
// 样式表合并对拍 · 预览区侧（R-10 第 3 条 / D-1 对拍用例）
//
// D-3 阶段 4a（拆 rustmark-core）后从 core 的 `presentation::theme::mapper::tests`
// 迁来：这三条对拍横跨两侧——编辑区 StyleMapper（core）与预览区
// `views::preview_view::test_only::span_format`（root），属跨 crate 契约，
// 只有能同时看见两侧的集成测试才能承载。oracle 抄录与断言迁移前逐字一致。
// ============================================================================

/// 旧 `inline::span_format` 的**原文抄录**（合并前的第二张样式表）。
///
/// 抄进来当 oracle：合并把「样式名 → 语义」的派发收敛到
/// [`StyleSemantics::of`]，本函数是「合并前预览区行内长什么样」的**唯一**记录。
/// 若哪天共享派发出 bug，本函数与新实现的差异会当场暴露。
///
/// 与 `inline.rs` 的现行实现逐字符对应——**故意不改**，它就是历史快照。
fn legacy_inline_span_format(
    theme: &Theme,
    style: &str,
    size: f32,
    line_height: f32,
) -> egui::TextFormat {
    use egui::{FontId, Stroke};
    let base = egui::TextFormat {
        color: theme.text_primary,
        font_id: FontId::proportional(size),
        line_height: Some(line_height),
        ..Default::default()
    };
    match style {
        "heading" => egui::TextFormat {
            color: theme.syn_heading,
            ..base
        },
        "quote" => egui::TextFormat {
            color: theme.syn_quote,
            italics: true,
            ..base
        },
        "strong" => egui::TextFormat {
            color: theme.syn_bold,
            ..base
        },
        "emphasis" => egui::TextFormat {
            color: theme.syn_italic,
            italics: true,
            ..base
        },
        "strikethrough" => egui::TextFormat {
            color: theme.text_secondary,
            strikethrough: Stroke::new(1.0_f32, theme.text_secondary),
            ..base
        },
        "code" => egui::TextFormat {
            color: theme.syn_code,
            background: theme.code_bg,
            font_id: FontId::monospace(size),
            ..base
        },
        "link" => egui::TextFormat {
            color: theme.syn_link,
            underline: Stroke::new(1.0_f32, theme.syn_link),
            ..base
        },
        "math" => egui::TextFormat {
            color: theme.syn_code,
            italics: true,
            ..base
        },
        "keycap" => egui::TextFormat {
            color: theme.syn_code,
            background: theme.code_bg,
            font_id: FontId::monospace(size * 0.92),
            ..base
        },
        "mark" => egui::TextFormat {
            color: theme.text_primary,
            background: theme.find_bg,
            ..base
        },
        "task_done" => egui::TextFormat {
            color: theme.text_secondary,
            strikethrough: Stroke::new(1.0_f32, theme.text_secondary),
            ..base
        },
        "sub" | "sup" => egui::TextFormat {
            color: theme.text_secondary,
            font_id: FontId::proportional(size * 0.78),
            ..base
        },
        "footnote_ref" => egui::TextFormat {
            color: theme.syn_link,
            font_id: FontId::proportional(size * 0.78),
            ..base
        },
        _ => base,
    }
}

/// `TextFormat` 的可比指纹：颜色 / 背景 / 字体族 / 字号 / 行高 / 装饰。
///
/// 与 core 侧 `theme::mapper::tests` 的同名函数逐字一致（那份是 core 私有的
/// 测试符号，跨 crate 够不着，故在此留一份拷贝）——两处若需调整须同步。
fn fmt_fingerprint(f: &egui::TextFormat) -> String {
    format!(
        "color={:?} bg={:?} font={:?} size={} line_height={:?} ital={} strike=({},{:?}) ul=({},{:?})",
        f.color,
        f.background,
        f.font_id.family,
        f.font_id.size,
        f.line_height,
        f.italics,
        f.strikethrough.width,
        f.strikethrough.color,
        f.underline.width,
        f.underline.color,
    )
}

/// 合并后，**预览区行内侧**的样式必须与合并前**逐项相同**。
///
/// 判据形态与上一条对称：抄录的 oracle 是「合并前的第二张样式表」。
#[test]
fn merged_style_table_keeps_preview_inline_behavior() {
    let theme = Theme::dark();
    let size = theme.metrics.preview_font_size;
    let line_height = size * theme.metrics.preview_line_height;
    // 预览区行内侧的样式名（= `InlineStyle` 的 14 个变体 + 基准样式）。
    let inline_styles = [
        "plain",
        "",
        "heading",
        "quote",
        "strong",
        "emphasis",
        "strikethrough",
        "code",
        "link",
        "math",
        "keycap",
        "mark",
        "task_done",
        "sub",
        "sup",
        "footnote_ref",
        // 未知样式：退回正文色。
        "unknown_xyz",
    ];
    for s in inline_styles {
        let new = rustmark::presentation::views::preview_view::test_only::span_format(
            &theme,
            s,
            size,
            line_height,
        );
        assert_eq!(
            fmt_fingerprint(&new),
            fmt_fingerprint(&legacy_inline_span_format(&theme, s, size, line_height)),
            "预览区行内样式 [{s}] 与合并前不等价"
        );
    }
}

/// 两表**共同**的样式，颜色必须**逐项相同**。
///
/// 这是方案文档 R-10 第 3 条「改一处颜色两处同步」的**核心判据**：
/// 合并前两张表各写一遍 `match`，改一处颜色语义要改两处；
/// 合并后派发只有一份 → 颜色**必然**同步。
///
/// 本用例从**两个方向**钉住：
/// - 共同样式：颜色相同（这是「同步」的含义）；
/// - 各自特有样式：颜色**不同**（否则「同步」就成了「一律相同」，
///   反而丢掉了区分——比如编辑区的 `keyword` 与预览区的 `math` 不该同色）。
#[test]
fn shared_styles_have_identical_colors_in_both_tables() {
    // 两个明暗主题各自钉一遍：syn_* 在 light/dark 下取值不同，
    // 只测 dark 会让「light 主题下有人在单侧加颜色覆盖」漏网。
    for theme in [Theme::dark(), Theme::light()] {
        let theme_name = if theme.is_dark() { "dark" } else { "light" };
        let mapper = StyleMapper::new(&theme);
        let size = theme.metrics.preview_font_size;
        let line_height = size * theme.metrics.preview_line_height;

        let common = [
            "heading",
            "quote",
            "strong",
            "emphasis",
            "strikethrough",
            "code",
            "link",
        ];
        for s in common {
            let editor = mapper.format(s);
            let preview = rustmark::presentation::views::preview_view::test_only::span_format(
                &theme,
                s,
                size,
                line_height,
            );
            assert_eq!(
                editor.color, preview.color,
                "共同样式 [{s}] 的颜色必须两表相同（{theme_name} 主题）"
            );
        }
    }
}

/// 反方向：**各自特有**的样式不该同色。
///
/// 上一条只钉「共同样式相同」，单独存在时有个漏洞：把 `span_format`
/// 改成无脑转发 `format`（退化成「一律相同」）它照样绿——而那恰恰
/// 丢掉了两区的区分。本用例与上一条合起来才是注释里说的**两个方向**。
///
/// 代表对取自 [`crate::presentation::theme::mapper::span_job`] 模块头
/// 登记的「覆盖面几乎不重叠」清单：
/// - `keyword`（编辑区语法色）vs `math`（预览行内特有）
/// - `comment`（编辑区语法色）vs `footnote_ref`（预览行内特有）
/// - `list_marker`（编辑区特有）vs `keycap`（预览行内特有）
///
/// # 例外纪律
///
/// 若将来某对在某主题下**刻意**同色，把该对从清单移出并在此登记
/// 理由——与共同样式清单的例外纪律一致，不允许无声放宽。
#[test]
fn exclusive_styles_stay_distinct_between_tables() {
    let pairs = [
        ("keyword", "math"),
        ("comment", "footnote_ref"),
        ("list_marker", "keycap"),
    ];
    for theme in [Theme::dark(), Theme::light()] {
        let theme_name = if theme.is_dark() { "dark" } else { "light" };
        let mapper = StyleMapper::new(&theme);
        let size = theme.metrics.preview_font_size;
        let line_height = size * theme.metrics.preview_line_height;
        for (editor_only, preview_only) in pairs {
            let editor = mapper.format(editor_only).color;
            let preview = rustmark::presentation::views::preview_view::test_only::span_format(
                &theme,
                preview_only,
                size,
                line_height,
            )
            .color;
            assert_ne!(
                editor, preview,
                "特有样式 [{editor_only}]（编辑区）与 [{preview_only}]（预览区）\
                 在 {theme_name} 主题同色——两表区分丢失，或该对已刻意同色（须登记）"
            );
        }
    }
}
