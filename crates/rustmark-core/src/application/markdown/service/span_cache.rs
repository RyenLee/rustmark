//! 高亮缓存与高亮防抖计时表（版本 / 语言 / 区间为键，真 LRU）。

use std::sync::Arc;

use ahash::{HashMap, HashMapExt};

use crate::application::dto::SpanView;
use crate::ports::time::Timestamp;
use crate::shared::ids::DocumentId;
use crate::shared::lru::LruMap;

/// 一份缓存的高亮结果。
///
/// `spans` 用 `Arc` 共享：编辑区**每帧**都要向服务要这份片段，
/// 若按值返回就等于每帧把几千个 `String` 克隆一遍（问题 046 实测：
/// 100KB 文档 3080 个 span，每帧 0.108ms 纯粹花在这上面）。
/// 片段是不可变快照，共享零成本且语义完全一致。
struct CachedHighlight {
    revision: u64,
    lang: Option<String>,
    range: (usize, usize),
    spans: Arc<Vec<SpanView>>,
}

/// 空片段常量：防抖期与大文件降级两条路径每帧都要返回它。
///
/// 返回共享的 `Arc` 而不是每次 `Arc::new(Vec::new())`，让这两条
/// 「本帧不上色」的路径做到零分配。`Arc<Vec<T>>` 本身不是 `Send` 约束的
/// 障碍（`SpanView` 全是 `Send + Sync`）。
pub(super) fn empty_spans() -> Arc<Vec<SpanView>> {
    static EMPTY: std::sync::OnceLock<Arc<Vec<SpanView>>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| Arc::new(Vec::new())).clone()
}

/// 高亮缓存：文档 ID → 上一次的高亮结果（**真 LRU**：按访问序驱逐最旧者）。
///
/// # 为什么需要（S8-05 实测）
///
/// 编辑区每帧请求全文 span，而 syntect 解析是 O(全文)：10KB 文档一次 56.5ms。
/// 文档未变的帧（滚动 / 空闲 / 切换面板）占绝大多数，却要反复付这份钱。
/// 键里带上版本号 → 内容一改即失效，**语义与重算完全一致**，不存在「颜色滞后」。
///
/// # 为什么必须是 LRU 而不是「驱逐任意一个键」（问题 043）
///
/// 原实现用 `self.entries.keys().next()`（`HashMap` **任意**迭代序）挑驱逐对象，
/// 可能正好驱逐下一次就要命中的文档：容量 8 小于打开文档数时，切一轮标签
/// 就是一次 56ms 级的重算尖峰。现在访问序由 [`LruMap`] 维护（D-4 收敛：
/// 四处缓存共用一份实现）——`get` 命中把键挪到尾部，`insert` 从头部驱逐
/// 最旧者，行为完全确定。
///
/// 与 [`RenderCache`](super::RenderCache) 的区别：那份缓存的是解析产物（预览用），这份缓存的是
/// 高亮产物（编辑区用）；两者的失效条件都是「文档版本变化」。
pub struct HighlightCache {
    entries: LruMap<DocumentId, CachedHighlight>,
    /// 命中次数（调试 / 测试断言用）。
    pub(crate) hits: u64,
    /// 未命中次数。
    pub(crate) misses: u64,
}

impl HighlightCache {
    /// 构造缓存；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: LruMap::new(capacity),
            hits: 0,
            misses: 0,
        }
    }

    /// 取缓存命中（版本 / 语言 / 区间任一不同即视为失效）。
    ///
    /// 返回共享的 `Arc`：命中路径**不得**克隆片段本身（见 [`CachedHighlight`]）。
    pub fn get(
        &mut self,
        id: DocumentId,
        revision: u64,
        lang: Option<&str>,
        range: (usize, usize),
    ) -> Option<Arc<Vec<SpanView>>> {
        let matched = self.entries.peek(&id).is_some_and(|e| {
            e.revision == revision && e.lang.as_deref() == lang && e.range == range
        });
        if matched {
            self.hits += 1;
            // 条件命中才续命（版本 / 语言 / 区间全对才是这一次查询的命中）。
            self.entries.touch(&id);
        } else {
            self.misses += 1;
            return None;
        }
        self.entries.peek(&id).map(|e| Arc::clone(&e.spans))
    }

    /// 写入缓存（达到容量且为新键时驱逐**最旧**的一个键）。
    pub fn insert(
        &mut self,
        id: DocumentId,
        revision: u64,
        lang: Option<String>,
        range: (usize, usize),
        spans: Arc<Vec<SpanView>>,
    ) {
        self.entries.insert(
            id,
            CachedHighlight {
                revision,
                lang,
                range,
                spans,
            },
        );
    }

    /// 失效单个文档的缓存。
    pub(crate) fn invalidate(&mut self, id: DocumentId) {
        self.entries.invalidate(&id);
    }

    /// 失效全部缓存。
    pub(crate) fn invalidate_all(&mut self) {
        self.entries.invalidate_all();
    }
}

/// 高亮防抖：记住「最近一次发现文档版本变化的时刻」（OPT-011 ①）。
///
/// # 为什么必须打在「版本变化被发现的那一帧」
///
/// 与预览防抖（OPT-012）同一个教训：时间戳若打在「算完了」的时刻，计时起点会被
/// 每次计算刷新，语义就从**防抖**退化成**节流**——连续打字照样每帧重算，
/// 10KB 文档白付 56ms/帧。这里打的戳来自「本帧见到的 revision 与上次不同」
/// 这一事实，**与是否真的算了高亮无关**，因此防抖期内戳不会被刷新。
///
/// # 为什么首次见某篇文档不防抖
///
/// 打开 / 切换文档应当**立刻**有颜色：那是用户停下来看的时刻，
/// 再等 150ms 只会看到一段「先无色、再上色」的闪烁。
#[derive(Default)]
pub struct HighlightPace {
    /// 文档 ID → 该文档的防抖状态。
    seen: HashMap<DocumentId, PaceEntry>,
    capacity: usize,
}

/// 单篇文档的防抖状态。
#[derive(Clone, Copy, Debug)]
struct PaceEntry {
    /// 上次见到的版本。
    revision: u64,
    /// 上次见到**变化**的时刻（防抖窗口的起点，不是「算完」的时刻）。
    at: Timestamp,
    /// 该文档是否还没算到当前版本（正在等窗口到期）。
    ///
    /// 缺了这个标记，[`HighlightPace::next_deadline`] 无法区分「已算完、窗口早过了」
    /// 与「还没算、正等窗口」——前者会算出一个过去的时刻，被帧循环钳成 1ms，
    /// 于是静止的应用变成每帧重绘的忙循环。
    pending: bool,
}

impl HighlightPace {
    /// 构造；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            seen: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    /// 记录本帧见到的版本，返回 `true` 表示「文档已静止，可以重算」。
    ///
    /// - 首次见该文档 → `true`（立刻上色，见类型文档）；
    /// - 版本变了 → 记下新时刻并返回 `false`（本帧不上色）；
    /// - 版本未变 → 距上次变化是否已满 `interval_ms`。
    pub(crate) fn settled(
        &mut self,
        id: DocumentId,
        revision: u64,
        now: Timestamp,
        interval_ms: u64,
    ) -> bool {
        let Some(entry) = self.seen.get_mut(&id) else {
            self.remember(id, revision, now, false);
            return true;
        };
        if entry.revision != revision {
            entry.revision = revision;
            entry.at = now;
            entry.pending = true;
            return false;
        }
        // 版本没变：窗口够了就收口（`pending` 清零），否则继续等。
        entry.pending = now.elapsed_ms_since(entry.at) < interval_ms;
        !entry.pending
    }

    /// 距下一次「需要补算」的绝对毫秒时刻；没有文档在等补算时返回 `None`。
    ///
    /// 供帧循环登记到期帧（T11）：`settled` 返回 `false` 时本帧不上色，
    /// 若没人保证窗口到期时再跑一帧，语法色会一直停在无色状态——
    /// 编辑区聚焦时靠光标闪烁（每 0.5s）偶然补上，失焦后就是永久无色。
    ///
    /// 只统计 `pending` 的文档：已算完的文档窗口早已过去，把它们算进来会得到
    /// 一个过去的时刻，反而让应用每帧都被叫醒。
    pub(crate) fn next_deadline(&self, interval_ms: u64) -> Option<u64> {
        self.seen
            .values()
            .filter(|entry| entry.pending)
            .map(|entry| entry.at.millis().saturating_add(interval_ms))
            .min()
    }

    /// 登记「本帧见到该文档」。
    ///
    /// 满容量时**任意**淘汰一个（`keys().next()`）——与高亮缓存的真 LRU 不同，
    /// 这是刻意为之：本表只存「版本 + 时刻」这类廉价 pacing 元数据，误淘汰的代价
    /// 至多是该文档下次编辑时被当成「首次见到」而**多上色一次**（保守降级，既非
    /// 重算尖峰、也非陈旧显示）。若哪天这里也改存昂贵产物，再改为与缓存一致的 LRU。
    fn remember(&mut self, id: DocumentId, revision: u64, now: Timestamp, pending: bool) {
        if self.seen.len() >= self.capacity
            && !self.seen.contains_key(&id)
            && let Some(&old) = self.seen.keys().next()
        {
            self.seen.remove(&old);
        }
        self.seen.insert(
            id,
            PaceEntry {
                revision,
                at: now,
                pending,
            },
        );
    }

    /// 防抖状态快照（测试与调试用）：`(文档 ID, 上次见到的版本, 上次见到的时刻毫秒)`。
    ///
    /// 回答「这一帧为什么不上色」：版本刚变（未静止）、
    /// 还是静止未满 `interval_ms`。缺了它，回归只能凭耗时间接推断。
    pub(crate) fn snapshot(&self) -> Vec<(DocumentId, u64, u64)> {
        self.seen
            .iter()
            .map(|(id, entry)| (*id, entry.revision, entry.at.millis()))
            .collect()
    }
}
