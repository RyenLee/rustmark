//! 共享内核的**值对象**分组（R-02 二分，2026-10-27；R-10 项 A，2026-10-28）。
//!
//! 这一组是跨层共用的纯值对象：主题种类、字体族、导出格式。
//! 它们被 domain / application / infrastructure / presentation 多层共用，
//! 自身零 IO、零业务归属层。
//!
//! 端口（dialog / event / system / image_asset / time）已迁至独立的
//! `rustmark-ports` crate（R-10 项 A，契约归领域、实现归 infra）——见
//! `docs/spec/architecture-constraints.md` §4.5 登记表路径 `ports/...`。
//!
//! 落点保留在 `shared/kernel/`，由 `shared.rs` 以 `pub use kernel::*` 拼回
//! `shared::<名>`，调用点零改动（与 V11-57 / V11-58 / V11-62 同款桥接）。

pub mod export_format;
pub mod fonts;
pub mod theme;
