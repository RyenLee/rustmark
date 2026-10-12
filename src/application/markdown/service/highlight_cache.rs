//! 高亮缓存与高亮防抖计时表（版本 / 语言 / 区间为键，近似 LRU）。

use ahash::{HashMap, HashMapExt};

use crate::application::dto::SpanView;
use crate::shared::ids::DocumentId;
use crate::shared::time::Timestamp;

/// 一份缓存的高亮结果。
struct CachedHighlight {
    revision: u64,
    lang: Option<String>,
    range: (usize, usize),
    spans: Vec<SpanView>,
}

/// 高亮缓存：文档 ID → 上一次的高亮结果（近似 LRU，达到容量后驱逐一个旧键）。
///
/// # 为什么需要（S8-05 实测）
///
/// 编辑区每帧请求全文 span，而 syntect 解析是 O(全文)：10KB 文档一次 56.5ms。
/// 文档未变的帧（滚动 / 空闲 / 切换面板）占绝大多数，却要反复付这份钱。
/// 键里带上版本号 → 内容一改即失效，**语义与重算完全一致**，不存在「颜色滞后」。
///
/// 与 [`RenderCache`](super::RenderCache) 的区别：那份缓存的是解析产物（预览用），这份缓存的是
/// 高亮产物（编辑区用）；两者的失效条件都是「文档版本变化」。
#[derive(Default)]
pub struct HighlightCache {
    entries: HashMap<DocumentId, CachedHighlight>,
    capacity: usize,
    /// 命中次数（调试 / 测试断言用）。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
}

impl HighlightCache {
    /// 构造缓存；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: capacity.max(1),
            hits: 0,
            misses: 0,
        }
    }

    /// 取缓存命中（版本 / 语言 / 区间任一不同即视为失效）。
    pub fn get(
        &mut self,
        id: DocumentId,
        revision: u64,
        lang: Option<&str>,
        range: (usize, usize),
    ) -> Option<&Vec<SpanView>> {
        let matched = self.entries.get(&id).is_some_and(|e| {
            e.revision == revision && e.lang.as_deref() == lang && e.range == range
        });
        if matched {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        if !matched {
            return None;
        }
        self.entries.get(&id).map(|e| &e.spans)
    }

    /// 写入缓存（达到容量且为新键时驱逐一个旧键）。
    pub fn insert(
        &mut self,
        id: DocumentId,
        revision: u64,
        lang: Option<String>,
        range: (usize, usize),
        spans: Vec<SpanView>,
    ) {
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&id)
            && let Some(&old) = self.entries.keys().next()
        {
            self.entries.remove(&old);
        }
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
    pub fn invalidate(&mut self, id: DocumentId) {
        self.entries.remove(&id);
    }

    /// 失效全部缓存。
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }

    /// 缓存条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
    /// 文档 ID → (上次见到的版本, 上次见到它的时刻)。
    seen: HashMap<DocumentId, (u64, Timestamp)>,
    capacity: usize,
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
    pub fn settled(
        &mut self,
        id: DocumentId,
        revision: u64,
        now: Timestamp,
        interval_ms: u64,
    ) -> bool {
        match self.seen.get(&id) {
            None => {
                self.remember(id, revision, now);
                true
            }
            Some((rev, _)) if *rev != revision => {
                self.remember(id, revision, now);
                false
            }
            Some((_, at)) => now.elapsed_ms_since(*at) >= interval_ms,
        }
    }

    fn remember(&mut self, id: DocumentId, revision: u64, now: Timestamp) {
        if self.seen.len() >= self.capacity
            && !self.seen.contains_key(&id)
            && let Some(&old) = self.seen.keys().next()
        {
            self.seen.remove(&old);
        }
        self.seen.insert(id, (revision, now));
    }

    /// 已登记的文档数（测试用）。
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// 是否为空（测试用）。
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}
