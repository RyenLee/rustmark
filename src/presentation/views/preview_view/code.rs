//! 代码块：语言标签 + 复制按钮 + 语法高亮（含可单测的 `LayoutJob` 构造）。

use egui::FontId;

use crate::application::markdown::queries::HighlightSnippetQuery;
use crate::presentation::theme::mapper::StyleMapper;
use crate::shared::geometry::clamp_to_char_boundaries;

use super::*;

/// 代码块：语言标签 + 复制按钮 + 语法高亮。
///
/// `wrap` 跟随「自动换行」开关：开 → 代码随面板宽度折行，不再撑出预览区
/// 横向滚动条（问题 2）；关 → 保持一行不折，由预览区底部横向滚动条查看
/// （与编辑区行为一致，长代码行折断会破坏缩进可读性）。
pub(super) fn draw_code_block(
    app: &mut EditorApp,
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
            let spans = app
                .services
                .markdown
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
    spans: Option<&[crate::application::dto::SpanView]>,
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
pub(super) fn highlighted_code_job(
    theme: &Theme,
    text: &str,
    spans: Option<&[crate::application::dto::SpanView]>,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let mapper = StyleMapper::new(theme);
    let base = mapper.format("plain");
    let font = FontId::monospace(theme.metrics.preview_font_size);

    let mut job = egui::text::LayoutJob::default();
    // 按可用宽度折行；宽度为 0 或负时退化为不折行（单行长排）。
    job.wrap.max_width = if wrap_width > 0.0 {
        wrap_width
    } else {
        f32::INFINITY
    };
    let mut cursor = 0usize;
    if let Some(spans) = spans {
        let mut sorted: Vec<_> = spans.to_vec();
        sorted.sort_by_key(|s| s.range.start);
        for span in &sorted {
            // 边界吸附到字符边界（DEF-04）：span 坐标由解析器给出，与 `text`
            // 未必同源；落在多字节字符中间时 `&text[..]` 会 panic，
            // 而本项目 `panic = "abort"`。吸附后空区间直接跳过（少一段色而已）。
            let Some((start, end)) =
                clamp_to_char_boundaries(text, span.range.start, span.range.end)
            else {
                continue;
            };
            if start > cursor {
                job.append(&text[cursor..start], 0.0, base.clone());
            }
            let mut fmt = mapper.format(&span.style);
            fmt.font_id = font.clone();
            job.append(&text[start..end], 0.0, fmt);
            cursor = end;
        }
    }
    if cursor < text.len() {
        let mut fmt = base;
        fmt.font_id = font;
        job.append(&text[cursor..], 0.0, fmt);
    }
    job
}
