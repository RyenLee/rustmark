//! 左侧行号栏：按本帧真实排版的行顶描画，以及光标行号换算。

use egui::{FontId, Ui};

use crate::presentation::theme::mapper;

/// 描画左侧行号栏（当前行号用强调色）。
///
/// # 为什么不用「每行一个 `ui.label`」
///
/// 旧实现把每个行号铺成一个独立 `ui.label`，与正文 `TextEdit` 各自排版。
/// 三条差异叠加成**稳定错位**：
///
/// 1. **行距不同**：独立 label 之间隔着 `item_spacing.y`，与 `TextEdit`
///    内部的行高不是同一个值；
/// 2. **字体不同**：label 用比例字体、正文用等宽字体，同一字号的行高不等；
/// 3. **换行**：自动换行（默认开）时一个逻辑行在 `TextEdit` 里折成多行，
///    而 label 仍只占一行——从第一条长行起按累积量越错越远。
///
/// 现改为：行号**不是**独立控件，而是按编辑器**本帧真实排版结果**画上去的——
/// [`mapper::line_tops`] 给出编辑器逐行 `Galley` 的顶边（含折行，折行那一行
/// 更高），配合 `galley_pos`（文本左上角）换算成屏幕坐标。
///
/// # 行高为什么现在是**等距**的（对齐的根本保障）
///
/// 编辑区所有内容（标题 / 正文 / 代码 / 列表 / 行内代码）现在共用同一字号与
/// 同一**显式行高**（`editor_font_size × editor_line_height`，见
/// [`crate::presentation::theme::mapper::StyleMapper::format`]）。显式行高一旦给出，
/// epaint 便不再按字体度量算行高——于是「中文回退字体行高更高」不再影响行距，
/// `tops[i+1] - tops[i]` 恒为同一个常数（除非该行折行）。
/// 行号按行带垂直居中描画，与正文的基线完全重合，重叠问题从结构上消失。
///
/// # 为什么按「行带垂直居中」而不是「行顶对齐」
///
/// 每个行号锚定在**本行的行带**（`tops[i] .. tops[i+1]`，即上一行顶到
/// 下一行顶）的垂直中心上。正文行的字形落在行带的**基线**上，而基线位置
/// 取决于该行实际用到的字体：中文经系统字体回退后，其 ascent 比纯等宽
/// 数字高出一截——若行号与行带**顶**对齐，数字会整体浮在文字上方，
/// 在行距紧凑时看起来就像与上一行文字重叠（中文的有序列表行行行相邻，
/// 最明显）。按行带居中后，行号的光学位置与文字无关地落在本行正中，
/// 对任何字体度量都稳定。
pub(super) fn paint_gutter(
    ui: &Ui,
    theme: &crate::presentation::theme::builtin::Theme,
    gutter_rect: egui::Rect,
    galley_pos: egui::Pos2,
    text: &str,
    cursor_line: usize,
    font_size: f32,
) {
    // 首帧（本帧尚未完成整篇排版）取不到行顶表：留空，下一帧补齐。
    // **绝不**凭空猜一个行高——那会让行号稳定地错位（而不是偶尔错位）。
    let Some(tops) = mapper::line_tops(theme, text) else {
        return;
    };
    let line_count = crate::shared::text::line_count(text);
    let font = FontId::monospace(font_size);
    let clip = ui.clip_rect();
    let painter = ui.painter();
    for i in 0..line_count {
        // `tops` 末元素是整篇高度的哨兵，故 `i` 必然落在有效范围内。
        let Some(&top) = tops.get(i) else {
            break;
        };
        let Some(&bottom) = tops.get(i + 1) else {
            break;
        };
        // 行带的垂直中心：与字体度量无关的「本行正中」。
        let y = galley_pos.y + (top + bottom) * 0.5;
        // 只画落在可视区内的行号：大文档里绝大部分行在视口之外，
        // 无谓地 `format!` 每一行会拖慢滚动。
        if y + font_size < clip.top() || y - font_size > clip.bottom() {
            continue;
        }
        let color = if i == cursor_line {
            theme.accent
        } else {
            theme.text_secondary
        };
        painter.text(
            egui::pos2(gutter_rect.right() - theme.metrics.space_sm, y),
            egui::Align2::RIGHT_CENTER,
            format!("{i}"),
            font.clone(),
            color,
        );
    }
}

/// 给定字节偏移，返回其所在行号（0-based）。
///
/// 规则本身（按字节数换行、越界夹紧、多字节中间不 panic）住在
/// [`crate::shared::text::line_index_at`]——状态栏与编辑器其它位置要的是同一个数，
/// 各写一份必然漂移。
pub(super) fn current_line_index(text: &str, byte: usize) -> usize {
    crate::shared::text::line_index_at(text, byte)
}
