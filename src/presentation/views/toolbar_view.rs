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
//! facade 保留顶栏入口 `show` 与标签条、面包屑；应用级菜单栏在 `menu_bar`，
//! 编辑区工具栏与控件视觉规范在 `controls`。

mod controls;
mod menu_bar;

pub use controls::{
    FLAT_GROUP, ToolbarControl, draw_editor_toolbar, editor_toolbar_groups, toolbar_controls,
};
pub use menu_bar::{MENU_LABEL_SHORTCUT_GAP, MENU_ROW_PAD, menu_row_width};

use egui::{RichText, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::icons;
use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::widgets;

/// 绘制全局顶栏：应用级菜单栏 + 标签条 + 路径面包屑。
///
/// 编辑区工具栏已移到编辑区顶部（见 `editor_view::show`），不再占用此处高度。
/// 垂直预算统一从 [`Metrics`](crate::presentation::theme::builtin::Metrics) 取：
/// 应用级菜单栏占 `topbar_height`、标签条占 `tabstrip_height`、面包屑占 `tabbar_height`。
/// 三者之和即顶部面板高度，改主题尺寸时只有一条通路
/// （避免「菜单栏改了，别处没改」的错位）。
pub fn show(app: &mut EditorApp, ui: &mut Ui) {
    let metrics = app.theme().metrics;
    let height = metrics.topbar_height + metrics.tabstrip_height + metrics.tabbar_height;
    egui::Panel::top("rustmark.topbar")
        .exact_size(height)
        .show(ui, |ui| {
            menu_bar::draw_app_menu_bar(app, ui);
            // 分隔线：仅画 1px 发丝线、不再占布局高度。默认 `ui.separator()`
            // 预留 ~9–11pt（1px 线 + 上下各 ~4pt 留白），而该留白**不在顶栏
            // 预算内**（`topbar+tabstrip+tabbar=100`）。菜单栏实测又常溢出其
            // 40pt 设计值，二者叠加把面包屑行挤到只剩 ~16pt——15pt 文字加
            // 字脚后底部探出面板底缘 1pt 被裁（问题 060）。改成分隔线只留
            // 1pt 高度，省下的空间还给标签条与面包屑。
            ui.add(egui::Separator::default().spacing(1.0));
            draw_tab_strip(app, ui);
            draw_breadcrumb(app, ui);
        });
}

/// 标签条：渲染所有打开的文档，可点击切换、关闭、并显示脏标记。
///
/// # 四条硬规则（问题 037 / 038 / 040）
///
/// 1. **同一文件只出现一个标签**：去重开死在 `EditorApp::open_workspace_file`
///    （五条打开入口都汇聚到那里），这里只负责画。
/// 2. **单行横向滚动 + 标题截断**：旧实现用 `horizontal_wrapped`，标签多到
///    一行放不下就换行——而外层 `TopBottomPanel` 是 `exact_height`，第二行
///    越出面板高度后**面板外无人绘制**，露出交换链清屏色（黑框，与侧栏
///    025 / 033 同一根因）。现在高度锁死、内容横向滚动，超宽只延伸可滚动
///    区域；单个标题再截断到 `TAB_MAX_W`，一个超长名也不会把别的标签
///    挤出视口。**横向不是滚动区的属性而是内层 Ui 的属性**：`ScrollArea` 的
///    内层 Ui 继承父级 `top_down` 布局，`allocate_exact_size` 会纵向推进
///    光标——标签逐行堆叠（问题 038 截图实证，症状恰恰是「竖向排列」）。
///    故循环必须显式包在 `horizontal_top` 里。
/// 3. **「全部关闭」常驻右端、不随标签滚动**：它放进滚动区就会在「标签多到
///    要滚动」时一起滚走——而那正是用户最需要它的时刻。041 起文案换成
///    固定宽的图标按钮（描边方框 + ×，语义走悬停提示），把宽度让给滚动区。
/// 4. **溢出必须有左右导航箭头**：滚轮不动横向、滚动条又刻意隐藏（下文），
///    纯鼠标用户没有任何滚动手段——标签多时后面的文档永远看不到
///    （问题 040 截图实证）。故溢出时在滚动区与「全部关闭」之间画一对
///    向量三角形箭头（◀ / ▶ 字形不在字体覆盖内，见 `paint_row_icon`），
///    点击按标签宽度步进滚动；活动标签变化时也自动滚入视野（新开的
///    文档常落在视口外，用户看不见「打开成功」的反馈）。
/// 5. **本函数是滚动 offset 的唯一写者**（问题 041）：滚动区动画关死
///    （`.animated(false)`），滚入与箭头都走「show 之后统一写回」，
///    两者互不覆盖。
fn draw_tab_strip(app: &mut EditorApp, ui: &mut Ui) {
    let docs = app.open_document_list();
    let theme = app.theme().clone();
    // 顶栏预算是**硬约束**（`TopBottomPanel::exact_height(100)`，而 egui 的
    // 面板会按内容伸展）：菜单栏实测占用常超过其 40pt 设计值，若这里再按
    // 设计值足额占 32pt，后面的面包屑就被挤出面板——顶面板随之变高，
    // 中央面板整体下移，顶边留出一条无人绘制的清屏黑带
    // （`preview_border_test` 的回归）。故按「剩余 − 面包屑一行」钳高，
    // 并给滚动区一个**恰好**的分配矩形（`allocate_ui`），光标推进量确定。
    let strip_h = app
        .theme()
        .metrics
        .tabstrip_height
        .min(ui.available_height() - BREADCRUMB_ROW_H);
    let strip_top = ui.cursor().top();
    // 标签行的**统一垂直基准**（问题 044）：条带高度常被菜单栏预算压到小于
    // 行高（`strip_h` 实测 17~20 < `TAB_ROW_H` 24），此时「居中」得到的是**负**
    // 偏移——导航箭头与「全部关闭」图标会整体上移约 1.5pt，而标签行由
    // `ScrollArea` 贴顶排布（内容顶 = `strip_top`），两者不再同一中线
    // （egui 0.37 下 `tab_strip_test` 的行锚断言实测 64.5 vs 66）。
    // 故把基准钳到 ≥ `strip_top`：条带够高时居中，不够时与标签一样贴顶，
    // 三者的中线始终重合。
    let row_top = strip_top + ((strip_h - TAB_ROW_H) / 2.0).max(0.0);
    // 「全部关闭」：一个文档都没开时**不画**——那时它在欢迎页上方，
    // 点它等于「关闭空气」。041 起文案换成图标：原文案连内边距吃 ~70pt，
    // 标签一多全在挤占滚动区；图标固定 `CLOSE_ALL_BTN_W`，语义由悬停
    // 提示补全（`on_hover_text`）。
    let close_all_w = (!docs.is_empty()).then_some(CLOSE_ALL_BTN_W);
    // 条带**内容**的左边界（面板有内边距，x 不是 0）：导航箭头与「全部关闭」
    // 的固定矩形都以它为基准。早前直接拿 `scroll_w` 当绝对 x，等于把它们压在
    // 滚动区右端上——重叠区里两个 widget 会同时收到点击（问题 042 的
    // 表现之一：导航按钮时灵时不灵）。
    let strip_left = ui.cursor().left();
    let avail_w = ui.available_width();
    let base_w = match close_all_w {
        Some(w) => (avail_w - w - CLOSE_ALL_GAP).max(0.0),
        None => avail_w,
    };
    // 溢出预估：标签宽固定 `TAB_MAX_W`（标题截断保证不超）加间距。
    // 判定按「让出导航位之后」的宽度做——宁可保守多让 40pt 也不会误判，
    // 还避免了「按钮画上又缩回去」的宽度来回跳动。
    let estimated_w = docs.len() as f32 * (TAB_MAX_W + TAB_GAP);
    let overflow = !docs.is_empty() && estimated_w > base_w - NAV_TOTAL_W;
    let nav_w: f32 = if overflow { NAV_TOTAL_W } else { 0.0 };
    let scroll_w = (base_w - nav_w).max(0.0);
    // 活动标签滚入视野的**内容坐标**暂存：闭包里量到的标签矩形，show 之后
    // 在统一写回点换算（见下方 `reveal_slot` 的消费处）。
    let reveal_slot = std::cell::Cell::new(None::<egui::Rect>);
    // **权威偏移**：滚动位置由本函数独占写入（`app.ui.tabstrip_scroll_x`），
    // 不再信任 `ScrollArea` 自己算出的 offset——egui 的滚动增量
    // `pass_state.scroll_delta` 是全局的、不分滚动区，任何视图调
    // `ui.scroll_to_rect` 都会写进去并被**下一个 begin 的 ScrollArea** 消费。
    // 标签条在顶栏、总是最先执行，于是把编辑区 / 预览区的滚动请求整个抢走
    // （问题 042：标签条 offset 随机漂移、导航箭头点了没反应）。
    // 内容宽由「标签数 × 槽宽」精确算出，故最大偏移也是确定的。
    let content_w = docs.len() as f32 * (TAB_MAX_W + TAB_GAP);
    let max_offset = (content_w - scroll_w).max(0.0);
    let mut scroll_x = app.ui.tabstrip_scroll_x.clamp(0.0, max_offset);
    // show 之前先把权威值写进 State：egui 的 begin 会 load 它，于是本帧的
    // 布局就落在权威位置，闭包里量到的标签矩形才是准确的。
    // （`tabs_id` 不再需要：`ScrollArea` 直接用 `.id_salt(TABSTRIP_ID_SALT)`，
    // 旧代码里 `tabs.state.store(ui.ctx(), tabs.id)` 已随 0.37 私有化移除。）
    let tabs = ui
        .allocate_ui(egui::vec2(scroll_w, strip_h.max(0.0)), |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(TABSTRIP_ID_SALT)
                .auto_shrink([false, false])
                // egui 0.37 把 `ScrollArea::State::offset` 设为私有，偏移只能经
                // 构造器设定：每帧把权威值 `scroll_x` 喂给 `scroll_offset`，
                // 写回点只更新 `app.ui.tabstrip_scroll_x`（下一帧再由这里喂回），
                // offset 的唯一写者仍是本函数，行为完全确定。
                .scroll_offset(egui::vec2(scroll_x, 0.0))
                // 动画一律关掉（问题 041）：滚轮与 `scroll_to_*` 在开启动画时
                // 会把目标写进 `State.offset_target`——**私有字段**，外部清不掉，
                // 之后每帧插值器都把 offset 拉向旧目标、到 1pt 内才清除。导航
                // 箭头手动写回的 offset 会被它整个抹掉：用户实测「切到最右标签
                // 后点 ◀ 没反应」——自动滚入的目标恰在右边，◀ 的位移被逐帧拉
                // 回；▶ 与旧目标同向，反而「看着正常」。关掉后 offset 的唯一
                // 写者就是本函数（箭头 / 活动标签滚入），行为完全确定。
                .animated(false)
                // 滚动条**不画**，两个理由：
                // ① 标签条本来就不该有滚动条（VS Code / 浏览器均如此）——
                //    标题在右缘被截断已经暗示「右边还有」，再加一条 12pt 的条
                //    只是噪音；
                // ② egui 的滚动条在视口**内部**占位
                //    （`inner_size = outer_size - current_bar_use`）：画出横向条
                //    会吃掉约 12pt 行高，而内容高钉死 `TAB_ROW_H`——于是每个
                //    多标签的帧都会附带一条毫无用处的竖向滚动条。
                // `AlwaysHidden` 只隐藏绘制，滚轮 / 触控板 / 拖拽仍可滚动；
                // 纯鼠标的滚动需求由导航箭头承担（函数头规则 4）。
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    // 与 `row_top` 同一基准：条带高于行高时把整行下移居中，
                    // 不高于时不偏移（`add_space(0)` 为空操作）。
                    ui.add_space((row_top - strip_top).max(0.0));
                    ui.set_height(TAB_ROW_H);
                    // 活动标签变化时滚入视野：新开的文档常落在视口外，
                    // 没有「打开成功」的可见反馈。只比前一帧，不逐帧强拉——
                    // 用户手动滚走后视图不该被拽回来。
                    //
                    // ⚠ 上一帧的 active id **不能**存在 egui 的 temp memory
                    // （`data.insert_temp`）里：temp 项每帧 `begin_frame` 就被清掉，
                    // 下一帧读回来恒为 `None`，于是「变化」恒成立——reveal 每帧
                    // 触发，把用户刚点出来的滚动位移立刻拉回活动标签处。
                    // 表现即问题 042：「◀ 能滚、▶ 点了没反应」。故存在应用状态里。
                    let now_active = app.active_doc.map(|id| id.as_u64());
                    let changed = app.ui.tabstrip_last_active != now_active;
                    app.ui.tabstrip_last_active = now_active;
                    // 横向布局是**必须的**，见函数头规则 2。
                    ui.horizontal_top(|ui| {
                        // 横向间隙归零：egui 会在每个 widget 之后自动追加
                        // `item_spacing.x`（默认 8pt），真实槽宽因此变成 184 而
                        // 非 176——步进与标签边界对不齐，滚到底时最后一个标签
                        // 总被切一半，用户据此以为「右边还有」。间距从此完全由
                        // `TAB_GAP` 控制，槽宽精确等于 `TAB_MAX_W + TAB_GAP`。
                        ui.spacing_mut().item_spacing.x = 0.0;
                        // 槽位基准（问题 061）：循环体内的 `ui.put`（标题 /
                        // 关闭按钮）会把**父 Ui 的光标**推到 put 矩形右缘——
                        // 最后一枚 put 是 × 按钮（右缘 = 标签右缘 − 2pt），下一
                        // 槽位若仍从光标出发，步进变成 166 而非 176，相邻标签
                        // 底色重叠 2pt、× 距下一标签仅 6pt（探针实测
                        // slot 8..178 → 174..344；用户截图即此症状）。故行内
                        // 基准在此**一次性**捕获，x 按序号显式推进，与光标漂移
                        // 彻底解耦。
                        let row_left = ui.cursor().left();
                        let row_top = ui.cursor().top();
                        for (i, doc) in docs.iter().enumerate() {
                            let title = if doc.title.is_empty() {
                                "未命名"
                            } else {
                                doc.title.as_str()
                            };
                            let active = app.active_doc == Some(doc.id);
                            // 整块标签是一个点击区（切换）；关闭按钮后加，天然盖在其上。
                            //
                            // 分配**槽宽**（标签宽 + 间隙）而不是标签宽：间隙若用
                            // `add_space` 追加，它的矩形高度为 0、不会计入内容 Ui 的
                            // `min_size()`——于是 ScrollArea 认为的内容宽比真实可见
                            // 宽度少一个间隙，滚到「底」时最后一个标签仍被切掉一截，
                            // 用户据此以为右边还有（问题 042 的另一半）。并入槽位后
                            // 内容宽精确等于「标签数 × 槽宽」。
                            let slot_rect = egui::Rect::from_min_size(
                                egui::pos2(row_left + i as f32 * (TAB_MAX_W + TAB_GAP), row_top),
                                egui::vec2(TAB_MAX_W + TAB_GAP, TAB_ROW_H),
                            );
                            let response = ui.allocate_rect(slot_rect, egui::Sense::click());
                            // 标签本体只占槽位的前 `TAB_MAX_W`，尾部留作间隙。
                            let rect = egui::Rect::from_min_size(
                                slot_rect.min,
                                egui::vec2(TAB_MAX_W, TAB_ROW_H),
                            );
                            if active && changed {
                                // 不走 `ui.scroll_to_rect`（会写动画目标，见
                                // `.animated(false)` 处注释）：rect 是内容坐标，
                                // 记下来交给 show 之后的统一写回点换算。
                                reveal_slot.set(Some(rect));
                            }
                            if response.clicked() {
                                app.switch_document(doc.id);
                            }
                            // 选中底色与 `selectable_label` 选中态同源（`selection_bg`），
                            // 悬停给一档弱底色；未选中且未悬留空，保持条带干净。
                            let fill = if active {
                                Some(theme.selection_bg)
                            } else if response.hovered() {
                                Some(theme.bg_tertiary)
                            } else {
                                None
                            };
                            if let Some(fill) = fill {
                                ui.painter()
                                    .rect_filled(rect, egui::CornerRadius::same(4), fill);
                            }
                            // 脏标记：小 galley 手动画，宽度已知，供标题算剩余宽。
                            let mut cursor_x = rect.left() + TAB_PAD;
                            if doc.is_dirty {
                                let dot = ui.painter().layout_no_wrap(
                                    "●".to_string(),
                                    egui::FontId::proportional(11.0),
                                    theme.text_secondary,
                                );
                                ui.painter().galley(
                                    egui::pos2(cursor_x, rect.center().y - dot.size().y / 2.0),
                                    dot.clone(),
                                    theme.text_secondary,
                                );
                                cursor_x += dot.size().x + 3.0;
                            }
                            // 标题截断到「标签宽 − 内边距 − 关闭按钮预留」。
                            let name_avail =
                                (rect.width() - (cursor_x - rect.left()) - CLOSE_W - TAB_PAD)
                                    .max(0.0);
                            // 标题必须从槽位左缘起画（问题 061）：`ui.put` 的子
                            // 布局是 `centered_and_justified`，没被 elide 的标题
                            // 会从矩形中点起画、整体右溢压进右邻标签（用户截图
                            // 「上线前体检报告」压住左邻的 ×）。061 当年用
                            // `Label::halign(LEFT)` 修，但那次修复在 egui 0.37 下
                            // 已失效——`halign` 只决定字形相对**自身分配矩形**的
                            // 落点，而那个矩形已被父布局居中。现改走
                            // `widgets::text::put_elided`（`left_to_right` 布局），
                            // 对照表见该函数文档。
                            widgets::text::put_elided(
                                ui,
                                egui::Rect::from_min_size(
                                    egui::pos2(cursor_x, rect.top()),
                                    egui::vec2(name_avail, rect.height()),
                                ),
                                RichText::new(title).color(if active {
                                    theme.text_primary
                                } else {
                                    theme.text_secondary
                                }),
                            );
                            // 关闭按钮：用极小的「×」；点击后关闭该标签并结束本帧渲染
                            // （避免继续遍历已失效的 `docs` 列表造成重复关闭）。
                            let close = ui.put(
                                egui::Rect::from_min_size(
                                    egui::pos2(
                                        rect.right() - CLOSE_W,
                                        rect.top() + (TAB_ROW_H - 16.0) / 2.0,
                                    ),
                                    egui::vec2(CLOSE_W - 2.0, 16.0),
                                ),
                                egui::Button::new("×").small().frame(false),
                            );
                            if close.clicked() {
                                // 关掉**最后一个**标签时回到欢迎页，而不是什么都不做。
                                // `close_document` 拒绝关最后一个（「永远至少一页可编辑」），
                                // 那个约束保护的是「编辑区突然变成空白灰板」；
                                // 欢迎页正是那个灰板的替代品，故此处显式走全关路径。
                                // 走 `close_all_with_confirm` 而非裸 `close_all_documents`：
                                // 丢的是全部未保存修改，与「全部关闭」按钮同险同防
                                // （问题 038）。
                                if app.open_document_list().len() <= 1 {
                                    app.close_all_with_confirm();
                                } else {
                                    app.close_document(doc.id);
                                }
                                return;
                            }
                            // 间隙已并入槽位（见 `slot_rect` 处注释）：相邻两个标签
                            // 若都被填色（选中 + 悬停）也不相连——8pt 间隙在两个
                            // 圆角色块之间留出明确的分界（问题 061 前仅 2pt，用户
                            // 截图实证观感上与重叠无异），又不破坏「一行」的观感。
                        }
                    });
                })
        })
        .inner;
    // ── 统一写回点（问题 042）：滚动偏移只由本函数写 ────────────────────
    // egui 的滚动增量 `pass_state.scroll_delta` 是**全局**的、不分滚动区：
    // 任何视图调 `ui.scroll_to_rect` 都会写进去，由**下一个 begin 的
    // ScrollArea** 消费。标签条在顶栏、总是最先执行，于是把编辑区 / 预览区
    // 的滚动请求整个抢走——offset 随机漂移，导航箭头「点了没反应」。
    // 故把偏移提升为应用状态（`app.ui.tabstrip_scroll_x`），每帧末尾
    // **无条件**钳制并写回，外部污染当帧即被纠正。
    let view_w = tabs.inner_rect.width();
    // ① 滚轮 / 触控板：仅当指针悬停在标签条内才采信 egui 算出的偏移，
    //    否则那是别的滚动区泄漏过来的，丢弃。
    if ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| tabs.inner_rect.contains(p))
    {
        scroll_x = tabs.state.clamped_offset().x;
    }
    // ② 活动标签滚入视野：闭包里量到的 rect 是**内容坐标**，视口显示
    //    [offset.x, offset.x + view_w]（内容原点 = inner_rect.min − offset，
    //    见 egui 的 `content_max_rect`），把 rect 挪进该区间所需的 offset
    //    直接解出来——即时生效，不再经任何动画。
    if let Some(rect) = reveal_slot.get() {
        if rect.left() < scroll_x {
            scroll_x = rect.left();
        } else if rect.right() > scroll_x + view_w {
            scroll_x = rect.right() - view_w;
        }
    }
    // ③ 导航箭头步进（函数头规则 4）：溢出时贴在滚动区与「全部关闭」之间，
    //    按一个标签宽步进；到头到底置灰，给「没有更多了」的反馈。
    if overflow {
        // 与标签行同一垂直基准（问题 044）：旧的「条带居中」在
        // `strip_h < TAB_ROW_H` 时是负偏移，箭头会比标签高出一截。
        let y = row_top;
        let nav_x = strip_left + scroll_w;
        let arrows = [
            (
                egui::Rect::from_min_size(egui::pos2(nav_x, y), egui::vec2(NAV_BTN_W, TAB_ROW_H)),
                -1.0_f32,
                "prev",
            ),
            (
                egui::Rect::from_min_size(
                    egui::pos2(nav_x + NAV_BTN_W + NAV_GAP, y),
                    egui::vec2(NAV_BTN_W, TAB_ROW_H),
                ),
                1.0_f32,
                "next",
            ),
        ];
        for (rect, dir, name) in arrows {
            let response = ui.interact(
                rect,
                ui.scope_id().with(("rustmark.topbar.tabnav", name)),
                egui::Sense::click(),
            );
            let enabled = if dir < 0.0 {
                scroll_x > 0.5
            } else {
                scroll_x < max_offset - 0.5
            };
            let color = if !enabled {
                theme.border
            } else if response.hovered() {
                theme.text_primary
            } else {
                theme.text_secondary
            };
            if enabled && response.hovered() {
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(4), theme.bg_tertiary);
            }
            paint_nav_arrow(ui.painter(), rect, dir, color);
            if enabled && response.clicked() {
                scroll_x += dir * NAV_STEP;
            }
        }
    }
    // ④ 无条件钳制 + 写回：内容变短（关掉文档）或外部 delta 泄漏时，
    //    都在这被纠正——滚动位置永远只有一个写者，行为完全确定。
    scroll_x = scroll_x.clamp(0.0, max_offset);
    // 只更新权威偏移；下一帧由 `scroll_offset` 构造器把它喂回滚动区，
    // 不再直接写 `State`（egui 0.37 的 `offset` 字段已私有）。
    app.ui.tabstrip_scroll_x = scroll_x;

    // 「全部关闭」按钮：固定矩形贴在条带右端（不随滚动区滚动）。
    //
    // ⚠ x 必须以 `strip_left` 为基准（问题 043 截图实证）：导航箭头用
    // `strip_left + scroll_w` 定位，而这里曾直接写 `scroll_w + …`——面板有
    // 内边距，两者差出一个内边距宽，于是「全部关闭」整个压进 ▶ 的热区、
    // 分隔线画在 ▶ 的矩形里，右侧三个控件挤成一团还互相吃点击。
    if let Some(btn_w) = close_all_w {
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                strip_left + scroll_w + nav_w + CLOSE_ALL_GAP,
                // 同一基准（问题 044），与导航箭头 / 标签行中线重合。
                row_top,
            ),
            egui::vec2(btn_w, TAB_ROW_H),
        );
        // 左侧 1px 分隔线：把「文档区」与「操作区」分开，按钮不显得
        // 贴在最后一个标签后面。颜色用 border——比 text_secondary 更弱，
        // 不抢标签的视觉重量。
        ui.painter().vline(
            rect.left() - CLOSE_ALL_GAP / 2.0,
            strip_top..=(strip_top + strip_h),
            egui::Stroke::new(1.0_f32, theme.border),
        );
        // 手绘而非 `Button`：悬停反馈与标签同源（`bg_tertiary` 弱底色），
        // 宽度由量出的文本决定，不依赖 widget 内边距的猜测。
        let response = ui.interact(
            rect,
            ui.scope_id().with("rustmark.topbar.close_all"),
            egui::Sense::click(),
        );
        let text_color = if response.hovered() {
            theme.text_primary
        } else {
            theme.text_secondary
        };
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(4), theme.bg_tertiary);
        }
        // 图标：圆角方框（一枚「标签」）+ 内部 ×——「关掉整排标签」的语义，
        // 与单个标签右上角的裸 × 区分。向量手绘（项目纪律：图标不走字形，
        // 本机字体链路缺 ◀ / ▶ / ▾ 这类装饰字形）。语义文字走悬停提示。
        let stroke = egui::Stroke::new(1.4_f32, text_color);
        let c = rect.center();
        ui.painter().rect(
            egui::Rect::from_center_size(c, egui::vec2(CLOSE_ALL_ICON, CLOSE_ALL_ICON)),
            3,
            egui::Color32::TRANSPARENT,
            stroke,
            egui::StrokeKind::Middle,
        );
        let k = 2.7_f32; // × 的半臂长（13pt 方框内留出边距）
        ui.painter().line_segment(
            [egui::pos2(c.x - k, c.y - k), egui::pos2(c.x + k, c.y + k)],
            stroke,
        );
        ui.painter().line_segment(
            [egui::pos2(c.x - k, c.y + k), egui::pos2(c.x + k, c.y - k)],
            stroke,
        );
        if response.clicked() {
            // 有未保存修改时先确认（丢弃全部修改不可逆，与删除确认同哲学）。
            app.close_all_with_confirm();
        }
        let _ = response.on_hover_text(CLOSE_ALL_LABEL);
    }
}

/// 面包屑一行的自然高（实测一行文字的高度；标签条钳高时预留出来）。
const BREADCRUMB_ROW_H: f32 = 24.0;
/// 标签行高（32pt 的条带里上下各留 4）。
const TAB_ROW_H: f32 = 24.0;
/// 单标签最大宽（标题在这里被截断）。
const TAB_MAX_W: f32 = 168.0;
/// 标签内左侧边距。
const TAB_PAD: f32 = 8.0;
/// 关闭按钮预留宽。
const CLOSE_W: f32 = 22.0;
/// 相邻标签的间距（问题 038 引入时 2pt，问题 061 起放宽到 8pt）：选中 /
/// 悬停两块底色之间要有明确分界，2pt 在深色主题下与重叠无异（用户截图）。
const TAB_GAP: f32 = 8.0;
/// 「全部关闭」的悬停提示文案（041 起按钮本体是图标，语义靠它补全）。
const CLOSE_ALL_LABEL: &str = "全部关闭";
/// 标签条滚动区的 `id_salt`——`ScrollArea` 的滚动状态按它寻址，手写 offset
/// 前后必须能算出**同一个** id（见 `draw_tab_strip`）。
const TABSTRIP_ID_SALT: &str = "rustmark.topbar.tabstrip";
/// 「全部关闭」图标按钮宽（高随 `TAB_ROW_H`，接近方形）。
const CLOSE_ALL_BTN_W: f32 = 28.0;
/// 「全部关闭」图标的方框边长。
const CLOSE_ALL_ICON: f32 = 13.0;
/// 「全部关闭」按钮与滚动区之间的间隙（分隔线画在它的一半处）。
const CLOSE_ALL_GAP: f32 = 8.0;
/// 导航箭头按钮宽（◀ / ▶ 是向量三角形，不依赖字体字形——见 `paint_row_icon`）。
const NAV_BTN_W: f32 = 18.0;
/// 两个导航箭头之间的间隙。
const NAV_GAP: f32 = 4.0;
/// 两个导航箭头占用的总宽（溢出时从滚动区让出这么多）。
const NAV_TOTAL_W: f32 = NAV_BTN_W * 2.0 + NAV_GAP;
/// 点一次箭头滚动的距离（约一个标签宽）。
const NAV_STEP: f32 = TAB_MAX_W + TAB_GAP;

/// 画一枚导航箭头：指向 `dir`（负 = 左，正 = 右）的小三角形。
fn paint_nav_arrow(painter: &egui::Painter, rect: egui::Rect, dir: f32, color: egui::Color32) {
    let s = 3.5_f32;
    let cx = rect.center().x + dir * 1.5;
    let cy = rect.center().y;
    let points = if dir < 0.0 {
        vec![
            egui::pos2(cx - s, cy),
            egui::pos2(cx + s * 0.7, cy - s),
            egui::pos2(cx + s * 0.7, cy + s),
        ]
    } else {
        vec![
            egui::pos2(cx + s, cy),
            egui::pos2(cx - s * 0.7, cy - s),
            egui::pos2(cx - s * 0.7, cy + s),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::epaint::PathStroke::NONE,
    ));
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
fn draw_breadcrumb(app: &mut EditorApp, ui: &mut Ui) {
    let path = app.current_document_path();
    let theme = app.theme().clone();
    let clip = ui.clip_rect();
    // 面板内容左内边距（TopDown 布局下光标 x 即内容左界）。
    let inset = ui.cursor().left() - clip.left();
    let content_right = clip.right() - inset;
    ui.horizontal_top(|ui| {
        // 侧栏切换按钮在行**最左端**（问题 063）：展开时紧贴侧栏右缘、
        // 收起时贴窗口左上角——两种状态下按钮都长在侧栏边上，而不是孤悬
        // 在窗口另一头。必须先画它，后面的文档图标与路径才排在它右侧。
        draw_sidebar_toggle(app, ui, clip, inset);
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
/// `EditorApp::execute`——三入口（按钮 / 快捷键 / 菜单）行为一致由
/// 既有回归钉住。矩形左界按 `clip_rect` 推（问题 059：环境可用宽不可信，
/// 左界同理不取 `cursor().left()` 而取面板左内边距，行首两者相等）。
fn draw_sidebar_toggle(app: &mut EditorApp, ui: &mut Ui, clip: egui::Rect, inset: f32) {
    let btn_rect = egui::Rect::from_min_size(
        egui::pos2(clip.left() + inset, ui.cursor().top().max(clip.top())),
        egui::vec2(SIDEBAR_TOGGLE_W, SIDEBAR_TOGGLE_W),
    );
    let response = ui.allocate_rect(btn_rect, egui::Sense::click());
    if response.clicked() {
        app.execute(UiAction::ToggleSidebar);
    }
    // 悬停底色与 056 的图标按钮同源：取 egui widget 视觉的 weak_bg_fill，
    // 与 Button 的悬停反馈像素级一致。
    if response.hovered() || response.clicked() {
        let hover_bg = ui.style().interact(&response).weak_bg_fill;
        ui.painter()
            .rect_filled(btn_rect, egui::CornerRadius::same(4), hover_bg);
    }
    let color = if response.hovered() {
        app.theme().text_primary
    } else {
        app.theme().text_secondary
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
    response.on_hover_text(if app.ui.sidebar_visible {
        "收起侧栏（Ctrl+\\）"
    } else {
        "展开侧栏（Ctrl+\\）"
    });
}
