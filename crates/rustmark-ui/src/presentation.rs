//! 表现层的非视图部分（V11-62 起从 core 分批迁入本 crate）。
//!
//! 当前（批次 3 起）：`widgets`（egui 布局不变式封装，T8，批次 1）+
//! `state`（UI State，批次 2）+ `shortcuts`（快捷键，批次 3）。
//! theme / rich 仍在 core，按批次计划随后迁移。
//!
//! 保留 `presentation/` 路径前缀是有意设计——见 `crate` 级文档：
//! `layer_of` 按根内相对路径判层，前缀让本 crate 的文件落在与 core 旧路径
//! 同一子层，禁令表零改动。

pub mod shortcuts;
pub mod state;
pub mod widgets;
