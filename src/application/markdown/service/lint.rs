//! 格式检查用例：偏好 lint 开关 → 领域规则集映射，文档问题投影为 DTO。

use crate::domain::markdown::lint::{LintRule, LintRuleSet, MarkdownLinter};

use crate::application::dto::{LintIssueView, LintRulesView};

use super::super::queries::*;
use super::MarkdownAppService;

/// 偏好里的 lint 开关 → 领域规则集（V11-08）。
///
/// 映射放在应用层而不是让领域去读偏好：领域不该知道「设置」这个上下文，
/// 它只该知道「要跑哪几条规则」。
fn lint_rule_set(view: &LintRulesView) -> LintRuleSet {
    let mut rules = Vec::new();
    if view.heading_jump {
        rules.push(LintRule::HeadingJump);
    }
    if view.list_marker_mix {
        rules.push(LintRule::ListMarkerMixed);
    }
    if view.trailing_whitespace {
        rules.push(LintRule::TrailingWhitespace);
    }
    if view.unclosed_fence {
        rules.push(LintRule::UnclosedFence);
    }
    LintRuleSet::new(rules)
}

impl MarkdownAppService {
    /// 格式检查（V11-08）：返回当前文档里的全部格式问题。
    ///
    /// # 为什么不做缓存
    ///
    /// lint 是**线性扫描**（O(行数)），1MB 文档在 debug 下也就几十毫秒量级；
    /// 而「检查」标签不是每帧可见的——只有用户切过去才会调。
    /// 为它引入一份带失效规则的缓存，收益不抵复杂度（缓存一旦失效不彻底，
    /// 用户就会看到「改完了问题还在」）。
    ///
    /// # 边界
    ///
    /// - 规则全关 → 空集（领域层直接短路，连扫描都不做）；
    /// - 文档不存在 → 空集；
    /// - 大文档**不降级**：与高亮 / 预览不同，lint 不走渲染路径，
    ///   且「检查不出来」比「慢一点」更让人困惑。
    pub fn lint_document(&self, q: LintQuery) -> Vec<LintIssueView> {
        let rules = lint_rule_set(&q.rules);
        if rules.enabled().is_empty() {
            return Vec::new();
        }
        let Ok((content, _revision)) = self.fetch(q.doc_id) else {
            return Vec::new();
        };
        MarkdownLinter::lint(&content, &rules)
            .into_iter()
            .map(|issue| LintIssueView {
                rule: issue.rule().id().to_string(),
                rule_label: issue.rule().label().to_string(),
                line: issue.line(),
                column: issue.column(),
                message: issue.message().to_string(),
            })
            .collect()
    }
}
