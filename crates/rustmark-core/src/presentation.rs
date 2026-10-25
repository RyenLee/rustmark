//! 表现层的非视图部分：UI State、主题、富文本与快捷键（阶段 06）。
//!
//! 视图（`views`）留在 root crate——它依赖组合根 `app`，而 core 不允许
//! 反向依赖 root（D-3 阶段 4a 的 crate 边界）。
//!
//! V11-58：`icons`（向量图标资源）抽去独立 crate `rustmark-icons`——
//! core 内零消费者，root 经 `pub use rustmark_icons as icons;` 拼回原路径。
//!
//! V11-62 批次 1：`widgets`（egui 布局不变式封装，T8）抽去独立 crate
//! `rustmark-ui`——core 内零消费者，root 经 `pub use rustmark_ui::widgets;`
//! 拼回原路径。
//!
//! V11-62 批次 2：`state`（UI State，24 文件 / 6,682 行）抽去 `rustmark-ui`。
//! 它是表现层内部依赖的枢纽（theme / shortcuts 均被 state 读取，反向零引用），
//! 故按依赖拓扑先于 theme / shortcuts 搬迁——批次序由 DAG 定，不由行数定。
//! V11-62 批次 3：`shortcuts`（5 文件 / 892 行）抽去 `rustmark-ui`——纯叶
//! （仅依赖 `application::dto`），不引用 theme/state/rich，顺序自由。
//! V11-62 批次 4：`theme`（主题，11 文件 / 3,487 行）抽去 `rustmark-ui`——纯叶
//! （仅依赖 `shared::theme` / `application::dto` / `shared::geometry`），顺序自由。
//! 至此 core 的 `presentation` 仅剩 `rich`（批次 5 计划迁出）。

pub mod rich;
