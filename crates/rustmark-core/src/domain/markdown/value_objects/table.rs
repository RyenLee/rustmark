//! 结构化块载荷值对象：[`CellAlignment`] / [`ListItem`] / [`DefinitionBody`]
//! / [`DefinitionEntry`] / [`TableData`]。
//!
//! 列表、定义列表、表格的结构是**解析期**才知道的，必须由领域层持有，
//! 到表现层才能画成复选框 / 定义序列 / 网格。

use crate::shared::geometry::TextRange;

use super::block::InlineImage;
use super::inline::InlineSpan;

/// 表格单元格的水平对齐方式（GFM 分隔行 `:` 决定）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CellAlignment {
    /// 未指定（按源写作惯例：表头居中、表体左对齐）。
    #[default]
    None,
    /// 左对齐（`:---`）。
    Left,
    /// 居中（`:---:`）。
    Center,
    /// 右对齐（`---:`）。
    Right,
}

/// 列表的一项。
///
/// # 为什么要单独存 `source_range`
///
/// 任务列表的「点击勾选」要回写源码，就必须知道该行在文档里的字节区间——
/// 否则 UI 只能拿着文本去全文里模糊匹配，既慢又会改错同名项。
///
/// # 为什么 `depth` 与 `ordinal` 在**项**上而不是在列表上
///
/// 嵌套列表在事件流里是一串交错的 `List` / `Item` 开闭，若每个 `List`
/// 各产出一个块，同一层级结构会被切成多个互不相干的块，且父项的文本会被
/// 子项的文本污染（子项文本经「并入最近的列表项」这条规则回流到父项）。
/// 故解析器把**整棵列表树拍平成一个块**：每个项自带缩进层级与序号，
/// 顺序即文档顺序。渲染方按 `depth` 缩进、按 `ordinal` 编号即可还原层级。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListItem {
    /// 该项的纯文本。
    text: String,
    /// 勾选状态：`Some(true)` 已勾选、`Some(false)` 未勾选、`None` 表示普通列表项。
    checked: Option<bool>,
    /// 该项在源码中的字节区间。
    source_range: TextRange,
    /// 缩进层级（顶层列表项为 0）。
    depth: u8,
    /// 有序列表的序号；无序项为 `None`。
    ///
    /// 存在项上而非由渲染方按位置推算：同一层级可能并列多个列表
    /// （如 `1.` 列表里嵌 `1.` 子列表，子列表结束后父列表继续编号），
    /// 按位置推算会串号。
    ordinal: Option<u64>,
    /// 该项文本内的行内样式片段（相对 `text` 的字节区间）。
    spans: Vec<InlineSpan>,
    /// 该项文本里混排的图片（区间相对 `text`；见 [`InlineImage`]）。
    ///
    /// 列表项是**文本容器**而非块：它里面的图片没法像段落那样升级成独立的
    /// 图片块（列表整体才是一个块），只能挂在这里由渲染方插回项文本里；
    /// 不挂的话 `- ![图标](u.png) 说明` 里的图标会凭空消失。
    images: Vec<InlineImage>,
}

impl ListItem {
    /// 构造列表项（顶层、无序号、无行内样式）。
    pub fn new(text: impl Into<String>, checked: Option<bool>, source_range: TextRange) -> Self {
        Self {
            text: text.into(),
            checked,
            source_range,
            depth: 0,
            ordinal: None,
            spans: Vec::new(),
            images: Vec::new(),
        }
    }

    /// 设置缩进层级。
    pub(crate) fn with_depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    /// 设置有序列表序号。
    pub(crate) fn with_ordinal(mut self, ordinal: u64) -> Self {
        self.ordinal = Some(ordinal);
        self
    }

    /// 设置行内样式片段。
    pub(crate) fn with_spans(mut self, spans: Vec<InlineSpan>) -> Self {
        self.spans = spans;
        self
    }

    /// 设置项文本里混排的图片。
    pub fn with_images(mut self, images: Vec<InlineImage>) -> Self {
        self.images = images;
        self
    }

    /// 该项的纯文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 勾选状态（`None` 表示非任务项）。
    pub const fn checked(&self) -> Option<bool> {
        self.checked
    }

    /// 该项在源码中的字节区间。
    pub const fn source_range(&self) -> TextRange {
        self.source_range
    }

    /// 缩进层级（顶层为 0）。
    pub const fn depth(&self) -> u8 {
        self.depth
    }

    /// 有序列表序号（无序项为 `None`）。
    pub const fn ordinal(&self) -> Option<u64> {
        self.ordinal
    }

    /// 行内样式片段。
    pub fn spans(&self) -> &[InlineSpan] {
        &self.spans
    }

    /// 项文本里混排的图片。
    pub fn images(&self) -> &[InlineImage] {
        &self.images
    }
}

/// 定义列表里**一条术语的一个定义**（一条术语可以有多条定义）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DefinitionBody {
    /// 定义正文。
    text: String,
    /// 定义正文内的行内样式片段（相对 `text`）。
    spans: Vec<InlineSpan>,
}

impl DefinitionBody {
    /// 构造定义正文。
    pub fn new(text: impl Into<String>, spans: Vec<InlineSpan>) -> Self {
        Self {
            text: text.into(),
            spans,
        }
    }

    /// 定义正文。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 行内样式片段。
    pub(crate) fn spans(&self) -> &[InlineSpan] {
        &self.spans
    }
}

/// 定义列表里的一条术语及其定义。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DefinitionEntry {
    /// 术语（`术语` 那一行）。
    term: String,
    /// 术语里的行内样式片段（相对 `term`）。
    term_spans: Vec<InlineSpan>,
    /// 该术语的定义（`:` 开头的行；可有多条）。
    definitions: Vec<DefinitionBody>,
}

impl DefinitionEntry {
    /// 构造条目。
    pub fn new(term: impl Into<String>, term_spans: Vec<InlineSpan>) -> Self {
        Self {
            term: term.into(),
            term_spans,
            definitions: Vec::new(),
        }
    }

    /// 追加一条定义。
    pub(crate) fn push_definition(&mut self, body: DefinitionBody) {
        self.definitions.push(body);
    }

    /// 术语。
    pub fn term(&self) -> &str {
        &self.term
    }

    /// 术语的行内样式片段。
    pub(crate) fn term_spans(&self) -> &[InlineSpan] {
        &self.term_spans
    }

    /// 该术语的全部定义。
    pub fn definitions(&self) -> &[DefinitionBody] {
        &self.definitions
    }
}

/// GFM 表格的结构化内容。
///
/// # 单元格的行内样式为什么另存一份
///
/// `header` / `rows` 是**扁平纯文本**（搜索、大纲、字数都按它算），
/// 而单元格里的 `**粗体**` 需要位置信息才能渲染。两者是「同一份内容的不同投影」，
/// 故平行保存而非把单元格换成带样式的结构体——后者会让所有
/// 「拿单元格当字符串比」的调用点（含测试）全部改写，收益却只是少一层数组。
/// `header_spans[c]` 对应 `header[c]`，`rows_spans[r][c]` 对应 `rows[r][c]`。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TableData {
    /// 表头单元格。
    header: Vec<String>,
    /// 表体各行（每行长度可能不足列数——GFM 允许行末缺单元格）。
    rows: Vec<Vec<String>>,
    /// 每列的对齐方式（长度对应 `header`）。
    aligns: Vec<CellAlignment>,
    /// 表头各单元格的行内样式片段。
    header_spans: Vec<Vec<InlineSpan>>,
    /// 表体各单元格的行内样式片段。
    rows_spans: Vec<Vec<Vec<InlineSpan>>>,
}

impl TableData {
    /// 构造表格数据。
    pub fn new(header: Vec<String>, rows: Vec<Vec<String>>, aligns: Vec<CellAlignment>) -> Self {
        // 对齐数组与列数对齐：GFM 里「列数」以表头为准，缺少的按 `None` 补。
        let mut aligns = aligns;
        aligns.resize(header.len(), CellAlignment::None);
        Self {
            header,
            rows,
            aligns,
            header_spans: Vec::new(),
            rows_spans: Vec::new(),
        }
    }

    /// 附带单元格行内样式（构造链）。
    pub(crate) fn with_cell_spans(
        mut self,
        header_spans: Vec<Vec<InlineSpan>>,
        rows_spans: Vec<Vec<Vec<InlineSpan>>>,
    ) -> Self {
        self.header_spans = header_spans;
        self.rows_spans = rows_spans;
        self
    }

    /// 表头单元格。
    pub fn header(&self) -> &[String] {
        &self.header
    }

    /// 表体行。
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// 列对齐方式。
    pub fn aligns(&self) -> &[CellAlignment] {
        &self.aligns
    }

    /// 第 `c` 列表头的行内样式片段（未记录时为空切片）。
    pub fn header_spans(&self, c: usize) -> &[InlineSpan] {
        self.header_spans.get(c).map_or(&[], Vec::as_slice)
    }

    /// 第 `r` 行第 `c` 列的行内样式片段（未记录时为空切片）。
    pub fn cell_spans(&self, r: usize, c: usize) -> &[InlineSpan] {
        self.rows_spans
            .get(r)
            .and_then(|row| row.get(c))
            .map_or(&[], Vec::as_slice)
    }

    /// 列数（以表头为准）。
    pub fn columns(&self) -> usize {
        self.header.len()
    }
}
