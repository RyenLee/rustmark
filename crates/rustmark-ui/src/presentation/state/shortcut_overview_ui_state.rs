//! 快捷键一览浮层状态（S6-11f）。
//!
//! # 三处显示必须读**同一份**生效键位（原型缺陷 B-08 根治）
//!
//! 菜单、命令面板、快捷键一览三处都调 `ShortcutRegistry::display_for`，
//! 于是用户在设置里改键后三处**同时**变化。这里刻意**不**构造自己的
//! 键位快照——快照会在改键后变成过期数据，而过期的键位显示比
//! 「没显示」更糟：用户按着说明里的键位发现没反应，会以为是快捷键系统坏了。
//!
//! 本模块只做**分组与筛选**这类纯展示逻辑，数据一律从注册表现取。

use crate::presentation::shortcuts::registry::{ActionEntry, MenuGroup};

/// 一行展示项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutRow {
    /// 动作。
    pub action: crate::presentation::shortcuts::registry::UiAction,
    /// 中文显示名。
    pub label: &'static str,
    /// 当前生效键位（空串 = 未绑定）。
    pub combo: String,
}

/// 一组展示项（`MenuGroup` 标题 + 该组动作）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutSection {
    /// 分组。
    pub group: MenuGroup,
    /// 该组动作（按枚举序；已按过滤词筛选）。
    pub rows: Vec<ShortcutRow>,
}

impl ShortcutSection {
    /// 该组是否一个动作都没有。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 快捷键一览浮层状态。
#[derive(Clone, Debug, Default)]
pub struct ShortcutOverviewUiState {
    /// 过滤词（按显示名与关键字筛）。
    pub query: String,
}

impl ShortcutOverviewUiState {
    /// 构造空状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 打开面板：清空过滤词。
    ///
    /// 保留上次的过滤词会让用户看到「我搜的明明在这里，怎么没有了」——
    /// 而他从没在这个面板里搜过。
    pub fn open(&mut self) {
        self.query.clear();
    }

    /// 当前过滤词（已去空白）。
    pub fn query(&self) -> &str {
        self.query.trim()
    }
}

/// 按分组构造展示项，并按过滤词筛选。
///
/// `entries` 由调用方从注册表现取（`ActionTable::group`），本函数
/// **只负责加过滤**。不自己遍历 `UiAction::ALL` 是刻意的：
/// 那会绕过注册表，于是「改键后显示同步」这条保证就断了。
///
/// 空结果的分组**保留**（`rows` 为空）而不是从列表里删掉：
/// 删掉会让面板高度随过滤词忽变忽无，用户以为面板坏了。
pub fn sections(groups: Vec<(MenuGroup, Vec<ActionEntry>)>, query: &str) -> Vec<ShortcutSection> {
    let q = query.trim();
    groups
        .into_iter()
        .map(|(group, entries)| ShortcutSection {
            group,
            rows: entries
                .into_iter()
                .filter(|entry| matches_query(entry.label, entry.action.keywords(), q))
                .map(|entry| ShortcutRow {
                    action: entry.action,
                    label: entry.label,
                    combo: entry.combo,
                })
                .collect(),
        })
        .collect()
}

/// 一行是否命中过滤词。
///
/// 匹配中文显示名与英文关键字两侧，理由同命令面板：
/// 用户记得的往往是「加粗 bold」里的任一半。
pub(crate) fn matches_query(label: &str, keywords: &str, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let q = query.trim().to_lowercase();
    if contains_subsequence(&label.to_lowercase(), &q) {
        return true;
    }
    keywords
        .split_whitespace()
        .any(|keyword| contains_subsequence(&keyword.to_lowercase(), &q))
}

/// 子序列包含（`str::contains` 的子序列版）。
///
/// 刻意不用子串：用户记得的常常是「sd」这种「大概有这么两个字母」，
/// 子串要求他记得顺序，而文件名 / 动作名里的顺序无从记忆。
fn contains_subsequence(haystack: &str, needle: &str) -> bool {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return true;
    }
    let mut chars = haystack.chars();
    needle.into_iter().all(|want| chars.any(|ch| ch == want))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::shortcuts::registry::{ShortcutRegistry, UiAction};

    fn groups_of(reg: &ShortcutRegistry) -> Vec<(MenuGroup, Vec<ActionEntry>)> {
        let table = reg.table();
        MenuGroup::ALL
            .iter()
            .map(|g| (*g, table.group(*g)))
            .collect()
    }

    #[test]
    fn every_group_shows_up_under_the_empty_query() {
        let reg = ShortcutRegistry::with_defaults();
        let out = sections(groups_of(&reg), "");
        assert_eq!(out.len(), MenuGroup::ALL.len());
        let total: usize = out.iter().map(|s| s.rows.len()).sum();
        assert_eq!(total, UiAction::ALL.len(), "一览必须覆盖全部动作");
    }

    #[test]
    fn query_filters_by_chinese_label() {
        let reg = ShortcutRegistry::with_defaults();
        let out = sections(groups_of(&reg), "加粗");
        let rows: Vec<&ShortcutRow> = out.iter().flat_map(|s| s.rows.iter()).collect();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| r.label.contains("加粗")));
    }

    #[test]
    fn query_matches_english_keywords_too() {
        let reg = ShortcutRegistry::with_defaults();
        let out = sections(groups_of(&reg), "bold");
        let rows: Vec<&ShortcutRow> = out.iter().flat_map(|s| s.rows.iter()).collect();
        assert!(rows.iter().any(|r| r.label == "加粗"));
    }

    #[test]
    fn query_is_a_subsequence_not_a_substring() {
        assert!(contains_subsequence("bold", "bd"));
        assert!(!contains_subsequence("bold", "bb"));
        assert!(contains_subsequence("加粗", "加"));
    }

    #[test]
    fn empty_sections_stay_present() {
        let reg = ShortcutRegistry::with_defaults();
        let out = sections(groups_of(&reg), "zzzz");
        assert_eq!(out.len(), MenuGroup::ALL.len(), "分组不能消失——高度会忽变");
        assert!(out.iter().all(ShortcutSection::is_empty));
    }

    #[test]
    fn combos_come_from_the_live_registry() {
        // 改键后本模块读到的必须是新值——这是原型缺陷 B-08 的验收点。
        let mut reg = ShortcutRegistry::with_defaults();
        let before = reg.display_for(UiAction::Save);
        let combo =
            crate::presentation::shortcuts::registry::normalize("Ctrl+Alt+K").expect("应能解析");
        reg.rebind(UiAction::Save, combo);
        let out = sections(groups_of(&reg), "保存");
        let row = out
            .iter()
            .flat_map(|s| s.rows.iter())
            .find(|r| r.action == UiAction::Save)
            .expect("保存动作应在文件组");
        assert_ne!(row.combo, before);
        assert_eq!(row.combo, "Ctrl+Alt+K");
    }

    #[test]
    fn unbound_actions_keep_an_empty_combo_verbatim() {
        // 状态层**不改写**空键位：视图据它显示「未绑定」。
        // 若状态层擅自填一个默认键位，用户解除绑定后会看到
        // 「设置里明明没绑，一览里却写着 Ctrl+S」。
        let reg = ShortcutRegistry::with_defaults();
        let out = sections(groups_of(&reg), "");
        let rows: Vec<&ShortcutRow> = out.iter().flat_map(|s| s.rows.iter()).collect();
        let unbound: Vec<&str> = rows
            .iter()
            .filter(|r| r.combo.is_empty())
            .map(|r| r.label)
            .collect();
        assert!(
            unbound.contains(&"快捷键一览"),
            "该动作默认无键位，实际未绑定列表：{unbound:?}"
        );
        assert!(rows.iter().any(|r| !r.combo.is_empty()), "多数动作应有键位");
    }

    #[test]
    fn opening_clears_the_filter() {
        let mut state = ShortcutOverviewUiState::new();
        state.query = "加粗".to_string();
        state.open();
        assert!(state.query().is_empty());
    }
}
