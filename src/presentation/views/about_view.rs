//! 关于浮层（S6-11f）。
//!
//! # 与设置里的「关于」分区的区别
//!
//! `SettingsDialog` 的「关于」分区只占一行（`settings_view.rs`），
//! 因为那是个分区，要给其余四项留地方；这里是从菜单单独打开的浮层，
//! 可以把技术栈与运行环境讲清楚。
//!
//! # 版本号取自 `CARGO_PKG_VERSION` 而不是写死
//!
//! 两处「关于」（`about_view.rs` 与 `settings_view.rs`）都用 `env!` 在编译期
//! 从 `Cargo.toml` 取版本号，杜绝第二份真相——此前 `settings_view.rs` 曾
//! 硬编码 `"0.1.0"`，2026-10-22 版本裁定升 0.2.0 时一并清偿（DOC-01）。
//!
//! # 不写「项目地址」与「许可证全文」
//!
//! 本仓库尚未确定公开地址，写一个假链接比不写更糟（点了打不开）。
//! 许可证信息改为显示 `Cargo.toml` 里真实存在的许可标识。

use egui::{Context, RichText, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::theme::builtin::Theme;

/// 对话框默认尺寸（对齐原型 `BaseModal :width="380" :height="220"`）。
const WIDTH: f32 = 380.0;
const HEIGHT: f32 = 260.0;

/// 版本号（编译期从 `Cargo.toml` 取，见模块文档）。
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 许可证标识（与 `Cargo.toml` 的 `license` 字段一致）。
const LICENSE: &str = "MIT OR Apache-2.0";

/// 一条「名称 — 值」信息项。
pub struct AboutItem {
    /// 名称。
    pub label: &'static str,
    /// 值。
    pub value: String,
}

/// 关于面板要展示的全部条目。
///
/// 返回 `Vec` 而不是在视图里散着写，是为了能被测试逐项核对：
/// 「已打开文档数」这类**随状态变化**的条目最容易在某次重构里
/// 被悄悄改成固定值，而那不会引发任何编译错误。
pub fn items(app: &EditorApp) -> Vec<AboutItem> {
    vec![
        AboutItem {
            label: "版本",
            value: format!("v{VERSION}"),
        },
        AboutItem {
            label: "许可证",
            value: LICENSE.to_string(),
        },
        AboutItem {
            label: "界面",
            value: "Rust + egui（即时模式，原生渲染）".to_string(),
        },
        AboutItem {
            label: "当前主题",
            value: app.theme_kind_label().to_string(),
        },
        AboutItem {
            label: "已打开文档",
            value: format!("{} 个", app.open_document_list().len()),
        },
        AboutItem {
            label: "工作区",
            value: {
                let root = app.workspace_root();
                if root.is_empty() {
                    "未打开".to_string()
                } else {
                    root
                }
            },
        },
    ]
}

/// 绘制关于浮层。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::About.title())
        .id(egui::Id::unique("rustmark.overlay.about"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    if !open {
        app.dialog.close(Overlay::About);
    }
}

/// 主体。
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    brand(ui, &theme);
    ui.add_space(theme.metrics.space_sm);
    ui.colored_label(
        theme.text_secondary,
        "原生轻量 · 秒级启动 · 低内存占用。全部数据留在本地，不上传任何内容。",
    );
    ui.add_space(theme.metrics.space_lg);
    ui.separator();
    ui.add_space(theme.metrics.space_sm);
    for item in items(app) {
        ui.horizontal(|ui| {
            ui.set_min_width(90.0);
            ui.weak(item.label);
            ui.label(RichText::new(item.value).color(theme.text_primary));
        });
    }
}

/// 品牌区。
fn brand(ui: &mut Ui, theme: &Theme) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new("Rust Markdown Editor")
                .size(22.0)
                .strong()
                .color(theme.text_primary),
        );
        ui.label(
            RichText::new(format!("v{VERSION}"))
                .size(12.0)
                .color(theme.text_secondary),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_matches_the_manifest() {
        // 两处各写一遍许可标识必然漂移；这里钉住与 Cargo.toml 一致。
        assert_eq!(LICENSE, env!("CARGO_PKG_LICENSE"));
    }

    #[test]
    fn version_comes_from_the_manifest() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
