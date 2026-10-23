//! 表现层：egui 视图、UI State、主题与快捷键（阶段 06）。

pub mod views;

// D-3 阶段 4a：非视图子模块住在 `rustmark-core`，re-export 拼回
// `crate::presentation::<子模块>` 原路径（views 依赖组合根 `app`，只能留 root）。
pub use rustmark_core::presentation::{icons, rich, shortcuts, state, theme, widgets};
