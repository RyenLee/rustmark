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
    // 条件是「当前没有焦点」而不是「每帧都请求」：每帧请求会让用户无法
    // 把焦点移到别的控件上（每帧都被抢回来），而那种表现是「点哪儿都不上焦点」。
    if app.settings_ui.section == SettingsTab::Appearance
        && !ctx.memory(|m| m.has_focus(egui::Id::new(FONT_INPUT_ID)))
        && !ctx.memory(|m| m.has_focus(egui::Id::new("rustmark.overlay.settings")))
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
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    ui.horizontal_top(|ui| {
        nav(app, ui, &theme);
        ui.add_space(theme.metrics.space_md);
        ui.separator();
        ui.add_space(theme.metrics.space_lg);
        content(app, ui, &theme);
    });
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

/// 自定义主题调色板编辑器：8 个核心语义色各一行「标签 + 取色器」。
///
/// 调色板从当前设置读取（缺失则用默认深色调色板），任一色改动后整体写回，
/// 保证一次交互只触发一次 `apply_theme_field`（即一次落盘 + 一次预览刷新）。
fn custom_palette_editor(app: &mut EditorApp, ui: &mut Ui, _theme: &Theme) {
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
