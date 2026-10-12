//! 行内富文本：span → `TextFormat`、`LayoutJob` 排版与分隔符规范化。

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{FontId, Stroke, Ui};

use crate::application::dto::SpanView;
use crate::shared::geometry::clamp_to_char_boundaries;

use super::*;

/// 行内样式 → `TextFormat`（预览字号体系，与编辑区度量无关）。
///
/// 刻意**不复用** `StyleMapper::format`：那个函数的字号取自
/// `theme.editor_font_size`（编辑区设置），且带编辑区的行高——直接拿来渲染预览
/// 会让预览字号跟随编辑区设置漂移。这里显式用预览字号族，并统一给出
/// `line_height`，使同一段文本里的不同样式**不改变行高**（上下标变小也不撑行）。
fn span_format(theme: &Theme, style: &str, size: f32, line_height: f32) -> TextFormat {
    let base = TextFormat {
        color: theme.text_primary,
        font_id: FontId::proportional(size),
        line_height: Some(line_height),
        ..Default::default()
    };
    match style {
        // 标题基准样式（调用方给出标题字号）：只需换色，字号由 `size` 决定。
        "heading" => TextFormat {
            color: theme.syn_heading,
            ..base
        },
        // 引用基准样式：次级色 + 斜体（与旧实现的观感一致）。
        "quote" => TextFormat {
            color: theme.syn_quote,
            italics: true,
            ..base
        },
        "strong" => TextFormat {
            color: theme.syn_bold,
            ..base
        },
        "emphasis" => TextFormat {
            color: theme.syn_italic,
            italics: true,
            ..base
        },
        "strikethrough" => TextFormat {
            color: theme.text_secondary,
            strikethrough: Stroke::new(1.0_f32, theme.text_secondary),
            ..base
        },
        "code" => TextFormat {
            color: theme.syn_code,
            background: theme.code_bg,
            font_id: FontId::monospace(size),
            ..base
        },
        "link" => TextFormat {
            color: theme.syn_link,
            underline: Stroke::new(1.0_f32, theme.syn_link),
            ..base
        },
        // 行内公式：**只做样式区分，不做二维排版**。
        //
        // `LayoutJob` 里没有「行内自定义控件」的位置（epaint 0.33 既没有
        // placeholder 段，也没有可插进文字流的 `Shape::Callback` 挂点），
        // 想在段落中段嵌一块二维公式就得自己接管整个段落的折行——
        // 那是把「段落渲染」这条最常用的代码路径整个重写，风险远大于收益。
        // 故行内公式渲染为「倾斜 + 公式色」的文字，并把 `$` 分隔符去掉
        // （见 `render_inline_text`）——分隔符是语法，不是内容。
        "math" => TextFormat {
            color: theme.syn_code,
            italics: true,
            ..base
        },
        // 键帽：等宽 + 底色，视觉上是一枚小按键。
        "keycap" => TextFormat {
            color: theme.syn_code,
            background: theme.code_bg,
            font_id: FontId::monospace(size * 0.92),
            ..base
        },
        // 高亮 ==标记==：叠一层查找命中的底色（语义同为「此处被标出」）。
        "mark" => TextFormat {
            color: theme.text_primary,
            background: theme.find_bg,
            ..base
        },
        // 已勾选的任务项：整行次级色 + 删除线。
        //
        // 刻意**不**复用 `strikethrough` 样式：那个是给 `~~删除~~` 用的，
        // 语义不同（一个是「用户划掉的文字」，一个是「已完成」）。
        // 分开命名后，将来想给「已完成」换色（比如绿色对勾配色）不会波及删除线。
        "task_done" => TextFormat {
            color: theme.text_secondary,
            strikethrough: Stroke::new(1.0_f32, theme.text_secondary),
            ..base
        },
        // 上下标：epaint 无基线位移，用略小字号近似（行高由 `base.line_height`
        // 固定，故不会把行撑高、不破坏与相邻行的对齐）。
        "sub" | "sup" => TextFormat {
            color: theme.text_secondary,
            font_id: FontId::proportional(size * 0.78),
            ..base
        },
        // 脚注引用 `[^1]`：与上下标同一近似——小一号字 + 链接色。
        // 它本质是文中指向脚注定义的交叉引用，用链接色才能传达「可对应」；
        // 显示文本由 `footnote_ref_mark` 换成标号 `1`（剥掉 `[^` `]` 语法）。
        "footnote_ref" => TextFormat {
            color: theme.syn_link,
            font_id: FontId::proportional(size * 0.78),
            ..base
        },
        _ => base,
    }
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

pub(super) fn render_inline_text(
    ui: &mut Ui,
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    typo: Typography<'_>,
    sense: egui::Sense,
    wrap_width: f32,
) -> egui::Response {
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
    let mut job = LayoutJob {
        wrap: TextWrapping {
            max_width: wrap_width.max(0.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let len = content.len();
    let mut cursor = 0usize;
    if spans.is_empty() {
        let mut fmt = base.clone();
        if done {
            fmt = done_overlay(theme, fmt);
        }
        job.append(content, 0.0, fmt);
    } else {
        for span in spans {
            // 越界 / 落在多字节字符中间的区间会被吸附或退化为空——跳过而不是崩。
            let Some((start, end)) = clamp_to_char_boundaries(
                content,
                span.range.start.min(len),
                span.range.end.min(len),
            ) else {
                continue;
            };
            let start = start.max(cursor);
            let end = end.max(start);
            if start > cursor {
                job.append(&content[cursor..start], 0.0, base.clone());
            }
            if end > start {
                let mut fmt = span_format(theme, &span.style, size, line_height);
                if done {
                    fmt = done_overlay(theme, fmt);
                }
                // 行内公式剥掉 `$` / `$$` 分隔符：它们是语法，不是内容。
                // 不剥的话预览里会显示成 `$E=mc^2$`，看起来像没解析成功。
                // 脚注引用同理：`[^1]` 换成标号 `1`（上标小字 + 链接色），
                // 否则正文里会以原始 Markdown 形态显示 `[^1]`。
                let slice = match span.style.as_str() {
                    "math" => strip_math_delimiters(&content[start..end]),
                    "footnote_ref" => footnote_ref_mark(&content[start..end]),
                    _ => &content[start..end],
                };
                job.append(slice, 0.0, fmt);
            }
            cursor = end.max(cursor);
        }
        if cursor < len {
            job.append(&content[cursor..], 0.0, base.clone());
        }
    }
    // 手工排版 + 绘制：比 `ui.add(Label)` 多一步，但能精确控制折行宽度，
    // 且为「命中区 / 点击」等扩展留好位置（见图表链接的 `draw_diagram`）。
    //
    // 命中区 id 用调用方 ui 的 id + 固定盐 `"rinline"`：**要求每个块子树
    // 的 `ui.id()` 唯一**（`show()` 里已用 `block.id` 作 `id_salt`）。
    // 若同一父级下多次调用且 sense 可交互（如标题的 `Sense::click()`），
    // id 会相撞 —— egui 会画出红框与「Double use of widget ID」提示
    // （2026-10-10 曾因此让预览标题被套红框，改 `show()` 的 `id_salt` 修复）。
    let galley = ui.painter().layout_job(job);
    let (_id, rect) = ui.allocate_space(galley.size());
    ui.painter().galley(rect.min, galley, base.color);
    ui.interact(rect, ui.id().with("rinline"), sense)
}

/// 给一个行内格式叠上「已完成」装饰（次级色 + 删除线）。
fn done_overlay(theme: &Theme, mut fmt: TextFormat) -> TextFormat {
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
