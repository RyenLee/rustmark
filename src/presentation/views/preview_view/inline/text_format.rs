//! 行内样式 → `TextFormat`、排版参数 `Typography` 与「已完成」装饰。
//!
//! 这部分只决定「预览区这一侧的行内度量」（比例字体、预览字号、预览行高、
//! 1.0 线条），不碰排版与绘制——后者在 [`super::text_render`]。

use egui::text::TextFormat;
use egui::{FontId, Stroke};

use crate::presentation::theme::builtin::Theme;
use crate::presentation::theme::mapper::StyleSemantics;

/// 行内样式 → `TextFormat`（预览字号体系，与编辑区度量无关）。
///
/// 刻意**不复用** `StyleMapper::format`：那个函数的字号取自
/// `theme.editor_font_size`（编辑区设置），且带编辑区的行高——直接拿来渲染预览
/// 会让预览字号跟随编辑区设置漂移。这里显式用预览字号族，并统一给出
/// `line_height`，使同一段文本里的不同样式**不改变行高**（上下标变小也不撑行）。
///
/// # 样式名 → 语义的派发已收敛（R-10）
///
/// 本函数此前是**第二张**样式表，与 [`StyleMapper::format`] 各写一遍
/// 「样式名 → 颜色 / 斜体 / 下划线 / 删除线 / 背景」的 `match`。
/// 现改为：派发走共享的 [`StyleSemantics::of`]，本函数只负责
/// **预览区这一侧的度量**（比例字体、预览字号、预览行高、1.0 线条）。
///
/// 实测依据（探针 2026-10-20）：两表**共同**的 7 种样式
/// （`heading` / `quote` / `strong` / `emphasis` / `strikethrough` /
/// `code` / `link`）颜色本就**完全相同**（同源于 `theme.syn_*`），
/// 故合并后**用户可见效果不变**——这条由 `style_tables_keep_their_look`
/// （`mapper/tests.rs`）对照合并前的快照钉住。
/// 可见性为 `pub(crate)` 而非 `pub(super)`：入口 [`super`] 再导出给
/// `preview_view` 的子模块（blocks/table/toc/tests）与跨模块单测。
#[doc(hidden)]
pub fn span_format(theme: &Theme, style: &str, size: f32, line_height: f32) -> TextFormat {
    let base = TextFormat {
        color: theme.text_primary,
        font_id: FontId::proportional(size),
        line_height: Some(line_height),
        ..Default::default()
    };
    let sem = StyleSemantics::of(theme, style);
    // 预览区：比例字体为正文，行内代码 / 键帽用等宽；
    // 上下标 / 脚注引用按 `size_scale` 缩小（行高仍取 `base` 的，故不撑行）。
    let font = if sem.monospace {
        FontId::monospace(size * sem.size_scale)
    } else if sem.size_scale != 1.0 {
        FontId::proportional(size * sem.size_scale)
    } else {
        base.font_id
    };
    // `TextFormat` 的下划线 / 删除线是 `Stroke`（不是 `Option`）：
    // 「不画」= 零宽 `Stroke`（颜色也置为透明）。
    let stroke = |c: Option<egui::Color32>| match c {
        Some(c) => Stroke::new(1.0_f32, c),
        None => Stroke::new(0.0_f32, egui::Color32::TRANSPARENT),
    };
    TextFormat {
        color: sem.color,
        italics: sem.italics,
        underline: stroke(sem.underline),
        strikethrough: stroke(sem.strikethrough),
        background: sem.background.unwrap_or(egui::Color32::TRANSPARENT),
        font_id: font,
        line_height: base.line_height,
        ..Default::default()
    }
}

/// 一段行内文本的排版参数：字号、行高、基准样式。
///
/// 三者总是一起决定 `TextFormat`（字号与行高还必须成对——行高一旦给出，
/// epaint 就不再按字体度量算行高），打包传递可避免把行高错配给另一个字号。
#[derive(Clone, Copy)]
pub(crate) struct Typography<'a> {
    pub(crate) size: f32,
    pub(crate) line_height: f32,
    pub(crate) base_style: &'a str,
}

/// 给一个行内格式叠上「已完成」装饰（次级色 + 删除线）。
pub(crate) fn done_overlay(theme: &Theme, mut fmt: TextFormat) -> TextFormat {
    fmt.color = theme.text_secondary;
    fmt.strikethrough = Stroke::new(1.0_f32, theme.text_secondary);
    fmt
}
