//! 文档上下文的 DTO：区间 / 文档视图 / 统计 / 大纲 / 搜索命中。

use serde::{Deserialize, Serialize};

use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

/// 文本区间（字节偏移，左闭右开），DTO 版本（与领域 [`crate::shared::geometry::TextRange`] 对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RangeView {
    /// 起点字节偏移。
    pub start: usize,
    /// 终点字节偏移（不含）。
    pub end: usize,
}

impl RangeView {
    /// 构造区间。
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 区间长度（字节）。
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// 是否为空区间。
    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// 单个打开文档的视图。
#[derive(Clone, Debug, Serialize)]
pub struct DocumentView {
    /// 文档 ID。
    pub id: DocumentId,
    /// 标题（文件名去扩展名，或「未命名」）。
    pub title: String,
    /// 保存路径；未保存过的新文档为 `None`。
    pub path: Option<FilePath>,
    /// 内容预览（前若干字符，用于标签页 tooltip / 欢迎页）。
    pub content_preview: String,
    /// 是否存在未保存修改。
    pub is_dirty: bool,
    /// 光标字节偏移。
    pub cursor: usize,
    /// 选区（无选区为 `None`）。
    pub selection: Option<RangeView>,
    /// 行数。
    pub line_count: usize,
    /// 字节长度。
    pub byte_len: usize,
}

/// 打开文档的**轻量摘要**（标签条 / 打开列表 / 脏标记判定用）。
///
/// # 为什么不直接用 [`DocumentView`]（问题 043）
///
/// 构造 `DocumentView` 必须先 `doc.text()`——那是 rope 的 O(全文) 快照，
/// 为的是填 `content_preview` 与 `line_count`。而标签条每帧只需要
/// 标题 / 脏标记 / 路径，却按打开文档数 N 各付一次全文克隆：
/// 10 个 100KB 文档 ≈ 每帧 1MB 分配 + 10 次全文档扫描，这正是
/// 「打开多个文档后明显卡顿」的主因。摘要只读聚合元数据，构造 O(1)。
#[derive(Clone, Debug, Serialize)]
pub struct TabSummary {
    /// 文档 ID。
    pub id: DocumentId,
    /// 标题（文件名去扩展名，或「未命名」）。
    pub title: String,
    /// 保存路径；未保存过的新文档为 `None`。
    pub path: Option<FilePath>,
    /// 是否存在未保存修改。
    pub is_dirty: bool,
    /// 字节长度。
    pub byte_len: usize,
}

/// 文档字数统计视图（字段对齐原型 `StatsDialog.vue`）。
///
/// # 口径必须与原型逐项一致
///
/// 状态栏的字数与统计浮层都直接显示这些数字，用户会拿它跟原型比。
/// 口径不一致不是「实现细节差异」而是**可见的错误**：原型里
/// 「这是一段中文 mixed with English」算 6 + 6 = 12 字，
/// 按空白切分只算 6 —— 差一倍。
#[derive(Clone, Debug, Serialize)]
pub struct StatsView {
    /// 字数：**中日韩按字 + 西文按词**（原型的 `countWords`）。
    ///
    /// 不能用 `split_whitespace().count()`：中文整段无空白，
    /// 一整篇中文会被算成 1 个「词」。
    pub words: usize,
    /// 字符数（含空格，Unicode 标量值计数）。
    pub characters: usize,
    /// 字符数（不含任何空白）。
    pub characters_no_space: usize,
    /// 行数。
    pub lines: usize,
    /// 段落数（非空段落：被空行分隔的连续非空行各算一段）。
    pub paragraphs: usize,
    /// 标题数（`#` ~ `######`，围栏代码块**内**的不计）。
    pub headings: usize,
    /// 围栏代码块数。
    pub code_blocks: usize,
    /// 链接数（`[文字](地址)`，**不含**图片）。
    pub links: usize,
    /// 图片数（`![说明](地址)`）。
    pub images: usize,
    /// 标题。
    pub title: String,
    /// 已完成任务数（`- [x]`）。
    pub tasks_done: usize,
    /// 任务总数（`- [ ]` + `- [x]`）。
    pub tasks_total: usize,
    /// 预计阅读时长（分钟）。
    ///
    /// 原型口径 `max(1, round(words / 300))`：中文文档普遍读得慢
    /// （300 字/分是偏快的估计），但**下限为 1** —— 一篇只有几个字
    /// 的草稿显示「约 0 分钟」比显示「约 1 分钟」更让人困惑。
    pub reading_minutes: u32,
}

/// 大纲条目视图。
#[derive(Clone, Debug, Serialize)]
pub struct OutlineItemView {
    /// 标题级别（1–6）。
    pub level: u8,
    /// 标题文本（已去 `#` 标记）。
    pub text: String,
    /// 源码行号（0-based）。
    pub line: usize,
}

/// 大纲视图（标题列表）。
#[derive(Clone, Debug, Serialize)]
pub struct OutlineView(pub Vec<OutlineItemView>);

/// 跨文件搜索命中视图。
#[derive(Clone, Debug, Serialize)]
pub struct SearchHitView {
    /// 命中所在文档 ID（磁盘文件由路径派生稳定 ID）。
    pub file_id: DocumentId,
    /// 命中所在路径。
    pub path: FilePath,
    /// 命中所在行（0-based）。
    pub line: usize,
    /// 命中所在列（0-based）。
    pub col: usize,
    /// 命中上下文片段（含前后若干字符）。
    pub snippet: String,
    /// 命中区间（相对整篇内容的字节偏移）。
    pub match_range: RangeView,
}
