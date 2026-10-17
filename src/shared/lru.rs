//! 有界 LRU 映射（D-4：四处缓存同构实现的收敛）。
//!
//! # 为什么需要它
//!
//! 高亮 / 拼写 / 渲染 / 图片四个缓存曾各写一份「`entries` + `order` +
//! `touch` + `evict`」四件套：同一淘汰策略四份实现，任何修复（批量失效、
//! 统计、驱逐口径）要改四处，且已实际发生过「模块头注释漂移」（四份注释
//! 对同一行为的描述各自演化）。收敛到这一份后，行为只有一处真相。
//!
//! # 契约（与四处原实现逐条对齐，勿随意更改）
//!
//! - **容量至少为 1**：`new(0)` 按 1 处理（0 容量的缓存没有存在意义）；
//! - **驱逐只发生在「已满且插入的是新键」时**：覆盖已有键永不驱逐，
//!   且覆盖会刷新访问序（正在用的文档不会被自己挤掉）；
//! - **命中即把键挪到访问序尾部**：`get` / `get_mut` 命中时 touch，
//!   调用方自行判定「条件命中」（版本号 / 语言 / 区间）时用
//!   [`LruMap::peek`] + [`LruMap::touch`] 两步，避免未命中的查询也
//!   污染访问序（那会把即将被驱逐的键一次次续命）；
//! - **`entries` 与 `order` 的键集始终保持一致**：所有写路径都经
//!   [`LruMap::insert`] / [`LruMap::invalidate`] / [`LruMap::invalidate_all`]，
//!   不向外暴露可变引用。
//!
//! # 为什么键用 ahash
//!
//! 四个缓存的键都是 `DocumentId`（u64 新类型）或引用字符串，
//! 与 `Cargo.toml` 里 ahash 的登记理由相同：内部键、非攻击者可控，
//! 不需要 HashDoS 防护，要的是速度。

use ahash::{HashMap, HashMapExt};
use std::borrow::Borrow;
use std::hash::Hash;

/// 有界 LRU 映射：键 → 值，超容量时驱逐**访问序最旧**的键。
///
/// 线程安全性：**不是** `Sync` 的（与四处分处相同，调用方各自持锁或
/// 限定在单线程）。跨线程共享请在外面包 `Mutex`。
pub struct LruMap<K, V> {
    entries: HashMap<K, V>,
    /// 访问序（LRU）：最旧在前。与 `entries` 的键集保持一致。
    order: Vec<K>,
    capacity: usize,
}

impl<K: Eq + Hash + Clone, V> LruMap<K, V> {
    /// 构造缓存；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    /// 当前容量上限。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// **不触碰访问序**的读取（条件命中的第一步，见类型文档）。
    ///
    /// 形如 `HashMap::get`：`K = String` 时可直接传 `&str`。
    pub fn peek<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.get(key)
    }

    /// 读取；命中时把键挪到访问序尾部。
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        if self.entries.contains_key(key) {
            self.move_to_back(key);
        }
        self.entries.get(key)
    }

    /// 可变读取；命中时把键挪到访问序尾部。
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        if self.entries.contains_key(key) {
            self.move_to_back(key);
        }
        self.entries.get_mut(key)
    }

    /// 把键挪到访问序尾部（条件命中的第二步）。
    ///
    /// 调用方须先用 [`Self::peek`] 确认键存在（三处缓存的条件命中模式
    /// 正是如此）。键不在序中时是 no-op——两表不可能不同步：所有写路径
    /// 都经本类型，`entries` 与 `order` 的键集由结构保证一致。
    pub fn touch<Q>(&mut self, key: &Q)
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.move_to_back(key);
    }

    /// 把键移到访问序尾部；不在序中则不动（由 `insert` 负责补入新键）。
    ///
    /// 用「取出再压回」而不是「克隆一份压回」：`order` 里已有owned 键，
    /// 移动零分配，且不要求调用方能从 `&Q` 造出 `K`。
    fn move_to_back<Q>(&mut self, key: &Q)
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        if let Some(pos) = self.order.iter().position(|k| k.borrow() == key) {
            let k = self.order.remove(pos);
            self.order.push(k);
        }
    }

    /// 写入（达到容量且为新键时驱逐**最旧**的一个键）。
    pub fn insert(&mut self, key: K, value: V) {
        let full_and_new = self.entries.len() >= self.capacity && !self.entries.contains_key(&key);
        if full_and_new {
            self.evict_oldest();
        }
        // 覆盖已有键时也要刷新访问序；新键则补入序尾。
        match self.order.iter().position(|k| *k == key) {
            Some(pos) => {
                let k = self.order.remove(pos);
                self.order.push(k);
            }
            None => self.order.push(key.clone()),
        }
        self.entries.insert(key, value);
    }

    /// 摘除访问序最旧的一个键（两表同步）。
    ///
    /// 调用方须先确认「已满且要插入的是新键」——`order` 非空是 `entries`
    /// 非空的必要条件，`first()` 恒有值，`if let` 仅为防御。
    fn evict_oldest(&mut self) {
        if let Some(old) = self.order.first().cloned() {
            self.entries.remove(&old);
            self.order.remove(0);
        }
    }

    /// 失效单个键。
    pub fn invalidate<Q>(&mut self, key: &Q)
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.remove(key);
        self.order.retain(|k| k.borrow() != key);
    }

    /// 失效全部。
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
        self.order.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> u64 {
        n
    }

    /// 核心契约：驱逐的是**最旧未用**，不是任意键。
    #[test]
    fn evicts_least_recently_used_not_arbitrary() {
        let mut m = LruMap::new(2);
        m.insert(id(1), "a");
        m.insert(id(2), "b");
        // 触碰 1 → 访问序变为 [2, 1]，2 成为最旧。
        assert_eq!(m.get(&id(1)), Some(&"a"));
        m.insert(id(3), "c");
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&id(1)), Some(&"a"), "近期命中的不应被驱逐");
        assert_eq!(m.get(&id(2)), None, "最旧且未再访问的应被驱逐");
        assert_eq!(m.get(&id(3)), Some(&"c"));
    }

    /// 覆盖已有键**不驱逐**，且刷新访问序。
    #[test]
    fn overwrite_never_evicts_and_refreshes_order() {
        let mut m = LruMap::new(2);
        m.insert(id(1), "a");
        m.insert(id(2), "b");
        m.insert(id(1), "a2"); // 覆盖 1 → 序变为 [2, 1]
        m.insert(id(3), "c"); // 满 + 新键 → 驱逐最旧的 2
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&id(1)), Some(&"a2"), "覆盖后的新值必须在");
        assert_eq!(m.get(&id(2)), None, "被覆盖续命的 1 不应驱逐 2 之外者");
        assert_eq!(m.get(&id(3)), Some(&"c"));
    }

    /// `peek` 不续命：条件未命中的查询不得把键挪新。
    #[test]
    fn peek_does_not_touch_order() {
        let mut m = LruMap::new(2);
        m.insert(id(1), "a");
        m.insert(id(2), "b");
        let _ = m.peek(&id(1)); // 只看，不续命
        m.insert(id(3), "c"); // 最旧的 1 应被驱逐
        assert_eq!(m.get(&id(1)), None, "peek 不得续命");
        assert_eq!(m.get(&id(2)), Some(&"b"));
    }

    /// `touch` 与 `peek` 组合即「条件命中」：判定命中才续命。
    #[test]
    fn peek_then_touch_models_conditional_hit() {
        let mut m = LruMap::new(2);
        m.insert(id(1), 10u32);
        m.insert(id(2), 20);
        // 条件：值 > 15 才算命中（模拟「版本号一致」这类附带条件）。
        if m.peek(&id(1)).is_some_and(|v| *v > 15) {
            m.touch(&id(1));
        }
        m.insert(id(3), 30);
        assert_eq!(m.get(&id(1)), None, "条件未命中不得续命");
        assert_eq!(m.get(&id(2)), Some(&20));
    }

    /// 失效要把两表同步摘干净。
    ///
    /// 不同步的症状：`order` 里残留的键继续占用淘汰名额，后续插入即使
    /// 触发驱逐也删不到它，缓存实际容量会超出上限（`len() > capacity`）。
    #[test]
    fn invalidate_removes_from_order_too() {
        let mut m = LruMap::new(2);
        m.insert(id(1), "a");
        m.insert(id(2), "b");
        m.invalidate(&id(1));
        assert_eq!(m.get(&id(1)), None);
        assert_eq!(m.get(&id(2)), Some(&"b"));
        m.insert(id(3), "c");
        m.insert(id(4), "d"); // 两次插入各触发一次驱逐
        assert_eq!(m.len(), 2, "失效不同步会让缓存超出容量上限");
        assert_eq!(m.get(&id(3)), Some(&"c"));
        assert_eq!(m.get(&id(4)), Some(&"d"));
        assert_eq!(m.get(&id(1)), None);
        // 失效后再插入同键，不得留下「序里有但 entries 没有」的残影。
        m.insert(id(1), "a2");
        m.insert(id(5), "e");
        assert_eq!(m.len(), 2);
        m.invalidate_all();
        assert!(m.is_empty());
    }

    /// 容量下限为 1（0 按 1 处理）。
    #[test]
    fn capacity_floor_is_one() {
        let mut m = LruMap::new(0);
        assert_eq!(m.capacity(), 1);
        m.insert(id(1), "a");
        m.insert(id(2), "b");
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(&id(2)), Some(&"b"));
    }
}
