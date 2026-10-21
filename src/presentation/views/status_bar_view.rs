//! 状态栏视图（S6-11e）：类型 / 编码 / 行列 / 字数 / 保存状态 / 拼写 / 主题 / 行数。
//!
//! # 为什么用 `TopBottomPanel::bottom` 而非在 CentralPanel 里画一行
//!
//! 后者会随内容一起滚动，而状态栏必须**始终钉在窗口底部**。
//!
//! # 每一项为什么可点
//!
//! 状态栏项在原型里全是 `<button>`：点编码切编码、点字数开统计、
//! 点「未保存」直接保存、点拼写切开关、点主题切主题。
//! 若某项不可点，它就该用**弱化色 + 无手型光标**画出来，
//! 让用户一眼看出哪些能用 —— 而不是全都画成一样、点了没反应。
//!
//! # 提示消息画在**左端**，不挤占固定项
//!
//! 固定项（类型 / 编码 / 行列 / …）位置必须稳定：用户是**扫**状态栏的，
//! 字数忽然从第三格跳到第一格会让眼睛找不到。消息占用左端的空白处，
//! 出现时把固定项整体右推，消失后复位。

use egui::{Align, CornerRadius, Layout, Rect, Response, Sense, Ui, Vec2};

use crate::app::frame::EditorApp;
use crate::application::dto::{GoalProgressView, StatsView};
use crate::presentation::shortcuts::registry::UiAction;
use crate::presentation::theme::builtin::Theme;

/// 一项的固定高度（与原型的 `--statusbar-h` 内边距一致）。
const ITEM_HEIGHT: f32 = 18.0;
/// 竖线分隔符的宽度。
const SEPARATOR_W: f32 = 1.0;
/// 写作目标进度条的宽度与高度（V11-05）。
const GOAL_BAR: Vec2 = Vec2::new(46.0, 6.0);

/// 把状态栏的编码项同步到**当前活动文档**的真实编码（V11-03 / DEF-05）。
///
/// 选择每帧同步而不是在「打开 / 新建 / 切换标签 / 关闭标签」各处分别写：
/// 那些入口分散在拖拽、命令面板、快捷键、文件树与最近文件里，漏一处就会
/// 留下一个过期值——状态栏显示上一个文档的编码比不显示更糟。
/// 代价只是一次 `HashMap` 查询（应用层旁路表），每帧可忽略。
fn sync_encoding(app: &mut EditorApp) {
    app.set_status_bar_encoding(
        app.active_doc
            .and_then(|id| app.services.documents.encoding_of(id)),
    );
}

/// 绘制底部状态栏。
/// 结算写作目标进度（V11-05）。
///
/// 与 `sync_encoding` 同一套路：每帧从应用层投影，而不是在「打开 / 编辑 /
/// 切换 / 改设置」各处分别写——那些入口太多，漏一处就留一个过期值。
/// 代价是一次读锁 + 一次整数除法，每帧可忽略。
///
/// # 先问「设了目标吗」再取字数（问题 053）
///
/// 未设目标时整项不画（`StatusBarModel.goal`），而字数是一次**整篇统计**
/// （应用层六遍全文遍历，1MB 上 9.49ms / miss，见 `docs/metrics.md` §3.2.5）。
/// 旧写法无条件先取字数、再把算不出来的目标投影丢弃，等于为一条不画的进度条
/// 白付一次统计。改成先问设置（一次读锁 + 一个整数比较）再决定要不要字数——
/// 这与「新增每帧查询先问：构造是 O(1) 还是 O(全文)」（问题 043 的纪律）同源。
fn sync_goal(app: &mut EditorApp, stats: Option<&StatsView>) {
    let now_ms = app.services.markdown.now().millis();
    let progress = if app.services.preferences.has_writing_goal() {
        let words = stats.map_or(0, |s| s.words);
        app.services.preferences.goal_progress(words)
    } else {
        // 未设目标：与 `goal_progress(0)` 的可见结果**逐字段等价**
        // （`goal = 0` → `is_set()` 为假 → 状态栏整项不画，
        // `percent` 为 `None`、`reached` 为假，`words` 在未设目标时无人读），
        // 但省掉了一次整篇统计。
        GoalProgressView::default()
    };
    let doc = app.active_doc.map(|id| id.as_u64());
    app.status_bar.note_goal(progress, doc, now_ms);
}

pub fn show(app: &mut EditorApp, ui: &mut Ui) {
    sync_encoding(app);
    // 状态栏统计防抖（建议一 / 问题 039）：每帧只算一次，供「写作目标进度」
    // 与「字数」共用；打字期沿用上一次静止结果，停手满窗口才刷新。
    // 统计浮层另走 `current_stats`（点击即时），不受影响。
    let stats = app.current_stats_for_status_bar();
    sync_goal(app, stats.as_ref());
    let height = app.theme().metrics.statusbar_height;
    // 先把消息取出来：它带到期时间，必须在绘制前结算，
    // 否则一条 3 天前就该消失的错误会一直画着。
    let now_ms = app.services.markdown.now().millis();
    let message = app.status_bar.take_message(now_ms);
    // 消息 TTL 的到期点交给帧末统一定时（T11）：状态栏是唯一知道
    // 「这条消息还剩多久」的地方。缺了它，用户停手后不会再有帧，
    // 一条 4 秒该消失的提示会一直挂在状态栏上。
    app.arm_status_message_deadline();
    egui::Panel::bottom("rustmark.statusbar")
        .exact_size(height)
        .show(ui, |ui| draw(app, ui, message, stats));
}

/// 状态栏要显示的**全部**信息，一次算清。
///
/// 刻意做成独立函数并返回结构体，而不是散在 `draw` 里就地算：
/// 集成测试需要断言「行列算得对不对」「字数取的是不是当前文档」，
/// 而渲染函数里的临时值测试够不到。
#[derive(Clone, Debug, PartialEq)]
pub struct StatusBarModel {
    /// 文件类型（`Markdown` / `纯文本` …）。
    pub kind: String,
    /// 编码显示名。
    pub encoding: String,
    /// 编码项的提示（说明为何不能点）。
    pub encoding_hint: String,
    /// 光标位置文案（`行 3, 列 5`；有选区时是 `行 3, 列 5 - 行 3, 列 12`）。
    pub position: String,
    /// 字数文案（`字数 1,234`）。
    pub words: String,
    /// 保存状态文案（`已保存` / `未保存`）。
    pub save_state: String,
    /// 是否有未保存修改（决定保存项的颜色与可点性）。
    pub is_dirty: bool,
    /// 拼写检查开关文案。
    pub spell_check: String,
    /// 拼写检查是否开启。
    pub spell_on: bool,
    /// 自动换行开关文案。
    pub word_wrap: String,
    /// 自动换行是否开启。
    pub wrap_on: bool,
    /// 写作目标进度（V11-05）；`None` 表示未设置目标（整项不画）。
    pub goal: Option<GoalProgressView>,
    /// 写作目标项文案（`目标 62%` / `目标达成`）。
    pub goal_label: String,
    /// 写作目标项提示。
    pub goal_hint: String,
    /// 主题文案。
    pub theme_label: String,
    /// 主题项提示。
    pub theme_hint: String,
    /// 行数文案（`128 行`）。
    pub line_count: String,
}

/// 由应用状态构造状态栏模型。
pub fn build_model(app: &EditorApp, stats: Option<StatsView>) -> StatusBarModel {
    // 三处行相关文案（光标行列 / 选区行列 / 「N 行」）共用**一次**全文扫描
    // （问题 053）：旧写法各算各的，1MB 上一帧要扫三遍、其中两遍还各建一份
    // 58k 项的 `Vec<usize>`，实测每帧恒定 1.1ms。
    let index = app.line_index();
    let (line, col) = app.cursor_line_col_in(&index);
    let position = match app.selection_line_col_in(&index) {
        Some(((sl, sc), (el, ec))) => {
            if (sl, sc) == (el, ec) {
                format!("行 {line}, 列 {col}")
            } else {
                format!("行 {sl}, 列 {sc} - 行 {el}, 列 {ec}")
            }
        }
        None => format!("行 {line}, 列 {col}"),
    };
    let words = stats.map_or_else(
        // `None` 表示「未预计算」：退回即时统计（测试 / 浮层路径等同旧行为）。
        || app.current_stats().map_or(0, |s| s.words),
        |s| s.words,
    );
    let is_dirty = app.current_document_dirty();
    let wrap_on = app.editor.word_wrap;
    let spell_on = app.editor.spell_check;
    let (theme_label, theme_hint) = theme_label(app);
    let goal = app.status_bar.goal;
    let goal_visible = goal.is_set();

    StatusBarModel {
        // 未设目标时整项不画：`goal` 仍保留在模型里，
        // 测试要能断言「为什么没画」（是没设目标，还是画漏了）。
        goal: goal_visible.then_some(goal),
        goal_label: app.status_bar.goal_label(),
        goal_hint: app.status_bar.goal_hint(),
        kind: app.current_document_kind().to_string(),
        encoding: app.status_bar.encoding_label().to_string(),
        encoding_hint: app.status_bar.encoding_hint().to_string(),
        position,
        // 千分位：四位以上数字不加分隔符会很难读（「字数 12345」要数位数）。
        words: format!("字数 {}", thousands(words)),
        save_state: if is_dirty { "未保存" } else { "已保存" }.to_string(),
        is_dirty,
        spell_check: format!("拼写检查 {}", if spell_on { "开" } else { "关" }),
        spell_on,
        word_wrap: format!("自动换行 {}", if wrap_on { "开" } else { "关" }),
        wrap_on,
        theme_label,
        theme_hint,
        line_count: format!("{} 行", thousands(index.count())),
    }
}

/// 主题显示名与提示（对齐原型 `StatusBar.vue`）。
///
/// 状态栏的「一键切换」现在是**深↔浅直接互换**（见 `EditorApp::cycle_theme`）：
/// 当前显示深色就提示「点一下变浅色」、反之变深色。这是可预期的一元动作，
/// 不再有三态轮换带来的「浅色点回深色要点两次」困惑（#039）。
fn theme_label(app: &EditorApp) -> (String, String) {
    if app.ui.theme.is_dark() {
        ("深色主题".to_string(), "点击切换为浅色主题".to_string())
    } else {
        ("浅色主题".to_string(), "点击切换为深色主题".to_string())
    }
}

/// 千分位分隔（`12345` → `12,345`）。
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn draw(
    app: &mut EditorApp,
    ui: &mut Ui,
    message: Option<crate::presentation::state::status_bar_ui_state::StatusMessage>,
    stats: Option<StatsView>,
) {
    let theme = app.theme().clone();
    ui.painter()
        .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, theme.bg_secondary);
    let model = build_model(app, stats);
    let now_ms = app.services.markdown.now().millis();

    let mut intents: Vec<IntentKind> = Vec::new();

    ui.horizontal(|ui| {
        // 消息占左端；没有消息时留白，固定项位置不变。
        if let Some(msg) = &message {
            let color = if msg.is_error {
                theme.error
            } else {
                theme.text_secondary
            };
            ui.colored_label(
                color,
                // 成功标记用 √(U+221A) 而非 ✓(U+2713)：后者无字形覆盖，会渲染成方框。
                // ⚠(U+26A0) 有字形，保留。
                format!("{} {}", if msg.is_error { "⚠" } else { "√" }, msg.text),
            );
        }

        // —— 左段（固定顺序：类型 / 编码 / 行列 / 字数 / 保存 / 拼写）——
        item(
            ui,
            &theme,
            &model.kind,
            "文件类型（按扩展名判定）",
            None,
            &mut intents,
        );
        separator(ui, &theme);
        // 编码项**不可点**：底层按文件自动识别编码并写回原编码，
        // 但不提供「切了也不生效」的假开关（详见 `TextEncoding` 的文档）。
        item(
            ui,
            &theme,
            &model.encoding,
            &model.encoding_hint,
            None,
            &mut intents,
        );
        separator(ui, &theme);
        item(ui, &theme, &model.position, "光标位置", None, &mut intents);
        separator(ui, &theme);
        item(
            ui,
            &theme,
            &model.words,
            "点击查看完整文档统计",
            Some(IntentKind::Stats),
            &mut intents,
        );
        separator(ui, &theme);
        // 写作目标：未设置时整项不出现（V11-05）。
        // 与「字数」放在相邻位置——它是字数的**进度解释**，隔开了读不出关系。
        if let Some(goal) = model.goal {
            goal_item(ui, &theme, &model, goal, &mut intents);
            separator(ui, &theme);
        }
        item(
            ui,
            &theme,
            &model.save_state,
            "点击保存（Ctrl+S）",
            Some(IntentKind::Save),
            &mut intents,
        );
        separator(ui, &theme);
        item(
            ui,
            &theme,
            &model.spell_check,
            "点击开启 / 关闭拼写检查",
            Some(IntentKind::Spell),
            &mut intents,
        );

        // —— 右段 ——
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // `line_count` 是**静态**项：原型里它没有点击行为，
            // 画成可点样式会让用户去找「点了会怎样」。
            static_text(ui, &theme, &model.line_count);
            separator(ui, &theme);
            item(
                ui,
                &theme,
                &model.theme_label,
                &model.theme_hint,
                Some(IntentKind::Theme),
                &mut intents,
            );
            separator(ui, &theme);
            item(
                ui,
                &theme,
                &model.word_wrap,
                "点击切换自动换行",
                Some(IntentKind::Wrap),
                &mut intents,
            );
        });
    });

    // 渲染完再执行：绘制过程中改 `app.editor.spell_check` 会与
    // 本帧已读出的 `model` 不一致（model 里还是旧值），
    // 用户会看到「开关显示关了但已经开了」这种一帧错位。
    for intent in intents {
        run_intent(app, intent, now_ms);
    }
}

/// 状态栏项被点击后要做什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntentKind {
    Save,
    Stats,
    Spell,
    Wrap,
    Theme,
}

/// 执行一项点击意图。
fn run_intent(app: &mut EditorApp, intent: IntentKind, now_ms: u64) {
    match intent {
        IntentKind::Save => match app.save_document(false) {
            Ok(()) => app.status_bar.notice("已保存", now_ms),
            Err(e) => app.status_bar.error(format!("保存失败：{e}"), now_ms),
        },
        IntentKind::Spell => app.toggle_spell_check(),
        IntentKind::Wrap => app.toggle_word_wrap(),
        IntentKind::Theme => app.execute(UiAction::ToggleTheme),
        IntentKind::Stats => app.execute(UiAction::DocumentStats),
    }
}

/// 画一个状态栏项。
///
/// `kind = None` 的项用**弱化色**画且不注册点击：让用户一眼看出
/// 哪些能点，比全都画成一样、点了没反应要好。
///
/// # 悬停底色为什么先画
///
/// egui 的 `Label` 在 `ui.add` 内部就把文字画掉了，底色若在
/// `ui.add` **之后**画会把文字盖住。只能反过来：先
/// `allocate_exact_size` 占位并画底色，再用 `painter.galley`
/// 把预先排版好的文字画进同一块矩形（底色在下层、文字在上层）。
/// 不用 `painter.text` 重画：那个 API 每次调用都重新排版且不参与
/// egui 的字体回退，非 ASCII 混排文本会画歪。
fn item(
    ui: &mut Ui,
    theme: &Theme,
    text: &str,
    hint: &str,
    kind: Option<IntentKind>,
    intents: &mut Vec<IntentKind>,
) -> Response {
    let clickable = kind.is_some();
    let color = if clickable {
        theme.text_secondary
    } else {
        theme.text_secondary.gamma_multiply(0.75)
    };
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), color);
    // 宽度按实际排版结果 + 左右各 3px 内边距（原型 `.item { padding: 0 6px }`
    // 的一半，让相邻项的底色不粘连）。
    let desired = Vec2::new(galley.size().x + 6.0, ITEM_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let hovered = response.hovered();
    let clicked = response.clicked();

    if clickable && hovered {
        // 悬停底色用 `border`（对齐原型 `.item:hover { background: var(--border) }`），
        // 用 `accent_soft` 会与「选中」态撞色。
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(3), theme.border);
    }

    // 文字画在底色之上，且用 painter 直接画进**已分配**的矩形，
    // 不再走 `ui.add`（那会二次分配空间、把刚画的底色顶到别处）。
    ui.painter().galley(
        rect.left_top() + Vec2::new(3.0, (rect.height() - galley.size().y) / 2.0),
        galley,
        if clickable && hovered {
            theme.text_primary
        } else {
            color
        },
    );

    if let Some(kind) = kind
        && clicked
    {
        intents.push(kind);
    }
    // 提示**无条件**挂（不区分 clickable）：不可点的项也要能解释
    // 「为什么点了没反应」（如编码项的「当前仅支持 UTF-8」）。
    //
    // 必须放在最后 —— `on_hover_text` 按值消耗 `Response`，
    // 而上面已经读过它的 `hovered()` / `clicked()`。
    response.on_hover_text(hint)
}

/// 画写作目标项：文案 + 一条细进度条（V11-05）。
///
/// # 为什么画条而不只写「目标 62%」
///
/// 百分比是**数字**，要读；条的填充比例是**图形**，余量多少一眼就够，
/// 不必在状态栏里做除法。这是状态栏本就承担「扫一眼」职责的地方。
///
/// 条用 `border` 作底、`accent`（达成时 `success`）作填充：
/// 不用 `accent_soft` 是为了和「选中态」区分，且细条需要足够对比度才看得见。
fn goal_item(
    ui: &mut Ui,
    theme: &Theme,
    model: &StatusBarModel,
    goal: GoalProgressView,
    intents: &mut Vec<IntentKind>,
) {
    item(
        ui,
        theme,
        &model.goal_label,
        &model.goal_hint,
        // 点它开统计浮层：用户看到 62% 的下一个问题必然是「还差多少字」，
        // 而答案就在统计里。另开一个「目标设置」入口会让状态栏第一次承担配置职责。
        Some(IntentKind::Stats),
        intents,
    );
    let (rect, _) = ui.allocate_exact_size(Vec2::new(GOAL_BAR.x, ITEM_HEIGHT), Sense::hover());
    let ratio = goal.percent.unwrap_or(0) as f32 / 100.0;
    let bar = Rect::from_center_size(rect.center(), GOAL_BAR);
    ui.painter()
        .rect_filled(bar, CornerRadius::same(3), theme.border);
    if ratio > 0.0 {
        let filled = Rect::from_min_size(bar.min, Vec2::new(GOAL_BAR.x * ratio, GOAL_BAR.y));
        let color = if goal.reached {
            theme.success
        } else {
            theme.accent
        };
        ui.painter()
            .rect_filled(filled, CornerRadius::same(3), color);
    }
}

/// 静态项（不可点）。
///
/// 原型里 `{{ store.lineCount }} 行` 没有点击行为；若画成可点样式，
/// 用户会去找「点了会怎样」并认为界面有 bug。
fn static_text(ui: &mut Ui, theme: &Theme, text: &str) {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(12.0),
        theme.text_secondary.gamma_multiply(0.75),
    );
    let desired = Vec2::new(galley.size().x + 6.0, ITEM_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::hover());
    ui.painter().galley(
        rect.left_top() + Vec2::new(3.0, (rect.height() - galley.size().y) / 2.0),
        galley,
        theme.text_secondary.gamma_multiply(0.75),
    );
    let _ = response;
}

/// 项之间的竖线分隔符。
fn separator(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(SEPARATOR_W, 12.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme.border);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separates_correctly() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(12_345), "12,345");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
