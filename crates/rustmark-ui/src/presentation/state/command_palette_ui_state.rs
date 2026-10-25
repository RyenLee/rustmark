//! 命令面板状态（S6-12，对齐原型 A-24 `CommandPalette.vue` + `lib/commands.ts`）。
//!
//! # 本模块只管「视图状态」，检索逻辑**不在这里**
//!
//! 子序列模糊检索、标签命中权重（100）与关键词命中权重（50）、
//! 命中字符坐标，已经在 S6-09 由 [`ActionTable::search`] 实现并测透
//! （`tests/presentation/shortcut_registry_test.rs` 7 条用例）。
//!
//! **这里刻意不重写一遍。** 两套检索一旦分叉，就会出现
//! 「命令面板搜得到、菜单搜不到」或反之——而「三入口同源」正是
//! S6-09 反复强调的根治目标（原型缺陷 B-02 / B-08 的结构性解法）。
//! 本模块只负责三件事：查询词、当前选中下标、面板开合的重置语义。
//!
//! # 检索范围与原型的一处偏差
//!
//! 原型的 `fuzzyScore` 作用在 `` `${label} ${keywords} ${group}` `` 上，
//! 即**分组名也参与匹配**（搜「格式」能命中格式类命令）。
//! 本实现沿用 S6-09 的两段式（`label` 或 `keywords`），**不含 `group`**。
//!
//! 理由：分组名是「归类」不是「动作」，把「格式」当成可搜内容会命中
//! 一堆和用户意图无关的条目（搜「视图」想找的是「切换预览」而不是
//! 所有 12 条视图命令）。`ActionTable::search` 是 S6-09 已验收的资产，
//! 为命令面板单独扩检索范围等于**推翻一条已通过的验收**——收益不足。
//! 真要按分组筛，应该另给面板一个分组筛选器，而不是混进模糊匹配。

use rustmark_core::presentation::shortcuts::registry::ActionEntry;

/// 一行命令（视图用）。
///
/// 直接包 [`ActionEntry`] 而不重新定义字段：动作、显示名、分组、
/// 快捷键四样东西的**唯一来源**是动作表。复制一份就会漂移。
#[derive(Clone, Debug, PartialEq)]
pub struct CommandRow {
    /// 动作条目（显示与执行都用它）。
    pub entry: ActionEntry,
    /// 命中字符在 `label` 中的**字符**索引（供高亮）。
    ///
    /// 空 = 未命中（查询为空，或只命中了关键词）。
    /// 关键词命中时**不**高亮标签：那些字符根本不在 `label` 里，
    /// 硬要按坐标切标签只会切出乱码。
    pub hits: Vec<usize>,
}

impl CommandRow {
    /// 动作。
    pub fn action(&self) -> rustmark_core::presentation::shortcuts::registry::UiAction {
        self.entry.action
    }

    /// 显示名。
    pub fn label(&self) -> &'static str {
        self.entry.label
    }

    /// 分组显示名。
    pub fn group(&self) -> &'static str {
        self.entry.group.label()
    }
}

/// 命令面板状态。
#[derive(Clone, Debug, Default)]
pub struct CommandPaletteUiState {
    /// 查询词。
    pub query: String,
    /// 当前高亮行的下标。
    pub selected: usize,
}

impl CommandPaletteUiState {
    /// 构造空状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 打开面板：清查询与选中项。
    ///
    /// 与快速打开同理由（S6-11f）：复用上次的查询会让用户以为
    /// 「这些结果是我要的」，而命令面板的语境每次都不同
    /// （刚存完盘 vs 刚改完格式）。
    pub fn open(&mut self) {
        self.query.clear();
        self.selected = 0;
    }

    /// 设置查询词，并把选中项收回 0。
    ///
    /// 收回 0 而非「夹到末项」：结果集变了，末项在新集合里可能指向
    /// 完全无关的命令。输入的瞬间第一项被选中，符合「我在筛东西」的
    /// 直觉（VS Code / 原型 `onInput` 同一行为）。
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        self.selected = 0;
    }

    /// 移动选中项（`delta = ±1`，越界则环绕）。
    ///
    /// 环绕而非夹紧：面板通常是「看好了就回车」，在末尾再按 ↓
    /// 停在原地会让人以为列表到底了、面板卡住。原型 `move()` 同为环绕。
    pub fn move_selection(&mut self, delta: isize, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }
        let n = total as isize;
        let next = (self.selected as isize + delta) % n;
        self.selected = next.rem_euclid(n) as usize;
    }

    /// 把选中项夹进结果范围。
    ///
    /// 必须在**每次**重算结果后调用：查询一变结果就可能变短，
    /// 旧下标越界会让界面「选中一个不存在的行」。原型靠 `watch` 做，
    /// egui 侧没有 `watch`，故显式夹。
    pub fn clamp(&mut self, total: usize) {
        if total == 0 {
            self.selected = 0;
        } else if self.selected >= total {
            self.selected = total - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmark_core::presentation::shortcuts::registry::{MenuGroup, ShortcutRegistry, UiAction};

    fn entry(action: UiAction) -> ActionEntry {
        ShortcutRegistry::with_defaults().table().entry(action)
    }

    fn row(action: UiAction) -> CommandRow {
        CommandRow {
            entry: entry(action),
            hits: Vec::new(),
        }
    }

    #[test]
    fn opening_resets_query_and_selection() {
        let mut state = CommandPaletteUiState::new();
        state.set_query("保存");
        state.selected = 5;
        state.open();
        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn setting_the_query_pulls_the_selection_back_to_zero() {
        let mut state = CommandPaletteUiState::new();
        state.selected = 8;
        state.set_query("撤销");
        assert_eq!(state.query, "撤销");
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn selection_wraps_at_both_ends() {
        let mut state = CommandPaletteUiState::new();
        state.move_selection(1, 3);
        assert_eq!(state.selected, 1);
        state.move_selection(1, 3);
        state.move_selection(1, 3);
        assert_eq!(state.selected, 0, "末尾再往下应回到第一行");
        state.move_selection(-1, 3);
        assert_eq!(state.selected, 2, "第一行再往上应回到末行");
    }

    #[test]
    fn moving_in_an_empty_result_set_is_a_no_op() {
        let mut state = CommandPaletteUiState::new();
        state.selected = 3;
        state.move_selection(1, 0);
        assert_eq!(state.selected, 0, "没有结果时不该停在旧下标上");
    }

    #[test]
    fn clamp_pulls_an_out_of_range_selection_back() {
        let mut state = CommandPaletteUiState::new();
        state.selected = 9;
        state.clamp(4);
        assert_eq!(state.selected, 3);
        state.clamp(0);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn clamp_leaves_a_valid_selection_untouched() {
        let mut state = CommandPaletteUiState::new();
        state.selected = 2;
        state.clamp(5);
        assert_eq!(state.selected, 2, "夹紧不该动本来就在范围内的下标");
    }

    /// 行只是动作条目的包装：字段必须原样透出。
    ///
    /// 钉住这一点是为了防「有人为了方便在 `CommandRow` 上再加一份
    /// `label: String` 副本」——那份副本改键位或改显示名时不会跟着动。
    #[test]
    fn row_exposes_the_action_entry_fields_verbatim() {
        let r = row(UiAction::Save);
        assert_eq!(r.action(), UiAction::Save);
        assert_eq!(r.label(), entry(UiAction::Save).label);
        assert_eq!(
            r.group(),
            entry(UiAction::Save).group.label(),
            "分组名应取自 `MenuGroup::label`，不是另存一份"
        );
    }

    #[test]
    fn rows_compare_by_value_so_tests_can_assert_on_lists() {
        let a = row(UiAction::Save);
        let mut b = row(UiAction::Save);
        assert_eq!(a, b);
        b.hits.push(0);
        assert_ne!(a, b, "命中坐标不同应视为不同的行");
    }

    /// 检索不在本模块——但「面板搜得到动作表里有的全部动作」这条
    /// 契约要在状态层就成立，否则视图层会拿到一个空列表却无从察觉。
    #[test]
    fn every_action_is_reachable_with_an_empty_query() {
        let registry = ShortcutRegistry::with_defaults();
        let table = registry.table();
        let all = table.all();
        assert_eq!(
            all.len(),
            UiAction::ALL.len(),
            "动作表必须覆盖 `UiAction::ALL` 的每一项"
        );
        // 空查询 = 列出全部，原型同款。
        let hits = table.search("");
        assert_eq!(hits.len(), UiAction::ALL.len());
    }

    /// 分组名**不参与**匹配（与原型的一处刻意偏差，见模块文档）。
    ///
    /// 这条测试的作用是**把偏差钉住**：将来若有人把 `group` 加进
    /// 检索范围（看起来像「顺手补全」），它会红，从而被迫回到文档里
    /// 重新讨论，而不是无声改变搜索行为。
    #[test]
    fn group_names_are_not_searchable() {
        let registry = ShortcutRegistry::with_defaults();
        let table = registry.table();
        // 「格式」是 `MenuGroup::Format` 的显示名。
        let by_group = table.search("格式");
        assert!(
            by_group.is_empty(),
            "分组名不该参与匹配，实际命中 {} 条：{:?}",
            by_group.len(),
            by_group.iter().map(|s| s.entry.label).collect::<Vec<_>>()
        );
    }

    /// 检索命中会带上标签高亮坐标；只命中关键词则坐标为空。
    ///
    /// 断言**只针对 `Bold` 这一条**，而不是「`search("bold")` 的所有结果」。
    /// 首次写成后者，结果红了：子序列匹配下 `bold` 还会命中别的动作
    /// （凡是 label / keywords 含 b-o-l-d 子序列的都算），
    /// 其中可能有**靠 label 命中**的条目——那些当然该有高亮坐标。
    /// 「只命中关键词则无坐标」说的是**某一条**的性质，不是整个结果集。
    #[test]
    fn hits_are_provided_only_for_label_matches() {
        let registry = ShortcutRegistry::with_defaults();
        let table = registry.table();
        let by_label = table.search("保存");
        assert!(
            by_label.iter().any(|s| !s.hits.is_empty()),
            "命中显示名时必须给出高亮坐标"
        );
        // `Bold` 的显示名是「加粗」，不含 b/o/l/d —— 只能靠英文关键词命中。
        let bold = table
            .search("bold")
            .into_iter()
            .find(|s| s.entry.action == UiAction::Bold)
            .expect("「bold」应命中加粗");
        assert_eq!(bold.entry.label, "加粗");
        assert!(
            bold.hits.is_empty(),
            "只命中英文关键词时不该按坐标切中文标签（label={:?}）",
            bold.entry.label
        );
    }

    #[test]
    fn rows_carry_a_menu_group_for_display() {
        // 分组徽标是命令面板的一等信息（原型每行右侧都显示）。
        let registry = ShortcutRegistry::with_defaults();
        let table = registry.table();
        let save = table
            .search("保存")
            .into_iter()
            .find(|s| s.entry.action == UiAction::Save)
            .expect("「保存」应能搜到");
        assert_eq!(save.entry.group, MenuGroup::File);
    }
}
