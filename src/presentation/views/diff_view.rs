//! 差异对比浮层（S6-11f）。
//!
//! # 对比的是「当前文档 vs 工作区里的另一个文件」
//!
//! S6-11f 敲定的语义：左侧是**当前文档**（含未保存的编辑），
//! 右侧从工作区条目里选另一个文件。这样「我改了哪些」与
//! 「这两份文件差在哪」共用同一个面板。
//!
//! 刻意**不做**「与磁盘已保存版本对比」：那需要新增「读磁盘原始字节」
//! 的通路，而 `WorkspaceRepository::read_content` 读的是文件当前内容，
//! 对未保存的文档它与「另一个文件」在用户心智里不是一回事。
//!
//! # 结果**实时重算**，不缓存
//!
//! 用户改一个字符就期待差异立刻更新。缓存会让他看到过期的差异，
//! 而「差异看起来没变」比「慢半拍」更让人不信任。

use egui::{Align, Context, Layout, RichText, ScrollArea, Ui, Vec2};

use crate::app::frame::EditorApp;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::state::diff_ui_state::{CharSeg, CharSegKind, DisplayRow};
use crate::presentation::theme::builtin::Theme;
use rustmark_core::shared::path::file_name_of;

/// 对话框默认尺寸（对齐原型 `BaseModal :width="760" :height="560"`）。
const WIDTH: f32 = 760.0;
const HEIGHT: f32 = 560.0;
/// 单行高度（等宽排版时与行高一致，避免行距忽变）。
const ROW_H: f32 = 18.0;
/// 行号列宽。
const GUTTER: f32 = 46.0;
/// 标记列宽（+ / -）。
const SIGN_W: f32 = 16.0;

/// 绘制差异对比浮层。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::Diff.title())
        .id(egui::Id::unique("rustmark.overlay.diff"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    if !open {
        app.dialog.close(Overlay::Diff);
    }
}

/// 主体。
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    header(app, ui, &theme);
    ui.separator();

    // 没有对照文件时**只显示选择器**并明说原因，
    // 而不是显示一个空列表让用户以为「两文件完全一致」。
    let Some(right_path) = app.diff_ui.right_path.clone() else {
        ui.add_space(theme.metrics.space_md);
        picker(app, ui, &theme);
        return;
    };

    // 实时重算：左侧可能因用户正在编辑而每帧都变。
    let left = app.current_content().unwrap_or_default();
    let right = match app.read_workspace_file(&right_path) {
        Ok(text) => text,
        Err(e) => {
            app.set_diff_error(format!("读取对照文件失败：{e}"));
            String::new()
        }
    };
    app.diff_ui.recompute(&left, &right);

    if let Some(err) = app.diff_ui.error.clone() {
        ui.colored_label(theme.error, format!("⚠ {err}"));
        ui.add_space(theme.metrics.space_xs);
    }
    ui.horizontal(|ui| {
        // D-3 阶段 3：读出 → 就地切换 → 变化才值写回（字段的可变引用不出 state）。
        let mut only_changes = app.diff_ui.only_changes;
        if ui.checkbox(&mut only_changes, "只看差异行").changed() {
            app.diff_ui.set_only_changes(only_changes);
        }
        ui.label(
            RichText::new(format!(
                "+{}  −{}",
                app.diff_ui.stat.added, app.diff_ui.stat.removed
            ))
            .color(theme.text_secondary),
        );
        if app.diff_ui.is_identical() {
            ui.colored_label(theme.success, "两侧内容完全一致");
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("换一个文件").clicked() {
                // 清选择 + 清结果行是一件事，缺一半会让旧结果与错误提示同框。
                app.clear_diff_selection();
            }
            if ui.button("关闭").clicked() {
                app.dialog.close(Overlay::Diff);
            }
        });
    });
    ui.add_space(theme.metrics.space_xs);

    // 降级时**明说**：不提示的话用户会以为「这个文件全变了」，
    // 而真相是「我们算不动，只整段标了」。
    if app.diff_ui.degraded {
        ui.colored_label(
            theme.warning,
            "文件过大，已降级为整段对比：只能看出「变了」，看不出改了哪些行。",
        );
        ui.add_space(theme.metrics.space_xs);
    }

    // 取一份快照再渲染：`visible_rows` 借用了 `app.diff_ui`，
    // 而绘制过程要 `&mut Ui` —— 两者可共存，但先把数据拷出来
    // 可以让 `rows` 保持纯函数（不碰 app），测试也能直接验它。
    let visible = app.diff_ui.visible_rows();
    let owned: Vec<DisplayRow> = visible.into_iter().cloned().collect();
    rows(ui, &theme, &owned);
}

/// 头部：两侧文件名。
fn header(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("当前").size(11.0).color(theme.text_secondary));
        ui.label(
            RichText::new(app.diff_ui.left_title.clone())
                .size(12.0)
                .strong(),
        );
        ui.label(RichText::new("⇄").size(12.0).color(theme.text_secondary));
        ui.label(RichText::new("对照").size(11.0).color(theme.text_secondary));
        ui.label(
            RichText::new(app.diff_ui.right_title.clone())
                .size(12.0)
                .strong(),
        );
    });
}

/// 对照文件选择器。
fn picker(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.label(RichText::new("选择一个工作区里的文件与当前文档对比：").size(12.0));
    let files: Vec<(String, bool)> = app
        .workspace_entries
        .items
        .iter()
        .filter(|(_, is_dir)| !*is_dir)
        .cloned()
        .collect();
    if files.is_empty() {
        ui.colored_label(
            theme.text_secondary,
            "当前没有已装载的工作区文件。请先在左侧「打开文件夹」。",
        );
        return;
    }
    ScrollArea::vertical()
        .id_salt("rustmark.diff.picker")
        .max_height(HEIGHT - 120.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (path, _) in &files {
                let name = file_name_of(path);
                if ui
                    .add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::new(name).frame(false),
                    )
                    .clicked()
                {
                    let title = name.to_string();
                    app.diff_ui.open(title.clone(), path.clone(), title);
                    // 内容在下一帧的头部重算时填充：这里刚把路径写进去，
                    // 立刻读盘会让这一帧的列表还拿不到 rows。
                }
            }
        });
}

/// 差异行列表。
fn rows(ui: &mut Ui, theme: &Theme, visible: &[DisplayRow]) {
    if visible.is_empty() {
        ui.colored_label(theme.text_secondary, "没有可显示的行。");
        return;
    }
    ScrollArea::vertical()
        .id_salt("rustmark.diff.rows")
        .max_height(HEIGHT - 150.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for row in visible {
                draw_row(ui, theme, row);
            }
        });
}

/// 画一行。
fn draw_row(ui: &mut Ui, theme: &Theme, row: &DisplayRow) {
    match row {
        DisplayRow::Equal {
            text,
            old_no,
            new_no,
        } => {
            let rect = gutter_row(ui, theme, Some(*old_no), Some(*new_no), "", None);
            let _ = rect;
            ui.monospace(RichText::new(text.clone()).color(theme.text_secondary));
        }
        DisplayRow::Remove { text, old_no } => {
            gutter_row(ui, theme, Some(*old_no), None, "−", Some(theme.diff_del_bg));
            ui.monospace(RichText::new(text.clone()).color(theme.diff_del));
        }
        DisplayRow::Add { text, new_no } => {
            gutter_row(ui, theme, None, Some(*new_no), "+", Some(theme.diff_add_bg));
            ui.monospace(RichText::new(text.clone()).color(theme.diff_add));
        }
        DisplayRow::Replace {
            old_text,
            new_text,
            old_no,
            new_no,
            old_segs,
            new_segs,
        } => {
            // 替换画两行（先是旧、后是新），各自高亮变化字符。
            segs_row(
                ui,
                theme,
                Some(*old_no),
                None,
                "−",
                theme.diff_del_bg,
                &old_text.clone(),
                old_segs,
                CharSegKind::Remove,
                theme.diff_del,
            );
            segs_row(
                ui,
                theme,
                None,
                Some(*new_no),
                "+",
                theme.diff_add_bg,
                &new_text.clone(),
                new_segs,
                CharSegKind::Add,
                theme.diff_add,
            );
        }
    }
}

/// 行号栏的两列编号串（旧 | 新），缺哪一侧就留等宽空格。
///
/// # 为什么单独一个纯函数
///
/// 「两列各宽 4、中间一个空格」是**一条**排版约定，此前在
/// [`gutter_row`] 与 [`segs_row`] 里各写了一遍。两处必须逐字一致：
/// 一旦宽度分家，同一行在「整行替换」与「字符级替换」两种画法下行号
/// 会左右错位，看上去像行号栏在抖。抽成纯函数后这条约定只有一处，
/// 且能在没有窗口的环境里直接断言。
///
/// # 四种情形必须**等宽**
///
/// 左侧缺号（纯新增行）与右侧缺号（纯删除行）都要留出对方的格子：
/// 四个分支一律 9 个字符（`4 + 空格 + 4`）。原先「左侧缺号」那条少写
/// 一个空格（8 字符），纯新增行的**新行号整体左移一格**，与上下文行
/// 对不齐——行号栏看着在抖。回归用例 [`tests::every_arm_is_the_same_width`]
/// 钉死这条不变量。
fn gutter_numbers(old_no: Option<usize>, new_no: Option<usize>) -> String {
    match (old_no, new_no) {
        (Some(o), Some(n)) => format!("{o:>4} {n:>4}"),
        (Some(o), None) => format!("{o:>4}     "),
        (None, Some(n)) => format!("     {n:>4}"),
        (None, None) => "         ".to_string(),
    }
}

/// 在已分配的行矩形里画行号与 `+` / `−` 标记。
///
/// 底色由调用方先画（整行底色在分配矩形后立刻铺满），这里只管文字：
/// 两处调用点的底色策略不同（`gutter_row` 可无底色、`segs_row` 必有），
/// 而文字位置与字号必须完全一致。
fn paint_gutter(
    ui: &mut Ui,
    theme: &Theme,
    rect: egui::Rect,
    old_no: Option<usize>,
    new_no: Option<usize>,
    sign: &str,
) {
    let galley = ui.painter().layout_no_wrap(
        gutter_numbers(old_no, new_no),
        egui::FontId::monospace(11.0),
        theme.text_secondary,
    );
    ui.painter().galley(
        rect.left_top() + egui::vec2(4.0, (rect.height() - galley.size().y) / 2.0),
        galley,
        theme.text_secondary,
    );
    if !sign.is_empty() {
        let sign_galley = ui.painter().layout_no_wrap(
            sign.to_string(),
            egui::FontId::monospace(12.0),
            theme.text_primary,
        );
        ui.painter().galley(
            rect.left_top() + egui::vec2(GUTTER, (rect.height() - sign_galley.size().y) / 2.0),
            sign_galley,
            theme.text_primary,
        );
    }
}

/// 行号 + 标记列 + 整行底色。返回正文可用区域。
fn gutter_row(
    ui: &mut Ui,
    theme: &Theme,
    old_no: Option<usize>,
    new_no: Option<usize>,
    sign: &str,
    fill: Option<egui::Color32>,
) -> egui::Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), egui::Sense::hover());
    if let Some(fill) = fill {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, fill);
    }
    paint_gutter(ui, theme, rect, old_no, new_no, sign);
    let body = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(GUTTER + SIGN_W, rect.top()),
        rect.right_bottom(),
    );
    ui.allocate_space(Vec2::new(body.width(), 0.0));
    body
}

/// 带字符级差异的一行。
#[allow(clippy::too_many_arguments)] // egui 渲染函数；主题/坐标/两侧行号由 UI 上下文逐帧传入，打包成 struct 徒增 borrow 约束
fn segs_row(
    ui: &mut Ui,
    theme: &Theme,
    old_no: Option<usize>,
    new_no: Option<usize>,
    sign: &str,
    fill: egui::Color32,
    text: &str,
    segs: &[CharSeg],
    hot: CharSegKind,
    hot_color: egui::Color32,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, fill);
    paint_gutter(ui, theme, rect, old_no, new_no, sign);
    // 逐段绘制：变化段用强调色，其余用普通文本色。
    // 不能用单个 `monospace` Label——那样就没有段级着色。
    let mut x = rect.left() + GUTTER + SIGN_W + 4.0;
    let base_y = rect.center().y;
    let font = egui::FontId::monospace(12.0);
    for seg in segs {
        let color = if seg.kind == hot {
            hot_color
        } else {
            theme.text_primary
        };
        // `FontId` 在 egui 0.33 里含 `Arc`，不是 `Copy`——
        // 每段克隆一次而不是把它移走，否则第一次用完就没了。
        let seg_galley = ui
            .painter()
            .layout_no_wrap(seg.text.clone(), font.clone(), color);
        // `painter.galley` **按值消耗** galley，宽度要在消耗前取。
        let seg_w = seg_galley.size().x;
        ui.painter().galley(
            egui::pos2(x, base_y - seg_galley.size().y / 2.0),
            seg_galley,
            color,
        );
        x += seg_w;
    }
    if text.is_empty() && segs.is_empty() {
        // 空行也占位，否则连续空行会「塌掉」——行号与内容错位。
        let _ = ui.allocate_exact_size(Vec2::new(1.0, 1.0), egui::Sense::hover());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四种情形一律 `4 + 1 + 4` 个字符——宽度不同就会让行号左右抖。
    ///
    /// 这是一条**像素级**不变量：`layout_no_wrap` 用等宽字体排版，
    /// 字符数不同 ⇒ 起点不同。纯新增行曾经少一个空格（8 字符），
    /// 新行号整体左移一格，与上下文行对不齐。
    #[test]
    fn every_arm_is_the_same_width() {
        let arms = [
            gutter_numbers(Some(1), Some(1)),
            gutter_numbers(Some(1), None),
            gutter_numbers(None, Some(1)),
            gutter_numbers(None, None),
        ];
        for s in &arms {
            assert_eq!(s.chars().count(), 9, "{s:?} 必须是 9 个字符");
        }
    }

    /// 左侧缺号时，新行号仍落在**第二列**（与两侧都有号时同一位置）。
    #[test]
    fn missing_left_number_keeps_the_right_column_aligned() {
        // 同一个「42」在两种画法下必须落在同一列。
        let both = gutter_numbers(Some(9), Some(42));
        let only_new = gutter_numbers(None, Some(42));
        assert_eq!(
            both.find("42"),
            only_new.find("42"),
            "新行号必须与「两列都有」时同一列：both={both:?} only_new={only_new:?}"
        );
        // 同理：右侧缺号时旧行号仍在第一列。
        let both = gutter_numbers(Some(42), Some(7));
        let only_old = gutter_numbers(Some(42), None);
        assert_eq!(
            both.find("42"),
            only_old.find("42"),
            "旧行号必须与「两列都有」时同一列：both={both:?} only_old={only_old:?}"
        );
    }

    /// 三位数不补零、四位数占满：右对齐是「位宽固定」而不是「补零」。
    #[test]
    fn numbers_are_right_aligned_not_zero_padded() {
        assert_eq!(gutter_numbers(Some(7), Some(8)), "   7    8");
        assert_eq!(gutter_numbers(Some(1234), Some(5678)), "1234 5678");
    }
}
