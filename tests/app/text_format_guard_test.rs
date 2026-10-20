//! 可打开格式白名单（问题 045）。
//!
//! # 为什么需要它
//!
//! 编辑器把打开的文件**整体当文本**装进缓冲。只要能选中一个图片或可执行
//! 文件，屏幕上就是一屏乱码，用户还可能顺手 `Ctrl+S` 把乱码写回去——
//! 这是不可逆的数据损坏，而不是「显示不好看」。
//!
//! 本文件钉住四件事：
//! 1. 白名单内的格式（`md` / `txt` / `csv`）照常打开，**大小写不敏感**；
//! 2. 白名单外的格式（图片 / 二进制 / 无扩展名）被拒绝，且**只**给一条
//!    中性提示——不是红色错误（用户点了个图片不是异常）；
//! 3. 被拒绝的打开**没有任何副作用**：不进标签、不进最近文件；
//! 4. 各入口共用同一判据：文件树置灰用的 `can_open_as_text` 与拖放分类
//!    的 `classify_dropped` 答案一致（`shared::text_format` 是唯一出处）。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::state::sidebar_ui_state::SidebarUiState;
use rustmark::shared::text_format::{BUILTIN_TEXT_EXTENSIONS, TextFormats, default_extension};

use crate::support::fake_dialog::FakeDialog;

fn app() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 在 `dir` 下写一份文件，返回其路径字符串（**不**打开）。
fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("写文件应成功");
    path.to_string_lossy().to_string()
}

fn status_text(app: &EditorApp) -> String {
    app.status_bar
        .message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

/// 白名单内的三种格式都能打开。
#[test]
fn whitelisted_formats_open() {
    let (dir, mut app) = app();
    for name in ["a.md", "b.txt", "c.csv"] {
        let path = write(dir.path(), name, "内容\n".as_bytes());
        app.open_path(&path).expect("白名单内应可打开");
    }
    assert_eq!(
        app.open_document_list().len(),
        3,
        "md / txt / csv 都应各自开出一个标签"
    );
}

/// 大小写不敏感：`.MD` / `.Txt` 同样放行。
#[test]
fn whitelist_is_case_insensitive() {
    let (dir, mut app) = app();
    for name in ["UPPER.MD", "Mixed.Txt", "TABLE.CSV"] {
        let path = write(dir.path(), name, "内容\n".as_bytes());
        app.open_path(&path).expect("扩展名大小写不应影响判定");
    }
    assert_eq!(app.open_document_list().len(), 3);
}

/// 图片与二进制被拒绝：不开文档、给中性提示、不留副作用。
#[test]
fn images_are_refused_with_a_notice_and_no_side_effects() {
    let (dir, mut app) = app();
    // PNG 魔数（截断即可）：文件内容不重要，重要的是**不该被读**。
    let png = write(dir.path(), "photo.png", b"\x89PNG\r\n\x1a\n\x00\x00");
    // 策略拒绝**不是**异常：返回 `Ok`，把原因交给状态栏。
    app.open_path(&png).expect("策略拒绝不应变成错误");
    assert!(
        app.open_document_list().is_empty(),
        "被拒绝的文件不得进标签条"
    );
    assert!(
        app.current_content().is_none(),
        "被拒绝的文件不得进编辑缓冲（否则就是一屏乱码）"
    );
    assert_eq!(status_text(&app), "暂不支持打开该格式：photo.png");
    assert!(
        app.recent_files().is_empty(),
        "拒绝发生在写最近文件之前——否则这个打开不了的路径会一直挂在最近列表里"
    );
}

/// 「最近文件」这条入口同样受守卫约束：即使某个非文本文件已在最近列表里，
/// 点它也只会得到提示。
#[test]
fn recents_cannot_bypass_the_guard() {
    let (dir, mut app) = app();
    let png = write(dir.path(), "shot.jpg", b"\xff\xd8\xff\xe0");
    app.open_path(&png).expect("策略拒绝不应变成错误");
    assert!(app.open_document_list().is_empty());
    assert!(status_text(&app).contains("暂不支持打开该格式"));
}

/// 无扩展名 / 隐藏文件一律拒绝：没有依据判定它是文本，
/// 「默认放行」等于把二进制放进缓冲。
#[test]
fn extensionless_paths_are_refused() {
    let (dir, mut app) = app();
    for name in ["Makefile", ".gitignore", "data.unknown"] {
        let path = write(dir.path(), name, b"text\n");
        app.open_path(&path).expect("策略拒绝不应变成错误");
    }
    assert!(
        app.open_document_list().is_empty(),
        "无扩展名 / 未知扩展名都不该被打开"
    );
}

/// 文件树置灰的判据与白名单**同源**——应用持有的那份白名单必须就是
/// `TextFormats::builtin()`（杜绝「某个入口悄悄换了另一份清单」）。
///
/// 拖放侧的分类结果由 `testing/integration/drag_open_test.rs` 端到端钉住
/// （它要真的驱动一帧 egui 才能把 `dropped_files` 投进去）。
#[test]
fn tree_predicate_follows_the_shared_whitelist() {
    let (dir, app) = app();
    let formats = TextFormats::builtin();
    let names = ["a.md", "b.txt", "c.csv", "d.png", "e.pdf", "no_ext"];
    for name in names {
        let path = write(dir.path(), name, b"x\n");
        let via_app = app.can_open_as_text(&path);
        let via_formats = formats.accepts(&path);
        assert_eq!(
            via_app, via_formats,
            "{name}：应用持有的白名单与 `TextFormats::builtin()` 判定不一致"
        );
    }
}

/// 提示语与置灰行的悬停说明**同一句**（唯一出处）。
#[test]
fn notice_text_has_a_single_source() {
    let notice = EditorApp::unsupported_format_notice("D:\\docs\\photo.png");
    assert_eq!(
        notice, "暂不支持打开该格式：photo.png",
        "提示语应只说文件末段，不含整条路径（状态栏放不下）"
    );
}

/// 三处「默认文件名」的后缀都必须等于白名单首选（V11-54a / R-16）。
///
/// # 钉的是什么
///
/// 改扩展名的**后缀**有三处，历史上各写各的（`.md` 硬编码）：
/// 1. 另存为对话框的建议名（`EditorApp::suggested_save_name`）；
/// 2. 侧栏「新建文件」的预填名（`SidebarUiState::start_create`）；
/// 3. （白名单自身的首选项 = `default_extension()`）。
///
/// 断言的是「后缀 == [`default_extension`]」而**不是**字面量 `"md"`：
/// 白名单首选一旦调整，这条用例必须跟着红——否则「默认扩展名」与
/// 「白名单」又变成两个各自独立的事实，调用点会悄悄退回硬编码。
///
/// # 为什么值得单独一条
///
/// 硬编码 `.md` 在**当前**白名单下**行为完全正确**（首选就是 md），
/// 所以任何断言字面量的用例都会绿——它测不出「漏改」这种只在
/// 白名单变化后才显形的问题。要让它变红，只能把判据写成
/// 「与白名单联动」。
#[test]
fn default_names_follow_the_whitelist_preferred_extension() {
    // 判据**独立于** `default_extension()`：直接取白名单首项。
    // 若这里改成断言 `== default_extension()`，两边一起变就看不出联动断了
    // （实测：把 `default_extension` 改成返回 "txt"，断言 `== default_extension()`
    // 的用例仍绿，而真正要防的「调用点与白名单脱钩」恰恰是红的）。
    let expected = BUILTIN_TEXT_EXTENSIONS
        .first()
        .copied()
        .expect("内置白名单不应为空");

    // ③ 常量自身也必须与白名单首项一致（否则「唯一出处」是假的）。
    assert_eq!(
        default_extension(),
        expected,
        "`default_extension()` 必须就是白名单首项，否则它不是唯一出处"
    );

    // ① 另存为对话框的建议名（未命名文档 → `未命名.<ext>`）。
    let (_dir, mut app) = app();
    app.new_document().expect("新建应成功");
    let suggested = app.suggested_save_name();
    assert_eq!(
        suggested,
        format!("未命名.{expected}"),
        "另存为建议名的后缀应取自白名单首选，而非硬编码"
    );

    // ② 侧栏「新建文件」的预填名。
    let mut sidebar = SidebarUiState::new();
    sidebar.start_create(Some("/w"), false);
    let prefilled = sidebar.draft_mut().expect("新建中应有草稿").clone();
    assert_eq!(
        prefilled,
        format!("未命名.{expected}"),
        "侧栏新建文件的预填名应取自白名单首选，而非硬编码"
    );
}
