//! 表现层的非视图部分：UI State、主题、富文本、小组件与快捷键（阶段 06）。
//!
//! 视图（`views`）留在 root crate——它依赖组合根 `app`，而 core 不允许
//! 反向依赖 root（D-3 阶段 4a 的 crate 边界）。
//!
//! V11-58：`icons`（向量图标资源）抽去独立 crate `rustmark-icons`——
//! core 内零消费者，root 经 `pub use rustmark_icons as icons;` 拼回原路径。

pub mod rich;
pub mod shortcuts;
pub mod state;
pub mod theme;
pub mod widgets;
