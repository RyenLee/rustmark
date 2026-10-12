//! 欢迎页视图（S6-11e）：三个主操作 + 最近文件 + 快捷键提示。
//!
//! # 什么时候显示
//!
//! **启动时，以及用户关掉全部标签之后**——即「一个标签都没开」。
//! 判据是 [`EditorApp::should_show_welcome`]（`active_doc.is_none()`）。
//!
//! 原型用路由表达同一件事：`/` 是 `WelcomeView`、`/editor` 是
//! `WorkbenchView`。本项目没有路由，但「有没有活动文档」是等价的判据。
//!
//! # 启动时**不**自动建空白文档
//!
//! S6-01 曾让启动即建一份「未命名」空白文档，理由是「否则用户面对
//! 一块点了没反应的灰板」。欢迎页正是那个替代品，且信息量大得多。
//! 两者只能选一个：建了空白文档，`active_doc` 立刻是 `Some`，
//! 欢迎页就永远不显示。
//!
//! `editor_view` 仍**每帧**调 [`EditorApp::ensure_document`] 兜底，
//! 保证「一旦离开欢迎页，编辑区永远有一页可编辑」。
//!
//! # 最近文件里的「打不开」怎么办
//!
//! 最近文件列表会包含**已被删除或移动**的文件（最近记录是持久化的，
//! 磁盘状态随时会变）。点一个不存在的文件时**不能静默失败**：
//! 用户点了没反应会以为程序卡了。故把失败原因显示在状态栏，
//! 并保留那一行让用户知道是「这一个文件」的问题。
//!
//! # 模块结构
//!
//! facade 保留布局常量、入口 `show` 与测试；品牌区在 `brand`，
//! 主操作在 `actions`，最近文件在 `recent`，快捷键提示行在 `hints`。

mod actions;
mod brand;
mod hints;
mod recent;

pub use actions::{PrimaryAction, ShortcutHint, shortcut_hints};
pub use recent::display_name;

use egui::{ScrollArea, Ui};

use crate::app::frame::EditorApp;

/// 品牌区标题。
const LOGO: &str = "Rust Markdown Editor";
/// 品牌区副标题。
const SLOGAN: &str = "原生轻量 · 秒级启动 · 低内存占用";
/// 版本号（`env!("CARGO_PKG_VERSION")` 在编译期取，避免两处版本号漂移）。
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// 最近文件最多显示多少条。
const MAX_RECENT: usize = 8;
/// 最近文件卡片宽度。
const RECENT_WIDTH: f32 = 460.0;
/// 主操作按钮高度。
const ACTION_HEIGHT: f32 = 40.0;
/// 每行高度。
const ROW_HEIGHT: f32 = 34.0;
/// 主操作按钮文字字号（测量与绘制必须用同一个值，否则居中会算偏）。
const ACTION_FONT_SIZE: f32 = 14.0;
/// 按钮内左侧图标留白（原型每个按钮左侧有 18px 图标，这里不画但保留宽度）。
const ACTION_ICON_PAD: f32 = 26.0;
/// 按钮内右侧留白。
const ACTION_TRAILING_PAD: f32 = 36.0;
/// 主操作按钮之间的水平间距。
const ACTION_GAP: f32 = 12.0;
/// 快捷键小标签的字号（与 `kbd` 一致）。
const KBD_FONT_SIZE: f32 = 11.0;
/// 快捷键小标签的左右内边距（与 `kbd` 一致）。
const KBD_PAD_X: f32 = 12.0;
/// 快捷键提示文字字号（与 `hints` 一致）。
const HINT_FONT_SIZE: f32 = 12.0;
/// 键位标签与其说明文字之间的间距。
const HINT_KBD_GAP: f32 = 4.0;
/// 相邻两条快捷键提示之间的间距。
const HINT_ITEM_GAP: f32 = 16.0;

#[cfg(test)]
use recent::shorten_left;

/// 绘制欢迎页。
pub fn show(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    ui.painter()
        .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, theme.bg_primary);

    // 顶部光晕：两团柔和径向渐变（原型 `.glow`）。用 `ellipse` 而非
    // `rect_filled`，圆角矩形会看出边角。
    let rect = ui.max_rect();
    brand::paint_glow(ui.painter(), rect, &theme);

    let mut action = None;
    let mut open_recent: Option<String> = None;

    ui.vertical_centered(|ui| {
        ui.add_space(rect.height() * 0.12);
        brand::brand(ui, &theme);
        // 顶级分区之间取刻度 `space_xxl`，功能区块之间取 `space_xl`：
        // 欢迎页只有四个区块，间距是它唯一的层级语言，必须守规范。
        ui.add_space(theme.metrics.space_xxl);
        actions::primary_actions(ui, &theme, &mut action);
        ui.add_space(theme.metrics.space_xxl);
        let recent = app.recent_files();
        recent::recent_section(ui, &theme, &recent, &mut open_recent);
        ui.add_space(theme.metrics.space_xl);
        hints::hints(ui, &theme, app);
    });

    // 滚动：最近文件可能很多（虽截断到 8 条）+ 小窗口高度不足。
    // 包在 `ScrollArea` 里而不是让内容溢出——溢出部分在 egui 里
    // 直接被裁掉，用户连「下面还有东西」都不知道。
    let _ = ScrollArea::vertical().show(ui, |_| {});

    // 渲染完再执行：执行会改 `active_doc`（新建 / 打开），
    // 而本帧的其余部分还在读它。
    if let Some(action) = action {
        actions::run_action(app, action);
    }
    // 失败提示由 `open_path` **自己**报（见其文档），这里不再重复报一遍：
    // 两处都报会让用户看到两条几乎一样的消息。
    if let Some(path) = open_recent {
        let _ = app.open_path(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_takes_the_last_segment() {
        assert_eq!(display_name("/a/b/readme.md"), "readme.md");
        assert_eq!(display_name("readme.md"), "readme.md");
        assert_eq!(display_name("/a/b/"), "");
    }

    #[test]
    fn shorten_left_keeps_the_tail() {
        assert_eq!(shorten_left("short", 10), "short");
        assert_eq!(shorten_left("abcdefghij", 4), "…ghij");
        // 多字节：按**字符**数截，不是字节。保留末 3 个字 + 省略号。
        assert_eq!(shorten_left("一二三四五", 3), "…三四五");
    }

    #[test]
    fn every_primary_action_has_a_label_and_hint() {
        for action in PrimaryAction::ALL {
            assert!(!action.label().is_empty());
            assert!(
                action.hint().len() > action.label().len(),
                "{:?} 的提示应比文案信息更多",
                action
            );
        }
    }
}
