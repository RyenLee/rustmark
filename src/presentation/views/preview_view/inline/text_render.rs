//! 行内文本渲染（含可点击命中区）、`LayoutJob` 攒装与链接 / 脚注点击分发。
//!
//! 公式段不在此处排版——含 `$...$` 的段落改走 [`super::math::render_with_math`]
//! 的流式排版。

use egui::text::{CCursor, CharIndex, LayoutJob};
use egui::{CursorIcon, Galley, Pos2, Rect, Response, Sense, Ui};

use crate::app::frame::EditorApp;
use crate::application::dto::SpanView;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::theme::mapper::{SpanHit, SpanPolicy, build_job};
use crate::shared::system;

use super::delimiters::{footnote_ref_mark, strip_math_delimiters};
use super::math::render_with_math;
use super::text_format::{Typography, done_overlay, span_format};

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
/// 文字流的绘制挂点。于是含 `$...$` 的段落改走 [`render_with_math`] 的
/// **分段流式**排版——文本段照旧交给 epaint（内部折行不变），公式段用
/// [`crate::presentation::views::math`] 引擎二维排版后按**基线**拼回文字流。
#[allow(clippy::too_many_arguments)] // 预览内联渲染的完整上下文（UI/主题/文本/片段/排版/交互/宽度/跳转表）本就九项；打包成 struct 会让每个调用方多一次所有权转换，且掩盖「漏传即错」
pub(crate) fn render_inline_text(
    ui: &mut Ui,
    app: &mut EditorApp,
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    typo: Typography<'_>,
    sense: Sense,
    wrap_width: f32,
    internal_targets: &[(String, u64)],
) -> Response {
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
    // 点击时打开链接或跳转到目标（见 [`handle_span_click`]）。
    let hit_rects = span_rects(&galley, &hits);
    let mut link_hovered = false;
    for (idx, local) in hit_rects {
        let screen = local.translate(rect.min.to_vec2());
        let resp = ui.interact(
            screen,
            ui.scope_id().with(("rinline_hit", idx)),
            Sense::click(),
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
        ui.output_mut(|o| o.cursor_icon = CursorIcon::PointingHand);
    }
    ui.interact(rect, ui.scope_id().with("rinline"), sense)
}

/// 由排版结果与片段字符区间，算出每个可点击片段在 galley 坐标系里的矩形。
///
/// 逐行求交：一段文本可能折成多行，片段矩形是这些行的并集（这正是
/// 「链接被折行」时仍整段可点」的关键）。`pos_from_cursor` 取的是字符光标的
/// 纵向撑满整行的细矩形，用它左右边即可圈出片段在某一行的横向范围。
fn span_rects(galley: &Galley, hits: &[SpanHit]) -> Vec<(usize, Rect)> {
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
/// 可见性：`pub`（R-03 起 additionally 经 `preview_view::test_only` 再导出给集成测试，
/// 外链改走 opener 端口后，「点击 link 会调 open、不安全地址不调」要在集成测试里断言）。
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
/// 切段逻辑已收敛到 [`build_job`]（R-10）；本函数只负责「预览区行内用哪套样式」——
/// 即 [`span_format`]（预览字号族 + 预览行高 + 行内特有样式），
/// 以及两处**行内特有**语义：
/// - `done_overlay`（任务项已勾选 → 整行次级色 + 删除线）；
/// - 文本改写（math 剥分隔符、脚注引用换标号）。
pub(crate) fn build_text_job(
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
