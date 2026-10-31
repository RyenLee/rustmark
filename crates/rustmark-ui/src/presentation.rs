//! 表现层的非视图部分（V11-62 起从 core 分批迁入本 crate）。
//!
//! 当前（批次 5 起，V11-62 收官）：`widgets`（egui 布局不变式封装，T8，批次 1）+
//! `state`（UI State，批次 2）+ `shortcuts`（快捷键，批次 3）+
//! `theme`（主题，批次 4）。`rich`（富文本自绘排版）已于 R-04 升为独立 crate
//! `rustmark-render`（见 `rustmark_render`），此处仅经 `pub mod rich` 作 re-export
//! shim 拼回原路径（`crates/rustmark-ui/src/presentation.rs` 内联声明）。
//! 至此 core 的 `presentation` 子模块已全部迁入本 crate；core 侧那个
//! doc-only 空壳（零消费者）已于 2026-10-30 删除，`presentation` 只存在于
//! 本 crate 与 root 的视图/组合根。
//!
//! 保留 `presentation/` 路径前缀是有意设计——见 `crate` 级文档：
//! `layer_of` 按根内相对路径判层，前缀让本 crate 的文件落在与 core 旧路径
//! 同一子层，禁令表零改动。

// R-04 升 crate：原 `presentation::rich` 物理搬出为 `rustmark-render`，此处仅作
// 路径兼容 shim——把 `rustmark_ui::presentation::rich::*` 拼回原路径，所有消费方
// （`views::preview_view` / `views::inline::math`）零代码改动。
/// 富内容渲染引擎（数学 / Mermaid）的兼容 shim：仅转发 `rustmark_render` 的公开项。
/// 真实实现见 `rustmark_render` crate（R-04 升 crate，2026-10-28）。
pub mod rich {
    pub use rustmark_render::*;
}
pub mod shortcuts;
pub mod state;
pub mod theme;
pub mod view_context;
pub mod widgets;
