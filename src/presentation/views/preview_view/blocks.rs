//! 标题 / 段落 / 引用块。

use egui::Stroke;

use rustmark_core::application::dto::{ImageSpecView, InlineImageView, RangeView, SpanView};

use crate::presentation::theme::mapper::StyleMapper;

use super::inline::{Typography, render_inline_text};
use super::media::draw_image_spec;
use super::*;

/// 「文字流 + 行内图片」切分后的一个片段。
enum FlowPart<'a> {
    /// 一段文字（借用所属文本），带落在本段内的样式片段。
    Text(&'a str, Vec<SpanView>),
    /// 一张图片。
    Image(&'a ImageSpecView),
}

/// 渲染「文字流 + 混排图片」：图片按它在文字里的位置插回文本流。
///
/// # 为什么要切段
///
/// 图片是行内元素，`前面 ![a](u.png) 后面` 的图文是**同一段文字**。
/// 早期实现只要块里有图片就把整块换成图片块，于是图片前后的文字全部消失
/// （问题 032）。现在块保持本来类型，图片挂在块上（`BlockView::images`），
/// 这里按区间把文字切成若干段、在图片的位置插入图片，文字一张不丢。
///
/// # 图片为什么独占一行
///
/// 预览区的行内文本是**手工排版**（`render_inline_text` 自算 galley），
/// 没有开放的「横向流式布局」可供插入任意高度的小部件；让图片与前后文字
/// 同行需要重写整套排版。故图片按 `#pic_*` 的对齐方式单独占一行——
/// 视觉上等价于 Typora 里「图片前后有文字」的常见观感，且实现可靠。
#[allow(clippy::too_many_arguments)] // 与 `render_inline_text` 同族：在其九项上下文之上多一组「行内图片」，打包成 struct 会让每个调用方多一次所有权转换
pub(super) fn draw_text_with_images(
    vctx: &mut dyn PreviewViewAccess,
    ui: &mut Ui,
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    images: &[InlineImageView],
    typo: Typography<'_>,
    sense: egui::Sense,
    max_w: f32,
    internal_targets: &[(String, u64)],
) -> egui::Response {
    if images.is_empty() {
        return render_inline_text(
            ui,
            vctx,
            theme,
            content,
            spans,
            typo,
            sense,
            max_w,
            internal_targets,
        );
    }
    let mut last = None;
    for part in split_text_by_images(content, spans, images) {
        match part {
            FlowPart::Text(text, part_spans) => {
                // 点击语义（标题跳转）取**最后一段**文字的响应：前几段文字
                // 在图之前，用户点到图后面的字时同样期待跳转。
                last = Some(render_inline_text(
                    ui,
                    vctx,
                    theme,
                    text,
                    &part_spans,
                    typo,
                    sense,
                    max_w,
                    internal_targets,
                ));
            }
            FlowPart::Image(spec) => draw_image_spec(vctx, ui, theme, spec),
        }
    }
    // 整块只有图（文字全被图片占位吃掉）时没有文字响应，返回一个零尺寸
    // 响应，调用方的 `resp.clicked()` 判定才不会拿到悬空值。
    last.unwrap_or_else(|| {
        let rect = ui.allocate_space(egui::vec2(0.0, 0.0)).1;
        ui.interact(rect, ui.scope_id().with("flow_empty"), sense)
    })
}

/// 按图片区间把文字切成「文字段 / 图片」序列。
///
/// 切点必须落在 UTF-8 字符边界上：图片区间来自解析期的 `content.len()`
/// 快照（必然是边界），但**防御性地再对齐一次**——渲染层拿到的文本可能因
/// 增量重渲染与图片区间短暂不一致，直接切片会 panic。
fn split_text_by_images<'a>(
    content: &'a str,
    spans: &[SpanView],
    images: &'a [InlineImageView],
) -> Vec<FlowPart<'a>> {
    let mut parts: Vec<FlowPart<'a>> = Vec::new();
    let mut cursor = 0usize;
    for inline in images {
        let mut start = inline.range.start.min(content.len());
        let mut end = inline.range.end.min(content.len());
        while start > cursor && !content.is_char_boundary(start) {
            start -= 1;
        }
        while end > start && !content.is_char_boundary(end) {
            end -= 1;
        }
        if start > cursor {
            parts.push(FlowPart::Text(
                content.get(cursor..start).unwrap_or_default(),
                spans_in_range(spans, cursor, start),
            ));
        }
        parts.push(FlowPart::Image(&inline.image));
        cursor = cursor.max(end);
    }
    if cursor < content.len() {
        parts.push(FlowPart::Text(
            content.get(cursor..).unwrap_or_default(),
            spans_in_range(spans, cursor, content.len()),
        ));
    }
    parts
}

/// 取落在 `[start, end)` 内的样式片段，并把区间平移到该段自己的坐标系。
///
/// 与解析期 `trim_spans` 同源的偏移修正：片段原本相对整块 `content`，
/// 切段后若不减掉段起点，粗体 / 链接会整段偏到别的字上。
fn spans_in_range(spans: &[SpanView], start: usize, end: usize) -> Vec<SpanView> {
    spans
        .iter()
        .filter(|s| s.range.end > start && s.range.start < end)
        .map(|s| SpanView {
            range: RangeView::new(
                s.range.start.saturating_sub(start),
                s.range.end.min(end).saturating_sub(start),
            ),
            style: s.style.clone(),
            url: s.url.clone(),
        })
        .collect()
}

/// 标题：级别决定字号与留白（h1 最大，逐级递减）。
///
/// 标题**可点击**，点击后编辑区跳到对应行（FR-02 验收用例）。
/// 这是预览区唯一「点击跳转」的入口——正文段落不给点击，
/// 否则用户在预览里想选中文本时会被跳转打断。
pub(super) fn draw_heading(
    vctx: &mut dyn PreviewViewAccess,
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
    let wrap = vctx.editor_state_read().word_wrap;
    let max_w = if wrap {
        ui.available_width()
    } else {
        f32::INFINITY
    };
    let resp = draw_text_with_images(
        vctx,
        ui,
        theme,
        &block.content,
        &block.spans,
        &block.images,
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
        vctx.ui_state().sync.request_click(block.id);
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
    vctx: &mut dyn PreviewViewAccess,
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
    draw_text_with_images(
        vctx,
        ui,
        theme,
        &block.content,
        &block.spans,
        &block.images,
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
    vctx: &mut dyn PreviewViewAccess,
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
                    draw_text_with_images(
                        vctx,
                        ui,
                        theme,
                        &block.content,
                        &block.spans,
                        &block.images,
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
