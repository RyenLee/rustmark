//! 设置对话框（S6-11）。
//!
//! # 两条验收要求各自的实现落点
//!
//! 两条验收要求的落点：
//!
//! - 「取消」回滚全部未确认修改 → [`EditorApp::cancel_settings`]。快照在
//!   [`EditorApp::open_settings`]（打开浮层那一刻）取，关闭时逐项写回。
//! - 「恢复默认」只重置当前分区 → [`EditorApp::reset_current_settings_section`]
//!   加应用层 `reset_section`。原型缺陷 3.6 用的是全量 `reset()`，一次打回
//!   13 个字段——用户只想改字体却连主题与缩进都被重置。
//!
//! # 实时预览与「保存」的关系
//!
//! 每个字段改动**立即**落盘（这是实时预览的代价），「保存」只是关闭。
//! 若以为「保存」才写盘，会去找一条并不存在的写盘路径。
//!
//! # 模块结构
//!
//! facade 保留对话框骨架（show/body/nav/content/footer/field）与动作反查；
//! 「外观」分区在 `appearance`，「编辑器 / 语法检查」在 `editor_prefs`，
//! 「快捷键」分区在 `shortcuts`，「关于」与实时预览在 `about`。

mod about;
mod appearance;
mod editor_prefs;
mod shortcuts;

use egui::{Align, Context, Layout, RichText, Ui};

use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::state::settings_ui_state::SettingsTab;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::EditorViewContext;
use rustmark_core::shared::theme::ThemeKind;

/// 对话框默认尺寸（对齐原型 `BaseModal :width="620" :height="420"`）。
const WIDTH: f32 = 620.0;
const HEIGHT: f32 = 420.0;

/// 分区导航栏宽度（对齐原型 `.cats { width: 150px }`）。
const NAV_WIDTH: f32 = 150.0;

/// 可选主题种类：**遍历真源**，不再维护一份成对清单。
///
/// 此前这里有一张 `const THEME_KINDS: &[(&str, &str)]`（「字符串 + 显示名」
/// 成对），是 R-09 收敛掉的**第五份**主题映射副本。
/// 那种成对清单的失败模式很典型：真源加了变体，它**不会**自动跟随，
/// 于是新主题在下拉里**不出现**（用户看不到、也选不了），而**零报错**。
///
/// 现在改为遍历真源 + [`theme_kind_label`]，新增主题种类自动出现在下拉里。
///
/// 顺序：真源的 [`ThemeKind::ALL`] 是「浅 / 深 / 跟随系统 / 自定义」，
/// 与原清单的「深 / 浅 / 跟随系统 / 自定义」仅前两项**顺序**不同。
/// 下拉项的顺序是界面选择，故按「深色在前」排列（与原型一致）——
/// 但**清单本身**不再手写：顺序由真源的 `ALL` 决定。
const THEME_KIND_ORDER: &[ThemeKind] = &[
    ThemeKind::Dark,
    ThemeKind::Light,
    ThemeKind::System,
    ThemeKind::Custom,
];

/// 可选字号（对齐原型 `previewFontSize` 的取值集合）。
const FONT_SIZES: &[f32] = &[
    8.0, 10.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0, 24.0, 32.0, 48.0,
];

/// 可选行高。
const LINE_HEIGHTS: &[f32] = &[1.0, 1.2, 1.4, 1.5, 1.6, 1.7, 1.8, 2.0, 2.4, 3.0];

/// 可选缩进宽度。
const TAB_SIZES: &[u8] = &[2, 4, 8];

/// 打开文档数量上限的可选范围（问题 038）。
///
/// 下限 1 = 「一次只看一份」也是合法用法；上限 30 与领域默认值同源的理由：
/// 再大没有意义，且 30 个标签早已超出任何常见窗口一屏放得下的宽度。
/// 范围端点写在表现层而不引用领域常量——check-arch 的依赖禁令
/// （表现层不得依赖 `domain`）；领域侧的默认值由 DTO 的 serde 缺省
/// 单独持有，两端各自有用例钉住。
const MAX_OPEN_DOCUMENTS_RANGE: std::ops::RangeInclusive<u32> = 1..=30;

/// 应用版本号（「关于」分区显示，编译期取自 `Cargo.toml`，杜绝第二份真相）。
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 字体族输入框的固定 `Id`。
///
/// 存在的理由有两条，缺一不可：
///
/// 1. **a11y**：设置打开后光标应有明确落点，否则用户要先去点一下输入框。
/// 2. **可测**：「浮层有没有真的被渲染」这件事，egui 0.33 没有文本回读接口，
///    唯一稳定的外部可观察量是控件状态。焦点能落到这个控件上，
///    就证明整棵控件树被布局过了。
pub const FONT_INPUT_ID: &str = "rustmark.settings.font";

/// 绘制设置对话框。
pub fn show(vctx: &mut dyn EditorViewContext, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::Settings.title())
        .id(egui::Id::unique("rustmark.overlay.settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(vctx, ui));
    // 打开后把焦点交给第一个输入控件（仅「外观」分区有它）。
    //
    // 条件是**打开事件**而不是「当前没有焦点」：后者每帧都成立
    //（`Memory::has_focus` 认不到窗口（`Area`）的 id），会把焦点每帧抢回来，
    // 用户点哪儿都不上焦点。开关顺序也重要——分区判断放在前面短路，
    // 这样「打开时不在外观分区、之后切过去」仍能拿到那次唯一的焦点。
    // 详见 `DialogUiState::take_open_focus` 的文档。
    //
    // 「分区」先取出再比较：`&mut dyn` 上的一次借用若活到 `if` 的条件作用域
    // 末尾，后面的 `dialog_state()` 就再也借不到同一个 `vctx`（可变借用唯一）。
    let section = vctx.settings_ui_state().section;
    if section == SettingsTab::Appearance && vctx.dialog_state().take_open_focus(Overlay::Settings)
    {
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique(FONT_INPUT_ID)));
    }
    if !open {
        // 点标题栏的 × 与点「取消」语义**一致**（都回滚）：
        // 原型的关闭按钮走 `cancel`。用户在设置里改了字号、
        // 关掉窗口却不生效，会以为程序坏了。
        //
        // `cancel_settings` 会再调一次 `dialog.close`，此时浮层已被
        // egui 的 `open(&mut false)` 摘掉，close 是幂等的，无副作用。
        vctx.cancel_settings();
    }
}

/// 主体：左侧导航 + 右侧内容 + 实时预览 + 页脚。
///
/// # 为什么竖直分隔线要「自己画」而不是 `ui.separator()`
///
/// `ui.separator()` 在**水平**布局里取 `available_size_before_wrap().y`
/// 作为高度（见 egui `widgets/separator.rs` 的 `is_horizontal_line` 分支），
/// 而设置对话框是 egui 的 `Window`——它**按内容自动定高**。两者就此构成
/// 正反馈：窗口越高 → 可用高度越大 → 分隔线越高 → 内容越高 → 下一帧窗口更高。
///
/// 实测（1280×900 屏）：高度 411 → 623 → 775 → 927 → **1066**，
/// 每帧 +~150pt，四帧内就顶出屏幕，底部「保存 / 取消」被推到屏幕之外，
/// 用户看到的就是「设置浮窗显示异常」（窗口忽地占满全屏、按钮点不到）。
///
/// 修法：按**已经布局出来的行矩形**画一条 1px 竖线。高度是确定值，
/// 不再回读 `available_height`，反馈环被切断。
/// 注意 egui 自带的 `Separator::default().vertical()` 同样取 available height，
/// 换它没用；`ui.separator()` 之后的两个分隔线在**竖直**布局里是水平线
/// （高度恒为 `spacing`），不参与该反馈，故保留。
fn body(vctx: &mut dyn EditorViewContext, ui: &mut Ui) {
    let theme = vctx.theme_state().clone();
    let mut bar_x = 0.0_f32;
    let row = ui.horizontal_top(|ui| {
        nav(vctx, ui, &theme);
        ui.add_space(theme.metrics.space_md);
        // 记下竖线所在的 x（此刻的光标位置），再留出线宽与右间距。
        bar_x = ui.available_rect_before_wrap().left();
        ui.add_space(1.0);
        ui.add_space(theme.metrics.space_lg);
        content(vctx, ui, &theme);
    });
    // 行画完后才知道真实高度，故在这一步补画竖线（高度 = 行矩形）。
    ui.painter().vline(
        bar_x,
        row.response.rect.y_range(),
        egui::Stroke::new(1.0_f32, theme.border),
    );
    ui.separator();
    about::live_preview(ui, &theme, vctx.settings().theme.kind.as_str());
    ui.separator();
    footer(vctx, ui);
}

/// 左侧导航栏。
fn nav(vctx: &mut dyn EditorViewContext, ui: &mut Ui, theme: &Theme) {
    ui.vertical(|ui| {
        ui.set_min_width(NAV_WIDTH);
        for tab in SettingsTab::ALL {
            let selected = vctx.settings_ui_state().section == *tab;
            let text = if selected {
                RichText::new(tab.label()).color(theme.accent)
            } else {
                RichText::new(tab.label())
            };
            if ui
                .add_sized([NAV_WIDTH, 28.0], egui::Button::new(text).frame(false))
                .clicked()
            {
                vctx.set_settings_section(*tab);
                vctx.clear_settings_messages();
            }
        }
    });
}

/// 右侧内容区（按当前标签分发）。
fn content(vctx: &mut dyn EditorViewContext, ui: &mut Ui, theme: &Theme) {
    ui.vertical(|ui| {
        ui.set_min_width(WIDTH - NAV_WIDTH - 40.0);
        ui.horizontal_wrapped(|ui| {
            ui.heading(vctx.settings_ui_state().section.label());
        });
        ui.separator();
        // 分区先取出再分发：`&mut dyn` 上的一次借用若活到 `match` 语句末尾，
        // 各个分支就再也借不到同一个 `vctx`（可变借用唯一）。
        let section = vctx.settings_ui_state().section;
        match section {
            SettingsTab::Appearance => appearance::appearance(vctx, ui, theme),
            SettingsTab::Editor => editor_prefs::editor(vctx, ui, theme),
            SettingsTab::Shortcuts => shortcuts::shortcuts(vctx, ui, theme),
            SettingsTab::SpellCheck => editor_prefs::spellcheck(vctx, ui, theme),
            SettingsTab::About => about::about(vctx, ui, theme),
        }
        if let Some(error) = vctx.settings_ui_state().error.clone() {
            ui.add_space(theme.metrics.space_sm);
            ui.colored_label(theme.error, format!("⚠ {error}"));
        }
        if let Some(notice) = vctx.settings_ui_state().notice.clone() {
            ui.add_space(theme.metrics.space_sm);
            ui.colored_label(theme.success, notice);
        }
    });
}

/// 页脚：恢复本组默认 / 恢复默认快捷键 / 取消 / 保存。
fn footer(vctx: &mut dyn EditorViewContext, ui: &mut Ui) {
    ui.horizontal(|ui| {
        // 「恢复默认快捷键」在**父级页面**（对话框页脚），不在快捷键分区的
        // 滚动列表里（问题 038）：它是分区级动作，而 60 个条目的列表要滑到
        // 最底部才看得见——「重置整个分区」不该藏得比任何单条键位都深。
        // 页脚不随分区内容滚动，与「恢复本组默认」同级、同位，
        // 两个重置入口从此只有一处。
        let section = vctx.settings_ui_state().section;
        if section == SettingsTab::Shortcuts {
            if ui.button("恢复默认快捷键").clicked() {
                vctx.reset_all_shortcuts();
            }
        } else if vctx.settings_ui_state().can_reset_section()
            && ui.button("恢复本组默认").clicked()
        {
            vctx.reset_current_settings_section();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 关闭浮层由 `confirm_settings` / `cancel_settings` 自己负责，
            // 这里不再重复调 `dialog.close`——三条关闭路径（两个按钮 + Esc）
            // 必须走同一段逻辑，少一处就少一条一致性保证。
            if ui.button("保存").clicked() {
                vctx.confirm_settings();
            }
            if ui.button("取消").clicked() {
                vctx.cancel_settings();
            }
        });
    });
}

/// 「标签 + 值」一行，行后留一个 `space_sm` 的呼吸位。
///
/// 表单行之间若只有全局 `item_spacing.y`（4pt），「一行 = 一个设置项」
/// 的分组感会很弱；6pt 的字段间距把它撑出来。间距取刻度而不是现场定数，
/// 与全应用的分区节奏同源。
fn field(ui: &mut Ui, theme: &Theme, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(label);
        add(ui);
    });
    ui.add_space(theme.metrics.space_sm);
}

/// 由持久化键反查 `UiAction`（录制与冲突定位都要用）。
///
/// 反查而非在视图里维护一张对照表：那张表一旦漏登记，
/// 症状是「某个动作的录制按钮点了没反应」——零报错。
fn action_of(key: &str) -> Option<UiAction> {
    UiAction::ALL
        .iter()
        .copied()
        .find(|a| a.persisted_key() == Some(key))
}
