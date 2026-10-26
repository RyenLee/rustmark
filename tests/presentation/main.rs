//! 表现层单元测试入口（阶段 06 · S6-09、S6-10、S6-12）。
//!
//! 表现层的多数逻辑（快捷键归一化、动作表、主题映射、UI State 转移）
//! **不依赖 egui 的渲染上下文**，因此可以像普通单元测试一样跑。
//! 真正需要 `egui::Context` 的用例放到阶段 07 的集成测试里。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate（原先仅 lib 目标
// 受 lib.rs attribute 管辖）。规范 §七-20 允许测试代码 unwrap/expect/
// panic，同 lib.rs 的 `cfg_attr(test, allow)` 惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[path = "../support.rs"]
mod support;

mod breadcrumb_test;
mod editor_frame_geometry_test;
mod editor_view_test;
mod enter_ime_test;
// R-03：预览区外链改经 opener 端口（假实现可断言）。
mod export_view_test;
mod external_opener_test;
mod find_replace_view_test;
// V11-63 阶段 B：字体端到端字形验证（从 core infrastructure/fonts.rs 迁出）。
mod font_glyph_test;
mod highlight_layout_bench;
mod highlight_layout_test;
mod preview_border_test;
// S8-05 高亮性能基线（必须 release 下跑，见文件内说明）。
mod highlight_perf_test;
mod lint_tab_test;
mod math_glyph_test;
mod multi_document_test;
mod outline_alignment_test;
mod outline_jump_scroll_test;
mod preview_view_test;
mod s6_11f_test;
mod s6_12_test;
mod scroll_sync_test;
mod search_alignment_test;
mod search_perf_test;
mod settings_view_test;
mod shortcut_registry_test;
mod sidebar_boundary_test;
mod sidebar_toggle_test;
mod sidebar_view_test;
mod spell_menu_test;
mod spell_squiggle_test;
// D-3 阶段 2：字段级可变借用白名单（R-12 的隐性穿透显式化）。
mod state_borrow_guard_test;
// R-10 对拍：统一构造器 vs 三套旧实现。
mod span_job_parity_test;
mod status_bar_view_test;
mod tab_strip_test;
mod textedit_cost_attribution_test;
mod textedit_frame_perf_test;
mod theme_mapper_test;
mod toolbar_test;
mod toolbar_visual_test;
mod top_menu_test;
mod ui_state_test;
mod welcome_view_test;
mod word_wrap_test;
