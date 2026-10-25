//! 图片纹理缓存（TD-02 拆分：自 preview_view.rs 抽出）。
//!
//! 三种来源（本地路径 / 远程 URL / data URI）由 `AssetLoader` 统一处理，
//! 本模块只负责「解码 → 纹理」与失败降级：
//!
//! - 本地与 data URI 走同步路径（读盘 + 解码在几十毫秒量级，失败当帧出占位）；
//! - 远程走后台线程下载 + 解码 → 通道回执 → UI 线程建纹理（F-3）：绘制路径
//!   只发「加载中」占位，完成后由后台线程 `request_repaint` 唤醒下一帧。
//!
//! 缓存按引用字符串键（同一张图在多个文档里引用只解码一次），带 LRU 淘汰
//! （缺陷 G3：GPU 纹理不设上限会持续占显存）。

use std::cell::RefCell;
use std::collections::HashSet;

use egui::Ui;

use crate::app::frame::EditorApp;
use crate::shared::lru::LruMap;

/// 一张图片的加载结果。
///
/// 本体与字段均 `pub(super)`：消费者是 `media::draw_image`（TD-02 拆分后跨
/// 子模块），`image_entry` 的返回类型要在 `preview_view` 层可见。
#[derive(Clone)]
pub(super) struct ImageEntry {
    /// 成功解码后的纹理。
    pub(super) texture: Option<egui::TextureHandle>,
    /// 原始像素尺寸。
    pub(super) size: [f32; 2],
    /// 失败原因（显示成占位文本，不弹窗、不 panic）。
    pub(super) error: Option<String>,
    /// 远程图片的后台加载进行中（占位文案用「加载中」而非「无法加载」）。
    pub(super) loading: bool,
    /// 解码后占用的字节数（`宽 × 高 × 4`，RGBA）；加载中 / 失败为 0。
    ///
    /// 缓存的淘汰按**字节预算**结算，而不是按张数：压缩文件的体积
    /// 完全不能代表解码后的显存占用（512 KB 的 JPEG 可以是 6000×4000）。
    pub(super) bytes: usize,
}

impl ImageEntry {
    /// 远程加载进行中的占位条目（F-3）。
    fn loading() -> Self {
        Self {
            texture: None,
            size: [0.0, 0.0],
            error: None,
            loading: true,
            bytes: 0,
        }
    }
}

/// 图片纹理缓存的条目上限（张）：兜底，防止极小图撑爆 HashMap 条目数。
const IMAGE_CACHE_CAPACITY: usize = 64;

/// 图片纹理缓存的**字节预算**（128 MiB）。
///
/// 只按张数淘汰挡不住大图：单张 12MP 图解码后约 46 MiB RGBA，
/// 64 张的理论峰值接近 2.9 GiB。按解码后的 `宽 × 高 × 4` 结算，
/// 才能把显存 / 内存占用收束到与 NFR-PERF-03 同量级。
const IMAGE_CACHE_BUDGET_BYTES: usize = 128 * 1024 * 1024;

/// 解码时的最长边上限（像素）。
///
/// 预览最宽只画到 620pt，超出该量级的分辨率对屏幕没有意义，
/// 却会按原始像素占用内存。解码后先缩到 1600px 再建纹理，
/// 一张 6000×4000 的相机图从 96 MiB 降到约 6 MiB。
const DECODE_MAX_EDGE: u32 = 1600;

/// 图片纹理缓存：引用字符串 → 已解码纹理 / 失败原因，带 LRU 淘汰。
///
/// 淘汰策略本体在 [`LruMap`]（D-4 收敛：高亮 / 拼写 / 渲染 / 图片四处共用）。
struct ImageCache {
    entries: LruMap<String, ImageEntry>,
    /// 当前缓存内所有纹理的合计字节数（与 `entries` 同步维护）。
    bytes: usize,
    /// 条目上限（兜底）。
    capacity: usize,
    /// 字节预算（主约束）。
    budget: usize,
}

impl ImageCache {
    fn new(capacity: usize, budget: usize) -> Self {
        Self {
            // 容量交给本类型自己结算：LruMap 的容量淘汰不知道值的大小，
            // 若让它先踢，本类型的字节账就对不上了。
            entries: LruMap::new(usize::MAX),
            bytes: 0,
            capacity: capacity.max(1),
            budget: budget.max(1),
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
        if let Some(old) = self.entries.peek(&url) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        self.bytes = self.bytes.saturating_add(entry.bytes);
        self.entries.insert(url, entry);
        // 双约束：条目数兜底 + 字节预算为主；至少保留一张（当前帧正在画的）。
        while (self.entries.len() > self.capacity || self.bytes > self.budget)
            && self.entries.len() > 1
        {
            let Some((_key, evicted)) = self.entries.pop_oldest() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.bytes);
        }
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
    static IMAGE_CACHE: RefCell<ImageCache> =
        RefCell::new(ImageCache::new(IMAGE_CACHE_CAPACITY, IMAGE_CACHE_BUDGET_BYTES));
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
    /// 解码后的颜色图 / 失败原因。解码放在后台线程：8MiB 上限内可到数百毫秒。
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
                // 必须在 `load_texture` 移动 `img` 之前结算字节数。
                let bytes = img.size[0].saturating_mul(img.size[1]).saturating_mul(4);
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
                    bytes,
                }
            }
            Err(reason) => ImageEntry {
                texture: None,
                size: [0.0, 0.0],
                error: Some(reason),
                loading: false,
                bytes: 0,
            },
        };
        IMAGE_CACHE.with(|c| c.borrow_mut().insert(msg.url, entry));
    }
    any
}

/// 取（或首次加载）一张图片。
///
/// `pub(super)`：唯一生产调用点是 `media::draw_image`，经
/// `super::image_cache::image_entry` 取用（TD-02 拆分自 facade）。
///
/// 三种来源（本地路径 / 远程 URL / data URI）由 `AssetLoader` 统一处理，
/// 本函数只负责「解码 → 纹理」与失败降级。
///
/// # 远程图片为什么异步（F-3）
///
/// 远程地址若在绘制路径同步下载，UI 线程会阻塞到响应或超时（默认 10s）——
/// 含远程图的文档首绘预览时整个界面卡住。故远程走「后台线程下载 + 解码 →
/// 通道回执 → UI 线程建纹理」：绘制路径只发一个「加载中」占位，
/// 完成后由后台线程 `request_repaint` 唤醒下一帧换上真图。
pub(super) fn image_entry(ui: &Ui, app: &EditorApp, url: &str) -> ImageEntry {
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
        bytes: 0,
    };
    let entry = match app.services.assets.load(url, base_dir.as_deref()) {
        Err(e) => failed(e.to_string()),
        Ok(asset) => match decode(&asset.bytes) {
            Ok(img) => {
                let size = [img.width() as f32, img.height() as f32];
                // 必须在 `load_texture` 移动 `img` 之前结算字节数。
                let bytes = img.size[0].saturating_mul(img.size[1]).saturating_mul(4);
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
                    bytes,
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
    // 先降采样再转 RGBA：`thumbnail` 保持宽高比，且比全尺寸缩放的峰值内存低。
    let img = if img.width().max(img.height()) > DECODE_MAX_EDGE {
        img.thumbnail(DECODE_MAX_EDGE, DECODE_MAX_EDGE)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(bytes: usize) -> ImageEntry {
        ImageEntry {
            texture: None,
            size: [0.0, 0.0],
            error: None,
            loading: false,
            bytes,
        }
    }

    /// 淘汰按字节预算结算，而不是按张数：条目数未超上限也要能踢掉最旧的大图。
    #[test]
    fn cache_evicts_by_byte_budget() {
        let mut cache = ImageCache::new(8, 300);
        cache.insert("a".to_string(), entry(100));
        cache.insert("b".to_string(), entry(100));
        cache.insert("c".to_string(), entry(100));
        assert_eq!(cache.bytes, 300);

        cache.insert("d".to_string(), entry(100));
        assert_eq!(cache.entries.len(), 3, "400 字节超过 300 预算，应淘汰一张");
        assert!(cache.entries.peek("a").is_none(), "应淘汰访问序最旧的 a");
        assert_eq!(cache.bytes, 300);
        assert_eq!(cache.entries.len(), 3);
    }

    /// 条目数兜底仍然生效（极小图不会无限堆积）。
    #[test]
    fn cache_keeps_entry_cap() {
        let mut cache = ImageCache::new(2, 1024 * 1024);
        for key in ["a", "b", "c"] {
            cache.insert(key.to_string(), entry(1));
        }
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.entries.peek("a").is_none());
    }
}
