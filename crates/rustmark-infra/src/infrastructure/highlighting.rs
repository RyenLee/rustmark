//! infrastructure::highlighting 模块声明。

pub mod syntect_highlighter;

/// 增量高亮的纯函数：变更区间计算与片段平移。
pub(crate) mod diff;

/// 增量高亮会话：连续编辑只重解变更行。
///
/// 已接入生产路径（2026-10-31 复核）：`SyntectHighlighter::start_session`
/// 返回 `SyntectSession`，由 `application::markdown::service::highlight_worker`
/// 按文档持有并复用。设计背景（原 T4 / OPT-011 预研）见模块文档。
pub(crate) mod incremental;
