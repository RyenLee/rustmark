//! 行内富文本：span → `TextFormat`、`LayoutJob` 排版与分隔符规范化。

use egui::text::{CCursor, CharIndex, LayoutJob, TextFormat};
use egui::{Align2, FontId, Pos2, Rect, Stroke, Ui, Vec2};

use crate::application::dto::{RangeView, SpanView};
use crate::presentation::theme::mapper::{SpanHit, SpanPolicy, StyleSemantics, build_job};
use crate::presentation::views::math::{self, MathLayout};
use crate::shared::geometry::clamp_to_char_boundaries;
use crate::shared::system;

use super::*;

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
/// 可见性为 `pub(crate)` 而非 `pub(super)`：`theme::mapper` 的样式表
/// 合并用例（`mapper/tests.rs`）要把它与**编辑区侧**的样式表做同表对照，
/// 而 `pub(super)`（限 `preview_view` 模块内）够不着跨模块访问。
/// R-03 起经 `preview_view::test_only` 再导出给集成测试，故此处为 `pub`。
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
pub(super) struct Typography<'a> {
    pub(super) size: f32,
    pub(super) line_height: f32,
    pub(super) base_style: &'a str,
}

/// 渲染一段可带行内样式的文本（`spans` 为空时等价于普通 `label`）。
///
/// # 为什么用 `LayoutJob` 而不是「一段一个 `ui.label`」
///
/// 逐段铺控件会让折行只发生在段与段之间，一段长文本照样整段溢出；
/// 而 `LayoutJob` 把整行交给 epaint 排版，**折行、行高、对齐都由它统一处理**，
/// 与普通段落完全一致。这也是段落渲染只有一条代码路径的原因——
/// 无样式时段数为零，`job` 退化成「一整段纯文本」，与旧实现等价。
///
/// 区间一律吸附到字符边界：`spans` 来自解析器，而编辑器里文本可能在
/// 150ms 防抖窗口内又变过，越界切片会 panic（本项目 `panic = "abort"`）。
///
/// # 行内公式为什么单独一条路
///
/// `LayoutJob` 只能排**文字**：epaint 0.33 既没有行内占位段，也没有能插进
/// 文字流的绘制挂点。于是含 `$...$` 的段落改走 `render_with_math` 的
/// **分段流式**排版——文本段照旧交给 epaint（内部折行不变），公式段用
/// [`math`] 引擎二维排版后按**基线**拼回文字流。这正是官方 KaTeX 的观感；
/// 旧实现把公式当斜体文本显示，用户看到的是 `\Gamma(n)` 这串源码。
#[allow(clippy::too_many_arguments)] // 预览内联渲染的完整上下文（UI/主题/文本/片段/排版/交互/宽度/跳转表）本就九项；打包成 struct 会让每个调用方多一次所有权转换，且掩盖「漏传即错」
pub(super) fn render_inline_text(
    ui: &mut Ui,
    app: &mut EditorApp,
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    typo: Typography<'_>,
    sense: egui::Sense,
    wrap_width: f32,
    internal_targets: &[(String, u64)],
) -> egui::Response {
    if spans.iter().any(|s| s.style == "math") {
        return render_with_math(
            ui,
            app,
            theme,
            content,
            spans,
            typo,
            sense,
            wrap_width,
            internal_targets,
        );
    }
    let Typography {
        size,
        line_height,
        base_style,
    } = typo;
    let base = span_format(theme, base_style, size, line_height);
    // 「已完成」是**叠加**在行内样式之上的装饰，不是一个基准样式。
    // `- [x] **重点**` 里已勾选的项仍要能看出哪几个词原本是粗体——只把样式
    // 替换掉会让勾选这个动作顺手抹掉用户写的格式信息。
    let done = base_style == "task_done";

    // 统一走 `LayoutJob`：有 / 无行内样式都先攒出一个 galley 再手工绘制。
    // 「自动换行开 / 关」只由 `wrap_width` 一个值决定（关 = `INFINITY`），
    // 不再依赖 `ui.add(Label)` 那套默认折行逻辑——那样关不掉折行。
    let (job, hits) = build_text_job(theme, content, spans, typo, done, wrap_width);
    // 手工排版 + 绘制：比 `ui.add(Label)` 多一步，但能精确控制折行宽度，
    // 且为「命中区 / 点击」等扩展留好位置（见图表链接的 `draw_diagram`）。
    //
    // 命中区 id 用调用方 ui 的 id + 固定盐 `"rinline"`：**要求每个块子树
    // 的 `ui.scope_id()` 唯一**（`show()` 里已用 `block.id` 作 `id_salt`）。
    // 若同一父级下多次调用且 sense 可交互（如标题的 `Sense::click()`），
    // id 会相撞 —— egui 会画出红框与「Double use of widget ID」提示
    // （2026-10-10 曾因此让预览标题被套红框，改 `show()` 的 `id_salt` 修复）。
    let galley = ui.painter().layout_job(job);
    let (_id, rect) = ui.allocate_space(galley.size());
    ui.painter().galley(rect.min, galley.clone(), base.color);
    // 行内链接 / 脚注引用：按字符区间求交互矩形，逐个 `interact`，
    // 点击时打开链接或跳转到目标（见 `handle_span_click`）。
    let hit_rects = span_rects(&galley, &hits);
    let mut link_hovered = false;
    for (idx, local) in hit_rects {
        let screen = local.translate(rect.min.to_vec2());
        let resp = ui.interact(
            screen,
            ui.scope_id().with(("rinline_hit", idx)),
            egui::Sense::click(),
        );
        let sv = &spans[idx];
        if resp.hovered() && (sv.style == "link" || sv.style == "footnote_ref") {
            link_hovered = true;
        }
        if resp.clicked() {
            handle_span_click(app, sv, content, internal_targets);
        }
    }
    if link_hovered {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
    }
    ui.interact(rect, ui.scope_id().with("rinline"), sense)
}

/// 由排版结果与片段字符区间，算出每个可点击片段在 galley 坐标系里的矩形。
///
/// 逐行求交：一段文本可能折成多行，片段矩形是这些行的并集（这正是
/// 「链接被折行」时仍整段可点」的关键）。`pos_from_cursor` 取的是字符光标的
/// 纵向撑满整行的细矩形，用它左右边即可圈出片段在某一行的横向范围。
fn span_rects(galley: &egui::Galley, hits: &[SpanHit]) -> Vec<(usize, Rect)> {
    let mut out: Vec<(usize, Rect)> = Vec::new();
    let mut running = 0usize;
    for row in galley.rows.iter() {
        let row_start = running;
        let row_end = running + row.char_count_including_newline().0;
        running = row_end;
        let row_rect = row.rect();
        for hit in hits {
            let lo = hit.c0.max(row_start);
            let hi = hit.c1.min(row_end);
            if lo >= hi {
                continue;
            }
            let a = galley.pos_from_cursor(CCursor {
                index: CharIndex(lo),
                prefer_next_row: false,
            });
            let b = galley.pos_from_cursor(CCursor {
                index: CharIndex(hi),
                prefer_next_row: false,
            });
            let piece = Rect::from_min_max(
                Pos2::new(a.min.x.min(b.min.x), row_rect.top()),
                Pos2::new(a.min.x.max(b.min.x), row_rect.bottom()),
            );
            if let Some(e) = out.iter_mut().find(|(i, _)| *i == hit.idx) {
                e.1 = e.1.union(piece);
            } else {
                out.push((hit.idx, piece));
            }
        }
    }
    out
}

/// 行内片段点击分发：链接打开、脚注引用跳转。
///
/// - `link`：绝对 / 相对地址交系统默认程序打开（`services.opener`，
///   端口在 `shared`、实现在 `infrastructure`，见 R-03）；
///   `#锚点` 在 `internal_targets` 里找对应标题块并跳转；地址经
///   `system::is_safe_to_open` 过滤，拒绝 `javascript:` 等 XSS 向量。
/// - `footnote_ref`：依标号在 `internal_targets` 里找脚注定义块并跳转
///   （预览点击跳转与连续滚动同步解耦，见 `scroll_sync` 改造）。
/// 可见性：`pub(super)` 供 `preview_view` 内的调用方使用；R-03 起 additionally
/// 经 `preview_view::test_only` 再导出给集成测试（外链改走 opener 端口后，
/// 「点击 link 会调 open、不安全地址不调」要在集成测试里断言）。
#[doc(hidden)]
pub fn handle_span_click(
    app: &mut EditorApp,
    sv: &SpanView,
    content: &str,
    internal_targets: &[(String, u64)],
) {
    match sv.style.as_str() {
        "link" => {
            let Some(url) = sv.url.as_ref() else {
                return;
            };
            let url = url.trim();
            if let Some(anchor) = url.strip_prefix('#') {
                if let Some((_, id)) = internal_targets.iter().find(|(k, _)| k == anchor) {
                    app.ui.sync.request_click(*id);
                }
                return;
            }
            if system::is_safe_to_open(url) {
                // 打开失败不打扰用户：链接打不开时预览区照常显示，
                // 弹窗反而挡住阅读（与旧实现 `let _ =` 的取舍一致）。
                let _ = app.services.opener.open(url);
            }
        }
        "footnote_ref" => {
            let label = footnote_ref_mark(&content[sv.range.start..sv.range.end]);
            if let Some((_, id)) = internal_targets.iter().find(|(k, _)| k == label) {
                app.ui.sync.request_click(*id);
            }
        }
        _ => {}
    }
}

/// 把一段文本 + 行内 span 攒成 `LayoutJob`，并标出每个可点击片段的字符区间。
///
/// 从原单 Galley 路径抽出，与流式排版（`render_with_math`）**共用**：
/// 同一段文本在两条路径里必须长得一样，否则「加个公式整段变色」这种
/// 诡异差别会无声出现。`done` 由调用方算好（`task_done` 叠加装饰）。
///
/// 返回的 `Vec<SpanHit>` 只记录「链接 / 脚注引用」这类需要交互的片段
/// （其余片段不记录，省去无谓的命中矩形）；字符区间相对拼接后的 job 文本，
/// 供 [`span_rects`] 定位。
///
/// # 本函数是「预览区行内那一侧」的薄封装
///
/// 切段逻辑已收敛到 [`crate::presentation::theme::mapper::build_job`]（R-10）；
/// 本函数只负责「预览区行内用哪套样式」——即 [`span_format`]
/// （预览字号族 + 预览行高 + 行内特有样式），
/// 以及两处**行内特有**语义：
/// - `done_overlay`（任务项已勾选 → 整行次级色 + 删除线）；
/// - 文本改写（math 剥分隔符、脚注引用换标号）。
pub(super) fn build_text_job(
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    typo: Typography<'_>,
    done: bool,
    wrap_width: f32,
) -> (LayoutJob, Vec<SpanHit>) {
    let Typography {
        size,
        line_height,
        base_style,
    } = typo;
    let base = span_format(theme, base_style, size, line_height);
    // 样式闭包：行内样式 + （已勾选时的）完成态覆盖。
    let style_of = |style: &str| {
        let fmt = span_format(theme, style, size, line_height);
        if done { done_overlay(theme, fmt) } else { fmt }
    };
    // 文本改写：`math` 剥掉 `$` / `$$` 分隔符（它们是语法，不是内容），
    // `footnote_ref` 把 `[^1]` 换成标号 `1`（否则正文里会以原始
    // Markdown 形态显示 `[^1]`）。
    let rewrite = |raw: &str, style: &str| -> String {
        match style {
            "math" => strip_math_delimiters(raw).to_string(),
            "footnote_ref" => footnote_ref_mark(raw).to_string(),
            _ => raw.to_string(),
        }
    };
    let policy = SpanPolicy::cursor(base, wrap_width, style_of, Some(&rewrite));
    build_job(theme, content, spans, &policy, |style| {
        style == "link" || style == "footnote_ref"
    })
}

// ============================================================================
// 行内公式：分段流式排版
// ============================================================================

/// 流式排版的一个片段。
enum Piece {
    /// 一段纯文本（span 区间相对本段起点；为空表示无样式）。
    Text { text: String, spans: Vec<SpanView> },
    /// 排版成功的行内公式（版面已按正文字号排好）。
    Math(MathLayout),
}

/// 一条绘制指令（坐标相对段落原点，最后统一平移到分配矩形）。
enum Cmd {
    Galley {
        galley: std::sync::Arc<egui::Galley>,
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
fn build_pieces(
    content: &str,
    spans: &[SpanView],
    size: f32,
    measure: math::Measure<'_>,
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
fn row_baseline_offset(row: &egui::epaint::text::PlacedRow) -> f32 {
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
fn render_with_math(
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
            .layout_no_wrap(
                text.to_owned(),
                FontId::proportional(sz),
                egui::Color32::WHITE,
            )
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

/// 给一个行内格式叠上「已完成」装饰（次级色 + 删除线）。
pub(super) fn done_overlay(theme: &Theme, mut fmt: TextFormat) -> TextFormat {
    fmt.color = theme.text_secondary;
    fmt.strikethrough = Stroke::new(1.0_f32, theme.text_secondary);
    fmt
}

/// 剥掉行内公式两端的 `$` / `$$` 分隔符（两侧各最多两个）。
///
/// 只在**两端成对**时才剥：`a $x$ 与 b` 的片段是 `$x$`，剥成 `x`；
/// 而片段本身若只剩一个 `$`（解析边界与渲染边界不一致时可能发生），
/// 原样返回比剥空更安全——剥空会让那一个字符凭空消失。
///
/// `pub` 是为了让表现层用例直接断言（切片越界会 panic，而本项目
/// `panic = "abort"`；这条规则不能只靠肉眼检查）。
pub fn strip_math_delimiters(s: &str) -> &str {
    let lead = s.len() - s.trim_start().len();
    let trail = s.trim_end().len();
    if trail <= lead {
        return s;
    }
    let body = &s[lead..trail];
    let open = body.len() - body.trim_start_matches('$').len();
    let close = body.len() - body.trim_end_matches('$').len();
    if open == 0 || close == 0 {
        return s;
    }
    let k = open.min(close).min(2);
    let (start, end) = (lead + k, trail - k);
    if end <= start { s } else { &s[start..end] }
}

/// 把脚注引用 `[^1]` 的显示文本换成**标号** `1`。
///
/// `[^` 与 `]` 是语法不是内容：正文里显示 `[^1]` 看起来就像「脚注没渲染」。
/// 主流渲染器（GitHub 等）把它画成上标数字，这里同样只留标号，字号缩小、
/// 用链接色（见 `span_format` 的 `"footnote_ref"`），与脚注定义区的 `[1]`
/// 标签对应。内容里保留的仍是 `[^1]` 原文，搜索 / 字数不受影响。
///
/// 只在**两端成对**时才剥（与 [`strip_math_delimiters`] 同一保守策略）：
/// 片段若不是完整的 `[^…]`（解析边界与渲染边界不一致时可能发生），
/// 原样返回比剥错更安全。
///
/// `pub` 是为了让表现层用例直接断言。
pub fn footnote_ref_mark(s: &str) -> &str {
    let Some(body) = s.strip_prefix("[^") else {
        return s;
    };
    let Some(label) = body.strip_suffix(']') else {
        return s;
    };
    if label.is_empty() { s } else { label }
}
