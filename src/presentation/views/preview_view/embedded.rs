//! 数学公式与 Mermaid 图表：内置引擎渲染 + 源码卡降级。

use egui::FontId;

use crate::presentation::rich::render::{self, HAlign};
use crate::presentation::theme::mapper::atlas_token;
use crate::presentation::views::{math, mermaid};

use super::code::draw_highlighted_code;
use super::embedded_cache;
use super::*;

/// 数学公式：**内置排版引擎**渲染（见 [`math`]）。
///
/// # 这次改了什么
///
/// 旧实现只把公式源码用等宽字打出来，并在旁边写「导出 HTML 后由浏览器渲染」——
/// 用户看到的是 `\int_0^1 x^2 dx` 这串字符，而不是积分式。这正是问题 3 的根因：
/// **解析层一直是对的**（`Math { display: true }` 与公式本体都拿到了），
/// 缺的是渲染。
///
/// 现在：
/// - 排版成功 → 用内置引擎画成二维公式（分数线、上下标、根号、矩阵都各就各位）；
/// - 排版失败（子集外的写法）→ **回退**到源码卡，并**写明原因**
///   （`\notarealcommand` 这类不支持的命令名会出现在提示里）。
///
/// # 为什么保留「复制」
///
/// 排版后的公式是**图**，选不中也复制不了。用户把公式贴到别处时需要的仍是
/// LaTeX 源码，故按钮留着。
pub(super) fn draw_math(ui: &mut Ui, theme: &Theme, block: &BlockView, wrap: bool) {
    let m = theme.metrics;
    let display = block.display.unwrap_or(false);
    // 块级公式放大一档：它是版面上的「展示元素」，与正文同号会显得局促。
    let size = if display {
        m.preview_font_size * 1.12
    } else {
        m.preview_font_size
    };
    // 排版结果按 (内容, 字号, 展示态, 图集令牌) 帧间记忆化（F-4）：
    // 此前每帧全额重排，含公式的文档滚动时每帧都付「解析 + 逐标签度量」。
    // 命中时下面的 `measure` 闭包根本不会被调用。
    // 图集令牌随主题色之外的字体重建一起变（V11-17 的教训），必须进键。
    let layout = {
        let ui_ref: &Ui = ui;
        let measure = move |text: &str, size: f32| {
            ui_ref
                .painter()
                .layout_no_wrap(
                    text.to_owned(),
                    FontId::proportional(size),
                    egui::Color32::WHITE,
                )
                .size()
                .x
        };
        embedded_cache::math_layout(
            &block.content,
            size,
            display,
            atlas_token(ui.ctx()),
            &measure,
        )
    };

    let Ok(math_layout) = layout.as_ref() else {
        // 子集外的写法：退回源码卡，并**说明为什么**没排版。
        // 只显示源码、不说原因，用户会以为是自己公式写错了。
        let reason = if let Err(e) = layout.as_ref() {
            Some(e.to_string())
        } else {
            None
        };
        draw_unrendered_source(ui, theme, block, "公式", reason, wrap);
        return;
    };

    if !display {
        // 行内公式：跟随正文基线，不套框。
        let h = math::fitted_height(math_layout, ui.available_width());
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
        let cmds = math::emit(math_layout, render::rgb8_from_color32(theme.text_primary));
        render::apply_egui(
            ui.painter(),
            rect,
            math_layout.width,
            math_layout.height(),
            HAlign::Left,
            true,
            &cmds,
        );
        return;
    }

    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(
            m.pane_padding as i8,
            m.space_md as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("公式")
                        .size(m.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });
            let h = math::fitted_height(math_layout, ui.available_width());
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
            let cmds = math::emit(math_layout, render::rgb8_from_color32(theme.text_primary));
            render::apply_egui(
                ui.painter(),
                rect,
                math_layout.width,
                math_layout.height(),
                HAlign::Center,
                true,
                &cmds,
            );
        });
}

/// 图表（Mermaid）：**内置布局引擎**渲染（见 [`mermaid`]）。
///
/// 与公式同一套思路：解析层本来就给出了 `Diagram { kind: "mermaid" }` 与源码，
/// 旧实现却只把源码当代码块打出来、再附一句「导出后由浏览器渲染」。
/// 现在直接画成节点—连线图（分层布局 + 正交走线 + 箭头 + 边标签）。
///
/// 子集外的图种（`sequenceDiagram` 等）与 `subgraph` 回退到源码卡并说明原因。
///
/// `wrap` 跟随「自动换行」开关：回退源码卡内的长行在开启时折行（问题 2）。
pub(super) fn draw_diagram(ui: &mut Ui, theme: &Theme, block: &BlockView, wrap: bool) {
    let m = theme.metrics;
    let engine = block.diagram.as_deref().unwrap_or("diagram");
    // 同 `draw_math`：图表几何按 (内容, 字号, 图集令牌) 帧间记忆化（F-4）。
    let layout = {
        let ui_ref: &Ui = ui;
        let measure = move |text: &str, size: f32| {
            ui_ref
                .painter()
                .layout_no_wrap(
                    text.to_owned(),
                    FontId::proportional(size),
                    egui::Color32::WHITE,
                )
                .size()
                .x
        };
        embedded_cache::mermaid_layout(
            &block.content,
            m.preview_font_size,
            atlas_token(ui.ctx()),
            &measure,
        )
    };

    let Ok(graph) = layout.as_ref() else {
        let reason = if let Err(e) = layout.as_ref() {
            Some(e.to_string())
        } else {
            None
        };
        draw_unrendered_source(ui, theme, block, engine, reason, wrap);
        return;
    };

    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(m.pane_padding as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(engine)
                        .size(m.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });
            let max_w = ui.available_width();
            let h = mermaid::fitted_height(graph, max_w);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(max_w, h), egui::Sense::hover());
            let style = mermaid::MermaidStyle {
                node_fill: theme.bg_tertiary,
                node_stroke: theme.border,
                text: theme.text_primary,
                edge: theme.text_secondary,
                label_bg: theme.code_bg,
            };
            mermaid::paint(ui.painter(), rect, graph, &style);

            // 图表内超链接可点击：把 mermaid 返回的命中区（左上角原点、
            // 已归一化，与图元同一坐标系）按 `paint` 完全相同的缩放 + 平移
            // 变换到屏幕坐标，再注册成可点击交互区。覆盖三类来源：
            // ① `click A href "url"` 指令（按节点 id 命中整块节点）；
            // ② 节点 / 参与者 / 类名 / 任务名里写的 `[文本](url)`；
            // ③ 时序图消息文案里写的 `[文本](url)`。
            // 三种图种（graph / sequence / class / gantt）都经 `finalize`
            // 产出 `hotspots`，这里无需区分图种。
            //
            // 命中区随 `paint` 的 `scale` / `origin` 同步计算，故缩放与
            // 横向居中两种情况下点击位置都精确对齐绘制位置。
            let scale = if graph.width > max_w && graph.width > 0.0 {
                max_w / graph.width
            } else {
                1.0
            };
            let origin = egui::Pos2::new(
                rect.left() + (rect.width() - graph.width * scale).max(0.0) * 0.5,
                rect.top(),
            );
            for (i, (hot, url)) in graph.click_targets().iter().enumerate() {
                // 屏幕坐标下的命中矩形（与绘制同一变换）。
                let screen = egui::Rect::from_min_max(
                    origin + hot.min.to_vec2() * scale,
                    origin + hot.max.to_vec2() * scale,
                );
                let resp = ui.interact(
                    screen,
                    ui.scope_id().with(("mhot", i)),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    // 指到可点击区域时换成手型，提示「这里能点」。
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    // 新标签页打开：外部链接不打断当前文档阅读。
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
                }
            }
        });
}

/// 「这块没被渲染」的降级卡：源码 + 类型标注 + **失败原因**。
///
/// # 为什么必须带上原因
///
/// 只显示源码时用户的下一步动作是「去查 Mermaid / LaTeX 语法」，
/// 而那多半是浪费时间——**不支持的是本编辑器，不是他的写法**。
/// 把原因写出来（`不支持的图表写法：sequenceDiagram`）就能立刻说清责任归属。
///
/// `wrap` 跟随「自动换行」开关（问题 2）：源码卡里的长行同样会在开启时
/// 折行，不再撑出预览区横向滚动条。
fn draw_unrendered_source(
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    label: &str,
    reason: Option<String>,
    wrap: bool,
) {
    let m = theme.metrics;
    egui::Frame::new()
        .fill(theme.code_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::same(m.pane_padding as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(label)
                        .size(m.ui_font_size - 1.0)
                        .color(theme.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("复制").clicked() {
                        ui.ctx().copy_text(block.content.clone());
                    }
                });
            });
            let wrap_width = if wrap {
                ui.available_width().max(0.0)
            } else {
                f32::INFINITY
            };
            draw_highlighted_code(ui, theme, &block.content, None, wrap_width);
            if let Some(reason) = reason {
                ui.add_space(m.space_xs);
                ui.label(
                    RichText::new(format!("未能排版：{reason}"))
                        .size(m.ui_font_size - 1.0)
                        .color(theme.warning),
                );
            }
        });
}
