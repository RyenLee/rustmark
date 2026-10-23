//! 渲染防抖策略与渲染缓存（文档 ID → (版本号, 块序列)，真 LRU）。

use crate::domain::markdown::value_objects::RenderedBlock;
use crate::shared::ids::DocumentId;
use crate::shared::lru::LruMap;
use crate::shared::time::Timestamp;

/// 防抖策略（FR-01，150ms）：把时间相关决策做成可注入、可单测的纯函数。
///
/// 原型把 150ms 防抖写在组件里（`setTimeout`），无法单测，这是可测性上的教训；
/// 本类型把「是否应当渲染」收束为一个纯判断。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DebouncePolicy {
    interval_ms: u64,
}

impl DebouncePolicy {
    /// 构造策略；`interval_ms` 通常为 150（FR-01）。
    pub const fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }

    /// 防抖间隔（毫秒）。
    pub const fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    /// 是否应当渲染：**距上次编辑 >= interval_ms（含端点）** 时返回 `true`。
    ///
    /// - 越短（仍在防抖窗口内）→ `false`：连续输入只保留最后一次；
    /// - 时钟回拨（now < last_edit，理论上不应发生）→ 饱和到 `0`，返回 `false`。
    pub fn should_render(&self, last_edit: Timestamp, now: Timestamp) -> bool {
        now.elapsed_ms_since(last_edit) >= self.interval_ms
    }
}

/// 一份缓存的渲染结果。
struct CachedRender {
    revision: u64,
    blocks: Vec<RenderedBlock>,
}

/// 渲染缓存：文档 ID → (版本号, 块序列)，**真 LRU**（按访问序驱逐最旧者）。
///
/// 驱逐策略本体在 [`LruMap`]（D-4 收敛：四处缓存共用一份实现）。
pub struct RenderCache {
    entries: LruMap<DocumentId, CachedRender>,
    /// 命中次数（调试 / 测试断言用）。
    pub(crate) hits: u64,
    /// 未命中次数。
    pub(crate) misses: u64,
}

impl RenderCache {
    /// 构造缓存；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: LruMap::new(capacity),
            hits: 0,
            misses: 0,
        }
    }

    /// 取缓存命中（要求版本号一致，否则视为失效）。
    ///
    /// 命中时触碰访问序并把键挪到尾部（最新），使**正在预览的文档**永不被误驱逐。
    pub(crate) fn get(&mut self, id: DocumentId, revision: u64) -> Option<&Vec<RenderedBlock>> {
        let hit = self
            .entries
            .peek(&id)
            .is_some_and(|entry| entry.revision == revision);
        if hit {
            self.hits += 1;
            self.entries.touch(&id);
            self.entries.peek(&id).map(|entry| &entry.blocks)
        } else {
            self.misses += 1;
            None
        }
    }

    /// 写入缓存（达到容量且为新键时，驱逐访问序最旧的一个键）。
    pub(crate) fn insert(&mut self, id: DocumentId, revision: u64, blocks: Vec<RenderedBlock>) {
        self.entries.insert(id, CachedRender { revision, blocks });
    }

    /// 失效单个文档的缓存。
    pub(crate) fn invalidate(&mut self, id: DocumentId) {
        self.entries.invalidate(&id);
    }

    /// 失效全部缓存（主题切换后调用）。
    pub(crate) fn invalidate_all(&mut self) {
        self.entries.invalidate_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut cache = RenderCache::new(2);
        cache.insert(id(1), 1, vec![]);
        cache.insert(id(2), 1, vec![]);
        // 触碰 1 → 访问序变为 [2, 1]，2 成为最旧。
        assert!(cache.get(id(1), 1).is_some());
        // 插入第 3 个，应驱逐最旧的 2，保留近期用过的 1。
        cache.insert(id(3), 1, vec![]);
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.get(id(1), 1).is_some(), "近期命中的文档不应被驱逐");
        assert!(cache.get(id(2), 1).is_none(), "最旧且未再访问的应被驱逐");
        assert!(cache.get(id(3), 1).is_some());
    }

    #[test]
    fn stale_revision_counts_as_miss() {
        let mut cache = RenderCache::new(4);
        cache.insert(id(1), 1, vec![]);
        assert!(cache.get(id(1), 2).is_none());
        assert_eq!(cache.misses, 1);
        assert_eq!(cache.hits, 0);
    }

    #[test]
    fn invalidate_removes_from_order() {
        let mut cache = RenderCache::new(4);
        cache.insert(id(1), 1, vec![]);
        cache.insert(id(2), 1, vec![]);
        cache.invalidate(id(1));
        assert!(cache.get(id(1), 1).is_none());
        assert!(cache.get(id(2), 1).is_some());
    }

    fn id(n: u64) -> DocumentId {
        DocumentId::from_u64(n)
    }
}
