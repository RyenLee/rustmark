//! 最近文件区与路径文本工具。

use egui::{RichText, Ui, Vec2};

use crate::application::dto::RecentFileView;
use crate::presentation::theme::builtin::Theme;

use super::*;

/// 最近文件区。
pub(super) fn recent_section(
    ui: &mut Ui,
    theme: &Theme,
    recent: &[RecentFileView],
    open: &mut Option<String>,
) {
    let width = ui.available_width().min(RECENT_WIDTH);
    ui.horizontal(|ui| {
        ui.add_space((ui.available_width() - width) / 2.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new("最近文件")
                    .size(12.0)
                    .strong()
                    .color(theme.text_secondary),
            );
            ui.add_space(theme.metrics.space_md);
            if recent.is_empty() {
                // 显式说「暂无」而不是留一个空框：空框看起来像坏了。
                ui.label(
                    RichText::new("暂无最近文件")
                        .size(12.0)
                        .color(theme.text_secondary.gamma_multiply(0.8)),
                );
                return;
            }
            for (i, item) in recent.iter().take(MAX_RECENT).enumerate() {
                if i > 0 {
                    // 行间用一条线分隔（原型 `.recent li + li { border-top }`）。
                    let (line, _) =
                        ui.allocate_exact_size(Vec2::new(width, 1.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(line, egui::CornerRadius::ZERO, theme.border);
                }
                if recent_row(ui, theme, width, item) {
                    *open = Some(item.path.as_str().to_string());
                }
            }
            if recent.len() > MAX_RECENT {
                ui.label(
                    RichText::new(format!("另有 {} 个更早的文件", recent.len() - MAX_RECENT))
                        .size(11.0)
                        .color(theme.text_secondary.gamma_multiply(0.8)),
                );
            }
        });
    });
}

/// 单行最近文件。返回是否被点击。
fn recent_row(ui: &mut Ui, theme: &Theme, width: f32, item: &RecentFileView) -> bool {
    let name = display_name(item.path.as_str());
    let name_galley = ui.painter().layout_no_wrap(
        name.clone(),
        egui::FontId::proportional(13.0),
        theme.text_primary,
    );
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, theme.bg_tertiary);
    }
    ui.painter().galley(
        rect.left_top() + Vec2::new(12.0, (rect.height() - name_galley.size().y) / 2.0),
        name_galley,
        theme.text_primary,
    );
    // 右侧显示完整路径：两个不同目录下的 `readme.md` 只看名字分不清。
    // 没有父目录时（`/readme.md`）退回显示分隔符，否则那一格是空的，
    // 看起来像渲染失败。
    let dir: String = item
        .path
        .parent()
        .map_or_else(|| "/".to_string(), |p| p.as_str().to_string());
    let dir_galley = ui.painter().layout_no_wrap(
        shorten_left(dir.as_str(), 28),
        egui::FontId::proportional(11.0),
        theme.text_secondary,
    );
    ui.painter().galley(
        egui::pos2(
            rect.right() - 12.0 - dir_galley.size().x,
            rect.center().y - dir_galley.size().y / 2.0,
        ),
        dir_galley,
        theme.text_secondary,
    );
    let clicked = response.clicked();
    if response.hovered() {
        response.on_hover_text(item.path.as_str());
    }
    clicked
}

/// 路径末段（文件名）；无分隔符时返回原串。
pub fn display_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// 目录过长时**从左侧**截断并加省略号。
///
/// 从**左**截而非从右：目录最有辨识度的是**结尾**
/// （`a/b/c/drafts` 里的 `drafts`），从右截会正好把它截掉。
pub(super) fn shorten_left(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().skip(count - max_chars).collect::<String>();
    format!("…{kept}")
}
