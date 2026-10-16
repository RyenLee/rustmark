//! Markdown 上下文的值对象（S3-09）。
//!
//! # `source_range` 为什么必须存在
//!
//! 每个渲染块都携带**源码字节区间**，这是 `FR-02`（双向滚动同步）与
//! `FR-03`（增量高亮）能否实现的决定因素。若省略，阶段 06 只能靠重新解析
//! 全文来定位，性能目标（`NFR-PERF` 解析 ≤ 5ms）无法达成。
//!
//! # 与 syntect 解耦
//!
//! [`SpanStyle`] 是**自有枚举**，绝不携带 `syntect` 的类型——否则领域层会被
//! 外部库污染（`.agent/architecture.md`：domain 禁止 `syntect`）。
//!
//! # 模块结构
//!
//! 按内聚的值对象边界拆为五个私有子模块，本文件作为 facade 全量 re-export，
//! 外部路径 `markdown::value_objects::*` 保持不变：
//! - `block`：渲染块 ID 与块类型（`BlockId` / `BlockKind`）；
//! - `table`：单元格对齐 / 列表项 / 定义列表 / GFM 表格等结构化载荷；
//! - `inline`：行内样式、行内片段、语法样式与高亮片段；
//! - `render`：渲染完成块 `RenderedBlock` 与拼写标记 `Misspelling`；
//! - `outline`：大纲条目 `OutlineItem`。

mod block;
mod inline;
mod outline;
mod render;
mod table;

pub use block::{BlockId, BlockKind, ImageAlign, ImageDisplay, InlineImage};
pub use inline::{InlineSpan, InlineStyle, SpanStyle, SyntaxSpan};
pub use outline::OutlineItem;
pub use render::{Misspelling, RenderedBlock};
pub use table::{CellAlignment, DefinitionBody, DefinitionEntry, ListItem, TableData};
