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

use egui::{Align, CornerRadius, FontId, Layout, RichText, ScrollArea, Stroke, Ui};

use crate::app::frame::EditorApp;
use crate::application::document::registry::DocumentContentProvider;
use crate::application::dto::{BlockView, CellAlignView, ListItemView, TableView};
use crate::application::markdown::queries::{HighlightSnippetQuery, RenderPreviewQuery};
use crate::presentation::state::scroll_sync_state::{BlockRect, Side};
use crate::presentation::theme::builtin::Theme;
use crate::presentation::theme::mapper::StyleMapper;
use crate::shared::geometry::clamp_to_char_boundaries;

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

    let start_y = if app.preview.scroll_y.is_finite() {
        app.preview.scroll_y
    } else {
        0.0
    };
    let out = ScrollArea::vertical()
        .id_salt("preview-scroll")
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
                    let res = ui.scope(|ui| draw_block(app, ui, &theme, block));
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
fn draw_block(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, block: &BlockView) {
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
        "math" => draw_math(ui, theme, block),
        "diagram" => draw_diagram(ui, theme, block),
        other => draw_raw(ui, theme, block, other),
    }
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
    let resp = ui.add(
        egui::Label::new(
            RichText::new(&block.content)
                .size(size)
                .color(theme.syn_heading)
                .strong(),
        )
        .sense(egui::Sense::click()),
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

/// 段落：按可用宽度折行。
fn draw_paragraph(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    ui.label(
        RichText::new(&block.content)
            .size(theme.metrics.preview_font_size)
            .color(theme.text_primary)
            .line_height(Some(
                theme.metrics.preview_font_size * theme.metrics.preview_line_height,
            )),
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
                    ui.label(
                        RichText::new(&block.content)
                            .italics()
                            .color(theme.syn_quote)
                            .size(theme.metrics.preview_font_size),
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

/// 列表：有序用递增编号，无序用圆点。
fn draw_list(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    for (i, item) in block.items.iter().enumerate() {
        ui.horizontal(|ui| {
            // 列表缩进取刻度 `space_xl`：与设置对话框的分区缩进同一来源。
            ui.add_space(theme.metrics.space_xl);
            let marker = if block.ordered {
                format!("{}.", i + 1)
            } else {
                "•".to_string()
            };
            ui.label(
                RichText::new(marker)
                    .size(theme.metrics.preview_font_size)
                    .color(theme.syn_marker),
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
            ui.add_space(theme.metrics.space_xl);
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

/// 列表项文本：已勾选的加删除线并转次级色。
fn draw_item_text(ui: &mut Ui, theme: &Theme, item: &ListItemView) {
    let mut text = RichText::new(&item.text).size(theme.metrics.preview_font_size);
    text = if item.checked == Some(true) {
        text.color(theme.text_secondary).strikethrough()
    } else {
        text.color(theme.text_primary)
    };
    ui.label(text);
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

            // —— 表头 + 分隔线 + 表体 ——
            table_row(ui, theme, &table.header, &widths, &table.aligns, true);
            let (line, _) =
                ui.allocate_exact_size(egui::vec2(available, 1.0), egui::Sense::hover());
            ui.painter()
                .hline(line.x_range(), line.top(), hairline(theme.border));
            for row in &table.rows {
                table_row(ui, theme, row, &widths, &table.aligns, false);
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

/// 表格的一行。`header = true` 时文字用强调色（egui 的 `strong` 即「加深颜色」）。
///
/// 每个单元格先按列宽**显式占位**（`allocate_ui_with_layout`，高度给 0、
/// 内容向下撑开），再在内部按列对齐画文字——宽度是我们算出来的，
/// 不依赖任何布局施舍；行高自动取最高单元格，顶部对齐。
fn table_row(
    ui: &mut Ui,
    theme: &Theme,
    cells: &[String],
    widths: &[f32],
    aligns: &[CellAlignView],
    header: bool,
) {
    let size = theme.metrics.preview_font_size;
    ui.horizontal(|ui| {
        for (c, cell) in cells.iter().enumerate().take(widths.len()) {
            let w = widths[c];
            let align = align_of(aligns.get(c));
            ui.allocate_ui_with_layout(
                egui::vec2(w, 0.0),
                Layout::left_to_right(Align::TOP),
                |ui| {
                    ui.add_space(TABLE_CELL_PAD);
                    ui.with_layout(Layout::top_down(align), |ui| {
                        ui.set_min_width(w - 2.0 * TABLE_CELL_PAD);
                        let mut text = RichText::new(cell).size(size);
                        if header {
                            text = text.strong();
                        }
                        ui.label(text);
                    });
                    ui.add_space(TABLE_CELL_PAD);
                },
            );
        }
    });
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
fn draw_footnote(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    ui.label(
        RichText::new(&block.content)
            .size(theme.metrics.preview_font_size - 2.0)
            .color(theme.text_secondary)
            .italics(),
    );
}

/// 数学公式：结构化公式卡（零依赖，不做像素级排版）。
///
/// 编辑器内不内嵌 KaTeX / MathJax——那会引入 JS 运行时与网络依赖。
/// 这里给出的是「可辨认的公式源码 + 类型标注」：行内公式随正文基线排版，
/// 块级公式独立居中成卡。用户要排版效果时导出 HTML，由浏览器侧渲染。
fn draw_math(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let display = block.display.unwrap_or(false);
    let body = RichText::new(&block.content)
        .font(FontId::monospace(theme.metrics.preview_font_size))
        .color(theme.syn_code);

    if !display {
        // 行内公式跟正文走，不套框，避免打断段落节奏。
        ui.label(body);
        return;
    }

    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(
            theme.metrics.pane_padding as i8,
            theme.metrics.space_md as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("公式")
                        .size(theme.metrics.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });
            ui.add(egui::Label::new(body).selectable(true));
        });
}

/// 图表（Mermaid）：图表卡 + 等宽源码。
///
/// 与代码块的区别在于**语义标注**：用户写 ` ```mermaid ` 是想出图，
/// 看到「未渲染的图表源码」比看到一个普通代码块更能解释预期落差。
/// 真正出图交给导出 HTML（浏览器端 Mermaid 运行时）。
fn draw_diagram(ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let engine = block.diagram.as_deref().unwrap_or("diagram");

    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(theme.metrics.pane_padding as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(engine)
                        .size(theme.metrics.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });
            draw_highlighted_code(ui, theme, &block.content, None);
            ui.weak("图表源码（导出 HTML 后由浏览器渲染）");
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
