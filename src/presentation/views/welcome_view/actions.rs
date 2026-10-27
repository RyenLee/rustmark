//! 三个主操作按钮、键位提示数据与动作执行。

use egui::{Ui, Vec2};

use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::state::pending_dialog::PendingDialog;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::EditorViewContext;

use super::*;

/// 欢迎页的主操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryAction {
    /// 新建一份空白文档（等价 [`UiAction::NewDocument`]）。
    NewFile,
    /// 打开本机已有的 Markdown 文件（等价 [`UiAction::OpenDocument`]）。
    OpenFile,
    /// 打开目录为工作区并在侧边栏显示文件树（等价 [`UiAction::OpenFolder`]）。
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
pub fn shortcut_hints(vctx: &dyn EditorViewContext) -> Vec<ShortcutHint> {
    let actions = [
        (UiAction::NewDocument, "新建"),
        (UiAction::OpenDocument, "打开"),
        (UiAction::Save, "保存"),
        (UiAction::Find, "查找"),
    ];
    actions
        .iter()
        .map(|(action, label)| ShortcutHint {
            combo: vctx.dispatcher_state().registry().display_for(*action),
            label: (*label).to_string(),
        })
        .filter(|h| !h.combo.is_empty())
        .collect()
}

/// 三个主操作按钮，**整行水平居中**（与标题、最近文件同一条中轴）。
///
/// `ui.horizontal` 的子布局是「从左边开始排」，放在 `vertical_centered` 里
/// 也**不会**自动居中——这正是之前三个按钮左对齐的原因。故先用
/// [`action_button_width`] 量出整行宽度，再在行首补一半的余量，
/// 复用本模块 `recent::recent_section` 已在用的同一套手动居中写法（不依赖 egui
/// 对嵌套 `horizontal` 的居中语义，行为确定、可被几何用例断言）。
pub(super) fn primary_actions(ui: &mut Ui, theme: &Theme, picked: &mut Option<PrimaryAction>) {
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

/// 执行主操作。
pub(super) fn run_action(vctx: &mut dyn EditorViewContext, action: PrimaryAction) {
    match action {
        PrimaryAction::NewFile => {
            if let Err(e) = vctx.new_document() {
                vctx.set_status_error(&format!("无法新建文档：{e}"));
            }
        }
        // 「打开文件 / 文件夹」都要弹原生对话框，而 `DialogPort` 是
        // **同步阻塞**的，不能在 `update()` 里调（S5-08 已写明）。
        // 故只登记意图，由后台线程取回。
        PrimaryAction::OpenFile => vctx.request_dialog(PendingDialog::OpenFile),
        PrimaryAction::OpenFolder => vctx.request_dialog(PendingDialog::PickFolder),
    }
}
