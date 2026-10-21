//! 「外观」分区：主题 / 字号 / 行高 / 字体族与自定义调色板。

use egui::Ui;

use crate::app::frame::EditorApp;
use crate::presentation::theme::builtin::{Theme, theme_kind_label};

use super::*;

/// 「外观」分区：主题 / 字号 / 行高 / 字体族。
pub(super) fn appearance(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let view = app.settings().theme;
    field(ui, theme, "主题", |ui| {
        ui.horizontal_wrapped(|ui| {
            for kind in THEME_KIND_ORDER {
                let kind_str = kind.as_str();
                let selected = view.kind == kind_str;
                if ui
                    .selectable_label(selected, theme_kind_label(*kind))
                    .clicked()
                    && !selected
                {
                    let kind = kind_str.to_string();
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
    field(ui, theme, "等宽字体", |ui| {
        // 主选择器：**真实可用字体下拉**。清单由基础设施层启动时发现
        // （项目 `fonts/` 目录 + 常见系统等宽字体，默认项为「系统等宽」），
        // 每一项都会在下次装配时置顶进 egui 的等宽字体族，选了必然生效。
        //
        // 影响范围：编辑区、预览区代码块，以及命令面板 / 差异对比等
        // 一切等宽界面（与 VS Code「编辑器字体」同语义）。
        //
        // 早期这里是 CSS 通用名（`sans-serif` / `serif`…）+ 自由文本框：
        // egui 的字体族里根本没有这些名字，选了也不会变——即本次要修的
        // 「死设置」。现在下拉只列**能生效**的选项。
        let choices = app.editor_font_choices().to_vec();
        let current = choices
            .iter()
            .find(|c| c.key == view.font_family)
            .map_or_else(|| view.font_family.clone(), |c| c.display_name.clone());
        egui::ComboBox::from_id_salt("rustmark.settings.font.preset")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for c in &choices {
                    let selected = view.font_family == c.key;
                    if ui.selectable_label(selected, &c.display_name).clicked() && !selected {
                        let key = c.key.clone();
                        app.apply_theme_field(|t| t.font_family = key);
                    }
                }
            });
        // 预设之外的**自定义**入口：保留原文本框（同时是「设置面板已渲染」的
        // 焦点锚点，见 [`FONT_INPUT_ID`]）。可填 `user:<字体名>` 形式的键；
        // 未发现的字体由装配层安全忽略（等宽族保持内置原样），不会不可用。
        let mut family = view.font_family.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut family)
                    .id(egui::Id::unique(FONT_INPUT_ID))
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
