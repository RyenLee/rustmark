//! 文档统计浮层（S6-11f，对齐原型 `StatsDialog.vue`）。
//!
//! # 口径在 S6-11e 已对齐，本视图只负责「如实呈现」
//!
//! 原型的 `StatsDialog.vue` 有 10 行 + 一个字数大数字 + 一句口径说明。
//! 本视图逐项复刻，**不复刻**原型的任何派生计算：
//! 字数 / 字符 / 段落 / 标题 / 代码块 / 链接 / 图片 / 任务 / 阅读时长
//! 全部取自 [`EditorApp::current_stats`]（即 `compute_stats`）。
//!
//! 理由与 S6-11e 相同：原型口径与本实现曾差一个数量级
//! （中文稿按 `split_whitespace` 只算 1 个词）。若视图再自己算一遍，
//! 状态栏与统计浮层就会显示两个不同的字数——用户会先信哪个？
//!
//! # 大数字只用 `words`
//!
//! 原型的 `.hero-num` 显示 `stats.words`。不用「字符数」：中文用户对
//! 「字数」的心智模型接近字数而非字符数，显示 8000 会以为在吹牛。

use egui::{Align, Context, Layout, RichText, Ui};

use crate::app::frame::EditorApp;
use crate::application::dto::StatsView;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::theme::builtin::Theme;

/// 对话框默认尺寸（对齐原型 `BaseModal :width="420" :height="400"`）。
const WIDTH: f32 = 420.0;
const HEIGHT: f32 = 400.0;

/// 千分位分隔（四位以上才加，否则「8,123」比「8123」更难读）。
pub fn thousands(value: usize) -> String {
    let raw = value.to_string();
    if raw.len() <= 3 {
        return raw;
    }
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    let bytes = raw.as_bytes();
    for (i, ch) in raw.chars().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 一行统计项。
fn row(ui: &mut Ui, theme: &Theme, label: &str, value: &str, hint: Option<&str>) {
    ui.horizontal(|ui| {
        ui.weak(label);
        if let Some(hint) = hint {
            ui.colored_label(theme.text_secondary.gamma_multiply(0.8), hint);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.monospace(value);
        });
    });
    let (line, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(line, egui::CornerRadius::ZERO, theme.border);
}

/// 绘制文档统计浮层。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::Stats.title())
        .id(egui::Id::new("rustmark.overlay.stats"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    if !open {
        // 统计是**只读**的，关掉不需要任何回滚——但仍要把它从
        // 对话框状态里摘掉，否则状态栏再点一次「字数」打不开。
        app.dialog.close(Overlay::Stats);
    }
}

/// 主体。
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    // 没有活动文档时**明说**而不是显示一堆 0：0 字的文件与
    // 「根本没打开文档」在界面上必须可区分。
    let Some(stats) = app.current_stats() else {
        ui.add_space(20.0);
        ui.centered_and_justified(|ui| {
            ui.colored_label(theme.text_secondary, "尚未打开任何文档，无可统计内容。");
        });
        ui.add_space(12.0);
        return;
    };

    // —— 顶部：文件名 + 预计阅读时长 ——
    ui.horizontal(|ui| {
        ui.strong(stats.title.clone());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.colored_label(
                theme.text_secondary,
                format!("约 {} 分钟阅读", stats.reading_minutes),
            );
        });
    });
    ui.add_space(10.0);

    // —— 大数字 ——
    egui::Frame::new()
        .fill(theme.bg_primary)
        .stroke(egui::Stroke::new(1.0_f32, theme.border))
        .corner_radius(4.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(thousands(stats.words))
                        .size(30.0)
                        .strong()
                        .color(theme.text_primary),
                );
                ui.colored_label(theme.text_secondary, "字数");
            });
        });
    ui.add_space(10.0);

    // —— 明细 ——
    row(
        ui,
        &theme,
        "字符数（含空格）",
        &thousands(stats.characters),
        None,
    );
    row(
        ui,
        &theme,
        "字符数（不含空格）",
        &thousands(stats.characters_no_space),
        None,
    );
    row(ui, &theme, "行数", &thousands(stats.lines), None);
    row(
        ui,
        &theme,
        "段落数",
        &thousands(stats.paragraphs),
        Some("非空段落"),
    );
    row(
        ui,
        &theme,
        "标题",
        &thousands(stats.headings),
        Some("# ~ ######"),
    );
    row(
        ui,
        &theme,
        "代码块",
        &thousands(stats.code_blocks),
        Some("围栏代码块"),
    );
    row(ui, &theme, "链接", &thousands(stats.links), None);
    row(ui, &theme, "图片", &thousands(stats.images), None);
    row(
        ui,
        &theme,
        "任务项",
        &format!(
            "{} / {}",
            thousands(stats.tasks_done),
            thousands(stats.tasks_total)
        ),
        Some("已完成 / 总数"),
    );

    ui.add_space(10.0);
    ui.separator();
    // 口径说明**必须留**：用户看到「字数 800」却发现编辑器状态栏
    // 写的是 812，会怀疑其中一个坏了。把规则写出来才能对上账。
    ui.colored_label(
        theme.text_secondary,
        "统计口径：中日韩字符按字计，西文按词计；阅读速度按 300 字 / 分钟估算。",
    );
}

/// 统计浮层的行定义（供测试逐项核对，不依赖 egui）。
pub fn rows_of(stats: &StatsView) -> Vec<(&'static str, String)> {
    vec![
        ("字符数（含空格）", thousands(stats.characters)),
        ("字符数（不含空格）", thousands(stats.characters_no_space)),
        ("行数", thousands(stats.lines)),
        ("段落数", thousands(stats.paragraphs)),
        ("标题", thousands(stats.headings)),
        ("代码块", thousands(stats.code_blocks)),
        ("链接", thousands(stats.links)),
        ("图片", thousands(stats.images)),
        (
            "任务项",
            format!(
                "{} / {}",
                thousands(stats.tasks_done),
                thousands(stats.tasks_total)
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separates_only_four_digit_numbers() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(8), "8");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(12345), "12,345");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
