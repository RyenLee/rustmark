//! 列表 / 任务列表：缩进、标记列与任务回写。

use egui::{Align, RichText};

use rustmark_core::application::dto::ListItemView;

use super::blocks::draw_text_with_images;
use super::inline::Typography;
use super::*;

/// 列表每加深一级增加的缩进（点）。
///
/// 取刻度 `space_lg` 而不是固定像素：缩进必须与主题的间距体系同源，
/// 否则换主题后列表会与段落、引用条的左缘参差不齐。
fn list_indent_step(theme: &Theme) -> f32 {
    theme.metrics.space_lg
}

/// 列表标记（`•` / `1.`）所在列的固定宽度。
///
/// 必须**固定**并右对齐：`1.` 与 `10.` 宽度差一个字符，不固定列宽的话
/// 两位数序号会把该项文本整体右推一格，同一列表里文字左缘就参差了。
fn list_marker_width(theme: &Theme) -> f32 {
    theme.metrics.preview_font_size * 1.8
}

/// 无序列表按层级换用不同符号（`•` → `◦` → `▪`，三层一循环）。
///
/// 只靠缩进区分层级在**窄面板**里不够用：面板一窄，每级缩进被压到几个点，
/// 加上软换行后子项可能顶到父项的缩进位，视觉上就分不清谁是谁的孩子。
/// 换符号是零成本的第二重线索。
fn bullet_for(depth: u8) -> &'static str {
    match depth % 3 {
        0 => "•",
        1 => "◦",
        _ => "▪",
    }
}

/// 列表：有序用递增编号，无序用圆点；**按 `depth` 缩进**、按 `ordinal` 编号。
///
/// # 缩进为什么由数据给出而不是从文本推
///
/// 之前的实现只画一层 `space_xl` 缩进，所有层级看起来都是平的——因为
/// `BlockView` 当初根本没带层级信息。现在解析器把整棵列表树**拍平**进
/// 同一个 `items`（见其 `root_list_index`），层级只存在于 `ListItemView::depth`：
/// 想从 `text` 的前导空格反推是行不通的（pulldown 已把标记与缩进吃掉）。
pub(super) fn draw_list(
    vctx: &mut dyn EditorViewContext,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    wrap: bool,
    internal_targets: &[(String, u64)],
) {
    let marker_w = list_marker_width(theme);
    for item in block.items.iter() {
        ui.horizontal(|ui| {
            // 基准缩进 + 每级一格。
            ui.add_space(theme.metrics.space_xl + f32::from(item.depth) * list_indent_step(theme));
            // 标记列右对齐（理由见 `list_marker_width`）。
            ui.add_sized(
                egui::vec2(marker_w, theme.metrics.preview_font_size),
                egui::Label::new(
                    RichText::new(match item.ordinal {
                        Some(n) => format!("{n}."),
                        None => bullet_for(item.depth).to_string(),
                    })
                    .size(theme.metrics.preview_font_size)
                    .color(theme.syn_marker),
                )
                .halign(Align::RIGHT),
            );
            draw_item_text(vctx, ui, theme, item, wrap, internal_targets);
        });
    }
}

/// 任务列表：复选框可点击，点击后回写源码。
pub(super) fn draw_task_list(
    vctx: &mut dyn EditorViewContext,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    internal_targets: &[(String, u64)],
) {
    let wrap = vctx.editor_state_read().word_wrap;
    for item in block.items.iter() {
        let mut checked = item.checked.unwrap_or(false);
        ui.horizontal(|ui| {
            ui.add_space(theme.metrics.space_xl + f32::from(item.depth) * list_indent_step(theme));
            if ui.checkbox(&mut checked, "").changed() {
                toggle_task(vctx, item, checked);
            }
            draw_item_text(vctx, ui, theme, item, wrap, internal_targets);
        });
    }
}

/// 切换一个任务项的勾选状态：把源码里的 `[ ]` ↔ `[x]` 换掉。
///
/// 走与编辑区完全相同的 `ReplaceRangeCommand`，因此可撤销、会推进文档版本号、
/// 会触发预览重解析——不会出现「预览勾上了、源码没变」的两处真相漂移。
fn toggle_task(vctx: &mut dyn EditorViewContext, item: &ListItemView, checked: bool) {
    // ⚠ 这里必须是 `to_string()` 而不是 `clone()`：`vctx.editor_text()` 给的是
    // `&str`，而 `&str` **自身**实现了 `Clone`（`impl Clone for &T`），方法解析
    // 会先命中它 → `clone()` 得到 `&str` 而非 `String`，于是 `text` 变成对
    // 上下文的**长期借用**，把后面 `replace_source` 的可变借用全堵死。
    let text = vctx.editor_text().to_string();
    let Some(from) = task_marker_offset(&text, item.start, item.end) else {
        return;
    };
    vctx.replace_source(from, from + 3, if checked { "[x]" } else { "[ ]" });
}

/// 在文本的一段区间里定位任务标记 `[ ]` / `[x]`，返回其**绝对**字节偏移。
///
/// 只认区间内**首个**标记：一个列表项里出现多个复选框是极端的写法，
/// 而「改错位置」的代价（把用户别的文字换成 `[x]`）远大于「不生效」。
///
/// 区间超界时按文本长度裁剪——`BlockView` 的区间来自上一次解析，
/// 之后的编辑可能让它们落在文本之外（此时直接放弃，下一帧解析结果会自愈）。
pub fn task_marker_offset(text: &str, start: usize, end: usize) -> Option<usize> {
    let len = text.len();
    let start = start.min(len);
    let end = end.min(len).max(start);
    // 逐字节扫描而非 `char_indices`：标记的括号是 ASCII，
    // 但 `find` 的结果必须是**字节**偏移才能直接用于替换。
    let bytes = &text.as_bytes()[start..end];
    let pos = bytes.windows(3).position(|w| w == b"[ ]" || w == b"[x]")?;
    Some(start + pos)
}

/// 列表项文本：走行内富文本（`- **粗体**` 必须与正文里的粗体一致），
/// 已勾选的项叠上删除线 + 次级色。
fn draw_item_text(
    vctx: &mut dyn EditorViewContext,
    ui: &mut Ui,
    theme: &Theme,
    item: &ListItemView,
    wrap: bool,
    internal_targets: &[(String, u64)],
) {
    let size = theme.metrics.preview_font_size;
    let max_w = if wrap {
        ui.available_width()
    } else {
        f32::INFINITY
    };
    // 项文本里可能混着图片（`- ![图标](u.png) 说明`）：列表项是文本容器，
    // 图片只能插在文本流里（见 `draw_text_with_images`）。
    draw_text_with_images(
        vctx,
        ui,
        theme,
        &item.text,
        &item.spans,
        &item.images,
        Typography {
            size,
            line_height: size * theme.metrics.preview_line_height,
            base_style: if item.checked == Some(true) {
                "task_done"
            } else {
                ""
            },
        },
        egui::Sense::hover(),
        max_w,
        internal_targets,
    );
}
