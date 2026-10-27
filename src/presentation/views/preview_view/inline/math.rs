//! 含行内公式的段落：分段流式排版（文本段交 epaint、公式段按基线拼回文字流）。
//!
//! `LayoutJob` 只能排**文字**：epaint 0.33 既没有行内占位段，也没有能插进
//! 文字流的绘制挂点。于是含 `$...$` 的段落改走这里的**分段流式**排版——
//! 文本段照旧交给 epaint（内部折行不变），公式段用 [`crate::presentation::views::math`]
//! 引擎二维排版后按**基线**拼回文字流，正是官方 KaTeX 的观感。

use egui::epaint::text::PlacedRow;
use egui::{Align2, Color32, FontId, Galley, Pos2, Rect, Ui, Vec2};

use crate::app::frame::EditorApp;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::views::math::{self, MathLayout, Measure};
use rustmark_core::application::dto::{RangeView, SpanView};
use rustmark_core::shared::geometry::clamp_to_char_boundaries;

use super::delimiters::strip_math_delimiters;
use super::text_format::{Typography, span_format};
use super::text_render::build_text_job;

/// 流式排版的一个片段。
pub(crate) enum Piece {
    /// 一段纯文本（span 区间相对本段起点；为空表示无样式）。
    Text { text: String, spans: Vec<SpanView> },
    /// 排版成功的行内公式（版面已按正文字号排好）。
    Math(MathLayout),
}

/// 一条绘制指令（坐标相对段落原点，最后统一平移到分配矩形）。
pub(crate) enum Cmd {
    Galley {
        galley: std::sync::Arc<Galley>,
        pos: Pos2,
    },
    Math {
        layout: MathLayout,
        rect: Rect,
    },
}

/// 把段落按公式 span 切成「文本段 / 公式段」。
///
/// 公式排版失败时**降级成文本段**（斜体 + 剥掉 `$`，与旧行为一致）：
/// 失败不该让整段消失，而半句话 + 半源码比整段源码更接近用户预期。
pub(crate) fn build_pieces(
    content: &str,
    spans: &[SpanView],
    size: f32,
    measure: Measure<'_>,
) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut text = String::new();
    let mut text_spans: Vec<SpanView> = Vec::new();
    let len = content.len();
    let mut cursor = 0usize;

    for span in spans {
        let Some((start, end)) =
            clamp_to_char_boundaries(content, span.range.start.min(len), span.range.end.min(len))
        else {
            continue;
        };
        let start = start.max(cursor);
        let end = end.max(start);
        if start > cursor {
            text.push_str(&content[cursor..start]);
        }
        if span.style == "math" {
            if !text.is_empty() {
                pieces.push(Piece::Text {
                    text: std::mem::take(&mut text),
                    spans: std::mem::take(&mut text_spans),
                });
            }
            let src = strip_math_delimiters(&content[start..end]);
            match math::layout(src, size, false, measure) {
                Ok(layout) => pieces.push(Piece::Math(layout)),
                // 退化段：span 记 `math` 样式，`build_text_job` 会再次剥分隔符
                // （此时已无 `$`，是空操作）并套上斜体公式样式。
                Err(_) => {
                    text.push_str(src);
                    text_spans.push(SpanView {
                        range: RangeView::new(0, src.len()),
                        style: "math".to_string(),
                        url: None,
                    });
                }
            }
        } else if end > start {
            let rel_start = text.len();
            text.push_str(&content[start..end]);
            let rel_end = text.len();
            text_spans.push(SpanView {
                range: RangeView::new(rel_start, rel_end),
                style: span.style.clone(),
                url: None,
            });
        }
        cursor = end.max(cursor);
    }
    if cursor < len {
        text.push_str(&content[cursor..]);
    }
    if !text.is_empty() {
        pieces.push(Piece::Text {
            text,
            spans: text_spans,
        });
    }
    pieces
}

/// 一行在 galley 坐标系里的**基线**（相对 galley 原点）。
///
/// epaint 的 `Glyph::pos.y` 注释写的是「相对行的基线位置」，
/// 而行自身还有 `PlacedRow::pos.y`（行顶相对 galley）。两者相加才是
/// 基线相对 galley 原点的偏移——数学引擎的 `Item::Text::y` 同样是
/// 「相对基线」，所以这个偏移就是两条路径对齐的基准。
/// 无字形（空行）时退化为行顶 + 0.8 倍行高（近似升部）。
fn row_baseline_offset(row: &PlacedRow) -> f32 {
    row.pos.y + row.glyphs.first().map_or(row.size.y * 0.8, |g| g.pos.y)
}

/// 含行内公式的段落：分段流式排版。
///
/// # 算法
///
/// 逐片段往前走，维护「当前行的光标 x / 行顶 / 行底 / 基线」：
/// - **文本段**：按剩余宽度攒 `LayoutJob` 交给 epaint 排版（内部折行不变），
///   画在当前行光标处；若它自己折了行，**最后一行**成为新的当前行，
///   之前的行整体计入段落高度；
/// - **公式段**：原子块——当前行放不下就整体挪到下一行，仍放不下
///   （公式比整行还宽）才等比缩小；落点按**基线**对齐：
///   矩形顶 = 行基线 − 公式升部，这样公式的基线与文字基线重合。
///
/// 基线由每行第一个片段定义，后续片段向它看齐：文本 galley 按
/// `基线 − 首行基线偏移` 定位，公式按 `基线 − 升部` 定位。
/// 这正是 TeX 把公式嵌进文字流的方式，也是「公式和文字不像拼起来」
/// 与「明显错位」之间的全部差别。
#[allow(clippy::too_many_arguments)] // 与 `render_inline_text` 同构（转发用），参数一一对应
pub(crate) fn render_with_math(
    ui: &mut Ui,
    _app: &mut EditorApp,
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    typo: Typography<'_>,
    sense: egui::Sense,
    wrap_width: f32,
    _internal_targets: &[(String, u64)],
) -> egui::Response {
    let Typography {
        size,
        line_height,
        base_style,
    } = typo;
    let done = base_style == "task_done";
    let wrap = wrap_width.max(0.0);
    let measure = |text: &str, sz: f32| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), FontId::proportional(sz), Color32::WHITE)
            .size()
            .x
    };
    let pieces = build_pieces(content, spans, size, &measure);

    // 行状态。`baseline = None` 表示该行还空着，由第一个片段定义基线。
    let mut x = 0.0_f32;
    let mut line_top = 0.0_f32;
    let mut line_bottom = line_height;
    let mut baseline: Option<f32> = None;
    // 段落总范围（可能为负：高公式会把首行顶上去）。
    let mut top = 0.0_f32;
    let mut bottom = line_height;
    let mut max_x = 0.0_f32;
    let mut cmds: Vec<Cmd> = Vec::new();

    for piece in &pieces {
        match piece {
            Piece::Math(ml) => {
                let nat_w = ml.width;
                let nat_h = ml.height();
                // 公式是原子的：当前行放不下就整体换行（不能从中间折断，
                // 折断的公式含义会变）。
                if x > 0.0 && wrap.is_finite() && nat_w > wrap - x {
                    x = 0.0;
                    line_top = line_bottom;
                    line_bottom = line_top + line_height;
                    baseline = None;
                }
                // 仍放不下（公式比整行还宽）→ 等比缩小，与块级公式同一策略：
                // 宁可小一点，也不要右侧被裁掉——裁掉的项会改变数学含义。
                let scale = if wrap.is_finite() && nat_w > wrap - x && nat_w > 0.0 {
                    (wrap - x) / nat_w
                } else {
                    1.0
                };
                let used_w = nat_w * scale;
                let used_h = nat_h * scale;
                let rect_top = match baseline {
                    Some(b) => b - ml.ascent * scale,
                    None => line_top,
                };
                let rect = Rect::from_min_size(Pos2::new(x, rect_top), Vec2::new(used_w, used_h));
                top = top.min(rect.top());
                bottom = bottom.max(rect.bottom());
                line_top = line_top.min(rect.top());
                line_bottom = line_bottom.max(rect.bottom());
                baseline = baseline.or(Some(rect.top() + ml.ascent * scale));
                x += used_w;
                cmds.push(Cmd::Math {
                    layout: ml.clone(),
                    rect,
                });
            }
            Piece::Text {
                text,
                spans: piece_spans,
            } => {
                if text.is_empty() {
                    continue;
                }
                // 折行宽度 = 当前行剩余宽度（首行之后 epaint 会按同一宽度折，
                // 与整行宽的偏差只有个把像素，可接受）。
                let job_wrap = (wrap - x).max(size);
                let (job, _hits) = build_text_job(theme, text, piece_spans, typo, done, job_wrap);
                let galley = ui.painter().layout_job(job);
                let Some(first_row) = galley.rows.first() else {
                    continue;
                };
                let origin_y = match baseline {
                    Some(b) => b - row_baseline_offset(first_row),
                    None => line_top - first_row.pos.y,
                };
                cmds.push(Cmd::Galley {
                    galley: galley.clone(),
                    pos: Pos2::new(x, origin_y),
                });
                // 所有行都计入段落高度（折出来的行同样占地方）。
                for row in &galley.rows {
                    let ry = origin_y + row.pos.y;
                    top = top.min(ry);
                    bottom = bottom.max(ry + row.size.y);
                }
                // 最后一行成为新的当前行；基线按它的首字形重算。
                let last = galley.rows.last().unwrap_or(first_row);
                line_top = origin_y + last.pos.y;
                line_bottom = line_top + last.size.y;
                baseline = Some(origin_y + row_baseline_offset(last));
                x = last.size.x;
                max_x = max_x.max(x);
            }
        }
    }
    max_x = max_x.max(x);

    let total_h = (bottom - top).max(line_height);
    let (_, rect) = ui.allocate_space(Vec2::new(max_x, total_h));
    // 段落原点（flow 坐标 0）落在分配矩形的哪里：内容可能从负 top 起。
    let origin = rect.min - Vec2::new(0.0, top);
    let color = span_format(theme, base_style, size, line_height).color;
    for cmd in &cmds {
        match cmd {
            Cmd::Galley { galley, pos } => {
                ui.painter()
                    .galley(origin + pos.to_vec2(), galley.clone(), color);
            }
            Cmd::Math { layout, rect } => {
                // 矩形高度已等于内容高度，`paint` 的纵向居中成为恒等操作，
                // 于是版面基线正好落在行基线上。
                math::paint(
                    ui.painter(),
                    rect.translate(origin.to_vec2()),
                    layout,
                    color,
                    Align2::LEFT_CENTER,
                );
            }
        }
    }
    ui.interact(rect, ui.scope_id().with("rinline"), sense)
}
