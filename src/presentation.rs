//! 表现层：egui 视图、UI State、主题与快捷键（阶段 06）。

pub mod views;

// D-3 阶段 4a：非视图子模块 re-export 拼回 `crate::presentation::<子模块>`
// 原路径（views 依赖组合根 `app`，只能留 root）。V11-62（动作 2，批次 1-5，
// 2026-10-25 收官）把五个非视图子层 `widgets` / `state` / `shortcuts` / `theme` /
// `rich` **全部**抽去独立 crate `rustmark-ui`——core 内零消费者，root 经本文件
// 把它们 re-export 回 `crate::presentation::<子模块>` 原路径，存量视图与集成测试零改动。
// 桥接路径与 ui 侧同形（`rustmark_ui::presentation::<子模块>`）。
pub use rustmark_ui::presentation::{rich, shortcuts, state, theme, widgets};

// V11-58：向量图标资源（16×16 规格数据 + Painter 渲染器）抽去独立 crate
// `rustmark-icons`——core 内零消费者，住 core 里没有 core 的职责。
// 把 crate 本身 re-export 成 `icons` 模块：`crate::presentation::icons::*`
// 公开路径不变，存量视图（toolbar_view）与测试零改动。
pub use rustmark_icons as icons;
