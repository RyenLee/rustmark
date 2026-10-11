//! 主窗口视图：三栏布局（侧边栏 / 编辑区 / 预览区）+ 顶部菜单栏 + 标签条 + 状态栏。
//!
//! # 布局顺序即 egui 的空间分配顺序
//!
//! egui 的 Panel **不是**声明式的：每 `show` 一个 Panel，它就地从
//! `ctx.available_rect()` 里切走一块，剩余区域交给下一个。
//! 因此本文件的调用顺序就是布局本身，调换顺序会得到不同的界面：
//!
//! ```text
//!   应用级菜单栏（topbar）
//!   标签条（tabstrip）
//!   路径面包屑（tabbar）
//!   编辑区工具栏（编辑区顶部，见 `editor_view`）
//!   statusbar（底部）
//!   sidebar（左侧 240px）
//!   editor（左侧，按比例）
//!   divider（左侧，4px）← 必须是 left，不是 right
//!   central（= 预览区，吃掉剩余全部）
//! ```
//!
//! # 垂直预算为什么统一从Metrics 取
//!
//! 原型的垂直预算是固定四段：topbar 40 / tabbar 28 + tabstrip 32 /
//! content 弹性 / statusbar 24。本项目的顶栏由应用级菜单栏（`topbar_height`）、
//! 标签条（`tabstrip_height`）、路径面包屑（`tabbar_height`）三段组成；编辑区工具栏
//! 已移到编辑区顶部（`editor_toolbar_height`，位于编辑区内部），不再占用顶栏高度。
//!
//! 这些数字同时出现在 `tokens.css` 与布局代码里，若让每个视图自己算高度，
//! 加一处必然破坏另一处。统一从 [`Metrics`] 取，
//! 「主题换尺寸」与「布局按尺寸排」之间只有一条通路。

use egui::Ui;

use crate::app::frame::EditorApp;
use crate::presentation::state::app_ui_state::{SPLIT_MAX, SPLIT_MIN};
use crate::presentation::theme::builtin::Metrics;

/// 分割条的**视觉**宽度（来自 Metrics）与**命中**宽度之比。
///
/// 命中区比视觉宽度宽一倍：4px 对鼠标来说太窄，几乎点不中。
/// 用户抱怨「分割条拖不动」十有八九是这个原因，而不是逻辑坏了。
const DIVIDER_HIT_SCALE: f32 = 1.5;

/// 主视图入口：由 `eframe::App::update` 每帧调用一次。
pub fn show(app: &mut EditorApp, ctx: &egui::Context) {
    let metrics = app.theme().metrics;

    // —— 顶部：菜单栏 + 标签条 ——
    crate::presentation::views::toolbar_view::show(app, ctx);

    // —— 底部：状态栏（可独立开关）——
    if app.ui.status_bar_visible {
        crate::presentation::views::status_bar_view::show(app, ctx);
    }

    // —— 左侧：侧栏（专注模式下整体隐藏）——
    let sidebar_on = app.ui.sidebar_visible && !app.ui.focus_mode;
    if sidebar_on {
        crate::presentation::views::sidebar_view::show(app, ctx);
    }

    // —— 主体：编辑区 + 分割条 + 预览 ——
    // 此处 `available_rect` 已经扣掉上面所有 Panel，**不要再减侧栏宽度**——
    // 侧栏自己开了 SidePanel，再减一次会让编辑区凭空窄 240px。
    let body_width = ctx.available_rect().width();
    if body_width <= 0.0 {
        // 首帧布局未完成时宽度为 0。此时直接跳过：强行用 0 宽做除法
        // 会让分割比例变成 NaN，且 NaN 一旦写进 `split_ratio` 就再也回不来
        //（夹紧对 NaN 无效），必须在这里挡住。
        return;
    }

    let preview_on = app.ui.preview_visible && !app.ui.focus_mode;
    // 欢迎页占**整个主体区**（侧栏之外）：它是「还没有任何文档」时的
    // 整屏入口，左边挂一个空文件树只会让人以为该点点看。
    if app.should_show_welcome() {
        egui::CentralPanel::default().show(ctx, |ui| {
            crate::presentation::views::welcome_view::show(app, ui);
        });
        // 浮层仍要画：用户在欢迎页上按 `Ctrl+,` 打开设置是合理的。
        crate::presentation::views::overlays::show(app, ctx);
        return;
    }
    render_split(app, ctx, metrics, body_width, preview_on);

    // —— 浮层 ——
    // 必须**最后**画：浮层是 `Window`，需要在所有 Panel 之后、
    // 坐标原点回到窗口左上角时画，否则 `anchor(RIGHT_TOP)` 会算到错误位置。
    //
    // 放在 `render_split` 之后还有第二个原因：`scroll_sync::apply` 要在
    // 编辑区与预览区都画完后才跑，浮层若夹在中间会打乱那个顺序。
    crate::presentation::views::overlays::show(app, ctx);
}

/// 渲染编辑区 + 分割条 + 预览区。
fn render_split(
    app: &mut EditorApp,
    ctx: &egui::Context,
    metrics: Metrics,
    body_width: f32,
    with_preview: bool,
) {
    let ratio = app.ui.editor_ratio();
    let divider_w = metrics.divider_width;
    let editor_w = if with_preview {
        // 夹紧上界为 `body_width - divider`：否则编辑区 + 分割条会超出可用宽度，
        // egui 会把多出的部分挤掉，预览区被压成负宽度而消失。
        (body_width * ratio).clamp(0.0, (body_width - divider_w).max(0.0))
    } else {
        body_width
    };

    egui::SidePanel::left("rustmark.editor")
        .exact_width(editor_w)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            crate::presentation::views::editor_view::show(app, ui)
        });

    if !with_preview {
        return;
    }

    // 分割条必须是 `left`：它要**紧贴编辑区右侧**。
    // 若写成 `right`，它会被推到窗口最右缘，与编辑区之间空出整个预览区——
    // 视觉上就是「分割条在最右边」，而拖动它时编辑区宽度却不变，完全无法理解。
    let mut next_ratio = ratio;
    // 拖拽结束的跳变检测：上一帧还在拖、这一帧松手 → 落盘一次分割比例。
    // 每帧都写配置文件太频繁，只在「松手」那一刻写。
    let was_dragging = app.ui.split_dragging;
    egui::SidePanel::left("rustmark.divider")
        .exact_width(divider_w)
        .resizable(false)
        .show_separator_line(false)
        // 边距必须**归零**：egui 的默认面板 frame（`Frame::side_top_panel`）
        // 自带 `Margin::symmetric(8, 2)`，而本面板只有 4pt 宽——egui 会把
        // frame 的响应矩形撑到约 16pt 宽（`set_cursor` 取 `rect.max.x` 排布
        // 下一面板），把预览 CentralPanel 推后约 12pt。这 12pt 落在分割条
        // 与预览之间，**没有任何面板绘制**，真实渲染下露出交换链清屏色
        // （黑）——用户看到的正是「预览区左侧一圈黑边」。同时本面板的
        // `ui.max_rect()` 会被边距挤成负宽，`paint_divider` 退化成零宽
        // 矩形、什么都画不出来（无头取证实测 `Rect [x..x]`）。
        .frame(egui::Frame::new().inner_margin(0))
        .show(ctx, |ui| {
            paint_divider(ui, metrics);
            let rect = ui.max_rect();
            let hit = egui::Rect::from_center_size(
                rect.center(),
                egui::vec2(divider_w * DIVIDER_HIT_SCALE, rect.height()),
            );
            let grab = ui.interact(hit, ui.id().with("grab"), egui::Sense::drag());
            app.ui.split_dragging = grab.dragged();
            if grab.dragged() {
                // 每帧只累加本帧位移：`ratio` 是**上一帧写回后**的值，
                // 跨帧累加因此天然成立，不需要额外的拖拽状态机。
                let delta = ui.input(|i| i.pointer.delta()).x;
                next_ratio = (ratio + delta / body_width).clamp(SPLIT_MIN, SPLIT_MAX);
            }
        });
    if (next_ratio - ratio).abs() > f32::EPSILON {
        app.ui.set_split_ratio(next_ratio);
    }
    if was_dragging && !app.ui.split_dragging {
        // 拖拽结束：把最终比例持久化到设置（跨重启保留）。
        app.persist_split_ratio();
    }

    // 剩下的全部宽度归预览区。
    //
    // 边距必须与 `SidePanel` **一致**：编辑区 / 分割条 / 侧栏是 `SidePanel`
    // （默认竖向内边距 2pt），`CentralPanel::default()` 却自带一圈更大的
    // 内边距（实测 6pt）——预览区整体会比编辑区低 4pt、矮 8pt，两栏的
    // 顶栏（工具栏 ↔ 标题栏）从此错位，内容区也不再从同一条水平线开始
    // （实测：编辑面板 y=[102,774] vs 预览面板 y=[108,768]）。
    // 左右仍取 0：分割条已经把编辑区与预览区隔开，预览区自己
    // `rect_filled` 画底色，不依赖面板默认填充。
    //
    // fill 不能省：`Frame::new()` 无填充，本面板的 2pt 竖向边距条落在
    // 预览自己的 `rect_filled`（只画内容区）之外——无填充时这两条 2pt
    // 露出交换链清屏色（黑），用户看到「预览区顶部一条黑线」。填上
    // 主背景色后与预览底色浑然一体。
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(0, 2))
                .fill(app.theme().bg_primary),
        )
        .show(ctx, |ui| {
            crate::presentation::views::preview_view::show(app, ui);
            // 记录本帧预览区矩形，供图片导出裁剪用（P2）。
            //
            // 记在这里而不是 `preview_view` 内部：CentralPanel 的 `max_rect`
            // 才是「预览区在窗口里的最终位置」，而 `preview_view` 看到的
            // `ui.max_rect()` 会因内部滚动区而未必等于它。
            app.set_preview_rect(ui.max_rect());
        });

    // 滚动同步放在**两侧都画完之后**：`ScrollArea` 的用户滚轮增量是在
    // 它自己的 `show` 内部叠加的，只有画完才能拿到本帧的真实偏移。
    // 算出的目标在下一帧生效（16ms），对连续滚动无感。
    let now_ms = app.services.markdown.now().millis();
    crate::presentation::views::scroll_sync::apply(app, now_ms);
}

/// 绘制分割条本体。
fn paint_divider(ui: &mut Ui, metrics: Metrics) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme_border(ui, metrics));
}

/// 分割条颜色取自主题边框 token。
///
/// 抽成函数而非内联取值，是为了让「分割条颜色来自主题」可被 grep 到——
/// 将来要做「拖拽时高亮分割条」时只有一个地方要改。
fn theme_border(ui: &Ui, _metrics: Metrics) -> egui::Color32 {
    ui.visuals().widgets.noninteractive.bg_stroke.color
}
