//! # rustmark-ui —— 表现层非视图部分（V11-62 从 core 分批抽出）
//!
//! 批次 1 只搬 `widgets`：egui 布局不变式的封装层（T8）——同一配方在 ≥2 处
//! 出现、写错不报错、正确写法与直觉写法不同的三类东西收在这里，每条封装
//! 对应至少一个已归档缺陷。state / theme / rich / shortcuts 按 V11-62 批次
//! 计划随后迁入；views 按 ADR-013 决策 3 **留 root**（见
//! `docs/architecture-constraints.md` §10.1 硬约束 2：presentation 外移采用
//! 「views 留 root 的中间 crate」形态，否则 R-11/R-12 的跨 crate 字段保护
//! 会被降级成同 crate 内约束）。
//!
//! # 视图契约住 ui，视图本体留 root（R-03 B1）
//!
//! `presentation::view_context::EditorViewContext`（视图窄接口）住在本 crate，
//! 但**视图本体不迁进来**——ADR-013 决策 3 已把 views 定在 root。
//! 住 ui 的理由不是「为将来搬迁铺路」（那条理由不成立），而是
//! **契约要独立于组合根**：本 crate 的 `state/` 存着视图要读的全部 UI 状态，
//! 契约与这些状态同侧才说得清「视图能对状态做什么」；root 侧只提供实现。
//!
//! # 依赖方向
//!
//! ```text
//! rustmark（root）──► rustmark-ui ──► rustmark-core ──► rustmark-actions
//!                  └──────────► rustmark-core
//!                  └──────────► rustmark-icons ──► rustmark-actions
//! ```
//!
//! root 经 `pub use rustmark_ui::presentation::widgets;` 把本 crate 的模块
//! 拼回 `crate::presentation::widgets` 原路径——公开路径零变化，存量视图
//! （8 个 view 文件）与集成测试零改动。
//!
//! # 为什么保留 `presentation/` 路径前缀
//!
//! `tools/arch_check.rs` 的 `layer_of` 按**扫描根内的相对路径**判层：
//! 本根的 `presentation/widgets.rs` 与 core 里的旧路径判同一子层
//! （`PresentationWidgets`）——层禁令表与豁免表零改动，且 views 若被误搬
//! 进本 crate，会落在 `presentation/views` 子层**自动触发既有禁令**
//! （fixture 第十七节钉住）。

// V11-65：unwrap 系列与 missing_docs 的 lint 声明已收敛至 root `Cargo.toml`
// 的 `[workspace.lints]` 单点真源（本 crate 经 `[lints] workspace = true` 继承）。
// 规范 §四-15 / DOC-03 的规则内容不变（详见 root `Cargo.toml` 该段注释）；
// 测试代码允许 `unwrap()` 的豁免是 cfg 形态，无法进 workspace.lints 表，
// 保留在本行，与 core / root lib.rs 同款。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod presentation;
