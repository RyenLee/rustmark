//! 公式 / 图表排版结果的帧间记忆化（缺陷 F-4）。
//!
//! # 问题
//!
//! `draw_math` / `draw_diagram` 此前**每帧**都调 `math::layout` /
//! `mermaid::layout`：RenderCache 只缓存到 `BlockView` 层（解析产物），
//! **几何排版每帧重做**——含公式 / 图表的文档滚动时，每帧付全额
//! 「解析 + 逐标签度量」成本，而这些都是纯函数式的重复劳动。
//!
//! # 方案
//!
//! 按 (内容, 字号, 展示态, 图集令牌) 记忆化排版结果，容量 32 的 LRU。
//! 失效哲学与编辑区两层缓存（`theme::mapper::galley_cache`）同源：
//!
//! - 主题**颜色**不影响几何（`paint` 时才取色），故不进键；
//! - 字号 / 展示态是排版输入，必须进键；
//! - **图集令牌**必须进键（V11-17：egui 重建字体图集后，旧几何里字形的
//!   `uv_rect` 全部失效；缓存命中会拿旧坐标采样新图集 → 中文渲染成错码）。
//!
//! # 为什么键里存全文而不存哈希
//!
//! 同 WHOLE_CACHE 的纪律：哈希碰撞会让「新内容误命中旧几何」→ 画错图。
//! 公式 / 图表源码都是几十到几百字节，逐字节比较的成本相对排版本身可忽略。
//!
//! # 为什么容量 32 的 LRU 而不是单槽
//!
//! 单槽（WHOLE_CACHE 的形态）面对「一屏多个公式」会每帧在块间来回抖动，
//! 命中率趋零。文档里公式 / 图表总数通常个位数到几十，32 条足够覆盖
//! 一屏 + 滚动邻近区，且内存有界（同 highlight / spell / render 的容量哲学）。

use std::sync::Arc;

use crate::presentation::views::{math, mermaid};

/// 记忆化容量：一屏同时可见的公式 + 图表通常远小于此；超出按 LRU 淘汰。
const CAPACITY: usize = 32;

/// 排版结果条目（公式）。
pub(super) type MathEntry = Arc<Result<math::MathLayout, math::MathError>>;

/// 排版结果条目（图表）。
pub(super) type MermaidEntry = Arc<Result<mermaid::MermaidLayout, mermaid::MermaidError>>;

/// 缓存键：内容 + 排版输入 + 图集令牌。
#[derive(Clone, PartialEq)]
struct Key {
    /// 块源码（公式 LaTeX / Mermaid 源码）。
    content: String,
    /// 字号 ×100 取整——浮点直接进键会因抖动造成假未命中。
    size_centi: u32,
    /// 公式专用：展示态（块级 / 行内）；Mermaid 恒 `false`。
    display: bool,
    /// 字体图集令牌（egui 重建字体后必须失效，见模块文档）。
    atlas: u64,
}

impl Key {
    fn new(content: &str, size: f32, display: bool, atlas: u64) -> Self {
        Self {
            content: content.to_string(),
            size_centi: (size * 100.0).round() as u32,
            display,
            atlas,
        }
    }
}

/// 小型 LRU：Vec 线性扫描 + 命中前移 + 末尾淘汰。
///
/// 32 条规模下比哈希表更简单，也顺带满足「键存全文不存哈希」的纪律。
struct Lru<V> {
    entries: Vec<(Key, V)>,
}

impl<V> Lru<V> {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// 命中则前移并返回既有值的克隆（`V` 为 `Arc`，克隆是指针）。
    fn get(&mut self, key: &Key) -> Option<V>
    where
        V: Clone,
    {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(pos);
        let value = entry.1.clone();
        self.entries.insert(0, entry);
        Some(value)
    }

    /// 插入 / 更新并前移；超容量时淘汰最久未用者（末尾）。
    fn insert(&mut self, key: Key, value: V) {
        if let Some(pos) = self.entries.iter().position(|(k, _)| *k == key) {
            self.entries.remove(pos);
        }
        self.entries.insert(0, (key, value));
        self.entries.truncate(CAPACITY);
    }
}

thread_local! {
    static MATH_CACHE: std::cell::RefCell<Lru<MathEntry>> = const { std::cell::RefCell::new(Lru::new()) };
    static MERMAID_CACHE: std::cell::RefCell<Lru<MermaidEntry>> = const { std::cell::RefCell::new(Lru::new()) };
    /// 实际排版调用次数（= 缓存未命中次数），测试与性能断言用。
    static MATH_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static MERMAID_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// 取公式排版结果：命中缓存则**既不重新排版也不调用 `measure`**。
pub(super) fn math_layout(
    content: &str,
    size: f32,
    display: bool,
    atlas: u64,
    measure: math::Measure<'_>,
) -> MathEntry {
    let key = Key::new(content, size, display, atlas);
    MATH_CACHE.with(|c| {
        if let Some(hit) = c.borrow_mut().get(&key) {
            return hit;
        }
        MATH_CALLS.with(|n| n.set(n.get() + 1));
        let entry = Arc::new(math::layout(content, size, display, measure));
        c.borrow_mut().insert(key, Arc::clone(&entry));
        entry
    })
}

/// 取图表排版结果：命中缓存则**既不重新排版也不调用 `measure`**。
pub(super) fn mermaid_layout(
    content: &str,
    size: f32,
    atlas: u64,
    measure: mermaid::Measure<'_>,
) -> MermaidEntry {
    let key = Key::new(content, size, false, atlas);
    MERMAID_CACHE.with(|c| {
        if let Some(hit) = c.borrow_mut().get(&key) {
            return hit;
        }
        MERMAID_CALLS.with(|n| n.set(n.get() + 1));
        let entry = Arc::new(mermaid::layout(content, size, measure));
        c.borrow_mut().insert(key, Arc::clone(&entry));
        entry
    })
}

/// 公式 / 图表的实际排版调用次数（测试断言「二次命中不重新排版」用）。
#[cfg(test)]
pub(super) fn layout_call_counts() -> (u64, u64) {
    (
        MATH_CALLS.with(|n| n.get()),
        MERMAID_CALLS.with(|n| n.get()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> (std::rc::Rc<std::cell::Cell<u64>>, impl Fn(&str, f32) -> f32) {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0_u64));
        let counter = calls.clone();
        let measure = move |text: &str, size: f32| {
            counter.set(counter.get() + 1);
            // 伪等宽：每字符 0.5 em。
            text.chars().count() as f32 * size * 0.5
        };
        (calls, measure)
    }

    #[test]
    fn same_input_second_call_skips_layout_and_measure() {
        let (calls, measure) = probe();
        let first = math_layout("x^2", 14.0, false, 1, &measure);
        let after_first = calls.get();
        let second = math_layout("x^2", 14.0, false, 1, &measure);
        assert!(after_first > 0, "首次必须真的排版");
        assert_eq!(calls.get(), after_first, "二次调用不得再进度量");
        assert!(
            Arc::ptr_eq(&first, &second),
            "二次调用必须返回同一条缓存条目"
        );
    }

    #[test]
    fn different_size_or_display_or_atlas_are_separate_entries() {
        let (_calls, measure) = probe();
        let base = math_layout("a+b", 14.0, false, 1, &measure);
        math_layout("a+b", 14.0, false, 1, &measure); // 全同 → 命中
        assert_eq!(layout_call_counts().0, 1, "全同输入只排一次");
        // 字号不同 → 重新排版。
        math_layout("a+b", 16.0, false, 1, &measure);
        assert_eq!(layout_call_counts().0, 2, "字号是排版输入，必须分开");
        // 展示态不同 → 重新排版。
        math_layout("a+b", 14.0, true, 1, &measure);
        assert_eq!(layout_call_counts().0, 3, "展示态影响排版，必须分开");
        // 图集令牌不同（egui 重建字体）→ 重新排版。
        math_layout("a+b", 14.0, false, 2, &measure);
        assert_eq!(layout_call_counts().0, 4, "图集重建后必须失效重排");
        assert!(base.is_ok());
    }

    #[test]
    fn cache_is_bounded_and_evicts_least_recently_used() {
        let (_calls, measure) = probe();
        // 灌满 32 + 1 条不同内容。
        for i in 0..=CAPACITY {
            let _ = math_layout(&format!("x{i}"), 14.0, false, 7, &measure);
        }
        // 第 0 条应已被淘汰 → 重新排版；其余仍在 → 命中。
        let before = layout_call_counts().0;
        let _ = math_layout("x0", 14.0, false, 7, &measure);
        assert_eq!(
            layout_call_counts().0,
            before + 1,
            "超出容量后最久未用条目必须被淘汰"
        );
        let before = layout_call_counts().0;
        let _ = math_layout(&format!("x{CAPACITY}"), 14.0, false, 7, &measure);
        assert_eq!(layout_call_counts().0, before, "刚用过的条目不得被淘汰");
    }

    #[test]
    fn mermaid_layout_is_memoized_too() {
        let (calls, measure) = probe();
        let first = mermaid_layout("graph TD\n  A-->B", 14.0, 3, &measure);
        let after_first = calls.get();
        let second = mermaid_layout("graph TD\n  A-->B", 14.0, 3, &measure);
        assert!(after_first > 0);
        assert_eq!(calls.get(), after_first, "图表同样不得重复排版");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(first.is_ok(), "合法流程图应排版成功");
    }
}
