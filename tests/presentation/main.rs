//! 表现层单元测试入口（阶段 06 · S6-09、S6-10、S6-12）。
//!
//! 表现层的多数逻辑（快捷键归一化、动作表、主题映射、UI State 转移）
//! **不依赖 egui 的渲染上下文**，因此可以像普通单元测试一样跑。
//! 真正需要 `egui::Context` 的用例放到阶段 07 的集成测试里。

#[path = "../support/mod.rs"]
mod support;

mod editor_frame_geometry_test;
mod editor_view_test;
mod enter_ime_test;
mod export_view_test;
mod find_replace_view_test;
mod highlight_layout_test;
mod preview_border_test;
// S8-05 高亮性能基线（必须 release 下跑，见文件内说明）。
mod highlight_perf_test;
mod lint_tab_test;
mod math_glyph_test;
mod multi_document_test;
mod preview_view_test;
mod s6_11f_test;
mod s6_12_test;
mod scroll_sync_test;
mod settings_view_test;
mod shortcut_registry_test;
mod sidebar_boundary_test;
mod sidebar_view_test;
mod spell_menu_test;
mod spell_squiggle_test;
mod status_bar_view_test;
mod tab_strip_test;
mod textedit_cost_attribution_test;
mod textedit_frame_perf_test;
mod theme_mapper_test;
mod toolbar_test;
mod toolbar_visual_test;
mod top_menu_test;
mod ui_state_test;
mod word_wrap_test;
