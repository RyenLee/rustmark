//! 编辑区视图（S6-03 实现）。
//!
//! # 设计与权衡
//!
//! 编辑面直接复用 egui 的 `TextEdit` 作为编辑表面——它原生支持 IME
//! （候选词窗口跟随光标、组合期不污染已提交文本），这正是 FR-05 要求的能力，
//! 也是 S6-03 风险预案里「输入法隔离层」最务实的兑现：**不自己造输入法**。
//!
//! 代价是 egui 的 `TextEdit` 自带一套撤销栈与 Tab 语义，会与本项目「领域是内容 /
//! 撤销唯一真相」冲突。处理方式（见 [`crate::app::frame::EditorApp`]）：
//!
//! - **撤销/重做**走领域：每次领域改了文本后清空 egui 内置撤销栈（`clear_undoer_next`），
//!   且 Ctrl+Z 由快捷键分发器路由到领域，绝不走 egui 原生撤销。
//! - **Tab / Shift+Tab** 用 `code_editor()` 的 `lock_focus(true)` 保证**焦点不丢失**；
//!   多行选区缩进/反缩进另由 `Indent` / `Outdent` 动作（egui 单行 `lock_focus` 语义之外）
//!   经命令面板可达。
//! - **内容单一真相**：egui 改过的字符串经 [`editor_ops::diff_replace`] 算出最小替换，
//!   发 [`crate::application::document::commands::ReplaceRangeCommand`] 回写领域，
//!   随后 `refresh_text()` 用 `get_content` 覆盖回来，二者永不漂移。
//!
//! IME 的「组合期不产生撤销快照」由 egui 天然满足：组合中的预编辑文本不进入
//! `&mut String`，`text_changed` 只在提交时触发，故每个汉字只产生**一次**领域命令。

use egui::{Align, FontId, Layout, ScrollArea, TextEdit, Ui};

use crate::app::frame::EditorApp;
use crate::application::markdown::queries::{HighlightQuery, SpellCheckQuery};
use crate::presentation::state::scroll_sync_state::Side;
use crate::presentation::theme::mapper;
use crate::presentation::views::editor_ops;
use crate::shared::geometry::{ByteOffset, TextRange};

/// 绘制编辑区。
pub fn show(app: &mut EditorApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    // 1) 把上一帧领域编辑留下的「清撤销栈 / 重设光标」落到 egui 内部状态
    //    （必须在 `TextEdit::show` 之前，否则本帧光标不会更新）。
    app.apply_pending_editor_state(&ctx);
    // 2) 确保有活动文档（每帧兜底：编辑区永远要有一页可编辑）。
    app.ensure_document();

    let theme = app.theme().clone();

    ui.painter()
        .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, theme.bg_primary);

    // 编辑区顶部工具栏：按工具作用分组的下拉菜单（问题 3）。贴在编辑区顶部、
    // 文本上方（上下布局），作用域与编辑区内容一致；预览区也可独立占满剩余宽度。
    let tb_h = theme.metrics.editor_toolbar_height;
    ui.allocate_ui(egui::vec2(ui.available_width(), tb_h), |ui| {
        crate::presentation::views::toolbar_view::draw_editor_toolbar(app, ui);
    });
    ui.separator();

    let line_count = app.editor_text.split('\n').count().max(1);
    let font_size = theme.editor_font_size;
    let gutter_w = 44.0;

    // 当前光标所在行（用于高亮当前行号）。
    let cursor_line = current_line_index(&app.editor_text, app.editor.cursor);

    // 自动换行开关（设置 / 状态栏可切换）。egui 0.33 的 `TextEdit` **没有**
    // `.wrap(bool)` 方法，换行与否完全由 `desired_width` + 自定义 layouter 决定。
    // egui 算出的 `wrap_width = desired_width.min(available_width)`，即使把
    // `desired_width` 设为 `INFINITY`，只要内部可用宽有限就仍是有限值、依旧换行
    // （见下方 `layouter` 内对 `effective_wrap` 的覆盖）。故在此先取一份副本，
    // 闭包里好按开关强制「不换行 = INFINITY」。
    let wrap = app.editor.word_wrap;

    // 预计算高亮 span（owned `Vec<SpanView>`），避免 `layouter` 闭包去借用
    // `app.services`——那会与下方 `&mut app.editor_text` 的可变借用冲突。
    // `theme` 是 `Copy`，闭包里会按值拷贝它；`doc_id` 也是 `Copy`。
    // 闭包因此**不借用 `app`**，把 `app` 的唯一可变借用让给 `TextEdit`。
    let doc_id = app.active_doc;
    let spans = doc_id.and_then(|id| {
        app.services
            .markdown
            .highlight(HighlightQuery {
                doc_id: id,
                range: TextRange::new(ByteOffset::new(0), ByteOffset::new(app.editor_text.len())),
                lang: Some("markdown".to_string()),
            })
            .ok()
    });
    // 拼写检查：开启时取错词区间（字节坐标，与 `editor_text` 同源）。
    // 关闭时返回空Vec，服务端也不查缓存不重算，故关闭的代价为零。
    let spell_marks: Vec<mapper::SpellMark> = {
        let enabled = app.editor.spell_check;
        doc_id
            .map(|id| {
                app.services
                    .markdown
                    .spell_check(SpellCheckQuery {
                        doc_id: id,
                        enabled,
                    })
                    .into_iter()
                    .map(|(start, end)| mapper::SpellMark { start, end })
                    .collect()
            })
            .unwrap_or_default()
    };

    // 波浪线用 `error` 色（与状态栏错误、导出失败提示同色），
    // 保持「红色 = 有问题」的全应用语义一致。
    let spell_color = theme.error;

    // `theme` 不是 `Copy`，这里克隆一份给闭包，原值留给下方行号栏 / 文本色等内联使用。
    let theme_for_layout = theme.clone();
    // 编辑区查找命中高亮：把当前文档的查找命中区间（文档字节坐标）交给排版层，
    // 由 `highlight_layout_with_find` 叠加背景。无命中（未搜索 / 无结果）时留空。
    let find_marks: Vec<mapper::FindMark> = app
        .find
        .finder
        .as_ref()
        .map(|finder| {
            let current = app.find.current;
            finder
                .matches()
                .iter()
                .enumerate()
                .map(|(i, m)| mapper::FindMark {
                    start: m.start,
                    end: m.end,
                    current: Some(i) == current,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut layouter = move |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_arg: f32| {
        let txt = buf.as_str();
        // egui 把 `wrap_width = desired_width.min(available_width)` 传进来；
        // 但「关闭换行」时 `available_width` 仍是有限的面板宽，`min(INFINITY, 有限)`
        // 还是有限值、照样折行。因此这里**显式按开关覆盖**：关掉换行就强制传
        // `INFINITY` 给排版器，彻底不折行（长行靠 `both()` 滚动区横向滚动）。
        let effective_wrap = if wrap { wrap_arg } else { f32::INFINITY };
        let find_ref = if find_marks.is_empty() {
            None
        } else {
            Some(find_marks.as_slice())
        };
        let galley = mapper::highlight_layout_with_find(
            ui.ctx(),
            &theme_for_layout,
            txt,
            spans.as_deref(),
            effective_wrap,
            find_ref,
        );
        // 波浪线必须**叠在文本之上**绘制，且要用本帧真实排版结果换算
        // 字节→像素，故在 layouter 内部（此处`galley` 刚生成）就地画出。
        // 此时 painter 的坐标系已平移到文本原点，`Vec2::ZERO` 即文本左上角。
        if !spell_marks.is_empty() {
            let squiggles = mapper::squiggles_for(&galley, txt, &spell_marks, egui::Vec2::ZERO);
            mapper::paint_squiggles(ui.painter(), &squiggles, spell_color);
        }
        galley
    };

    // 记录旧文本用于 diff（TextEdit 会就地改 `editor_text`）。
    let prev = app.editor_text.clone();

    // 滚动偏移：**读回**比写入更重要。
    // `ScrollArea` 的用户滚轮增量是在 `show` 内部叠加的，
    // 只有 `show` 返回的 `state.offset` 才是本帧的真实值——
    // 用它回写 UI State，同步逻辑才不会读到「上一帧的意图」。
    // 传入上一次的值不影响滚轮：`begin` 只是把它作为初值，
    // 增量在 `end` 里照样生效。
    let start_y = if app.editor.scroll_y.is_finite() {
        app.editor.scroll_y
    } else {
        0.0
    };
    // 换行时只能要纵向滚动（`vertical()` 会给子布局**有限**宽，换行才能正确
    // 随窗口重排）；不换行时才允许横向滚动（`both()`），否则长行会被裁掉看不见。
    // 若两条路径都用 `both()`，子布局会变成无限宽，反而把换行也破坏掉。
    let scroll_area = if wrap {
        ScrollArea::vertical()
    } else {
        ScrollArea::both()
    };
    let out = scroll_area
        .id_salt("editor-scroll")
        .vertical_scroll_offset(start_y)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                draw_gutter(ui, &theme, line_count, cursor_line, font_size, gutter_w);

                let editor = TextEdit::multiline(&mut app.editor_text)
                    .id(EditorApp::editor_id())
                    .font(FontId::monospace(font_size))
                    // 注意：一旦提供自定义 `layouter`，egui 就会**完全忽略**
                    // `.text_color()`——正文颜色只来自 layouter 产出的 Galley
                    // （见 `egui::widgets::text_edit::builder` 中 `default_layouter`
                    // 只在 `layouter.is_none()` 时生效）。
                    // 这里保留它只为「未来若无高亮 layouter 时的兜底」，真正决定
                    // 编辑区可读性的是 `mapper::StyleMapper::format("plain")`，
                    // 它取 `theme.text_primary`。
                    .text_color(theme.text_primary)
                    .desired_rows(line_count)
                    // 换行：用当前可用宽（有限）→ 在编辑器边缘换行、随窗口重排；
                    // 不换行：用 `INFINITY` → 不折行，靠 `both()` 的横向滚动查看。
                    .desired_width(if wrap {
                        ui.available_width()
                    } else {
                        f32::INFINITY
                    })
                    .code_editor() // 等宽字体 + lock_focus(true)：Tab 不丢焦点
                    .layouter(&mut layouter)
                    .show(ui);

                // 3) 光标 / 选区同步（egui 给的是字符索引，存成字节偏移）。
                if let Some(range) = editor.cursor_range {
                    let c = editor_ops::char_to_byte(&app.editor_text, range.primary.index);
                    let a = editor_ops::char_to_byte(&app.editor_text, range.secondary.index);
                    app.editor.cursor = c;
                    app.editor.anchor = Some(a);
                }

                // 4) 文本变更 → diff → 领域命令。
                //    egui 0.33 的 `TextEditOutput` 已无 `text_changed` 字段，
                //    改用「前后文本是否不同」判断——`prev` 在 show 前就已克隆。
                if prev != app.editor_text
                    && let Some(mut edit) = editor_ops::diff_replace(&prev, &app.editor_text)
                {
                    editor_ops::transform_insert(&prev, &mut edit);
                    app.flush_edit(edit);
                }
            });
        });

    app.editor.scroll_y = out.state.offset.y;
    app.ui
        .sync
        .set_geometry(Side::Editor, out.content_size.y, out.inner_rect.height());

    // 高亮增量：把重排 / 复用行数与行布局规模显示出来（S6-04 要求「缓存命中率
    // 记录到调试面板」）。统一调试面板由 S6-11 收敛，届时这段 readout 迁过去。
    let hl = mapper::last_highlight_stats();
    ui.weak(format!(
        "highlight · 共 {} 行 · 重排 {} · 复用 {} · 行布局 {}",
        hl.total_lines,
        hl.reshaped_lines,
        hl.cached_lines,
        mapper::line_cache_len()
    ));
    ui.weak(format!(
        "scroll-sync · 主动方 {} · 驱动 {} 次 · 块矩形 {}",
        app.ui
            .sync
            .last_driven
            .map_or("—", |s| if s == Side::Editor {
                "编辑区"
            } else {
                "预览区"
            }),
        app.ui.sync.drive_count,
        app.preview.rects.len()
    ));
}

/// 绘制左侧行号栏（当前行号用强调色）。
fn draw_gutter(
    ui: &mut Ui,
    theme: &crate::presentation::theme::builtin::Theme,
    line_count: usize,
    cursor_line: usize,
    font_size: f32,
    gutter_w: f32,
) {
    ui.allocate_ui(egui::vec2(gutter_w, ui.available_height()), |ui| {
        ui.with_layout(Layout::top_down(Align::RIGHT), |ui| {
            for i in 0..line_count {
                ui.label(egui::RichText::new(format!("{i}")).size(font_size).color(
                    if i == cursor_line {
                        theme.accent
                    } else {
                        theme.text_secondary
                    },
                ));
            }
        });
    });
}

/// 给定字节偏移，返回其所在行号（0-based）。
fn current_line_index(text: &str, byte: usize) -> usize {
    let clamped = byte.min(text.len());
    text[..clamped].chars().filter(|&c| c == '\n').count()
}
