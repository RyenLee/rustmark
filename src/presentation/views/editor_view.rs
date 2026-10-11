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

use egui::{FontId, ScrollArea, TextEdit, Ui};

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
    //
    // 垂直居中由 `draw_editor_toolbar` 内部完成（行高给足后按钮在其中居中），
    // 这里不再用 `add_space` 手工垫高——那个写法依赖「按钮高 = interact_size.y」
    // 的假设，按钮实际更高时会与下拉阶梯化（首个菜单项偏高）叠加。
    let tb_h = theme.metrics.editor_toolbar_height;
    ui.allocate_ui(egui::vec2(ui.available_width(), tb_h), |ui| {
        crate::presentation::views::toolbar_view::draw_editor_toolbar(app, ui);
    });
    ui.separator();

    let line_count = app.editor_text.split('\n').count().max(1);
    let font_size = theme.editor_font_size;
    // 行号栏：宽度按**最大行号的位数实测**（等宽字体），而不是写死 44pt——
    // 写死会在万行文档里挤掉最后一位、在十行文档里留一片空白。
    // 左右各留一个 `space_sm` 内边距，行号与正文之间另有一个 `space_sm` 间隔，
    // 这是「行号不与文字重叠」的横向保障。
    let gutter_pad = theme.metrics.space_sm;
    let show_gutter = app.editor.line_numbers;
    let gutter_w = if show_gutter {
        let digits = line_count.to_string().len().max(2);
        let digit_w = ui
            .painter()
            .layout_no_wrap(
                "0".repeat(digits),
                FontId::monospace(font_size),
                theme.text_secondary,
            )
            .size()
            .x;
        digit_w + 2.0 * gutter_pad
    } else {
        0.0
    };
    // 编辑区内容左缘内边距（与预览区正文同一条左边线）。
    let content_pad = theme.metrics.space_sm;

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
                // 内容左缘内边距：正文不贴着面板边线（与预览区同级留白）。
                ui.add_space(content_pad);
                // 行号栏：先只占住左侧宽度。**数字本帧不在此处画**——
                // 放到 TextEdit 排版完成之后（见第 3 步），才能用「本帧真实的
                // 行顶」来定位，彻底消除行号与正文的错位（问题 3）。
                // 关闭时不做任何占位：零宽占位也会插入 item_spacing，
                // 让正文凭空左移一个间距。
                let gutter_rect = show_gutter.then(|| {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(gutter_w, ui.available_height()),
                        egui::Sense::hover(),
                    );
                    rect
                });

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

                // 3) 行号：用**本帧真实排版的行顶**（含折行）+ 文本原点描画，
                //    使每个行号精确落在其对应文本行上（问题 3）。
                //    开关关闭（默认）时跳过——见 `show_gutter` 处的注释。
                if let Some(gutter_rect) = gutter_rect {
                    paint_gutter(
                        ui,
                        &theme,
                        gutter_rect,
                        editor.galley_pos,
                        &app.editor_text,
                        cursor_line,
                        font_size,
                    );
                }

                // 4) 光标 / 选区同步（egui 给的是字符索引，存成字节偏移）。
                if let Some(range) = editor.cursor_range {
                    let c = editor_ops::char_to_byte(&app.editor_text, range.primary.index);
                    let a = editor_ops::char_to_byte(&app.editor_text, range.secondary.index);
                    app.editor.cursor = c;
                    app.editor.anchor = Some(a);
                }

                // 5) 文本变更 → diff → 领域命令。
                //    egui 0.33 的 `TextEditOutput` 已无 `text_changed` 字段，
                //    改用「前后文本是否不同」判断——`prev` 在 show 前就已克隆。
                if prev != app.editor_text
                    && let Some(mut edit) = editor_ops::diff_replace(&prev, &app.editor_text)
                {
                    editor_ops::transform_insert(&prev, &mut edit);
                    // 关键：把「旧文本 + 编辑」拼成与领域完全一致的最终串，
                    // 直接写回 `editor_text`，然后再发领域命令。这样 `flush_edit`
                    // 就**不必再从领域整篇回读**——那次回读会在每一帧把绑定给
                    // egui 的字符串整体替换一遍，迫使 egui 的 `TextEdit` 每敲一个
                    // 键就重新推导光标 / 选区 / 撤销栈，表现为编辑区闪烁；列表
                    // 续行时还会让标记「晚一帧才弹出来」。此处就地拼出最终串，
                    // egui 与领域内容瞬时一致，下游 `flush_edit` 跳过回读。
                    //
                    // `edit.byte_start/end` 是相对 `prev`（旧文本）的字节边界，
                    // 经 `transform_insert` 后仍是 `prev` 的合法边界（空项退出分支
                    // 把起点拉到了行首），故切片安全。
                    let bs = edit.byte_start.min(prev.len());
                    let be = edit.byte_end.min(prev.len());
                    app.editor_text = format!("{}{}{}", &prev[..bs], edit.inserted, &prev[be..]);
                    // 第二个参数必须是 `prev.len()`（领域旧文本长度），**不是**
                    // `app.editor_text.len()`——上一行刚把 `editor_text` 拼成
                    // 变换后的新串，「退出列表」分支会把文本改短，新长度装不下
                    // 相对旧文本的 `byte_end`；flush_edit 若按新长度钳制就会把
                    // 区间砍短，领域层只删掉半个标记（实测：`4. ` 残留成 `. `）。
                    app.flush_edit(edit, prev.len());
                    // 文本一变，egui 就会「把光标滚进视口」（带动画、连续数帧）。
                    // 那是程序为了光标可见而滚的，用户并没有滚，故必须声明成
                    // 程序性位移——否则滚动同步会把它当成用户手势，一帧一次地把
                    // 预览区拉到映射位置（编辑区平滑滚、预览区逐帧跳），
                    // 两栏同时在动即用户看到的「界面闪烁」。
                    let now_ms = app.services.markdown.now().millis();
                    app.ui.sync.note_programmatic_scroll(Side::Editor, now_ms);
                }
            });
        });

    app.editor.scroll_y = out.state.offset.y;
    app.ui
        .sync
        .set_geometry(Side::Editor, out.content_size.y, out.inner_rect.height());
    // 说明：S6-04 时代曾把高亮 / 滚动同步的统计 readout 直接画在编辑区底部，
    // 那段「零高余量里的两行字」会吃掉编辑区的可视高度，使编辑区与预览区的
    // 滚动视口不再等高（两栏对齐的回归根源）。统计数据仍可经
    // `mapper::last_highlight_stats()` 取用（单测在用），只是不再占用版面。
}

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
fn paint_gutter(
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
    let line_count = text.split('\n').count().max(1);
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
fn current_line_index(text: &str, byte: usize) -> usize {
    let clamped = byte.min(text.len());
    text[..clamped].chars().filter(|&c| c == '\n').count()
}
