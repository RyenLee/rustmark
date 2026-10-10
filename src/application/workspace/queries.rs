//! Workspace 用例查询（阶段 04 · S4-07）。
//!
//! 查询与命令命名不冲突（动名词区分）。跨文件搜索沿用原型的命中上限
//! （单文件 50、总 300），由 [`SearchQuery`] 描述检索条件。

use crate::shared::path::FilePath;

/// 列出最近文件查询。
#[derive(Clone, Debug, Default)]
pub struct ListRecentQuery;

/// 列出标签页查询。
#[derive(Clone, Debug, Default)]
pub struct ListTabsQuery;

/// 跨文件搜索查询。
#[derive(Clone, Debug)]
pub struct SearchQuery {
    /// 待匹配的文本。
    pub pattern: String,
    /// 是否按正则匹配（本阶段退化为「精确子串匹配」；`true` 时等同字面量）。
    pub regex: bool,
    /// 是否区分大小写。
    pub case_sensitive: bool,
    /// 是否要求整词匹配（前后非字母数字）。
    pub whole_word: bool,
}

/// 读取单个工作区文件的内容查询（S6-11f：差异对比的右侧）。
///
/// 刻意**不开文档**：差异对比要读的是「磁盘上那个文件」，
/// 把它开成一个新标签会改变用户的编辑状态（多一个标签、
/// 预览区重排、`active_doc` 切换），而用户只想看一眼差异。
pub struct ReadFileQuery {
    /// 目标路径。
    pub path: FilePath,
}
