//! 标题 / 段落 / 引用块。

use egui::Stroke;

use crate::presentation::theme::mapper::StyleMapper;

use super::inline::{Typography, render_inline_text};
use super::*;

/// 标题：级别决定字号与留白（h1 最大，逐级递减）。
///
/// 标题**可点击**，点击后编辑区跳到对应行（FR-02 验收用例）。
/// 这是预览区唯一「点击跳转」的入口——正文段落不给点击，
/// 否则用户在预览里想选中文本时会被跳转打断。
pub(super) fn draw_heading(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    internal_targets: &[(String, u64)],
) {
    let m = theme.metrics;
    let level = block.level.unwrap_or(1).clamp(1, 6);
    let size = m.preview_font_size * StyleMapper::heading_scale(level);
    // 级别越小上方留白越多：层级感不只靠字号，也靠间距。
    // 刻度取值：h1 一个 space_xl、h2 一个 space_lg、h3 一个 space_md、
    // 其余一个 space_sm——层级差恒为「一格」，节奏可预期。
    ui.add_space(match level {
        1 => m.space_xl,
        2 => m.space_lg,
        3 => m.space_md,
        _ => m.space_sm,
    });
    // 行内样式（`**粗体**` / `` `代码` `` / `<kbd>` …）：有片段时走富文本路径，
    // 没有则走原来的单标签路径。两条路径都带点击感应用于「点击标题跳转」。
    // 标题随正文一起跟随「自动换行」开关。
    // 正文里的链接 / 脚注引用也在此获得点击能力（`internal_targets` 提供跳转目标）。
    let wrap = app.editor.word_wrap;
    let max_w = if wrap {
        ui.available_width()
    } else {
        f32::INFINITY
    };
    let resp = render_inline_text(
        ui,
        app,
        theme,
        &block.content,
        &block.spans,
        Typography {
            size,
            line_height: size * theme.metrics.preview_line_height,
            base_style: "heading",
        },
        egui::Sense::click(),
        max_w,
        internal_targets,
    );
    if resp.clicked() {
        app.ui.sync.request_click(block.id);
    }
    if resp.hovered() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
    }
    // h1 / h2 下方加一条淡分隔（对齐原型观感）。
    if level <= 2 {
        ui.add_space(m.space_xxs);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter()
            .hline(rect.x_range(), rect.top(), hairline(theme.border));
    }
}

/// 段落：按可用宽度折行；行内样式（粗体 / 行内代码 / 键帽 / 高亮 / 上下标）
/// 由 [`render_inline_text`] 按片段排版。
pub(super) fn draw_paragraph(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    wrap: bool,
    internal_targets: &[(String, u64)],
) {
    let size = theme.metrics.preview_font_size;
    let max_w = if wrap {
        ui.available_width()
    } else {
        f32::INFINITY
    };
    render_inline_text(
        ui,
        app,
        theme,
        &block.content,
        &block.spans,
        Typography {
            size,
            line_height: size * theme.metrics.preview_line_height,
            base_style: "",
        },
        egui::Sense::hover(),
        max_w,
        internal_targets,
    );
}

/// 引用块：左侧色条 + 斜体次级色。
///
/// 引用帧会吞掉内层段落（见解析器 `OpenBlock::absorbs`），正文（含多段落
/// 与行内样式）都回流在 `content` 里，画在色条右侧的框内；`content` 为空
/// 的场合（理论上只剩空引用 `>`）只画色条。
pub(super) fn draw_quote(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    wrap: bool,
    internal_targets: &[(String, u64)],
) {
    egui::Frame::new()
        .fill(theme.bg_tertiary)
        .inner_margin(egui::Margin::symmetric(
            theme.metrics.pane_padding as i8,
            theme.metrics.space_sm as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(QUOTE_BAR_W, ui.available_height()),
                    egui::Sense::hover(),
                );
                ui.painter().vline(
                    rect.center().x,
                    rect.y_range(),
                    Stroke::new(QUOTE_BAR_W, theme.syn_quote),
                );
                if !block.content.trim().is_empty() {
                    let max_w = if wrap {
                        ui.available_width()
                    } else {
                        f32::INFINITY
                    };
                    render_inline_text(
                        ui,
                        app,
                        theme,
                        &block.content,
                        &block.spans,
                        Typography {
                            size: theme.metrics.preview_font_size,
                            line_height: theme.metrics.preview_font_size
                                * theme.metrics.preview_line_height,
                            base_style: "quote",
                        },
                        egui::Sense::hover(),
                        max_w,
                        internal_targets,
                    );
                }
            });
        });
}
