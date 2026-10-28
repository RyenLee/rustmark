//! 全局搜索的视图模型：三个开关、按文件分组的命中投影与片段定位。

use rustmark_core::application::dto::SearchResultView;
use rustmark_core::shared::path::file_name_of;

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

    /// 关闭全部选项（搜索标签的「重置」按钮）。
    ///
    /// 整块回到 `Default` 而不是逐字段清零：以后新增开关时不会漏掉一个，
    /// 留下「按了重置但还开着一项」的半重置状态。
    pub fn reset(&mut self) {
        *self = Self::default();
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

/// 搜索范围（侧栏「搜索」标签的范围切换器）。
///
/// 两个选项不是两种算法，而是两种**作用域**：
/// - [`Self::CurrentDocument`] 与大纲 / 检查同属**文档级**，跟随活动文档，
///   文档一切换结果立刻变；
/// - [`Self::OpenDocuments`] 覆盖**多文档标签栏里所有打开的文档**——
///   含未保存的草稿，但不含侧栏文件夹下尚未打开的文件。
///
/// 默认「当前文档」：侧栏按作用域分组后，「搜索」归入文档级面板，
/// 打开文档才是主路径；跨文档查找作为可切换的次要形态保留，
/// 而不是反过来把文档级能力默认关掉。
///
/// # 为什么「打开的文档」而不是「整个工作区」
///
/// 旧语义是「搜工作区目录下的所有文件」——那会把用户**没打开**的
/// 成百上千个文件拖进检索，一次搜索读遍全盘；而用户想跨文件查找时，
/// 找的其实正是自己手头开着的那几份。范围改为「标签栏里所有打开的
/// 文档」后：结果集与用户的工作上下文一致，读盘量也从全盘降到几份。
/// （真需要按文件名找未打开的文件，走「快速打开」浮层。）
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SearchScope {
    /// 只搜当前打开的文档（含未保存的修改）。
    #[default]
    CurrentDocument,
    /// 搜标签栏里所有打开的文档（含未保存的草稿）。
    OpenDocuments,
}

impl SearchScope {
    /// 切换器上的短标签（面板只有 260pt 宽，用词必须短）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::CurrentDocument => "当前文档",
            Self::OpenDocuments => "打开的文档",
        }
    }

    /// 悬浮提示（说清这个范围到底覆盖谁——符号与短词都不自解释）。
    pub const fn hint(self) -> &'static str {
        match self {
            Self::CurrentDocument => "只搜索当前打开的文档（含未保存的修改）",
            Self::OpenDocuments => "搜索标签栏中所有打开的文档（含未保存的草稿）",
        }
    }

    /// 搜索框占位提示。
    ///
    /// 随范围变化：用户还没打字就知道这一框会搜到哪里，
    /// 不必等出一屏结果才发现搜错了对象。
    pub const fn placeholder(self) -> &'static str {
        match self {
            Self::CurrentDocument => "在当前文档中搜索",
            Self::OpenDocuments => "在所有打开的文档中搜索",
        }
    }
}

/// 切换器的展示顺序（文档级在前：它是默认项，也对应侧栏的分组主旨）。
pub const SEARCH_SCOPES: [SearchScope; 2] =
    [SearchScope::CurrentDocument, SearchScope::OpenDocuments];

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

/// 一次工作区搜索的结果（分组 + 是否被上限截断）。
///
/// 刻意**不**把「是否截断」藏进组合根的某个字段由搜索顺手改掉：
/// 那样「读结果」就有了隐藏副作用，而截断提示恰恰是**必须被看见**的信息
/// （用户看到截断结果会以为搜全了）。它和结果一起返回，界面不可能漏掉。
///
/// R-03 B1：本类型从 root 的 `app/frame/workspace.rs` 迁来，与 [`SearchGroup`]
/// 作伴——两者是同一份结果的内外两层，拆开住会让「结果」与「分组」分居两地。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceSearch {
    /// 按文件分组的命中。
    pub groups: Vec<SearchGroup>,
    /// 是否因超过总命中上限而被截断。
    pub truncated: bool,
}

impl WorkspaceSearch {
    /// 命中总数（各组之和）。
    pub fn total(&self) -> usize {
        self.groups.iter().map(|g| g.hits.len()).sum()
    }

    /// 是否没有任何命中。
    pub fn is_empty(&self) -> bool {
        self.total() == 0
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
    ///
    /// 值**取自 core 的 `SearchHitView::highlight`**（唯一真源，R-11 / DS-1）。
    /// 此前由本模块的 `locate_in_snippet` 在片段里重搜一遍 query 得出，
    /// 与 core 的扫描口径在四处不一致：
    /// 1. 取片段内**第一处** query——上下文里也有 query 时高亮打在上下文上；
    /// 2. 只收 `case_sensitive`，忽略 `whole_word`——整词命中可能落到非整词位置；
    /// 3. 大小写折叠用 `to_lowercase()`，与 core 的 `char_eq_ignore_case` 不同；
    /// 4. 命中被窗口截断时返回 `None` → `unwrap_or((0, 0))`，等于不高亮。
    pub highlight: (usize, usize),
}

/// 把命中按文件分组（保持文件首次出现的顺序）。
///
/// 分组而非平铺的原因与原型一致：跨文件搜索里同一文件的命中往往连成一片，
/// 平铺会让同一个文件名在结果里重复十几次，用户得自己扫着找。
///
/// 入参第 4 项是**片段内字符**高亮区间，由 core 的 `SearchHitView::highlight`
/// 直接给出（R-11 / DS-1 的唯一真源）；本函数只搬运，不重新计算——此前
/// 这里有 `locate_in_snippet`，拿片段再搜一遍 query，与 core 的口径在
/// 四处不一致（见 `SearchHitRow::highlight`）。
pub fn group_hits(hits: &[(String, usize, String, (usize, usize))]) -> Vec<SearchGroup> {
    let mut groups: Vec<SearchGroup> = Vec::new();
    for (path, line, snippet, highlight) in hits {
        let file_name = file_name_of(path).to_string();
        let row = SearchHitRow {
            line: *line,
            snippet: snippet.clone(),
            highlight: *highlight,
        };
        match groups.iter_mut().find(|g| &g.path == path) {
            Some(existing) => existing.hits.push(row),
            None => groups.push(SearchGroup {
                path: path.clone(),
                file_name,
                hits: vec![row],
            }),
        }
    }
    groups
}

/// 把领域命中直接转成展示用的分组结构（含每条命中的高亮区间）。
///
/// 分组逻辑在 [`group_hits`]，本入口只做 DTO → 元组的投影。
///
/// 高亮区间**取自 DTO**（`SearchHitView::highlight`）而不是在这里重算：
/// 本文件此前还有一个 `locate_in_snippet` 拿片段再搜一遍 query，那正是
/// DS-1 的双源——片段里取第一处命中（上下文抢高亮）、忽略 `whole_word`、
/// 大小写折叠走另一套实现、命中被窗口截断时直接放弃高亮。故本函数也**不再
/// 需要**查询词与选项参数：它们只是给那套重算用的。
pub fn group_search_hits(result: &SearchResultView) -> Vec<SearchGroup> {
    let rows: Vec<(String, usize, String, (usize, usize))> = result
        .hits
        .iter()
        .map(|hit| {
            (
                hit.path.as_str().to_string(),
                hit.line,
                hit.snippet.clone(),
                hit.highlight,
            )
        })
        .collect();
    group_hits(&rows)
}
