//! `pulldown-cmark` 版 Markdown 解析器（S5-02）。
//!
//! # 核心职责：把事件流转成「带源码区间的块序列」
//!
//! 领域层 `MarkdownParser` 要求每个 `RenderedBlock` 携带真实的
//! `source_range`（字节区间）。这是 `FR-02`（双向滚动同步）与 `FR-03`（增量高亮）
//! 的前提——没有它，表现层只能重解析全文来定位，性能目标（`NFR-PERF-06`
//! 10KB ≤ 5ms）无法达成。
//!
//! pulldown-cmark 的事件流是**流式**的，只有 `into_offset_iter()` 版本才携带
//! `Range<usize>`。本实现按「块级 Start 开栈、对应 End 出栈」配对来推导区间：
//! Start 的偏移是块的**开始**，End 的偏移是块的**结束**。
//!
//! # 为什么必须用栈，不能只记「上一个 Start」
//!
//! 三处让简单写法失效：
//! 1. 列表项可**嵌套**（`- a` 里嵌列表），`Item` 与 `List` 交错开闭；
//! 2. `TableHead` 之后**没有** `TableBody` 标签，表体行直接跟在后面；
//! 3. 标题虽然是自闭合语义，事件流里仍表现为 `Start` + `End` 一对。
//!
//! 故本实现维护一个显式的 `OpenBlock` 栈，End 到来时弹出栈顶配对。
//!
//! # 任务列表的定型时机
//!
//! `Tag::Item` 到底是普通列表项还是任务列表项，要看它内部是否出现
//! `Event::TaskListMarker`——该事件在 `End(Item)` **之前**才到达。
//! 因此 Item 在闭合时才能定型，故栈项上挂 `is_task` 标记。
//!
//! # 模块结构
//!
//! facade 只保留解析选项、解析器类型与「带偏移原始解析器」入口：
//! - `image_size`：CSDN 图片尺寸后缀剥离与偏移映射；
//! - `stack`：`OpenBlock` 栈帧、文本回流与行内标记扫描；
//! - `event_map`：pulldown 标签 → 领域类型的纯映射；
//! - `parse`：`MarkdownParser` 实现与事件主循环。

mod event_map;
mod image_size;
mod parse;
mod stack;

use pulldown_cmark::{DefaultBrokenLinkCallback, OffsetIter, Options, Parser};

pub use image_size::{SourceOffsets, desugar_image_sizes};

/// 解析器使用的 pulldown-cmark 选项集（对应需求文档 §3.1 的 P0 特性）。
///
/// **GFM 表格 / 任务列表 / 脚注在 pulldown-cmark 中是运行期 `Options` 位flag，
/// 而非 Cargo feature**（已在 ADR-002 记录此事实，极易误解为 feature）。
/// `ENABLE_GFM` 隐含表格与删除线，但**不含**任务列表与脚注，故仍需显式并列。
///
/// 导出器（`pulldown_html_exporter`）直接复用本函数，**这是唯一的选项来源**：
/// 早先两边各写一份，结果预览支持上下标而导出不支持（用户看到「预览有、导出没有」）。
pub fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_GFM
        // 上下标（CSDN / 掘金语法）：`H~2~O`、`2^10^`。
        // 注意 `ENABLE_SUBSCRIPT` 必须显式开——**关着时 `~x~` 会被当作删除线**
        // （pulldown 文档明确写了这个回退），于是 `H~2~O` 会显示成被划掉的 2。
        // 实测 pulldown 只在定界符**前面有空白**时才识别（`H ~2~ O` 能识别、
        // `H~2~O` 不能），故中文习惯写法仍由 `push_markup_aware` 兜底。
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_SUPERSCRIPT
        // 数学公式：`$...$`（行内）/ `$$...$$`（块级）需此选项才会被识别为
        // `Event::InlineMath` / `Event::DisplayMath`（否则按字面文本处理）。
        | Options::ENABLE_MATH
        // 定义列表（`术语` + 缩进的 `: 定义`）。不开时 pulldown 会把两行当成
        // **一个普通段落**，预览里就是「术语 A: 定义 A 的内容」糊在一行——
        // 用户写的层级结构全部丢失。
        | Options::ENABLE_DEFINITION_LIST
}

/// 基于 `pulldown-cmark` 的 Markdown 解析器。
#[derive(Clone, Copy, Debug, Default)]
pub struct PulldownMarkdownParser;

impl PulldownMarkdownParser {
    /// 构造解析器。
    pub fn new() -> Self {
        Self
    }

    /// 按当前选项集创建**带偏移**的 pulldown 解析器。
    ///
    /// 导出器（S5-09）与解析器共用同一份 [`parser_options`]，保证「预览所见」与
    /// 「导出所得」结构一致——若两者选项不同，用户会看到预览有表格而导出没有。
    ///
    /// 返回 [`OffsetIter`] 而非 `Parser`：只有它的事件流携带 `Range<usize>`，
    /// 而 `source_range` 正是 `FR-02` 的基础。
    ///
    /// 注意：调用方若关心**源码区间**，应先把文本交给 [`desugar_image_sizes`]
    /// 预处理再传进来（`parse` 就是这么做的）。
    pub fn raw_parser(text: &str) -> OffsetIter<'_, DefaultBrokenLinkCallback> {
        Parser::new_ext(text, parser_options()).into_offset_iter()
    }
}
