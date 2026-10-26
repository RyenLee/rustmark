//! 行内富文本：span → `TextFormat`、`LayoutJob` 排版与分隔符规范化。
//!
//! # 模块结构（D-4 阈值拆分，2026-10-26）
//!
//! facade 只保留入口与跨模块可见的 API 再导出；具体实现拆到子模块：
//!
//! - [`text_format`]：预览区行内样式 → `TextFormat`、`Typography`、`done_overlay`；
//! - [`text_render`]：行内文本渲染（含可点击命中区）、`build_text_job`、`handle_span_click`；
//! - [`math`]：含行内公式的段落分段流式排版；
//! - [`delimiters`]：`$…$` / `[^…]` 分隔符剥离。

mod delimiters;
mod math;
mod text_format;
mod text_render;

pub use delimiters::{footnote_ref_mark, strip_math_delimiters};
pub(super) use text_format::Typography;
pub use text_format::span_format;
pub use text_render::handle_span_click;
pub(super) use text_render::render_inline_text;
// 以下两项仅被 `#[cfg(test)]` 的 `tests.rs` 消费，故同口径门控，避免非测试构建的未用告警。
#[cfg(test)]
pub(super) use text_format::done_overlay;
#[cfg(test)]
pub(super) use text_render::build_text_job;
