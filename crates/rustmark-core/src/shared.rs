//! 共享内核（Shared Kernel）。
//!
//! 可被任意层依赖，但自身**不得依赖** `domain` / `application` /
//! `infrastructure` / `presentation`（见 `docs/architecture-constraints.md`）。
//!
//! - [`error`] / [`result`]：统一错误与结果类型
//! - [`ids`]：强类型标识符
//! - [`geometry`]：文本坐标值对象
//! - [`event`]：领域事件与事件总线抽象
//! - [`dialog`]：原生对话框端口（S5-08）
//! - [`encoding`]：文本编码标签（纯值，V11-03）
//! - [`export_format`]：导出格式与页面尺寸（领域 / 表现 / 基础设施三方共用）
//! - [`fonts`]：编辑区字体族的跨层共享契约（设置页字体选择，S6-06）
//! - [`path`]：路径值对象（`FilePath`）
//! - [`pattern`]：迷你正则引擎（自制，见其模块文档；查找替换与跨文件搜索共用）
//! - [`time`]：时间戳与可注入时钟
//! - [`text`]：与层无关的分词工具（V11-07）
//! - [`text_format`]：可打开的文本格式白名单（问题 045；三方共用的唯一出处）
//! - [`theme`]：主题种类（领域 / 应用 / 表现三方共用；R-09 收敛双枚举）
//! - [`system`]：外部打开器与链接安全校验（S6-05 超链接点击）
//! - [`image_asset`]：图片资源导入端口（R-04b 插入图片）
//! - [`lru`]：有界 LRU 映射（D-4；高亮 / 拼写 / 渲染 / 图片四缓存共用）

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod dialog;
pub mod encoding;
pub mod error;
pub mod event;
pub mod export_format;
pub mod fonts;
pub mod geometry;
pub mod ids;
pub mod image_asset;
pub mod lru;
pub mod path;
pub mod pattern;
pub mod result;
pub mod system;
pub mod text;
pub mod text_format;
pub mod theme;
pub mod time;
