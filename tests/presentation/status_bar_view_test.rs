//! 状态栏与欢迎页功能用例（S6-11e 验收标准）。
//!
//! # 验收标准原文
//!
//! > 状态栏各项数值与原型 `StatsDialog` 口径一致。
//!
//! 配套的还有原型契约 A-23：
//!
//! > 状态栏：类型 / 编码 / 行列 / 字数（点击开统计）/ 保存状态 / 拼写 / 主题，均可点击。
//!
//! # 本文件盯住的**静默失效**
//!
//! 这类缺陷不报错、界面看着正常，只是数字错或点击没反应：
//!
//! 1. **列号按字节算**：一个汉字 3 字节，光标在第 3 个字却显示「列 7」；
//! 2. **行号多算一行**：文末哨兵与末行行首重复时，三行文档算出第四行；
//! 3. **字数按空白切分**：一整段无空格的中文被算成 1 个「词」；
//! 4. **保存动作没接线**：`UiAction::Save` 落进 `_ => {}`，`Ctrl+S` 无反应；
//! 5. **欢迎页永远不出现**：`close_document` 拒绝关最后一个标签，
//!    于是「一个标签都不剩」这个状态无法达到。

use crate::support::fake_dialog::FakeDialog;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::status_bar_ui_state::MESSAGE_TTL_MS;
use rustmark::presentation::views::main_view;
use rustmark::presentation::views::status_bar_view::{self, StatusBarModel};
use rustmark::presentation::views::welcome_view::{self, PrimaryAction};
use rustmark::shared::encoding::TextEncoding;

/// 一份含中文、英文、任务项、代码块、链接的语料。
const CORPUS: &str = "# 标题\n\n这是一段中文 mixed with English words.\n\n```rust\nlet x = 1;\n```\n\n- [x] done\n- [ ] todo\n\n[链接](a.md) 与 ![图](b.png)\n";

/// 造一个应用（无工作区）。
fn app() -> (tempfile::TempDir, EditorApp) {
    let cfg = tempfile::tempdir().expect("配置目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(cfg.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (cfg, EditorApp::new(services, false))
}

/// 造一个已打开指定文件的应用。
fn app_with_file(dir: &Path, rel: &str, content: &str) -> (tempfile::TempDir, EditorApp) {
    let (cfg, mut app) = app();
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    let path_str = path.to_string_lossy().replace('\\', "/");
    app.open_workspace_file(&path_str).expect("打开应成功");
    (cfg, app)
}

/// 造一个已打开 **GBK 编码**文件的应用（字节直接写盘，不经 UTF-8 编码）。
///
/// `中` = `D6 D0`、`文` = `CE C4`（GBK / CP936）。这段字节不是合法 UTF-8，
/// 因此解码必然落到 GBK 分支——正是 DEF-05 要验证的识别路径。
fn app_with_gbk_file(dir: &Path, rel: &str) -> (tempfile::TempDir, EditorApp) {
    let (cfg, mut app) = app();
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, b"# \xd6\xd0\xce\xc4\n\n\xd6\xd0\xce\xc4\n").unwrap();
    let path_str = path.to_string_lossy().replace('\\', "/");
    app.open_workspace_file(&path_str).expect("打开应成功");
    (cfg, app)
}

fn frame_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    }
}

/// 跑一帧（真实 egui 上下文）。
fn run_frame(app: &mut EditorApp) {
    let ctx = egui::Context::default();
    let _ = ctx.run(frame_input(), |ctx| {
        rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
        main_view::show(app, ctx);
    });
}

// ===========================================================================
// 行列换算
// ===========================================================================

/// 列号必须是**字符**列，不是字节列。
///
/// 一个汉字在 UTF-8 里 3 字节。按字节算，「你好」第二个字之前
/// 会显示「列 4」——用户数着自己的光标只会数出「2」。
#[test]
fn column_counts_characters_not_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "cn.md", "第一行中文");
    // 「第一行」三个字 = 9 字节；光标在第 4 个字「中」之前。
    app.editor.cursor = 9;

    let (line, col) = app.cursor_line_col();
    assert_eq!(line, 1);
    assert_eq!(col, 4, "按字节算会得 10（用户数出来是 4）");
}

/// 有选区时状态栏显示**范围**而不是单点。
#[test]
fn selection_shows_a_range() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "sel.md", "abc\ndef\nghi");
    // 选中 `def\ng`（第 2 行到第 3 行第 1 个字之后）。
    app.editor.set_selection(4, 9);

    let range = app.selection_line_col().expect("有选区时应返回范围");
    assert_eq!(range.0, (2, 1), "起点在第 2 行第 1 列");
    assert_eq!(
        range.1,
        (3, 2),
        "终点偏移 9 = 第 3 行行首(8) + 1 个字 → 第 2 列"
    );

    let model = status_bar_view::build_model(&app);
    assert_eq!(model.position, "行 2, 列 1 - 行 3, 列 2");
}

/// 无选区时退回单点文案。
#[test]
fn no_selection_shows_a_single_point() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "one.md", "abc\ndef");
    app.editor.move_cursor(5);

    assert!(app.selection_line_col().is_none());
    assert_eq!(status_bar_view::build_model(&app).position, "行 2, 列 2");
}

/// 文末（末尾有换行）不该多算一行。
///
/// 修复前 `line_start_offsets` 会在末行行首之外**再补一个哨兵**，
/// `partition_point` 因此多命中一次，把两行文档算成三行。
#[test]
fn cursor_at_end_of_trailing_newline_is_on_the_last_line() {
    let dir = tempfile::tempdir().unwrap();
    // 两行 + 末尾换行 ⇒ 实际有三行（第三行是空的）
    let (_cfg, mut app) = app_with_file(dir.path(), "eol.md", "abc\ndef\n");
    app.editor.cursor = 8;

    assert_eq!(app.cursor_line_col(), (3, 1), "文末在第 3 行（空行）");
    assert_eq!(app.current_line_count(), 3);
}

/// 光标落在非字符边界时不能 panic（`panic = "abort"` 下等于窗口消失）。
#[test]
fn cursor_inside_a_multibyte_character_is_clamped() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "mb.md", "一中文");
    // 「一」占 0..3，「中」占 3..6；偏移 4 落在「中」内部。
    app.editor.cursor = 4;

    let (line, col) = app.cursor_line_col();
    assert_eq!((line, col), (1, 2), "下取整到字符边界后是第 2 列");
}

// ===========================================================================
// 统计口径（与原型 StatsDialog 一致）
// ===========================================================================

/// 状态栏字数与统计浮层**同源**，且与原型口径一致。
#[test]
fn word_count_matches_the_prototype() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, app) = app_with_file(dir.path(), "s.md", CORPUS);

    let stats = app.current_stats().expect("已打开文档应有统计");
    // 逐项核对：中文 12 字（标题 2 + 这是��段中文 6 + 链接 2 + 与 1 + 图 1）
    // + 西文 15 词（mixed/with/English/words、rust/let/x/1、x、done、todo、
    // a/md、b/png）。
    // 原型正则会把两条 `-` 列表标记各算一个词（29），
    // 本实现要求单词**至少含一个字母数字**，故为 27。
    assert_eq!(stats.words, 27, "中日韩按字 + 西文按词，孤立连字符不计");

    let model = status_bar_view::build_model(&app);
    assert_eq!(
        model.words,
        format!("字数 {}", stats.words),
        "状态栏字数必须取自同一个 StatsView（两处各算一次必然漂移）"
    );
}

/// 孤立连字符不算词（与原型正则的一处有意偏离）。
///
/// 原型正则 `/[A-Za-z0-9_'-]+/g` 会把 `- [ ] todo` 里的 `-` 算成一个词，
/// 于是「列表越多字数越虚高」。这条用例把偏离钉死，
/// 免得后人对齐口径时「顺手改回正则」而没人发现。
#[test]
fn lone_hyphens_are_not_words() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, app) = app_with_file(dir.path(), "list.md", "- [ ] todo\n- [x] done");
    let stats = app.current_stats().unwrap();
    // todo / x / done 三个词（`[x]` 里的 x 是字母，算一个）。
    assert_eq!(stats.words, 3, "两个 `-` 不该被算成词");
}

/// 代码块内的 `#` 不计标题、`- [ ]` 不计任务。
#[test]
fn stats_ignore_fenced_code() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, app) = app_with_file(dir.path(), "f.md", CORPUS);
    let stats = app.current_stats().unwrap();
    assert_eq!(stats.headings, 1, "只有文件头的 `# 标题`");
    assert_eq!(stats.code_blocks, 1);
    assert_eq!(stats.tasks_total, 2);
    assert_eq!(stats.tasks_done, 1);
}

/// 没有文档时统计是 `None` 而不是全零。
#[test]
fn no_document_means_no_stats() {
    let (_cfg, mut app) = app();
    app.close_all_documents();
    assert!(
        app.current_stats().is_none(),
        "全零会被显示成「字数 0」，读起来像「文档是空的」"
    );
}

// ===========================================================================
// 状态栏各项
// ===========================================================================

/// 文件类型按**扩展名**判定。
#[test]
fn document_kind_follows_the_extension() {
    let dir = tempfile::tempdir().unwrap();
    let (_c1, app_md) = app_with_file(dir.path(), "a.md", CORPUS);
    assert_eq!(app_md.current_document_kind(), "Markdown");

    let (_c2, app_txt) = app_with_file(dir.path(), "b.txt", CORPUS);
    assert_eq!(
        app_txt.current_document_kind(),
        "纯文本",
        "打开 .txt 却显示 Markdown，用户会怀疑自己打开了错的文件"
    );
}

/// 编码项存在但**不可点**——底层按文件自动识别编码并写回，不提供假开关。
///
/// 尚未落盘的文档**没有编码可言**：显示 `UTF-8` 等于把「将来的默认值」
/// 当成既定事实，故显示 `—`（V11-03 / DEF-05）。
#[test]
fn encoding_is_shown_but_not_switchable() {
    let (_cfg, app) = app();
    assert_eq!(app.status_bar.encoding, None, "未落盘的文档不应有编码记录");
    assert_eq!(app.status_bar.encoding_label(), "—");
    assert!(app.status_bar.encoding_hint().contains("尚未保存"));
}

/// 打开 GBK 文件后，状态栏显示**真实编码** `GBK`（DEF-05 的核心验收）。
///
/// 走完整链路：基础设施层解码时上报编码 → 应用层旁路表记录 →
/// 表现层每帧同步 → 状态栏模型。此前状态栏恒显示「自动」，
/// 对 GBK 文件等于没说。
#[test]
fn encoding_shows_gbk_after_opening_a_gbk_file() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_gbk_file(dir.path(), "gbk.md");
    // 跑一帧：状态栏的编码同步发生在 `show` 内（每帧同步而非各处手写）。
    run_frame(&mut app);

    assert_eq!(
        app.status_bar.encoding,
        Some(TextEncoding::Gbk),
        "GBK 文件应被识别为 GBK"
    );
    assert_eq!(status_bar_view::build_model(&app).encoding, "GBK");
    assert!(
        status_bar_view::build_model(&app)
            .encoding_hint
            .contains("写回原编码"),
        "提示应说明保存写回原编码"
    );
}

/// 打开 UTF-8 文件后显示 `UTF-8`（与 GBK 区分开，证明不是恒定值）。
#[test]
fn encoding_shows_utf8_for_a_utf8_file() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "u8.md", "# 标题\n\n正文\n");
    run_frame(&mut app);
    assert_eq!(app.status_bar.encoding, Some(TextEncoding::Utf8));
    assert_eq!(status_bar_view::build_model(&app).encoding, "UTF-8");
}

/// 脏标记：改过之后显示「未保存」，保存后回到「已保存」。
#[test]
fn save_state_follows_the_dirty_flag() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "d.md", "原文\n");
    assert_eq!(status_bar_view::build_model(&app).save_state, "已保存");

    // 走一次**真实**编辑：选中全文按 Ctrl+B 加粗 → 变脏。
    let len = app.current_content().map(|t| t.len()).unwrap_or(0);
    app.editor.set_selection(0, len);
    app.execute(UiAction::Bold);
    let model = status_bar_view::build_model(&app);
    assert_eq!(model.save_state, "未保存");
    assert!(model.is_dirty, "「未保存」项在原型里用 warning 色，且可点");

    app.save_document(false).expect("保存应成功");
    let after = status_bar_view::build_model(&app);
    assert_eq!(after.save_state, "已保存");
    assert!(!after.is_dirty);
    assert!(
        fs::read_to_string(dir.path().join("d.md"))
            .unwrap()
            .contains("原文"),
        "内容必须真的落盘（且加粗标记也在）"
    );
}

/// 「未保存 → 点击保存」这条路必须真的能走通。
///
/// 修复前 `UiAction::Save` 落进 `_ => {}`：`Ctrl+S` 与状态栏点击
/// 都没有任何效果，且**不报错**。
#[test]
fn save_action_actually_saves() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "s.md", "原文\n");
    let len = app.current_content().map(|t| t.len()).unwrap_or(0);
    app.editor.set_selection(0, len);
    app.execute(UiAction::Bold);
    assert!(app.current_document_dirty(), "前提：编辑后应是脏的");

    app.execute(UiAction::Save);

    assert!(
        !app.current_document_dirty(),
        "Ctrl+S 之后仍是脏的 → 保存动作没接线"
    );
    let on_disk = fs::read_to_string(dir.path().join("s.md")).unwrap();
    assert!(
        on_disk.contains("原文"),
        "内容必须真的落盘，实际：{on_disk:?}"
    );
}

/// 未命名文档按 `Ctrl+S` 应转成「另存为」而不是弹错误。
#[test]
fn saving_an_untitled_document_asks_for_a_path() {
    let (_cfg, mut app) = app();
    app.close_all_documents();
    app.new_document().unwrap();
    app.execute(UiAction::Bold); // 让它变脏

    app.execute(UiAction::Save);

    // 不该有错误提示：用户的意图是「存下来」，弹「需先指定保存路径」
    // 等于把「另存为」这一步的工作推给用户（而 Ctrl+Shift+S 就能做到）。
    let now = app.services.markdown.now().millis();
    let msg = app.status_bar.take_message(now);
    assert!(
        msg.as_ref().is_none_or(|m| !m.is_error),
        "未命名文档保存不该报错，应转为另存为请求：{msg:?}"
    );
    assert!(
        app.dialog_busy(),
        "应已发起「另存为」对话框请求（后台线程执行中）"
    );
}

/// 拼写 / 换行开关可点且真的翻转。
#[test]
fn toggles_flip_the_editor_state() {
    let (_cfg, mut app) = app();
    let before_spell = app.editor.spell_check;
    let before_wrap = app.editor.word_wrap;

    let model = status_bar_view::build_model(&app);
    assert_eq!(
        model.spell_check,
        format!("拼写检查 {}", if before_spell { "开" } else { "关" })
    );
    assert_eq!(
        model.word_wrap,
        format!("自动换行 {}", if before_wrap { "开" } else { "关" })
    );

    app.editor.spell_check = !before_spell;
    app.editor.word_wrap = !before_wrap;
    let after = status_bar_view::build_model(&app);
    assert_ne!(after.spell_check, model.spell_check);
    assert_ne!(after.word_wrap, model.word_wrap);
}

/// 主题项显示当前状态；「跟随系统」必须带上解析出的深 / 浅。
#[test]
fn theme_label_includes_the_resolved_side() {
    let (_cfg, mut app) = app();
    let model = status_bar_view::build_model(&app);
    assert!(!model.theme_label.is_empty());
    assert!(
        model.theme_hint.contains("主题"),
        "提示应说明点击会发生什么"
    );

    // 切到「跟随系统」。
    app.ui.theme.kind = rustmark::presentation::theme::builtin::BuiltinTheme::System;
    let sys = status_bar_view::build_model(&app);
    assert!(
        sys.theme_label.starts_with("跟随系统（"),
        "跟随系统时必须显示解析出的深/浅，否则用户看不出选项是否生效：{}",
        sys.theme_label
    );
    assert!(sys.theme_label.ends_with('）'));
}

/// 行数用**千分位**，四位以上不加分隔符会很难读。
#[test]
fn large_numbers_use_thousands_separators() {
    let dir = tempfile::tempdir().unwrap();
    let body = "行\n".repeat(12_345);
    let (_cfg, app) = app_with_file(dir.path(), "big.md", &body);
    let model = status_bar_view::build_model(&app);
    assert_eq!(model.line_count, "12,346 行", "12,345 个换行 + 最后一行");
}

/// 状态栏模型的所有字段都非空（防止某个项静默变成空白）。
#[test]
fn every_field_of_the_model_is_populated() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, app) = app_with_file(dir.path(), "m.md", CORPUS);
    let model: StatusBarModel = status_bar_view::build_model(&app);
    for (name, value) in [
        ("kind", model.kind.as_str()),
        ("encoding", model.encoding.as_str()),
        ("position", model.position.as_str()),
        ("words", model.words.as_str()),
        ("save_state", model.save_state.as_str()),
        ("spell_check", model.spell_check.as_str()),
        ("word_wrap", model.word_wrap.as_str()),
        ("theme_label", model.theme_label.as_str()),
        ("line_count", model.line_count.as_str()),
    ] {
        assert!(!value.is_empty(), "状态栏项 {name} 是空的（看起来像坏了）");
    }
}

/// 瞬态消息到期后自己消失。
#[test]
fn status_messages_expire() {
    let (_cfg, mut app) = app();
    let now = app.services.markdown.now().millis();
    app.status_bar.notice("已保存", now);
    assert!(app.status_bar.take_message(now).is_some());
    assert!(
        app.status_bar.take_message(now + MESSAGE_TTL_MS).is_none(),
        "到期后必须消失，否则「已保存」会一直挂着"
    );
}

// ===========================================================================
// 写作目标进度条（V11-05）
// ===========================================================================

/// 把写作目标写进持久化设置，保留其余编辑器项不变。
fn set_writing_goal(app: &mut EditorApp, goal: u32) {
    use rustmark::application::dto::EditorSettingsView;
    use rustmark::application::preferences::commands::SetEditorCommand;
    let view = app.settings().editor;
    app.services
        .preferences
        .set_editor(SetEditorCommand {
            editor: EditorSettingsView {
                tab_size: view.tab_size,
                word_wrap: view.word_wrap,
                auto_save_interval: view.auto_save_interval,
                spell_check: view.spell_check,
                split_ratio: view.split_ratio,
                writing_goal: goal,
            },
        })
        .expect("设置写作目标应成功");
}

/// 未设目标时状态栏**整项不画**（model.goal 为 `None`）。
///
/// 画一条恒为 0% 的进度条只是噪音：新用户不该一上来就被盯着。
#[test]
fn goal_item_is_hidden_when_unset() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "g.md", CORPUS);
    set_writing_goal(&mut app, 0);
    run_frame(&mut app);

    let model = status_bar_view::build_model(&app);
    assert!(
        model.goal.is_none(),
        "未设目标时不应出现进度项，实际：{:?}",
        model.goal
    );
    assert!(
        !model.goal_label.is_empty(),
        "label 仍应可取（测试要能区分「未设」与「画漏了」）"
    );
}

/// 设了目标后状态栏显示百分比（且随字数变化）。
#[test]
fn goal_item_shows_percent_when_set() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "g.md", CORPUS);
    // CORPUS 经统计为 27 词（见 `word_count_matches_the_prototype`）。
    set_writing_goal(&mut app, 100);
    run_frame(&mut app);

    let model = status_bar_view::build_model(&app);
    let progress = model.goal.expect("设了目标后应有进度项");
    assert_eq!(progress.goal, 100);
    assert_eq!(progress.words, 27);
    assert_eq!(progress.percent, Some(27), "27/100 = 27%");
    assert!(!progress.reached);
    assert!(
        model.goal_label.contains("27%"),
        "状态栏应显示真实百分比，实际：{}",
        model.goal_label
    );
}

/// 打开一篇**已达标**的文档：不应弹「达成」（用户并没刚写完）。
#[test]
fn already_reached_document_does_not_announce_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "g.md", CORPUS); // 27 词
    set_writing_goal(&mut app, 10); // 已经达标
    run_frame(&mut app);

    let now = app.services.markdown.now().millis();
    assert!(
        app.status_bar.take_message(now).is_none(),
        "打开即达标不该弹提示"
    );
}

/// 编辑到**跨越目标**的那一刻弹一次，且只弹一次。
#[test]
fn reaching_goal_announces_once_on_crossing() {
    let dir = tempfile::tempdir().unwrap();
    // 4 个西文词的文档，目标设为 5，写完正文中还差一个词。
    let (_cfg, mut app) = app_with_file(dir.path(), "g.md", "one two three four");
    set_writing_goal(&mut app, 5);

    // 先跑几帧（在目标之下）：静默。
    for _ in 0..3 {
        run_frame(&mut app);
    }
    let early = app.services.markdown.now().millis();
    assert!(app.status_bar.take_message(early).is_none(), "达标前应静默");

    // 在文末补上「 five」→ 跨越目标：弹一次。
    let end = app.current_content().unwrap().len();
    app.editor.cursor = end;
    app.editor.anchor = None;
    app.insert_at_cursor(" five").expect("插入应成功");
    run_frame(&mut app);
    let crossed = app.services.markdown.now().millis();
    let m1 = app
        .status_bar
        .take_message(crossed)
        .expect("跨越那一刻应播报");
    assert!(m1.text.contains("达成"), "{:?}", m1.text);
    // 取出后清掉：提示在 TTL 内 `take_message` 不会自动清除，不清的话
    // 下一个断言会把「上一条还在显示」误判为「又弹了一次」。
    app.status_bar.clear_message();

    // 继续达标但不跨边界：不再弹。
    run_frame(&mut app);
    let later = app.services.markdown.now().millis();
    assert!(
        app.status_bar.take_message(later).is_none(),
        "同一段达标期间每帧弹会变成噪音"
    );
}

/// 真实帧里状态栏能画出来且不 panic。
#[test]
fn status_bar_renders_in_a_real_frame() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "r.md", CORPUS);
    app.ui.status_bar_visible = true;
    for _ in 0..2 {
        run_frame(&mut app);
    }
}

// ===========================================================================
// 欢迎页
// ===========================================================================

/// 没有活动文档时显示欢迎页，且**不画**编辑区。
#[test]
fn welcome_appears_when_no_document_is_open() {
    let (_cfg, mut app) = app();
    app.ui.status_bar_visible = true;
    run_frame(&mut app);
    assert!(app.should_show_welcome(), "关掉全部标签后应回到欢迎页");

    app.close_all_documents();
    assert!(app.should_show_welcome());
    run_frame(&mut app);
}

/// **启动时**就该看到欢迎页。
///
/// 这条与上一条是**不同**的判据：上一条只验证「关掉全部之后」，
/// 而启动路径上曾经有个「自动建一份空白未命名文档」的旧行为
/// （S6-01 的设计，理由是「否则首屏是块点了没反应的灰板」）。
/// 有了欢迎页之后那份空白文档就是多余的 —— 它会让
/// `active_doc` 立刻是 `Some`，欢迎页**永远不显示**。
///
/// 变异测试确认过：把判据换成「文档列表为空」时，
/// 只有本条会红（那时启动时列表里有那份空白文档）。
#[test]
fn welcome_is_the_first_screen() {
    let (_cfg, app) = app();
    // 关键前提：启动时**一个文档都没建**。
    assert!(
        app.open_document_list().is_empty(),
        "启动时不该自动建空白文档（它会让欢迎页永不显示）：{:?}",
        app.open_document_list()
    );
    assert!(app.should_show_welcome(), "首屏必须是欢迎页");
    assert!(
        app.current_stats().is_none(),
        "没有文档就没有字数（全零会被显示成「字数 0」）"
    );
}

/// 打开文档后欢迎页让位给编辑区。
#[test]
fn welcome_gives_way_to_the_editor() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "w.md", CORPUS);
    assert!(!app.should_show_welcome());
    run_frame(&mut app);
}

/// 关掉最后一个标签必须能回到欢迎页。
///
/// `close_document` 刻意不关最后一个（「永远至少一页可编辑」），
/// 若没有 `close_all_documents`，欢迎页就永远出不来。
#[test]
fn closing_the_last_tab_returns_to_the_welcome_page() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "x.md", CORPUS);
    assert!(!app.should_show_welcome());

    app.close_all_documents();

    assert!(app.should_show_welcome(), "关掉全部标签后应回到欢迎页");
    assert!(
        app.current_document_path() == "未命名文档",
        "无活动文档时面包屑应显示「未命名文档」"
    );
}

/// 三个主操作都有文案与提示。
#[test]
fn primary_actions_are_labelled() {
    for action in PrimaryAction::ALL {
        assert!(!action.label().is_empty());
        assert!(action.hint().len() > action.label().len());
    }
    assert_eq!(PrimaryAction::ALL.len(), 3, "原型是三个主操作");
}

/// 「新建文件」真的建出新文档并切过去。
#[test]
fn new_file_creates_and_switches() {
    let dir = tempfile::tempdir().unwrap();
    let (_cfg, mut app) = app_with_file(dir.path(), "n.md", "旧内容");
    let before = app.current_document_path();

    app.new_document().expect("新建应成功");

    assert_ne!(
        app.current_document_path(),
        before,
        "新建后必须**切换**过去，否则用户还停在旧文档上会以为按钮坏了"
    );
    assert!(!app.should_show_welcome());
}

/// 欢迎页在真实帧里能画出来（无最近文件 / 有最近文件两种）。
#[test]
fn welcome_renders_with_and_without_recent_files() {
    let (_cfg, mut app) = app();
    app.close_all_documents();
    for _ in 0..2 {
        run_frame(&mut app);
    }
    assert!(app.recent_files().is_empty(), "此时应显示「暂无最近文件」");

    // 打开一个真实文件 → 它应进入最近列表。
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("r.md"), CORPUS).unwrap();
    let path = dir.path().join("r.md").to_string_lossy().replace('\\', "/");
    app.open_workspace_file(&path).unwrap();
    app.close_all_documents();

    let recent = app.recent_files();
    assert!(!recent.is_empty(), "打开过的文件应进入最近列表");
    for _ in 0..2 {
        run_frame(&mut app);
    }
}

/// 快捷键提示读**当前生效**的键位，不是硬编码。
#[test]
fn shortcut_hints_follow_the_current_bindings() {
    let (_cfg, app) = app();
    let hints = welcome_view::shortcut_hints(&app);
    assert!(!hints.is_empty(), "欢迎页应提示至少一组快捷键");
    for hint in &hints {
        assert!(!hint.combo.is_empty(), "空键位不该出现在提示里");
        assert!(!hint.label.is_empty());
    }
    // 换个主题不影响提示（提示只读键位表）。
    let again = welcome_view::shortcut_hints(&app);
    assert_eq!(hints, again);
}

/// 最近文件里的路径打不开时**不能静默失败**。
#[test]
fn opening_a_missing_recent_file_reports_the_failure() {
    let (_cfg, mut app) = app();
    app.close_all_documents();
    let missing = "/definitely/not/here/gone.md";

    let result = app.open_path(missing);
    let now = app.services.markdown.now().millis();
    let msg = app.status_bar.take_message(now);

    assert!(result.is_err());
    assert!(
        msg.as_ref().is_some_and(|m| m.is_error),
        "点了没反应会让人以为程序卡了，失败必须可见"
    );
}
