//! 快速打开浮层状态（S6-11f，对齐原型 A-25 `QuickOpen.vue`）。
//!
//! # 候选来源：工作区条目 + 最近文件，而不是重新扫盘
//!
//! 文件树已经装载了一份 [`WorkspaceEntries`] 快照（S6-11d），
//! 这里直接投影成候选列表。若在浮层打开时重新 `scan_tree`，
//! 一是每开一次面板就一次递归 `read_dir`，二是「文件树里有的文件
//! 快速打开搜不到」——两个数据源必然漂移。
//!
//! # 匹配是**子序列**而不是子串
//!
//! 原型的 `QuickOpen.vue` 用 `includes`（子串），但它那份候选列表
//! 只有文件名。子串匹配下输入 `sd` 命中不了 `src-docs/readme.md`
//! ——用户记得的是「大概有个 sd 的文件」，而精确子串要求他记得顺序。
//! 这里改用子序列，代价是命中略宽，收益是「记得几个字母就能找到」。

use std::collections::HashSet;

use rustmark_core::application::dto::RecentFileView;
use rustmark_core::shared::path::file_name_of;

/// 候选上限。
///
/// 与原型一致（`.slice(0, 12)`）。再多就得滚动，而快速打开的核心
/// 是「不打字就选」——12 条基本一屏放得下。
pub const MAX_CANDIDATES: usize = 12;

/// 一条候选（工作区文件或最近文件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickOpenItem {
    /// 完整路径（打开时用）。
    pub path: String,
    /// 文件名（显示与匹配用）。
    pub name: String,
    /// 来源分组（显示用）。
    pub group: QuickOpenGroup,
}

/// 候选来源分组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuickOpenGroup {
    /// 工作区文件。
    Workspace,
    /// 最近使用。
    Recent,
}

impl QuickOpenGroup {
    /// 分组显示名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Workspace => "工作区",
            Self::Recent => "最近使用",
        }
    }
}

/// 快速打开状态。
#[derive(Clone, Debug, Default)]
pub struct QuickOpenUiState {
    /// 查询词。
    pub query: String,
    /// 当前高亮候选的下标。
    pub selected: usize,
}

impl QuickOpenUiState {
    /// 构造空状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 打开面板：清查询与选中项。
    ///
    /// 复用上一次面板的查询会让人以为「这些结果是我要的」——
    /// 面板每次打开的语境不同（刚打开文件夹 vs 刚保存完）。
    pub fn open(&mut self) {
        self.query.clear();
        self.selected = 0;
    }

    /// 设置查询词，并把选中项收回 0。
    ///
    /// 收回 0 而不是「夹到末项」：候选集变了，末项在新集合里
    /// 可能指向完全无关的文件。用户看到自己刚输入的内容对应的
    /// 第一项被选中，符合「我在筛东西」的直觉（VS Code 同款行为）。
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        self.selected = 0;
    }

    /// 移动选中项（`delta = ±1`，越界则环绕）。
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
    ///
    /// 必须在**每次**重算候选后调用：候选变少时旧下标会越界，
    /// 界面会「选中了一个不存在的行」（原型 `QuickOpen.vue` 里的
    /// `watch` 就是干这个的，egui 侧没有 `watch`，故显式夹）。
    pub fn clamp(&mut self, total: usize) {
        if total == 0 {
            self.selected = 0;
        } else if self.selected >= total {
            self.selected = total - 1;
        }
    }
}

/// 子序列匹配：返回命中字符在 `text` 中的**字符**索引。
///
/// 按字符而非字节：`sd` 命中 `中文sd.md` 时，按字节切会把「sd」
/// 落在多字节字符中间，高亮出来的碎片既不是完整的字也不是完整的字母。
pub fn fuzzy_match(text: &str, query: &str) -> Option<Vec<usize>> {
    let needle: Vec<char> = query.trim().to_lowercase().chars().collect();
    if needle.is_empty() {
        return Some(Vec::new());
    }
    let haystack: Vec<char> = text.to_lowercase().chars().collect();
    let mut cursor = 0usize;
    let mut hits = Vec::with_capacity(needle.len());
    for ch in needle {
        let found = haystack.iter().skip(cursor).position(|c| *c == ch)?;
        hits.push(cursor + found);
        cursor = cursor + found + 1;
    }
    Some(hits)
}

/// 由工作区条目构造候选（目录不作为候选）。
///
/// 去重：工作区里已存在的文件不该在「最近使用」里再出现一次——
/// 同一路径出现两次会让用户以为是两个不同的文件。
///
/// `seen` 用 `HashSet<&str>` 而不是 `Vec<String>`：候选量在「工作区
/// 文件数」量级（几百到几千），线性 contains 每项还要 `.to_string()`
/// 分配一次； borrowed 哈希集合零分配、O(1)（C-3）。
pub fn candidates_from_workspace(
    root: &str,
    items: &[(String, bool)],
    recent: &[RecentFileView],
    limit: usize,
) -> Vec<QuickOpenItem> {
    let mut out: Vec<QuickOpenItem> = Vec::with_capacity(limit.max(16));
    let mut seen: HashSet<&str> = HashSet::new();
    for (path, is_dir) in items {
        if *is_dir {
            continue;
        }
        let name = match file_name_of(path) {
            "" => path,
            other => other,
        }
        .to_string();
        out.push(QuickOpenItem {
            path: path.clone(),
            name,
            group: QuickOpenGroup::Workspace,
        });
        seen.insert(path.as_str());
    }
    // 工作区未加载时（`root` 为空）最近文件仍是唯一可用来源。
    if root.is_empty() {
        for item in recent {
            if seen.contains(item.path.as_str()) {
                continue;
            }
            let name = item.path.file_name();
            out.push(QuickOpenItem {
                path: item.path.as_str().to_string(),
                // `file_name()` 返回末段，但路径以分隔符结尾时它是空串——
                // 直接用会让候选列表里出现一行「没有名字的文件」。
                name: if name.is_empty() {
                    item.path.as_str().to_string()
                } else {
                    name.to_string()
                },
                group: QuickOpenGroup::Recent,
            });
            seen.insert(item.path.as_str());
        }
    }
    out.truncate(limit);
    out
}

/// 按查询过滤并排序候选。
///
/// 排序规则：**完全等于查询的排最前**，其余保持来源顺序。
/// 不做更复杂的打分——候选上限只有 12 条，用户是在「认文件名」
/// 而不是「在 5000 个文件里检索」，多一层排序只会带来意外顺序。
pub fn filter_candidates(items: &[QuickOpenItem], query: &str, limit: usize) -> Vec<QuickOpenItem> {
    let mut exact: Vec<QuickOpenItem> = Vec::new();
    let mut fuzzy: Vec<QuickOpenItem> = Vec::new();
    for item in items {
        let name = item.name.to_lowercase();
        if !query.trim().is_empty() {
            if name == query.trim().to_lowercase() {
                exact.push(item.clone());
                continue;
            }
            if fuzzy_match(&item.name, query).is_none() {
                continue;
            }
        }
        fuzzy.push(item.clone());
    }
    exact.extend(fuzzy);
    exact.truncate(limit);
    exact
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(path: &str) -> (String, bool) {
        (path.to_string(), false)
    }

    fn recent(path: &str) -> RecentFileView {
        RecentFileView {
            path: rustmark_core::shared::path::FilePath::new(path).expect("测试路径应合法"),
            last_opened: 0,
        }
    }

    #[test]
    fn workspace_candidates_exclude_directories() {
        let items = vec![ws("/root/a.md"), ("/root/d".to_string(), true)];
        let out = candidates_from_workspace("/root", &items, &[], 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "a.md");
        assert_eq!(out[0].group, QuickOpenGroup::Workspace);
    }

    #[test]
    fn recent_files_are_used_only_without_a_workspace() {
        let items = vec![ws("/root/a.md")];
        let recents = vec![recent("/root/a.md"), recent("/other/b.md")];
        // 有工作区：最近文件**不**参与（工作区才是当前语境）。
        let with_ws = candidates_from_workspace("/root", &items, &recents, 10);
        assert_eq!(with_ws.len(), 1);
        // 无工作区：最近文件是唯一来源。
        let without_ws = candidates_from_workspace("", &[], &recents, 10);
        assert_eq!(without_ws.len(), 2);
        assert!(without_ws.iter().all(|i| i.group == QuickOpenGroup::Recent));
    }

    #[test]
    fn candidates_respect_the_limit() {
        let items: Vec<(String, bool)> = (0..30).map(|i| ws(&format!("/root/{i}.md"))).collect();
        let out = candidates_from_workspace("/root", &items, &[], MAX_CANDIDATES);
        assert_eq!(out.len(), MAX_CANDIDATES);
    }

    #[test]
    fn empty_query_returns_every_candidate() {
        let items = vec![ws("/root/a.md"), ws("/root/b.md")];
        let out = candidates_from_workspace("/root", &items, &[], 10);
        let filtered = filter_candidates(&out, "", 10);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn subsequence_matches_out_of_order_characters() {
        let items = vec![ws("/root/a-s-d-file.md")];
        let out = candidates_from_workspace("/root", &items, &[], 10);
        // 子串匹配下 "sd" 命中不了；子序列可以。
        assert_eq!(filter_candidates(&out, "sd", 10).len(), 1);
    }

    #[test]
    fn exact_match_is_ranked_first() {
        let items = vec![ws("/root/a-s-d.md"), ws("/root/abd.md")];
        let out = candidates_from_workspace("/root", &items, &[], 10);
        let filtered = filter_candidates(&out, "abd", 10);
        assert_eq!(filtered[0].name, "abd.md");
    }

    #[test]
    fn fuzzy_hits_are_reported_in_character_coordinates() {
        let hits = fuzzy_match("中文sd.md", "sd").expect("应命中");
        assert_eq!(hits, vec![2, 3]);
    }

    #[test]
    fn selection_wraps_in_both_directions() {
        let mut state = QuickOpenUiState::new();
        state.move_selection(-1, 3);
        assert_eq!(state.selected, 2, "从 0 往上应绕到末尾");
        state.move_selection(1, 3);
        assert_eq!(state.selected, 0);
        state.move_selection(1, 0);
        assert_eq!(state.selected, 0, "空列表不应 panic");
    }

    #[test]
    fn clamp_pulls_selection_back_into_range() {
        let mut state = QuickOpenUiState::new();
        state.selected = 9;
        state.clamp(3);
        assert_eq!(state.selected, 2);
        state.clamp(0);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn opening_resets_query_and_selection() {
        let mut state = QuickOpenUiState::new();
        state.set_query("readme");
        state.selected = 4;
        state.open();
        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn narrowing_the_query_resets_the_selection() {
        let mut state = QuickOpenUiState::new();
        state.selected = 8;
        state.set_query("readme");
        assert_eq!(state.selected, 0, "筛选后应高亮第一项");
        // 删回空串也仍回到 0：候选集变了，旧的 8 已无意义。
        state.set_query("");
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn clamping_pulls_selection_back_after_candidates_shrink() {
        let mut state = QuickOpenUiState::new();
        state.selected = 9;
        state.clamp(3);
        assert_eq!(state.selected, 2);
        state.clamp(0);
        assert_eq!(state.selected, 0);
    }
}
