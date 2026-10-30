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
//! 图片纹理按 URL 缓存在 `image_cache` 子模块的线程局部表里——解码一张
//! 图可能几十毫秒，每帧重解会让滚动直接卡死。
//!
//! # 模块结构
//!
//! 按渲染区块拆成私有子模块（本 facade 只保留入口、块分发与全区块共享的
//! 发丝线；图片缓存已抽去 `image_cache`）：
//!
//! - `inline`：行内富文本（span 格式、`LayoutJob` 排版、分隔符规范化）；
//! - `code`：代码块与语法高亮；
//! - `list`：列表与任务列表（含回写偏移纯函数）；
//! - `table`：GFM 表格测量、列宽分配与网格绘制；
//! - `media`：图片块；
//! - `blocks`：标题 / 段落 / 引用；
//! - `toc`：分割线 / 目录 / 脚注 / 定义列表 / 原始块；
//! - `embedded`：数学公式与 Mermaid 图表（内置引擎 + 源码卡降级）；
//! - `image_cache`：图片纹理缓存（本地 / 远程 / data URI → 纹理，LRU 淘汰
//!   与远程异步加载）；
//!
//! 单测外置在 `tests.rs`（与 `app::frame` 同款先例）。

mod blocks;
mod code;
mod embedded;
mod embedded_cache;
mod image_cache;
mod inline;
mod list;
mod media;
mod table;
mod toc;

pub use image_cache::{ImageEntryState, image_cache_state, is_remote_url};
pub use inline::{footnote_ref_mark, strip_math_delimiters};
pub use list::task_marker_offset;
pub use table::table_column_distribution;

/// 仅供**跨模块**单测访问的私有项转出。
///
/// 存在的理由：`theme::mapper` 的样式表合并用例（`mapper/tests.rs`）
/// 需要拿到预览区行内的样式表，与编辑区侧的做**同表对照**——
/// 而 `inline` 是本模块的私有子模块，`pub(super)` 的 `span_format`
/// 够不着跨模块访问。
///
/// R-03 起 additionally 导出 `handle_span_click`：外链改走 opener 端口后，
/// 「点击 link 会调 open、且不安全的地址不调」这条用户可见行为要能在
/// 集成测试里断言（真起进程的旧实现做不到）。
///
/// # 为什么不能加 `#[cfg(test)]`
///
/// integration test（`tests/`）编译的是**另一个 crate**，`#[cfg(test)]`
/// 只在 lib 自身的测试目标里为真——加了就看不见这些符号。
/// 故它在所有构建里存在，用 `#[doc(hidden)]` 藏起来不进公开文档。
/// 名字以 `test_only` 自明用途：**不是**公开 API，别在生产代码里用。
#[doc(hidden)]
pub mod test_only {
    pub use super::inline::handle_span_click;
    pub use super::inline::span_format;
}

use egui::{Align, CornerRadius, Layout, RichText, ScrollArea, Stroke, Ui};

use crate::presentation::state::scroll_sync_state::{BlockRect, Side};
use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::PreviewViewAccess;
use rustmark_core::application::dto::BlockView;
use rustmark_core::application::markdown::queries::RenderPreviewQuery;

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
// 入口
// ============================================================================

/// 绘制预览区。
pub fn show(vctx: &mut dyn PreviewViewAccess, ui: &mut Ui) {
    let theme = vctx.theme_state().clone();
    ui.painter()
        .rect_filled(ui.max_rect(), CornerRadius::ZERO, theme.bg_primary);

    // 预览区顶部标题栏：与编辑区工具栏**等高**（`editor_toolbar_height`）。
    //
    // 编辑区在问题 3 里把工具栏移到了编辑区顶部，于是它的滚动内容区比预览区
    // 少了一条工具栏的高度——两栏不再对齐。预览区补上等高的标题栏后，
    // 两栏的内容区从同一条水平线起、高度一致（问题 2）。
    pane_header(ui, &theme);

    let Some(doc_id) = vctx.active_doc() else {
        ui.weak("没有打开的文档");
        return;
    };

    // 0) 换文档即重置：清掉上一份文档遗留的块序列与滚动偏移。
    //    这是预览 ↔ 激活 tab 联动的**唯一入口**（见 `PreviewUiState::sync_document`）：
    //    重置会把 `last_key` 清成 `None`，于是下一句的 `first` 为真，本帧必定重解析，
    //    绝不会出现「上一份文档的块多显示一帧」。
    vctx.preview_state().sync_document(doc_id.as_u64());

    // 1) 取文档版本号 → 由 UI State 判定是否该重解析（换文档与外观变化立即生效，
    //    同一份文档的内容变化需等 150ms 防抖窗口）。
    //    只取版本号而不做全文快照：`content_and_revision` 会把整篇文本从
    //    rope 快照出来，每帧一次只为拿一个 u64（问题 043）。
    let version = vctx.document_revision(doc_id).unwrap_or(0);

    // 缓存键必须带上**文档标识**：`version` 是每份文档各自的计数器，
    // 两份都还没编辑过的文档版本号相同，只比对版本号会让切 tab 后的预览
    // 一直停留在第一份文档上（见 `PreviewCacheKey::doc_id`）。
    let key = crate::presentation::state::preview_ui_state::PreviewCacheKey::new(
        doc_id.as_u64(),
        version,
        &theme,
    );
    let now_ms = vctx.now_ms();
    // 两条判据都只要一个 `bool`，走只读出口——别为它拿 `&mut`，
    // 那会把后面同帧的 `document_revision` / `editor_state_read` 一并堵死。
    let first = vctx.preview_state_read().last_key.is_none();
    let want = vctx.preview_state_read().should_render(key, now_ms, false);

    if first || want {
        // **形态 B**：`render_preview()` 是 `&self` 借用，若把它写进 `match`
        // 判据，借用会覆盖整个 match 块，块内 `set_preview_blocks` 的可变借用
        // 直接撞车。故先落到一个 owned 结果上，借用随语句结束。
        let rendered = vctx.render_preview(RenderPreviewQuery { doc_id, version });
        match rendered {
            Ok(view) => {
                vctx.set_preview_blocks(view.blocks);
                vctx.preview_state().set_degraded(view.truncated);
            }
            Err(e) => {
                // 渲染失败不终止：预览区退化成一句话，编辑区照常可用。
                ui.weak(format!("预览渲染失败：{e}"));
                return;
            }
        }
        vctx.preview_state().record_render(key, now_ms);
    }

    // 降级提示（FR-06）：源过大时不解析，明确告知而非静默空白。
    // 先落到 `bool` 再进 `if`：判据里的 `&self` 借用会覆盖整块，块内若再调
    // `vctx` 的其它方法就是第二个借用。
    let degraded = vctx.preview_state_read().degraded;
    if degraded {
        ui.weak("文档过大，预览已降级");
    }

    // 2) 把块序列取出再还回：渲染期间 `app` 需要被可变更改（任务项回写），
    // 借用检查器不允许同时持有 `&app.preview.blocks` 与 `&mut app`。
    // `take` 是零拷贝的，渲染结束后原样放回。
    //
    // 内部跳转目标（标题 slug / 脚注标号 → 块 id）必须在 `mem::take` 之前收集：
    // `take` 会把块序列借走，之后无法再读 `app.preview.blocks`。
    // 预览里点击 `#锚点` 或脚注引用 `[^n]` 时据此跳到对应块。
    let internal_targets: Vec<(String, u64)> = vctx
        .preview_state_read()
        .blocks
        .iter()
        .filter_map(|b| {
            if b.kind == "footnote" {
                b.label.clone().map(|l| (l, b.id))
            } else if b.kind == "heading" {
                Some((heading_slug(&b.content), b.id))
            } else {
                None
            }
        })
        .collect();
    // D-3 阶段 3：取走/放回经 `take_blocks` / `store_preview_render` 对称走
    // state 方法，字段的可变引用不出 state（语义与原先的 `mem::take` 相同）。
    let blocks = vctx.preview_state().take_blocks();

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

    // 脚注定义：同样现场收集，但**不按源码位置渲染**——统一放到预览末尾的
    // 「脚注」区（V11-18，见 `toc::draw_footnote_section` 的长注释）。
    // 主循环里相应跳过，正文里只留上标编号。
    let footnotes: Vec<&BlockView> = blocks.iter().filter(|b| b.kind == "footnote").collect();

    let start_y = {
        let scroll_y = vctx.preview_state_read().scroll_y;
        if scroll_y.is_finite() { scroll_y } else { 0.0 }
    };
    // 脚注区与正文一致地跟随「自动换行」开关（在闭包外取，避免与闭包内的
    // `vctx` 重 Borrow 冲突）。
    let wrap_for_footnotes = vctx.editor_state_read().word_wrap;
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
                // 视口宽度基准（问题 068）：必须在**任何内容绘制之前**捕获。
                //
                // `ScrollArea::both()` 的内容 `Ui` 在某个子块超出视口宽时会把
                // 自己的 `max_rect` 向右撑大（横向滚动条的实现机制）——此后
                // 所有块的 `ui.available_width()` 都返回**膨胀后**的宽度。
                // 典型传播链：第一张超宽表格 → 后续表格跟着按膨胀宽分配列宽
                // → 一起溢出屏幕。在第一块之前捕获的宽度就是干净的视口宽，
                // 全部块共享同一个基准，互不污染。
                let viewport_w = ui.available_width();
                let mut rects: Vec<BlockRect> = Vec::with_capacity(blocks.len());
                // 相对原点：滚动内容的第一块顶边即 y = 0。
                // 用「第一块的实际顶边」而不是某个常量，是因为 `ScrollArea`
                // 内部的坐标系已经带上了偏移，绝对 y 每帧都不同。
                let mut origin: Option<f32> = None;
                for block in blocks.iter() {
                    // 脚注定义不在正文位置渲染：已收集到末尾的「脚注」区
                    // （V11-18）。跳过时也不占位、不记矩形——定义的整体
                    // 位置就在文档末尾，滚动同步按最近的正文块映射即可。
                    if block.kind == "footnote" {
                        continue;
                    }
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
                        // 开着自动换行时把本块钳到视口宽（问题 068）：块内所有
                        // `available_width()` 调用——包括表格 Frame 内、代码块、
                        // 列表——都读到钳制后的稳定值；不可拆分的长词由 egui
                        // 在钳制宽处硬折行。块内任何内容因此都超不出视口，
                        // 也就不会膨胀外层 `max_rect` 去污染后续块。
                        // 关闭换行时不钳：内容按自然宽排开，靠横向滚动条查看
                        // （这正是关闭换行的设计行为，膨胀即滚动条的依据）。
                        if wrap_for_footnotes {
                            ui.set_max_width(viewport_w);
                        }
                        draw_block(vctx, ui, &theme, block, &headings, &internal_targets)
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
                // 脚注区：紧跟最后一个正文块之后（V11-18）。与正文块同一条
                // 视口宽钳制口径——否则它读到的 `available_width()` 会被
                // 前序块（若有）的膨胀污染（问题 068）。
                if !footnotes.is_empty() {
                    if wrap_for_footnotes {
                        ui.set_max_width(viewport_w);
                    }
                    toc::draw_footnote_section(
                        vctx,
                        ui,
                        &theme,
                        &footnotes,
                        wrap_for_footnotes,
                        &internal_targets,
                    );
                }
                rects
            })
            .inner
        });

    vctx.store_preview_render(blocks, out.inner, out.state.clamped_offset().y);
    vctx.ui_state()
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
    vctx: &mut dyn PreviewViewAccess,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    headings: &[HeadingEntry],
    internal_targets: &[(String, u64)],
) {
    // 预览区也跟随「自动换行」开关：开 → 正文、列表、代码块、**表格**、
    // 降级源码卡都在面板宽度内折行，不出现横向滚动条；关 → 不折行，
    // 长行靠预览区底部横向滚动条查看（与编辑区行为一致）。
    // 表格在关闭时按自然宽排开（问题 068），而不是被压进面板宽度。
    let wrap = vctx.editor_state_read().word_wrap;
    match block.kind.as_str() {
        "heading" => blocks::draw_heading(vctx, ui, theme, block, internal_targets),
        "paragraph" => blocks::draw_paragraph(vctx, ui, theme, block, wrap, internal_targets),
        "quote" => blocks::draw_quote(vctx, ui, theme, block, wrap, internal_targets),
        "code_block" => code::draw_code_block(vctx, ui, theme, block, wrap),
        "list" => list::draw_list(vctx, ui, theme, block, wrap, internal_targets),
        "task_list" => list::draw_task_list(vctx, ui, theme, block, internal_targets),
        "table" => {
            // 结构化缺失（理论上不该发生）时退回纯文本，总比空白好。
            match &block.table {
                Some(table) => table::draw_table(vctx, ui, theme, table, wrap, internal_targets),
                None => blocks::draw_paragraph(vctx, ui, theme, block, wrap, internal_targets),
            }
        }
        "image" => media::draw_image(vctx, ui, theme, block),
        "rule" => toc::draw_rule(ui, theme),
        // 脚注定义不在正文位置渲染：统一收集到预览末尾的「脚注」区
        // （见 `show` 里的收集与 `toc::draw_footnote_section`）。
        // 主循环已跳过脚注块，这个分支是**防御性**的——万一收集逻辑被改坏，
        // 脚注也只会安静不画，而不会把 `[^1]: 原文` 当 raw 文本泼在正文里。
        "footnote" => {}
        "definition_list" => {
            toc::draw_definition_list(vctx, ui, theme, block, wrap, internal_targets)
        }
        "math" => embedded::draw_math(ui, theme, block, wrap),
        "diagram" => embedded::draw_diagram(ui, theme, block, wrap),
        "toc" => toc::draw_toc(vctx, ui, theme, block, headings, internal_targets),
        other => toc::draw_raw(vctx, ui, theme, block, other, wrap, internal_targets),
    }
}

/// 标题锚点 slug：与 CommonMark / GitHub 的锚点生成对齐（小写、空格转连字符、
/// 去掉标点）。点击预览里的 `#锚点` 链接时据此匹配标题块。
pub(super) fn heading_slug(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    for ch in text.trim().chars() {
        if ch.is_ascii_whitespace() {
            slug.push('-');
        } else if ch.is_alphanumeric() || ch == '-' {
            slug.push(ch.to_ascii_lowercase());
        }
        // 标点直接丢弃。CJK 字符**保留**：`is_alphanumeric()` 对汉字为真，
        // 于是中文标题的锚点就是它本身（`## 第一章` → `#第一章`），
        // 既可被直接链接，也不会因「全部丢弃」而撞上空 slug。
    }
    slug
}

/// 1px 实线描边（发丝线）。
///
/// 全文件统一走这里，避免每个 `Stroke::new(1.0, ...)` 触发 f32 字面量回退告警。
fn hairline(color: egui::Color32) -> Stroke {
    Stroke::new(1.0_f32, color)
}

#[cfg(test)]
mod tests;
