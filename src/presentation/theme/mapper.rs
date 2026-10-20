//! 样式映射：领域语义 → egui 呈现（阶段 06 · S6-10）。
//!
//! # 职责边界（这是 S5-03 那个决策的兑现）
//!
//! S5-03 决定**不用** syntect 的主题，改用领域自有的 `SpanStyle` 枚举，
//! 因为「颜色」属于表现层，不该泄漏进领域。本模块就是那个边界的具体实现：
//!
//! ```text
//! syntect scope ──► SpanStyle（领域，无颜色）
//!                        │  应用层转成字符串
//!                        ▼
//!                 style字符串 ──► **本模块** ──► egui TextFormat
//! ```
//!
//! 好处：换主题只改 [`Theme`]，不需要碰 syntect、不需要重新解析、
//! 也不需要把主题包塞进二进制。
//!
//! # 字符串契约
//!
//! 映射的输入是**字符串**（[`crate::application::dto::SpanView::style`]），
//! 而不是领域类型——因为表现层不得依赖 `domain`。
//! 取值来自 `MarkdownAppService::span_style_str`，共 21 种；
//! 本模块的 [`StyleMapper::format`] 是它们的**唯一**解释处。
//!
//! # 模块结构
//!
//! - [`StyleMapper`] / [`apply`]：样式字符串 → [`egui::TextFormat`]，以及把主题写入
//!   egui 全局样式（本文件）。
//! - `squiggle`：拼写错词波浪线与查找命中标记的纯几何 + 绘制。
//! - `highlight_cache`：语法高亮两层缓存（整文档 / 逐行）、缓存签名与单行排版作业。
//! - `highlight_layout`：逐行增量排版与整文档 `Galley` 拼接。
//! - `span_job`：**唯一**的 span → [`egui::text::LayoutJob`] 构造器（R-10）；
//!   编辑区 / 预览区代码块 / 预览区行内三处共用，差异由 `SpanPolicy` 显式表达。

mod highlight_cache;
mod highlight_layout;
mod span_job;
mod squiggle;

#[cfg(test)]
mod tests;

pub use highlight_cache::{
    HighlightStats, atlas_token, clear_highlight_caches, last_highlight_stats, line_cache_len,
    line_tops,
};
pub use highlight_layout::{highlight_layout, highlight_layout_with_find};
pub use span_job::{SpanHit, SpanPolicy, build_job};
pub use squiggle::{FindMark, SpellMark, Squiggle, paint_squiggles, squiggles_for};

/// 编辑区那一侧的单行 `LayoutJob` 构造（薄封装，切段逻辑见 [`build_job`]）。
///
/// 只给**同 crate 内**的测试用：真帧对拍要把它与预览区代码块路径
/// 放在同一帧里渲染并比对颜色序列，而预览区路径住在 `presentation::views`。
/// 生产代码请走 [`highlight_layout`]（它内部会调这个）。
#[cfg(test)]
pub(crate) use highlight_cache::build_line_job;

use egui::{Color32, TextFormat};

use super::builtin::Theme;

/// 语义样式 → egui 文本格式。
///
/// 未知样式**不报错**，退回普通文本色：新增一个 `SpanStyle` 变体时，
/// 表现层最多「不高亮」，不会「崩溃」或「整块变黑」。
#[derive(Clone, Copy, Debug)]
pub struct StyleMapper<'a> {
    theme: &'a Theme,
}

/// 标题级别 → 字号倍率。
///
/// 与原型 `PreviewPane.vue` 的观感对齐：`h1` 最大，逐级递减，
/// `h6` 仍略大于正文（否则大纲里的六级标题在预览里全都一样大）。
const fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.80,
        2 => 1.50,
        3 => 1.30,
        4 => 1.15,
        5 => 1.05,
        _ => 1.0,
    }
}

impl<'a> StyleMapper<'a> {
    /// 以主题构造。
    pub const fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    /// 样式字符串 → 文本格式。
    ///
    /// `heading` 的级别信息编码在字符串里（`heading:3`），
    /// 由应用层的 `SpanStyle::Heading { level }` 格式化而来。
    ///
    /// # 为什么编辑区**不**按级别放大字号
    ///
    /// 编辑区是「源码视图」：每一行的**行高必须一致**，行号才能与正文逐行对齐。
    /// 早期实现让标题行用 `body_size × heading_scale(level)`，于是标题行比正文行
    /// 高出一大截，行号、滚动同步、光标命中都随之错位（行距不再等距）。
    /// 层级信息改由**颜色**（[`Theme::syn_heading`]）表达——这也是主流编辑器
    /// （VS Code / Typora 源码模式）的做法：编辑区里标题只是换色，不换字号。
    /// 预览区才用 [`StyleMapper::heading_scale`] 放大（那是排版视图，见 `preview_view`）。
    ///
    /// 行高统一取 [`Theme::editor_line_height`]（外观设置里的「行高」）乘字号：
    /// `TextFormat::line_height` 一旦给出，epaint 就**不再**按字体度量算行高，
    /// 于是中英文混排、系统字体回退（CJK 行高天然高于等宽数字）都不会破坏等距——
    /// 这是「所有内容等高」得以成立的机制。
    ///
    pub fn format(&self, style: &str) -> TextFormat {
        let body_size = self.theme.editor_font_size;
        let base = TextFormat {
            color: self.theme.text_primary,
            font_id: egui::FontId::monospace(body_size),
            line_height: Some(body_size * self.theme.editor_line_height),
            ..Default::default()
        };

        match style {
            "plain" | "" => base,

            // —— Markdown 语义 ——
            // 标题只换色不换字号（见上方文档注释）：行高与正文严格相等。
            s if s == "heading" || s.starts_with("heading:") => TextFormat {
                color: self.theme.syn_heading,
                ..base
            },
            "strong" => TextFormat {
                color: self.theme.syn_bold,
                ..base
            },
            "emphasis" => TextFormat {
                color: self.theme.syn_italic,
                italics: true,
                ..base
            },
            "strikethrough" => TextFormat {
                color: self.theme.text_secondary,
                strikethrough: hairline(self.theme.text_secondary),
                ..base
            },
            "code" => TextFormat {
                color: self.theme.syn_code,
                background: self.theme.code_bg,
                ..base
            },
            "link" => TextFormat {
                color: self.theme.syn_link,
                underline: hairline(self.theme.syn_link),
                ..base
            },
            "image" => TextFormat {
                color: self.theme.syn_meta,
                italics: true,
                ..base
            },
            "quote" => TextFormat {
                color: self.theme.syn_quote,
                italics: true,
                ..base
            },
            "list_marker" | "list-marker" => TextFormat {
                color: self.theme.syn_marker,
                ..base
            },
            "table_border" | "table-border" => TextFormat {
                color: self.theme.syn_table,
                ..base
            },
            s if s == "task_marker" || s.starts_with("task_marker:") => TextFormat {
                color: self.theme.accent,
                ..base
            },

            // —— 代码语法（来自 syntect scope 映射）——
            "keyword" | "operator" => TextFormat {
                color: self.theme.syn_italic,
                ..base
            },
            "string" => TextFormat {
                color: self.theme.success,
                ..base
            },
            "comment" => TextFormat {
                color: self.theme.text_secondary,
                italics: true,
                ..base
            },
            "number" => TextFormat {
                color: self.theme.warning,
                ..base
            },
            "function" => TextFormat {
                color: self.theme.syn_code,
                ..base
            },
            "type" => TextFormat {
                color: self.theme.syn_code,
                ..base
            },
            "variable" | "identifier" => TextFormat {
                color: self.theme.syn_other,
                ..base
            },
            "punctuation" | "meta" | "escape" => TextFormat {
                color: self.theme.syn_meta,
                ..base
            },

            // 未知样式：退回正文色，不报错（见类型文档）。
            _ => base,
        }
    }

    /// 样式 → 颜色（只要颜色时用；等价于 `format(..).color`）。
    pub fn color(&self, style: &str) -> Color32 {
        self.format(style).color
    }

    /// 标题级别的字号倍率（供预览区排版）。
    pub fn heading_scale(level: u8) -> f32 {
        heading_scale(level)
    }

    /// 预览区某类块的前景色。
    ///
    /// `kind` 里带后缀的形式（如 `raw:html`）按前缀匹配。
    pub fn block_color(&self, kind: &str) -> Color32 {
        if kind == "heading" || kind.starts_with("heading:") {
            self.theme.syn_heading
        } else if kind.starts_with("quote") {
            self.theme.syn_quote
        } else {
            self.theme.text_primary
        }
    }

    /// 标题样式字符串 → 级别（供预览区决定排版）。
    ///
    /// 块级字符串只有 `"heading"`（`BlockView::kind` 不带级别），
    /// 片段级才有 `heading:N`。
    pub fn heading_level_of(style: &str) -> Option<u8> {
        if style == "heading" {
            return Some(1);
        }
        style
            .strip_prefix("heading:")
            .and_then(|n| n.parse::<u8>().ok())
    }
}

/// 把主题写进 egui 的全局样式（字体、背景、控件配色）。
///
/// 这是「切换主题无需重启」的落点：egui 的 `Context::style_mut` 是全局的，
/// 每帧应用一次当前主题即可，窗口不必重建。
///
/// # 为什么从 `Visuals::dark()` / `Visuals::light()` 起步
///
/// 早先的实现是在「上一帧的 `visuals`」上逐字段覆盖。问题在于 `ctx.style()`
/// 的基线是 egui 默认的 `Visuals::dark()`：只要有一个字段忘了覆盖，深色默认值
/// 就会被带进浅色主题。`text_cursor` 正是这样一个字段——它的默认 caret 是浅蓝
/// `rgb(192, 222, 255)`，落在白色画布上几乎不可见，用户看到的就是「聚焦后
/// 看不见光标」。同类字段还有 `override_text_color`、`window_stroke` 等。
///
/// 改为按明暗模式取基线后，两个方向都不会再互相污染，可读性也不再依赖
/// 「记得把每个字段都覆盖一遍」这种易碎前提。
pub fn apply(ctx: &egui::Context, theme: &Theme) {
    // egui 0.37 起样式是**按明暗模式分开存**的：`ctx.style_of(theme)` /
    // `ctx.set_style_of(theme, …)`。切换 `Context::theme` 时 egui 取的是另一份，
    // 故这里按本应用当前的明暗模式取、也按同一模式写回——与上面「按明暗取基线」
    // 的思路一致，避免深浅两份样式互相污染。
    let egui_theme = if theme.is_dark() {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    let mut style = (*ctx.style_of(egui_theme)).clone();

    let mut visuals = if theme.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    visuals.dark_mode = theme.is_dark();
    // `text_options.color_transfer_function` 必须**钉死**，不能随明暗模式取
    // egui 的默认值。
    //
    // 这是 V11-17「切换主题后编辑区乱码」的真正根因：egui 每帧 `begin_pass`
    // 都会把字体选项与上一帧比对，**一旦变化就整体重建 `Fonts`（连字体图集
    // 一起）**（`epaint::text::Fonts::begin_pass`：`options != 上次` →
    // `galley_caches.clear()`）。而 `Visuals::dark()` 的默认值是
    // `TwoCoverageMinusCoverageSq`、`Visuals::light()` 的是 `Linear`——
    // 深浅主题一切换这个值就变，下一帧图集即被重建。
    //
    // egui 自己只 flush 它内部的 galley 缓存；本应用**应用层**的
    // `LINE_LAYOUT` / `WHOLE_CACHE`（S6-04 增量高亮）持有跨帧的
    // `Arc<Galley>`，其字形 `uv_rect` 指向旧图集。重建后这些坐标全部失效，
    // 而缓存键（配色签名 / 文本 / 换行宽度 / 度量）都没变 → 缓存命中 →
    // 拿旧 uv 去采样新图集 → **中文渲染成错码**。预览区每帧重排故不受影响，
    // 侧栏 / 工具栏同样每帧重排——所以乱码只出现在编辑区（V11-17 截图）。
    //
    // 钉成 egui 启动时的默认值（`Style::default()` → `Visuals::dark()` 的
    // `TwoCoverageMinusCoverageSq`）：应用生命周期内该值永不变化，
    // `begin_pass` 永远不会因它重建 Fonts。
    // （图集填充率 >80% 导致的重建由 `highlight_cache::atlas_token`
    // 纳入缓存键兜底，见那儿的长注释。）
    //
    // 0.37 起该字段从 `Visuals::text_alpha_from_coverage` 迁到了
    // `Visuals::text_options.color_transfer_function`（类型也由
    // `epaint::AlphaFromCoverage` 改名为 `FontColorTransferFunction`）——
    // **明暗两份默认值的差异依旧存在**，这条钉死不能随升级一起删掉。
    visuals.text_options.color_transfer_function =
        egui::epaint::image::FontColorTransferFunction::TwoCoverageMinusCoverageSq;
    visuals.panel_fill = theme.bg_secondary;
    visuals.window_fill = theme.bg_secondary;
    visuals.extreme_bg_color = theme.bg_primary;
    visuals.faint_bg_color = theme.bg_tertiary;
    visuals.code_bg_color = theme.code_bg;
    visuals.hyperlink_color = theme.accent;

    // 选区：底色 + **选区内的文字色**。
    //
    // egui 会把被选中文字的顶点色强制改写成 `selection.stroke.color`
    // （见 `text_selection::visuals::paint_text_selection`）。若这里给强调色，
    // 选中文字就变成「强调色压在强调色底上」——浅色主题下几乎糊成一片。
    // 故选区文字固定用正文色，保证**选中态**同样清晰可读。
    visuals.selection.bg_fill = theme.selection_bg;
    visuals.selection.stroke = hairline(theme.text_primary);

    // 光标：显式按主题指定，不再依赖 egui 的「深色模式专用」默认值。
    visuals.text_cursor.stroke = egui::Stroke::new(2.0_f32, theme.accent);

    // 次要文字色显式给出，避免默认的「对正文色二次降亮」把次要文字压到不可读。
    visuals.weak_text_color = Some(theme.text_secondary);

    // 控件圆角统一到原型的 `--radius-btn`（2026-10 精修后为 6px）。
    let button_radius =
        egui::CornerRadius::same(theme.metrics.radius_button.round().clamp(0.0, 255.0) as u8);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = button_radius;
    }

    visuals.widgets.noninteractive.bg_stroke = hairline(theme.border);
    visuals.widgets.noninteractive.weak_bg_fill = theme.bg_tertiary;
    visuals.widgets.noninteractive.bg_fill = theme.bg_tertiary;
    visuals.widgets.noninteractive.fg_stroke = hairline(theme.text_secondary);
    visuals.widgets.inactive.bg_fill = theme.bg_tertiary;
    visuals.widgets.inactive.weak_bg_fill = theme.bg_tertiary;
    visuals.widgets.inactive.bg_stroke = hairline(theme.border);
    visuals.widgets.inactive.fg_stroke = hairline(theme.text_primary);
    visuals.widgets.hovered.bg_fill = theme.bg_tertiary;
    visuals.widgets.hovered.weak_bg_fill = theme.accent_soft;
    visuals.widgets.hovered.bg_stroke = hairline(theme.accent);
    visuals.widgets.hovered.fg_stroke = hairline(theme.text_primary);
    visuals.widgets.active.bg_fill = theme.accent_soft;
    visuals.widgets.active.weak_bg_fill = theme.accent_soft;
    visuals.widgets.active.bg_stroke = hairline(theme.accent);
    visuals.widgets.active.fg_stroke = hairline(theme.text_primary);
    visuals.widgets.open.bg_fill = theme.bg_tertiary;
    visuals.widgets.open.bg_stroke = hairline(theme.border);
    visuals.widgets.open.fg_stroke = hairline(theme.text_primary);
    visuals.error_fg_color = theme.error;
    visuals.warn_fg_color = theme.warning;

    style.visuals = visuals;

    // 间距：原型栅格以 4px 为基准（`--sp-1`）。把 egui 的控件间距与按钮内边距
    // 收敛到同一套节奏，避免桌面端比原型「松/紧一档」。
    // 两个值都从 [`Metrics`] 的间距刻度取——全局默认值与各视图的显式留白
    // 必须是同一个来源，否则「统一间距规范」只管得住显式调用、管不住默认值。
    let m = &theme.metrics;
    style.spacing.item_spacing = egui::vec2(m.space_md, m.space_xs);
    style.spacing.button_padding = egui::vec2(m.space_md, m.space_xs);

    // 字号：编辑区用等宽，其余用 UI 字体。
    for (font, size) in [
        (egui::TextStyle::Monospace, theme.editor_font_size),
        (egui::TextStyle::Body, theme.metrics.preview_font_size),
        (egui::TextStyle::Button, theme.metrics.ui_font_size),
        (
            egui::TextStyle::Small,
            (theme.metrics.ui_font_size - 1.0).max(8.0),
        ),
        (
            egui::TextStyle::Heading,
            theme.metrics.preview_font_size * 1.4,
        ),
    ] {
        // `Style::text_styles` 的值是 `FontId`（字段 `size` / `family`），
        // 不是独立结构体。保留 egui 调校过的其它设置，只改字号与字体族。
        let mut font_id = style.text_styles.get(&font).cloned().unwrap_or_default();
        font_id.size = size;
        if font == egui::TextStyle::Monospace {
            font_id.family = egui::FontFamily::Monospace;
        }
        style.text_styles.insert(font, font_id);
    }

    // egui 0.37 把样式按明暗分存：`set_style_of(theme, style)` 只影响该主题；
    // 同时必须把**当前活动主题**设成本应用正在套用的明暗，否则 egui 会用
    // `Context::theme_preference` 解析出的那一版样式，而非我们这里算好的。
    // `set_theme` 只是写入选项、幂等、不触发图集重建，每帧调用安全。
    ctx.set_theme(egui_theme);
    ctx.set_style_of(egui_theme, style);
}

/// 1px 实线描边（发丝线）。
///
/// 全文件统一走这里，避免每个 `Stroke::new(1.0, ...)` 都触发一次
/// `f32`/`f64` 字面量推断的回退警告。
fn hairline(color: egui::Color32) -> egui::Stroke {
    egui::Stroke::new(1.0_f32, color)
}
