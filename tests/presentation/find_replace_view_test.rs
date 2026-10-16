//! 查找替换功能用例（S6-08 验收标准）。
//!
//! # 验收标准原文
//!
//! > 功能用例 ≥ 10 条：普通/正则/大小写/全字四种模式命中数正确、
//! > 全部替换后计数与文档实际替换数一致、无效正则给出错误提示而非崩溃、
//! > 跨文档切换后查找状态重置或保留（明确定义并写进文档）。
//!
//! # 分层
//!
//! 本文件测**从动作到文档**的整条通路（`execute` → 应用层 → 领域 → DTO），
//! 因为验收标准关心的是「用户按了按钮之后文档变成什么样」，
//! 而不是「某个纯函数返回了什么」。匹配器与状态机的细粒度用例
//! 已在 `ui_state_test.rs`（S6-08 前置批次）覆盖，此处不重复。
//!
//! 另有一组**真实帧**用例：跑 `ctx.run` + `main_view::show`，
//! 钉住三类**静默失效**——不 panic、不报错、界面正常、只是功能没反应：
//! 浮层没被渲染、面板没接管输入、焦点没落到输入框。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::{
    NewDocumentCommand, ReplaceRangeCommand, UndoCommand,
};
use rustmark::presentation::shortcuts::dispatcher::Dispatch;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::dialog_ui_state::Overlay;
use rustmark::presentation::state::find_replace_ui_state::{FIND_INPUT_ID, FindOptions};
use rustmark::presentation::views::overlays;
use rustmark::shared::geometry::{ByteOffset, TextRange};
use std::sync::Arc;

/// 四种模式的「期望命中数」表——验收标准点名的四种模式各一条用例。
///
/// 语料刻意包含全部边界：大小写对（`Aa` 开与关结果不同）、
/// 子串与整词（`cat` vs `concatenate`）、正则量词（`h+`）、
/// 多字节（中文 / emoji，保证偏移是**字节**而不是字符）。
const CORPUS: &str = "cat concat CAT dog 中文 cat 🐱 cat";

/// 造一个装好默认配置、装入 `content` 的应用。
fn app_with(content: &str) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("启动即开空白文档");
    set_content(&mut app, id, content);
    (dir, app)
}

/// 直接写领域内容并刷新 UI 副本（绕过 `TextEdit`）。
fn set_content(app: &mut EditorApp, id: rustmark::shared::ids::DocumentId, content: &str) {
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: content.to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
}

/// 设查询与选项，然后重扫。
fn search(app: &mut EditorApp, query: &str, options: FindOptions) {
    app.find.find_text = query.to_string();
    app.find.options = options;
    app.sync_find();
}

/// 读回文档当前内容。
fn content_of(app: &EditorApp) -> String {
    app.current_content().expect("应能读到活动文档内容")
}

// ============================================================================
// 四种模式的命中数
// ============================================================================

/// 普通模式（默认）：大小写不敏感的字面量匹配。
#[test]
fn plain_mode_counts_case_insensitive_literals() {
    let (_d, mut app) = app_with(CORPUS);
    search(&mut app, "cat", FindOptions::default());
    // `cat` 出现 3 次（首词 / `concat` 内 / 中文后 / emoji 后共 4 次？）——
    // 逐个数：`cat` / `concat` 内 / `CAT` / `中文 cat` / `🐱 cat` = 5。
    assert_eq!(app.find.match_count(), 5, "默认应大小写不敏感");
    assert_eq!(app.find.count_label(), "1/5");
}

/// 大小写敏感（`Aa`）：`CAT` 不再命中。
#[test]
fn case_sensitive_mode_narrows_the_count() {
    let (_d, mut app) = app_with(CORPUS);
    let options = FindOptions {
        case_sensitive: true,
        ..Default::default()
    };
    search(&mut app, "cat", options);
    assert_eq!(
        app.find.match_count(),
        4,
        "区分大小写后 `CAT` 应被排除（{CORPUS}）"
    );
    search(&mut app, "CAT", options);
    assert_eq!(app.find.match_count(), 1, "大写查询只命中大写处");
}

/// 全字匹配：`concat` 里的 `cat` 不再命中。
#[test]
fn whole_word_mode_excludes_substrings() {
    let (_d, mut app) = app_with(CORPUS);
    let options = FindOptions {
        whole_word: true,
        ..Default::default()
    };
    search(&mut app, "cat", options);
    assert_eq!(
        app.find.match_count(),
        4,
        "全字匹配下 `concat` 内的 `cat` 应被排除（{CORPUS}）"
    );
    // 语料里没有「四边都不是词字符」的孤立 cat（`中文 cat` 前是空格、
    // `🐱 cat` 前是空格，都算边界成立），故上面是 4 而非 3。
}

/// 正则模式：量词与字符类。
#[test]
fn regex_mode_matches_patterns() {
    let (_d, mut app) = app_with(CORPUS);
    let options = FindOptions {
        regex: true,
        ..Default::default()
    };
    // 正则同样受 `Aa` 约束：默认不区分大小写，`c.t` 应命中 `CAT`。
    // （S6-08 修复前这里只匹配到 1 个——`Aa` 开关对正则完全无效。）
    search(&mut app, "c.t", options);
    assert_eq!(
        app.find.match_count(),
        5,
        "`.` 匹配单字符 + 不区分大小写 → 含 `CAT` 在内共 5 处（{CORPUS}）"
    );

    search(&mut app, "ca+", options);
    assert_eq!(app.find.match_count(), 5, "`ca+` 应匹配全部 cat / concat");

    search(&mut app, "[🐱🐶]", options);
    assert_eq!(app.find.match_count(), 1, "字符类应匹配 emoji");

    // 端点反序必须在编译期报错：否则它会编译成「永不匹配」的类，
    // 用户看到的是「正则没写错却一个都搜不到」，比明确报错难查得多。
    search(&mut app, "[z-a]", options);
    let err = app
        .find
        .error
        .as_deref()
        .unwrap_or_else(|| panic!("`[z-a]` 端点反序应报错"));
    assert!(err.contains("反序"), "错误信息应指出端点反序：{err}");
}

/// 三开关正交：同时打开时命中数必须等于三者语义**相交**的结果。
///
/// 这条比「分别测四个」更重要：三个开关是三个独立布尔，
/// 任何两个的实现若共用了同一段代码（例如 `Aa` 与全字都去改
/// `case_sensitive`），单独测都绿、组合起来才暴露。
#[test]
fn the_three_switches_are_orthogonal() {
    let (_d, mut app) = app_with(CORPUS);
    let all = FindOptions {
        case_sensitive: true,
        regex: true,
        whole_word: true,
    };
    assert_eq!(all.active_count(), 3);
    // 正则 `[cC]at` + 全字 + Aa：`concat` 被全字排除；`CAT` 因 `Aa` 开启
    // 而不匹配（模式里 `at` 是小写字面量）。剩 3 处。
    //
    // 这条同时钉住「`Aa` 对正则有效」——修复前正则无视 `Aa`，
    // 同一模式会返回 4 或 5，断言立刻红。
    search(&mut app, "[cC]at", all);
    assert_eq!(
        app.find.match_count(),
        3,
        "三开关同开应取交集：全字排除 `concat`，`Aa` 排除 `CAT`"
    );

    // 只开全字、关掉 Aa：`CAT` 回来了（4 处），证明两个开关真的正交。
    search(
        &mut app,
        "[cC]at",
        FindOptions {
            regex: true,
            whole_word: true,
            ..Default::default()
        },
    );
    assert_eq!(
        app.find.match_count(),
        4,
        "关掉 `Aa` 后 `CAT` 应重新计入——否则说明两个开关共用了同一段逻辑"
    );
}

// ============================================================================
// 无效正则
// ============================================================================

/// 无效正则给出可见提示，**且不崩溃、不吞掉文档**。
#[test]
fn invalid_regex_shows_error_instead_of_panicking() {
    let (_d, mut app) = app_with(CORPUS);
    let options = FindOptions {
        regex: true,
        ..Default::default()
    };
    for bad in ["[unclosed", "(unclosed", "*nothing", "[z-a]", "a)b"] {
        search(&mut app, bad, options);
        let error = app
            .find
            .error
            .as_deref()
            .unwrap_or_else(|| panic!("`{bad}` 应给出错误提示"));
        assert!(!error.is_empty(), "错误提示不应为空串");
        assert_eq!(
            app.find.current_match(),
            None,
            "`{bad}` 模式无效时不得有选中命中"
        );
        assert_eq!(app.find.count_label(), "无结果");
    }
    // 关键：文档一个字都没动，且应用仍能继续查找。
    assert_eq!(content_of(&app), CORPUS, "无效正则不得改动文档");
    search(&mut app, "cat", FindOptions::default());
    assert_eq!(app.find.match_count(), 5, "修正模式后应恢复正常查找");
}

/// 无效正则**清掉**旧命中：否则「下一个」会跳到与输入框无关的位置。
#[test]
fn invalid_regex_clears_previous_matches() {
    let (_d, mut app) = app_with(CORPUS);
    search(&mut app, "cat", FindOptions::default());
    assert_eq!(app.find.match_count(), 5);

    search(
        &mut app,
        "[bad",
        FindOptions {
            regex: true,
            ..Default::default()
        },
    );
    assert!(
        app.find.finder.is_none(),
        "模式无效时必须丢弃旧 finder（其字节偏移仍指向旧模式）"
    );
    // 此时按「下一个」不应崩溃、也不应移动光标。
    let before = app.editor.cursor;
    app.find_step(true);
    assert_eq!(app.editor.cursor, before, "无有效命中时不得移动光标");
}

// ============================================================================
// 上一个 / 下一个（循环）
// ============================================================================

/// 「下一个」走到末尾后回到开头，「上一个」反向。
#[test]
fn next_and_previous_wrap_around_in_a_real_document() {
    let (_d, mut app) = app_with("x\n\nfoo\n\nfoo\n\nfoo");
    search(&mut app, "foo", FindOptions::default());
    assert_eq!(app.find.match_count(), 3);

    let seen: Vec<usize> = (0..4)
        .map(|_| {
            app.find_step(true);
            app.find.current_match().expect("应始终有命中").start
        })
        .collect();
    // 3 次命中循环 4 次：第 4 次回到第 1 个。
    // 打开面板时 `current` 已指向第 1 条（`cat` 在偏移 3），故第一次
    // 「下一个」去第 2 条——与原型 `nextMatch = gotoMatch(findIndex + 1)` 一致。
    assert_eq!(seen, vec![8, 13, 3, 8], "第 4 次「下一个」应绕回首个");

    let back: Vec<usize> = (0..3)
        .map(|_| {
            app.find_step(false);
            app.find.current_match().expect("应始终有命中").start
        })
        .collect();
    // 上一轮结束时停在第 2 条（偏移 8），故第一次「上一个」回到第 1 条（3），
    // 随后依次 13（循环到末尾）、8——严格逆向且循环。
    assert_eq!(back, vec![3, 13, 8], "反向应逆向并循环");
}

/// 「上一个」以**当前命中起点**为界，不会在同一命中上打转。
///
/// 若实现成「从 `start` 往前找」（而不是「从 `end` 往前」），
/// `next_after(m.start)` 会立刻又选中 `m` 自己——连点两次「下一个」
/// 看起来毫无变化。这条用例专门钉这个差。
#[test]
fn stepping_never_repeats_the_same_match() {
    let (_d, mut app) = app_with("aa aa aa");
    search(&mut app, "aa", FindOptions::default());
    assert_eq!(app.find.match_count(), 3);

    let mut visited = Vec::new();
    for _ in 0..3 {
        app.find_step(true);
        visited.push(app.find.current_match().expect("应始终有命中"));
    }
    assert_eq!(visited.len(), 3);
    for pair in visited.windows(2) {
        assert_ne!(pair[0], pair[1], "相邻两次「下一个」落在同一命中上");
    }
}

// ============================================================================
// 全部替换
// ============================================================================

/// 全部替换后，**面板计数与文档实际替换数一致**（验收标准核心）。
#[test]
fn replace_all_count_matches_actual_replacements() {
    let (_d, mut app) = app_with(CORPUS);
    search(&mut app, "cat", FindOptions::default());
    let expected = app.find.match_count();
    assert_eq!(expected, 5);

    app.find.replace_text = "猫".to_string();
    app.find_replace_all();

    let after = content_of(&app);
    assert_eq!(app.find.last_replace_count, Some(expected));
    assert!(!after.contains("cat") && !after.contains("CAT"), "{after}");
    // 「替换文本本身含模式」的极端情形也要能算对：这里替换文本是中文，
    // 计数必须等于扫描出来的 5。
    assert_eq!(
        after.matches('猫').count(),
        expected,
        "文档里实际出现的替换次数应与面板计数一致"
    );
    assert_eq!(app.find.match_count(), 0, "替换后应重新扫描且无残留命中");
}

/// 全部替换只产生**一个**撤销点。
///
/// 逐条替换会在撤销栈里留 N 个点，用户按一次 Ctrl+Z 只撤掉一处，
/// 必须按 N 次——而「全部替换」在用户心智里是**一个**动作。
///
/// 断言「撤**一次**就全回去」而不是「连撤到栈空后痕迹消失」：
/// 后者在两种实现下都成立（逐条也能撤干净），抓不到本缺陷。
#[test]
fn replace_all_is_a_single_undo_step() {
    let (_d, mut app) = app_with("a a a a a");
    search(&mut app, "a", FindOptions::default());
    app.find.replace_text = "b".to_string();
    app.find_replace_all();
    assert_eq!(content_of(&app), "b b b b b");

    let id = app.ensure_document().expect("活动文档");
    app.services
        .documents
        .undo(UndoCommand { id })
        .expect("应存在可撤销的操作");
    assert_eq!(
        content_of(&app),
        "a a a a a",
        "撤**一次**应回到替换前——若只撤掉一处，说明「全部替换」被实现成了 N 次编辑"
    );
}

/// 空替换文本 = 删除命中。
#[test]
fn replace_all_with_empty_replacement_deletes() {
    let (_d, mut app) = app_with("a-b-c");
    search(&mut app, "-", FindOptions::default());
    app.find.replace_text.clear();
    app.find_replace_all();
    assert_eq!(content_of(&app), "abc");
    assert_eq!(app.find.last_replace_count, Some(2));
}

/// 会「无限增生」的替换必须被标记（否则用户点到手酸而文档指数膨胀）。
#[test]
fn replace_all_flags_runaway_replacement() {
    let (_d, mut app) = app_with("a");
    search(&mut app, "a", FindOptions::default());
    app.find.replace_text = "aa".to_string();
    app.find_replace_all();
    assert!(
        app.find.last_replace_grows,
        "用 `aa` 替换 `a` 会无限增生，必须警告用户"
    );
    assert_eq!(content_of(&app), "aa");
}

/// 收敛方向的替换**不**报警（误报会让用户以为替换不干净）。
#[test]
fn replace_all_does_not_flag_converging_replacement() {
    let (_d, mut app) = app_with("aa");
    search(&mut app, "aa", FindOptions::default());
    app.find.replace_text = "a".to_string();
    app.find_replace_all();
    assert!(
        !app.find.last_replace_grows,
        "用 `a` 替换 `aa` 是收敛的，误报反而让人不敢用"
    );
    assert_eq!(content_of(&app), "a");
}

/// 替换单个：只动当前命中，其余保持不变。
#[test]
fn replace_current_touches_only_the_selected_match() {
    let (_d, mut app) = app_with("foo bar foo");
    search(&mut app, "foo", FindOptions::default());
    app.find.replace_text = "baz".to_string();
    app.find_replace_current();
    assert_eq!(content_of(&app), "baz bar foo", "只应替换当前这条");
}

/// 无命中时点「替换」不产生任何改动（按钮虽被禁用，
/// 但快捷键 / 命令面板仍可触发，用例覆盖那条路径）。
#[test]
fn replace_with_no_match_is_a_noop() {
    let (_d, mut app) = app_with("hello");
    search(&mut app, "zzz", FindOptions::default());
    app.find.replace_text = "x".to_string();
    app.find_replace_current();
    app.find_replace_all();
    assert_eq!(content_of(&app), "hello");
    assert_eq!(app.find.last_replace_count, None);
}

/// 多字节边界：命中区间是**字节**偏移，替换不得切坏 UTF-8。
#[test]
fn replace_handles_multibyte_boundaries() {
    let (_d, mut app) = app_with("中文中文 🐱猫");
    search(&mut app, "猫", FindOptions::default());
    assert_eq!(app.find.match_count(), 1);
    app.find.replace_text = "狗".to_string();
    app.find_replace_all();
    assert_eq!(content_of(&app), "中文中文 🐱狗");
}

// ============================================================================
// 跨文档策略
// ============================================================================

/// 跨文档切换：**保留**查询与选项，**清空**属于旧文档的命中。
///
/// 这是验收标准要求「明确定义」的那条策略，理由是：用户常在多个文件里
/// 找同一个词；但命中位置属于旧文档，带过去会跳到错误位置。
#[test]
fn switching_document_keeps_query_but_drops_stale_matches() {
    let (_d, mut app) = app_with(CORPUS);
    search(&mut app, "cat", FindOptions::default());
    assert_eq!(app.find.match_count(), 5);

    // 新建第二份文档（内容不同），再切过去。
    // 切换走 `execute(GotoTab2)` 而非直接调 `switch_document`：
    // 后者是 `pub(crate)`，且**用户走的本来就是这条路径**——
    // 直接调内部方法会让用例在「动作没接线」时照样绿。
    let second = app
        .services
        .documents
        .new_document(NewDocumentCommand)
        .expect("应能新建文档");
    set_content(&mut app, second, "cat and cat");
    app.execute(UiAction::GotoTab2);

    assert_eq!(app.find.find_text, "cat", "查询应保留");
    assert_eq!(app.find.options, FindOptions::default(), "选项应保留");
    assert_eq!(app.find.current, None, "命中选中应重置");
    app.sync_find();
    assert_eq!(
        app.find.match_count(),
        2,
        "新文档只有 2 处命中，旧的 5 处必须已丢弃"
    );
}

/// 配置为「不保留」时，跨文档应把查询与选项一并清空。
#[test]
fn switching_document_can_clear_everything_when_configured() {
    let (_d, mut app) = app_with(CORPUS);
    app.find.keep_query_across_documents = false;
    search(&mut app, "cat", FindOptions::default());
    app.find.replace_text = "x".to_string();

    let _second = app
        .services
        .documents
        .new_document(NewDocumentCommand)
        .expect("应能新建文档");
    app.execute(UiAction::GotoTab2);

    assert_eq!(app.find.find_text, "", "配置为不保留时应清空查询");
    assert_eq!(app.find.replace_text, "");
    assert_eq!(app.find.options, FindOptions::default());
    assert_eq!(app.find.match_count(), 0);
}

// ============================================================================
// 动作接线
// ============================================================================

/// `Ctrl+F` 打开面板，`Ctrl+H` 打开并**展开替换框**。
///
/// 这条单独立出来是因为它锁的是一个**已修的缺陷**：`Replace` 曾是
/// 「菜单里有、快捷键有、但没有任何浮层的 `opened_by` 指向它」的动作，
/// 于是 `execute` 落进 `_ => {}` 静默失效——不报错、界面正常、
/// 按 Ctrl+H 毫无反应。
#[test]
fn ctrl_f_and_ctrl_h_open_the_panel_with_the_right_shape() {
    let (_d, mut app) = app_with(CORPUS);

    app.execute(UiAction::Find);
    assert!(app.dialog.is_open(Overlay::Find), "Ctrl+F 应打开查找面板");
    assert!(!app.find.replace_open, "Ctrl+F 只开查找，不该展开替换框");
    assert!(app.find.find_focused, "打开后应把焦点交给查找框");

    // 关闭后再用 Ctrl+H。
    app.dialog.close(Overlay::Find);
    app.execute(UiAction::Replace);
    assert!(app.dialog.is_open(Overlay::Find));
    assert!(
        app.find.replace_open,
        "Ctrl+H 应展开替换框（否则「替换」这个动作名不副实）"
    );
    // 面板已开着时再按 Ctrl+F 不应把替换框收回去。
    app.execute(UiAction::Find);
    assert!(
        app.find.replace_open,
        "面板已开时 Ctrl+F 的意图是「回到查找框」，不该收起替换框"
    );
}

/// 每个有开启动作的动作都能打开某个浮层——`Replace` 当初就漏了。
///
/// 覆盖面刻意只取「声明式开启动作」的那几张表，而不是遍历 `UiAction::ALL`：
/// 后者会牵扯大量「本就无浮层」的动作（布局 / 主题 / 格式），
/// 断言边界反而模糊。这里要锁的是**两张表互为逆映射**这条不变量。
#[test]
fn overlay_opening_tables_are_mutually_inverse() {
    for overlay in Overlay::ALL.iter().copied() {
        for action in [overlay.opened_by(), overlay.also_opened_by()]
            .into_iter()
            .flatten()
        {
            let back = Overlay::ALL
                .iter()
                .copied()
                .find(|o| o.opened_by() == Some(action) || o.also_opened_by() == Some(action));
            assert_eq!(
                back,
                Some(overlay),
                "{action:?} 反查不到 {overlay:?}——该动作会落进 `_ => {{}}` 静默失效"
            );
        }
    }
    // 反过来：每个浮层至多被两个动作打开，且两个动作互不相同
    // （相同会让其中一个永远打不开）。
    for overlay in Overlay::ALL.iter().copied() {
        if let (Some(a), Some(b)) = (overlay.opened_by(), overlay.also_opened_by()) {
            assert_ne!(a, b, "{overlay:?} 的两个开启动作相同，后一个永远打不开");
        }
    }
}

/// 快捷键 `Ctrl+H` 真的能触发 `Replace`（走真实 `egui::Event::Key`）。
#[test]
fn ctrl_h_shortcut_triggers_replace() {
    let (_d, app) = app_with(CORPUS);
    let combo = app
        .dispatcher()
        .registry()
        .combo_for(UiAction::Replace)
        .expect("Replace 应有默认键位（它曾经没注册，按 Ctrl+H 毫无反应）");
    let event = crate::support::keymap::key_event(
        crate::support::keymap::egui_key_of(combo.key()),
        crate::support::keymap::egui_modifiers_of(combo.modifiers()),
    );
    assert_eq!(
        app.ui.dispatcher.handle(&event),
        Dispatch::Triggered(UiAction::Replace),
        "Ctrl+H 应分发到 Replace"
    );
}

// ============================================================================
// 浮层渲染通路（S6-07 静默失效的同源根治）
// ============================================================================

/// 清单外的每个浮层都**必须有渲染实现**。
///
/// 这条锁的是 S6-07 的根因：动作 → 浮层的映射存在，浮层 → 视图的映射
/// 缺失，于是动作被 `openable_overlay` 拦下后打开一个「没有视图的浮层」，
/// `execute` 直接 return——全程零报错。
///
/// 用「不在 [`overlays::PENDING_VIEWS`] 里」而不是「全部都有视图」：
/// S6-12 尚未落地命令面板的视图，若直接断言全覆盖，本批次就得
/// 带着一条红灯交付，而红灯会掩盖真正的回归。登记待办清单后，
/// 语义变成「**本批次承诺的必须做到；清单外的新增漏了立刻红**」。
#[test]
fn every_overlay_outside_the_pending_list_has_a_view() {
    for overlay in Overlay::ALL.iter().copied() {
        let pending = overlays::PENDING_VIEWS.contains(&overlay);
        assert_eq!(
            overlays::has_view(overlay),
            !pending,
            "{overlay:?}：有开启动作却没有渲染实现，且未登记进 \
             `overlays::PENDING_VIEWS`——该动作会静默失效"
        );
    }
    // 清单本身不许与 `has_view` 矛盾（否则上条断言恒真，等于没测）。
    for overlay in overlays::PENDING_VIEWS {
        assert!(
            !overlays::has_view(*overlay),
            "{overlay:?} 同时出现在待办清单且声称已有视图，二者必有一错"
        );
        assert!(
            Overlay::ALL.contains(overlay),
            "待办清单里的 {overlay:?} 不在 `Overlay::ALL` 中（可能已改名或删除）"
        );
    }
}

/// 查找面板是**非模态**浮层：编辑区仍可交互。
///
/// 若算作模态，`is_modal()` 会让分发器拦下所有「打开浮层」的动作，
/// 用户会发现「按了 Ctrl+F 之后整个编辑器都不能用了」——
/// 而原型里查找面板是浮在编辑区右上角的工具面板，编辑区始终可编辑。
#[test]
fn find_panel_is_not_modal() {
    let mut state = rustmark::presentation::state::dialog_ui_state::DialogUiState::new();
    state.open(Overlay::Find);
    assert!(state.any_open(), "面板确实打开了");
    assert!(
        !state.is_modal(),
        "查找面板必须非模态，否则编辑区会被禁用来"
    );
    // 模态浮层仍然模态。
    state.open(Overlay::Settings);
    assert!(state.is_modal());
    state.close(Overlay::Settings);
    assert!(!state.is_modal());
}

/// 查找输入框有焦点时，全局快捷键让位（`Esc` 除外）。
///
/// 症状：用户在查找框里按 `Ctrl+B` 想找字符串，文档里的字被加粗了。
/// 这条通过真实帧 + 真实按键事件验证，不靠「读代码确认」。
#[test]
fn global_shortcuts_yield_while_the_find_input_has_focus() {
    let (_d, mut app) = app_with("hello");
    let ctx = egui::Context::default();
    app.execute(UiAction::Find);
    frames(&mut app, &ctx, 2);

    // 面板的输入框应真的拿到了焦点（否则下面这条断言是空转）。
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(FIND_INPUT_ID))),
        "打开面板后查找框应获得焦点"
    );

    // 按 Ctrl+B：加粗必须**不**发生。
    let before = content_of(&app);
    send_key(&mut app, &ctx, egui::Key::B, true);
    frames(&mut app, &ctx, 2);
    assert_eq!(content_of(&app), before, "查找框聚焦时 Ctrl+B 不该加粗");

    // Esc 仍然放行（否则用户会被困在查找框里）。
    send_key(&mut app, &ctx, egui::Key::Escape, false);
    frames(&mut app, &ctx, 2);
    assert!(
        !app.dialog.is_open(Overlay::Find),
        "查找框聚焦时 Esc 仍应能关闭面板"
    );
}

// ============================================================================
// 真实帧
// ============================================================================

/// 跑一帧真实界面（视口 900×400）。
fn frame(app: &mut EditorApp, ctx: &egui::Context) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(900.0, 400.0),
        )),
        ..Default::default()
    };
    crate::support::run_ui(ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
}

fn frames(app: &mut EditorApp, ctx: &egui::Context, n: usize) {
    for _ in 0..n.max(2) {
        frame(app, ctx);
    }
}

/// 送一个真实按键事件，走**生产代码**的 `collect_actions` → `execute`。
///
/// 刻意不调 `app.ui.dispatcher.handle` 再自行判断「该不该让位」：
/// 那样测的是测试自己写的那份规则，与生产代码各改各的。变异测试证实过
/// 这一点——把生产代码里的让位逻辑整段删掉，用例仍然全绿。
///
/// `collect_actions` 因此是 `pub` 的（见其文档）。
fn send_key(app: &mut EditorApp, ctx: &egui::Context, key: egui::Key, ctrl: bool) {
    let modifiers = egui::Modifiers {
        command: ctrl,
        ..Default::default()
    };
    ctx.input_mut(|i| {
        i.events
            .push(crate::support::keymap::key_event(key, modifiers));
    });
    for action in app.collect_actions(ctx) {
        app.execute(action);
    }
    ctx.input_mut(|i| i.events.clear());
}

/// 真实帧下：打开面板 → 面板被渲染（控件真的被布局出来）。
///
/// 这条钉的是「浮层渲染通路」本身。少了 `overlays::show` 这一步，
/// 面板状态一切正常（`is_open` 为真、命中数正确），
/// 但界面上什么都不会出现——典型的静默失效。
#[test]
fn opening_the_panel_actually_renders_it_in_a_real_frame() {
    let (_d, mut app) = app_with(CORPUS);
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    assert!(!app.dialog.is_open(Overlay::Find), "初始不应有面板");

    app.execute(UiAction::Find);
    frames(&mut app, &ctx, 2);

    assert!(app.dialog.is_open(Overlay::Find));
    // 面板真的被画了：焦点落到了查找输入框上。
    // 若 `overlays::show` 缺失，控件从未被布局，焦点请求也就无人响应。
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(FIND_INPUT_ID))),
        "面板未渲染：查找框拿不到焦点（`overlays::show` 可能没接上）"
    );
}

/// 真实帧下：改查询 → 命中数立刻更新。
///
/// 钉「输入变化必须重扫」这条不变量。它坏掉时界面完全正常，
/// 只是计数与实际跳转对不上。
#[test]
fn changing_the_query_rescans_in_a_real_frame() {
    let (_d, mut app) = app_with(CORPUS);
    let ctx = egui::Context::default();
    app.execute(UiAction::Find);
    frames(&mut app, &ctx, 2);

    // 直接改状态（模拟用户在输入框里打字后的 `changed()` 分支已执行）。
    app.find.find_text = "cat".to_string();
    app.sync_find();
    frames(&mut app, &ctx, 2);
    assert_eq!(app.find.match_count(), 5);
    assert_eq!(app.find.count_label(), "1/5");

    // 打开正则并给一个无效模式：面板应显示错误而不是崩溃。
    app.find
        .toggle(rustmark::presentation::state::find_replace_ui_state::OptionKind::Regex);
    app.find.find_text = "[oops".to_string();
    app.sync_find();
    frames(&mut app, &ctx, 3);
    assert!(app.find.error.is_some(), "无效正则应显示错误");
    assert_eq!(content_of(&app), CORPUS, "界面必须还活着，文档未被改动");
}

/// 真实帧下：跳转到文档深处的命中，编辑区滚动位置必须变化。
///
/// 钉「跳转要滚进视口」——不滚的话用户看不到自己跳到了哪。
#[test]
fn stepping_to_a_far_match_scrolls_the_editor() {
    // 造一篇长文档，让最后一处命中远在视口之外。
    let mut doc = String::new();
    for i in 0..60 {
        doc.push_str(&format!("第 {i} 行 filler filler filler\n\n"));
    }
    doc.push_str("needle\n");
    let (_d, mut app) = app_with(&doc);
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 3);

    search(&mut app, "needle", FindOptions::default());
    assert_eq!(app.find.match_count(), 1);
    let before = app.editor.scroll_y;
    assert!(before.abs() < 1.0, "初始应在顶部：{before}");

    app.find_step(true);
    frames(&mut app, &ctx, 2);
    assert!(
        app.editor.scroll_y > 100.0,
        "跳到文档末尾的命中后应滚下去，实际 scroll_y = {}",
        app.editor.scroll_y
    );
    // 命中应被选中（用户接着要按 Ctrl+H 替换的就是它）。
    let (s, e) = app.editor.selection().expect("应有选区");
    assert_eq!(&doc[s..e], "needle");
}
