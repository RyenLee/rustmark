//! 全局搜索标签：搜索框、范围切换器、选项开关、命中分组渲染与跳转。

use egui::epaint::text::{LayoutJob, TextFormat};
use egui::{Color32, Layout, RichText, ScrollArea, Ui};

use crate::app::frame::EditorApp;
use crate::presentation::state::sidebar_ui_state::{
    SEARCH_SCOPES, SEARCH_TOGGLES, SearchGroup, SearchScope,
};
use crate::presentation::theme::builtin::Theme;

use super::files::display_name;
use super::*;

/// 搜索标签。
pub(super) fn search_tab(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let mut query = app.sidebar.search_query.clone();
    let response = ui.add(
        egui::TextEdit::singleline(&mut query)
            .id(egui::Id::unique(SEARCH_INPUT_ID))
            // 占位提示随范围变化：打字之前就知道这一框会搜到哪里。
            .hint_text(app.sidebar.search_scope.placeholder())
            .desired_width(ui.available_width()),
    );
    if response.changed() {
        app.sidebar.search_query = query;
    }
    // 范围切换器紧贴输入框：它是比下面那排选项更高一级的决定
    // （决定「搜谁」先于「怎么搜」），位置上也要读得出这个次序。
    scope_row(app, ui, theme);
    toggle_row(app, ui, theme);
    ui.separator();

    // 「当前文档」范围 + 没有活动文档：说清为什么空，并指出去路。
    //
    // 这条分支必须在结果渲染之前——否则用户看到的是
    // 「没有找到匹配」而不是「没有打开的文档」，会以为搜索坏了。
    // （计数此时为 0 是 `search_workspace` 的保证，见其文档。）
    if app.sidebar.search_scope == SearchScope::CurrentDocument && app.active_doc.is_none() {
        ui.weak("没有打开的文档");
        ui.weak("打开一个 .md 后，这里会列出它的命中；也可切换到「打开的文档」跨文档查找。");
        return;
    }
    let Some(result) = app.search_workspace() else {
        ui.colored_label(theme.error, "搜索失败，请检查工作区目录");
        return;
    };
    if !app.sidebar.should_search() {
        // 与输入框占位提示同源：默认范围已是「当前文档」，这里若还写
        // 「在所有打开的文档中搜索」，等于界面的两处声明自相矛盾。
        ui.weak(app.sidebar.search_scope.placeholder());
        return;
    }
    if result.is_empty() {
        ui.weak(format!(
            "没有找到匹配「{}」的内容",
            app.sidebar.search_query
        ));
        return;
    }
    let mut jump: Option<(String, usize)> = None;
    ScrollArea::vertical()
        .id_salt("rustmark.sidebar.search_results")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for group in &result.groups {
                group_header(ui, theme, group);
                for hit in &group.hits {
                    if hit_row(ui, theme, group, hit.line, &hit.snippet, hit.highlight).clicked() {
                        jump = Some((group.path.clone(), hit.line));
                    }
                }
            }
            if result.truncated {
                ui.colored_label(theme.warning, "结果较多，已显示前 300 条，请缩小搜索范围");
            }
        });
    if let Some((path, line)) = jump {
        jump_to_hit(app, &path, line);
    }
}

/// 范围切换行：当前文档 / 打开的文档。
///
/// 形态与顶部标签栏、大纲层级条一致（`selectable_label`）——同一套
/// 「选中即高亮」的语言，用户不必重新学。层级差别靠**位置与用词**
/// 表达：它紧贴输入框（比选项更靠近「搜什么」），且写的是词不是符号。
///
/// 为什么不做成下拉框：两个选项 + 需要常瞥见当前范围，一次点击
/// 就能到位；下拉要多一步展开，还会把「现在搜的是谁」藏起来。
///
/// # 选中态文字不能用强调色（问题 037）
///
/// `selectable_label` 选中时的底色取自 `visuals.selection.bg_fill`
/// （egui 的按钮选中态即 `widgets.active` → 本主题映射到 `selection_bg`），
/// 而 `selection_bg` 是**用户可配的**（自定义调色板里常见
/// `selection_bg = accent`）。此时若把文字也染成 `theme.accent`，
/// 文字与底色完全相同 → 「看不到按钮的文案」。
/// 故选中文案用正文色（ mapper 同时把 `fg_stroke` 映射为正文色，
/// 两条路都保证对比度），未选中才用次要色。
fn scope_row(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        for scope in SEARCH_SCOPES {
            let on = app.sidebar.search_scope == scope;
            let text = RichText::new(scope.label()).color(if on {
                theme.text_primary
            } else {
                theme.text_secondary
            });
            if ui
                .selectable_label(on, text)
                .on_hover_text(scope.hint())
                .clicked()
            {
                app.sidebar.search_scope = scope;
            }
        }
    });
}

/// 三个搜索开关。
///
/// 开关是**双态**的：选中后再点一次是取消，不是无操作（旧实现里
/// `clicked() && !on` 把「已开启时再点」整个吞掉，开关点不灭）。
///
/// 选中态配色与 [`scope_row`] 同源：强调色文字可能撞上用户自选的
/// `selection_bg`（同色即不可见），故选中文案一律用正文色。
fn toggle_row(app: &mut EditorApp, ui: &mut Ui, theme: &Theme) {
    let options = app.sidebar.search_options;
    ui.horizontal(|ui| {
        for toggle in SEARCH_TOGGLES {
            let on = toggle.is_on(&options);
            let text = RichText::new(toggle.badge()).color(if on {
                theme.text_primary
            } else {
                theme.text_secondary
            });
            if ui
                .selectable_label(on, text)
                .on_hover_text(toggle.hint())
                .clicked()
            {
                app.sidebar.search_options = options.toggled(toggle);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 一键重置：三个开关全部关闭。已全关时置灰（点了没反应会让人
            // 以为按钮坏了，置灰则明示「已是默认状态」）。
            ui.add_enabled(!options.is_plain(), egui::Button::new("重置").small())
                .on_hover_text("关闭全部搜索选项（区分大小写 / 全字匹配 / 正则）");
            ui.weak(format!("{} 条结果", count_hits(app)));
        });
    });
}

/// 命中总数（开关行右侧的计数）。
///
/// 复用 `search_workspace` 的结果而不是再查一次：每次点击开关都重扫全盘
/// 会让界面在点「Aa」时卡一下。代价是这一帧的计数用的是**当前查询**的结果——
/// 而这正是用户要看到的数字。
fn count_hits(app: &mut EditorApp) -> usize {
    app.search_workspace().map(|r| r.total()).unwrap_or(0)
}

/// 分组头（文件名 + 命中数）。
///
/// 文件名必须截断（缺陷类 024）：水平布局里的裸 `label` 不换行，长文件名
/// 会把侧栏内容撑出面板——egui `SidePanel` 按**内容 response.rect** 给后续
/// 布局保留空间，超出面板的部分没有任何面板绘制，真实渲染下露出交换链
/// 清屏色（黑），编辑区也被整体推右（与「检查」标签的黑框同一根因）。
fn group_header(ui: &mut Ui, theme: &Theme, group: &SearchGroup) {
    ui.horizontal(|ui| {
        // 右侧命中数预留一段固定宽，文件名截断到剩余宽。
        let avail = (ui.available_width() - HITS_COUNT_RESERVE).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(avail, 0.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&group.file_name).color(theme.text_primary))
                        .truncate(),
                );
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.weak(format!("{}", group.hits.len()));
        });
    });
}

/// 分组头右侧命中数的预留宽（含行内边距）。
const HITS_COUNT_RESERVE: f32 = 36.0;

/// 单条命中（行号 + 片段，高亮命中词）。
///
/// # 片段必须截断，且必须是**一个 galley**（缺陷类 033）
///
/// 片段不截断会把侧栏撑出面板（缺陷类 024，见 [`group_header`]）。而
/// 「前 / 命中 / 后」三段各自 `truncate()` 同样不行——曾经正是黑框的根因：
/// 水平布局里第三段的可用宽往往只剩几个点，比一个字形还窄，而 epaint 的
/// 截断实现保证「行不为空」——宽度装不下任何字形时它仍会排出一个字形
/// （或省略号），于是这一段的 galley 宽超过可用宽，本行 `min_rect` 冲出面板。
/// 溢出再经 `Ui::expand_to_include_rect` 逐级放大，把侧栏的 response 矩形
/// 撑宽，`SidePanel` 据此把编辑区整体推右：侧栏右缘之外没有任何面板绘制，
/// 真实渲染下露出交换链清屏色（黑）——用户看到的正是「切到搜索标签时
/// 侧栏右侧一条竖向黑框」。
///
/// 修法是把三段拼成**一个** `LayoutJob`（用不同底色的 section 表达高亮），
/// 只截断一次：可用宽就是整行的剩余宽，不再出现「只够放一个字形」的残条。
///
/// 整行必须显式带 `Sense::click()`：`ui.horizontal` 的子 Ui 默认 sense 是
/// **hover**（`UiBuilder::sense` 默认值），其 `Response::clicked()` 恒为
/// false——症状是「搜索命中点击无响应」（问题 1）。
fn hit_row(
    ui: &mut Ui,
    theme: &Theme,
    group: &SearchGroup,
    line: usize,
    snippet: &str,
    highlight: (usize, usize),
) -> egui::Response {
    // 整行显式声明 `Sense::click()`：行号 + 命中片段两段 `Ui` 默认只有 hover
    // sense，会盖在行交互之上、吞掉点击（与大纲行同款缺陷）。先 `ui.horizontal`
    // 排出内容取回整行矩形，再用 `ui.interact` 挂 click-sense 命中区。
    let row_response = ui
        .horizontal(|ui| {
            ui.add_space(ROW_PAD);
            ui.weak(format!("{}", line + 1));
            // 片段占「剩余宽」并截断；它自己是一个 galley，截断一次即可
            // （见 [`hit_row`] 上面对缺陷类 033 的说明）。
            ui.add(egui::Label::new(snippet_job(ui, theme, snippet, highlight)).truncate());
        })
        .response;
    let row_id = ui.scope_id().with((
        "rustmark.search.hit",
        &group.path,
        line,
        snippet,
        highlight.0,
        highlight.1,
    ));
    ui.interact(row_response.rect, row_id, egui::Sense::click())
}

/// 命中片段按字节区间切分（DTO 的 `highlight` 是**字符**下标，见
/// [`SearchHitView`](crate::application::dto::document::SearchHitView)）。
struct ByteRange {
    start: usize,
    end: usize,
}

/// 字符下标 → 字节下标（越界时收敛到字符串末尾，绝不切在字符中间）。
fn byte_index(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(i, _)| i)
}

/// 把高亮区间换算成字节区间，并夹紧到合法范围。
fn highlight_bytes(snippet: &str, highlight: (usize, usize)) -> ByteRange {
    let end = byte_index(snippet, highlight.1);
    let start = byte_index(snippet, highlight.0).min(end);
    ByteRange { start, end }
}

/// 片段排成**一个** `LayoutJob`：命中词自带底纹，前后文是默认色。
///
/// 只用 section 表达差异，不拆成多个 `Label`——拆开就是缺陷类 033（见
/// [`hit_row`]）。底纹色沿用主题的 `find_current_bg`（与查找栏高亮同一色）。
fn snippet_job(ui: &Ui, theme: &Theme, snippet: &str, highlight: (usize, usize)) -> LayoutJob {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let plain = TextFormat {
        font_id,
        // PLACEHOLDER：取当前视觉主题的正文色（与 `ui.weak` 的西文默认色一致），
        // 不在这一层把颜色写死。
        color: Color32::PLACEHOLDER,
        ..Default::default()
    };
    let marked = TextFormat {
        background: theme.find_current_bg,
        // 与 `RichText::background_color` 的默认外扩一致（1pt），
        // 高亮块不会紧贴字形边缘。
        expand_bg: 1.0,
        ..plain.clone()
    };
    let range = highlight_bytes(snippet, highlight);
    let mut job = LayoutJob::default();
    if range.start > 0 {
        job.append(&snippet[..range.start], 0.0, plain.clone());
    }
    if range.end > range.start {
        job.append(&snippet[range.start..range.end], 0.0, marked);
    }
    if range.end < snippet.len() {
        job.append(&snippet[range.end..], 0.0, plain);
    }
    job
}

/// 跳到某文件某行（先切文件，再按行号定位）。
fn jump_to_hit(app: &mut EditorApp, path: &str, line: usize) {
    let already_open = app.current_document_path().eq_ignore_ascii_case(path);
    if !already_open && let Err(e) = app.open_workspace_file(path) {
        app.sidebar
            .fail(format!("无法打开 {}：{e}", display_name(path)));
        return;
    }
    // 行号 → 字节偏移，与大纲跳转走同一个换算，不各算一遍。
    let starts = crate::presentation::views::scroll_sync::line_start_offsets(&app.editor_text);
    let Some(&offset) = starts.get(line) else {
        return;
    };
    app.goto_byte_offset(offset);
}
