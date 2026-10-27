//! 代码块：语言标签 + 复制按钮 + 语法高亮（含可单测的 `LayoutJob` 构造）。

use egui::FontId;

use crate::presentation::theme::mapper::{SpanPolicy, StyleMapper, build_job};
use rustmark_core::application::markdown::queries::HighlightSnippetQuery;

use super::*;

/// 代码块：语言标签 + 复制按钮 + 语法高亮。
///
/// `wrap` 跟随「自动换行」开关：开 → 代码随面板宽度折行，不再撑出预览区
/// 横向滚动条（问题 2）；关 → 保持一行不折，由预览区底部横向滚动条查看
/// （与编辑区行为一致，长代码行折断会破坏缩进可读性）。
pub(super) fn draw_code_block(
    vctx: &mut dyn EditorViewContext,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    wrap: bool,
) {
    let lang = block
        .lang
        .as_deref()
        .filter(|l| !l.is_empty())
        .unwrap_or(CODE_NO_LANG);

    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(theme.metrics.pane_padding as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(lang)
                        .size(theme.metrics.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });

            // 语法高亮走与编辑区同一个高亮器，保证两处配色一致。
            let spans = vctx
                .markdown_service()
                .highlight_snippet(HighlightSnippetQuery {
                    text: block.content.clone(),
                    lang: block.lang.clone(),
                })
                .ok();
            draw_highlighted_code(
                ui,
                theme,
                &block.content,
                spans.as_deref(),
                code_wrap_width(ui, wrap),
            );
        });
}

/// 代码块的排版宽度：开「自动换行」时取面板可用宽（`Frame` 的内边距已扣过），
/// 关闭时取 `INFINITY`（不折行）。
fn code_wrap_width(ui: &Ui, wrap: bool) -> f32 {
    if wrap {
        ui.available_width().max(0.0)
    } else {
        f32::INFINITY
    }
}

/// 把带 span 的代码文本画成彩色文本（无 span 时退化为等宽纯色）。
pub(super) fn draw_highlighted_code(
    ui: &mut Ui,
    theme: &Theme,
    text: &str,
    spans: Option<&[rustmark_core::application::dto::SpanView]>,
    wrap_width: f32,
) {
    let job = highlighted_code_job(theme, text, spans, wrap_width);

    // 唯一 id 保证每个代码块的选择状态互相独立。
    ui.add(egui::Label::new(job).selectable(true));
}

/// 代码块文本 → 带语法色的 `LayoutJob`（纯函数）。
///
/// 从 [`draw_highlighted_code`] 里抽出来是为了**可测**：后者需要 `&mut Ui`，
/// 无头环境构造不出来；而「span 边界落在字符中间」这类缺陷（DEF-04）
/// 恰恰必须能被断言钉住，不能只靠真机偶遇。
///
/// `wrap_width` 为排版最大宽度：传入预览区可用宽度即可让代码随区域宽度折行，
/// 不再溢出窄面板。
///
/// # 本函数是「预览区代码块那一侧」的薄封装
///
/// 切段逻辑已收敛到 [`crate::presentation::theme::mapper::build_job`]（R-10）；
/// 本函数只负责「预览区代码块用哪套样式」——即
/// [`StyleMapper::format`] 的颜色，但**字号与字体**换成预览区等宽字体。
pub(super) fn highlighted_code_job(
    theme: &Theme,
    text: &str,
    spans: Option<&[rustmark_core::application::dto::SpanView]>,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let mapper = StyleMapper::new(theme);
    let font = FontId::monospace(theme.metrics.preview_font_size);

    // 基础样式：`plain` 的颜色 + 预览等宽字体。
    let mut base = mapper.format("plain");
    base.font_id = font.clone();
    // 覆盖字体：预览区代码块一律用等宽（预览字号），
    // 与编辑区的「等宽（编辑区字号）」只差字号。
    let style_of = move |style: &str| {
        let mut fmt = mapper.format(style);
        fmt.font_id = font.clone();
        fmt
    };

    // span 需按起点升序（统一构造器的顺序游标假定如此）。
    let mut sorted: Vec<_> = spans.map(|s| s.to_vec()).unwrap_or_default();
    sorted.sort_by_key(|s| s.range.start);

    let policy = SpanPolicy::cursor(base, wrap_width, style_of, None);
    let (job, _hits) = build_job(theme, text, &sorted, &policy, |_| false);
    job
}
