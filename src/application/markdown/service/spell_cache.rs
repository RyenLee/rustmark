//! 拼写检查缓存（文档 ID → 上一次的错词区间，近似 LRU）。

use ahash::{HashMap, HashMapExt};

use crate::shared::ids::DocumentId;

/// 一份缓存的拼写检查结果。
struct CachedSpell {
    revision: u64,
    hits: Vec<(usize, usize)>,
}

/// 拼写检查缓存：文档 ID → 上一次的错词区间（近似 LRU）。
///
/// # 为什么拼写检查也需要缓存
///
/// 词表近邻查找虽已做到 O(词长)，但整篇文档仍是 O(全文)——实测 900KB 文档
/// 单次 94ms（debug）。编辑区每帧都可能请求，滚动/空闲帧反复付这份钱不可接受。
/// 键带版本号 → 内容一改即失效，**与重算完全一致**，不存在「波浪线滞后」。
///
/// 与 [`HighlightCache`](super::HighlightCache) 同构，故刻意复用其容量常量而非另立配置。
#[derive(Default)]
pub struct SpellCache {
    entries: HashMap<DocumentId, CachedSpell>,
    capacity: usize,
    /// 命中次数（调试 / 测试断言用）。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
}

impl SpellCache {
    /// 构造容量为 `capacity` 的缓存。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            hits: 0,
            misses: 0,
        }
    }

    /// 读取；版本不匹配视为未命中。
    pub fn get(&mut self, doc_id: DocumentId, revision: u64) -> Option<Vec<(usize, usize)>> {
        match self.entries.get(&doc_id) {
            Some(e) if e.revision == revision => {
                self.hits += 1;
                Some(e.hits.clone())
            }
            _ => {
                self.misses += 1;
                None
            }
        }
    }

    /// 写入（超容量时驱逐一个旧键）。
    pub fn insert(&mut self, doc_id: DocumentId, revision: u64, hits: Vec<(usize, usize)>) {
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&doc_id)
            && let Some(k) = self.entries.keys().next().copied()
        {
            self.entries.remove(&k);
        }
        self.entries.insert(doc_id, CachedSpell { revision, hits });
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
