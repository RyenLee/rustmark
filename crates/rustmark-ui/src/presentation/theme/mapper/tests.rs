use super::galley_cache::build_line_job;
use super::*;
use egui::Color32;
use egui::text::{LayoutJob, LayoutSection};
use rustmark_core::application::dto::{RangeView, SpanView};

/// 可用于排版的 `Context`（先跑空帧装好字库，否则 `fonts_mut` 会 panic）。
fn test_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();
    ctx
}

/// 取某段覆盖的文本（egui 0.37 的 `LayoutSection` 用 `byte_range: Range<ByteIndex>` 索引 `job.text`）。
fn section_text<'a>(job: &'a LayoutJob, s: &LayoutSection) -> &'a str {
    job.text
        .get(s.byte_range.start.0..s.byte_range.end.0)
        .unwrap_or("")
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
        url: None,
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
        url: None,
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
        url: None,
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
        url: None,
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

/// V11-16（根因回归）：空行必须排成**非零高度**的 Galley。
///
/// 见 `build_line_job` 内的说明——空行若不补一个空 section，egui 会把该行排成
/// 0 高度，连续回车产生的空行全部不可见（用户报的「连续 N 次回车只显示一个空行」），
/// 且行号栏按 0 高度空行定位会与正文错位。
#[test]
fn empty_line_job_yields_nonzero_height_galley() {
    let theme = Theme::dark();
    let job = build_line_job(&theme, "", &[], &[], 0.0);
    let ctx = test_ctx();
    let g = ctx.fonts_mut(|f| f.layout_job(job));
    assert!(
        g.rect.height() > 0.0,
        "空行必须排成非零高度，否则连续回车产生的空行不可见"
    );
    assert_eq!(g.rows.len(), 1, "空行应排成恰好一行（供光标落位）");
}

/// V11-16（根因回归）：含空行的多行文本，其整篇 Galley 高度必须随空行增长。
///
/// 修复前空行高度为 0，于是 "a\n\nb"（3 行）与 "ab"（视觉 1 行，但这里是单段）
/// 的高度并不比单行多出「两个空行」的量；修复后每个空行贡献一行高度。
#[test]
fn blank_lines_preserve_height() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();
    let single = highlight_layout(&ctx, &theme, "a", None, 900.0);
    let with_blank = highlight_layout(&ctx, &theme, "a\n\nb", None, 900.0);
    assert!(
        with_blank.rect.height() > single.rect.height() * 2.0,
        "含一个空行的文本高度必须大于单行高度的两倍（空行须贡献行高）"
    );
}

/// 提取 Galley 全部字形的 `(字符, uv_min, uv_max)` 序列（uv 为图集纹素坐标）。
fn galley_uvs(galley: &egui::Galley) -> Vec<(char, [u16; 2], [u16; 2])> {
    galley
        .rows
        .iter()
        .flat_map(|placed| placed.row.glyphs.iter())
        .map(|g| (g.chr, g.uv_rect.min, g.uv_rect.max))
        .collect()
}

/// 构造一份与编辑区同字体族的排版作业（单段、默认格式只改字体）。
fn mono_job(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::monospace(14.0),
            ..Default::default()
        },
    );
    job
}

/// V11-17 回归（根因侧）：**切换主题不得让已缓存 Galley 的字形坐标失效**。
///
/// 现象复现路径（修复前）：深色 → 浅色后，编辑区中文渲染成错码，
/// 而预览区 / 侧栏 / 工具栏一切正常。
///
/// 机理：egui 每帧 `begin_pass` 比对 `visuals.text_alpha_from_coverage`，
/// 一旦变化就**整体重建 `Fonts` 与字体图集**；`Visuals::dark()` 与
/// `Visuals::light()` 的该默认值不同（前者
/// `TwoCoverageMinusCoverageSq`、后者 `Linear`），于是深浅一切换，
/// 下一帧图集即重建。egui 只 flush 自己的 galley 缓存，应用层
/// `LINE_LAYOUT` / `WHOLE_CACHE` 持有的 `Arc<Galley>` 坐标全部失效，
/// 缓存键却未变 → 命中 → 旧 uv 采样新图集 → 乱码。
///
/// 本测试按真实帧序驱动：先排一版（模拟编辑区缓存），再
/// `apply(dark)` → 跑帧 → `apply(light)` → 跑帧，随后让**别的 UI**
/// 先排版（抢占图集前段——真实应用里侧栏 / 预览排在编辑区之前），
/// 最后重排同一份作业。若图集被重建，同一批字形会落到不同坐标。
#[test]
fn theme_switch_keeps_cached_galley_uvs_valid() {
    let ctx = test_ctx();
    let job = mono_job("编辑区缓存的中文行 editor cache line");

    // 1) 切换前先排一版：相当于编辑区缓存下来的 Galley。
    let before = ctx.fonts_mut(|f| f.layout_job(job.clone()));
    let uvs_before = galley_uvs(&before);
    assert!(!uvs_before.is_empty(), "前提：作业应产出字形");

    // 2) 与真实帧序一致的主题切换：apply 发生在帧内（begin_pass 之后），
    //    故每次 apply 后再跑一帧，让 begin_pass 读到新样式。
    apply(&ctx, &Theme::dark());
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();
    apply(&ctx, &Theme::light());
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();

    // 3) 切换后别的 UI 先排版（图集前段被抢占），再重排同一份作业。
    let _ = ctx.fonts_mut(|f| f.layout_job(mono_job("侧栏与预览先排版 other ui")));
    let after = ctx.fonts_mut(|f| f.layout_job(job));
    let uvs_after = galley_uvs(&after);

    assert_eq!(
        uvs_before, uvs_after,
        "切换主题后同一作业的字形坐标必须保持有效——\
         否则编辑区缓存 Galley 会拿旧 uv 采样新图集，中文渲染成错码（V11-17）"
    );
}

/// V11-17 回归（兜底侧）：**字体图集尺寸变化后，逐行缓存必须整篇失效**。
///
/// 即使 `text_alpha_from_coverage` 被 `apply` 钉死，egui 仍可能因
/// **图集填充率超过 80%** 重建 `Fonts`（见 `atlas_token` 的长注释）。
/// 缓存键纳入图集尺寸令牌后，此类重建同样能让缓存作废、重新取坐标。
///
/// 撑大图集用一个 200pt 的字符：字形位图随字号放大，一个就够把
/// 图集高度从初始 32 顶到 64，与字体覆盖无关、完全确定。
#[test]
fn highlight_cache_invalidates_when_atlas_size_changes() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();
    let text = "第一行 first line\n第二行 second line";

    // 第一遍：从未排版过 → 全部重排。
    let first_galley = highlight_layout(&ctx, &theme, text, None, 900.0);
    let first = last_highlight_stats();
    assert_eq!(first.total_lines, 2);
    assert_eq!(first.reshaped_lines, 2, "首排必须重排全部行");

    // 第二遍：一切未变 → 命中整文档缓存（提前 return，返回同一个 Arc）。
    // 注意：命中路径不更新 `LAST_STATS`，故用 Arc 同一性判定而非统计。
    let second_galley = highlight_layout(&ctx, &theme, text, None, 900.0);
    assert!(
        std::sync::Arc::ptr_eq(&first_galley, &second_galley),
        "空闲帧必须命中整文档缓存"
    );

    // 撑大字体图集（高度 32 → 64）→ 令牌变化。
    let size_before = ctx.fonts(|f| f.font_image_size());
    let mut big = LayoutJob::default();
    big.append(
        "M",
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::monospace(200.0),
            ..Default::default()
        },
    );
    let _ = ctx.fonts_mut(|f| f.layout_job(big));
    let size_after = ctx.fonts(|f| f.font_image_size());
    assert_ne!(size_before, size_after, "测试前提：图集必须被撑大");

    // 第三遍：令牌变了 → 必须重排（返回新的 Galley，而非复用坐标可能已失效的旧 Galley）。
    let third_galley = highlight_layout(&ctx, &theme, text, None, 900.0);
    assert!(
        !std::sync::Arc::ptr_eq(&second_galley, &third_galley),
        "图集尺寸变化后缓存必须失效重排，否则可能采样到失效的字形坐标"
    );
    let third = last_highlight_stats();
    assert!(
        third.reshaped_lines > 0,
        "图集尺寸变化后缓存必须失效重排，否则可能采样到失效的字形坐标"
    );
}

/// V11-17 回归（不变式）：图集**增长**（非重建）不会移动既有字形。
///
/// 这条钉住 `atlas_token` 的设计前提：增长是向下的追加
/// （epaint `TextureAtlas::resize_to_min_height` 只翻倍高度、保留原像素），
/// 既有 glyph 的图集位置不变——所以令牌变化引起的重排是**保守**的
/// （不会少排），且旧 Galley 在纯增长场景下本仍有效。
#[test]
fn atlas_growth_preserves_existing_glyph_positions() {
    let ctx = test_ctx();
    let job = mono_job("增长前的位置 anchor text");

    let before = ctx.fonts_mut(|f| f.layout_job(job.clone()));
    let uvs_before = galley_uvs(&before);

    // 撑大图集（同一批字形之后还会有别的字形进来）。
    let mut big = LayoutJob::default();
    big.append(
        "M",
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::monospace(200.0),
            ..Default::default()
        },
    );
    let _ = ctx.fonts_mut(|f| f.layout_job(big));

    let after = ctx.fonts_mut(|f| f.layout_job(job));
    assert_eq!(
        galley_uvs(&after),
        uvs_before,
        "图集增长不得移动既有字形的位置（令牌重排因此只是保守，不会漏排）"
    );
}

// ============================================================================
// 样式表合并（R-10 第 3 条）：两张表 → 一份派发
// ============================================================================

/// 旧 `StyleMapper::format` 的**原文抄录**（合并前的第一张样式表）。
///
/// 同上：编辑区侧的合并前快照。
fn legacy_mapper_format(theme: &Theme, style: &str) -> egui::TextFormat {
    let body_size = theme.editor_font_size;
    let base = egui::TextFormat {
        color: theme.text_primary,
        font_id: egui::FontId::monospace(body_size),
        line_height: Some(body_size * theme.editor_line_height),
        ..Default::default()
    };
    match style {
        "plain" | "" => base,
        s if s == "heading" || s.starts_with("heading:") => egui::TextFormat {
            color: theme.syn_heading,
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
            strikethrough: egui::Stroke::new(1.0_f32, theme.text_secondary),
            ..base
        },
        "code" => egui::TextFormat {
            color: theme.syn_code,
            background: theme.code_bg,
            ..base
        },
        "link" => egui::TextFormat {
            color: theme.syn_link,
            underline: egui::Stroke::new(1.0_f32, theme.syn_link),
            ..base
        },
        "image" => egui::TextFormat {
            color: theme.syn_meta,
            italics: true,
            ..base
        },
        "quote" => egui::TextFormat {
            color: theme.syn_quote,
            italics: true,
            ..base
        },
        "list_marker" | "list-marker" => egui::TextFormat {
            color: theme.syn_marker,
            ..base
        },
        "table_border" | "table-border" => egui::TextFormat {
            color: theme.syn_table,
            ..base
        },
        s if s == "task_marker" || s.starts_with("task_marker:") => egui::TextFormat {
            color: theme.accent,
            ..base
        },
        "keyword" | "operator" => egui::TextFormat {
            color: theme.syn_italic,
            ..base
        },
        "string" => egui::TextFormat {
            color: theme.success,
            ..base
        },
        "comment" => egui::TextFormat {
            color: theme.text_secondary,
            italics: true,
            ..base
        },
        "number" => egui::TextFormat {
            color: theme.warning,
            ..base
        },
        "function" => egui::TextFormat {
            color: theme.syn_code,
            ..base
        },
        "type" => egui::TextFormat {
            color: theme.syn_code,
            ..base
        },
        "variable" | "identifier" => egui::TextFormat {
            color: theme.syn_other,
            ..base
        },
        "punctuation" | "meta" | "escape" => egui::TextFormat {
            color: theme.syn_meta,
            ..base
        },
        _ => base,
    }
}

/// `TextFormat` 的可比指纹：颜色 / 背景 / 字体族 / 字号 / 行高 / 装饰。
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

/// 合并后，**编辑区侧**的样式必须与合并前**逐项相同**。
///
/// 覆盖两张旧表的**并集**：编辑区侧本来只认前 20 种，后 7 种
/// （`math` / `keycap` / `mark` / `task_done` / `sub` / `sup` /
/// `footnote_ref`）是「未知样式 → 正文色」。
///
/// 合并后共享派发认得这 7 种 → 编辑区侧**行为有变**（它们会带上颜色）。
/// 这条用例把「有变」这件事**显式钉住**，而不是让它悄悄发生。
///
/// # 那 7 种在编辑区**不可达**
///
/// 编辑区的 span 来自 `MarkdownAppService::highlight` → syntect，
/// 样式名由 `span_style_str(SpanStyle)` 产生；而 **`SpanStyle` 枚举没有**
/// `Math` / `Keycap` / `Mark` / `TaskDone` / `Subscript` / `Superscript` /
/// `FootnoteRef` 变体（那些在 `InlineStyle` 里，属预览区侧）。
/// 故编辑区**永远**不会收到这 7 个样式名 → 行为差异**不可达**。
///
/// 那为什么不干脆让编辑区侧退回旧行为？——那要在派发表里再加一层
/// 「这张表不认哪些样式」，把共享派发又拆成两半，正是本次要消除的东西。
/// 现状（认得但用不上）是**更简单**的那个。
#[test]
fn merged_style_table_keeps_editor_side_behavior_for_reachable_styles() {
    let theme = Theme::dark();
    let mapper = StyleMapper::new(&theme);
    // 编辑区侧**可能**收到的样式名（= `SpanStyle` 的 20 个变体 + 别名）。
    let editor_styles = [
        "plain",
        "",
        "heading",
        "heading:3",
        "emphasis",
        "strong",
        "strikethrough",
        "code",
        "link",
        "image",
        "quote",
        "list_marker",
        "list-marker",
        "table_border",
        "table-border",
        "task_marker",
        "task_marker:true",
        "keyword",
        "string",
        "comment",
        "number",
        "function",
        "type",
        "variable",
        "identifier",
        "operator",
        "punctuation",
        "meta",
        "escape",
        // 未知样式：退回正文色，不报错。
        "unknown_xyz",
    ];
    for s in editor_styles {
        assert_eq!(
            fmt_fingerprint(&mapper.format(s)),
            fmt_fingerprint(&legacy_mapper_format(&theme, s)),
            "编辑区样式 [{s}] 与合并前不等价"
        );
    }
}
