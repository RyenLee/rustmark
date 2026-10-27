//! 共享内核的**端口与值对象**分组（R-02 二分，2026-10-27）。
//!
//! 这一组是跨层契约的「真源」：原生对话框端口、事件总线、外部打开器端口、
//! 图片导入端口、可注入时钟，以及主题种类、字体族、导出格式等纯值对象。
//! 它们被 domain / application / infrastructure / presentation 多层共用，
//! 自身零 IO、零业务归属层。
//!
//! 落点保留在 `shared/kernel/`，由 `shared.rs` 以 `pub use kernel::*` 拼回
//! `shared::<名>`，调用点零改动（与 V11-57 / V11-58 / V11-62 同款桥接）。

pub mod dialog;
pub mod event;
pub mod export_format;
pub mod fonts;
pub mod image_asset;
pub mod system;
pub mod theme;
pub mod time;
