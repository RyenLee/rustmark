//! 导出对话框（S6-11）。
//!
//! # 四类格式的现状（诚实优先）
//!
//! 原型要求「导出四类各产出一次真实文件并人工打开确认」。本实现中：
//!
//! | 格式 | 状态 | 说明 |
//! |---|---|---|
//! | HTML | ✅ 可用 | `PulldownHtmlExporter`，含 TOC 与内联图片 |
//! | Markdown | ✅ 可用 | 直接写源码 |
//! | PDF | ✅ 可用（V11-04） | 装有 Edge / Chrome 时直接落盘；否则经系统打印对话框 |
//! | 图片 | ✅ 可用（P2） | **视口截图**：截取当前预览区所见 |
//!
//! PDF 的两种产出方式要在用户点导出**之前**讲清楚
//!（`ExportFormatChoice::pdf_caveat`）：「目标 `.pdf` 没出现」在经对话框那条路上是
//! **正常**的，不预告就会被当成失败。
//!
//! # 图片导出：视口截图与整篇长图
//!
//! 图片格式提供两种模式，都走 egui 官方截图 API（`ViewportCommand::Screenshot`）：
//!
//! - **视口截图**（默认）：截取预览区此刻所见，单帧完成。
//! - **整篇长图**（勾选）：把预览区逐屏滚动、分段截图，再在基础设施层
//!   纵向拼接成一张 PNG。egui 没有离屏重建布局的公开路径，
//!   这是复用截图回路的唯一可行做法；文档超过段数 / 高度上限时
//!   提前拒绝并建议改用 HTML / PDF 导出。
//!
//! **不做假的进度条**：原型的进度是 `setInterval` 随机数，纯装饰。
//! 装饰性进度条会让用户在导出早已完成后仍以为「还在导出」，从而再点一次。

use egui::{Align, Context, Layout, RichText, Ui};

use crate::app::frame::{EditorApp, PendingDialog};
use crate::application::dto::ExportRequestView;
use crate::presentation::state::dialog_ui_state::Overlay;
use crate::presentation::state::export_ui_state::{ExportFormatChoice, PageSizeChoice};
use crate::presentation::theme::builtin::Theme;

/// 对话框默认尺寸（对齐原型 `BaseModal :width="540" :height="430"`）。
const WIDTH: f32 = 540.0;
const HEIGHT: f32 = 430.0;

/// 输出目录输入框的固定 `Id`（打开后的焦点落点 + 可测锚点）。
pub const OUTPUT_DIR_INPUT_ID: &str = "rustmark.export.dir";

/// 文件名输入框的固定 `Id`。
///
/// 与 [`OUTPUT_DIR_INPUT_ID`] 一样必须是**显式** id，而不是 let egui 按布局
/// 位置自动生成：
///
/// 1. 自动 id 随控件在布局中的位置变化，浮层里加一行控件就可能换 id，
///    光标位置与撤销栈会跟着丢；
/// 2. 测试需要能问出「这个框能不能被选中」——egui 0.33 没有文本回读接口
///    （`ctx.debug_text` 是写入方向），控件状态是唯一稳定的外部可观察量。
pub const FILE_STEM_INPUT_ID: &str = "rustmark.export.stem";

/// 绘制导出对话框。
pub fn show(app: &mut EditorApp, ctx: &Context) {
    let mut open = true;
    egui::Window::new(Overlay::Export.title())
        .id(egui::Id::new("rustmark.overlay.export"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .default_height(HEIGHT)
        .show(ctx, |ui| body(app, ui));
    if !open {
        // 关闭时若还在导出中，中断不了底层任务，但必须把 UI 从
        // 「导出中」状态里放出来——否则对话框再打开时会一直显示
        // 「正在导出，请稍候」，而实际上早已没有任务在跑。
        //
        // 截图任务**不**在此作废：GPU 回传只认`pending_capture`，
        // 用户关掉对话框时那一帧已经发出去了，丢弃它等于让后端
        // 白做一次回读，而用户永远等不到文件。任务继续在后台兑现，
        // 结果由 `pump_capture` 写进状态；下次打开对话框能看到提示。
        app.export_ui.running = false;
        app.export_ui.close();
        app.dialog.close(Overlay::Export);
    }
    // 打开后把焦点交给输出路径输入框：导出是一次性动作，
    // 用户最可能先改的就是「存到哪」。
    //
    // **只给一次**（边沿触发）。这里绝不能用「输出路径框当前没有焦点就请求」
    // 当守卫：`Memory::has_focus` 只认控件 id，而窗口（`Area`）的 id 永远不会
    // 成为「有焦点的控件」，那个条件恒为真 → 每帧都抢 → 用户点进下面的
    // 「文件名」框会被立刻拽回来，一个字也打不进去。
    // 详见 `DialogUiState::take_open_focus` 的文档。
    if app.dialog.take_open_focus(Overlay::Export) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(OUTPUT_DIR_INPUT_ID)));
    }
}

/// 主体。
fn body(app: &mut EditorApp, ui: &mut Ui) {
    let theme = app.theme().clone();
    format_row(app, ui, &theme);
    ui.add_space(theme.metrics.space_sm);
    path_rows(app, ui, &theme);
    ui.separator();
    match app.export_ui.format {
        ExportFormatChoice::Markdown => markdown_note(ui, &theme),
        ExportFormatChoice::Pdf => pdf_options(app, ui, &theme),
        _ => common_options(app, ui, &theme),
    }
    ui.separator();
    messages(app, ui, &theme);
    footer(app, ui, &theme);
}

/// 格式单选组。
fn format_row(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.horizontal_wrapped(|ui| {
        ui.label("格式");
        for format in ExportFormatChoice::ALL {
            let selected = app.export_ui.format == *format;
            let available = format.is_available();
            let label = match format.unavailable_reason() {
                Some(_) => format!("{}（暂未开放）", format.label()),
                None => format.label().to_string(),
            };
            let text = if selected {
                RichText::new(label).color(theme.accent)
            } else if available {
                RichText::new(label)
            } else {
                RichText::new(label).color(theme.text_secondary)
            };
            if ui.selectable_label(selected, text).clicked() && !selected {
                app.export_ui.set_format(*format);
            }
        }
    });
}

/// 输出目录与文件名。
fn path_rows(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.label("输出路径");
        ui.add(
            egui::TextEdit::singleline(&mut app.export_ui.output_dir)
                .id(egui::Id::new(OUTPUT_DIR_INPUT_ID))
                .hint_text("导出到（留空则用当前目录）")
                .desired_width(280.0),
        );
        // 「选择文件夹」：输出目录很可能是用户不记得的绝对路径，
        // 纯手填容易填错。按钮走原生目录选择对话框（后台线程执行，
        // 见 `dialog_runner`），结果由 `apply_dialog_outcome` 回填到
        // `output_dir`——它按「导出浮层是否打开」区分去向，不会误改工作区。
        if ui.button("选择文件夹…").clicked() {
            app.request_dialog(PendingDialog::PickFolder);
        }
    });
    // 输出目录存在性检查：不存在时**当场**说清「导出会自动创建」，
    // 而不是等提交后拿到一条底层的「找不到路径」。自动创建在
    // `EditorApp::export_current_document` 里真正兑现（幂等）。
    let dir = app.export_ui.output_dir.trim();
    if !dir.is_empty() && !std::path::Path::new(dir).is_dir() {
        ui.colored_label(
            theme.warning,
            format!("输出目录不存在：{dir}（导出时会自动创建）"),
        );
    }
    let stem_changed = ui
        .horizontal(|ui| {
            ui.label("文件名");
            ui.add(
                egui::TextEdit::singleline(&mut app.export_ui.file_stem)
                    .id(egui::Id::new(FILE_STEM_INPUT_ID))
                    .desired_width(320.0)
                    .hint_text("文档名"),
            )
        })
        .inner
        .changed();
    if stem_changed {
        app.export_ui.refresh_resolved_path();
    }
    if let Some(reason) = app.export_ui.format.unavailable_reason() {
        ui.colored_label(theme.text_secondary, reason);
    } else {
        ui.colored_label(
            theme.text_secondary,
            format!("将导出到 {}", app.export_ui.resolved_path),
        );
    }
}

/// Markdown 格式说明（源码导出没有格式选项）。
fn markdown_note(ui: &mut Ui, theme: &Theme) {
    ui.label("直接导出编辑区原始 Markdown 源码，不做渲染与样式处理。");
    ui.colored_label(
        theme.text_secondary,
        "目录 / 分页选项对 Markdown 不适用，故不显示。",
    );
}

/// 通用格式选项（HTML / 图片）。
fn common_options(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let format = app.export_ui.format;
    if format == ExportFormatChoice::Image {
        // 图片走截图路径：内容、目录、分页对它都没有意义。
        // 仍显示这些开关只会让用户以为勾了会影响结果。
        // 唯一有意义的选项是「整篇长图」：决定截图的范围模式。
        let mut long = app.export_ui.long_image;
        ui.horizontal_wrapped(|ui| {
            if ui
                .checkbox(&mut long, "整篇长图（滚动截取全部内容）")
                .changed()
            {
                app.export_ui.long_image = long;
            }
        });
        image_note(ui, theme, long);
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if format.supports_toc() {
            let mut on = app.export_ui.include_toc;
            if ui.checkbox(&mut on, "包含目录").changed() {
                app.export_ui.include_toc = on;
            }
        }
        let mut embed = app.export_ui.embed_images;
        if ui.checkbox(&mut embed, "内联图片（data URI）").changed() {
            app.export_ui.embed_images = embed;
        }
    });
}

/// 图片格式的说明（视口截图 / 整篇长图的能力边界）。
fn image_note(ui: &mut Ui, theme: &Theme, long_image: bool) {
    if long_image {
        ui.label("自动滚动预览区逐屏截取，拼接为一张完整长图 PNG。");
        ui.colored_label(
            theme.text_secondary,
            "内容不足一屏时与视口截图等价；过长或过高会提示改用 HTML / PDF 导出。",
        );
    } else {
        ui.label("截取当前预览区所见，保存为 PNG。");
        ui.colored_label(
            theme.text_secondary,
            "范围＝预览区在窗口中的可见部分（不含滚动区之外的内容）。",
        );
    }
}

/// PDF 专属选项（纸张 / 分页 / 目录）。
fn pdf_options(app: &mut EditorApp, ui: &mut Ui, _theme: &Theme) {
    ui.horizontal_wrapped(|ui| {
        ui.label("纸张");
        for size in PageSizeChoice::ALL {
            let selected = app.export_ui.page_size == *size;
            if ui.selectable_label(selected, size.label()).clicked() && !selected {
                app.export_ui.page_size = *size;
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        let mut paginate = app.export_ui.paginate;
        if ui.checkbox(&mut paginate, "按标题分页").changed() {
            app.export_ui.paginate = paginate;
        }
        let mut toc = app.export_ui.include_toc;
        if ui.checkbox(&mut toc, "包含目录（需分页）").changed() {
            app.export_ui.include_toc = toc;
            // 领域规则：PDF 的目录必须配分页（`TocRequiresPagination`）。
            // 与其让用户提交后看到一条他无从理解的错误，不如在此自动补上分页。
            if toc {
                app.export_ui.paginate = true;
            }
        }
    });
    ui.colored_label(
        _theme.text_secondary,
        "纸张、页边距与分页位置会写入导出件的 `@page` 与 `break-before` 规则。",
    );
    // V11-04：PDF 的产出方式与其它格式完全不同（经系统打印对话框），
    // 必须**在用户点导出之前**讲清楚，否则他会以为导出失败。
    if let Some(caveat) = app.export_ui.format.pdf_caveat() {
        ui.colored_label(_theme.text_secondary, caveat);
    }
}

/// 错误 / 成功提示。
fn messages(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    if let Some(error) = app.export_ui.error.clone() {
        ui.colored_label(theme.error, format!("⚠ {error}"));
    }
    if let Some(notice) = app.export_ui.notice.clone() {
        ui.colored_label(theme.success, notice);
    }
    if let Some(reason) = app.export_ui.blocked_reason()
        && app.export_ui.error.is_none()
    {
        ui.weak(reason);
    }
}

/// 页脚：取消 / 导出。
fn footer(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.weak(format!(
            "扩展名必须是 .{}",
            app.export_ui.format.extension()
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if app.export_ui.running {
                "导出中…"
            } else {
                "导出"
            };
            let button = egui::Button::new(RichText::new(label).color(theme.accent));
            if ui.add_enabled(app.export_ui.can_submit(), button).clicked() {
                // `pixels_per_point` 在点击时刻读取并传下去：长图模式
                // 要在登记阶段就用它估算成品高度、提前拦截超限请求。
                let ppp = ui.ctx().pixels_per_point();
                run_export(app, ppp);
            }
            if ui.button("取消").clicked() {
                app.export_ui.close();
                app.dialog.close(Overlay::Export);
            }
        });
    });
}

/// 真正执行一次导出。
///
/// 编排全在 [`EditorApp::export_current_document`]（应用层两段式），
/// 这里只把 UI 状态翻译成 DTO 并把结果翻译成提示文案。
fn run_export(app: &mut EditorApp, pixels_per_point: f32) {
    app.export_ui.begin();
    let path = app.export_ui.resolved_path.clone();

    // 图片走**截图**路径：像素来自 GPU 对当前帧的回读，
    // 由 `pump_capture` 在下一帧兑现，不经过同步的 `export_current_document`。
    if app.export_ui.format == ExportFormatChoice::Image {
        // 勾了「整篇长图」走多段滚动拼接；否则单帧视口截图。
        // 两条路径都在帧循环里异步兑现（`pump_capture`），错误文案同理。
        let result = if app.export_ui.long_image {
            app.begin_long_capture(&path, pixels_per_point)
        } else {
            app.begin_viewport_capture(&path)
        };
        match result {
            Ok(()) => {
                // 结果（成功/失败）在后续帧由 `pump_capture` 写入；
                // 这里保持 `running`，按钮据此显示「导出中…」并禁用重复提交。
            }
            Err(e) => app.export_ui.fail(e.to_string()),
        }
        return;
    }

    let request = ExportRequestView {
        format: app.export_ui.format.id().to_string(),
        include_toc: app.export_ui.include_toc,
        embed_images: app.export_ui.embed_images,
        theme: app.export_ui.theme.clone(),
        page_size: app.export_ui.page_size.id().to_string(),
        paginate: app.export_ui.paginate,
    };
    match app.export_current_document(request, &path) {
        Ok(result) => {
            if app.export_ui.format == ExportFormatChoice::Pdf && result.bytes == 0 {
                // PDF 有两种产出方式，靠 `bytes` 区分：
                // - 直接落盘（无头内核）：`bytes > 0`，走下面的通用文案；
                // - 经系统打印对话框：本进程拿不到最终大小，`bytes` 恒为 0。
                //   显示「已导出 xxx（0 字节）」会让人以为失败，故改述实际行动。
                app.export_ui.succeed(format!(
                    "已生成打印用 HTML 并调起系统打印（目标 {}）：请在对话框中选择「另存为 PDF」",
                    result.output_path.as_str()
                ));
            } else {
                app.export_ui.succeed(format!(
                    "已导出 {}（{} 字节）",
                    result.output_path.as_str(),
                    result.bytes
                ));
            }
        }
        Err(e) => app.export_ui.fail(e.to_string()),
    }
}
