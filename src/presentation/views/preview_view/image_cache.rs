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

use crate::presentation::view_context::EditorViewContext;
use rustmark_core::shared::lru::LruMap;

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

/// 本地 / data URI 图片超过该体积就**不在 UI 线程解码**。
///
/// 这两种来源原先整条链路走同步路径，理由是「读盘 + 解码在几十毫秒量级」
/// ——那对几十 KB 的小图成立，对接近 `AssetLimits::max_local_bytes`（8 MiB）
/// 上限的大图不成立：`image::load_from_memory` 要先完整解码到数千万像素
/// （24MP 约 96 MiB 临时缓冲）才轮到 [`DECODE_MAX_EDGE`] 降采样，单次开销
/// 足以把一帧拖到几百毫秒——表现为滚动预览时可感知的一次卡顿尖刺。
///
/// 超过阈值的改为后台解码（复用远程那条回执通道）。阈值取 512 KiB：压缩后的
/// JPEG / PNG 到这个量级，对应像素数已足以越过帧预算；更小的图保持同步，
/// 是为了不让「本地文件出现在磁盘上却要闪一下占位」这种观感退化。
const BACKGROUND_DECODE_BYTES: usize = 512 * 1024;

/// 后台任务（远程下载、本地大图解码）的**并发上限**。
///
/// 早先每个不同的远程 URL 起一条脱离线程：一篇引用上百张远程图的文档会同时
/// 拉起上百个线程，各自占栈、各自一次 TLS 握手，而它们往往指向同一两个图床。
/// 设上限后多余的 URL 本帧只出占位、**下一帧重试**——所以那个占位**不能**
/// 写进 `IMAGE_CACHE`（写进去就成了永久缓存命中，再也没人来重试了，见
/// [`image_entry`] 的注释）。
///
/// 取 8：够撑住「一屏十几张图」的并发观感，又不至于让 OS 线程数失控。
const MAX_BACKGROUND_LOADS: usize = 8;

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
    /// 也是「已有线程在下载」的标记（配合 [`BACKGROUND_INFLIGHT`] 防重复 spawn）。
    static IMAGE_CACHE: RefCell<ImageCache> =
        RefCell::new(ImageCache::new(IMAGE_CACHE_CAPACITY, IMAGE_CACHE_BUDGET_BYTES));
    /// 正在后台处理的图片（防重复 spawn 的守卫）。
    ///
    /// 覆盖**全部**后台任务（远程下载 + 本地大图解码）。正常情况下列
    /// 「加载中」条目在缓存里就能挡住重复 spawn；本集合兜住「加载中被 LRU
    /// 淘汰、线程还没回来」这一窗口，避免同一张图起两条线程。
    static BACKGROUND_INFLIGHT: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// 当前在跑的后台任务数（[`MAX_BACKGROUND_LOADS`] 的计数器）。
static BACKGROUND_SLOTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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

/// 是否远程地址（http / https）：只有它们强制走后台线程（F-3）。
///
/// 本地路径与 data URI 原则上保持同步路径，但**超过 [`BACKGROUND_DECODE_BYTES`]
/// 的例外**——不能让一张相机原图把 UI 线程按在地上。
pub fn is_remote_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

/// 关闭「远程图片」后占位里显示的原因（PRIV-01）。
///
/// 与断网 / 主机不可达共用同一套占位版式，只换这句原因——用户要能一眼区分
/// 「网络不通」和「我自己关了它」，否则会去查网络而不是查设置。
/// 常量而非就地字面量：测试按它断言，文案改动必须有编译期牵引。
pub(super) const REMOTE_DISABLED_REASON: &str = "远程图片加载已在设置中关闭（设置 → 编辑器）";

/// 后台任务的实际活儿：下载 + 解码，或只解码（已在 UI 线程读到的本地大图）。
///
/// 装箱是因为两类作业的闭包类型不同（捕获的上下文不一样），而回执通道只有
/// 一种消息。作业数为 O(文档引用到的图片数)，装箱开销可以忽略。
type LoadingWork = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

/// 后台起一个加载任务；完成后经通道回执并唤醒 UI。
///
/// # 返回值是有含义的
///
/// **返回 `false` 表示槽位已满、这次没起**：调用方必须把它当成「还没开始」——
/// 本帧给个占位、下一帧再来重试。绝不能顺手把占位写进缓存了事：那会让占位
/// 变成永久命中，这张图从此再也没人来重试（见 [`image_entry`] 的分支注释）。
///
/// # 为什么要有上限
///
/// 早先来一个 URL 就 `std::thread::spawn` 一条：线程数随**文档里不同远程地址
/// 的个数**线性增长，而每条线程都要占栈、都要自己握手一遍 TLS。上限把「并发」
/// 与「待办数量」解耦——待办的图排队等空位，而不是同时把自己塞进来。
///
/// # 额度怎么归还
///
/// 额度在线程**即将退出前**归还，且这条路不经过任何可能提前返回的分支
/// （无论 `run` 成功还是失败都会走到 `fetch_sub`）。
fn spawn_background(url: &str, run: LoadingWork, ctx: egui::Context) -> bool {
    use std::sync::atomic::Ordering;

    let taken = BACKGROUND_SLOTS.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
        (n < MAX_BACKGROUND_LOADS).then_some(n + 1)
    });
    if taken.is_err() {
        return false;
    }
    let url = url.to_string();
    let tx = remote_channel().tx.clone();
    std::thread::spawn(move || {
        let result = run();
        // 先归还额度再回执：回执会立刻唤醒下一帧，那一帧可能马上要排新任务。
        BACKGROUND_SLOTS.fetch_sub(1, Ordering::SeqCst);
        let _ = tx.send(RemoteLoadDone { url, result });
        // **为什么必须 `request_repaint`**：加载中的占位符不会自行重绘，
        // 后台线程完成时若只是往通道里塞消息，UI 可能隔很久才跑下一帧——
        // 用户看着「加载中」干等。`egui::Context` 是 `Send + Sync`，
        // 专为这种跨线程唤醒设计。
        ctx.request_repaint();
    });
    true
}

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
        BACKGROUND_INFLIGHT.with(|f| f.borrow_mut().remove(&msg.url));
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
/// 三种来源（本地路径 / 远程 URL / data URI）的**字节取用**由
/// [`EditorViewContext::preview_image_fetcher`] 统一交给组合根
/// （`AssetLoader` 住 infra、文档目录是组合根的 `pub(crate)`，本层都名不到）；
/// 本函数只负责「解码 → 纹理」与失败降级。
///
/// # 远程图片为什么异步（F-3）
///
/// 远程地址若在绘制路径同步下载，UI 线程会阻塞到响应或超时（默认 10s）——
/// 含远程图的文档首绘预览时整个界面卡住。故远程走「后台线程下载 + 解码 →
/// 通道回执 → UI 线程建纹理」：绘制路径只发一个「加载中」占位，
/// 完成后由后台线程 `request_repaint` 唤醒下一帧换上真图。
/// 取字节的闭包是 `'static + Send` 的，正好能整个搬进后台线程。
pub(super) fn image_entry(ui: &Ui, vctx: &dyn EditorViewContext, url: &str) -> ImageEntry {
    // 回执先入账：后台线程在本帧之前完成的加载，此刻就能当缓存命中返回。
    drain_remote_results(ui.ctx());
    // 远程图片开关（PRIV-01）**在缓存查找之前**判：关闭后这张图在本会话里
    // 一律按「不可加载」呈现，既不发请求、也不复用此前已缓存的纹理——
    // 「关了还看得见」会让用户无法确认自己是否真的断开了外联。
    // 占位不进缓存：重新打开开关时下一帧就会重新走正常加载路径。
    if is_remote_url(url) && !vctx.editor_state_read().load_remote_images {
        return ImageEntry {
            texture: None,
            size: [0.0, 0.0],
            error: Some(REMOTE_DISABLED_REASON.to_string()),
            loading: false,
            bytes: 0,
        };
    }
    if let Some(cached) = IMAGE_CACHE.with(|c| c.borrow_mut().get(url)) {
        return cached;
    }

    if is_remote_url(url) {
        let in_flight = BACKGROUND_INFLIGHT.with(|f| f.borrow().contains(url.trim()));
        if !in_flight {
            // 取字节的闭包是 `'static + Send` 的：整个搬进后台线程，
            // 组合根那边的加载器不会被借出 `image_entry` 的栈帧。
            let fetch = vctx.preview_image_fetcher(url);
            let spawned = spawn_background(
                url,
                Box::new(move || fetch().and_then(|bytes| decode(&bytes))),
                ui.ctx().clone(),
            );
            // 仅在真正起了一条线程时才登记「在飞」；槽位满（`spawn_background`
            // 返回 `false`）则下一帧应重试，不登记、也不写缓存。
            if spawned {
                BACKGROUND_INFLIGHT.with(|f| f.borrow_mut().insert(url.trim().to_string()));
            }
        }
        let entry = ImageEntry::loading();
        // 槽位满的占位不进缓存（下一帧重试）；已起线程的占位进缓存，
        // 待 `drain_remote_results` 换上真图。
        if BACKGROUND_INFLIGHT.with(|f| f.borrow().contains(url.trim())) {
            IMAGE_CACHE.with(|c| c.borrow_mut().insert(url.to_string(), entry.clone()));
        }
        return entry;
    }

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
    // 本地 / data URI：先取到字节；超过 [`BACKGROUND_DECODE_BYTES`] 的大图
    // 不在 UI 线程解码（E-13），改走后台解码、本帧出「加载中」占位，避免
    // 帧尖刺。远程地址已在上面的分支处理。
    let entry = match vctx.preview_image_fetcher(url)() {
        Err(e) => failed(e),
        Ok(bytes) => {
            if bytes.len() > BACKGROUND_DECODE_BYTES {
                let in_flight = BACKGROUND_INFLIGHT.with(|f| f.borrow().contains(url.trim()));
                if !in_flight {
                    let spawned =
                        spawn_background(url, Box::new(move || decode(&bytes)), ui.ctx().clone());
                    if spawned {
                        BACKGROUND_INFLIGHT.with(|f| f.borrow_mut().insert(url.trim().to_string()));
                    }
                }
                let placeholder = ImageEntry::loading();
                // 槽位满（未能起线程）的占位不进缓存，下一帧重试。
                if BACKGROUND_INFLIGHT.with(|f| f.borrow().contains(url.trim())) {
                    IMAGE_CACHE
                        .with(|c| c.borrow_mut().insert(url.to_string(), placeholder.clone()));
                }
                return placeholder;
            }
            // 小图：同步解码（几十毫秒量级，当帧出纹理）。
            match decode(&bytes) {
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
            }
        }
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
