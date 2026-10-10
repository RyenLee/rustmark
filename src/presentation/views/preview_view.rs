//! 预览区视图（阶段 06 · S6-05 实现）。
//!
//! # 数据流
//!
//! ```text
//! 领域解析（pulldown）→ RenderedBlock → BlockView（DTO）→ 本模块 → egui 控件
//! ```
//!
//! 本模块**不解析 Markdown**，也不碰 domain：只消费 [`BlockView`] 的
//! `kind` 与结构化字段（表格行、列表项、图片地址）。
//! 换解析器或换渲染后端都不应波及这里。
//!
//! # 三种可点击交互
//!
//! 1. 任务列表的复选框——点击回写源码（走 `ReplaceRangeCommand`，与编辑区同源，
//!    因此可撤销、可参与领域撤销栈）；
//! 2. 代码块的复制按钮——把块内容放进剪贴板；
//! 3. 链接——**本期不处理**：`BlockView` 只有纯文本，链接的 URL 未下沉到 DTO，
//!    故不假装可点击（宁可显示成普通文本，也不要点了没反应）。
//!
//! # 缓存
//!
//! 重解析由 [`PreviewUiState`] 的缓存键 + 150ms 防抖控制（FR-01），
//! 键变了才重新请求 `render_preview`；结果存在 UI State 里供后续帧复用。
//! 图片纹理按 URL 缓存在本模块的线程局部表里——解码一张图可能几十毫秒，
//! 每帧重解会让滚动直接卡死。

use std::cell::RefCell;
use std::collections::HashMap;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align, Align2, CornerRadius, FontId, Layout, RichText, ScrollArea, Stroke, Ui};

use crate::app::frame::EditorApp;
use crate::application::document::registry::DocumentContentProvider;
use crate::application::dto::{BlockView, CellAlignView, ListItemView, SpanView, TableView};
use crate::application::markdown::queries::{HighlightSnippetQuery, RenderPreviewQuery};
use crate::presentation::state::scroll_sync_state::{BlockRect, Side};
use crate::presentation::theme::builtin::Theme;
use crate::presentation::theme::mapper::StyleMapper;
use crate::shared::geometry::clamp_to_char_boundaries;

/// 目录条目（`draw_toc` 用）：文档里一个标题块的位置与文本。
#[derive(Clone, Debug)]
struct HeadingEntry {
    /// 标题级别（1–6）。
    level: u8,
    /// 标题文本。
    text: String,
    /// 对应块 ID（点击目录项时用于请求滚动到该块）。
    id: u64,
}

/// 代码块在源码没写语言时的标签。
const CODE_NO_LANG: &str = "text";

/// 引用块的左侧色条宽度。
const QUOTE_BAR_W: f32 = 3.0;

/// 预览图片的最大显示宽度（超过则等比缩放，避免撑破面板）。
const IMAGE_MAX_W: f32 = 620.0;

/// 图片**尚未就绪 / 加载失败**时预留的占位高度（点）。
///
/// 存在理由（FR-02「文档含大量图片时同步不失控」）：若坏图或未加载的图
/// 高度为 0，那么它解码完成（或失败被判定）的那一帧，整页高度会突然变化，
/// 其后所有块的矩形整体下移——滚动同步用的正是这些矩形，于是一次解码
/// 会让预览「跳位」。预留固定高度后，块的纵向布局与加载结果无关。
const IMAGE_PLACEHOLDER_H: f32 = 96.0;

/// 表格单元格的左右内边距（点，单侧）。
///
/// `pub` 供列宽分配的单测换算「内容自然宽 → 含内边距的列宽」。
pub const TABLE_CELL_PAD: f32 = 8.0;
/// 表格相邻两列之间的间距（点，取间距刻度的 `space_md`）。
pub const TABLE_COL_GAP: f32 = 8.0;
/// 压缩时每列的**保底**宽度（点）：低于它一行只放得下两三个字，几乎不可读。
pub const TABLE_MIN_COL_W: f32 = 64.0;

// ============================================================================
// 图片纹理缓存
// ============================================================================

/// 一张图片的加载结果。
#[derive(Clone)]
struct ImageEntry {
    /// 成功解码后的纹理。
    texture: Option<egui::TextureHandle>,
    /// 原始像素尺寸。
    size: [f32; 2],
    /// 失败原因（显示成占位文本，不弹窗、不 panic）。
    error: Option<String>,
}

thread_local! {
    /// 图片引用字符串 → 已解码纹理 / 失败原因。
    ///
    /// 按键是**引用字符串**而不是文档路径：同一张图在多个文档里引用时只解码一次。
    /// 条目数等于文档里独立图片的数量（有界于用户内容），故不设淘汰上限。
    static IMAGE_CACHE: RefCell<HashMap<String, ImageEntry>> = RefCell::new(HashMap::new());
}

/// 取（或首次加载）一张图片。
///
/// 三种来源（本地路径 / 远程 URL / data URI）由 `AssetLoader` 统一处理，
/// 本函数只负责「解码 → 纹理」与失败降级。
fn image_entry(ui: &Ui, app: &EditorApp, url: &str) -> ImageEntry {
    if let Some(cached) = IMAGE_CACHE.with(|c| c.borrow().get(url).cloned()) {
        return cached;
    }

    let base_dir = app.document_dir();
    // 「加载失败」与「解码失败」是两类不同的错误（前者 AssetError、后者 String），
    // 但对预览的意义相同——都降级成占位。故在这里统一成一条消息。
    let failed = |msg: String| ImageEntry {
        texture: None,
        size: [0.0, 0.0],
        error: Some(msg),
    };
    let entry = match app.services.assets.load(url, base_dir.as_deref()) {
        Err(e) => failed(e.to_string()),
        Ok(asset) => match decode(&asset.bytes) {
            Ok(img) => {
                let size = [img.width() as f32, img.height() as f32];
                let texture = ui.ctx().load_texture(
                    format!("preview-image:{url}"),
                    img,
                    egui::TextureOptions::LINEAR,
                );
                ImageEntry {
                    texture: Some(texture),
                    size,
                    error: None,
                }
            }
            Err(e) => failed(e),
        },
    };

    IMAGE_CACHE.with(|c| {
        c.borrow_mut().insert(url.to_string(), entry.clone());
    });
    entry
}

/// 解码位图字节 → egui 颜色图。
///
/// 失败一律返回 `Err` 而**不 panic**：`panic = "abort"` 下坏图会直接终止进程，
/// 而图片解码是外部输入驱动的（损坏的 PNG、伪装成 PNG 的文本）。
fn decode(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("图片解码失败：{e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    ))
}

// ============================================================================
// 入口
// ============================================================================

/// 绘制预览区。
pub fn show(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    ui.painter()
        .rect_filled(ui.max_rect(), CornerRadius::ZERO, theme.bg_primary);

    // 预览区顶部标题栏：与编辑区工具栏**等高**（`editor_toolbar_height`）。
    //
    // 编辑区在问题 3 里把工具栏移到了编辑区顶部，于是它的滚动内容区比预览区
    // 少了一条工具栏的高度——两栏不再对齐。预览区补上等高的标题栏后，
    // 两栏的内容区从同一条水平线起、高度一致（问题 2）。
    pane_header(ui, &theme);

    let Some(doc_id) = app.active_doc else {
        ui.weak("没有打开的文档");
        return;
    };

    // 0) 换文档即重置：清掉上一份文档遗留的块序列与滚动偏移。
    //    这是预览 ↔ 激活 tab 联动的**唯一入口**（见 `PreviewUiState::sync_document`）：
    //    重置会把 `last_key` 清成 `None`，于是下一句的 `first` 为真，本帧必定重解析，
    //    绝不会出现「上一份文档的块多显示一帧」。
    app.preview.sync_document(doc_id.as_u64());

    // 1) 取文档版本号 → 由 UI State 判定是否该重解析（换文档与外观变化立即生效，
    //    同一份文档的内容变化需等 150ms 防抖窗口）。
    let version = app
        .services
        .registry
        .read()
        .ok()
        .and_then(|reg| reg.content_and_revision(doc_id))
        .map_or(0, |(_, v)| v);

    // 缓存键必须带上**文档标识**：`version` 是每份文档各自的计数器，
    // 两份都还没编辑过的文档版本号相同，只比对版本号会让切 tab 后的预览
    // 一直停留在第一份文档上（见 `PreviewCacheKey::doc_id`）。
    let key = crate::presentation::state::preview_ui_state::PreviewCacheKey::new(
        doc_id.as_u64(),
        version,
        &theme,
    );
    let now_ms = app.services.markdown.now().millis();
    let first = app.preview.last_key.is_none();

    if first || app.preview.should_render(key, now_ms, false) {
        match app
            .services
            .markdown
            .render_preview(RenderPreviewQuery { doc_id, version })
        {
            Ok(view) => {
                app.preview.blocks = view.blocks;
                app.preview.set_degraded(view.truncated);
            }
            Err(e) => {
                // 渲染失败不终止：预览区退化成一句话，编辑区照常可用。
                ui.weak(format!("预览渲染失败：{e}"));
                return;
            }
        }
        app.preview.record_render(key, now_ms);
    }

    // 降级提示（FR-06）：源过大时不解析，明确告知而非静默空白。
    if app.preview.degraded {
        ui.weak("文档过大，预览已降级");
    }

    // 2) 把块序列取出再还回：渲染期间 `app` 需要被可变更改（任务项回写），
    //    借用检查器不允许同时持有 `&app.preview.blocks` 与 `&mut app`。
    //    `take` 是零拷贝的，渲染结束后原样放回。
    let blocks = std::mem::take(&mut app.preview.blocks);

    // 目录条目：由**当前块序列**现场收集（标题会随编辑增删，存一份副本必然过期）。
    // 含 `toc` 块自身之外的标题，顺序与文档一致。
    let headings: Vec<HeadingEntry> = blocks
        .iter()
        .filter(|b| b.kind == "heading")
        .map(|b| HeadingEntry {
            level: b.level.unwrap_or(1).clamp(1, 6),
            text: b.content.clone(),
            id: b.id,
        })
        .collect();

    let start_y = if app.preview.scroll_y.is_finite() {
        app.preview.scroll_y
    } else {
        0.0
    };
    let out = ScrollArea::vertical()
        .id_salt("preview-scroll")
        // ⚠️ 必须关掉**横向**自动收缩。写 `.auto_shrink([false, true])` 不是
        // 为了腾地方，而是修一个「预览区滚不动」的缺陷：
        //
        // `ScrollArea::vertical()` 的 `direction_enabled` 是 `[false, true]`
        // （横向关、纵向开），而 `auto_shrink` 默认 `Vec2b::TRUE` 两个方向都开。
        // 于是横向落进 egui 的这一支（`containers/scroll_area.rs`，0.33.3）：
        //
        //   (false, true) => content_size[d]   // **收缩到内容宽度**
        //
        // 滚动区因此只有正文那么宽。实测（本机 1000×640、默认主题）：
        // 面板可用 348pt，纯段落文档的内容宽 81.5pt，滚轮只有落在这 81.5pt
        // 里才算「悬停在滚动区上」（`is_hovering_outer_rect` 用 `outer_rect`
        // 判定），右侧那 260 多 pt 的空白上是滚不动的——用户看到的正是
        // 「预览区滚不动、下半截内容看不到」。横向关掉后滚动区占满面板宽度，
        // 在预览区任意位置滚轮都生效。
        //
        // 纵向仍保持自动收缩：内容不足一屏时不该占满高度（否则会出现
        // 一条永远拖不动的空滚动条）。
        .auto_shrink([false, true])
        .vertical_scroll_offset(start_y)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                let mut rects: Vec<BlockRect> = Vec::with_capacity(blocks.len());
                // 相对原点：滚动内容的第一块顶边即 y = 0。
                // 用「第一块的实际顶边」而不是某个常量，是因为 `ScrollArea`
                // 内部的坐标系已经带上了偏移，绝对 y 每帧都不同。
                let mut origin: Option<f32> = None;
                for block in blocks.iter() {
                    // `ui.scope` 会返回本块占据的**实际矩形**（等价 `min_rect`），
                    // 比「画之前 / 画之后各读一次光标」稳——后者会被块内部
                    // 自己和 `item_spacing` 的交互骗到。
                    let res = ui.scope(|ui| draw_block(app, ui, &theme, block, &headings));
                    let rect = res.response.rect;
                    let base = *origin.get_or_insert(rect.top());
                    rects.push(BlockRect {
                        id: block.id,
                        top: rect.top() - base,
                        bottom: rect.bottom() - base,
                    });
                    // 块间距取刻度 `space_xs`：`scroll_sync` 的块矩形换算
                    // 假定块与块之间就是这个值（见其文件头注释），改这里要同步那边。
                    ui.add_space(theme.metrics.space_xs);
                }
                rects
            })
            .inner
        });

    app.preview.blocks = blocks;
    app.preview.rects = out.inner;
    app.preview.scroll_y = out.state.offset.y;
    app.ui
        .sync
        .set_geometry(Side::Preview, out.content_size.y, out.inner_rect.height());
}

/// 预览区顶部标题栏：与编辑区工具栏等高，保证两栏内容区对齐（问题 2）。
///
/// 高度**必须**与 [`crate::presentation::theme::builtin::Metrics::editor_toolbar_height`]
/// 同源，不能写死一个数字：编辑区工具栏高度是主题的一部分，两处各写一遍
/// 会在换主题尺寸时重新错位。
fn pane_header(ui: &mut Ui, theme: &Theme) {
    let m = theme.metrics;
    let height = m.editor_toolbar_height;
    ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            // 与编辑区工具栏同一条左边线（`space_md`，见 `draw_editor_toolbar`）：
            // 两栏内容从同一竖线起，顶栏才像「一对」。
            ui.add_space(m.space_md);
            ui.strong("预览");
        });
    });
    ui.separator();
}

/// 按块类型分发渲染器。
fn draw_block(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    headings: &[HeadingEntry],
) {
    match block.kind.as_str() {
        "heading" => draw_heading(app, ui, theme, block),
        "paragraph" => draw_paragraph(ui, theme, block),
        "quote" => draw_quote(ui, theme, block),
        "code_block" => draw_code_block(app, ui, theme, block),
        "list" => draw_list(ui, theme, block),
        "task_list" => draw_task_list(app, ui, theme, block),
        "table" => {
            // 结构化缺失（理论上不该发生）时退回纯文本，总比空白好。
            match &block.table {
                Some(table) => draw_table(ui, theme, table),
                None => draw_paragraph(ui, theme, block),
            }
        }
        "image" => draw_image(app, ui, theme, block),
        "rule" => draw_rule(ui, theme),
        "footnote" => draw_footnote(ui, theme, block),
        "definition_list" => draw_definition_list(ui, theme, block),
        "math" => draw_math(ui, theme, block),
        "diagram" => draw_diagram(ui, theme, block),
        "toc" => draw_toc(app, ui, theme, block, headings),
        other => draw_raw(ui, theme, block, other),
    }
}

// ============================================================================
// 行内富文本
// ============================================================================

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
struct Typography<'a> {
    size: f32,
    line_height: f32,
    base_style: &'a str,
}

fn render_inline_text(
    ui: &mut Ui,
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    typo: Typography<'_>,
    sense: egui::Sense,
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
    // 无样式片段：走单标签快路径（与旧实现完全一致，零额外开销）。
    if spans.is_empty() {
        let mut text = RichText::new(content).size(size).color(base.color);
        if done {
            text = text.strikethrough();
        }
        return ui.add(egui::Label::new(text).sense(sense));
    }
    let mut job = LayoutJob {
        wrap: TextWrapping {
            max_width: ui.available_width().max(0.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let len = content.len();
    let mut cursor = 0usize;
    for span in spans {
        // 越界 / 落在多字节字符中间的区间会被吸附或退化为空——跳过而不是崩。
        let Some((start, end)) =
            clamp_to_char_boundaries(content, span.range.start.min(len), span.range.end.min(len))
        else {
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
            let slice = if span.style == "math" {
                strip_math_delimiters(&content[start..end])
            } else {
                &content[start..end]
            };
            job.append(slice, 0.0, fmt);
        }
        cursor = end.max(cursor);
    }
    if cursor < len {
        job.append(&content[cursor..], 0.0, base);
    }
    ui.add(egui::Label::new(job).sense(sense))
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

// ============================================================================
// 各块渲染器
// ============================================================================

/// 标题：级别决定字号与留白（h1 最大，逐级递减）。
///
/// 标题**可点击**，点击后编辑区跳到对应行（FR-02 验收用例）。
/// 这是预览区唯一「点击跳转」的入口——正文段落不给点击，
/// 否则用户在预览里想选中文本时会被跳转打断。
fn draw_heading(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, block: &BlockView) {
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
    let resp = render_inline_text(
        ui,
        theme,
        &block.content,
        &block.spans,
        Typography {
            size,
            line_height: size * theme.metrics.preview_line_height,
            base_style: "heading",
        },
        egui::Sense::click(),
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
fn draw_paragraph(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let size = theme.metrics.preview_font_size;
    render_inline_text(
        ui,
        theme,
        &block.content,
        &block.spans,
        Typography {
            size,
            line_height: size * theme.metrics.preview_line_height,
            base_style: "",
        },
        egui::Sense::hover(),
    );
}

/// 引用块：左侧色条 + 斜体次级色。
///
/// 注意：引用块的 `content` 通常是空的（正文在它的子段落块里，见解析器注释），
/// 此时这里只画一根色条作为视觉容器。
fn draw_quote(ui: &mut Ui, theme: &Theme, block: &BlockView) {
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
                    render_inline_text(
                        ui,
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
                    );
                }
            });
        });
}

/// 代码块：语言标签 + 复制按钮 + 语法高亮。
fn draw_code_block(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, block: &BlockView) {
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
            draw_highlighted_code(ui, theme, &block.content, spans.as_deref());
        });
}

/// 把带 span 的代码文本画成彩色文本（无 span 时退化为等宽纯色）。
fn draw_highlighted_code(
    ui: &mut Ui,
    theme: &Theme,
    text: &str,
    spans: Option<&[crate::application::dto::SpanView]>,
) {
    let wrap_width = ui.available_width();
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
fn highlighted_code_job(
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

/// 列表每加深一级增加的缩进（点）。
///
/// 取刻度 `space_lg` 而不是固定像素：缩进必须与主题的间距体系同源，
/// 否则换主题后列表会与段落、引用条的左缘参差不齐。
fn list_indent_step(theme: &Theme) -> f32 {
    theme.metrics.space_lg
}

/// 列表标记（`•` / `1.`）所在列的固定宽度。
///
/// 必须**固定**并右对齐：`1.` 与 `10.` 宽度差一个字符，不固定列宽的话
/// 两位数序号会把该项文本整体右推一格，同一列表里文字左缘就参差了。
fn list_marker_width(theme: &Theme) -> f32 {
    theme.metrics.preview_font_size * 1.8
}

/// 无序列表按层级换用不同符号（`•` → `◦` → `▪`，三层一循环）。
///
/// 只靠缩进区分层级在**窄面板**里不够用：面板一窄，每级缩进被压到几个点，
/// 加上软换行后子项可能顶到父项的缩进位，视觉上就分不清谁是谁的孩子。
/// 换符号是零成本的第二重线索。
fn bullet_for(depth: u8) -> &'static str {
    match depth % 3 {
        0 => "•",
        1 => "◦",
        _ => "▪",
    }
}

/// 列表：有序用递增编号，无序用圆点；**按 `depth` 缩进**、按 `ordinal` 编号。
///
/// # 缩进为什么由数据给出而不是从文本推
///
/// 之前的实现只画一层 `space_xl` 缩进，所有层级看起来都是平的——因为
/// `BlockView` 当初根本没带层级信息。现在解析器把整棵列表树**拍平**进
/// 同一个 `items`（见其 `root_list_index`），层级只存在于 `ListItemView::depth`：
/// 想从 `text` 的前导空格反推是行不通的（pulldown 已把标记与缩进吃掉）。
fn draw_list(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let marker_w = list_marker_width(theme);
    for item in block.items.iter() {
        ui.horizontal(|ui| {
            // 基准缩进 + 每级一格。
            ui.add_space(theme.metrics.space_xl + f32::from(item.depth) * list_indent_step(theme));
            // 标记列右对齐（理由见 `list_marker_width`）。
            ui.add_sized(
                egui::vec2(marker_w, theme.metrics.preview_font_size),
                egui::Label::new(
                    RichText::new(match item.ordinal {
                        Some(n) => format!("{n}."),
                        None => bullet_for(item.depth).to_string(),
                    })
                    .size(theme.metrics.preview_font_size)
                    .color(theme.syn_marker),
                )
                .halign(Align::RIGHT),
            );
            draw_item_text(ui, theme, item);
        });
    }
}

/// 任务列表：复选框可点击，点击后回写源码。
fn draw_task_list(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, block: &BlockView) {
    for item in block.items.iter() {
        let mut checked = item.checked.unwrap_or(false);
        ui.horizontal(|ui| {
            ui.add_space(theme.metrics.space_xl + f32::from(item.depth) * list_indent_step(theme));
            if ui.checkbox(&mut checked, "").changed() {
                toggle_task(app, item, checked);
            }
            draw_item_text(ui, theme, item);
        });
    }
}

/// 切换一个任务项的勾选状态：把源码里的 `[ ]` ↔ `[x]` 换掉。
///
/// 走与编辑区完全相同的 `ReplaceRangeCommand`，因此可撤销、会推进文档版本号、
/// 会触发预览重解析——不会出现「预览勾上了、源码没变」的两处真相漂移。
fn toggle_task(app: &mut EditorApp, item: &ListItemView, checked: bool) {
    let text = app.editor_text.clone();
    let Some(from) = task_marker_offset(&text, item.start, item.end) else {
        return;
    };
    app.replace_source(from, from + 3, if checked { "[x]" } else { "[ ]" });
}

/// 在文本的一段区间里定位任务标记 `[ ]` / `[x]`，返回其**绝对**字节偏移。
///
/// 只认区间内**首个**标记：一个列表项里出现多个复选框是极端的写法，
/// 而「改错位置」的代价（把用户别的文字换成 `[x]`）远大于「不生效」。
///
/// 区间超界时按文本长度裁剪——`BlockView` 的区间来自上一次解析，
/// 之后的编辑可能让它们落在文本之外（此时直接放弃，下一帧解析结果会自愈）。
pub fn task_marker_offset(text: &str, start: usize, end: usize) -> Option<usize> {
    let len = text.len();
    let start = start.min(len);
    let end = end.min(len).max(start);
    // 逐字节扫描而非 `char_indices`：标记的括号是 ASCII，
    // 但 `find` 的结果必须是**字节**偏移才能直接用于替换。
    let bytes = &text.as_bytes()[start..end];
    let pos = bytes.windows(3).position(|w| w == b"[ ]" || w == b"[x]")?;
    Some(start + pos)
}

/// 列表项文本：走行内富文本（`- **粗体**` 必须与正文里的粗体一致），
/// 已勾选的项叠上删除线 + 次级色。
fn draw_item_text(ui: &mut Ui, theme: &Theme, item: &ListItemView) {
    let size = theme.metrics.preview_font_size;
    render_inline_text(
        ui,
        theme,
        &item.text,
        &item.spans,
        Typography {
            size,
            line_height: size * theme.metrics.preview_line_height,
            base_style: if item.checked == Some(true) {
                "task_done"
            } else {
                ""
            },
        },
        egui::Sense::hover(),
    );
}

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
fn draw_table(ui: &mut Ui, theme: &Theme, table: &TableView) {
    let m = theme.metrics;
    egui::Frame::new()
        .stroke(hairline(theme.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(m.pane_padding as i8))
        .show(ui, |ui| {
            let available = ui.available_width();

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

            // 行与行**不留间距**，否则列间竖线会在每行交界处断开，网格变成虚线。
            // 行距改由单元格自己的纵向内边距提供（见 `table_row`）。
            ui.spacing_mut().item_spacing.y = 0.0;

            let row_count = table.rows.len();
            let last = row_count.saturating_sub(1);
            let cols_layout = ColumnLayout {
                widths: &widths,
                aligns: &table.aligns,
            };
            if !table.header.is_empty() {
                table_row(
                    ui,
                    theme,
                    &table.header,
                    &table.header_spans,
                    cols_layout,
                    true,
                    true,
                );
            }
            for (r, row) in table.rows.iter().enumerate() {
                // 最后一行**不画底线**：它与外框的下边重合，画两遍会在圆角处
                // 露出一道比别处亮的线。
                table_row(
                    ui,
                    theme,
                    row,
                    table.rows_spans.get(r).map_or(&[][..], Vec::as_slice),
                    cols_layout,
                    false,
                    r < last,
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
/// * `available`：表格可用总宽。
/// * 返回每列的最终宽度（**含**左右内边距 [`TABLE_CELL_PAD`]）。
///
/// 两条不变量（各有单测钉住；「列宽和」均不含列间距——间距由布局层的
/// `item_spacing` 提供，值同为 [`TABLE_COL_GAP`]，故分配只对
/// 「面板宽 − 列间距」负责）：
///
/// 1. `sum(widths) + 列间距 == min(需求宽, available)`——放得下时占满面板、
///    放不下时正好用满，既不溢出也不留大段空白；
/// 2. 放不下时每列 `>= [`TABLE_MIN_COL_W`]`——内容再长也不把短列挤没。
///
/// 分配策略：长列拿大头（按内容占比），这正是「长内容展示合理」的实现点。
pub fn table_column_distribution(natural: &[f32], available: f32) -> Vec<f32> {
    let cols = natural.len();
    if cols == 0 {
        return Vec::new();
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
            // 窗口极窄：预算连保底都给不起，宁可溢出也不把内容挤没了。
            return vec![TABLE_MIN_COL_W; cols];
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
fn table_row(
    ui: &mut Ui,
    theme: &Theme,
    cells: &[String],
    cell_spans: &[Vec<SpanView>],
    cols_layout: ColumnLayout<'_>,
    header: bool,
    bottom_line: bool,
) {
    let ColumnLayout { widths, aligns } = cols_layout;
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
                let res = ui.allocate_ui_with_layout(
                    egui::vec2(w, 0.0),
                    Layout::left_to_right(Align::TOP),
                    |ui| {
                        ui.add_space(TABLE_CELL_PAD);
                        ui.with_layout(Layout::top_down(align), |ui| {
                            ui.set_min_width(w - 2.0 * TABLE_CELL_PAD);
                            ui.add_space(pad_y);
                            // 单元格文本走行内富文本：`| a **粗** |` 里的粗体
                            // 之前被当作纯文本直接打印，与正文里的粗体表现不一致。
                            render_inline_text(
                                ui,
                                theme,
                                cell,
                                spans,
                                Typography {
                                    size,
                                    line_height,
                                    // 表头用 `strong` 基准色（egui 的「强调」=
                                    // 加深颜色而非换粗体字形，与旧实现观感一致）。
                                    base_style: if header { "strong" } else { "" },
                                },
                                egui::Sense::hover(),
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

/// 1px 实线描边（发丝线）。
///
/// 全文件统一走这里，避免每个 `Stroke::new(1.0, ...)` 触发 f32 字面量回退告警。
fn hairline(color: egui::Color32) -> Stroke {
    Stroke::new(1.0_f32, color)
}

/// 图片：三种来源统一经 `AssetLoader`；失败显示占位而非崩溃。
fn draw_image(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let url = block.url.clone().unwrap_or_default();
    let alt = block.alt.clone().unwrap_or_default();
    if url.is_empty() {
        ui.weak(format!("图片地址为空：{alt}"));
        return;
    }

    // 图片按「预览区可用宽度」与上限 `IMAGE_MAX_W` 的较小值缩放：窄面板里图片
    // 缩到区域宽（不再溢出被裁），宽面板里也不超过上限（问题 2：预览未适配区域宽度）。
    let max_w = ui.available_width().min(IMAGE_MAX_W);
    let entry = image_entry(ui, app, &url);
    match entry.texture {
        Some(texture) => {
            let [w, h] = entry.size;
            // 等比缩放到面板宽度内。
            let scale = (max_w / w.max(1.0)).min(1.0);
            ui.add(
                egui::Image::new(egui::load::SizedTexture::from_handle(&texture))
                    .fit_to_exact_size(egui::vec2(w * scale, h * scale)),
            );
        }
        None => {
            // 占位高度**固定**（见 [`IMAGE_PLACEHOLDER_H`]）：坏图与未就绪的图
            // 都占同样的版面，滚动同步的块矩形才不会随加载结果跳变。
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(max_w, IMAGE_PLACEHOLDER_H), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), theme.bg_tertiary);
            let reason = entry.error.unwrap_or_default();
            // 文案画在占位框内（`allocate_ui_at_rect` 不推进光标，故不改变高度）。
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(rect.shrink(theme.metrics.space_md)),
                |ui| {
                    ui.label(
                        RichText::new(format!(
                            "图片无法加载：{}\n{}",
                            if alt.is_empty() { &url } else { &alt },
                            reason
                        ))
                        .size(theme.metrics.ui_font_size)
                        .color(theme.warning),
                    );
                },
            );
        }
    }
}

/// 分割线。
fn draw_rule(ui: &mut Ui, theme: &Theme) {
    ui.add_space(theme.metrics.space_sm);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.top(), hairline(theme.border));
    ui.add_space(theme.metrics.space_sm);
}

/// 脚注定义：小字 + 次级色。
/// 目录（`@[TOC](标题)` / `[TOC]`）：标题列表 + 点击跳转。
///
/// # 条目为什么现场收集
///
/// `headings` 由 `show()` 从**本帧的块序列**收集，而不是解析期写死在目录块里：
/// 标题会随编辑增删、改名，任何副本都会在下一帧过期；而目录**本来就该**
/// 与当前文档结构一致（这正是它存在的意义）。
fn draw_toc(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    headings: &[HeadingEntry],
) {
    let m = theme.metrics;
    let title = if block.content.trim().is_empty() {
        "目录"
    } else {
        block.content.trim()
    };
    egui::Frame::new()
        .fill(theme.bg_tertiary)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(
            m.pane_padding as i8,
            m.space_md as i8,
        ))
        .show(ui, |ui| {
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(m.preview_font_size)
                    .color(theme.text_primary),
            );
            ui.add_space(m.space_sm);
            if headings.is_empty() {
                ui.weak("（文档里还没有标题）");
                return;
            }
            for h in headings {
                // 缩进按层级递进（一级不缩进）：层级感靠左缘位置表达。
                let indent = (h.level.saturating_sub(1)) as f32 * m.space_md;
                let resp = ui
                    .horizontal(|ui| {
                        ui.add_space(indent);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&h.text)
                                    .size(m.preview_font_size - 1.0)
                                    .color(if h.level <= 2 {
                                        theme.text_primary
                                    } else {
                                        theme.text_secondary
                                    }),
                            )
                            .sense(egui::Sense::click()),
                        )
                    })
                    .inner;
                if resp.clicked() {
                    app.ui.sync.request_click(h.id);
                }
                if resp.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }
            }
        });
    ui.add_space(m.space_xs);
}

/// 脚注定义：`[1] 正文` 形式，小字 + 次级色，行内样式照常生效。
///
/// # 标签为什么要单独画
///
/// 解析器把 `[^1]: 脚注内容` 收成 `Footnote { label: "1" }`，**正文里不含
/// `[^1]`**（那是标签，被 pulldown 单独给出）。若只画 `content`，用户看到的
/// 是一条没有编号的孤立句子，与正文里的 `[^1]` 引用对不上号；
/// 若把标签拼进 `content`，它又会参与搜索命中与字数统计。
/// 故由渲染层在这里把两者并排画出来。
fn draw_footnote(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let m = theme.metrics;
    let size = m.preview_font_size - 2.0;
    ui.horizontal_top(|ui| {
        if let Some(label) = block.label.as_deref().filter(|l| !l.is_empty()) {
            ui.label(
                RichText::new(format!("[{label}]"))
                    .size(size)
                    .color(theme.syn_marker),
            );
        }
        ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
            ui.set_min_width(ui.available_width().max(0.0));
            render_inline_text(
                ui,
                theme,
                &block.content,
                &block.spans,
                Typography {
                    size,
                    line_height: size * m.preview_line_height,
                    base_style: "quote",
                },
                egui::Sense::hover(),
            );
        });
    });
}

/// 定义列表（`术语` + `: 定义`）：术语加粗、定义缩进在术语下方。
///
/// 不套项目符号：定义列表的语义是「词条—释义」，画成 `• 术语: 定义`
/// 就与普通无序列表混同了，而且术语与释义的从属关系在横向上会被拉平。
fn draw_definition_list(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let m = theme.metrics;
    let size = m.preview_font_size;
    let line_height = size * m.preview_line_height;
    for entry in &block.definitions {
        // 术语：用与标题同族的强调色，与下面的释义拉开层次。
        render_inline_text(
            ui,
            theme,
            &entry.term,
            &entry.term_spans,
            Typography {
                size,
                line_height,
                base_style: "strong",
            },
            egui::Sense::hover(),
        );
        for body in &entry.definitions {
            ui.horizontal_top(|ui| {
                // 释义缩进一格（与列表的缩进刻度同源），并画一条短竖线
                // 作为「属于上面那个术语」的视觉连缀。
                let indent = m.space_lg;
                ui.add_space(indent);
                let (bar, _) = ui.allocate_exact_size(
                    egui::vec2(QUOTE_BAR_W, ui.available_height().max(line_height)),
                    egui::Sense::hover(),
                );
                ui.painter().vline(
                    bar.center().x,
                    bar.y_range(),
                    Stroke::new(1.0_f32, theme.border),
                );
                ui.add_space(m.space_xs);
                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                    ui.set_min_width(ui.available_width().max(0.0));
                    render_inline_text(
                        ui,
                        theme,
                        &body.text,
                        &body.spans,
                        Typography {
                            size,
                            line_height,
                            base_style: "",
                        },
                        egui::Sense::hover(),
                    );
                });
            });
        }
        ui.add_space(m.space_xs);
    }
}

/// 数学公式：**内置排版引擎**渲染（见 [`super::math`]）。
///
/// # 这次改了什么
///
/// 旧实现只把公式源码用等宽字打出来，并在旁边写「导出 HTML 后由浏览器渲染」——
/// 用户看到的是 `\int_0^1 x^2 dx` 这串字符，而不是积分式。这正是问题 3 的根因：
/// **解析层一直是对的**（`Math { display: true }` 与公式本体都拿到了），
/// 缺的是渲染。
///
/// 现在：
/// - 排版成功 → 用内置引擎画成二维公式（分数线、上下标、根号、矩阵都各就各位）；
/// - 排版失败（子集外的写法）→ **回退**到源码卡，并**写明原因**
///   （`\notarealcommand` 这类不支持的命令名会出现在提示里）。
///
/// # 为什么保留「复制」
///
/// 排版后的公式是**图**，选不中也复制不了。用户把公式贴到别处时需要的仍是
/// LaTeX 源码，故按钮留着。
fn draw_math(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let m = theme.metrics;
    let display = block.display.unwrap_or(false);
    // 块级公式放大一档：它是版面上的「展示元素」，与正文同号会显得局促。
    let size = if display {
        m.preview_font_size * 1.12
    } else {
        m.preview_font_size
    };
    let layout = {
        let ui_ref: &Ui = ui;
        let measure = move |text: &str, size: f32| {
            ui_ref
                .painter()
                .layout_no_wrap(
                    text.to_owned(),
                    FontId::proportional(size),
                    egui::Color32::WHITE,
                )
                .size()
                .x
        };
        super::math::layout(&block.content, size, display, &measure)
    };

    let Ok(math_layout) = layout else {
        // 子集外的写法：退回源码卡，并**说明为什么**没排版。
        // 只显示源码、不说原因，用户会以为是自己公式写错了。
        draw_unrendered_source(
            ui,
            theme,
            block,
            "公式",
            layout.as_ref().err().map(ToString::to_string),
        );
        return;
    };

    if !display {
        // 行内公式：跟随正文基线，不套框。
        let h = super::math::fitted_height(&math_layout, ui.available_width());
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
        super::math::paint(
            ui.painter(),
            rect,
            &math_layout,
            theme.text_primary,
            Align2::LEFT_CENTER,
        );
        return;
    }

    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(
            m.pane_padding as i8,
            m.space_md as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("公式")
                        .size(m.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });
            let h = super::math::fitted_height(&math_layout, ui.available_width());
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
            super::math::paint(
                ui.painter(),
                rect,
                &math_layout,
                theme.text_primary,
                Align2::CENTER_CENTER,
            );
        });
}

/// 图表（Mermaid）：**内置布局引擎**渲染（见 [`super::mermaid`]）。
///
/// 与公式同一套思路：解析层本来就给出了 `Diagram { kind: "mermaid" }` 与源码，
/// 旧实现却只把源码当代码块打出来、再附一句「导出后由浏览器渲染」。
/// 现在直接画成节点—连线图（分层布局 + 正交走线 + 箭头 + 边标签）。
///
/// 子集外的图种（`sequenceDiagram` 等）与 `subgraph` 回退到源码卡并说明原因。
fn draw_diagram(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let m = theme.metrics;
    let engine = block.diagram.as_deref().unwrap_or("diagram");
    let layout = {
        let ui_ref: &Ui = ui;
        let measure = move |text: &str, size: f32| {
            ui_ref
                .painter()
                .layout_no_wrap(
                    text.to_owned(),
                    FontId::proportional(size),
                    egui::Color32::WHITE,
                )
                .size()
                .x
        };
        super::mermaid::layout(&block.content, m.preview_font_size, &measure)
    };

    let Ok(graph) = layout else {
        draw_unrendered_source(
            ui,
            theme,
            block,
            engine,
            layout.as_ref().err().map(ToString::to_string),
        );
        return;
    };

    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(m.pane_padding as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(engine)
                        .size(m.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });
            let max_w = ui.available_width();
            let h = super::mermaid::fitted_height(&graph, max_w);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(max_w, h), egui::Sense::hover());
            let style = super::mermaid::MermaidStyle {
                node_fill: theme.bg_tertiary,
                node_stroke: theme.border,
                text: theme.text_primary,
                edge: theme.text_secondary,
                label_bg: theme.code_bg,
            };
            super::mermaid::paint(ui.painter(), rect, &graph, &style);
        });
}

/// 「这块没被渲染」的降级卡：源码 + 类型标注 + **失败原因**。
///
/// # 为什么必须带上原因
///
/// 只显示源码时用户的下一步动作是「去查 Mermaid / LaTeX 语法」，
/// 而那多半是浪费时间——**不支持的是本编辑器，不是他的写法**。
/// 把原因写出来（`不支持的图表写法：sequenceDiagram`）就能立刻说清责任归属。
fn draw_unrendered_source(
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    label: &str,
    reason: Option<String>,
) {
    let m = theme.metrics;
    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(m.pane_padding as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(label)
                        .size(m.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });
            draw_highlighted_code(ui, theme, &block.content, None);
            if let Some(reason) = reason {
                ui.add_space(m.space_xs);
                ui.label(
                    RichText::new(format!("未能排版：{reason}"))
                        .size(m.ui_font_size - 1.0)
                        .color(theme.warning),
                );
            }
        });
}

/// 未支持的扩展块（原始 HTML）：等宽原样显示并标注类型。
///
/// 保留原文而不是隐藏——用户至少能看到自己写了什么，也知道这块没被渲染。
fn draw_raw(ui: &mut Ui, theme: &Theme, block: &BlockView, kind: &str) {
    ui.label(
        RichText::new(&block.content)
            .font(FontId::monospace(theme.metrics.preview_font_size - 1.0))
            .color(theme.text_secondary),
    );
    if let Some(marker) = kind.strip_prefix("raw:") {
        ui.weak(format!("未渲染的 {marker} 块"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::dto::{RangeView, SpanView};

    /// DEF-04：span 边界落在多字节字符中间时，预览区代码块不得 panic。
    ///
    /// `panic = "abort"` 下 panic 即进程终止，故这条是崩溃级回归。
    /// 生产路径里 span 与文本同源，缺陷不会触发；但「同源」只是约定，
    /// 缓存版本错位就能打破它——表现层必须自己扛住。
    #[test]
    fn highlighted_code_job_survives_non_char_boundary_spans() {
        let theme = Theme::dark();
        // "中文abc"：中=0..3、文=3..6。1 与 5 都落在字符内部。
        let text = "中文abc";
        assert!(
            !text.is_char_boundary(1) && !text.is_char_boundary(5),
            "用例前提：这两个偏移必须是非字符边界"
        );
        let spans = vec![SpanView {
            range: RangeView::new(1, 5),
            style: "keyword".to_string(),
        }];
        let job = highlighted_code_job(&theme, text, Some(&spans), f32::INFINITY);
        // 不 panic 即通过；再断言文本被完整覆盖（少上色可以，丢字符不行）。
        let covered: String = job
            .sections
            .iter()
            .map(|s| job.text.get(s.byte_range.clone()).unwrap_or(""))
            .collect();
        assert_eq!(covered, text, "吸附边界后不得丢字符");
    }

    /// 无 span 时退化为纯文本，覆盖整段（防止吸附逻辑误伤正常路径）。
    #[test]
    fn highlighted_code_job_covers_whole_text_without_spans() {
        let theme = Theme::dark();
        let text = "fn main() {}\n";
        let job = highlighted_code_job(&theme, text, None, f32::INFINITY);
        let covered: String = job
            .sections
            .iter()
            .map(|s| job.text.get(s.byte_range.clone()).unwrap_or(""))
            .collect();
        assert_eq!(covered, text);
    }
}
