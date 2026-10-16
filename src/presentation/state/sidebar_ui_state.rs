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
//!
//! # 模块结构
//!
//! facade 保留 `SidebarUiState` 本身（展开 / 选中 / 编辑菜单等交互）与内联测试；
//! 全局搜索开关与命中投影在 `search`，文件树折行与名字编辑模型在 `tree`。

mod search;
mod tree;

pub use search::*;
pub use tree::*;

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
    /// 搜索范围（「当前文档」/「整个工作区」，见 [`SearchScope`]）。
    ///
    /// 与 `search_options` 一样只活在会话里，不落偏好：范围是**这一轮
    /// 查找**的临时决定，不是用户的长期设置；重启后回到默认值
    /// （当前文档）比恢复一个可能已不合时宜的范围更可预期。
    pub search_scope: SearchScope,
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

    /// 范围切换器的两个选项都带得出去的标签与提示，且默认是「当前文档」。
    ///
    /// 「默认当前文档」是被设计固定下来的语义（侧栏按作用域分组后
    /// 「搜索」属文档级），写成断言防止后人随手改默认值——
    /// 那一改会让所有既有用户第一次打开就看到跨文件结果，
    /// 与面板标题声明的作用域不符。
    #[test]
    fn every_scope_has_a_label_and_a_hint_and_defaults_to_current_document() {
        for scope in SEARCH_SCOPES {
            assert!(!scope.label().is_empty());
            assert!(!scope.hint().is_empty());
            assert!(!scope.placeholder().is_empty());
        }
        assert_eq!(SEARCH_SCOPES[0], SearchScope::CurrentDocument);
        assert_eq!(SearchScope::default(), SearchScope::CurrentDocument);
        assert_eq!(
            SidebarUiState::new().search_scope,
            SearchScope::CurrentDocument
        );
        // 占位提示必须随范围变化，否则切换器成了摆设。
        assert_ne!(
            SearchScope::CurrentDocument.placeholder(),
            SearchScope::OpenDocuments.placeholder()
        );
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
