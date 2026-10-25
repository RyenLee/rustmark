//! 表现层：egui 视图、UI State、主题与快捷键（阶段 06）。

pub mod views;

// D-3 阶段 4a：非视图子模块住在 `rustmark-core`，re-export 拼回
// `crate::presentation::<子模块>` 原路径（views 依赖组合根 `app`，只能留 root）。
pub use rustmark_core::presentation::{rich, shortcuts, state, theme};

// V11-62 批次 1：widgets（egui 布局不变式封装，T8）抽去独立 crate
// `rustmark-ui`——core 内零消费者，把该 crate 的模块 re-export 回
// `crate::presentation::widgets` 原路径，存量视图（8 个 view 文件）与
// 集成测试零改动。ui 依赖 core（`row::paint_row_background` 读 `Theme` 的
// accent_soft / bg_tertiary），root → ui → core 与 root → core 两条边并存，
// 与 icons → actions 在 actions 汇合，无环。
// 桥接路径与 core 侧同形（`rustmark_ui::presentation::<子模块>`）：
// 批次 2-5 迁入 state / theme / rich / shortcuts 时只改这一行的花括号列表。
pub use rustmark_ui::presentation::widgets;

// V11-58：向量图标资源（16×16 规格数据 + Painter 渲染器）抽去独立 crate
// `rustmark-icons`——core 内零消费者，住 core 里没有 core 的职责。
// 把 crate 本身 re-export 成 `icons` 模块：`crate::presentation::icons::*`
// 公开路径不变，存量视图（toolbar_view）与测试零改动。
pub use rustmark_icons as icons;
