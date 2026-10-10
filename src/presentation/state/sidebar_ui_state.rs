//! 侧边栏 UI 状态（文件树展开、全局搜索输入、大纲）。
//!
//! 全部是**视图状态**：展开到哪一层、搜索框里打了什么字。
//! 文件列表本身来自 `WorkspaceService::scan_tree`（DTO），不在此缓存——
//! 缓存在这里就会与磁盘状态不一致（外部改了文件、界面还显示旧的）。
//!
//! 唯一的例外是 [`SidebarUiState::tree_rows`]：它把 DTO 的**扁平**条目列表
//! 折成带缩进层次的行。这不是缓存业务数据，而是同一份数据的**投影**——
//! 每次调用都以传入的条目为准，条目一变结果立刻变。
//! 折行本身是纯函数，单独放在 [`tree_rows`] 里以便直接单测。

/// 全局搜索选项（对齐原型 `searchOptions`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SearchOptions {
    /// 区分大小写。
    pub case_sensitive: bool,
    /// 正则表达式。
    pub regex: bool,
    /// 全字匹配。
    pub whole_word: bool,
}

impl SearchOptions {
    /// 是否处于「默认」（三项全关）——UI 据此决定是否显示高亮徽标。
    pub const fn is_plain(&self) -> bool {
        !self.case_sensitive && !self.regex && !self.whole_word
    }

    /// 活跃选项数（显示成 `Aa` / `.*` 之类的角标）。
    pub const fn active_count(&self) -> usize {
        self.case_sensitive as usize + self.regex as usize + self.whole_word as usize
    }

    /// 取反某一项（三个开关共用一条路径，避免三处各写一遍）。
    pub const fn toggled(&self, which: SearchToggle) -> Self {
        match which {
            SearchToggle::CaseSensitive => Self {
                case_sensitive: !self.case_sensitive,
                ..*self
            },
            SearchToggle::Regex => Self {
                regex: !self.regex,
                ..*self
            },
            SearchToggle::WholeWord => Self {
                whole_word: !self.whole_word,
                ..*self
            },
        }
    }
}

/// 三个搜索开关（供 `toggled` 按值索引，避免字符串匹配）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SearchToggle {
    /// 区分大小写。
    CaseSensitive,
    /// 正则。
    Regex,
    /// 全字匹配。
    WholeWord,
}

/// 三个开关的展示顺序（`Aa` / 全词 / 正则，与原型一致）。
pub const SEARCH_TOGGLES: [SearchToggle; 3] = [
    SearchToggle::CaseSensitive,
    SearchToggle::WholeWord,
    SearchToggle::Regex,
];

impl SearchToggle {
    /// 按钮上的短标签（宽度有限，用符号而非文字）。
    pub const fn badge(self) -> &'static str {
        match self {
            Self::CaseSensitive => "Aa",
            Self::WholeWord => "字",
            Self::Regex => ".*",
        }
    }

    /// 悬浮提示（无障碍：符号本身不表意，必须有文字说明）。
    pub const fn hint(self) -> &'static str {
        match self {
            Self::CaseSensitive => "区分大小写",
            Self::WholeWord => "全字匹配",
            Self::Regex => "正则表达式",
        }
    }

    /// 该开关当前是否开启。
    pub const fn is_on(self, options: &SearchOptions) -> bool {
        match self {
            Self::CaseSensitive => options.case_sensitive,
            Self::Regex => options.regex,
            Self::WholeWord => options.whole_word,
        }
    }
}

/// 文件树的一行（**投影**，非缓存）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TreeRow {
    /// 完整路径。
    pub path: String,
    /// 显示名（末段）。
    pub name: String,
    /// 缩进层级（根为 0）。
    pub depth: usize,
    /// 是否为目录。
    pub is_directory: bool,
    /// 是否为该层级的最后一项（决定树线的「└」还是「├」）。
    pub is_last: bool,
    /// 是否有后续兄弟（即不是该层最后一项）。
    pub has_next_sibling: bool,
}

impl TreeRow {
    /// 该行是否应显示展开箭头。
    pub const fn is_expandable(&self) -> bool {
        self.is_directory
    }
}

/// 一个搜索结果分组（按文件聚合）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SearchGroup {
    /// 文件路径。
    pub path: String,
    /// 显示名（末段）。
    pub file_name: String,
    /// 该文件内的命中（保持出现顺序 = 行号升序）。
    pub hits: Vec<SearchHitRow>,
}

/// 一条命中的展示投影。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SearchHitRow {
    /// 行号（0-based，领域给的值；展示时 +1）。
    pub line: usize,
    /// 片段文本。
    pub snippet: String,
    /// 命中在该片段内的**字符**区间（用于高亮）。
    ///
    /// 用字符而非字节：片段最终要交给 egui 排版，字节下标在中文里会切错位置。
    pub highlight: (usize, usize),
}

/// 一次「名字编辑」的提交内容（新建或重命名）。
///
/// 用枚举而非 `(String, String, bool, bool)`：四种布尔组合里有两个字段
/// 永远不可能同时为真（新建与重命名互斥），用位置不同的布尔表达互斥关系，
/// 调用点只能靠注释记住「第四个布尔是 is_new」——迟早写反。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NameEdit {
    /// 新建：`(父目录, 名字, 是否目录)`；父目录为空串表示工作区根。
    Create {
        /// 父目录路径（空串 = 根目录）。
        parent: String,
        /// 新名字。
        name: String,
        /// 是否为目录。
        is_directory: bool,
    },
    /// 重命名：`(原路径, 新名字)`。
    Rename {
        /// 原路径。
        path: String,
        /// 新名字。
        name: String,
    },
}

impl NameEdit {
    /// 提交的名字（两种情况共有）。
    pub fn name(&self) -> &str {
        match self {
            Self::Create { name, .. } | Self::Rename { name, .. } => name,
        }
    }

    /// 提交的名字（可写，用于「输入框绑定」场景）。
    pub fn name_mut(&mut self) -> &mut String {
        match self {
            Self::Create { name, .. } | Self::Rename { name, .. } => name,
        }
    }

    /// 名字是否可用（非空且非纯空白）。
    pub fn has_usable_name(&self) -> bool {
        !self.name().trim().is_empty()
    }
}

/// 侧边栏 UI 状态。
#[derive(Clone, Debug, Default)]
pub struct SidebarUiState {
    /// 展开的目录路径集合。
    pub expanded_dirs: Vec<String>,
    /// 选中的文件路径。
    pub selected_path: Option<String>,
    /// 搜索输入框内容。
    pub search_query: String,
    /// 搜索选项。
    pub search_options: SearchOptions,
    /// 大纲当前展开到第几级（1–6）。
    pub outline_max_level: u8,
    /// 大纲当前选中的标题文本（用于高亮定位）。
    pub outline_selected: Option<String>,
    /// 右键菜单当前作用于哪个路径（`None` 表示菜单关闭）。
    pub context_menu_path: Option<String>,
    /// 正在重命名的路径（该行渲染成输入框）。
    pub renaming_path: Option<String>,
    /// 重命名输入框的草稿文本。
    pub rename_draft: String,
    /// 正在新建的条目：`(父目录路径, 名字, 是否目录)`。
    pub creating: Option<(String, String, bool)>,
    /// 侧栏错误提示（操作失败时显示，成功后清空）。
    pub error: Option<String>,
    /// 侧栏提示（如「已创建 xxx.md」）。
    pub notice: Option<String>,
    /// 是否还没做过「首次自动展开」。
    ///
    /// 单独记一个布尔而不是拿 `expanded_dirs.is_empty()` 判断：
    /// 用户手动全部收起后 `expanded_dirs` 也是空的，那样每次重扫都会
    /// 把他收起的目录重新展开——他刚做的操作被系统悄悄撤销。
    pub needs_initial_expand: bool,
    /// 上一帧**实际绘制过**的标签（`None` 表示侧栏还没画过）。
    ///
    /// 存在的理由：焦点只能交给「刚切过来」的搜索标签，但标签值可能
    /// 在本帧绘制**之前**就被改掉（快捷键、测试直接赋值、命令面板）。
    /// 拿本帧开头读到的值当「上一个」永远等于当前值，切换就永远检测不到。
    /// 故由视图在画完后回写。
    pub drawn_tab: Option<crate::presentation::state::app_ui_state::SidebarTab>,
}

impl SidebarUiState {
    /// 本帧要绘制的标签是否为搜索，且是从别的标签**刚切过来**的。
    ///
    /// 两次调用之间必须夹一次 [`SidebarUiState::mark_drawn`]（由视图在
    /// 画完后调用），否则读到的永远是同一个值。
    pub fn search_just_activated(
        &self,
        current: crate::presentation::state::app_ui_state::SidebarTab,
    ) -> bool {
        current == crate::presentation::state::app_ui_state::SidebarTab::Search
            && self.drawn_tab != Some(current)
    }

    /// 记下本帧画过哪个标签（视图在画完后调用）。
    pub fn mark_drawn(&mut self, tab: crate::presentation::state::app_ui_state::SidebarTab) {
        self.drawn_tab = Some(tab);
    }
}

impl SidebarUiState {
    /// 构造默认状态。
    pub fn new() -> Self {
        Self {
            outline_max_level: 6,
            needs_initial_expand: true,
            ..Default::default()
        }
    }

    /// 目录是否已展开。
    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded_dirs.iter().any(|p| p == path)
    }

    /// 切换目录展开状态，返回切换后的值。
    pub fn toggle_expanded(&mut self, path: &str) -> bool {
        if let Some(index) = self.expanded_dirs.iter().position(|p| p == path) {
            self.expanded_dirs.remove(index);
            return false;
        }
        self.expanded_dirs.push(path.to_string());
        true
    }

    /// 展开某目录（大纲 / 搜索跳转时用）。
    pub fn expand(&mut self, path: &str) {
        if !self.is_expanded(path) {
            self.expanded_dirs.push(path.to_string());
        }
    }

    /// 全部收起。
    pub fn collapse_all(&mut self) {
        self.expanded_dirs.clear();
    }

    /// 设置大纲可见层级（夹紧到 1–6）。
    pub fn set_outline_max_level(&mut self, level: u8) {
        self.outline_max_level = level.clamp(1, 6);
    }

    /// 大纲中某级是否应显示。
    pub fn outline_level_visible(&self, level: u8) -> bool {
        level != 0 && level <= self.outline_max_level
    }

    /// 搜索框是否为空（空则不发起搜索，避免无谓的全盘扫描）。
    pub fn should_search(&self) -> bool {
        !self.search_query.trim().is_empty()
    }

    /// 文档切换后清空选中项。
    pub fn reset_for_new_document(&mut self) {
        self.selected_path = None;
        self.outline_selected = None;
    }

    // ---------- 右键菜单与重命名 ----------

    /// 打开某条目的右键菜单。
    ///
    /// 同时结束重命名与新建：三者共用同一块「正在编辑」的视觉空间，
    /// 同时开着会让用户不知道输入框属于谁。
    pub fn open_context_menu(&mut self, path: &str) {
        self.context_menu_path = Some(path.to_string());
        self.renaming_path = None;
        self.creating = None;
    }

    /// 关闭右键菜单。
    pub fn close_context_menu(&mut self) {
        self.context_menu_path = None;
    }

    /// 当前右键菜单是否作用于 `path`。
    pub fn menu_is_for(&self, path: &str) -> bool {
        self.context_menu_path.as_deref() == Some(path)
    }

    /// 开始重命名（草稿预填当前文件名，含扩展名——用户多半只改主干）。
    pub fn start_rename(&mut self, path: &str, current_name: &str) {
        self.renaming_path = Some(path.to_string());
        self.rename_draft = current_name.to_string();
        self.context_menu_path = None;
        self.creating = None;
        self.error = None;
    }

    /// 取消重命名 / 新建。
    pub fn cancel_edit(&mut self) {
        self.renaming_path = None;
        self.rename_draft.clear();
        self.creating = None;
    }

    /// 是否正处于「输入新名字」状态（重命名或新建）。
    pub fn is_editing_name(&self) -> bool {
        self.renaming_path.is_some() || self.creating.is_some()
    }

    /// 开始新建（`parent` 为 `None` 表示建在工作区根目录）。
    pub fn start_create(&mut self, parent: Option<&str>, is_directory: bool) {
        self.creating = Some((
            parent.unwrap_or_default().to_string(),
            if is_directory {
                "新建文件夹".to_string()
            } else {
                "未命名.md".to_string()
            },
            is_directory,
        ));
        self.renaming_path = None;
        self.context_menu_path = None;
        self.error = None;
    }

    /// 当前正在编辑的草稿文本（新建时取 `creating` 的名字，重命名时取 draft）。
    ///
    /// 视图据此**绑定同一个 `String`**，两种编辑就共用一套渲染与提交逻辑。
    pub fn draft_mut(&mut self) -> Option<&mut String> {
        if let Some((_, name, _)) = self.creating.as_mut() {
            return Some(name);
        }
        if self.renaming_path.is_some() {
            return Some(&mut self.rename_draft);
        }
        None
    }

    /// 取出待提交的名字（并结束编辑态）；没在编辑时返回 `None`。
    ///
    /// 名字为空 / 只有空白时返回 `None`：**不**在这里报错，
    /// 因为「新建」与「重命名」对空名的处理不同（前者可以给默认名，
    /// 后者必须取消），判定权交给各自的调用点。
    pub fn take_edit(&mut self) -> Option<NameEdit> {
        if let Some((parent, name, is_directory)) = self.creating.take() {
            return Some(NameEdit::Create {
                parent,
                name,
                is_directory,
            });
        }
        let path = self.renaming_path.take()?;
        Some(NameEdit::Rename {
            path,
            name: std::mem::take(&mut self.rename_draft),
        })
    }

    /// 记一条错误提示（同时清掉成功提示，避免两条同时出现互相矛盾）。
    pub fn fail(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
        self.notice = None;
    }

    /// 记一条成功提示（同时清掉错误提示）。
    pub fn succeed(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
        self.error = None;
    }
}

/// 把扁平的扫描条目折成带缩进的可见行。
///
/// # 为什么要「折」而不是直接画
///
/// `scan` 返回的是**深度优先的前序扁平列表**（父目录紧跟其子项），
/// 而界面需要的是「当前展开状态下可见的那些行」——未展开的目录
/// 其子树整体不画。折行必须知道每个条目的**深度**与**是否最后一项**，
/// 这两个信息在扁平列表里不存在，只能靠路径逐级推导。
///
/// # 三个必须处理的边界
///
/// 1. **顺序**：假定输入已按「目录在前、同层按名称」排好（`FsWorkspaceRepository`
///    保证）。若自行重排，「重命名后位置不动」这个预期就没了。
/// 2. **深度**：按**路径分隔符个数**相对根计算，而不是「数前面有多少个目录条目」——
///    后者在条目列表不完整时（如跳过隐藏目录）会算错。
/// 3. **折叠的目录自身仍要显示**：收起 `a/` 时 `a` 这行还在，只是它的子项不画。
pub fn tree_rows(
    entries: &[(String, bool)],
    root: &str,
    expanded: &dyn Fn(&str) -> bool,
) -> Vec<TreeRow> {
    let root_depth = depth_of(root);
    let mut out: Vec<TreeRow> = Vec::with_capacity(entries.len());
    for (path, is_directory) in entries {
        if !is_under_root(path, root) {
            continue;
        }
        let depth = depth_of(path).saturating_sub(root_depth + 1);
        // 父目录没展开 → 整棵子树不画。但这一行本身要画（收起时仍可见）。
        if depth > 0 && !visible_ancestors(path, root, expanded) {
            continue;
        }
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        out.push(TreeRow {
            path: path.clone(),
            name,
            depth,
            is_directory: *is_directory,
            is_last: false,
            has_next_sibling: false,
        });
    }
    // 标注「是否最后一项」：同深度的相邻两行，前者不是最后一项。
    // 逐层扫一遍即可，条目数上限 5000，代价可忽略。
    for i in 0..out.len() {
        let (depth, path) = (out[i].depth, out[i].path.clone());
        let has_next = out
            .iter()
            .skip(i + 1)
            .find(|row| row.depth <= depth)
            .is_some_and(|row| row.depth == depth && row.path != path);
        out[i].has_next_sibling = has_next;
        out[i].is_last = !has_next;
    }
    out
}

/// 某路径是否在根目录之下（含根本身）。
fn is_under_root(path: &str, root: &str) -> bool {
    if root.is_empty() {
        return true;
    }
    path == root || path.starts_with(&format!("{root}/"))
}

/// 路径的深度（`/` 分隔的段数）。
fn depth_of(path: &str) -> usize {
    if path.is_empty() {
        return 0;
    }
    path.split('/').filter(|s| !s.is_empty()).count()
}

/// `path` 的所有祖先目录（不含自身）是否都已展开。
fn visible_ancestors(path: &str, root: &str, expanded: &dyn Fn(&str) -> bool) -> bool {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let root_depth = depth_of(root);
    let mut current = String::from(root);
    // 逐级向下拼，每到一级目录就问一次「展开了吗」。
    for segment in segments.iter().skip(root_depth) {
        current = if current.is_empty() {
            segment.to_string()
        } else {
            format!("{current}/{segment}")
        };
        if current == path {
            return true;
        }
        if !expanded(&current) {
            return false;
        }
    }
    true
}

/// 把命中按文件分组（保持文件首次出现的顺序）。
///
/// 分组而非平铺的原因与原型一致：跨文件搜索里同一文件的命中往往连成一片，
/// 平铺会让同一个文件名在结果里重复十几次，用户得自己扫着找。
pub fn group_hits(hits: &[(String, usize, String)]) -> Vec<SearchGroup> {
    let mut groups: Vec<SearchGroup> = Vec::new();
    for (path, line, snippet) in hits {
        let file_name = path.rsplit('/').next().unwrap_or(path).to_string();
        match groups.iter_mut().find(|g| &g.path == path) {
            Some(existing) => existing.hits.push(SearchHitRow {
                line: *line,
                snippet: snippet.clone(),
                highlight: (0, 0),
            }),
            None => groups.push(SearchGroup {
                path: path.clone(),
                file_name,
                hits: vec![SearchHitRow {
                    line: *line,
                    snippet: snippet.clone(),
                    highlight: (0, 0),
                }],
            }),
        }
    }
    groups
}

/// 在片段里定位查询词，返回**字符**区间（未找到时返回 `None`）。
///
/// 找不到不是错误：大小写不敏感 + 全词匹配时片段可能不含完整查询
/// （片段被截断过），此时不高亮比高亮错位置好。
pub fn locate_in_snippet(
    snippet: &str,
    query: &str,
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }
    let haystack: Vec<char> = snippet.chars().collect();
    let needle: Vec<char> = query.chars().collect();
    if needle.len() > haystack.len() {
        return None;
    }
    let hit = haystack.windows(needle.len()).position(|w| {
        w.iter().zip(&needle).all(|(a, b)| {
            if case_sensitive {
                a == b
            } else {
                a.to_lowercase().eq(b.to_lowercase())
            }
        })
    })?;
    Some((hit, hit + needle.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三个开关的 `toggled` 互不干扰。
    #[test]
    fn toggling_one_option_leaves_the_others_alone() {
        let base = SearchOptions::default();
        let after = base.toggled(SearchToggle::Regex);
        assert!(after.regex);
        assert!(!after.case_sensitive && !after.whole_word);
        assert_eq!(base.active_count(), 0);
        assert_eq!(after.active_count(), 1);
    }

    /// 树行折叠：收起目录后子项不出现，但目录自身仍在。
    #[test]
    fn collapsed_directory_hides_children_but_keeps_itself() {
        let entries = vec![
            ("/w/a".to_string(), true),
            ("/w/a/x.md".to_string(), false),
            ("/w/b.md".to_string(), false),
        ];
        let never = |_: &str| false;
        let rows = tree_rows(&entries, "/w", &never);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b.md"], "收起的目录其子项应消失");

        let always = |_: &str| true;
        let rows = tree_rows(&entries, "/w", &always);
        assert_eq!(rows.len(), 3, "全展开时应有三行");
    }

    /// 深度按路径层级算，不是按「前面有几个目录条目」。
    #[test]
    fn depth_counts_path_levels() {
        let entries = vec![
            ("/w/a".to_string(), true),
            ("/w/a/deep".to_string(), true),
            ("/w/a/deep/y.md".to_string(), false),
        ];
        let always = |_: &str| true;
        let rows = tree_rows(&entries, "/w", &always);
        let depths: Vec<usize> = rows.iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![0, 1, 2]);
    }

    /// 根目录之外的文件不出现（工作区可能包含未扫描到的兄弟目录）。
    #[test]
    fn entries_outside_the_root_are_dropped() {
        let entries = vec![
            ("/w/a.md".to_string(), false),
            ("/other/b.md".to_string(), false),
        ];
        let always = |_: &str| true;
        let rows = tree_rows(&entries, "/w", &always);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "a.md");
    }

    /// 同层最后一项被正确标注（树线要靠它区分「└」与「├」）。
    #[test]
    fn last_sibling_flag_matches_the_traversal_order() {
        let entries = vec![
            ("/w/dir".to_string(), true),
            ("/w/dir/x".to_string(), false),
            ("/w/y.md".to_string(), false),
            ("/w/z.md".to_string(), false),
        ];
        let always = |_: &str| true;
        let rows = tree_rows(&entries, "/w", &always);
        let flags: Vec<(&str, bool)> = rows.iter().map(|r| (r.name.as_str(), r.is_last)).collect();
        assert_eq!(
            flags,
            vec![("dir", false), ("x", true), ("y.md", false), ("z.md", true)]
        );
    }

    /// 搜索结果按文件分组，文件顺序按首次出现。
    #[test]
    fn hits_are_grouped_by_file() {
        let hits = vec![
            ("/w/b.md".to_string(), 1, "one".to_string()),
            ("/w/a.md".to_string(), 0, "two".to_string()),
            ("/w/b.md".to_string(), 4, "three".to_string()),
        ];
        let groups = group_hits(&hits);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].file_name, "b.md");
        assert_eq!(groups[0].hits.len(), 2, "同一文件的命中应合并");
        assert_eq!(groups[1].file_name, "a.md");
    }

    /// 片段里定位查询词返回**字符**下标（中文片段按字节切会错位）。
    #[test]
    fn snippet_lookup_returns_character_offsets() {
        assert_eq!(
            locate_in_snippet("needle 一处", "needle", false),
            Some((0, 6))
        );
        // 中文在「needle」之后：字节下标会是 6..12，字符下标才是 6..7。
        assert_eq!(locate_in_snippet("needle 一处", "一", false), Some((7, 8)));
        assert_eq!(locate_in_snippet("Needle", "needle", true), None);
        assert_eq!(locate_in_snippet("Needle", "needle", false), Some((0, 6)));
    }

    /// 新建 / 重命名共用一套草稿存取。
    ///
    /// 注意新建时草稿**预填了默认名**（`未命名.md` / `新建文件夹`），
    /// 与原型的 `createFile(parentId, name='未命名.md')` 一致：
    /// 用户点「新建」后通常直接敲名字，默认名只是省一次全选。
    #[test]
    fn draft_is_shared_by_create_and_rename() {
        let mut state = SidebarUiState::new();
        state.start_create(Some("/w"), false);
        *state.draft_mut().expect("新建中应有草稿") = "笔记.md".into();
        assert!(state.is_editing_name());
        let edit = state.take_edit().expect("应能取出");
        assert_eq!(
            edit,
            NameEdit::Create {
                parent: "/w".into(),
                name: "笔记.md".into(),
                is_directory: false,
            }
        );
        assert!(!state.is_editing_name(), "提交后应退出编辑态");

        let mut state = SidebarUiState::new();
        state.start_rename("/w/a.md", "a.md");
        assert_eq!(state.draft_mut().map(|s| s.as_str()), Some("a.md"));
        assert_eq!(
            state.take_edit(),
            Some(NameEdit::Rename {
                path: "/w/a.md".into(),
                name: "a.md".into(),
            })
        );
    }

    /// 新建时预填默认名（用户多半只是改主干，不必全选重打）。
    #[test]
    fn create_seeds_a_default_name() {
        let mut state = SidebarUiState::new();
        state.start_create(None, false);
        assert_eq!(state.draft_mut().map(|s| s.as_str()), Some("未命名.md"));
        state.cancel_edit();

        let mut state = SidebarUiState::new();
        state.start_create(None, true);
        assert_eq!(state.draft_mut().map(|s| s.as_str()), Some("新建文件夹"));
    }

    /// 空白名字不算「可用」（否则会建出无名文件 / 把文件改成空名）。
    #[test]
    fn blank_names_are_rejected() {
        let mut state = SidebarUiState::new();
        state.start_create(None, true);
        *state.draft_mut().unwrap() = "   ".into();
        let edit = state.take_edit().unwrap();
        assert!(!edit.has_usable_name());
        assert_eq!(edit.name(), "   ");
    }

    /// 三个开关与展示顺序一一对应（徽标不能串位）。
    #[test]
    fn every_toggle_has_a_badge_and_a_hint() {
        for toggle in SEARCH_TOGGLES {
            assert!(!toggle.badge().is_empty());
            assert!(!toggle.hint().is_empty());
        }
        assert_eq!(SEARCH_TOGGLES[0].badge(), "Aa");
    }

    /// 开右键菜单会结束重命名与新建（同一块视觉空间不能同时开着两种编辑）。
    #[test]
    fn opening_the_menu_ends_any_name_editing() {
        let mut state = SidebarUiState::new();
        state.start_rename("/w/a.md", "a.md");
        state.open_context_menu("/w/a.md");
        assert!(state.renaming_path.is_none());
        assert!(state.menu_is_for("/w/a.md"));
        assert!(!state.menu_is_for("/w/b.md"));
    }

    /// 错误与成功提示互斥：两条同时显示会互相矛盾。
    #[test]
    fn error_and_notice_clear_each_other() {
        let mut state = SidebarUiState::new();
        state.succeed("已创建");
        state.fail("重名");
        assert_eq!(state.notice, None, "报错时不应还留着上一条成功提示");
        state.succeed("已重命名");
        assert_eq!(state.error, None, "成功时不应还留着上一条错误");
    }
}
