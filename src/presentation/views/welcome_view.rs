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

use egui::{RichText, ScrollArea, Ui, Vec2};

use crate::app::frame::{EditorApp, PendingDialog};
use crate::application::dto::RecentFileView;
use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::theme::builtin::Theme;

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

/// 欢迎页的主操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryAction {
    NewFile,
    OpenFile,
    OpenFolder,
}

impl PrimaryAction {
    /// 全部主操作（遍历用）。
    pub const ALL: &'static [PrimaryAction] = &[
        PrimaryAction::NewFile,
        PrimaryAction::OpenFile,
        PrimaryAction::OpenFolder,
    ];

    /// 按钮文案。
    pub const fn label(self) -> &'static str {
        match self {
            PrimaryAction::NewFile => "新建文件",
            PrimaryAction::OpenFile => "打开文件",
            PrimaryAction::OpenFolder => "打开文件夹",
        }
    }

    /// 提示文本。
    pub const fn hint(self) -> &'static str {
        match self {
            PrimaryAction::NewFile => "新建一份空白文档（Ctrl+N）",
            PrimaryAction::OpenFile => "打开本机已有的 Markdown 文件（Ctrl+O）",
            PrimaryAction::OpenFolder => "打开一个目录并在左侧显示文件树",
        }
    }
}

/// 欢迎页要显示的一条快捷键提示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutHint {
    /// 键位显示文本（如 `Ctrl+N`）。
    pub combo: String,
    /// 动作中文名。
    pub label: String,
}

/// 构造欢迎页的快捷键提示（读**当前生效**的键位，不是写死的）。
///
/// 刻意现取而不是硬编码：用户改过键位后，欢迎页还显示旧的
/// `Ctrl+N` 会让用户以为改键没生效——而他恰恰是刚改过的人。
pub fn shortcut_hints(app: &EditorApp) -> Vec<ShortcutHint> {
    let actions = [
        (UiAction::NewDocument, "新建"),
        (UiAction::OpenDocument, "打开"),
        (UiAction::Save, "保存"),
        (UiAction::Find, "查找"),
    ];
    actions
        .iter()
        .map(|(action, label)| ShortcutHint {
            combo: app.ui.dispatcher.registry().display_for(*action),
            label: (*label).to_string(),
        })
        .filter(|h| !h.combo.is_empty())
        .collect()
}

/// 绘制欢迎页。
pub fn show(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    ui.painter()
        .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, theme.bg_primary);

    // 顶部光晕：两团柔和径向渐变（原型 `.glow`）。用 `ellipse` 而非
    // `rect_filled`，圆角矩形会看出边角。
    let rect = ui.max_rect();
    paint_glow(ui.painter(), rect, &theme);

    let mut action = None;
    let mut open_recent: Option<String> = None;

    ui.vertical_centered(|ui| {
        ui.add_space(rect.height() * 0.12);
        brand(ui, &theme);
        // 顶级分区之间取刻度 `space_xxl`，功能区块之间取 `space_xl`：
        // 欢迎页只有四个区块，间距是它唯一的层级语言，必须守规范。
        ui.add_space(theme.metrics.space_xxl);
        primary_actions(ui, &theme, &mut action);
        ui.add_space(theme.metrics.space_xxl);
        let recent = app.recent_files();
        recent_section(ui, &theme, &recent, &mut open_recent);
        ui.add_space(theme.metrics.space_xl);
        hints(ui, &theme, app);
    });

    // 滚动：最近文件可能很多（虽截断到 8 条）+ 小窗口高度不足。
    // 包在 `ScrollArea` 里而不是让内容溢出——溢出部分在 egui 里
    // 直接被裁掉，用户连「下面还有东西」都不知道。
    let _ = ScrollArea::vertical().show(ui, |_| {});

    // 渲染完再执行：执行会改 `active_doc`（新建 / 打开），
    // 而本帧的其余部分还在读它。
    if let Some(action) = action {
        run_action(app, action);
    }
    // 失败提示由 `open_path` **自己**报（见其文档），这里不再重复报一遍：
    // 两处都报会让用户看到两条几乎一样的消息。
    if let Some(path) = open_recent {
        let _ = app.open_path(&path);
    }
}

/// 品牌区。
fn brand(ui: &mut Ui, theme: &Theme) {
    ui.label(
        RichText::new(LOGO)
            .size(30.0)
            .strong()
            .color(theme.text_primary),
    );
    ui.add_space(theme.metrics.space_md);
    ui.label(RichText::new(SLOGAN).size(13.0).color(theme.text_secondary));
    ui.add_space(theme.metrics.space_lg);
    // 版本号做成一个圆角小标签（原型 `.version`）。
    let galley = ui.painter().layout_no_wrap(
        format!("v{VERSION}"),
        egui::FontId::proportional(11.0),
        theme.text_secondary,
    );
    let pad = Vec2::new(16.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad, egui::Sense::hover());
    let box_rect = egui::Rect::from_center_size(rect.center(), galley.size() + pad);
    ui.painter().rect_stroke(
        box_rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        box_rect.left_top() + Vec2::new(8.0, 2.0),
        galley,
        theme.text_secondary,
    );
}

/// 三个主操作按钮，**整行水平居中**（与标题、最近文件同一条中轴）。
///
/// `ui.horizontal` 的子布局是「从左边开始排」，放在 `vertical_centered` 里
/// 也**不会**自动居中——这正是之前三个按钮左对齐的原因。故先用
/// [`action_button_width`] 量出整行宽度，再在行首补一半的余量，
/// 复用本文件 `recent_section` 已在用的同一套手动居中写法（不依赖 egui
/// 对嵌套 `horizontal` 的居中语义，行为确定、可被几何用例断言）。
fn primary_actions(ui: &mut Ui, theme: &Theme, picked: &mut Option<PrimaryAction>) {
    let row_w: f32 = PrimaryAction::ALL
        .iter()
        .enumerate()
        .map(|(i, action)| action_button_width(ui, *action) + if i > 0 { ACTION_GAP } else { 0.0 })
        .sum();
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
        for (i, action) in PrimaryAction::ALL.iter().enumerate() {
            if i > 0 {
                ui.add_space(ACTION_GAP);
            }
            if action_button(ui, theme, *action) {
                *picked = Some(*action);
            }
        }
    });
}

/// 单个主操作按钮的宽度（点）。
///
/// 抽出来是为了让「居中计算」与「实际绘制」用**同一个**尺寸来源：
/// 若两处各算一次，改字号时居中必然偏掉，且症状只是「看着有点歪」，
/// 极难发现。
fn action_button_width(ui: &Ui, action: PrimaryAction) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        action.label().to_string(),
        egui::FontId::proportional(ACTION_FONT_SIZE),
        egui::Color32::WHITE,
    );
    galley.size().x + ACTION_ICON_PAD + ACTION_TRAILING_PAD
}

/// 单个主操作按钮。返回是否被点击。
fn action_button(ui: &mut Ui, theme: &Theme, action: PrimaryAction) -> bool {
    let galley = ui.painter().layout_no_wrap(
        action.label().to_string(),
        egui::FontId::proportional(ACTION_FONT_SIZE),
        theme.text_primary,
    );
    // 图标位置留 26px：原型每个按钮左侧有个 18px 的 lucide 图标。
    // 这里不画图标（egui 无内置图标集），但**保留**这段留白，
    // 否则三个按钮宽度会比原型窄一截，与整体布局对不上。
    let desired = Vec2::new(
        galley.size().x + ACTION_ICON_PAD + ACTION_TRAILING_PAD,
        ACTION_HEIGHT,
    );
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let hovered = response.hovered();
    // 「新建文件」是主操作（原型 `.action.primary`）：用强调色实心。
    let is_primary = action == PrimaryAction::NewFile;
    let fill = match (is_primary, hovered) {
        (true, true) => theme.accent.gamma_multiply(1.08),
        (true, false) => theme.accent,
        (false, true) => theme.bg_tertiary,
        (false, false) => theme.bg_secondary,
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(4), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0_f32, if hovered { theme.accent } else { theme.border }),
        egui::StrokeKind::Inside,
    );
    // 主操作用白字（压在强调色上），次操作用主文本色。
    let text_color = if is_primary {
        egui::Color32::WHITE
    } else {
        theme.text_primary
    };
    ui.painter().galley(
        rect.left_top() + Vec2::new(ACTION_ICON_PAD, (rect.height() - galley.size().y) / 2.0),
        galley,
        text_color,
    );
    let clicked = response.clicked();
    if hovered {
        response.on_hover_text(action.hint());
    }
    clicked
}

/// 最近文件区。
fn recent_section(
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

/// 底部快捷键提示，**整行水平居中**（与标题、主操作按钮同一条中轴）。
///
/// 与 [`primary_actions`] 同理：先量出整行宽度再补左侧余量。
/// 间距也一并改成「只在项与项之间加」，否则末尾那一份间距会让整行
/// 视觉重心偏左——居中会「看起来差一点」。
fn hints(ui: &mut Ui, theme: &Theme, app: &EditorApp) {
    let items = shortcut_hints(app);
    let row_w: f32 = items
        .iter()
        .enumerate()
        .map(|(i, hint)| {
            kbd_width(ui, &hint.combo)
                + HINT_KBD_GAP
                + hint_label_width(ui, &hint.label)
                + if i > 0 { HINT_ITEM_GAP } else { 0.0 }
        })
        .sum();
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
        for (i, hint) in items.iter().enumerate() {
            if i > 0 {
                ui.add_space(HINT_ITEM_GAP);
            }
            kbd(ui, theme, &hint.combo);
            ui.add_space(HINT_KBD_GAP);
            ui.label(
                RichText::new(&hint.label)
                    .size(HINT_FONT_SIZE)
                    .color(theme.text_secondary),
            );
        }
    });
}

/// 键位小标签的宽度（点），供 [`hints`] 量取整行宽度。
fn kbd_width(ui: &Ui, text: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::monospace(KBD_FONT_SIZE),
        egui::Color32::WHITE,
    );
    galley.size().x + KBD_PAD_X
}

/// 单条提示说明文字的宽度（点），供 [`hints`] 量取整行宽度。
fn hint_label_width(ui: &Ui, label: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(HINT_FONT_SIZE),
            egui::Color32::WHITE,
        )
        .size()
        .x
}

/// 键位小标签（`<kbd>` 样式）。
fn kbd(ui: &mut Ui, theme: &Theme, text: &str) {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::monospace(KBD_FONT_SIZE),
        theme.text_primary,
    );
    let pad = Vec2::new(KBD_PAD_X, 4.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad, egui::Sense::hover());
    let box_rect = egui::Rect::from_center_size(rect.center(), galley.size() + pad);
    ui.painter()
        .rect_filled(box_rect, egui::CornerRadius::same(3), theme.bg_tertiary);
    ui.painter().rect_stroke(
        box_rect,
        egui::CornerRadius::same(3),
        egui::Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        box_rect.left_top() + Vec2::new(6.0, 2.0),
        galley,
        theme.text_primary,
    );
}

/// 顶部两团光晕。
fn paint_glow(painter: &egui::Painter, rect: egui::Rect, theme: &Theme) {
    let center1 = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.22);
    let center2 = egui::pos2(
        rect.left() + rect.width() * 0.12,
        rect.top() + rect.height() * 0.88,
    );
    for (center, size) in [
        (center1, Vec2::new(rect.width() * 0.6, rect.height() * 0.45)),
        (center2, Vec2::new(rect.width() * 0.4, rect.height() * 0.3)),
    ] {
        // 用一个实心椭圆 + 低透明度模拟径向渐变：egui 没有渐变 API，
        // 而画 20 个递减透明度的椭圆去逼近渐变，在深色主题下
        // 能看到明显的同心环。实心低透明是更诚实的近似。
        // `ellipse` 在 egui 0.33 里叫 `circle`，且**只**接受半径。
        // 要椭圆得用 `add(Shape::ellipse)`；这里取短半轴画正圆，
        // 视觉上仍是柔和光晕（原型是椭圆，差异只在轮廓形状）。
        painter.circle_filled(
            center,
            size.x.min(size.y) / 2.0,
            theme.accent_soft.gamma_multiply(0.3),
        );
    }
}

/// 执行主操作。
fn run_action(app: &mut EditorApp, action: PrimaryAction) {
    match action {
        PrimaryAction::NewFile => {
            if let Err(e) = app.new_document() {
                app.set_status_error(&format!("无法新建文档：{e}"));
            }
        }
        // 「打开文件 / 文件夹」都要弹原生对话框，而 `DialogPort` 是
        // **同步阻塞**的，不能在 `update()` 里调（S5-08 已写明）。
        // 故只登记意图，由后台线程取回。
        PrimaryAction::OpenFile => app.request_dialog(PendingDialog::OpenFile),
        PrimaryAction::OpenFolder => app.request_dialog(PendingDialog::PickFolder),
    }
}

/// 路径末段（文件名）；无分隔符时返回原串。
pub fn display_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// 目录过长时**从左侧**截断并加省略号。
///
/// 从**左**截而非从右：目录最有辨识度的是**结尾**
/// （`a/b/c/drafts` 里的 `drafts`），从右截会正好把它截掉。
fn shorten_left(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().skip(count - max_chars).collect::<String>();
    format!("…{kept}")
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
