//! 共享内核（Shared Kernel）。
//!
//! 可被任意层依赖，但自身**不得依赖** `domain` / `application` /
//! `infrastructure` / `presentation`（见 `.agent/architecture.md`）。
//!
//! - [`error`] / [`result`]：统一错误与结果类型
//! - [`ids`]：强类型标识符
//! - [`geometry`]：文本坐标值对象
//! - [`event`]：领域事件与事件总线抽象
//! - [`dialog`]：原生对话框端口（S5-08）
//! - [`encoding`]：文本编码标签（纯值，V11-03）
//! - [`fonts`]：编辑区字体族的跨层共享契约（设置页字体选择，S6-06）
//! - [`path`]：路径值对象（`FilePath`）
//! - [`time`]：时间戳与可注入时钟
//! - [`text`]：与层无关的分词工具（V11-07）
//! - [`text_format`]：可打开的文本格式白名单（问题 045；三方共用的唯一出处）
//! - [`system`]：外部打开器与链接安全校验（S6-05 超链接点击）
//! - [`lru`]：有界 LRU 映射（D-4；高亮 / 拼写 / 渲染 / 图片四缓存共用）

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod dialog;
pub mod encoding;
pub mod error;
pub mod event;
pub mod fonts;
pub mod geometry;
pub mod ids;
pub mod lru;
pub mod path;
pub mod result;
pub mod system;
pub mod text;
pub mod text_format;
pub mod time;
