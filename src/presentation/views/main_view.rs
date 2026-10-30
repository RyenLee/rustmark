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
//!   sidebar（左侧，宽度可拖拽，默认取 Metrics::sidebar_width）
//!   sidebar divider（左侧，4px）← 必须是 left，贴侧栏右缘
//!   editor（左侧，按比例）
//!   divider（左侧，4px）← 必须是 left，不是 right
//!   central（= 预览区，吃掉剩余全部）
//! ```
//!
//! # 两条分割条的分工
//!
//! 侧栏分割条（`rustmark.sidebar.divider`）调**侧栏宽度**（像素），
//! 编辑↔预览分割条（`rustmark.divider`）调**分割比例**。两者同构：
//! 4px 视觉宽 + 2× 命中区 + 悬停光标 + 悬停/拖拽微高亮；侧栏那条
//! 额外支持双击复位到主题默认宽度。
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

use crate::presentation::state::app_ui_state::{SPLIT_MAX, SPLIT_MIN};
use crate::presentation::theme::builtin::Metrics;
use crate::presentation::view_context::EditorViewContext;

/// 分割条的**视觉**宽度（来自 Metrics）与**命中**宽度之比。
///
/// 命中区是视觉宽度的两倍（4px → 8px）：4px 对鼠标来说太窄，几乎点不中。
/// 用户抱怨「分割条拖不动」十有八九是这个原因，而不是逻辑坏了。
/// 两侧各探出的 2px 落在相邻面板的默认内边距里（`Frame::side_top_panel`
/// 自带 `Margin::symmetric(8, 2)`），抢不到相邻面板的交互。
const DIVIDER_HIT_SCALE: f32 = 2.0;

/// 主视图入口：由 `eframe::App::ui` 每帧调用一次。
///
/// 入参是 eframe 0.37 传入的根 `Ui`（覆盖整个窗口、无默认边距与底色），
/// 所有 `Panel` 都从这里就地切分。
pub fn show(vctx: &mut dyn EditorViewContext, ui: &mut Ui) {
    draw(vctx, ui);
    // 帧末：侧栏开合**变了就落盘**（问题 062）。
    //
    // 必须放在**所有绘制之后**：右上角按钮的点击发生在顶栏绘制阶段，
    // 放在前面会漏掉本帧那一次切换（用户点完立刻关窗口就丢了）。
    // 与 `persist_split_ratio` 同一手法（都在主视图帧末统一落盘，而不是在
    // 每个切换入口各写一遍——入口有四条以上，漏一个是静默失效）。
    vctx.persist_sidebar_visibility();
    // 帧末：侧栏宽度**变了就落盘**（问题 067）。拖拽期间宽度每帧都在变，
    // 由 `persist_sidebar_width` 内部的 `sidebar_dragging` 门控跳过，
    // 松手那一帧自然落盘一次；双击复位与设置页「恢复本组默认」不走
    // 拖拽，同样在这一处被捕获（入口逐个写必漏，062 的教训）。
    vctx.persist_sidebar_width();
}

/// 绘制三栏布局（[`show`] 的实现体；单独成函数只为让帧末的落盘
/// 不受中途 `return` 影响）。
fn draw(vctx: &mut dyn EditorViewContext, ui: &mut Ui) {
    let metrics = vctx.theme_state().metrics;

    // 防御性底色：先把整块可用区域铺成面板底色（`bg_secondary`），画在最底层。
    //
    // 相邻 `Panel` 在分数缩放（如 125% DPI）下会因像素取整在边界留下亚像素
    // 缝隙，缝隙里没有任何面板绘制，露出交换链清屏色（黑）。浅色主题下边界两侧都是
    // 浅灰面板，黑缝格外显眼；深色主题下缝隙同为黑、与深色面板浑然一体，故只在浅色
    // 模式被察觉（#039）。这里铺一层面板底色，任何亚像素缝隙都会落回浅灰而非黑，
    // 黑线即消失。后续所有面板都画在这层之上，不会造成重影。
    ui.layer_painter(egui::LayerId::background()).rect_filled(
        ui.available_rect_before_wrap(),
        egui::CornerRadius::ZERO,
        vctx.theme_state().bg_secondary,
    );

    // —— 顶部：菜单栏 + 标签条 ——
    crate::presentation::views::toolbar_view::show(vctx, ui);

    // —— 底部：状态栏（可独立开关）——
    let status_bar_on = vctx.ui_state().status_bar_visible;
    if status_bar_on {
        crate::presentation::views::status_bar_view::show(vctx, ui);
    }

    // —— 左侧：侧栏 + 侧栏分割条（专注模式下整体隐藏）——
    let sidebar_on = {
        let ui_state = vctx.ui_state();
        ui_state.sidebar_visible && !ui_state.focus_mode
    };
    if sidebar_on {
        // 生效宽必须在侧栏面板**之前**算：它按「整个主体带宽度」做动态
        // 夹紧（不超过编辑区+预览区的 60%），而面板一开，可用宽就被
        // 切掉侧栏那一份，再也量不到总数。
        let total = ui.available_rect_before_wrap().width();
        let width = vctx
            .ui_state()
            .effective_sidebar_width(total, metrics.divider_width);
        crate::presentation::views::sidebar_view::show(vctx, ui, width);
        render_sidebar_divider(vctx, ui, metrics, width);
    }

    // —— 主体：编辑区 + 分割条 + 预览 ——
    // 此处 `available_rect_before_wrap` 已经扣掉上面所有 Panel（含侧栏
    // 分割条），**不要再减侧栏宽度**——侧栏自己开了 Panel，再减一次
    // 会让编辑区凭空窄一截。
    let body_width = ui.available_rect_before_wrap().width();
    if body_width <= 0.0 {
        // 首帧布局未完成时宽度为 0。此时直接跳过：强行用 0 宽做除法
        // 会让分割比例变成 NaN，且 NaN 一旦写进 `split_ratio` 就再也回不来
        //（夹紧对 NaN 无效），必须在这里挡住。
        return;
    }

    let preview_on = {
        let ui_state = vctx.ui_state();
        ui_state.preview_visible && !ui_state.focus_mode
    };
    // 欢迎页占**整个主体区**（侧栏之外）：它是「还没有任何文档」时的
    // 整屏入口，左边挂一个空文件树只会让人以为该点点看。
    if vctx.should_show_welcome() {
        egui::CentralPanel::default().show(ui, |ui| {
            crate::presentation::views::welcome_view::show(vctx, ui);
        });
        // 浮层仍要画：用户在欢迎页上按 `Ctrl+,` 打开设置是合理的。
        crate::presentation::views::overlays::show(vctx, ui);
        return;
    }
    render_split(vctx, ui, metrics, body_width, preview_on);

    // —— 浮层 ——
    // 必须**最后**画：浮层是 `Window`，需要在所有 Panel 之后、
    // 坐标原点回到窗口左上角时画，否则 `anchor(RIGHT_TOP)` 会算到错误位置。
    //
    // 放在 `render_split` 之后还有第二个原因：`scroll_sync::apply` 要在
    // 编辑区与预览区都画完后才跑，浮层若夹在中间会打乱那个顺序。
    crate::presentation::views::overlays::show(vctx, ui);
}

/// 渲染编辑区 + 分割条 + 预览区。
fn render_split(
    vctx: &mut dyn EditorViewContext,
    ui: &mut Ui,
    metrics: Metrics,
    body_width: f32,
    with_preview: bool,
) {
    let ratio = vctx.ui_state().editor_ratio();

    // 防御性底色：先把整块主体区铺成面板底色（`bg_secondary`）。
    //
    // 相邻 `SidePanel` 在分数缩放（如 125% DPI）下会因像素取整在边界留下
    // 亚像素缝隙，缝隙里没有任何面板绘制，露出交换链清屏色（黑）。
    // 浅色主题下边界两侧都是浅灰面板，黑缝因此格外显眼；深色主题下缝隙同样是黑、
    // 与深色面板浑然一体，故只在浅色模式被察觉（#039）。
    //
    // 这里铺一层面板底色，任何亚像素缝隙都会落回浅灰而非黑，黑线即消失。
    // 作用域仅主体区：侧栏 / 顶栏 / 状态栏 / 预览各自有填充，且本绘制在最底层，
    // 后续面板会盖在上面，不会造成重影。
    ui.layer_painter(egui::LayerId::background()).rect_filled(
        ui.available_rect_before_wrap(),
        egui::CornerRadius::ZERO,
        vctx.theme_state().bg_secondary,
    );

    let divider_w = metrics.divider_width;
    let editor_w = if with_preview {
        // 夹紧上界为 `body_width - divider`：否则编辑区 + 分割条会超出可用宽度，
        // egui 会把多出的部分挤掉，预览区被压成负宽度而消失。
        (body_width * ratio).clamp(0.0, (body_width - divider_w).max(0.0))
    } else {
        body_width
    };

    egui::Panel::left("rustmark.editor")
        .exact_size(editor_w)
        .resizable(false)
        .show_separator_line(false)
        .show(ui, |ui| {
            crate::presentation::views::editor_view::show(vctx, ui)
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
    let was_dragging = vctx.ui_state().split_dragging;
    egui::Panel::left("rustmark.divider")
        .exact_size(divider_w)
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
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let hit = egui::Rect::from_center_size(
                rect.center(),
                egui::vec2(divider_w * DIVIDER_HIT_SCALE, rect.height()),
            );
            // 先交互再上色：颜色要反映**本帧**的悬停 / 拖拽态（微高亮），
            // 响应算出来之前画只能是静止色。
            let grab = ui
                .interact(hit, ui.scope_id().with("grab"), egui::Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, divider_color(ui, &grab));
            vctx.set_split_dragging(grab.dragged());
            if grab.dragged() {
                // 每帧只累加本帧位移：`ratio` 是**上一帧写回后**的值，
                // 跨帧累加因此天然成立，不需要额外的拖拽状态机。
                let delta = ui.input(|i| i.pointer.delta()).x;
                next_ratio = (ratio + delta / body_width).clamp(SPLIT_MIN, SPLIT_MAX);
            }
        });
    if (next_ratio - ratio).abs() > f32::EPSILON {
        vctx.ui_state().set_split_ratio(next_ratio);
    }
    if was_dragging && !vctx.ui_state().split_dragging {
        // 拖拽结束：把最终比例持久化到设置（跨重启保留）。
        vctx.persist_split_ratio();
    }

    // 剩下的全部宽度归预览区。
    //
    // 边距必须与 `Panel` **一致**：编辑区 / 分割条 / 侧栏是 `Panel`
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
                .fill(vctx.theme_state().bg_primary),
        )
        .show(ui, |ui| {
            crate::presentation::views::preview_view::show(vctx, ui);
            // 记录本帧预览区矩形，供图片导出裁剪用（P2）。
            //
            // 记在这里而不是 `preview_view` 内部：CentralPanel 的 `max_rect`
            // 才是「预览区在窗口里的最终位置」，而 `preview_view` 看到的
            // `ui.max_rect()` 会因内部滚动区而未必等于它。
            vctx.set_preview_rect(ui.max_rect());
        });

    // 滚动同步放在**两侧都画完之后**：`ScrollArea` 的用户滚轮增量是在
    // 它自己的 `show` 内部叠加的，只有画完才能拿到本帧的真实偏移。
    // 算出的目标在下一帧生效（16ms），对连续滚动无感。
    let now_ms = vctx.now_ms();
    crate::presentation::views::scroll_sync::apply(vctx, now_ms);
}

/// 渲染侧栏分割条：侧栏右缘的 4px 拖拽柄（问题 067）。
///
/// 与编辑区↔预览区的分割条同构（同一条「贴左缘」铁律：必须是
/// `Panel::left`、紧贴侧栏右侧；写成 `right` 会被推到窗口最右缘），
/// 差别只在拖拽数学：那条除 `body_width` 得比例，这条加指针横移得
/// 像素宽。另多一个能力：**双击复位**到主题默认宽度。
fn render_sidebar_divider(
    vctx: &mut dyn EditorViewContext,
    ui: &mut Ui,
    metrics: Metrics,
    width: f32,
) {
    let divider_w = metrics.divider_width;
    // 拖拽从**生效宽**起算，而不是 `AppUiState` 里的用户值：生效宽被
    // 动态上界夹住时（窄窗口），用户值可能更大，从它起算会让分割条
    // 「拖了不动」——要等用户值跌回上界以下才恢复跟手。从生效宽起算
    // 则分割条永远跟手，只在硬边界处停住（与分割比例的夹紧同感）。
    let mut next_width = width;
    egui::Panel::left("rustmark.sidebar.divider")
        .exact_size(divider_w)
        .resizable(false)
        .show_separator_line(false)
        // 边距必须**归零**：默认面板 frame 自带 `Margin::symmetric(8, 2)`，
        // 4pt 宽会被撑成约 16pt，把编辑区推后、缝隙里露出清屏色（黑）
        // ——与 `rustmark.divider` 同一处坑（#039），写法照抄。
        .frame(egui::Frame::new().inner_margin(0))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let hit = egui::Rect::from_center_size(
                rect.center(),
                egui::vec2(divider_w * DIVIDER_HIT_SCALE, rect.height()),
            );
            // `click` 与 `drag` 并集：双击复位要 `double_clicked()`，
            // 而它要求 sense 含 click（纯 drag sense 下恒为 false）。
            // 单击本身不做事——同层重叠控件都会收到点击，不会抢走
            // 相邻面板左缘的交互（命中区探出的 2px 落在面板内边距里）。
            let grab = ui
                .interact(
                    hit,
                    ui.scope_id().with("grab"),
                    egui::Sense::drag().union(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, divider_color(ui, &grab));
            vctx.set_sidebar_dragging(grab.dragged());
            if grab.dragged() {
                // 每帧只累加本帧位移：`width` 是**上一帧写回后**的生效值，
                // 跨帧累加因此天然成立（与分割比例同一手法）。
                let delta = ui.input(|i| i.pointer.delta()).x;
                next_width = width + delta;
            }
            if grab.double_clicked() {
                // 双击复位到主题默认宽度（`set_sidebar_width` 会夹紧，
                // 主题默认值越界时也不会破）。
                next_width = vctx.theme_state().metrics.sidebar_width;
            }
        });
    if (next_width - width).abs() > f32::EPSILON {
        vctx.ui_state().set_sidebar_width(next_width);
    }
}

/// 分割条颜色：静止 = 主题边框 token；悬停 / 拖拽 = egui 控件态描边。
///
/// 抽成函数而非内联取值，是为了让「分割条颜色来自主题」可被 grep 到，
/// 且两条分割条共用同一套状态色语言——悬停微亮、拖拽更亮，用户因此
/// 看得见「这里是能拖的」（可发现性，问题 067 的顺手项）。
fn divider_color(ui: &Ui, response: &egui::Response) -> egui::Color32 {
    let widgets = &ui.visuals().widgets;
    if response.dragged() {
        widgets.active.bg_stroke.color
    } else if response.hovered() {
        widgets.hovered.bg_stroke.color
    } else {
        widgets.noninteractive.bg_stroke.color
    }
}
