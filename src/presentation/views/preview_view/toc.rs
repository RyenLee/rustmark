//! 分割线 / 目录 / 脚注 / 定义列表 / 原始块等杂项块渲染器。

use egui::FontId;

use super::inline::{Typography, render_inline_text};
use super::*;

/// 分割线。
pub(super) fn draw_rule(ui: &mut Ui, theme: &Theme) {
    ui.add_space(theme.metrics.space_sm);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.top(), hairline(theme.border));
    ui.add_space(theme.metrics.space_sm);
}

/// 脚注定义：小字 + 次级色。
/// 目录（`@[TOC](标题)` / `[TOC]`）：标题列表 + 点击跳转。
///
/// # 条目为什么现场收集
///
/// `headings` 由 `show()` 从**本帧的块序列**收集，而不是解析期写死在目录块里：
/// 标题会随编辑增删、改名，任何副本都会在下一帧过期；而目录**本来就该**
/// 与当前文档结构一致（这正是它存在的意义）。
pub(super) fn draw_toc(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    headings: &[HeadingEntry],
    _internal_targets: &[(String, u64)],
) {
    let m = theme.metrics;
    let title = if block.content.trim().is_empty() {
        "目录"
    } else {
        block.content.trim()
    };
    egui::Frame::new()
        .fill(theme.bg_tertiary)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(
            m.pane_padding as i8,
            m.space_md as i8,
        ))
        .show(ui, |ui| {
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(m.preview_font_size)
                    .color(theme.text_primary),
            );
            ui.add_space(m.space_sm);
            if headings.is_empty() {
                ui.weak("（文档里还没有标题）");
                return;
            }
            for h in headings {
                // 缩进按层级递进（一级不缩进）：层级感靠左缘位置表达。
                let indent = (h.level.saturating_sub(1)) as f32 * m.space_md;
                let resp = ui
                    .horizontal(|ui| {
                        ui.add_space(indent);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&h.text)
                                    .size(m.preview_font_size - 1.0)
                                    .color(if h.level <= 2 {
                                        theme.text_primary
                                    } else {
                                        theme.text_secondary
                                    }),
                            )
                            .sense(egui::Sense::click()),
                        )
                    })
                    .inner;
                if resp.clicked() {
                    app.ui.sync.request_click(h.id);
                }
                if resp.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }
            }
        });
    ui.add_space(m.space_xs);
}

/// 预览末尾的「脚注」区：把全文的脚注定义**收集到一起**渲染。
///
/// # 为什么不按源码位置内联渲染（V11-18）
///
/// 主流渲染器（Typora / GitHub / mermaid.js 文档站）都把脚注定义收在文档
/// 末尾，正文里只留上标编号。早期实现按源码位置内联画 `[1] 内容`，用户滚到
/// 文档底部找不到脚注——反馈「没有实现注脚」：截图里预览底部一片空白，
/// 而编辑区明明写着定义。内联还会把定义塞进正文流中间，阅读被硬生截断。
///
/// # 标签为什么可点击
///
/// 与标题 / 目录条目同一条通路：`sync.request_click(block_id)` 把**编辑区**
/// 滚到该脚注定义的源码行。正文里的上标编号暂不做点击（行内片段的
/// 逐Span 命中需要 galley 字形级命中测试，成本与收益不成比例）。
pub(super) fn draw_footnote_section(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    footnotes: &[&BlockView],
    wrap: bool,
    internal_targets: &[(String, u64)],
) {
    let m = theme.metrics;
    let size = m.preview_font_size - 2.0;
    // 与正文的分隔：上方留白 + 一条细线。比 `rule` 轻——脚注是附属区，不是新章节。
    ui.add_space(m.space_md);
    let (line, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(line.x_range(), line.top(), hairline(theme.border));
    ui.add_space(m.space_sm);
    ui.label(
        RichText::new("脚注")
            .strong()
            .size(size + 1.0)
            .color(theme.text_secondary),
    );
    ui.add_space(m.space_xs);
    for (i, block) in footnotes.iter().enumerate() {
        // 标签缺失（理论不上会发生）时退化为序号：没有编号的条目对不上正文里的引用。
        let label = block
            .label
            .as_deref()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| (i + 1).to_string());
        ui.horizontal_top(|ui| {
            let resp = ui.add(
                egui::Label::new(
                    RichText::new(format!("[{label}]"))
                        .size(size)
                        .color(theme.syn_link),
                )
                .sense(egui::Sense::click()),
            );
            if resp.clicked() {
                app.ui.sync.request_click(block.id);
            }
            if resp.hovered() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
            }
            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                ui.set_min_width(ui.available_width().max(0.0));
                let max_w = if wrap {
                    ui.available_width()
                } else {
                    f32::INFINITY
                };
                render_inline_text(
                    ui,
                    app,
                    theme,
                    &block.content,
                    &block.spans,
                    Typography {
                        size,
                        line_height: size * m.preview_line_height,
                        base_style: "quote",
                    },
                    egui::Sense::hover(),
                    max_w,
                    internal_targets,
                );
            });
        });
        ui.add_space(m.space_xs);
    }
}

/// 定义列表（`术语` + `: 定义`）：术语加粗、定义缩进在术语下方。
///
/// 不套项目符号：定义列表的语义是「词条—释义」，画成 `• 术语: 定义`
/// 就与普通无序列表混同了，而且术语与释义的从属关系在横向上会被拉平。
pub(super) fn draw_definition_list(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    wrap: bool,
    internal_targets: &[(String, u64)],
) {
    let m = theme.metrics;
    let size = m.preview_font_size;
    let line_height = size * m.preview_line_height;
    for entry in &block.definitions {
        // 术语：用与标题同族的强调色，与下面的释义拉开层次。
        let term_max_w = if wrap {
            ui.available_width()
        } else {
            f32::INFINITY
        };
        render_inline_text(
            ui,
            app,
            theme,
            &entry.term,
            &entry.term_spans,
            Typography {
                size,
                line_height,
                base_style: "strong",
            },
            egui::Sense::hover(),
            term_max_w,
            internal_targets,
        );
        for body in &entry.definitions {
            ui.horizontal_top(|ui| {
                // 释义缩进一格（与列表的缩进刻度同源），并画一条短竖线
                // 作为「属于上面那个术语」的视觉连缀。
                let indent = m.space_lg;
                ui.add_space(indent);
                let (bar, _) = ui.allocate_exact_size(
                    egui::vec2(QUOTE_BAR_W, ui.available_height().max(line_height)),
                    egui::Sense::hover(),
                );
                ui.painter().vline(
                    bar.center().x,
                    bar.y_range(),
                    Stroke::new(1.0_f32, theme.border),
                );
                ui.add_space(m.space_xs);
                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                    ui.set_min_width(ui.available_width().max(0.0));
                    let body_max_w = if wrap {
                        ui.available_width()
                    } else {
                        f32::INFINITY
                    };
                    render_inline_text(
                        ui,
                        app,
                        theme,
                        &body.text,
                        &body.spans,
                        Typography {
                            size,
                            line_height,
                            base_style: "",
                        },
                        egui::Sense::hover(),
                        body_max_w,
                        internal_targets,
                    );
                });
            });
        }
        ui.add_space(m.space_xs);
    }
}

/// 未支持的扩展块（原始 HTML）：等宽原样显示并标注类型。
///
/// 保留原文而不是隐藏——用户至少能看到自己写了什么，也知道这块没被渲染。
///
/// 折行跟随「自动换行」开关（问题 068）：egui 的 `Label` 默认按可用宽折行，
/// 与正文其它块「关闭 = 不折行、靠横向滚动条」的行为不一致，故显式指定：
/// 开 → `Wrap`（在钳制后的视口宽内折行），关 → `Extend`（单行排开，
/// 撑出预览区的横向滚动条）。
pub(super) fn draw_raw(
    _app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    block: &BlockView,
    kind: &str,
    wrap: bool,
    _internal_targets: &[(String, u64)],
) {
    ui.add(
        egui::Label::new(
            RichText::new(&block.content)
                .font(FontId::monospace(theme.metrics.preview_font_size - 1.0))
                .color(theme.text_secondary),
        )
        .wrap_mode(if wrap {
            egui::TextWrapMode::Wrap
        } else {
            egui::TextWrapMode::Extend
        }),
    );
    if let Some(marker) = kind.strip_prefix("raw:") {
        ui.weak(format!("未渲染的 {marker} 块"));
    }
}
