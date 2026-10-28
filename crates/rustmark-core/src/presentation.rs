//! 表现层的非视图部分（阶段 06）。
//!
//! 历史上本模块承载 `widgets` / `state` / `shortcuts` / `theme` / `rich` 五个
//! 非视图子层。V11-62（动作 2）按依赖拓扑把它们**全部**分批迁入独立 crate
//! `rustmark-ui`：
//!
//! - 批次 1（2026-10-24）：`widgets`
//! - 批次 2（2026-10-25）：`state`
//! - 批次 3（2026-10-25）：`shortcuts`
//! - 批次 4（2026-10-25）：`theme`
//! - 批次 5（2026-10-25）：`rich`（V11-62 收官）
//!
//! 至此 core 的 `presentation` **不再持有任何子模块**——本文件退化为 doc-only
//! 空壳，仅保留 `pub mod presentation;` 以维持 crate 路径稳定（root 经
//! `rustmark_ui::presentation::*` 桥接拼回 `crate::presentation::<子模块>` 原路径，
//! 见 `src/presentation.rs`）。视图（`views`）自始留在 root crate。
//!
//! 架构真源见 `docs/spec/architecture-constraints.md` §7 / §10.1 与
//! `docs/archive/architecture-reviews/core-refactor-tasklist-2026-10-24.md` 的 V11-62 节。
