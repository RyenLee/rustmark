//! 符号 / Emoji 面板状态（S6-11f，对齐原型 A-26 `SymbolPicker.vue`）。
//!
//! # 静态表照搬原型，但**刻意不加**原型里的一个分组
//!
//! 原型的 `SNIPPETS` 里有 `{{< highlight rust >}}`——那是 **Hugo 短代码**，
//! 本项目导出的是 HTML / Markdown / PDF，产物里不会出现任何 Hugo 语法，
//! 插进去只会得到一段永远不会被渲染的文本。
//! **插入一个本应用永远不消费的东西，比不提供更糟**：用户看不出它无效。
//!
//! # 搜索用子序列匹配
//!
//! 与快速打开同一套理由（见 `quick_open_ui_state`），且这里更需要：
//! 符号的中文名常有 3–4 个字（「实心星」「无序列表」），
//! 子串匹配下用户得记得词序。
//!
//! # 模块结构
//!
//! facade 保留面板状态 `SymbolUiState`、`search` 过滤与内联测试；
//! 分组模型与表情 / 符号静态目录在 `catalog`，Markdown 片段表在 `snippets`。

mod catalog;
mod snippets;

use crate::presentation::state::quick_open_ui_state::fuzzy_match;

pub use catalog::{SymbolGroup, SymbolItem, items_of};

/// 符号面板状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolUiState {
    /// 当前分组。
    pub group: SymbolGroup,
    /// 搜索词。
    pub query: String,
    /// 选中项在该分组**可见列表**中的下标。
    pub selected: usize,
    /// 打开时的搜索词（判据：搜索是否改过）。
    opened_query: Option<String>,
}

impl Default for SymbolUiState {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolUiState {
    /// 构造默认状态（第一个分组、无搜索）。
    pub fn new() -> Self {
        Self {
            group: SymbolGroup::Emoji,
            query: String::new(),
            selected: 0,
            opened_query: Some(String::new()),
        }
    }

    /// 打开面板：重置到第一个分组、清搜索、夹紧选中项。
    pub fn open(&mut self) {
        self.group = SymbolGroup::Emoji;
        self.query.clear();
        self.selected = 0;
        self.opened_query = Some(String::new());
    }

    /// 切换分组并把选中项夹回 0。
    ///
    /// 不夹回 0 会让用户从「符号」第 30 项切到「片段」第 2 项——
    /// 两个分组长度差 10 倍，视觉上像是随机的。
    pub fn set_group(&mut self, group: SymbolGroup) {
        if self.group != group {
            self.group = group;
            self.selected = 0;
        }
    }

    /// 设置搜索词。
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
    }

    /// 搜索是否改过。
    #[cfg(test)]
    pub(crate) fn query_changed(&self) -> bool {
        self.opened_query.as_deref() != Some(self.query.as_str())
    }

    /// 当前分组在搜索下的可见条目。
    pub fn visible(&self) -> Vec<SymbolItem> {
        search(items_of(self.group), &self.query)
    }

    /// 移动选中项（`delta = ±1`，越界环绕）。
    pub fn move_selection(&mut self, delta: isize, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }
        let n = total as isize;
        let next = (self.selected as isize + delta) % n;
        self.selected = next.rem_euclid(n) as usize;
    }

    /// 把选中项夹进候选范围。
    pub fn clamp(&mut self, total: usize) {
        if total == 0 {
            self.selected = 0;
        } else if self.selected >= total {
            self.selected = total - 1;
        }
    }
}

/// 在分组的条目里按搜索词过滤。
///
/// 匹配 `label` 的**任一关键词**（按空格拆），而非整串。
/// 原型的 `label` 就是空格分隔的多关键词（「实心星 star」），
/// 整串匹配会让 `star` 命中不了「实心星 star」这一条——因为
/// 中间夹了别的词。
pub fn search(items: &[SymbolItem], query: &str) -> Vec<SymbolItem> {
    let q = query.trim();
    if q.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| {
            item.label
                .split_whitespace()
                .any(|keyword| fuzzy_match(keyword, q).is_some())
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_has_items_and_a_label() {
        for group in SymbolGroup::ALL {
            assert!(!items_of(*group).is_empty(), "{:?} 不应是空分组", group);
            assert!(!group.label().is_empty());
        }
    }

    #[test]
    fn every_item_has_distinct_display_data() {
        for group in SymbolGroup::ALL {
            for item in items_of(*group) {
                assert!(!item.text.is_empty(), "{:?} 有空插入文本", group);
                assert!(!item.glyph.is_empty(), "{:?} 有空字形", group);
                assert!(!item.label.is_empty(), "{:?} 有空名称", group);
            }
        }
    }

    #[test]
    fn no_snippet_is_a_foreign_template_shortcode() {
        // 本项目不消费 Hugo 短代码，插进去只会得到永不渲染的文本。
        for item in items_of(SymbolGroup::Snippet) {
            assert!(
                !item.text.contains("{{<"),
                "片段不应含本应用不消费的短代码：{}",
                item.text
            );
        }
    }

    #[test]
    fn empty_query_returns_the_whole_group() {
        let out = search(items_of(SymbolGroup::Snippet), "");
        assert_eq!(out.len(), items_of(SymbolGroup::Snippet).len());
    }

    #[test]
    fn any_keyword_can_match() {
        // 「实心星 star」：搜中文名与搜英文名都应命中。
        let out = search(items_of(SymbolGroup::Symbol), "实心星");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "★");
        let out = search(items_of(SymbolGroup::Symbol), "star");
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(search(items_of(SymbolGroup::Emoji), "zzzz").is_empty());
    }

    #[test]
    fn search_does_not_leak_across_groups() {
        // 「表格」只在片段组；符号组与表情组都不该出现它。
        assert!(!search(items_of(SymbolGroup::Snippet), "表格").is_empty());
        assert!(search(items_of(SymbolGroup::Symbol), "表格").is_empty());
        assert!(search(items_of(SymbolGroup::Emoji), "表格").is_empty());
    }

    #[test]
    fn switching_group_resets_selection() {
        let mut state = SymbolUiState::new();
        state.selected = 15;
        state.set_group(SymbolGroup::Symbol);
        assert_eq!(state.selected, 0);
        // 同组重复点击不应重置（点「表情」时不该跳回第一项）。
        state.selected = 3;
        state.set_group(SymbolGroup::Symbol);
        assert_eq!(state.selected, 3);
    }

    #[test]
    fn opening_resets_everything() {
        let mut state = SymbolUiState::new();
        state.set_group(SymbolGroup::Snippet);
        state.set_query("表格");
        state.selected = 2;
        state.open();
        assert_eq!(state.group, SymbolGroup::Emoji);
        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
        assert!(!state.query_changed());
    }

    #[test]
    fn selection_wraps_and_clamps() {
        let mut state = SymbolUiState::new();
        state.move_selection(-1, 5);
        assert_eq!(state.selected, 4);
        state.move_selection(1, 5);
        assert_eq!(state.selected, 0);
        state.selected = 9;
        state.clamp(5);
        assert_eq!(state.selected, 4);
        state.move_selection(1, 0);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn narrowing_the_search_pulls_selection_back() {
        let mut state = SymbolUiState::new();
        state.set_group(SymbolGroup::Snippet);
        state.selected = 15;
        // 切到「片段」组（片段在默认的「表情」组里搜不到），
        // 再用能命中片段的词收窄。
        state.set_query("代码块");
        let total = state.visible().len();
        assert!(total > 0, "前提：这一组应有命中");
        state.clamp(total);
        assert!(state.selected < total);
    }
}
