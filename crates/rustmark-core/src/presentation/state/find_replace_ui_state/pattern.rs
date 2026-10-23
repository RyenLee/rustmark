//! 查找替换的模式编译适配层。
//!
//! 迷你正则引擎本体在 [`crate::shared::pattern`]——它同时服务本面板与
//! 侧栏跨文件搜索（应用层），而分层依赖单向（`check-arch` 机械校验），
//! 应用层不能引用表现层，引擎因此下沉到 shared。
//! 这里只保留依赖 [`FindOptions`] 的编译入口：`compile(pattern, options)`。

use super::{FindOptions, PatternError};
pub(crate) use crate::shared::pattern::{Pattern, Regex};

/// 编译模式（字面量或正则，取决于开关）。
pub(crate) fn compile(pattern: &str, options: FindOptions) -> Result<Pattern, PatternError> {
    if pattern.is_empty() {
        return Err(PatternError::Empty);
    }
    if options.regex {
        Regex::compile(pattern, options.case_sensitive).map(Pattern::Regex)
    } else {
        Ok(Pattern::Literal(pattern.to_string()))
    }
}
