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
mod embedded_cache;
mod inline;
mod list;
mod media;
mod table;
mod toc;

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

use std::cell::RefCell;
use std::collections::HashSet;

use egui::{Align, CornerRadius, Layout, RichText, ScrollArea, Stroke, Ui};

use crate::app::frame::EditorApp;
use crate::application::dto::BlockView;
use crate::application::markdown::queries::RenderPreviewQuery;
use crate::presentation::state::scroll_sync_state::{BlockRect, Side};
use crate::presentation::theme::builtin::Theme;
use crate::shared::lru::LruMap;

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
    /// 远程图片的后台加载进行中（占位文案用「加载中」而非「无法加载」）。
    loading: bool,
}

impl ImageEntry {
    /// 远程加载进行中的占位条目（F-3）。
    fn loading() -> Self {
        Self {
            texture: None,
            size: [0.0, 0.0],
            error: None,
            loading: true,
        }
    }
}

/// 图片纹理缓存的 LRU 容量上限（张）。
///
/// 旧实现「不设淘汰上限」，理由是「有界于用户内容」；但 `ImageEntry.texture`
/// 是 egui GPU 纹理，多图 / 大图场景会持续占显存（缺陷 G3）。正常文档引用的
/// 独立图片远少于此，仅当引用图极多时才触发淘汰，把显存 / 内存占用收束到可预测的量级。
const IMAGE_CACHE_CAPACITY: usize = 64;

/// 图片纹理缓存：引用字符串 → 已解码纹理 / 失败原因，带 LRU 淘汰。
///
/// 淘汰策略本体在 [`LruMap`]（D-4 收敛：高亮 / 拼写 / 渲染 / 图片四处共用）。
struct ImageCache {
    entries: LruMap<String, ImageEntry>,
}

impl ImageCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: LruMap::new(capacity),
        }
    }

    /// 取缓存命中；命中把键挪到访问序尾部（正在渲染的图永不被误踢）。
    fn get(&mut self, url: &str) -> Option<ImageEntry> {
        self.entries.get(url).cloned()
    }

    /// 写入；达到容量且为新键时，驱逐访问序最旧者（其 `TextureHandle` 随
    /// `ImageEntry` 析构自动释放 GPU 资源：egui 在最后一个 handle 落下时
    /// `free_texture`）。
    fn insert(&mut self, url: String, entry: ImageEntry) {
        self.entries.insert(url, entry);
    }
}

thread_local! {
    /// 图片引用字符串 → 已解码纹理 / 失败原因，带 LRU 淘汰（缺陷 G3）。
    ///
    /// 按键是**引用字符串**而不是文档路径：同一张图在多个文档里引用时只解码一次。
    /// 超出 [`IMAGE_CACHE_CAPACITY`] 时淘汰访问序最旧的图片；当前帧正在渲染的图
    /// 每帧都会被 `get` / `insert` 触碰，因此不会被误踢。
    ///
    /// 远程图片的「加载中」占位条目同样进这张表（F-3）：它既是渲染状态，
    /// 也是「已有线程在下载」的标记（配合 [`REMOTE_INFLIGHT`] 防重复 spawn）。
    static IMAGE_CACHE: RefCell<ImageCache> = RefCell::new(ImageCache::new(IMAGE_CACHE_CAPACITY));
    /// 正在后台下载的远程 URL（防重复 spawn 的守卫）。
    ///
    /// 正常情况下「加载中」条目在缓存里就能挡住重复 spawn；本集合兜住
    /// 「加载中被 LRU 淘汰、线程还没回来」这一窗口，避免同一 URL 起两条线程。
    static REMOTE_INFLIGHT: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// 远程图片的后台加载结果回执（后台线程 → UI 线程，F-3）。
struct RemoteLoadDone {
    /// 引用字符串（与缓存同键）。
    url: String,
    /// 解码后的颜色图 / 失败原因。解码放在后台线程：512KB 上限内也要几十毫秒。
    result: Result<egui::ColorImage, String>,
}

/// 后台回执通道：`Sender` 可克隆进线程，`Receiver` 只在 UI 线程 drain。
struct RemoteChannel {
    tx: std::sync::mpsc::Sender<RemoteLoadDone>,
    rx: std::sync::Mutex<std::sync::mpsc::Receiver<RemoteLoadDone>>,
}

static REMOTE_CHANNEL: std::sync::OnceLock<RemoteChannel> = std::sync::OnceLock::new();

fn remote_channel() -> &'static RemoteChannel {
    REMOTE_CHANNEL.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        RemoteChannel {
            tx,
            rx: std::sync::Mutex::new(rx),
        }
    })
}

/// 是否远程地址（http / https）：只有它们走后台线程（F-3）。
///
/// 本地路径与 data URI 保持同步路径：读盘 + 解码在几十毫秒量级，
/// 且失败要立即出占位（同步路径的失败回执也是当帧的）。
pub fn is_remote_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

/// 起一个后台线程下载 + 解码远程图片；完成后经通道回执并唤醒 UI。
///
/// `loader` 由调用方从 `Services::assets` **拷贝**传入（不于此点名具体类型：
/// 表现层不得依赖 infrastructure，与既有 `assets.load(...)` 调用同一立场）。
///
/// **为什么必须 `request_repaint`**：加载中的占位符不会自行重绘，
/// 后台线程完成时若只是往通道里塞消息，UI 可能隔很久才跑下一帧——
/// 用户看着「加载中」干等。`egui::Context` 是 `Send + Sync`，
/// 专为这种跨线程唤醒设计。
fn spawn_remote_load<L>(url: &str, loader: L, ctx: egui::Context)
where
    L: FnOnce(&str) -> Result<LoadedBytes, String> + Send + 'static,
{
    let url = url.to_string();
    let tx = remote_channel().tx.clone();
    std::thread::spawn(move || {
        // 下载与解码都在后台：`AssetLoader::load` 自带超时 / 体积 / MIME 边界，
        // 远程 URL 不需要 base_dir。解码失败与下载失败对预览是同一种降级。
        let result = loader(&url).and_then(|bytes| decode(&bytes.0));
        let _ = tx.send(RemoteLoadDone { url, result });
        ctx.request_repaint();
    });
}

/// 后台线程用的「下载结果」：只拿字节（`LoadedAsset` 活在 infrastructure 层，
/// 表现层不引进其类型；字段访问同样不需要）。
struct LoadedBytes(Vec<u8>);

/// 收干后台回执：解码完成的图在此**建纹理**（`load_texture` 只能在 UI 线程调）
/// 并入缓存。返回是否有回执到达（有则本帧已request_repaint，无需额外动作）。
fn drain_remote_results(ctx: &egui::Context) -> bool {
    // 锁只在这几十行里持有，且内部不会 panic；中毒（持锁线程 panic）时
    // 按 recovering guard 处理，与全仓 `lock_or_recover` 同一立场。
    let rx = remote_channel()
        .rx
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut any = false;
    while let Ok(msg) = rx.try_recv() {
        any = true;
        REMOTE_INFLIGHT.with(|f| f.borrow_mut().remove(&msg.url));
        let entry = match msg.result {
            Ok(img) => {
                let size = [img.size[0] as f32, img.size[1] as f32];
                let texture = ctx.load_texture(
                    format!("preview-image:{}", msg.url),
                    img,
                    egui::TextureOptions::LINEAR,
                );
                ImageEntry {
                    texture: Some(texture),
                    size,
                    error: None,
                    loading: false,
                }
            }
            Err(reason) => ImageEntry {
                texture: None,
                size: [0.0, 0.0],
                error: Some(reason),
                loading: false,
            },
        };
        IMAGE_CACHE.with(|c| c.borrow_mut().insert(msg.url, entry));
    }
    any
}

/// 取（或首次加载）一张图片。
///
/// 三种来源（本地路径 / 远程 URL / data URI）由 `AssetLoader` 统一处理，
/// 本函数只负责「解码 → 纹理」与失败降级。
///
/// # 远程图片为什么异步（F-3）
///
/// 远程地址若在绘制路径同步下载，UI 线程会阻塞到响应或超时（默认 5s）——
/// 含远程图的文档首绘预览时整个界面卡住。故远程走「后台线程下载 + 解码 →
/// 通道回执 → UI 线程建纹理」：绘制路径只发一个「加载中」占位，
/// 完成后由后台线程 `request_repaint` 唤醒下一帧换上真图。
fn image_entry(ui: &Ui, app: &EditorApp, url: &str) -> ImageEntry {
    // 回执先入账：后台线程在本帧之前完成的加载，此刻就能当缓存命中返回。
    drain_remote_results(ui.ctx());
    if let Some(cached) = IMAGE_CACHE.with(|c| c.borrow_mut().get(url)) {
        return cached;
    }

    if is_remote_url(url) {
        let spawn = REMOTE_INFLIGHT.with(|f| !f.borrow().contains(url.trim()));
        if spawn {
            REMOTE_INFLIGHT.with(|f| f.borrow_mut().insert(url.trim().to_string()));
            // 从 `Services` 拷贝一份加载器进线程（Copy，不借用 app）。
            let loader = app.services.assets;
            spawn_remote_load(
                url,
                move |u| {
                    loader
                        .load(u, None)
                        .map(|asset| LoadedBytes(asset.bytes))
                        .map_err(|e| e.to_string())
                },
                ui.ctx().clone(),
            );
        }
        let entry = ImageEntry::loading();
        IMAGE_CACHE.with(|c| c.borrow_mut().insert(url.to_string(), entry.clone()));
        return entry;
    }

    let base_dir = app.document_dir();
    // 「加载失败」与「解码失败」是两类不同的错误（前者 AssetError、后者 String），
    // 但对预览的意义相同——都降级成占位。故在这里统一成一条消息。
    //
    // 尺寸参数**不进缓存**：同一 URL 可被 `=30x30` 与 `=600x200` 两个块
    // 引用（问题 3），按块计算才能区分；URL 查询参数同样是纯函数，
    // 每帧重算的成本可忽略。
    let failed = |msg: String| ImageEntry {
        texture: None,
        size: [0.0, 0.0],
        error: Some(msg),
        loading: false,
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
                    loading: false,
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

/// 图片缓存状态快照（测试与调试用）：`None` 表示该 URL 未进过缓存。
pub fn image_cache_state(url: &str) -> Option<ImageEntryState> {
    IMAGE_CACHE.with(|c| {
        c.borrow().entries.peek(url).map(|e| ImageEntryState {
            loading: e.loading,
            has_texture: e.texture.is_some(),
            error: e.error.clone(),
        })
    })
}

/// 图片缓存状态（[`image_cache_state`] 的返回）。
pub struct ImageEntryState {
    /// 远程加载进行中（占位显示「加载中」）。
    pub loading: bool,
    /// 是否已拿到纹理。
    pub has_texture: bool,
    /// 失败原因（加载中与成功均为 `None`）。
    pub error: Option<String>,
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
    //    只取版本号而不做全文快照：`content_and_revision` 会把整篇文本从
    //    rope 快照出来，每帧一次只为拿一个 u64（问题 043）。
    let version = app
        .services
        .registry
        .read()
        .ok()
        .and_then(|reg| reg.revision_of(doc_id))
        .unwrap_or(0);

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
    let want = app.preview.should_render(key, now_ms, false);

    if first || want {
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
    //
    // 内部跳转目标（标题 slug / 脚注标号 → 块 id）必须在 `mem::take` 之前收集：
    // `take` 会把块序列借走，之后无法再读 `app.preview.blocks`。
    // 预览里点击 `#锚点` 或脚注引用 `[^n]` 时据此跳到对应块。
    let internal_targets: Vec<(String, u64)> = app
        .preview
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

    // 脚注定义：同样现场收集，但**不按源码位置渲染**——统一放到预览末尾的
    // 「脚注」区（V11-18，见 `toc::draw_footnote_section` 的长注释）。
    // 主循环里相应跳过，正文里只留上标编号。
    let footnotes: Vec<&BlockView> = blocks.iter().filter(|b| b.kind == "footnote").collect();

    let start_y = if app.preview.scroll_y.is_finite() {
        app.preview.scroll_y
    } else {
        0.0
    };
    // 脚注区与正文一致地跟随「自动换行」开关（在闭包外取，避免与闭包内的
    // `&mut app` 重 Borrow 冲突）。
    let wrap_for_footnotes = app.editor.word_wrap;
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
                        draw_block(app, ui, &theme, block, &headings, &internal_targets)
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
                        app,
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

    app.preview.blocks = blocks;
    app.preview.rects = out.inner;
    app.preview.scroll_y = out.state.clamped_offset().y;
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
    internal_targets: &[(String, u64)],
) {
    // 预览区也跟随「自动换行」开关：开 → 正文、列表、代码块、**表格**、
    // 降级源码卡都在面板宽度内折行，不出现横向滚动条；关 → 不折行，
    // 长行靠预览区底部横向滚动条查看（与编辑区行为一致）。
    // 表格在关闭时按自然宽排开（问题 068），而不是被压进面板宽度。
    let wrap = app.editor.word_wrap;
    match block.kind.as_str() {
        "heading" => blocks::draw_heading(app, ui, theme, block, internal_targets),
        "paragraph" => blocks::draw_paragraph(app, ui, theme, block, wrap, internal_targets),
        "quote" => blocks::draw_quote(app, ui, theme, block, wrap, internal_targets),
        "code_block" => code::draw_code_block(app, ui, theme, block, wrap),
        "list" => list::draw_list(app, ui, theme, block, wrap, internal_targets),
        "task_list" => list::draw_task_list(app, ui, theme, block, internal_targets),
        "table" => {
            // 结构化缺失（理论上不该发生）时退回纯文本，总比空白好。
            match &block.table {
                Some(table) => table::draw_table(app, ui, theme, table, wrap, internal_targets),
                None => blocks::draw_paragraph(app, ui, theme, block, wrap, internal_targets),
            }
        }
        "image" => media::draw_image(app, ui, theme, block),
        "rule" => toc::draw_rule(ui, theme),
        // 脚注定义不在正文位置渲染：统一收集到预览末尾的「脚注」区
        // （见 `show` 里的收集与 `toc::draw_footnote_section`）。
        // 主循环已跳过脚注块，这个分支是**防御性**的——万一收集逻辑被改坏，
        // 脚注也只会安静不画，而不会把 `[^1]: 原文` 当 raw 文本泼在正文里。
        "footnote" => {}
        "definition_list" => {
            toc::draw_definition_list(app, ui, theme, block, wrap, internal_targets)
        }
        "math" => embedded::draw_math(ui, theme, block, wrap),
        "diagram" => embedded::draw_diagram(ui, theme, block, wrap),
        "toc" => toc::draw_toc(app, ui, theme, block, headings, internal_targets),
        other => toc::draw_raw(app, ui, theme, block, other, wrap, internal_targets),
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
mod tests {
    use super::*;
    use crate::application::dto::{RangeView, SpanView};
    // R-10 对拍用例要访问行内路径的私有项（样式表 / 构造器 / 命中区间）。
    use super::inline::{Typography, build_text_job, done_overlay, span_format};
    use crate::presentation::theme::mapper::SpanHit;
    use crate::shared::geometry::clamp_to_char_boundaries;

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
            url: None,
        }];
        let job = highlighted_code_job(&theme, text, Some(&spans), f32::INFINITY);
        // 不 panic 即通过；再断言文本被完整覆盖（少上色可以，丢字符不行）。
        let covered: String = job
            .sections
            .iter()
            .map(|s| {
                job.text
                    .get(s.byte_range.start.0..s.byte_range.end.0)
                    .unwrap_or("")
            })
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
            .map(|s| {
                job.text
                    .get(s.byte_range.start.0..s.byte_range.end.0)
                    .unwrap_or("")
            })
            .collect();
        assert_eq!(covered, text);
    }

    /// 问题 2 回归：开启「自动换行」时，代码块必须按给定宽度折行。
    ///
    /// 修复前 `draw_highlighted_code` 把 `wrap_width` 写死 `INFINITY`，
    /// 长代码行永远撑出预览区横向滚动条——「自动换行」开关对代码块
    /// 完全不生效。现在宽度由调用方（`draw_code_block` 的 `wrap` 分支）
    /// 透传，本测试钉住这条通路。
    #[test]
    fn highlighted_code_job_wraps_at_requested_width() {
        let theme = Theme::dark();
        let text = "let x = 1;";
        let wrapped = highlighted_code_job(&theme, text, None, 120.0);
        assert_eq!(
            wrapped.wrap.max_width, 120.0,
            "开启自动换行时代码应按面板宽度折行"
        );
        // 关闭时（INFINITY）保持不折行：由预览区横向滚动条查看，
        // 与编辑区行为一致（既有设计不变）。
        let nowrap = highlighted_code_job(&theme, text, None, f32::INFINITY);
        assert!(
            nowrap.wrap.max_width.is_infinite(),
            "关闭自动换行时代码不应折行"
        );
    }

    /// 注脚跳转（S6-05 #10）：正文 `[^1]` 的显示标号必须与 `internal_targets`
    /// 里脚注定义的 key 一致，点击才能命中对侧块。
    #[test]
    fn footnote_ref_mark_strips_brackets() {
        assert_eq!(footnote_ref_mark("[^1]"), "1");
        assert_eq!(footnote_ref_mark("[^note]"), "note");
        // 不完整片段（解析 / 渲染边界不一致时）原样返回，绝不剥错。
        assert_eq!(footnote_ref_mark("^1]"), "^1]");
        assert_eq!(footnote_ref_mark("[]"), "[]");
    }

    /// 标题锚点：`internal_targets` 的 key 由 `heading_slug` 生成，预览里
    /// 点 `#锚点` 链接即据此匹配标题块。
    #[test]
    fn heading_slug_is_deterministic_and_ascii_safe() {
        assert_eq!(heading_slug("Hello World"), "hello-world");
        // 与 GitHub 锚点生成对齐：连续空白各转一个连字符，不合并
        // （GitHub 上 `## Trim  Me` 的锚点就是 `#trim--me`）。
        assert_eq!(heading_slug("  Trim  Me  "), "trim--me");
        // 标点直接丢弃；CJK 字符**保留**（`is_alphanumeric()` 对汉字为真），
        // 于是 `## 第一章` 的锚点就是 `#第一章`——中文标题得以被直接链接，
        // 且不同中文标题不会因「全部丢弃」而撞同一个空 slug。
        assert_eq!(heading_slug("第一章"), "第一章");
        assert_eq!(heading_slug("Chapter 1: 引言"), "chapter-1-引言");
    }

    // ========================================================================
    // R-10 对拍：行内路径（顺序游标 + 文本改写 + 命中区间）
    //
    // 行内的 `span_format` / `done_overlay` / `build_text_job` 都是私有项，
    // 集成测试（`tests/`）拿不到 → 对拍用例只能放在 crate 内部。
    // ========================================================================

    /// 旧 `build_text_job`（顺序游标语义）——R-10 收敛前的实现，原文抄录。
    ///
    /// 抄自 `67210f2^:src/presentation/views/preview_view/inline.rs`。
    fn legacy_build_text_job(
        theme: &Theme,
        content: &str,
        spans: &[SpanView],
        typo: Typography<'_>,
        done: bool,
        wrap_width: f32,
    ) -> (egui::text::LayoutJob, Vec<SpanHit>) {
        let Typography {
            size,
            line_height,
            base_style,
        } = typo;
        let base = span_format(theme, base_style, size, line_height);
        let mut job = egui::text::LayoutJob {
            wrap: egui::text::TextWrapping {
                max_width: wrap_width.max(0.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut hits: Vec<SpanHit> = Vec::new();
        let len = content.len();
        let mut cursor = 0usize;
        let mut char_cursor = 0usize;
        if spans.is_empty() {
            let mut fmt = base.clone();
            if done {
                fmt = done_overlay(theme, fmt);
            }
            job.append(content, 0.0, fmt);
        } else {
            for (idx, span) in spans.iter().enumerate() {
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
                    let base_slice = &content[cursor..start];
                    job.append(base_slice, 0.0, base.clone());
                    char_cursor += base_slice.chars().count();
                }
                if end > start {
                    let mut fmt = span_format(theme, &span.style, size, line_height);
                    if done {
                        fmt = done_overlay(theme, fmt);
                    }
                    let slice = match span.style.as_str() {
                        "math" => strip_math_delimiters(&content[start..end]),
                        "footnote_ref" => footnote_ref_mark(&content[start..end]),
                        _ => &content[start..end],
                    };
                    let slice_chars = slice.chars().count();
                    job.append(slice, 0.0, fmt);
                    if span.style == "link" || span.style == "footnote_ref" {
                        hits.push(SpanHit {
                            idx,
                            c0: char_cursor,
                            c1: char_cursor + slice_chars,
                        });
                    }
                    char_cursor += slice_chars;
                }
                cursor = end.max(cursor);
            }
            if cursor < len {
                let tail = &content[cursor..];
                job.append(tail, 0.0, base.clone());
            }
        }
        (job, hits)
    }

    /// `LayoutJob` 的可比指纹（每段文本 + 全部可见样式）。
    fn job_fingerprint(job: &egui::text::LayoutJob) -> Vec<String> {
        job.sections
            .iter()
            .map(|s| {
                let t = job
                    .text
                    .get(s.byte_range.start.0..s.byte_range.end.0)
                    .unwrap_or("<越界>");
                format!(
                    "text={t:?} color={:?} bg={:?} size={} ital={} strike={} ul={}",
                    s.format.color,
                    s.format.background,
                    s.format.font_id.size,
                    s.format.italics,
                    s.format.strikethrough.width > 0.0,
                    s.format.underline.width > 0.0,
                )
            })
            .collect()
    }

    /// 行内路径：新实现必须与旧实现**逐段等价**。
    ///
    /// 覆盖：无 span / 单 span / 多 span / 有间隙 / 多字节 / 非字符边界 /
    /// `math` 改写 / `footnote_ref` 改写 / 已勾选任务项。
    #[test]
    fn inline_cursor_matches_legacy_implementation() {
        let theme = Theme::dark();
        let size = theme.metrics.preview_font_size;
        let line_height = size * theme.metrics.preview_line_height;
        let mk = |start: usize, end: usize, style: &str| SpanView {
            range: RangeView::new(start, end),
            style: style.to_string(),
            url: None,
        };

        let cases: Vec<(&str, &str, Vec<SpanView>, &str, bool)> = vec![
            ("无 span", "普通文本", vec![], "", false),
            // **字节区间**（不是字符数！）：中文字符占 3 字节。
            // 每条都带「前提」断言（见下）确保区间没写错。
            ("单 span", "a**粗**b", vec![mk(3, 6, "strong")], "", false),
            (
                "多 span",
                "a**粗**_斜_",
                vec![mk(3, 6, "strong"), mk(9, 12, "emphasis")],
                "",
                false,
            ),
            (
                "有间隙",
                "ab **粗** cd",
                vec![mk(5, 8, "strong")],
                "",
                false,
            ),
            ("多字节", "中**文**", vec![mk(5, 8, "strong")], "", false),
            ("非字符边界", "中文abc", vec![mk(1, 5, "strong")], "", false),
            (
                "math 改写",
                "值 $E=mc^2$ 结束",
                vec![mk(4, 12, "math")],
                "",
                false,
            ),
            (
                "footnote 改写",
                "见[^1]处",
                vec![mk(3, 7, "footnote_ref")],
                "",
                false,
            ),
            (
                // **两个命中 span**：命中区间的 `c0` / `c1` 是**字符**下标，
                // 必须随前文长度**累计**。
                //
                // 为什么单命中那条测不出来：只有一个命中 span 时，
                // `char_cursor` 无论怎么算都从 0 起（`c0=0`），
                // 而 `c1 = c0 + slice_chars` 也只依赖本段自身长度——
                // 把 `char_cursor += slice_chars` 改成 `+= 0` **看不出差别**。
                // 实测（变异验证）：本条是唯一能让该变异变红的用例。
                //
                // 字节区间："ab [x] cd [y] ef"
                //   a=0 b=1 ' '=2 [=3 x=4 ]=5 ' '=6 c=7 d=8 ' '=9
                //   [=10 y=11 ]=12 ' '=13 e=14 f=15
                // 第一个 link 占 3..6，第二个占 10..13。
                // 第二个命中的 `c0` 应为 10（字符下标），
                // 而不是「第一个 link 结束处」或别的错值。
                "两个命中 span",
                "ab [x] cd [y] ef",
                vec![mk(3, 6, "link"), mk(10, 13, "link")],
                "",
                false,
            ),
            ("已勾选", "已完成", vec![], "task_done", true),
            (
                "已勾选 + span",
                "已完成**重要**",
                vec![mk(11, 17, "strong")],
                "task_done",
                true,
            ),
        ];

        for (name, content, spans, base_style, done) in cases {
            // 前提：span 区间必须**非空**。
            //
            // 不加这条断言，字节数写错（如把「粗」的 3..6 写成 3..5）会让区间
            // 被吸附成空 → span 段根本不生成 → 用例「通过」但没测到东西。
            //
            // 注意**不能**断言「必须是字符边界」：本表里「非字符边界」那条
            // **故意**用非边界区间（测 DEF-04 的防护）。故只查非空，
            // 字符边界由下面「非字符边界」那条用例自己的断言负责。
            for s in &spans {
                assert!(
                    s.range.end > s.range.start,
                    "用例前提错误 [{name}]：span {:?} 为空区间",
                    s.range
                );
            }
            let typo = Typography {
                size,
                line_height,
                base_style,
            };
            let (old, old_hits) = legacy_build_text_job(&theme, content, &spans, typo, done, 300.0);
            let (new, new_hits) = build_text_job(&theme, content, &spans, typo, done, 300.0);

            assert_eq!(
                job_fingerprint(&old),
                job_fingerprint(&new),
                "行内路径 [{name}] 与旧实现不等价"
            );
            assert_eq!(
                old_hits, new_hits,
                "行内路径 [{name}] 的命中区间与旧实现不等价"
            );
        }
    }

    /// 行内路径：base 段（span 未覆盖的文本）必须用 **base 样式**。
    ///
    /// 行内的 base 样式由 `base_style` 决定（`"heading"` / `"strong"` /
    /// `"quote"`…），与 span 样式表是**两套**。若 base 段也走 span 样式表，
    /// 基准样式（标题色 / 粗体 / 引用斜体）就会**丢失**。
    ///
    /// 判据：base_style = `"quote"`（次级色 + 斜体）时，base 段必须是
    /// quote 的样式；span 段是 `strong` 的样式。两者必须不同。
    #[test]
    fn inline_base_segment_uses_base_style_not_span_style_table() {
        let theme = Theme::dark();
        let size = theme.metrics.preview_font_size;
        let line_height = size * theme.metrics.preview_line_height;
        let typo = Typography {
            size,
            line_height,
            base_style: "quote",
        };
        // "ab 粗 cd"：span 盖住中间的 "粗"（字节 3..6），两侧是 base 段。
        let spans = vec![SpanView {
            range: RangeView::new(3, 6),
            style: "strong".to_string(),
            url: None,
        }];
        assert!(
            "ab 粗 cd".is_char_boundary(3) && "ab 粗 cd".is_char_boundary(6),
            "用例前提：span 区间必须是字符边界"
        );
        let (job, _) = build_text_job(&theme, "ab 粗 cd", &spans, typo, false, 300.0);

        let quote_fmt = span_format(&theme, "quote", size, line_height);
        let strong_fmt = span_format(&theme, "strong", size, line_height);
        assert_ne!(
            quote_fmt.color, strong_fmt.color,
            "用例前提：quote 与 strong 的颜色必须不同，否则测不出 base 段是否走错样式表"
        );

        let base_seg = job
            .sections
            .iter()
            .find(|s| {
                job.text
                    .get(s.byte_range.start.0..s.byte_range.end.0)
                    .is_some_and(|t| t.contains("ab "))
            })
            .expect("应存在 base 段");
        assert_eq!(
            base_seg.format.color, quote_fmt.color,
            "base 段必须用 base 样式（quote），不得走 span 样式表"
        );
    }

    /// **用例自身的正确性**：每个 span 区间必须覆盖**预期的文本**。
    ///
    /// 这条用例存在的理由：对拍用例的 span 区间是**字节**偏移，中文字符占
    /// 3 字节——写错（把「粗」的 `3..6` 写成 `3..5`）会让区间被吸附成空
    /// → span 段根本不生成 → 对拍用例「通过」但**根本没测到那条路径**。
    ///
    /// 教训：判据里若含手工计算的数字，必须有一条独立用例钉住那些数字。
    /// 否则数字错了，判据会**恒绿**且看起来一切正常。
    #[test]
    fn parity_fixture_spans_cover_expected_text() {
        // 类型别名：`Vec<(&str, &str, Vec<(usize, usize)>)>` 触发 clippy 的
        // `type_complexity`（嵌套元组 + 嵌套 Vec）。
        type Case = (&'static str, &'static str, Vec<(usize, usize)>);
        let cases: Vec<Case> = vec![
            ("a**粗**b", "粗", vec![(3, 6)]),
            ("a**粗**_斜_", "粗", vec![(3, 6)]),
            ("a**粗**_斜_", "斜", vec![(9, 12)]),
            ("ab **粗** cd", "粗", vec![(5, 8)]),
            ("中**文**", "文", vec![(5, 8)]),
            ("值 $E=mc^2$ 结束", "$E=mc^2$", vec![(4, 12)]),
            ("见[^1]处", "[^1]", vec![(3, 7)]),
            ("已完成**重要**", "重要", vec![(11, 17)]),
        ];
        for (text, expected, ranges) in cases {
            for (s, e) in ranges {
                assert!(
                    text.is_char_boundary(s) && text.is_char_boundary(e),
                    "用例前提错误：{text:?} 的 ({s},{e}) 不是字符边界"
                );
                assert_eq!(
                    text.get(s..e),
                    Some(expected),
                    "用例前提错误：{text:?} 的 ({s},{e}) 应覆盖 {expected:?}"
                );
            }
        }
    }

    /// 行内路径：拼接文本必须与原文逐字节相同。
    #[test]
    fn inline_job_text_always_equals_source_text() {
        let theme = Theme::dark();
        let size = theme.metrics.preview_font_size;
        let line_height = size * theme.metrics.preview_line_height;
        let typo = Typography {
            size,
            line_height,
            base_style: "",
        };
        let cases: Vec<(&str, Vec<SpanView>)> = vec![
            ("普通文本", vec![]),
            (
                "a**粗**b",
                vec![SpanView {
                    range: RangeView::new(1, 3),
                    style: "strong".to_string(),
                    url: None,
                }],
            ),
            (
                "值 $E=mc^2$ 结束",
                vec![SpanView {
                    range: RangeView::new(2, 10),
                    style: "math".to_string(),
                    url: None,
                }],
            ),
        ];
        for (content, spans) in cases {
            let (job, _) = build_text_job(&theme, content, &spans, typo, false, 300.0);
            let text: String = job
                .sections
                .iter()
                .map(|s| {
                    job.text
                        .get(s.byte_range.start.0..s.byte_range.end.0)
                        .unwrap_or("")
                })
                .collect();
            // `math` 会被改写（剥掉 `$` 分隔符）→ 文本**故意**不等于原文。
            // 这里只断言「不丢字符、不重复」：改写后的文本应是原文的子序列。
            assert!(
                text.chars().count() <= content.chars().count(),
                "行内文本不得变长（content={content:?} → {text:?}）"
            );
            if !content.contains('$') {
                assert_eq!(text, content, "行内文本必须与原文逐字节相同");
            }
        }
    }

    // ========================================================================
    // 真帧对拍：编辑区路径 vs 预览区代码块路径
    // ========================================================================

    /// **真帧对拍**：编辑区路径与预览区代码块路径对**同一段代码**画出的
    /// 颜色序列必须**逐 span 相同**。
    ///
    /// # 为什么不是「两侧同一段 Markdown」
    ///
    /// 方案文档的原始判据写的是「预览区与编辑区**同一段代码**的语法色
    /// 逐 span 相同」。实测（2026-10-20，探针）：
    ///
    /// - 编辑区对**全文**用 `lang = "markdown"` 高亮 → syntect 的 markdown
    ///   语法只把 ``` 围栏**标记本身**标成 `Code`，**围栏内的代码一个字
    ///   都不上色**（实测：`"```rust\nfn main() {...}\n```"` 只产出 2 个 span，
    ///   分别是 0..3 与 43..47 的围栏标记）；
    /// - 预览区代码块用**围栏自己的语言**（如 `rust`）对**块内文本**单独
    ///   高亮 → 产出 6 个 span（`Keyword` / `Function` / …）。
    ///
    /// 也就是说：**两侧的语法来源本就不同**，「同一段代码两侧语法色相同」
    /// 在当前架构下**不可能达成**——它要的是「编辑区也按围栏语言高亮围栏
    /// 内的代码」，那是**另一个改造项**（编辑区多语言高亮），不属于 R-10
    /// （R-10 只统一「带样式的一段文本 → `LayoutJob`」的**构造器**）。
    ///
    /// 故本条断言**可达成的等价命题**：
    /// **同一段代码 + 同一套 span**，分别经
    /// 「编辑区路径」（`build_line_job`，边界并集）与
    /// 「预览区代码块路径」（`highlighted_code_job`，顺序游标）
    /// 渲染，两侧画出的**颜色序列逐 span 相同**。
    ///
    /// 这才是 R-10 的核心保证：**切段策略不同，但同一组 span 的配色必须一致**
    /// ——否则用户会看到「同一段代码在编辑区一个色、在预览区另一个色」。
    ///
    /// # 判据形态
    ///
    /// 真帧（`ctx.run_ui` → `FullOutput::shapes` → `Shape::Text`），
    /// 两侧各画一次 → 从画出的 `Shape::Text` 里取 `galley.job.sections` 的颜色。
    /// **不**直接断言 `LayoutJob`——那只是中间产物；真帧才证明「画到屏幕上
    /// 的颜色」一致。
    ///
    /// 字号**允许不同**（编辑区 14px vs 预览区 15px，是刻意设计），
    /// 故只比**颜色**。
    ///
    /// # 为什么**背景色也要比**
    ///
    /// 只比前景色会漏掉「查找命中背景叠加」这类差异：变异验证实测，
    /// 给预览区路径的 span 段叠加一个背景色后，本用例**仍绿**——
    /// 背景不同但前景色相同，肉眼看是「同一行代码有一段底色不同」，
    /// 一样是用户可见的漂移。故前景与背景**都比**。
    #[test]
    fn real_frame_editor_and_preview_code_paths_paint_same_colors() {
        let theme = Theme::dark();
        // 一段有多种语法的代码：`fn` / `main` / `let` / `=` / `1` / 注释。
        let code = "fn main() { let x = 1; // hi\n}";
        let spans = vec![
            SpanView {
                range: RangeView::new(0, 2),
                style: "keyword".to_string(),
                url: None,
            },
            SpanView {
                range: RangeView::new(3, 7),
                style: "function".to_string(),
                url: None,
            },
            SpanView {
                range: RangeView::new(10, 13),
                style: "keyword".to_string(),
                url: None,
            },
            SpanView {
                range: RangeView::new(16, 17),
                style: "operator".to_string(),
                url: None,
            },
            SpanView {
                range: RangeView::new(18, 19),
                style: "number".to_string(),
                url: None,
            },
            SpanView {
                range: RangeView::new(21, 25),
                style: "comment".to_string(),
                url: None,
            },
        ];
        // 前提：span 区间必须落在字符边界上，否则会被吸附 → 段数变化，
        // 颜色序列不可比。
        for s in &spans {
            assert!(
                code.is_char_boundary(s.range.start) && code.is_char_boundary(s.range.end),
                "用例前提错误：span {:?} 不是字符边界",
                s.range
            );
        }

        // 两侧各画一次，收集画出的 `Shape::Text` 的颜色序列。
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();

        let mut painted: Vec<Vec<(String, egui::Color32, egui::Color32)>> = Vec::new();
        for which in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 400.0),
                )),
                ..Default::default()
            };
            let job = if which == 0 {
                // 编辑区路径：边界并集 + 编辑区样式表。
                crate::presentation::theme::mapper::build_line_job(&theme, code, &spans, &[], 0.0)
            } else {
                // 预览区代码块路径：顺序游标 + 预览区等宽字体。
                highlighted_code_job(&theme, code, Some(&spans), f32::INFINITY)
            };
            let mut out = ctx.run_ui(input, |ui| {
                ui.add(egui::Label::new(std::sync::Arc::new(job.clone())));
            });
            // egui 0.37 起 `FullOutput` 携带未应用的 `TexturesDelta`，其 `Drop`
            // 会在非空即 panic（无头环境没有渲染后端去 apply）。
            out.textures_delta.clear();
            let seq: Vec<(String, egui::Color32, egui::Color32)> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t),
                    _ => None,
                })
                .filter(|t| t.galley.text().contains("fn main"))
                .flat_map(|t| {
                    t.galley.job.sections.iter().map(move |s| {
                        let txt = t
                            .galley
                            .job
                            .text
                            .get(s.byte_range.start.0..s.byte_range.end.0)
                            .unwrap_or("")
                            .to_string();
                        (txt, s.format.color, s.format.background)
                    })
                })
                .collect();
            painted.push(seq);
        }

        let editor_seq = &painted[0];
        let preview_seq = &painted[1];

        assert!(
            !editor_seq.is_empty() && !preview_seq.is_empty(),
            "两侧都必须真的画出文本（editor={editor_seq:?} preview={preview_seq:?}）"
        );
        // 前提：两侧段数必须一致——段数不同说明切段策略在这组 span 上
        // 分歧，颜色序列逐项比对就没有意义了。
        assert_eq!(
            editor_seq.len(),
            preview_seq.len(),
            "两侧段数应一致（editor={editor_seq:?} preview={preview_seq:?}）"
        );
        // 前提：颜色确实有多种（若全是同一个颜色，逐项比对恒真）。
        let distinct: std::collections::HashSet<_> =
            editor_seq.iter().map(|(_, c, _)| *c).collect();
        assert!(
            distinct.len() >= 3,
            "用例前提：颜色种类应 ≥3（实测 {} 种）",
            distinct.len()
        );

        for (i, ((et, ec, eb), (pt, pc, pb))) in
            editor_seq.iter().zip(preview_seq.iter()).enumerate()
        {
            assert_eq!(
                et, pt,
                "第 {i} 段的文本不同：编辑区 {et:?} vs 预览区 {pt:?}"
            );
            assert_eq!(
                ec, pc,
                "第 {i} 段（{et:?}）前景色不同：编辑区 {ec:?} vs 预览区 {pc:?}"
            );
            assert_eq!(
                eb, pb,
                "第 {i} 段（{et:?}）背景色不同：编辑区 {eb:?} vs 预览区 {pb:?}"
            );
        }
    }
}
