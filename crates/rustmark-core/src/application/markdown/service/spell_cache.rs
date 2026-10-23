//! 拼写检查缓存（文档 ID → 上一次的错词区间，真 LRU）。

use crate::shared::ids::DocumentId;
use crate::shared::lru::LruMap;

/// 一份缓存的拼写检查结果。
struct CachedSpell {
    revision: u64,
    hits: Vec<(usize, usize)>,
}

/// 拼写检查缓存：文档 ID → 上一次的错词区间（**真 LRU**：按访问序驱逐最旧者）。
///
/// # 为什么拼写检查也需要缓存
///
/// 词表近邻查找虽已做到 O(词长)，但整篇文档仍是 O(全文)——实测 900KB 文档
/// 单次 94ms（debug）。编辑区每帧都可能请求，滚动/空闲帧反复付这份钱不可接受。
/// 键带版本号 → 内容一改即失效，**与重算完全一致**，不存在「波浪线滞后」。
///
/// 与 [`HighlightCache`](super::HighlightCache) 同构，故刻意复用其容量常量而非另立配置。
/// 驱逐策略本体在 [`LruMap`]（D-4 收敛：四处缓存共用一份实现）。
pub(crate) struct SpellCache {
    entries: LruMap<DocumentId, CachedSpell>,
    /// 命中次数（调试 / 测试断言用）。
    pub(crate) hits: u64,
    /// 未命中次数。
    pub(crate) misses: u64,
}

impl SpellCache {
    /// 构造容量为 `capacity` 的缓存。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: LruMap::new(capacity),
            hits: 0,
            misses: 0,
        }
    }

    /// 读取；版本不匹配视为未命中。
    pub(crate) fn get(&mut self, doc_id: DocumentId, revision: u64) -> Option<Vec<(usize, usize)>> {
        let matched = self
            .entries
            .peek(&doc_id)
            .is_some_and(|e| e.revision == revision);
        if matched {
            self.hits += 1;
            // 条件命中才续命（`LruMap` 的 peek/touch 两步式，见其类型文档）。
            self.entries.touch(&doc_id);
            self.entries.peek(&doc_id).map(|e| e.hits.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    /// 写入（超容量且为新键时，驱逐访问序最旧的一个键）。
    pub(crate) fn insert(&mut self, doc_id: DocumentId, revision: u64, hits: Vec<(usize, usize)>) {
        self.entries.insert(doc_id, CachedSpell { revision, hits });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_evicts_least_recently_used_not_arbitrary() {
        let mut cache = SpellCache::new(2);
        cache.insert(id(1), 1, vec![]);
        cache.insert(id(2), 1, vec![]);
        // 触碰 1 → 访问序变为 [2, 1]，2 成为最旧。
        let _ = cache.get(id(1), 1);
        // 插入第 3 个，应驱逐最旧的 2，保留近期用过的 1。
        cache.insert(id(3), 1, vec![]);
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.get(id(1), 1).is_some(), "近期命中的文档不应被驱逐");
        assert!(cache.get(id(2), 1).is_none(), "最旧且未再访问的应被驱逐");
        assert!(cache.get(id(3), 1).is_some());
    }

    #[test]
    fn invalidate_removes_from_order() {
        let mut cache = SpellCache::new(4);
        cache.insert(id(1), 1, vec![]);
        cache.insert(id(2), 1, vec![]);
        cache.invalidate(id(1));
        assert!(cache.get(id(1), 1).is_none());
        assert!(cache.get(id(2), 1).is_some());
    }

    #[test]
    fn stale_revision_is_miss() {
        let mut cache = SpellCache::new(4);
        cache.insert(id(1), 1, vec![(0, 3)]);
        assert!(cache.get(id(1), 2).is_none());
        assert_eq!(cache.misses, 1);
    }

    /// 版本不匹配的查询**不得**续命（否则陈旧键永不被驱逐）。
    #[test]
    fn stale_query_does_not_touch_order() {
        let mut cache = SpellCache::new(2);
        cache.insert(id(1), 1, vec![]);
        cache.insert(id(2), 1, vec![]);
        let _ = cache.get(id(1), 99); // 版本不符 → 未命中，不续命
        cache.insert(id(3), 1, vec![]);
        assert!(cache.get(id(1), 1).is_none(), "陈旧查询不得续命最旧键");
        assert!(cache.get(id(2), 1).is_some());
    }

    fn id(n: u64) -> DocumentId {
        DocumentId::from_u64(n)
    }
}
