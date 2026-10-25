//! Workspace 用例查询（阶段 04 · S4-07）。
//!
//! 查询与命令命名不冲突（动名词区分）。跨文件搜索沿用原型的命中上限
//! （单文件 100、总 1000），由 [`SearchQuery`] 描述检索条件。

use crate::shared::ids::DocumentId;
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
    /// 是否按正则匹配（走 [`crate::shared::pattern`] 迷你引擎；
    /// 模式无效时整次检索返回 `Err`，文案可直接呈现用户）。
    pub regex: bool,
    /// 是否区分大小写。
    pub case_sensitive: bool,
    /// 是否要求整词匹配（前后非字母数字）。
    pub whole_word: bool,
    /// 限定只搜某一个文档（`None` = 见 [`Self::only_open_documents`]）。
    ///
    /// 侧栏「搜索」的范围切换器：`Some(id)` 即「当前文档」。
    ///
    /// 用 [`DocumentId`] 而不是文件路径：未落盘的草稿同样有 ID，
    /// 而它的「路径」是候选收集时合成的 `/draft/<id>`，调用方拼不出来；
    /// 且按 ID 过滤天然命中「草稿优先于磁盘内容」那条规则
    /// （`gather_candidates` 已让打开的文档用内存内容）。
    pub only_document: Option<DocumentId>,
    /// 只搜**已打开的文档**（标签栏里那些），不读工作区目录下未打开的文件。
    ///
    /// 侧栏「搜索」的范围切换器：`true` 即「打开的文档」。
    /// 与 [`Self::only_document`] 同时为真时以 `only_document` 为准
    /// （前者是后者的子集，先过滤更具体的那个）。
    ///
    /// 收益有两条：结果集与用户的编辑上下文一致；读盘量从「全工作区」
    /// 降到「几份打开的文档」——按目录全盘检索会把用户没打开的文件
    /// 也读一遍，那既慢又不是用户要的。
    pub only_open_documents: bool,
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
