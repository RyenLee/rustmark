//! Markdown 格式检查（lint）的规则与判定（V11-08）。
//!
//! # 为什么放在领域层
//!
//! 规则是对 **Markdown 语义**的判定（标题层级、列表标记、代码围栏），
//! 与颜色 / 字体 / 面板布局无关。放在表现层会让同一份规则在
//! 「侧栏问题列表」与将来可能的「导出前检查」里各写一遍。
//!
//! # 设计取舍：字典型检查而非解析型
//!
//! 本模块**不复用** `MarkdownParser`——解析器丢掉了行号、缩进与原始标记
//! （`RenderedBlock::content` 是纯文本），而 lint 恰恰要指出
//! 「第几行的哪个字符不对」。故这里按**行**扫描，只在需要语义时
//! （代码块内部）做极小的状态跟踪。
//!
//! 代价：处理不了「缩进代码块里恰好写着 `# 标题`」这类边角。
//! 收益：行号与列号天然准确，实现只有百来行、零依赖、易断言。
//!
//! # 误报防线
//!
//! 全部规则都**跳过围栏代码块内部**：代码里出现行尾空格、
//! `#` 注释、`-` 列表符是常态，把它们报成问题只会让用户关掉这个功能。
//!
//! # 模块结构
//!
//! 规则的逐行判定函数收在私有子模块 [`rules`] 中（一条规则一项业务能力），
//! 本文件保留规则类型、问题模型与无状态入口 [`MarkdownLinter`]。

mod rules;

use rules::{fenced_lines, heading_jump, list_marker_mixed, trailing_whitespace, unclosed_fence};
use std::collections::BTreeSet;

/// 一条格式规则。
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) enum LintRule {
    /// 标题层级跳跃（如 `H1` 直接到 `H3`）。
    HeadingJump,
    /// 同一个列表里混用 `-` / `*` / `+`。
    ListMarkerMixed,
    /// 行尾多余空格。
    TrailingWhitespace,
    /// 代码围栏未闭合。
    UnclosedFence,
}

impl LintRule {
    /// 全部规则（设置页开关列表与「全选」的来源）。
    pub(crate) const ALL: [LintRule; 4] = [
        LintRule::HeadingJump,
        LintRule::ListMarkerMixed,
        LintRule::TrailingWhitespace,
        LintRule::UnclosedFence,
    ];

    /// 稳定标识（持久化与跨层传递用，不随文案变化）。
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::HeadingJump => "heading_jump",
            Self::ListMarkerMixed => "list_marker_mixed",
            Self::TrailingWhitespace => "trailing_whitespace",
            Self::UnclosedFence => "unclosed_fence",
        }
    }

    /// 中文显示名（设置页开关与问题列表前缀）。
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::HeadingJump => "标题层级跳跃",
            Self::ListMarkerMixed => "列表符号混用",
            Self::TrailingWhitespace => "行尾空格",
            Self::UnclosedFence => "未闭合代码围栏",
        }
    }

    /// 由稳定标识还原规则（持久化往返用）；未知标识返回 `None`。
    ///
    /// 仅单测使用（`rule_ids_round_trip`），生产路径只写不读。
    #[cfg(test)]
    pub(crate) fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rule| rule.id() == id)
    }
}

/// 一组启用中的规则。
///
/// 用 `BTreeSet` 而非 `Vec`：规则个数固定且需要「是否包含」的判断，
/// 集合语义比线性查找更贴切，也顺带去重（重复登记同一条规则不该报两遍）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct LintRuleSet {
    rules: BTreeSet<LintRule>,
}

impl LintRuleSet {
    /// 由规则列表构造（自动去重）。
    pub fn new(rules: impl IntoIterator<Item = LintRule>) -> Self {
        Self {
            rules: rules.into_iter().collect(),
        }
    }

    /// 该规则是否启用。
    pub(crate) fn contains(&self, rule: LintRule) -> bool {
        self.rules.contains(&rule)
    }

    /// 启用中的规则（按 [`LintRule::ALL`] 的顺序，便于 UI 稳定排列）。
    pub(crate) fn enabled(&self) -> Vec<LintRule> {
        LintRule::ALL
            .iter()
            .copied()
            .filter(|r| self.rules.contains(r))
            .collect()
    }

    /// 全部规则都启用的规则集（测试基线，生产由偏好设置构造）。
    #[cfg(test)]
    pub(crate) fn all() -> Self {
        Self::new(LintRule::ALL)
    }

    /// 空规则集（测试「全关」路径，生产由偏好设置构造）。
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self::new([])
    }
}

/// 一处格式问题。
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct LintIssue {
    rule: LintRule,
    /// 行号（**0-based**，与大纲 / 搜索一致）。
    line: usize,
    /// 列号（**0-based 字符**，行级问题为 0）。
    column: usize,
    /// 说明文案（可直接呈现给用户）。
    message: String,
}

impl LintIssue {
    /// 构造一处问题。
    pub fn new(rule: LintRule, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            rule,
            line,
            column,
            message: message.into(),
        }
    }

    /// 命中的规则。
    pub(crate) const fn rule(&self) -> LintRule {
        self.rule
    }

    /// 行号（0-based）。
    pub(crate) const fn line(&self) -> usize {
        self.line
    }

    /// 列号（0-based 字符）。
    pub(crate) const fn column(&self) -> usize {
        self.column
    }

    /// 说明文案。
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

/// Markdown 格式检查器（**无状态纯函数**）。
pub(crate) struct MarkdownLinter;

impl MarkdownLinter {
    /// 检查全文，返回全部问题（按行号、再按列号升序）。
    ///
    /// # 边界
    ///
    /// - 空文本 → 空集；
    /// - 规则集为空 → 空集（不逐个规则跑一遍，省掉无谓的扫描）；
    /// - 全部规则都跳过围栏代码块内部（见模块文档「误报防线」）。
    pub(crate) fn lint(text: &str, rules: &LintRuleSet) -> Vec<LintIssue> {
        if text.is_empty() || rules.enabled().is_empty() {
            return Vec::new();
        }
        let lines: Vec<&str> = text.split('\n').collect();
        let fenced = fenced_lines(&lines);
        let mut out = Vec::new();

        if rules.contains(LintRule::HeadingJump) {
            out.extend(heading_jump(&lines, &fenced));
        }
        if rules.contains(LintRule::ListMarkerMixed) {
            out.extend(list_marker_mixed(&lines, &fenced));
        }
        if rules.contains(LintRule::TrailingWhitespace) {
            out.extend(trailing_whitespace(&lines, &fenced));
        }
        if rules.contains(LintRule::UnclosedFence) {
            out.extend(unclosed_fence(&lines));
        }

        out.sort_by_key(|issue| (issue.line, issue.column));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issues(text: &str) -> Vec<LintIssue> {
        MarkdownLinter::lint(text, &LintRuleSet::all())
    }

    fn rules_of(text: &str) -> Vec<LintRule> {
        issues(text).iter().map(|i| i.rule()).collect()
    }

    /// 空文本与空规则集都返回空。
    #[test]
    fn empty_input_and_empty_rules_yield_nothing() {
        assert!(issues("").is_empty());
        assert!(MarkdownLinter::lint("# a\n", &LintRuleSet::none()).is_empty());
    }

    /// 标题层级跳跃会被报出，且报在**跳的那一层**。
    #[test]
    fn heading_level_jumps_are_reported() {
        let got = issues("# 一\n### 三\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].rule(), LintRule::HeadingJump);
        assert_eq!(got[0].line(), 1);
        assert!(got[0].message().contains("H1"), "文案应说清从哪一级跳的");
    }

    /// 逐级递进不报；**回退**也不报（从 H3 回到 H1 是常见写法）。
    #[test]
    fn gradual_and_backward_headings_are_clean() {
        assert!(issues("# 一\n## 二\n### 三\n").is_empty());
        assert!(issues("### 三\n# 一\n").is_empty());
    }

    /// 围栏里的 `#` 注释不算标题。
    #[test]
    fn headings_inside_code_fences_are_ignored() {
        assert!(issues("# 一\n```\n### 这是注释不是标题\n```\n").is_empty());
    }

    /// 同一列表混用标记会被报出，且只报第一次变化。
    #[test]
    fn mixed_list_markers_are_reported_once() {
        let got = issues("- a\n- b\n* c\n+ d\n");
        assert_eq!(got.len(), 1, "报一次就够，实际 {got:?}");
        assert_eq!(got[0].rule(), LintRule::ListMarkerMixed);
        assert_eq!(got[0].line(), 2);
    }

    /// 统一标记的列表干净。
    #[test]
    fn a_consistent_list_is_clean() {
        assert!(issues("- a\n- b\n").is_empty());
    }

    /// 嵌套列表（缩进不同）用不同标记不报。
    #[test]
    fn nested_lists_may_use_their_own_marker() {
        assert!(issues("- a\n  * b\n").is_empty());
    }

    /// 分隔线 `---` 不是列表。
    #[test]
    fn thematic_breaks_are_not_lists() {
        assert!(issues("---\n***\n").is_empty());
    }

    /// 行尾空格：1 个与 3 个都报，2 个（硬换行）不报。
    #[test]
    fn trailing_spaces_are_reported_but_hard_breaks_are_not() {
        assert_eq!(rules_of("a \nb").len(), 1, "1 个空格应报");
        assert_eq!(rules_of("a   \nb").len(), 1, "3 个空格应报");
        assert!(issues("a  \nb").is_empty(), "2 个空格是硬换行，不该报");
    }

    /// 未闭合围栏会被报出，并指向**开始**的那一行。
    #[test]
    fn an_unclosed_fence_points_at_where_it_started() {
        let got = issues("正文\n```rust\nfn main() {}\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].rule(), LintRule::UnclosedFence);
        assert_eq!(got[0].line(), 1, "应指向围栏开始的那一行");
    }

    /// 闭合了的围栏干净。
    #[test]
    fn a_closed_fence_is_clean() {
        assert!(issues("正文\n```\ncode\n```\n").is_empty());
    }

    /// 围栏内部的行尾空格不报（代码里的空格是常态）。
    #[test]
    fn code_blocks_are_exempt_from_trailing_space_checks() {
        assert!(issues("```\na   \n```\n").is_empty());
    }

    /// 结果按行号、再按列号升序（侧栏列表不该乱序）。
    #[test]
    fn issues_are_sorted_by_position() {
        let got = issues("a   \n# 一\n### 三\n");
        let keys: Vec<(usize, usize)> = got.iter().map(|i| (i.line(), i.column())).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "结果必须按位置升序，实际 {keys:?}");
    }

    /// 规则开关真的生效（关掉的规则不产生问题）。
    #[test]
    fn disabled_rules_produce_nothing() {
        let text = "# 一\n### 三\n";
        let set = LintRuleSet::new([LintRule::UnclosedFence]);
        assert!(MarkdownLinter::lint(text, &set).is_empty());
        assert_eq!(LintRuleSet::all().enabled().len(), 4);
        assert!(LintRuleSet::all().contains(LintRule::HeadingJump));
    }

    /// 规则标识可往返解析（持久化用）。
    #[test]
    fn rule_ids_round_trip() {
        for rule in LintRule::ALL {
            assert_eq!(LintRule::parse(rule.id()), Some(rule));
        }
        assert_eq!(LintRule::parse("no_such_rule"), None);
    }
}
