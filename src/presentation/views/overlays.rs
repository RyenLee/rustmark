//! 浮层渲染分发（S6-08 建立，S6-11 补齐各浮层视图）。
//!
//! # 这个文件为什么必须存在
//!
//! S6-07 踩过一个**极难发现**的坑：三个动作被 [`Overlay::opened_by`]
//! 列为「打开浮层」，但对应的浮层**视图从未实现**（文件只有一行占位）。
//! 于是 `openable_overlay` 拦截动作 → 打开一个没有任何视图的浮层 →
//! `execute` 直接 `return`。全程**不报错、界面完全正常、只是功能毫无反应**。
//!
//! 根因是「动作 → 浮层」与「浮层 → 视图」是两条**互不校验**的映射。
//! 本文件把后者也收敛成一张表（[`has_view`]），并由
//! `tests/presentation/find_replace_view_test.rs` 的
//! `every_overlay_with_an_opening_action_has_a_view` 钉住：
//! 任何 `Overlay::ALL` 成员若没有渲染实现，用例立刻红。
//!
//! 与 S6-07 的做法同源——**不靠「记得写」，靠「漏了就红」**。

use egui::Context;

use crate::app::frame::EditorApp;
use crate::presentation::state::dialog_ui_state::Overlay;

/// 绘制当前栈顶浮层。
///
/// 只画**栈顶**一个：浮层可叠加（「设置」上再开「关于」），
/// 但同时画多个会让下层完全无法交互，用户看不出自己点到了哪一层。
///
/// 刻意不在这里画「遮罩」：查找面板是**非模态工具面板**（原型里它浮在
/// 编辑区右上角，编辑区仍可编辑），画遮罩会直接违反原型行为。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let Some(overlay) = app.dialog.top() else {
        return;
    };
    match overlay {
        Overlay::Find => crate::presentation::views::find_replace_view::show(app, ctx),
        Overlay::Settings => crate::presentation::views::settings_view::show(app, ctx),
        Overlay::Export => crate::presentation::views::export_view::show(app, ctx),
        Overlay::QuickOpen => crate::presentation::views::quick_open_view::show(app, ctx),
        Overlay::ShortcutOverview => {
            crate::presentation::views::shortcut_overview_view::show(app, ctx)
        }
        Overlay::About => crate::presentation::views::about_view::show(app, ctx),
        Overlay::Stats => crate::presentation::views::stats_view::show(app, ctx),
        Overlay::Symbol => crate::presentation::views::symbol_view::show(app, ctx),
        Overlay::Diff => crate::presentation::views::diff_view::show(app, ctx),
        Overlay::CommandPalette => crate::presentation::views::command_palette_view::show(app, ctx),
    }
}

/// 该浮层是否已有渲染实现。
///
/// 与 [`show`] 的 `match` 严格对应。存在的意义是给测试一个**可查询**的答案：
/// 没有它，测试只能靠「跑一帧看有没有像素」这种间接且脆弱的方式判断。
///
/// S6-12 起**全部浮层都有视图**，故判定改为「未列入 [`PENDING_VIEWS`]」
/// 而不是逐个列举。逐个列举的隐患是：`Overlay` 新增成员时
/// `matches!` 不会报错，只会让新成员**静默返回 false**——
/// 而「有开启动作却没视图」正是 S6-07 那种静默失效的形态。
/// 不是 `const fn`：`slice::contains` 不是 const 稳定函数。
/// 代价是常量上下文（如数组长度）里用不了 `has_view`——
/// 现有调用方全在运行期（测试与视图），不受影响。
pub fn has_view(overlay: Overlay) -> bool {
    !PENDING_VIEWS.contains(&overlay)
}

/// 已知「有开启动作但视图未实现」的浮层。
///
/// **S6-12 起已清空**：`Overlay::ALL` 的每个成员都有渲染实现。
///
/// 之所以保留这个（空）清单而不是删掉：它是 `has_view` 的**数据源**，
/// 且 `every_overlay_with_an_opening_action_has_a_view` 那条用例需要
/// 一个可查询的答案。清单清空时改 `has_view` 为「不在清单里」，
/// 是为了让「新增浮层漏了视图」这件事**从需要枚举变成不可能**。
///
/// 进度（S6-11）：设置（S6-11a/b）、导出（S6-11c）、侧栏三标签（S6-11d）、
/// 状态栏与欢迎页（S6-11e）、余下六个浮层（S6-11f）已落地。
/// S6-12 落地 `CommandPalette` 后清单清空。
pub const PENDING_VIEWS: &[Overlay] = &[];
