//! GFM 表格：列宽测量分配 + 自绘网格。

use egui::{Align, CornerRadius, FontId, Layout};

use rustmark_core::application::dto::{CellAlignView, SpanView, TableView};

use super::inline::{Typography, render_inline_text};
use super::*;

/// GFM 表格：列宽按**单元格实际内容**自适应，超宽时按内容占比压缩。
///
/// # 为什么不用 `egui::Grid`
///
/// `Grid` 的列宽来自 sizing pass 里各单元格**愿意收缩到的最小宽度**，
/// 与内容真实长度无关。表头短、正文长的表格里，每列都被压到表头那个量级，
/// 长单元格在窄列里折成六七行，密不透风——这正是「按表头宽度换行」的观感来源。
///
/// 现改为两步：
///
/// 1. **测量**：每列取「表头 + 全部行」中最宽单元格的**单行**宽度；
/// 2. **分配**：交给纯函数 [`table_column_distribution`]——
///    总宽放得下就按自然宽度给列（一行不折，富余摊满面板）；
///    放不下才按内容占比压缩，且每列保底 [`TABLE_MIN_COL_W`]，
///    长内容列拿大头、短列保持紧凑。
///
/// 测量用的字体与渲染相同（`Metrics::preview_font_size`），且 egui 的
/// `strong` 只加深颜色、不换粗体字形（见 `RichText::strong` 的实现），
/// 因此表头用同一字体测出的自然宽度是**精确**的。
///
/// # 宽度基准为什么必须跟「自动换行」开关走（问题 068）
///
/// * 开：`available` 取当前 `ui.available_width()`。调用点所在的块子树已被
///   `preview_view::show` 用 `set_max_width` 钳到**视口宽**，故这里读到的是
///   不受前序块污染的稳定值——每张表都独立按同一个视口宽算列宽，
///   第一张表再宽也不会把后面的表带偏。
/// * 关：`available` 取 `INFINITY`，列宽退回**自然宽**，超出视口的部分交给
///   预览区的横向滚动条——与关闭换行时的代码块同一行为，而不是被压进面板。
pub(super) fn draw_table(
    vctx: &mut dyn EditorViewContext,
    ui: &mut Ui,
    theme: &Theme,
    table: &TableView,
    wrap: bool,
    internal_targets: &[(String, u64)],
) {
    let m = theme.metrics;
    egui::Frame::new()
        .stroke(hairline(theme.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(m.pane_padding as i8))
        .show(ui, |ui| {
            let available = if wrap {
                ui.available_width()
            } else {
                f32::INFINITY
            };

            // —— 测量：每列最宽内容的单行宽度 ——
            let font = FontId::proportional(m.preview_font_size);
            let mut natural = vec![0.0_f32; table.header.len().max(1)];
            for (c, cell) in table.header.iter().enumerate() {
                natural[c] = natural[c].max(measure_cell(ui, &font, cell));
            }
            for row in &table.rows {
                for (c, cell) in row.iter().enumerate() {
                    if let Some(n) = natural.get_mut(c) {
                        *n = (*n).max(measure_cell(ui, &font, cell));
                    }
                }
            }
            let widths = table_column_distribution(&natural, available);
            let cols_layout = ColumnLayout {
                widths: &widths,
                aligns: &table.aligns,
                // 单元格文本是否在格子内折行：跟随「自动换行」开关——
                // 关闭时单元格按自然宽单行排开（与整表口径一致）。
                wrap,
            };

            // 行与行**不留间距**，否则列间竖线会在每行交界处断开，网格变成虚线。
            // 行距改由单元格自己的纵向内边距提供（见 `table_row`）。
            ui.spacing_mut().item_spacing.y = 0.0;

            let row_count = table.rows.len();
            let last = row_count.saturating_sub(1);
            if !table.header.is_empty() {
                table_row(
                    vctx,
                    ui,
                    theme,
                    &table.header,
                    &table.header_spans,
                    cols_layout,
                    true,
                    true,
                    internal_targets,
                );
            }
            for (r, row) in table.rows.iter().enumerate() {
                // 最后一行**不画底线**：它与外框的下边重合，画两遍会在圆角处
                // 露出一道比别处亮的线。
                table_row(
                    vctx,
                    ui,
                    theme,
                    row,
                    table.rows_spans.get(r).map_or(&[][..], Vec::as_slice),
                    cols_layout,
                    false,
                    r < last,
                    internal_targets,
                );
            }
        });
}

/// 量一个单元格文本的**单行**自然宽度（点）。
///
/// 颜色不参与度量，统一用白色探针；`no_wrap` 保证拿到的是「不折行」的宽度。
fn measure_cell(ui: &Ui, font: &FontId, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
        .size()
        .x
}

/// 表格列宽分配（**纯函数**，不依赖 egui，可脱离 UI 单测）。
///
/// * `natural[c]`：第 c 列最宽内容的单行宽度（**不含**内边距）。
/// * `available`：表格可用总宽；`INFINITY` 表示「关闭自动换行」——
///   不压缩、不摊满，直接按自然宽排开，超出部分交给横向滚动条。
/// * 返回每列的最终宽度（**含**左右内边距 [`TABLE_CELL_PAD`]）。
///
/// 三条不变量（各有单测钉住；「列宽和」均不含列间距——间距由布局层的
/// `item_spacing` 提供，值同为 [`TABLE_COL_GAP`]，故分配只对
/// 「面板宽 − 列间距」负责）：
///
/// 1. `sum(widths) + 列间距 == min(需求宽, available)`——放得下时占满面板、
///    放不下时正好用满，既不溢出也不留大段空白；
/// 2. 放不下且预算给得起时每列 `>= [`TABLE_MIN_COL_W`]`——内容再长也不把短列挤没；
/// 3. 预算连保底都给不起（窗口极窄 / 列数极多）时**均分预算**，总和仍恰为
///    `available`——宁可每列窄一点，也不能溢出视口（问题 068：溢出会膨胀
///    `ScrollArea` 的内容宽，把后续所有块的可用宽一起带偏）。
///
/// 分配策略：长列拿大头（按内容占比），这正是「长内容展示合理」的实现点。
pub fn table_column_distribution(natural: &[f32], available: f32) -> Vec<f32> {
    let cols = natural.len();
    if cols == 0 {
        return Vec::new();
    }
    // 关闭自动换行：不压缩、不摊满，按自然宽排开。超出视口的部分交给预览区
    // 的横向滚动条（与关闭换行时的代码块同一行为）。放在最前：下面的
    // 「放得下」分支会拿 `available - need` 做富余摊分，对 INFINITY 无意义。
    if available.is_infinite() {
        return natural.iter().map(|n| n + 2.0 * TABLE_CELL_PAD).collect();
    }
    let cols_f = cols as f32;
    let gaps = TABLE_COL_GAP * (cols - 1) as f32;
    let need: f32 = natural
        .iter()
        .map(|w| w + 2.0 * TABLE_CELL_PAD)
        .sum::<f32>()
        + gaps;
    // 内容占比：全空表退化为均分（`share` 的分母不可能是 0）。
    let weight_sum: f32 = natural.iter().copied().sum();
    let share = |w: f32| {
        if weight_sum > 0.0 {
            w / weight_sum
        } else {
            1.0 / cols_f
        }
    };

    if need <= available {
        // 放得下：每列拿「自然宽 + 内边距」，富余量按内容占比摊满面板。
        // 摊法用占比而非均分：内容多的列多占一点，视觉权重与内容长度一致。
        let extra = available - need;
        natural
            .iter()
            .map(|n| n + 2.0 * TABLE_CELL_PAD + extra * share(*n))
            .collect()
    } else {
        // 放不下：每列先保底，剩余预算按内容占比分配（总和恰为 available）。
        let budget = available - gaps;
        let floor_total = TABLE_MIN_COL_W * cols_f;
        if budget <= floor_total {
            // 窗口极窄 / 列数极多：预算连保底都给不起。均分预算（保底 1pt 防零宽），
            // 总和仍恰为 available——「宁可溢出」是旧实现，溢出会把 ScrollArea 的
            // 内容宽撑大，让后续所有块的可用宽一起失真（问题 068 的传播源）。
            let per_col = (budget / cols_f).max(1.0);
            return vec![per_col; cols];
        }
        let flex = budget - floor_total;
        natural
            .iter()
            .map(|n| TABLE_MIN_COL_W + flex * share(*n))
            .collect()
    }
}

/// 表格的**列布局**：各列宽度与对齐方式。整表共用，每一行拿到的都是同一份。
///
/// 打包成结构而不是拆成两个平铺参数，是因为 [`table_row`] 的参数个数已经
/// 顶到 clippy 的 7 个上限（`src/lib.rs` 把告警升成了硬错误）。而这两个值
/// 语义上本就是一对「列的定义」，合成一个反而更好读。
#[derive(Clone, Copy)]
struct ColumnLayout<'a> {
    widths: &'a [f32],
    aligns: &'a [CellAlignView],
    /// 单元格文本是否在格子内折行（跟随「自动换行」开关）。
    wrap: bool,
}

/// 表格的一行：表头底色 + 列间竖线 + 行底线 + 单元格行内富文本。
///
/// # 为什么自己画网格线
///
/// 早先只在表头下画一条横线，表体各行之间**没有任何分隔**：一个单元格
/// 内容折成三行时，它与下一行的边界只能靠行距去猜，多行单元格里尤其糊。
/// 现在三样都补齐——列间竖线、每行底线、表头底色。
///
/// # 竖线为什么在行画完之后才补
///
/// 每行里各单元格的高度**不相等**（多行单元格比单行的矮邻居高），
/// 若在单元格分配时顺手画竖线，线只能占到自己那一格的高度，行内就成了一段段
/// 长短不齐的短线。故先把各列分界点的 x 记下来，等整行矩形出来后再统一画到
/// 行高——线是齐的。画在文字之上也无妨：分界点落在列与列之间的空隙里。
///
/// # 行距
///
/// 相邻行的间距已由 `draw_table` 置 0（否则竖线会在行交界处断开），
/// 纵向留白改由这里的 `pad_y` 提供。
#[allow(clippy::too_many_arguments)] // 与 `render_inline_text` 同族：表格行渲染需要完整上下文，且单元格链接跳转要 `internal_targets`
fn table_row(
    vctx: &mut dyn EditorViewContext,
    ui: &mut Ui,
    theme: &Theme,
    cells: &[String],
    cell_spans: &[Vec<SpanView>],
    cols_layout: ColumnLayout<'_>,
    header: bool,
    bottom_line: bool,
    internal_targets: &[(String, u64)],
) {
    let ColumnLayout {
        widths,
        aligns,
        wrap,
    } = cols_layout;
    let m = theme.metrics;
    let size = m.preview_font_size;
    let line_height = size * m.preview_line_height;
    let pad_y = m.space_xs.max(2.0);
    let cols = widths.len().min(cells.len());
    // 表头底色：让「哪一行是表头」不只靠颜色加深来承担——
    // 深色主题里加粗几乎与正文同色，一眼分不出表头与数据行。
    let bg = if header {
        theme.bg_tertiary
    } else {
        egui::Color32::TRANSPARENT
    };

    // 各列分界点（绝对 x），行画完后统一补竖线。
    let mut seps: Vec<f32> = Vec::with_capacity(cols);
    let frame = egui::Frame::new().fill(bg).show(ui, |ui| {
        // 显式对齐列间距与 `table_column_distribution` 的预算口径：
        // 那里按 [`TABLE_COL_GAP`] 预留了间距，靠 egui 的默认 item_spacing
        // 恰好等于 8.0 只是巧合，改主题间距就会让列宽与预算对不上。
        ui.spacing_mut().item_spacing.x = TABLE_COL_GAP;
        ui.horizontal(|ui| {
            for (c, cell) in cells.iter().enumerate().take(cols) {
                let w = widths[c];
                let align = align_of(aligns.get(c));
                let spans = cell_spans.get(c).map_or(&[][..], Vec::as_slice);
                // 单元格文本的折行宽：**完整减去左右两侧内边距**。旧实现读
                // `ui.available_width()`，此时只扣了左侧一次 `add_space`，
                // 比格子内容区宽出 8pt——文本会侵入右侧内边距。
                // 关闭自动换行时传 `INFINITY`：单元格单行排开，与整表口径一致。
                let cell_max_w = if wrap {
                    (w - 2.0 * TABLE_CELL_PAD).max(0.0)
                } else {
                    f32::INFINITY
                };
                let res = ui.allocate_ui_with_layout(
                    egui::vec2(w, 0.0),
                    Layout::left_to_right(Align::TOP),
                    |ui| {
                        // 格子内部横向间距归零（问题 068 的真正根因）：
                        // `add_space` 在横向布局里会**额外**吞一份 item_spacing
                        // （分配 8pt 实际推进 16pt），加上行级列间距 8pt，
                        // 实际列距变成 `列宽 + 16`，而列宽分配只预算了
                        // `列宽 + 8`——列数一多整表就比预算宽出
                        // `(列数-1)×8`，溢出视口并膨胀 ScrollArea 的内容宽，
                        // 把后续所有块的可用宽一起带偏。归零后格子精确占据
                        // `widths[c]`，与 [`table_column_distribution`] 的
                        // 预算口径严格一致。
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(TABLE_CELL_PAD);
                        ui.with_layout(Layout::top_down(align), |ui| {
                            ui.set_min_width(w - 2.0 * TABLE_CELL_PAD);
                            ui.add_space(pad_y);
                            // 单元格文本走行内富文本：`| a **粗** |` 里的粗体
                            // 之前被当作纯文本直接打印，与正文里的粗体表现不一致。
                            render_inline_text(
                                ui,
                                vctx,
                                theme,
                                cell,
                                spans,
                                Typography {
                                    size,
                                    line_height,
                                    // 表头用 `strong` 基准色（egui 的「强调」=
                                    // 加深颜色而非换粗体字形，旧实现观感一致）。
                                    base_style: if header { "strong" } else { "" },
                                },
                                egui::Sense::hover(),
                                cell_max_w,
                                internal_targets,
                            );
                            ui.add_space(pad_y);
                        });
                        ui.add_space(TABLE_CELL_PAD);
                    },
                );
                if c + 1 < cols {
                    // 落在两列之间的空隙中点：贴住某一列会让线看起来
                    // 「属于左边那格」。
                    seps.push(res.response.rect.right() + TABLE_COL_GAP * 0.5);
                }
            }
        });
    });

    let rect = frame.response.rect;
    for x in seps {
        ui.painter()
            .vline(x, rect.y_range(), hairline(theme.border));
    }
    if bottom_line {
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), hairline(theme.border));
    }
}

/// 列对齐枚举 → egui 对齐。
fn align_of(align: Option<&CellAlignView>) -> Align {
    match align {
        Some(CellAlignView::Left) | Some(CellAlignView::None) | None => Align::LEFT,
        Some(CellAlignView::Center) => Align::Center,
        Some(CellAlignView::Right) => Align::RIGHT,
    }
}
