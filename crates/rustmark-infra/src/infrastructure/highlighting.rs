//! infrastructure::highlighting 模块声明。

pub mod syntect_highlighter;

/// 增量高亮 PoC（T4 / OPT-011 复议）。
///
/// 暂未接入生产路径：先验证「正确性 + 收益」，再决定是否为其设计
/// 领域接口（预研报告方案 A：`HighlightSession`）。见模块文档。
#[allow(dead_code)]
pub(crate) mod diff;
pub(crate) mod incremental;
