//! 顶部区（全局顶栏）与编辑区顶部：**应用级菜单栏** + 标签条 + 路径面包屑
//! （全局顶栏，S6-07），以及编辑区顶部的**编辑区工具栏**（见 `editor_view::show`），
//! 并承载 S6-02 的标签条与路径面包屑。
//!
//! # 应用级菜单与编辑区工具栏分处不同区域
//!
//! 命令按**作用域**分处两处，而不是把一切挤在顶栏一行里：
//!
//! | 区域 | 内容 | 作用域 | 形态 |
//! |---|---|---|---|
//! | 全局顶栏 `draw_app_menu_bar` | `MenuGroup::APP_LEVEL` | 整个应用（打开 / 保存 / 视图开关 / 设置） | 全部是**下拉菜单** |
//! | 编辑区顶部 `draw_editor_toolbar` | `MenuGroup::Format` | 当前编辑区里的**内容**（加粗 / 列表 / 缩进……） | 按作用分组的**下拉菜单** |
//!
//! 混在一行时，「文件」下拉与「加粗」按钮看起来同级，职责边界随之消失：
//! 用户分不清哪些命令会改变全局状态、哪些只动光标处的文本。
//! 分层之后，「有没有下拉」恰好成了两处的区分特征——
//! 下拉是「一组命令的入口」（应用级），编辑区工具栏是「按作用分组的命令」（编辑区）。
//! 编辑区工具栏不再占用全局顶栏，而是贴在编辑区顶部（上下布局：工具栏在上、文本在下），
//! 作用域与编辑区内容一致，预览区也可独立占满剩余宽度。
//!
//! # 本文件的核心约束：三入口行为一致（S6-07 验收）
//!
//! 菜单项、快捷键、命令面板三者**必须**给出同样的结果，否则用户会看到
//! 「菜单里能做的事，快捷键做不到」。原型正是在这里出的问题（B-02：
//! `Ctrl+Shift+S` 写在菜单里却从未注册）。
//!
//! 本项目的根治方式不是「记得两边都写」，而是**结构上不可能漏**：
//! 菜单项与工具栏命令本身都是遍历 `MenuGroup` × `UiAction::ALL` 生成的，
//! 文案与键位都来自同一个 `UiAction` 的 `label()` / `ShortcutRegistry::display_for()`。
//! 想让某个动作不上菜单，只能从动作表里删它——那样快捷键和命令面板会**同时**消失。
//!
//! # 模块结构
//!
//! facade 保留顶栏入口 `show` 与面包屑、侧栏切换按钮；应用级菜单栏在
//! `menu_bar`，编辑区工具栏与控件视觉规范在 `controls`，标签条在
//! `tab_strip`。

mod controls;
mod menu_bar;
mod tab_strip;

pub use controls::{
    FLAT_GROUP, ToolbarControl, draw_editor_toolbar, editor_toolbar_groups, toolbar_controls,
};
pub use menu_bar::{MENU_LABEL_SHORTCUT_GAP, MENU_ROW_PAD, menu_row_width};

use egui::Ui;

use crate::presentation::icons;
use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::view_context::ToolbarViewAccess;
use crate::presentation::widgets;

/// 绘制全局顶栏：应用级菜单栏 + 标签条 + 路径面包屑。
///
/// 编辑区工具栏已移到编辑区顶部（见 `editor_view::show`），不再占用此处高度。
/// 垂直预算统一从 [`Metrics`](crate::presentation::theme::builtin::Metrics) 取：
/// 应用级菜单栏占 `topbar_height`、标签条占 `tabstrip_height`、面包屑占 `tabbar_height`。
/// 三者之和即顶部面板高度，改主题尺寸时只有一条通路
/// （避免「菜单栏改了，别处没改」的错位）。
pub fn show(vctx: &mut dyn ToolbarViewAccess, ui: &mut Ui) {
    let metrics = vctx.theme_state().metrics;
    let height = metrics.topbar_height + metrics.tabstrip_height + metrics.tabbar_height;
    egui::Panel::top("rustmark.topbar")
        .exact_size(height)
        .show(ui, |ui| {
            menu_bar::draw_app_menu_bar(vctx, ui);
            // 分隔线：仅画 1px 发丝线、不再占布局高度。默认 `ui.separator()`
            // 预留 ~9–11pt（1px 线 + 上下各 ~4pt 留白），而该留白**不在顶栏
            // 预算内**（`topbar+tabstrip+tabbar=100`）。菜单栏实测又常溢出其
            // 40pt 设计值，二者叠加把面包屑行挤到只剩 ~16pt——15pt 文字加
            // 字脚后底部探出面板底缘 1pt 被裁（问题 060）。改成分隔线只留
            // 1pt 高度，省下的空间还给标签条与面包屑。
            ui.add(egui::Separator::default().spacing(1.0));
            tab_strip::draw_tab_strip(vctx, ui);
            draw_breadcrumb(vctx, ui);
        });
}

/// 路径面包屑：显示当前文档的保存路径（未保存显示「未命名文档」）。
///
/// 路径必须截断（问题 059）：顶栏是横向布局，裸 `ui.label` 的长路径会
/// 一路延伸出面板右缘被**硬切**（截图实证：`logs\问题记录1.txt` 只剩
/// 残形）。截断走仓库 sanctioned 形态——**`ui.put` 显式矩形 + `truncate()`**
/// （标签标题同款）：put 的矩形就是换行的 `max_width`，截断宽度确定可证。
/// 单挂 `.truncate()` 不够——put 的矩形右界取自环境可用宽，而它本身
/// 就是错的（见下）。「多枚 truncate 标签同排会越界冲面板」的禁令
/// （040 布局纪律）针对多标签同排，此处仅一枚、独占剩余宽。
/// 全路径走悬停提示补全——截断保宽度，语义不丢。
///
/// # 环境可用宽不可信（问题 059 的深层根因）
///
/// egui 的 `Placer::expand_to_include_rect` 把 Ui 的 min_rect **与
/// max_rect 一起**扩：同面板里更早的超宽内容（菜单栏、标签条横向溢出）
/// 会把面板 Ui 的 max_rect 连带撑出窗口——实测 360pt 窗口下
/// `available_rect_before_wrap` 的右界漂到 440。`clip_rect` 不随内容
/// 膨胀，是唯一稳定的面板界：右界按面板左内边距对称推出。
///
/// # 纵向必须 `horizontal_top`（问题 060 补充）
///
/// 顶栏菜单栏的实占常超过其 40pt 设计值，连带把面包屑行压到只剩 ~16pt
/// （实测 `cursor.top=84`、`clip.bottom=100`）。若用 `ui.horizontal`，子
/// 控件会被**交叉轴居中**到这一窄带里——15pt 高的路径文字居中后底部
/// 探出面板底缘 1pt 被裁掉（截图「路径行文字底部被遮挡」）。改用
/// `horizontal_top`：子控件贴顶对齐、占据整条窄带，文字顶缘落在行首，
/// 底部留 1pt 余量不再越界。这是项目 egui 布局纪律「顶层横向排布必须
/// `horizontal_top`」在面包屑处的落地。
fn draw_breadcrumb(vctx: &mut dyn ToolbarViewAccess, ui: &mut Ui) {
    let path = vctx.current_document_path();
    let theme = vctx.theme_state().clone();
    let clip = ui.clip_rect();
    // 面板内容左内边距（TopDown 布局下光标 x 即内容左界）。
    let inset = ui.cursor().left() - clip.left();
    let content_right = clip.right() - inset;
    ui.horizontal_top(|ui| {
        // 侧栏切换按钮在行**最左端**（问题 063）：展开时紧贴侧栏右缘、
        // 收起时贴窗口左上角——两种状态下按钮都长在侧栏边上，而不是孤悬
        // 在窗口另一头。必须先画它，后面的文档图标与路径才排在它右侧。
        draw_sidebar_toggle(vctx, ui, clip, inset);
        // 文档图标：向量手绘，替代原 📄 字形。废止字形的理由与工具栏图标
        // 同源（emoji 依赖字体覆盖，见 `presentation::icons` 的模块文档），
        // 且从此与文件树里手绘的页面图标共用同一套规格表。
        let (rect, _) = ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
        let color = theme.text_secondary;
        icons::paint_icon(
            ui.painter(),
            icons::Icon::Document,
            rect,
            icons::IconColors {
                primary: color,
                accent: color,
            },
        );
        // 剩余宽度独占给路径，右界钳到内容右界（竖向上 `horizontal_top`
        // 已贴顶，无需再钳）。侧栏按钮已挪到行左端（问题 063），右界
        // 不再为它让位。
        //
        // `halign(LEFT)` 与标签条标题同理（问题 061）：`put` 子布局居中，
        // **没被 elide 的短路径会从 rest 矩形的水平中点起画**（用户截图
        // 「未命名文档」浮在行中偏右即此），显式左对齐修正。
        let mut rest = ui.available_rect_before_wrap();
        rest.max.x = rest.max.x.min(content_right);
        // `halign(LEFT)` 与标签条标题同理（问题 061）：`put` 子布局居中，
        // **没被 elide 的短路径会从 rest 矩形的水平中点起画**（用户截图
        // 「未命名文档」浮在行中偏右即此），显式左对齐修正。该配方收在
        // `widgets::text::put_elided` 里，此处不再有写漏 halign 的余地。
        widgets::text::put_elided(ui, rest, path.clone());
    });
}

/// 侧栏收起/展开按钮的边长（正方形，贴面包屑行左缘）。
const SIDEBAR_TOGGLE_W: f32 = 18.0;

/// 面包屑行**最左端**的常驻侧栏切换按钮（问题 061 引入，问题 063 从
/// 右上角挪来）。
///
/// # 为什么必须在行左端（问题 063：与侧栏的邻近性）
///
/// 061 把按钮放在面包屑行右端（窗口右上角），侧栏一展开，按钮离它
/// 实际控制的对象隔了整个编辑区——用户反馈「与侧边栏间距过远，不
/// 人性化」。挪到行左端后：展开时按钮紧贴侧栏右缘（面包屑行从侧栏
/// 右侧起排），收起时贴窗口左上角（收起后行从窗口左缘起排）——两种
/// 状态下按钮都**长在侧栏边上**，位置不随状态跳到窗口另一头。
///
/// # 为什么它必须在顶栏、而不是侧栏自己的头部
///
/// 收起后整个侧栏面板（含活动栏与面板头）都不再渲染——头部的任何入口
/// 都会随之消失，「收起」得是单程票。面包屑行左端是两种状态下都存在、
/// 且始终与侧栏相邻的位置。
///
/// 状态复用 `sidebar_visible`（与 Ctrl+\ / 视图菜单同一路径，不引入第二
/// 份「收起」状态）；动作走 [`UiAction::ToggleSidebar`] 汇入
/// `CommandPort::execute`——三入口（按钮 / 快捷键 / 菜单）行为一致由
/// 既有回归钉住。矩形左界按 `clip_rect` 推（问题 059：环境可用宽不可信，
/// 左界同理不取 `cursor().left()` 而取面板左内边距，行首两者相等）。
fn draw_sidebar_toggle(
    vctx: &mut dyn ToolbarViewAccess,
    ui: &mut Ui,
    clip: egui::Rect,
    inset: f32,
) {
    let btn_rect = egui::Rect::from_min_size(
        egui::pos2(clip.left() + inset, ui.cursor().top().max(clip.top())),
        egui::vec2(SIDEBAR_TOGGLE_W, SIDEBAR_TOGGLE_W),
    );
    let response = ui.allocate_rect(btn_rect, egui::Sense::click());
    if response.clicked() {
        vctx.execute(UiAction::ToggleSidebar);
    }
    // 悬停底色与 056 的图标按钮同源：取 egui widget 视觉的 weak_bg_fill，
    // 与 Button 的悬停反馈像素级一致。
    if response.hovered() || response.clicked() {
        let hover_bg = ui.style().interact(&response).weak_bg_fill;
        ui.painter()
            .rect_filled(btn_rect, egui::CornerRadius::same(4), hover_bg);
    }
    let color = if response.hovered() {
        vctx.theme_state().text_primary
    } else {
        vctx.theme_state().text_secondary
    };
    icons::paint_icon(
        ui.painter(),
        icons::Icon::Sidebar,
        btn_rect,
        icons::IconColors {
            primary: color,
            accent: color,
        },
    );
    response.on_hover_text(if vctx.ui_state().sidebar_visible {
        "收起侧栏（Ctrl+\\）"
    } else {
        "展开侧栏（Ctrl+\\）"
    });
}
