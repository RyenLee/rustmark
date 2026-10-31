pub mod about_view;
pub mod command_palette_view;
pub mod diff_view;
pub mod editor_ops;
pub mod editor_view;
pub mod export_view;
pub mod find_replace_view;
pub mod main_view;
pub mod overlays;
pub mod preview_view;
pub mod quick_open_view;
pub mod scroll_sync;
pub mod settings_view;
pub mod shortcut_overview_view;
pub mod sidebar_view;
pub mod stats_view;
pub mod status_bar_view;
pub mod symbol_view;
pub mod toolbar_view;
pub mod welcome_view;

// 富内容渲染引擎已下沉到 `presentation::rich`（T9）：它们是引擎而不是视图。
// 这里保留 re-export 是为了让既有路径 `views::math` / `views::mermaid`
// 继续有效——替换引擎时只需改 `rich` 内部，消费方与路径都不用动。
pub use crate::presentation::rich::{math, mermaid};
