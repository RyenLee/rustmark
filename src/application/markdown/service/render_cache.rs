//! 渲染防抖策略与渲染缓存（文档 ID → (版本号, 块序列)，近似 LRU）。

use std::collections::HashMap;

use crate::domain::markdown::value_objects::RenderedBlock;
use crate::shared::ids::DocumentId;
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

/// 渲染缓存：文档 ID → (版本号, 块序列)，近似 LRU（达到容量后驱逐一个旧键）。
#[derive(Default)]
pub struct RenderCache {
    entries: HashMap<DocumentId, CachedRender>,
    capacity: usize,
}

impl RenderCache {
    /// 构造缓存；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    /// 取缓存命中（要求版本号一致，否则视为失效）。
    pub fn get(&self, id: DocumentId, revision: u64) -> Option<&Vec<RenderedBlock>> {
        self.entries
            .get(&id)
            .filter(|entry| entry.revision == revision)
            .map(|entry| &entry.blocks)
    }

    /// 写入缓存（达到容量且为新键时驱逐一个旧键）。
    pub fn insert(&mut self, id: DocumentId, revision: u64, blocks: Vec<RenderedBlock>) {
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&id)
            && let Some(&old) = self.entries.keys().next()
        {
            self.entries.remove(&old);
        }
        self.entries.insert(id, CachedRender { revision, blocks });
    }

    /// 失效单个文档的缓存。
    pub fn invalidate(&mut self, id: DocumentId) {
        self.entries.remove(&id);
    }

    /// 失效全部缓存（主题切换后调用）。
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
