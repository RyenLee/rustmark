//! 阶段 07 · 端到端用例集（S7-01，可自动化部分）。
//!
//! 每条用例都以 `tempfile::TempDir` 起一个**全新配置目录**，满足
//! S7-01「干净环境（全新配置目录）下从零执行通过」的验收。
//! 用例清单与判定方式见 `docs/archive/stage-reports/e2e-cases.md`。
//!
//! 仅收「可脱离真实窗口驱动 `EditorApp` 逻辑」的用例；
//! 必须看屏幕 / IME / 像素 / 跨平台的进 `docs/manual-qa-*.md`（S7-05）。

#[path = "../support.rs"]
mod support;

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::dto::ExportRequestView;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::find_replace_ui_state::FindOptions;
use std::sync::Arc;

/// 造一个装好默认配置、运行在独立临时目录里的应用根状态。
fn app() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

// ————————————————————————————————————————————————————————————
// FUNC-P0-11 新建 / 打开 / 保存
// ————————————————————————————————————————————————————————————

#[test]
fn e2e_new_document_insert_content() {
    let (_dir, mut app) = app();
    app.new_document().expect("新建应成功");
    app.insert_at_cursor("# 标题\n正文").expect("插入应成功");
    assert_eq!(app.open_document_list().len(), 1, "应只有一个打开的文档");
    assert_eq!(app.current_content().unwrap(), "# 标题\n正文");
}

#[test]
fn e2e_save_to_path_reopen_identical() {
    let (dir, mut app) = app();
    app.new_document().expect("新建应成功");
    app.insert_at_cursor("# 标题\n正文").expect("插入应成功");
    let path = dir.path().join("note.md");
    app.save_document_to(path.to_str().unwrap())
        .expect("保存应成功");
    app.open_path(path.to_str().unwrap()).expect("重开应成功");
    assert_eq!(
        app.current_content().unwrap(),
        "# 标题\n正文",
        "重开后内容应逐字节一致"
    );
    assert!(!app.current_document_dirty(), "重开后不应是脏文档");
}

#[test]
fn e2e_open_existing_file() {
    let (dir, mut app) = app();
    let path = dir.path().join("existing.md");
    std::fs::write(&path, "# 已存在\n内容").expect("应可写磁盘文件");
    app.open_path(path.to_str().unwrap()).expect("打开应成功");
    assert_eq!(app.current_content().unwrap(), "# 已存在\n内容");
    // 应用会把路径归一化为正斜杠（FilePath::as_str），故只比对尾部。
    assert!(app.current_document_path().ends_with("existing.md"));
}

#[test]
fn e2e_unnamed_save_prompts_save_as() {
    let (_dir, mut app) = app();
    app.new_document().expect("新建应成功");
    app.insert_at_cursor("x").expect("插入应成功");
    // 未命名文档直接保存应被拒绝（提示先指定路径），而非静默落盘到不可见位置。
    let err = app.save_document(false);
    assert!(err.is_err(), "未命名文档直接保存应被拒绝");
    assert_eq!(app.suggested_save_name(), "未命名.md");
}

#[test]
fn e2e_multi_tab_and_welcome() {
    let (_dir, mut app) = app();
    app.new_document().expect("新建 1");
    app.insert_at_cursor("doc1").expect("插入 1");
    app.new_document().expect("新建 2");
    app.insert_at_cursor("doc2").expect("插入 2");
    assert_eq!(app.open_document_list().len(), 2, "应有两个标签");
    app.close_all_documents();
    assert!(app.should_show_welcome(), "关闭全部标签后应回到欢迎页");
}

#[test]
fn e2e_dirty_tracking() {
    let (dir, mut app) = app();
    app.new_document().expect("新建应成功");
    assert!(!app.current_document_dirty(), "新建空白文档不应是脏的");
    app.insert_at_cursor("x").expect("插入应成功");
    assert!(app.current_document_dirty(), "编辑后应标记为脏");
    let path = dir.path().join("saved.md");
    app.save_document_to(path.to_str().unwrap())
        .expect("保存应成功");
    assert!(!app.current_document_dirty(), "保存后不应是脏的");
}

// ————————————————————————————————————————————————————————————
// FUNC-P0-01 语法高亮（E2E 仅验内容经高亮路径不损坏；高亮结构见 S6-04）
// ————————————————————————————————————————————————————————————

#[test]
fn e2e_highlight_preserves_content() {
    let (_dir, mut app) = app();
    app.new_document().expect("新建应成功");
    let src = "# H1\n\n- 列表项\n\n```rust\nlet x = 1;\n```\n\n> 引用\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    app.insert_at_cursor(src).expect("插入应成功");
    // 高亮是排版层，绝不得改动业务文本。
    assert_eq!(app.current_content().unwrap(), src);
}

// ————————————————————————————————————————————————————————————
// FUNC-P0-02 撤销 / 重做
// ————————————————————————————————————————————————————————————

#[test]
fn e2e_undo_redo_round_trip() {
    let (_dir, mut app) = app();
    app.new_document().expect("新建应成功");
    app.insert_at_cursor("hello").expect("插入");
    let base = app.current_content().unwrap();
    assert_eq!(base, "hello");
    // 用与「插入」不同种类的操作（缩进 = Replace 类）以避开测试时钟下的同类合并：
    // 测试时钟恒为 0，连续 Insert 会被撤销栈合并成一条，无法逐步撤销。
    app.indent_selection();
    let formatted = app.current_content().unwrap();
    assert_ne!(formatted, base, "缩进应改变内容");
    app.undo_edit();
    assert_eq!(app.current_content().unwrap(), base, "撤销应回到缩进前");
    app.redo_edit();
    assert_eq!(
        app.current_content().unwrap(),
        formatted,
        "重做应回到缩进后"
    );
}

#[test]
fn e2e_continuous_undo_redo() {
    let (_dir, mut app) = app();
    app.new_document().expect("新建应成功");
    app.insert_at_cursor("a").expect("插入 a");
    let after_insert = app.current_content().unwrap();
    assert_eq!(after_insert, "a");
    app.indent_selection();
    let after_indent = app.current_content().unwrap();
    assert_ne!(after_indent, after_insert, "缩进应改变内容");
    app.undo_edit();
    assert_eq!(app.current_content().unwrap(), after_insert, "撤销缩进");
    app.undo_edit();
    assert_eq!(app.current_content().unwrap(), "", "撤销插入回到空文档");
    app.redo_edit();
    assert_eq!(app.current_content().unwrap(), after_insert, "重做插入");
    app.redo_edit();
    assert_eq!(app.current_content().unwrap(), after_indent, "重做缩进");
}

// ————————————————————————————————————————————————————————————
// FUNC-P0-03 查找 / 替换（含正则）
// ————————————————————————————————————————————————————————————

fn seeded_app(text: &str) -> (tempfile::TempDir, EditorApp) {
    let (dir, mut app) = app();
    app.new_document().expect("新建应成功");
    app.insert_at_cursor(text).expect("插入应成功");
    (dir, app)
}

#[test]
fn e2e_find_replace_all_literal() {
    let (_dir, mut app) = seeded_app("foo bar foo foo");
    app.find.find_text = "foo".to_string();
    app.find.replace_text = "baz".to_string();
    app.find.options = FindOptions {
        case_sensitive: true,
        regex: false,
        whole_word: false,
    };
    app.sync_find();
    assert_eq!(app.find.match_count(), 3, "字面量 foo 应命中 3 处");
    app.find_replace_all();
    assert_eq!(app.current_content().unwrap(), "baz bar baz baz");
}

#[test]
fn e2e_find_replace_all_regex() {
    let (_dir, mut app) = seeded_app("abc adc aec");
    app.find.find_text = "a.c".to_string();
    app.find.replace_text = "X".to_string();
    app.find.options = FindOptions {
        case_sensitive: false,
        regex: true,
        whole_word: false,
    };
    app.sync_find();
    app.find_replace_all();
    assert_eq!(app.current_content().unwrap(), "X X X");
}

#[test]
fn e2e_find_whole_word() {
    let (_dir, mut app) = seeded_app("this is test");
    app.find.find_text = "is".to_string();
    app.find.options = FindOptions {
        case_sensitive: true,
        regex: false,
        whole_word: true,
    };
    app.sync_find();
    // "is" 在 "this" 中是子串、不应命中；仅独立的 "is" 命中一次。
    assert_eq!(
        app.find.match_count(),
        1,
        "全字匹配 is 只应命中独立的 is（不计 this）"
    );
}

// ————————————————————————————————————————————————————————————
// FUNC-P0-12 导出 HTML / Markdown
// ————————————————————————————————————————————————————————————

#[test]
fn e2e_export_html_file_created() {
    let (dir, mut app) = app();
    app.new_document().expect("新建应成功");
    app.insert_at_cursor("# 标题\n正文").expect("插入应成功");
    let out = dir.path().join("out.html");
    let req = ExportRequestView {
        format: "html".to_string(),
        include_toc: false,
        embed_images: false,
        theme: "github".to_string(),
        page_size: "a4".to_string(),
        paginate: false,
    };
    let result = app
        .export_current_document(req, out.to_str().unwrap())
        .expect("导出应成功");
    assert!(result.bytes > 0, "导出字节数应大于 0");
    let html = std::fs::read_to_string(&out).expect("应可读取导出文件");
    assert!(html.contains("标题"), "导出 HTML 应包含源标题文本");
}

#[test]
fn e2e_export_markdown_file_created() {
    let (dir, mut app) = app();
    app.new_document().expect("新建应成功");
    app.insert_at_cursor("# 标题\n正文").expect("插入应成功");
    let out = dir.path().join("out.md");
    let req = ExportRequestView {
        format: "markdown".to_string(),
        include_toc: false,
        embed_images: false,
        theme: "github".to_string(),
        page_size: "a4".to_string(),
        paginate: false,
    };
    app.export_current_document(req, out.to_str().unwrap())
        .expect("导出应成功");
    let md = std::fs::read_to_string(&out).expect("应可读取导出文件");
    assert!(
        md.contains("# 标题") && md.contains("正文"),
        "导出 Markdown 应保留源内容"
    );
}

// ————————————————————————————————————————————————————————————
// FUNC-P0-13 快捷键系统（端到端接线冒烟；三入口同源见 S6-09）
// ————————————————————————————————————————————————————————————

#[test]
fn e2e_shortcut_system_wired() {
    let (_dir, app) = app();
    // 核心动作在分发器注册表里都有键位显示——菜单 / 命令面板 / 一览三处同源。
    for action in [
        UiAction::Save,
        UiAction::Undo,
        UiAction::Find,
        UiAction::TogglePreview,
    ] {
        assert!(
            !app.dispatcher().registry().display_for(action).is_empty(),
            "动作 {:?} 应有键位显示",
            action
        );
    }
}

// ————————————————————————————————————————————————————————————
// FR-06 大文件降级（>500KB 仅高亮可视区，不崩溃）
// ————————————————————————————————————————————————————————————

#[test]
fn e2e_large_document_no_crash() {
    let (dir, mut app) = app();
    let big = "x".repeat(510 * 1024); // >500KB 触发降级路径
    app.new_document().expect("新建应成功");
    app.insert_at_cursor(&big).expect("插入应成功");
    let content = app.current_content().expect("应可读内容");
    assert_eq!(content.len(), big.len(), "大文件内容应完整不丢字节");
    assert!(app.current_line_count() >= 1);
    let path = dir.path().join("large.md");
    app.save_document_to(path.to_str().unwrap())
        .expect("保存应成功");
    app.open_path(path.to_str().unwrap()).expect("重开应成功");
    assert_eq!(
        app.current_content().unwrap().len(),
        big.len(),
        "重开后仍应完整"
    );
}
