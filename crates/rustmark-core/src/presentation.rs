//! 表现层的非视图部分：UI State、主题、富文本、小组件、图标与快捷键（阶段 06）。
//!
//! 视图（`views`）留在 root crate——它依赖组合根 `app`，而 core 不允许
//! 反向依赖 root（D-3 阶段 4a 的 crate 边界）。

pub mod icons;
pub mod rich;
pub mod shortcuts;
pub mod state;
pub mod theme;
pub mod widgets;
