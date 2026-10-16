//! Markdown 预览上下文的 DTO：块 / 表格 / 列表 / 定义列表 / 渲染结果 / 高亮片段。

use serde::Serialize;

use super::document::RangeView;

/// 表格单元格的水平对齐方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum CellAlignView {
    /// 未指定。
    None,
    /// 左对齐。
    Left,
    /// 居中。
    Center,
    /// 右对齐。
    Right,
}

/// 预览用表格结构。
#[derive(Clone, Debug, Serialize)]
pub struct TableView {
    /// 表头单元格。
    pub header: Vec<String>,
    /// 表体各行。
    pub rows: Vec<Vec<String>>,
    /// 每列对齐方式（长度对应 `header`）。
    pub aligns: Vec<CellAlignView>,
    /// 表头各单元格的行内样式片段（与 `header` 同形；区间相对该单元格文本）。
    ///
    /// 与 [`TableView::header`] 并列而不是塞进一个结构体：单元格文本早已被
    /// 多处调用方按 `Vec<String>` 读取（导出、搜索、测试断言），
    /// 换成 `Vec<Cell>` 会波及所有调用点；并行的片段表是**增量**改动。
    pub header_spans: Vec<Vec<SpanView>>,
    /// 表体各行各单元格的行内样式片段（与 `rows` 同形）。
    pub rows_spans: Vec<Vec<Vec<SpanView>>>,
}

/// 预览用列表项。
#[derive(Clone, Debug, Serialize)]
pub struct ListItemView {
    /// 该项纯文本（不含任务标记）。
    pub text: String,
    /// 勾选状态：`None` 表示普通列表项。
    pub checked: Option<bool>,
    /// 该项在源码中的字节区间起点。
    pub start: usize,
    /// 该项在源码中的字节区间终点（不含）。
    pub end: usize,
    /// 缩进层级（顶层为 `0`；嵌套列表按层递增）。
    ///
    /// **必须由解析层给出**：pulldown 的事件流里嵌套深度只体现为
    /// `List` 标签的开闭层次，块一旦被"拍平"成单个列表块（见解析器的
    /// `root_list_index`），层级信息就只存在于这个字段里了。
    /// 渲染层若想用「文本前导空格数」反推，会在 `1.` 与 `-` 混排时算错。
    pub depth: u8,
    /// 有序项的编号；无序项为 `None`。
    ///
    /// 不自己数序号的原因：有序列表可从任意数字起（`3.` 开头），
    /// 且中间可能被嵌套列表打断，渲染层从 1 开始数会与源文不符。
    pub ordinal: Option<u64>,
    /// 该项文本的行内样式片段（区间相对 [`Self::text`]）。
    pub spans: Vec<SpanView>,
    /// 该项文本里混排的图片（区间相对 [`Self::text`]）。
    ///
    /// 列表项是文本容器而非块，它里面的图片无法升级成独立图片块，
    /// 只能挂在这里由渲染方插回项文本里（否则 `- ![图标](u.png) 说明`
    /// 的图标会整张消失）。
    pub images: Vec<InlineImageView>,
}

/// 定义列表的**单条定义正文**视图。
#[derive(Clone, Debug, Serialize)]
pub struct DefinitionBodyView {
    /// 定义正文文本。
    pub text: String,
    /// 正文的行内样式片段（区间相对 `text`）。
    pub spans: Vec<SpanView>,
}

/// 定义列表的**一个词条**视图（一个术语 + 若干条定义）。
#[derive(Clone, Debug, Serialize)]
pub struct DefinitionEntryView {
    /// 术语（`术语` 行）。
    pub term: String,
    /// 术语的行内样式片段（区间相对 `term`）。
    pub term_spans: Vec<SpanView>,
    /// 该术语下的各条定义（`: 定义` 行，可多条）。
    pub definitions: Vec<DefinitionBodyView>,
}

/// 一张图片的显示参数（图片块与行内图片共用）。
///
/// 字段与 [`BlockView`] 的图片字段同形：图片块（`kind == "image"`）与
/// **混排在文字里的图片**（`前面 ![a](u.png) 后面`，见 [`InlineImageView`]）
/// 走的是同一套尺寸 / 对齐参数，只是后者还要带上它在文字里的位置。
#[derive(Clone, Debug, Serialize)]
pub struct ImageSpecView {
    /// 图片地址（本地路径 / 远程 URL / data URI）。
    pub url: String,
    /// 替代文本。
    pub alt: String,
    /// 声明宽度（像素；源码写了 `=30x30` 时有值）。
    pub width: Option<u32>,
    /// 声明高度（像素）。
    pub height: Option<u32>,
    /// 对齐：`center` / `left` / `right`（未写标记为 `left`）。
    pub align: String,
}

/// 混排在文字里的图片（区间相对所属文本）。
///
/// 图片是**行内**元素：它可以出现在段落 / 标题 / 引用 / 列表项的文字中间。
/// 这类图片不能再升级成独立图片块（那样图片前后的文字会整段消失），
/// 故以本结构挂在文本上，渲染方按 `range` 把图片插回文字流的对应位置。
#[derive(Clone, Debug, Serialize)]
pub struct InlineImageView {
    /// 图片在所属文本里的字节区间（区间内的文字就是 alt，渲染时被图片取代）。
    pub range: RangeView,
    /// 图片本身。
    pub image: ImageSpecView,
}

/// 渲染块视图（预览区的单个块）。
///
/// # 为什么不是枚举
///
/// 领域层的 [`crate::domain::markdown::value_objects::BlockKind`] 是带载荷的枚举，
/// 但 DTO 用 **`kind` 字符串 + 可选字段**表达同样的信息：
/// 表现层按 `kind` 分发渲染器，再按需要读取对应字段。
/// 这样既能承载表格 / 列表结构，又不会把领域类型引进表现层（`check-arch` 的硬约束）。
#[derive(Clone, Debug, Serialize)]
pub struct BlockView {
    /// 块 ID（单次解析内唯一）。
    pub id: u64,
    /// 块类型（如 `heading` / `paragraph` / `code_block` / `table` / `task_list`）。
    pub kind: String,
    /// 块纯文本内容。
    pub content: String,
    /// 标题级别（1–6；`kind == "heading"` 时有值）。
    pub level: Option<u8>,
    /// 代码块语言（`kind == "code_block"` 且源码写了语言时有值）。
    pub lang: Option<String>,
    /// 列表是否有序（`kind` 为 `list` / `task_list` 时有意义）。
    pub ordered: bool,
    /// 列表项（`kind` 为 `list` / `task_list` 时有值）。
    pub items: Vec<ListItemView>,
    /// 表格结构（`kind == "table"` 时有值）。
    pub table: Option<TableView>,
    /// 图片替代文本（`kind == "image"` 时有值）。
    pub alt: Option<String>,
    /// 图片地址（本地路径 / 远程 URL / data URI；`kind == "image"` 时有值）。
    pub url: Option<String>,
    /// 图片声明宽度（像素；`kind == "image"` 且源码写了 `=30x30` 时有值）。
    ///
    /// Typora / CSDN 私有扩展（问题 3）：渲染层据此区分每张图的展示尺寸，
    /// 而不是所有图片都按纹理实际尺寸渲染。
    pub image_width: Option<u32>,
    /// 图片声明高度（像素；`kind == "image"` 且源码写了 `=30x30` 时有值）。
    pub image_height: Option<u32>,
    /// 图片对齐（`center` / `left` / `right`；`kind == "image"` 时有值）。
    ///
    /// 来自源码里的 `#pic_center` / `#pic_left` / `#pic_right` 标记；
    /// 未写标记时为 `left`——CSDN / Typora 的无标记图片即左对齐
    /// （效果截图里「带尺寸的图片」贴在左缘），要居中得显式写标记。
    pub image_align: Option<String>,
    /// 未支持扩展块的标记（`kind` 形如 `raw:html`）。
    pub marker: Option<String>,
    /// 是否块级公式（`kind == "math"` 时有值；`true` 为 `$$...$$`，`false` 为 `$...$`）。
    pub display: Option<bool>,
    /// 图表引擎名（`kind == "diagram"` 时有值，当前恒为 `"mermaid"`）。
    pub diagram: Option<String>,
    /// 脚注标签（`kind == "footnote"` 时有值）。
    ///
    /// 解析器把 `[^1]: 内容` 收成一个 `footnote` 块，**正文放在 `content`**，
    /// 标签单列在此：渲染时要画成「`1.` 正文」而不是把标签再拼回正文里
    /// （正文里本来只有 `内容`，不含 `[^1]`）。
    pub label: Option<String>,
    /// 定义列表词条（`kind == "definition_list"` 时有值）。
    pub definitions: Vec<DefinitionEntryView>,
    /// 块内行内样式片段（区间相对 `content`；无样式时为空）。
    ///
    /// 复用 [`SpanView`] 的形状（区间 + 样式字符串）：它与编辑区高亮的
    /// 数据结构同形，表现层只需一套「按区间切段渲染」的代码。
    pub spans: Vec<SpanView>,
    /// 混排在文字里的图片（区间相对 `content`；无图片时为空）。
    ///
    /// 只在「块里除图片外还写着别的字」时才有值（问题 032）：整块就一张图时
    /// 块本身已是 `kind == "image"`，走 [`BlockView::url`] 那套字段。
    pub images: Vec<InlineImageView>,
}

/// 渲染结果视图（预览区整体）。
#[derive(Clone, Debug, Serialize)]
pub struct RenderedView {
    /// 渲染块序列。
    pub blocks: Vec<BlockView>,
    /// 对应的源文档版本号。
    pub version: u64,
    /// 是否因源过大而截断（UI 据此降级）。
    pub truncated: bool,
}

/// 语法高亮片段视图。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SpanView {
    /// 片段字节区间（相对被高亮文本起点）。
    pub range: RangeView,
    /// 样式类别（如 `keyword` / `string` / `heading`）。
    pub style: String,
    /// 链接目标（`style == "link"` 时有值；其余为 `None`）。
    ///
    /// 预览点击链接时据此打开——这是「超链接不可点击」的修复关键：
    /// 旧实现 `SpanView` 只有 `range + style`，链接目标在解析侧被丢弃，
    /// 渲染层根本拿不到地址（见 `view_mapper` 透传逻辑）。
    pub url: Option<String>,
}
