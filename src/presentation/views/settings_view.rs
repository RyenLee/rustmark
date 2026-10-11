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

use egui::{Align, Context, Layout, RichText, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::state::settings_ui_state::SettingsTab;
use crate::presentation::theme::builtin::Theme;

/// 对话框默认尺寸（对齐原型 `BaseModal :width="620" :height="420"`）。
const WIDTH: f32 = 620.0;
const HEIGHT: f32 = 420.0;

/// 分区导航栏宽度（对齐原型 `.cats { width: 150px }`）。
const NAV_WIDTH: f32 = 150.0;

/// 可选主题种类（与领域 `ThemeKind` 一一对应）。
const THEME_KINDS: &[(&str, &str)] = &[
    ("dark", "深色"),
    ("light", "浅色"),
    ("system", "跟随系统"),
    ("custom", "自定义"),
];

/// 可选字号（对齐原型 `previewFontSize` 的取值集合）。
const FONT_SIZES: &[f32] = &[12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0];

/// 可选行高。
const LINE_HEIGHTS: &[f32] = &[1.4, 1.5, 1.6, 1.7, 1.8, 2.0];

/// 可选缩进宽度。
const TAB_SIZES: &[u8] = &[2, 4, 8];

/// 可选的**合法字体族预设**（`(取值, 显示名)`）。
///
/// 字体族此前是一个裸文本输入框：用户能填出系统里根本不存在的字体名，
/// 落盘后设置项「不合规」——渲染层静默回退到默认字体，用户以为改了却没变。
/// 故补上这组预设，取值与领域 `ThemeSettings::font_family` 的字符串契约一致
/// （默认 `system-ui`；`monospace` 与编辑区默认等宽字体同源）。
/// 预设之外的系统字体仍可经下方文本框自定义。
const FONT_FAMILIES: &[(&str, &str)] = &[
    ("system-ui", "系统默认"),
    ("sans-serif", "无衬线"),
    ("serif", "衬线"),
    ("monospace", "等宽"),
];

/// 应用版本号（「关于」分区显示）。
const VERSION: &str = "0.1.0";

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
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::Settings.title())
        .id(egui::Id::new("rustmark.overlay.settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    // 打开后把焦点交给第一个输入控件（仅「外观」分区有它）。
    //
    // 条件是**打开事件**而不是「当前没有焦点」：后者每帧都成立
    //（`Memory::has_focus` 认不到窗口（`Area`）的 id），会把焦点每帧抢回来，
    // 用户点哪儿都不上焦点。开关顺序也重要——分区判断放在前面短路，
    // 这样「打开时不在外观分区、之后切过去」仍能拿到那次唯一的焦点。
    // 详见 `DialogUiState::take_open_focus` 的文档。
    if app.settings_ui.section == SettingsTab::Appearance
        && app.dialog.take_open_focus(Overlay::Settings)
    {
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(FONT_INPUT_ID)));
    }
    if !open {
        // 点标题栏的 × 与点「取消」语义**一致**（都回滚）：
        // 原型的关闭按钮走 `cancel`。用户在设置里改了字号、
        // 关掉窗口却不生效，会以为程序坏了。
        //
        // `cancel_settings` 会再调一次 `dialog.close`，此时浮层已被
        // egui 的 `open(&mut false)` 摘掉，close 是幂等的，无副作用。
        app.cancel_settings();
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
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    let mut bar_x = 0.0_f32;
    let row = ui.horizontal_top(|ui| {
        nav(app, ui, &theme);
        ui.add_space(theme.metrics.space_md);
        // 记下竖线所在的 x（此刻的光标位置），再留出线宽与右间距。
        bar_x = ui.available_rect_before_wrap().left();
        ui.add_space(1.0);
        ui.add_space(theme.metrics.space_lg);
        content(app, ui, &theme);
    });
    // 行画完后才知道真实高度，故在这一步补画竖线（高度 = 行矩形）。
    ui.painter().vline(
        bar_x,
        row.response.rect.y_range(),
        egui::Stroke::new(1.0_f32, theme.border),
    );
    ui.separator();
    live_preview(ui, &theme, app.settings().theme.kind.as_str());
    ui.separator();
    footer(app, ui);
}

/// 左侧导航栏。
fn nav(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.vertical(|ui| {
        ui.set_min_width(NAV_WIDTH);
        for tab in SettingsTab::ALL {
            let selected = app.settings_ui.section == *tab;
            let text = if selected {
                RichText::new(tab.label()).color(theme.accent)
            } else {
                RichText::new(tab.label())
            };
            if ui
                .add_sized([NAV_WIDTH, 28.0], egui::Button::new(text).frame(false))
                .clicked()
            {
                app.settings_ui.section = *tab;
                app.settings_ui.error = None;
                app.settings_ui.notice = None;
            }
        }
    });
}

/// 右侧内容区（按当前标签分发）。
fn content(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.vertical(|ui| {
        ui.set_min_width(WIDTH - NAV_WIDTH - 40.0);
        ui.horizontal_wrapped(|ui| {
            ui.heading(app.settings_ui.section.label());
        });
        ui.separator();
        match app.settings_ui.section {
            SettingsTab::Appearance => appearance(app, ui, theme),
            SettingsTab::Editor => editor(app, ui, theme),
            SettingsTab::Shortcuts => shortcuts(app, ui, theme),
            SettingsTab::SpellCheck => spellcheck(app, ui, theme),
            SettingsTab::About => about(app, ui, theme),
        }
        if let Some(error) = app.settings_ui.error.clone() {
            ui.add_space(theme.metrics.space_sm);
            ui.colored_label(theme.error, format!("⚠ {error}"));
        }
        if let Some(notice) = app.settings_ui.notice.clone() {
            ui.add_space(theme.metrics.space_sm);
            ui.colored_label(theme.success, notice);
        }
    });
}

/// 「外观」分区：主题 / 字号 / 行高 / 字体族。
fn appearance(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let view = app.settings().theme;
    field(ui, theme, "主题", |ui| {
        ui.horizontal_wrapped(|ui| {
            for (kind, label) in THEME_KINDS {
                let selected = view.kind == *kind;
                if ui.selectable_label(selected, *label).clicked() && !selected {
                    let kind = (*kind).to_string();
                    app.apply_theme_field(|t| t.kind = kind);
                }
            }
        });
    });
    if view.kind == "system" {
        ui.colored_label(
            theme.text_secondary,
            format!(
                "当前跟随操作系统外观：系统为{}。更改系统外观即时生效，无需重启。",
                if app.system_prefers_dark() {
                    "深色"
                } else {
                    "浅色"
                }
            ),
        );
    }
    if view.kind == "custom" {
        ui.add_space(theme.metrics.space_xs);
        ui.colored_label(theme.text_secondary, "自定义核心配色（其余沿用内置主题）：");
        custom_palette_editor(app, ui, theme);
    }
    field(ui, theme, "字号", |ui| {
        ui.horizontal_wrapped(|ui| {
            for size in FONT_SIZES {
                let selected = (view.font_size - size).abs() < f32::EPSILON;
                if ui.selectable_label(selected, format!("{size}px")).clicked() && !selected {
                    let size = *size;
                    app.apply_theme_field(|t| t.font_size = size);
                }
            }
        });
    });
    field(ui, theme, "行高", |ui| {
        ui.horizontal_wrapped(|ui| {
            for height in LINE_HEIGHTS {
                let selected = (view.line_height - height).abs() < f32::EPSILON;
                if ui.selectable_label(selected, format!("{height}")).clicked() && !selected {
                    let height = *height;
                    app.apply_theme_field(|t| t.line_height = height);
                }
            }
        });
    });
    field(ui, theme, "字体族", |ui| {
        // 主选择器：**合法预设下拉**。用下拉而非裸文本，是为了让「能选到的
        // 都是合法的」——自由文本可以填出系统不存在的字体，落盘后设置项
        // 不合规，渲染层静默回退，用户以为改了却没变（即本次要修的问题）。
        let current = FONT_FAMILIES
            .iter()
            .find(|(id, _)| view.font_family == *id)
            .map_or("自定义", |(_, label)| *label);
        egui::ComboBox::from_id_salt("rustmark.settings.font.preset")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (id, label) in FONT_FAMILIES {
                    let selected = view.font_family == *id;
                    if ui.selectable_label(selected, *label).clicked() && !selected {
                        let id = (*id).to_string();
                        app.apply_theme_field(|t| t.font_family = id);
                    }
                }
            });
        // 预设之外的**自定义**入口：保留原文本框（同时是「设置面板已渲染」的
        // 焦点锚点，见 [`FONT_INPUT_ID`]）。未知字体由渲染层回退，不会不可用。
        let mut family = view.font_family.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut family)
                    .id(egui::Id::new(FONT_INPUT_ID))
                    .desired_width(150.0)
                    .hint_text("自定义"),
            )
            .changed()
        {
            app.apply_theme_field(|t| t.font_family = family.clone());
        }
    });
}

/// 自定义主题调色板编辑器：8 个核心语义色 + 10 个语法高亮色（V11-09）。
///
/// 调色板从当前设置读取（缺失则用默认深色调色板），任一色改动后整体写回，
/// 保证一次交互只触发一次 `apply_theme_field`（即一次落盘 + 一次预览刷新）。
///
/// 语法色收进折叠区：18 行颜色一次铺开会把「字号 / 行高 / 字体族」挤出视野，
/// 而语法色多数人只调一两个。折叠标题里写清「未改动的跟随主题」，
/// 免去用户对「这一项到底是不是我在控制」的疑惑。
fn custom_palette_editor(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let mut palette = app.settings().theme.custom.clone().unwrap_or_default();
    let mut changed = false;
    changed |= color_row(ui, "主背景", &mut palette.bg_primary);
    changed |= color_row(ui, "次背景", &mut palette.bg_secondary);
    changed |= color_row(ui, "主文本", &mut palette.text_primary);
    changed |= color_row(ui, "次文本", &mut palette.text_secondary);
    changed |= color_row(ui, "强调色", &mut palette.accent);
    changed |= color_row(ui, "代码块背景", &mut palette.code_bg);
    changed |= color_row(ui, "边框", &mut palette.border);
    changed |= color_row(ui, "选区背景", &mut palette.selection_bg);
    egui::CollapsingHeader::new("语法高亮配色（未改动的跟随主题）")
        .id_salt("rustmark.settings.palette.syn")
        .show(ui, |ui| {
            changed |= syn_color_row(ui, "标题", &mut palette.syn_heading, theme.syn_heading);
            changed |= syn_color_row(ui, "加粗", &mut palette.syn_bold, theme.syn_bold);
            changed |= syn_color_row(ui, "斜体", &mut palette.syn_italic, theme.syn_italic);
            changed |= syn_color_row(ui, "代码", &mut palette.syn_code, theme.syn_code);
            changed |= syn_color_row(ui, "链接", &mut palette.syn_link, theme.syn_link);
            changed |= syn_color_row(ui, "引用", &mut palette.syn_quote, theme.syn_quote);
            changed |= syn_color_row(ui, "标记符", &mut palette.syn_marker, theme.syn_marker);
            changed |= syn_color_row(ui, "元信息", &mut palette.syn_meta, theme.syn_meta);
            changed |= syn_color_row(ui, "表格", &mut palette.syn_table, theme.syn_table);
            changed |= syn_color_row(ui, "其它", &mut palette.syn_other, theme.syn_other);
        });
    if changed {
        app.apply_theme_field(|t| t.custom = Some(palette.clone()));
    }
}

/// 一行：标签 + 颜色取色器。返回是否改动（用于决定是否需要写回调色板）。
fn color_row(ui: &mut Ui, label: &str, hex: &mut String) -> bool {
    let mut rgb = hex_to_rgb(hex);
    let before = rgb;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgb(&mut rgb);
    });
    if rgb != before {
        *hex = rgb_to_hex(rgb);
        true
    } else {
        false
    }
}

/// 一行语法高亮色：标签 + 取色器 + （已覆盖时）「跟随」复位按钮。
///
/// `over` 为 `None`（跟随主题）时取色器初值用 `effective`（当前生效色），
/// 这样用户看到的永远是「真实生效的颜色」，而不是一个突兀的黑色方块。
fn syn_color_row(
    ui: &mut Ui,
    label: &str,
    over: &mut Option<String>,
    effective: egui::Color32,
) -> bool {
    let mut rgb =
        over.as_deref()
            .map(hex_to_rgb)
            .unwrap_or([effective.r(), effective.g(), effective.b()]);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *over = Some(rgb_to_hex(rgb));
            changed = true;
        }
        if over.is_some()
            && ui
                .small_button("跟随")
                .on_hover_text("清除此项覆盖，改为跟随内置主题")
                .clicked()
        {
            *over = None;
            changed = true;
        }
    });
    changed
}

/// `#RRGGBB`（兼容无 `#`）→ `[u8; 3]`；非法时退回黑色（仅影响取色器初值）。
fn hex_to_rgb(hex: &str) -> [u8; 3] {
    let s = hex.trim().trim_start_matches('#');
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16);
        let g = u8::from_str_radix(&s[2..4], 16);
        let b = u8::from_str_radix(&s[4..6], 16);
        if let (Ok(r), Ok(g), Ok(b)) = (r, g, b) {
            return [r, g, b];
        }
    }
    [0, 0, 0]
}

/// `[u8; 3]` → `#RRGGBB`（大写，与默认值写法一致，避免无意义 diff）。
fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// 「编辑器」分区：缩进 / 自动换行 / 自动保存间隔。
fn editor(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let view = app.settings().editor;
    field(ui, theme, "Tab 缩进", |ui| {
        ui.horizontal(|ui| {
            for size in TAB_SIZES {
                let selected = view.tab_size == *size;
                if ui
                    .selectable_label(selected, format!("{size} 空格"))
                    .clicked()
                    && !selected
                {
                    let size = *size;
                    app.apply_editor_field(|e| e.tab_size = size);
                }
            }
        });
    });
    field(ui, theme, "自动换行", |ui| {
        let mut on = view.word_wrap;
        if ui.checkbox(&mut on, "按窗口宽度折行").changed() {
            app.apply_editor_field(|e| e.word_wrap = on);
        }
    });
    // 行号栏：**会话开关立即生效**（与 word_wrap 不同——那要重启或状态栏
    // 切换才对编辑区生效，这里不能照抄）。持久化值在启动时播种进会话
    // 状态（`EditorApp::new`），故复选框读会话值即可与设置页其它项一致。
    field(ui, theme, "行号栏", |ui| {
        let mut on = app.editor.line_numbers;
        if ui.checkbox(&mut on, "在编辑区左侧显示行号").changed() {
            app.editor.line_numbers = on;
            app.apply_editor_field(|e| e.show_line_numbers = on);
        }
    });
    field(ui, theme, "自动保存", |ui| {
        let mut interval = view.auto_save_interval.unwrap_or(0);
        if ui
            .add(egui::Slider::new(&mut interval, 0..=60).suffix(" 秒"))
            .changed()
        {
            let value = (interval > 0).then_some(interval);
            app.apply_editor_field(|e| e.auto_save_interval = value);
        }
    });
    if view.auto_save_interval.is_none() {
        ui.colored_label(theme.text_secondary, "自动保存已关闭。");
    }
}

/// 「语法检查」分区。
fn spellcheck(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let view = app.settings().editor;
    let mut on = view.spell_check;
    if ui.checkbox(&mut on, "在编辑区标记拼写错误").changed() {
        app.apply_editor_field(|e| e.spell_check = on);
    }
    ui.colored_label(
        theme.text_secondary,
        "语法检查状态同时显示在底部状态栏，可点击切换。",
    );
    // 右键菜单是不可见的交互，不写出来的话大多数人永远不会发现
    // 「波浪线下面还能点出替换建议」。
    ui.colored_label(
        theme.text_secondary,
        "在带波浪线的词上点右键，可看到「你可能想输入」的建议并一键替换。",
    );
    ui.add_space(theme.metrics.space_sm);
    lint_rules(app, ui, theme);
}

/// 格式检查（lint）规则开关（V11-08）。
///
/// # 为什么与拼写检查同处「语法检查」标签
///
/// 两者对用户是**同一件事**：「帮我看正文有没有毛病」。分到两个标签会让
/// 用户来回找；而侧栏的「检查」标签正是这两类结果共同的去处。
fn lint_rules(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.label(RichText::new("格式检查（Markdown）").color(theme.text_primary));
    let lint = app.settings().editor.lint;
    let toggles: [(&str, bool); 4] = [
        ("标题层级跳跃（如 H1 直接到 H3）", lint.heading_jump),
        ("列表符号混用（- / * / +）", lint.list_marker_mix),
        ("行尾多余空格", lint.trailing_whitespace),
        ("未闭合的代码围栏 ```", lint.unclosed_fence),
    ];
    let mut updated = lint;
    for (i, (label, value)) in toggles.iter().enumerate() {
        let mut on = *value;
        if ui.checkbox(&mut on, *label).changed() {
            match i {
                0 => updated.heading_jump = on,
                1 => updated.list_marker_mix = on,
                2 => updated.trailing_whitespace = on,
                _ => updated.unclosed_fence = on,
            }
        }
    }
    if updated != lint {
        app.apply_editor_field(|e| e.lint = updated);
    }
    ui.colored_label(
        theme.text_secondary,
        "检查结果显示在侧栏的「检查」标签里，点击问题可跳转到对应行。",
    );
}

/// 「快捷键」分区：改键 + 冲突提示 + 恢复默认。
fn shortcuts(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    // 先取快照再进闭包：闭包里要 `&mut app`，而 `entries` 借用了 `app` 的返回值。
    let entries: Vec<(String, String, String)> = app
        .settings()
        .shortcuts
        .iter()
        .map(|s| (s.key.clone(), s.action.clone(), s.combo.clone()))
        .collect();

    ui.horizontal(|ui| {
        ui.weak("操作");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.weak("快捷键 · 点击后按下新组合键");
        });
    });
    ui.separator();

    let mut clear_conflict = false;
    let mut reset_all = false;
    egui::ScrollArea::vertical()
        .id_salt("rustmark.settings.shortcuts")
        .max_height(HEIGHT - 190.0)
        .show(ui, |ui| {
            for (key, label, combo) in &entries {
                let Some(action) = action_of(key) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let recording = app.settings_ui.recording == Some(action);
                        let text = if recording {
                            "录制中…".to_string()
                        } else if combo.is_empty() {
                            "未绑定".to_string()
                        } else {
                            combo.clone()
                        };
                        if ui.button(text).clicked() {
                            if recording {
                                app.settings_ui.stop_recording();
                            } else {
                                // 原键位的登记在 `EditorApp::rebind_shortcut` 里做，
                                // 视图不重复登记（同一处登记两次会让「取消」把
                                // 中间值也当成原值写回）。
                                app.settings_ui.start_recording(action);
                            }
                        }
                    });
                });
            }
            ui.add_space(theme.metrics.space_md);
            if ui.button("恢复默认快捷键").clicked() {
                reset_all = true;
            }
        });

    // 冲突提示条独占一行（对齐原型 `.conflict { width: 100% }`）。
    //
    // 用「先收集意图、出闭包后再执行」的写法而不是在闭包里直接改状态：
    // 闭包持有 `&mut ui`，而解除绑定需要 `&mut app`，两者同时可变借用会编译不过；
    // 绕开的代价是「闭包里改 app」这种写法一旦出现就极难读。
    if let Some(conflict) = app.settings_ui.pending_conflict.clone() {
        ui.horizontal(|ui| {
            ui.colored_label(
                theme.error,
                format!(
                    "{} 与「{}」冲突：同一组合键不能绑定两个动作",
                    conflict.combo, conflict.with_label
                ),
            );
            if ui.button("解除原绑定").clicked() {
                clear_conflict = true;
            }
            if ui.button("放弃本次修改").clicked() {
                app.settings_ui.pending_conflict = None;
            }
        });
    }

    if clear_conflict
        && let Some(conflict) = app.settings_ui.pending_conflict.take()
        && let Some(action) = action_of(&conflict.with_key)
    {
        // 先记住占用者原来的键位，再把它清空——否则「解除原绑定」
        // 就变成「删掉用户的键位」，按「取消」也回不来了。
        app.settings_ui
            .remember_shortcut(action, conflict.with_combo.clone());
        app.rebind_shortcut(action, "");
    }
    if reset_all {
        app.reset_all_shortcuts();
    }
}

/// 「关于」分区。
fn about(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.heading("Rust Markdown Editor");
    ui.label(format!("版本 {VERSION}"));
    ui.colored_label(
        theme.text_secondary,
        "原生轻量 · 秒级启动 · 低内存占用。基于 Rust + egui 的即时模式实现。",
    );
    ui.add_space(theme.metrics.space_sm);
    ui.colored_label(
        theme.text_secondary,
        format!("已打开文档：{} 个", app.open_document_list().len()),
    );
}

/// 实时预览（对齐原型右下角 `.preview`）。
///
/// 用**当前生效**的主题与字号渲染，而不是硬编码字符串：
/// 原型的预览文字写死「深色主题 · 预览示例」，改成浅色后仍显示「深色主题」，
/// 用户会以为设置没生效。
fn live_preview(ui: &mut Ui, theme: &Theme, source_kind: &str) {
    ui.weak("预览");
    let kind_label = if source_kind.trim().eq_ignore_ascii_case("custom") {
        "自定义主题"
    } else {
        match theme.kind {
            crate::presentation::theme::builtin::BuiltinTheme::Dark => "深色主题",
            crate::presentation::theme::builtin::BuiltinTheme::Light => "浅色主题",
            crate::presentation::theme::builtin::BuiltinTheme::System => "跟随系统主题",
        }
    };
    egui::Frame::new()
        .fill(theme.bg_primary)
        .stroke(egui::Stroke::new(1.0_f32, theme.border))
        .inner_margin(10.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong(format!("{kind_label} · 预览示例"));
            ui.label("正文示例：普通文字。");
            ui.monospace("inline code / fn main() {}");
        });
}

/// 页脚：恢复本组默认 / 取消 / 保存。
fn footer(app: &mut EditorApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        if app.settings_ui.can_reset_section() && ui.button("恢复本组默认").clicked() {
            app.reset_current_settings_section();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 关闭浮层由 `confirm_settings` / `cancel_settings` 自己负责，
            // 这里不再重复调 `dialog.close`——三条关闭路径（两个按钮 + Esc）
            // 必须走同一段逻辑，少一处就少一条一致性保证。
            if ui.button("保存").clicked() {
                app.confirm_settings();
            }
            if ui.button("取消").clicked() {
                app.cancel_settings();
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
