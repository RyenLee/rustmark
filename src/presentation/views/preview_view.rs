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
//! 3. 链接——正文内的 Markdown 链接**不处理**：`BlockView` 只有纯文本，
//!    链接的 URL 未下沉到 DTO，故不假装可点击（宁可显示成普通文本，
//!    也不要点了没反应）；
//! 4. 图表内链接——**已处理**：Mermaid 图表经 `mermaid::layout` 返回
//!    `hotspots`，`draw_diagram` 把它们映射成可点击交互区，点击在新标签页打开
//!    （覆盖 `click` 指令与标签内 `[文本](url)` 两类来源）。
//!
//! # 缓存
//!
//! 重解析由 [`PreviewUiState`] 的缓存键 + 150ms 防抖控制（FR-01），
//! 键变了才重新请求 `render_preview`；结果存在 UI State 里供后续帧复用。
//! 图片纹理按 URL 缓存在本模块的线程局部表里——解码一张图可能几十毫秒，
//! 每帧重解会让滚动直接卡死。
//!
//! # 模块结构
//!
//! 按渲染区块拆成私有子模块（本 facade 只保留入口、块分发、图片缓存与
//! 全区块共享的发丝线）：
//!
//! - `inline`：行内富文本（span 格式、`LayoutJob` 排版、分隔符规范化）；
//! - `code`：代码块与语法高亮；
//! - `list`：列表与任务列表（含回写偏移纯函数）；
//! - `table`：GFM 表格测量、列宽分配与网格绘制；
//! - `media`：图片块；
//! - `blocks`：标题 / 段落 / 引用；
//! - `toc`：分割线 / 目录 / 脚注 / 定义列表 / 原始块；
//! - `embedded`：数学公式与 Mermaid 图表（内置引擎 + 源码卡降级）。

mod blocks;
mod code;
mod embedded;
mod inline;
mod list;
mod media;
mod table;
mod toc;

pub use inline::{footnote_ref_mark, strip_math_delimiters};
pub use list::task_marker_offset;
pub use table::table_column_distribution;

use std::cell::RefCell;
use std::collections::HashMap;

use egui::{Align, CornerRadius, Layout, RichText, ScrollArea, Stroke, Ui};

use crate::app::frame::EditorApp;
use crate::application::document::registry::DocumentContentProvider;
use crate::application::dto::BlockView;
use crate::application::markdown::queries::RenderPreviewQuery;
use crate::presentation::state::scroll_sync_state::{BlockRect, Side};
use crate::presentation::theme::builtin::Theme;

// 代码块文本 → 带语法色 `LayoutJob` 的纯函数留在 `code` 子模块；facade 内的
// 单测经 `super::*` 取用，故仅在测试构建里导入。
#[cfg(test)]
use code::highlighted_code_job;

/// 目录条目（`draw_toc` 用）：文档里一个标题块的位置与文本。
#[derive(Clone, Debug)]
pub(super) struct HeadingEntry {
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
    // 借用检查器不允许同时持有 `&app.preview.blocks` 与 `&mut app`。
    // `take` 是零拷贝的，渲染结束后原样放回。
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
    // 预览区需要**双向**滚动：纵向看长文，横向看超宽内容（不折行的代码块、
    // 比面板宽的图表）。`ScrollArea::both()` 的 `direction_enabled` 是
    // `[true, true]`，故两个方向都开滚动条。
    //
    // `auto_shrink` 必须**两向都关**：默认 `Vec2b::TRUE` 会把滚动区收缩到
    // 内容尺寸——横向收缩到内容宽度正是之前「预览区滚不动」缺陷的根因
    // （见上一段历史注释）。两向都关后滚动区占满面板宽高，正文在面板宽度内
    // 折行、超宽内容（长代码行 / 宽图表）则出现底部横向滚动条。
    let out = ScrollArea::both()
        .id_salt("preview-scroll")
        .auto_shrink([false, false])
        .vertical_scroll_offset(start_y)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                let mut rects: Vec<BlockRect> = Vec::with_capacity(blocks.len());
                // 相对原点：滚动内容的第一块顶边即 y = 0。
                // 用「第一块的实际顶边」而不是某个常量，是因为 `ScrollArea`
                // 内部的坐标系已经带上了偏移，绝对 y 每帧都不同。
                let mut origin: Option<f32> = None;
                for block in blocks.iter() {
                    // `scope_builder` 会返回本块占据的**实际矩形**（等价 `min_rect`），
                    // 比「画之前 / 画之后各读一次光标」稳——后者会被块内部
                    // 自己和 `item_spacing` 的交互骗到。
                    //
                    // # 为什么必须给每个块一个唯一 `id_salt`（否则标题被套红框）
                    //
                    // `ui.scope` 产出的子 `Ui`，其 `ui.id()` 恒为「父 ui 的 id + "child"」
                    // ——**同一父级下每个 scope 完全相同**（egui 的 `Ui::new_child`
                    // 把它作为 `stable_id`，真正唯一的只是内部 `unique_id`）。
                    // 而 `render_inline_text` 用固定 id `ui.id().with("rinline")`
                    // 注册可交互区；标题又是唯一带 `Sense::click()` 的内联文本
                    // （点击标题 → 编辑区跳到对应行）。于是文章里**第二个起**每个
                    // 标题都与前一个撞 id，egui 的 `Context::check_for_id_clash`
                    // 便给两端各套一个红框，并在旁边画出
                    // 「🔥 First/Second use of widget ID …」；鼠标移到那条红字上
                    // 还会弹出一大段解释 ID 冲突、建议改用 `ui.push_id` 的提示。
                    // `id_salt` 正是 `ui.push_id` 的等价物（也是 egui 提示里给的解法）：
                    // 传入 `block.id`（单次解析内唯一）后，各块子树 id 互不相同。
                    let res = ui.scope_builder(egui::UiBuilder::new().id_salt(block.id), |ui| {
                        draw_block(app, ui, &theme, block, &headings)
                    });
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
    // 预览区也跟随「自动换行」开关：开 → 正文在面板宽度内折行；关 → 不折行，
    // 长行靠新增的底部横向滚动条查看（与编辑区行为一致）。
    let wrap = app.editor.word_wrap;
    match block.kind.as_str() {
        "heading" => blocks::draw_heading(app, ui, theme, block),
        "paragraph" => blocks::draw_paragraph(ui, theme, block, wrap),
        "quote" => blocks::draw_quote(ui, theme, block, wrap),
        "code_block" => code::draw_code_block(app, ui, theme, block),
        "list" => list::draw_list(ui, theme, block, wrap),
        "task_list" => list::draw_task_list(app, ui, theme, block),
        "table" => {
            // 结构化缺失（理论上不该发生）时退回纯文本，总比空白好。
            match &block.table {
                Some(table) => table::draw_table(ui, theme, table),
                None => blocks::draw_paragraph(ui, theme, block, wrap),
            }
        }
        "image" => media::draw_image(app, ui, theme, block),
        "rule" => toc::draw_rule(ui, theme),
        "footnote" => toc::draw_footnote(ui, theme, block, wrap),
        "definition_list" => toc::draw_definition_list(ui, theme, block, wrap),
        "math" => embedded::draw_math(ui, theme, block),
        "diagram" => embedded::draw_diagram(ui, theme, block),
        "toc" => toc::draw_toc(app, ui, theme, block, headings),
        other => toc::draw_raw(ui, theme, block, other),
    }
}

/// 1px 实线描边（发丝线）。
///
/// 全文件统一走这里，避免每个 `Stroke::new(1.0, ...)` 触发 f32 字面量回退告警。
fn hairline(color: egui::Color32) -> Stroke {
    Stroke::new(1.0_f32, color)
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
