//! 共享内核（Shared Kernel）的模块根——`rustmark_shared::shared`（TD-04 抽出）。
//!
//! 可被任意层依赖，但自身**不得依赖** `domain` / `application` /
//! `infrastructure` / `presentation`（见 `docs/architecture-constraints.md`）。
//! 这一条不再只靠词法守卫：crate 的 `Cargo.toml` 只有 serde / thiserror /
//! ahash 三个纯数据依赖，反向依赖任何层都会在 Cargo 解析期成环报错。
//!
//! `rustmark-core` 以 `pub use rustmark_shared::shared;` 桥接，公开路径
//! `rustmark_core::shared::…` 与拆分前逐字一致（V11-58 / V11-62 同款桥接）。
//! 共享内核的准入门槛仍是 §4.5 三问 + 登记表，守卫在 `check-arch` 的
//! `scan_project`——登记键沿用 `shared/<文件名>`，搬迁不改变登记口径。
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
//! - [`constants`]：跨层共用的阈值常量（大文件 / 防抖 / 滚动容差，单一真源）

// 测试代码允许 unwrap / expect（规范 §七-20）；生产代码保持 deny。
// lint 级别声明在 crate 根 `lib.rs`（模块级重复声明会与 crate 级 cfg_attr
// 叠加出歧义，且 clippy 只认最外层配置）。

pub mod constants;
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
