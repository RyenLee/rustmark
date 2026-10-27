//! 编辑区视图（S6-03 实现）。
//!
//! # 设计与权衡
//!
//! 编辑面直接复用 egui 的 `TextEdit` 作为编辑表面——它原生支持 IME
//! （候选词窗口跟随光标、组合期不污染已提交文本），这正是 FR-05 要求的能力，
//! 也是 S6-03 风险预案里「输入法隔离层」最务实的兑现：**不自己造输入法**。
//!
//! 代价是 egui 的 `TextEdit` 自带一套撤销栈与 Tab 语义，会与本项目「领域是内容 /
//! 撤销唯一真相」冲突。处理方式（由组合根的 `EditorApp` 承担，见
//! `apply_pending_editor_state` 与 `flush_edit` 两条契约）：
//!
//! - **撤销/重做**走领域：每次领域改了文本后清空 egui 内置撤销栈（`clear_undoer_next`），
//!   且 Ctrl+Z 由快捷键分发器路由到领域，绝不走 egui 原生撤销。
//! - **Tab / Shift+Tab** 用 `code_editor()` 的 `lock_focus(true)` 保证**焦点不丢失**；
//!   多行选区缩进/反缩进另由 `Indent` / `Outdent` 动作（egui 单行 `lock_focus` 语义之外）
//!   经命令面板可达。
//! - **内容单一真相**：egui 改过的字符串经 [`editor_ops::diff_replace`] 算出最小替换，
//!   发 [`rustmark_core::application::document::commands::ReplaceRangeCommand`] 回写领域，
//!   随后 `refresh_text()` 用 `get_content` 覆盖回来，二者永不漂移。
//!
//! IME 的「组合期不产生撤销快照」由 egui 天然满足：组合中的预编辑文本不进入
//! `&mut String`，`text_changed` 只在提交时触发，故每个汉字只产生**一次**领域命令。
//!
//! # 编辑区几何：框必须撑满面板（问题 047）
//!
//! `TextEdit` 自带的 frame 就是用户看到的那个圆角框，尺寸等于控件尺寸。
//! 若任由 egui 默认（`ScrollArea::auto_shrink` 为 `TRUE`、`TextEdit` 高度只由
//! `desired_rows` 决定），框就只包住文本——短文档下框底到状态栏之间是一大片
//! 裸底色，横滚动条也贴在框底而非面板底。故这里显式
//! `auto_shrink([false, false])` + `TextEdit::min_size` 给满视口高，
//! 让框始终等于编辑区的可视范围。两个易错点：高度必须取 `ui.clip_rect()`
//! （视口，不随滚动变化）而不是 `ui.available_height()`（其顶端是
//! `inner_rect.min - offset`，滚动后越撑越长）；宽度一个点都不能给
//! （`min_size.x` 会顶掉 `available_width`，把正文撑出视口、凭空生出横滚动条）。
//!
//! # 模块结构
//!
//! facade 保留编辑区入口 `show`（324 行函数体整体不拆：函数体内部提取 helper
//! 属高风险动作，明确禁止）；拼写检查右键菜单在 `spell`，行号栏在 `gutter`。

mod gutter;
mod spell;

pub use spell::spell_menu_for;

use egui::{FontId, ScrollArea, TextEdit, Ui};

use crate::presentation::state::editor_ui_state::editor_id;
use crate::presentation::state::scroll_sync_state::Side;
use crate::presentation::theme::mapper;
use crate::presentation::view_context::EditorViewContext;
use crate::presentation::views::editor_ops;
use rustmark_core::application::markdown::queries::{HighlightQuery, SpellCheckQuery};
use rustmark_core::shared::geometry::{ByteOffset, TextRange};

/// 绘制编辑区。
pub fn show(vctx: &mut dyn EditorViewContext, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    // 1) 把上一帧领域编辑留下的「清撤销栈 / 重设光标」落到 egui 内部状态
    //    （必须在 `TextEdit::show` 之前，否则本帧光标不会更新）。
    vctx.apply_pending_editor_state(&ctx);
    // 2) 确保有活动文档（每帧兜底：编辑区永远要有一页可编辑）。
    vctx.ensure_document();

    let theme = vctx.theme_state().clone();

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
        crate::presentation::views::toolbar_view::draw_editor_toolbar(vctx, ui);
    });
    ui.separator();

    let line_count = vctx.current_line_count();
    let font_size = theme.editor_font_size;
    // 行号栏：宽度按**最大行号的位数实测**（等宽字体），而不是写死 44pt——
    // 写死会在万行文档里挤掉最后一位、在十行文档里留一片空白。
    // 左右各留一个 `space_sm` 内边距，行号与正文之间另有一个 `space_sm` 间隔，
    // 这是「行号不与文字重叠」的横向保障。
    let gutter_pad = theme.metrics.space_sm;
    let show_gutter = vctx.editor_state().line_numbers;
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
    //
    // **只在显示行号栏时才算**（问题 046）：它是一次 O(光标位置) 的前缀扫描，
    // 而行号栏默认关闭——关着的时候每帧白扫一遍纯属浪费。
    //
    // 闭包只捕获 `cursor`（`usize`，Copy）与 `vctx` 的只读用法，
    // 不捕获 `vctx` 的可变借用——闭包结束时借用即释放。
    let cursor_line = {
        let cursor = vctx.editor_state().cursor;
        show_gutter.then(|| gutter::current_line_index(vctx.editor_text(), cursor))
    };

    // 自动换行开关（设置 / 状态栏可切换）。egui 0.33 的 `TextEdit` **没有**
    // `.wrap(bool)` 方法，换行与否完全由 `desired_width` + 自定义 layouter 决定。
    // egui 算出的 `wrap_width = desired_width.min(available_width)`，即使把
    // `desired_width` 设为 `INFINITY`，只要内部可用宽有限就仍是有限值、依旧换行
    // （见下方 `layouter` 内对 `effective_wrap` 的覆盖）。故在此先取一份副本，
    // 闭包里好按开关强制「不换行 = INFINITY」。
    let wrap = vctx.editor_state().word_wrap;

    // 预计算高亮 span（owned `Vec<SpanView>`），避免 `layouter` 闭包去借用
    // 服务——那会与下方 `editor_text_mut()` 的独占借用冲突。
    // `theme` 是 `Copy`，闭包里会按值拷贝它；`doc_id` 也是 `Copy`。
    // 闭包因此**不借用上下文**，把唯一的可变借用让给 `TextEdit`。
    let doc_id = vctx.active_doc();
    let text_len = vctx.editor_text().len();
    let spans = match doc_id {
        Some(id) => vctx
            .markdown_service()
            .highlight(HighlightQuery {
                doc_id: id,
                range: TextRange::new(ByteOffset::new(0), ByteOffset::new(text_len)),
                lang: Some("markdown".to_string()),
            })
            .ok(),
        None => None,
    };
    // 拼写检查：开启时取错词区间（字节坐标，与 `editor_text` 同源）。
    // 关闭时返回空Vec，服务端也不查缓存不重算，故关闭的代价为零。
    let spell_marks: Vec<mapper::SpellMark> = {
        let enabled = vctx.editor_state().spell_check;
        match doc_id {
            Some(id) => vctx
                .markdown_service()
                .spell_check(SpellCheckQuery {
                    doc_id: id,
                    enabled,
                })
                .into_iter()
                .map(|(start, end)| mapper::SpellMark { start, end })
                .collect(),
            None => Vec::new(),
        }
    };

    // 波浪线用 `error` 色（与状态栏错误、导出失败提示同色），
    // 保持「红色 = 有问题」的全应用语义一致。
    let spell_color = theme.error;

    // `theme` 不是 `Copy`，这里克隆一份给闭包，原值留给下方行号栏 / 文本色等内联使用。
    let theme_for_layout = theme.clone();
    // 编辑区查找命中高亮：把当前文档的查找命中区间（文档字节坐标）交给排版层，
    // 由 `highlight_layout_with_find` 叠加背景。无命中（未搜索 / 无结果）时留空。
    let find_marks: Vec<mapper::FindMark> = {
        let find = vctx.find_state();
        match find.finder.as_ref() {
            Some(finder) => {
                let current = find.current;
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
            }
            None => Vec::new(),
        }
    };
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
            // 服务层返回共享快照（`Arc<Vec<SpanView>>`，问题 046），
            // 这里只借切片——排版层只读，不需要所有权。
            spans.as_ref().map(|v| v.as_slice()),
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
    //
    // # 为什么这次克隆**省不掉**（问题 053，读 egui 0.37.0 源码后的结论）
    //
    // 1MB 上它值 0.22ms／帧（`docs/metrics.md` §3.2.5 的 S1），看着像白给，
    // 但本项目**无法回避**：`TextEdit` 直接写调用方给的 `&mut dyn TextBuffer`
    // （`text_edit/builder.rs` 的 `insert_replacing_selection` / `insert_text_at`），
    // 编辑前的字节**就地在缓冲里被覆盖**。而「领域是内容单一真相」又要求把
    // 这次编辑换算成相对旧文本的区间命令（`diff_replace`）——旧文本必须留一份。
    //
    // 唯一的省法「无输入事件就不克隆」**不成立**：§14.2 的 #4 假设「没有事件
    // 文本就不会变」，但同一文件里有两条**与 `InputState::events` 无关**的写入路径——
    // `accesskit_set_text_request(ui, id)`（无障碍 `SetValue`，:640 起）与拖放文本
    // 插入（读 `raw.dropped_files`，:898 起），两者都在事件列表为空时改写缓冲。
    // 按事件判空会**静默丢掉**这两类编辑（辅助技术用户 + 拖放粘贴）。
    //
    // 补一条「旧文本退回领域内容」的兜底固然能救回正确性，但那条路径要么不触发
    // （优化收益为零）、要么触发时正是它要省掉的那次全文复制——两边都不划算。
    // 故此处**据实保留克隆**，并把结论记在报告 §14.3.2。
    // ⚠ 这里必须是 `to_string()` 而不是 `clone()`：`vctx.editor_text()` 给的是
    // `&str`，而 `&str` **自身**实现了 `Clone`（`impl Clone for &T`），
    // 方法解析会先命中它 → `clone()` 得到的是 `&str` 而不是 `String`，
    // 于是 `prev` 变成对上下文的**长期借用**，把整段闭包的可变借用全堵死。
    let prev = vctx.editor_text().to_string();

    // 滚动偏移：**读回**比写入更重要。
    // `ScrollArea` 的用户滚轮增量是在 `show` 内部叠加的，
    // 只有 `show` 返回的 `state.offset` 才是本帧的真实值——
    // 用它回写 UI State，同步逻辑才不会读到「上一帧的意图」。
    // 传入上一次的值不影响滚轮：`begin` 只是把它作为初值，
    // 增量在 `end` 里照样生效。
    let start_y = {
        let scroll_y = vctx.editor_state().scroll_y;
        if scroll_y.is_finite() { scroll_y } else { 0.0 }
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
        // 视口撑满面板（问题 047）：`auto_shrink` 默认取 `Vec2b::TRUE`，语义是
        // 「内容多高，滚动区就多高」。短文档下滚动区收缩到文本高度，于是
        // `TextEdit` 自带的 frame 只包住那几行——框底到状态栏之间留下大片
        // 裸底色（044 像素取证实测差约 220 逻辑像素），横滚动条也贴在
        // 「收缩后的框底」而非面板底。置 `[false, false]` 后 `inner_rect`
        // 保持满高：框撑满、滚动条归位，`ui.clip_rect()` 才等于真实视口高。
        //
        // ⚠ 撑满剩余高的滚动区会把**其后**在同层 Ui 绘制的元素推出面板底缘
        // （见 `MEMORY.md` 的硬规则）。本面板内滚动区之后只有 `ctx` 绘制的
        // 浮层菜单（`draw_spell_menu`），不参与同层布局，故安全；后续若要在
        // 编辑区底部加常驻提示，必须放在滚动区**之前**。
        .auto_shrink([false, false])
        .vertical_scroll_offset(start_y)
        .show(ui, |ui| {
            // 滚动内容原点：本帧内容坐标系的 y=0（`ScrollArea` 的内容从
            // 这里开始排）。TextEdit 的 Galley **并不**从这儿起画——它
            // 的 atom frame 有竖向内边距，差值在下方实测。
            let content_top = ui.min_rect().top();
            // 视口高（已扣掉滚动条占位）= 文本区 frame 要撑到的高度。
            //
            // 这里刻意**不用** `ui.available_height()`：`content_max_rect` 的顶端
            // 是 `inner_rect.min - offset`，一旦滚动，它反而随 `offset` 变大，
            // 把 frame 越撑越长。`clip_rect()` 是滚动不动的视口，用它才对。
            let viewport_h = ui.clip_rect().height();
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

                // 形态 B 的极端形态：`TextEdit` 要**独占**编辑区文本。
                //
                // egui 0.37 的 `TextEditOutput` **不带生命周期参数**（`galley: Arc<Galley>`），
                // 借用在 `.show(ui)` 返回后**立即结束**——故把独占借用关进
                // **显式块**，块内不得调上下文的其它方法（含 `&self` 的
                // `markdown_service()`）：需要的数据一律块外预取。
                let editor = {
                    let buf = vctx.editor_text_mut();
                    TextEdit::multiline(buf as &mut dyn egui::TextBuffer)
                        .id(editor_id())
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
                        // 框（`TextEdit` 自带 frame）撑到整个视口高（问题 047）。
                        //
                        // 只给高度、**不给**宽度：egui 算宽度是
                        // `allocate_width = desired_width.at_most(available_width).at_least(min_size.x)`，
                        // 即 `min_size.x` 会**顶掉** `available_width`——传了宽度，正文区
                        // 就会横向溢出视口，凭空多出一条横滚动条。
                        .min_size(egui::vec2(0.0, viewport_h))
                        .code_editor() // 等宽字体 + lock_focus(true)：Tab 不丢焦点
                        .layouter(&mut layouter)
                        .show(ui)
                };

                // 3) 行号：用**本帧真实排版的行顶**（含折行）+ 文本原点描画，
                //    使每个行号精确落在其对应文本行上（问题 3）。
                //    开关关闭（默认）时跳过——见 `show_gutter` 处的注释。
                if let Some(gutter_rect) = gutter_rect {
                    gutter::paint_gutter(
                        ui,
                        &theme,
                        gutter_rect,
                        editor.galley_pos,
                        vctx.editor_text(),
                        cursor_line.unwrap_or(0),
                        font_size,
                    );
                }

                // 4) 光标 / 选区同步（egui 给的是字符索引，存成字节偏移）。
                if let Some(range) = editor.cursor_range {
                    let c = editor_ops::char_to_byte(vctx.editor_text(), range.primary.index.0);
                    let a = editor_ops::char_to_byte(vctx.editor_text(), range.secondary.index.0);
                    vctx.set_editor_selection(c, Some(a));
                }

                // 5) 文本顶端偏移：Galley 坐标系 → 滚动内容坐标系。
                //
                //    `TextEdit` 的 atom frame 默认把 `spacing.button_padding`
                //    当内边距（egui `theme/default_style.rs`；本项目 `mapper`
                //    将其设为 `(space_md, space_xs)`），于是 Galley 顶端比
                //    内容原点低一截（实测 4pt）。`line_tops` 报的是 Galley
                //    坐标（第 0 行顶 = 0），而滚动换算要内容坐标——差一个
                //    常量就等于所有跳转 / 同步系统性偏一截（问题 066：
                // 探针实测标题应贴视口顶 +8pt、实际 +12pt）。
                //    这里每帧实测并交给同步层：既不被主题间距的具体数值
                //    绑死，也不把 UI 结构细节泄露进滚动数学。
                vctx.ui_state()
                    .sync
                    .set_editor_text_top(editor.galley_pos.y - content_top);

                // 6) 文本变更 → diff → 领域命令。
                //
                //    早退策略：egui 每帧都要调 `.show(ui)`（UI 必须渲染），
                //    但 diff 计算 + flush_edit 只在本帧**真的改了**时才值得做。
                //    `response.changed()` 是 egui 原生「本帧 TextEdit 内容是否变了」的信号——
                //    用户停止打字后立刻跳过整个 diff 路径，大文档上每帧可省掉 O(n) 的 diff 计算。
                //    prev.clone() 仍保留（diff_replace 需要 old 参数），无法省。
                if editor.response.changed() {
                    // 借用纪律（形态 B）：`vctx.editor_text()` 的共享借用若直接
                    // 出现在 `if let` 判据里，它会**覆盖整个块**——块内紧接着的
                    // `set_editor_text` / `flush_edit` 是可变借用，直接撞车。
                    // 故先在**内层块**里把这次编辑算完（借用在此结束），
                    // 外层再 `if let` 取用：全程**不多一次全文克隆**。
                    let edit = {
                        let new_text = vctx.editor_text();
                        if prev == new_text {
                            None
                        } else {
                            editor_ops::diff_replace(&prev, new_text)
                        }
                    };
                    if let Some(mut edit) = edit {
                        // 变换是否真的改动了这次编辑（列表续行 / 括号配对 / 退出列表）。
                        // 返回 false 说明领域应用的就是 egui 给的那次编辑，
                        // 光标位置 egui 自己已经算对了。
                        let transformed = editor_ops::transform_insert(&prev, &mut edit);
                        // 关键：把「旧文本 + 编辑」拼成与领域完全一致的最终串，
                        // 直接写回编辑区文本，然后再发领域命令。这样 `flush_edit`
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
                        vctx.set_editor_text(format!(
                            "{}{}{}",
                            &prev[..bs],
                            edit.inserted,
                            &prev[be..]
                        ));
                        // 第二个参数必须是 `prev.len()`（领域旧文本长度），**不是**
                        // 当前编辑区文本的长度——上一行刚把文本拼成变换后的新串，
                        // 「退出列表」分支会把文本改短，新长度装不下
                        // 相对旧文本的 `byte_end`；flush_edit 若按新长度钳制就会把
                        // 区间砍短，领域层只删掉半个标记（实测：`4. ` 残留成 `. `）。
                        vctx.flush_edit(edit, prev.len(), transformed);
                        // 文本一变，egui 就会「把光标滚进视口」（带动画、连续数帧）。
                        // 那是程序为了光标可见而滚的，用户并没有滚，故必须声明成
                        // 程序性位移——否则滚动同步会把它当成用户手势，一帧一次地把
                        // 预览区拉到映射位置（编辑区平滑滚、预览区逐帧跳），
                        // 两栏同时在动即用户看到的「界面闪烁」。
                        let now_ms = vctx.markdown_service().now().millis();
                        vctx.ui_state()
                            .sync
                            .note_programmatic_scroll(Side::Editor, now_ms);
                    }
                }

                // 7) 右键：落在错词上就开「你可能想输入」菜单（V11-07）。
                //
                //    左键点进编辑区一律关菜单：那表示用户要继续打字，
                //    一个浮在正文上的菜单只会挡住他要看的那行。
                if editor.response.clicked() {
                    vctx.close_spell_menu();
                }
                //    指针位置**先取出来**再调 `spell_menu_for`：后者要读文本与
                //    服务，若塞进 `and_then` 的闭包里，闭包会捕获上下文的可变
                //    借用，与紧随其后的 `set_spell_menu` 撞车。
                if editor.response.secondary_clicked()
                    && let Some(pointer) = editor.response.interact_pointer_pos()
                {
                    let opened =
                        spell::spell_menu_for(vctx, &editor.galley, editor.galley_pos, pointer);
                    vctx.set_spell_menu(opened);
                }
            });
        });

    let spell_menu_open = vctx.editor_state().spell_menu.is_some();
    if spell_menu_open {
        spell::draw_spell_menu(vctx, &ctx, &theme);
    }

    vctx.set_editor_scroll_y(out.state.clamped_offset().y);
    vctx.ui_state()
        .sync
        .set_geometry(Side::Editor, out.content_size.y, out.inner_rect.height());
    // 说明：S6-04 时代曾把高亮 / 滚动同步的统计 readout 直接画在编辑区底部，
    // 那段「零高余量里的两行字」会吃掉编辑区的可视高度，使编辑区与预览区的
    // 滚动视口不再等高（两栏对齐的回归根源）。统计数据仍可经
    // `mapper::last_highlight_stats()` 取用（单测在用），只是不再占用版面。
}
