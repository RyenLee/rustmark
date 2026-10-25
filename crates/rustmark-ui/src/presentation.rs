//! 表现层的非视图部分（V11-62 起从 core 分批迁入本 crate）。
//!
//! 当前（批次 5 起，V11-62 收官）：`widgets`（egui 布局不变式封装，T8，批次 1）+
//! `state`（UI State，批次 2）+ `shortcuts`（快捷键，批次 3）+
//! `theme`（主题，批次 4）+ `rich`（富文本自绘排版，批次 5）。
//! 至此 core 的 `presentation` 子模块已全部迁入本 crate（core 仅留 doc-only 空壳）。
//!
//! 保留 `presentation/` 路径前缀是有意设计——见 `crate` 级文档：
//! `layer_of` 按根内相对路径判层，前缀让本 crate 的文件落在与 core 旧路径
//! 同一子层，禁令表零改动。

pub mod rich;
pub mod shortcuts;
pub mod state;
pub mod theme;
pub mod widgets;
