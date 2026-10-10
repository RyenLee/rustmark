//! S7-03 原型缺陷回归 · 自动化断言部分。
//!
//! 对应 `docs/regression-checklist.md` 的 **B 清单**。
//! 覆盖要求「≥6 项转自动化」：B-05 / B-06 / B-07（撤销三类）+ B-01 / B-04（快捷键两类）
//! + B-09（sanitize）+ B-14（TOC 转义），共 8 项。
//!
//! B-02（默认表零冲突）一并纳入，证明全表无冲突注册。

use crate::support::text_buffer::buffer_of;
use rustmark::domain::document::aggregate::Document;
use rustmark::domain::export::value_objects::TocEntry;
use rustmark::domain::preferences::value_objects::{
    ActionId, ConflictKind, KeyCombo, ShortcutBinding, ShortcutTable,
};
use rustmark::infrastructure::export::html_sanitize::sanitize_html;
use rustmark::infrastructure::export::pulldown_html_exporter::render_toc;
use rustmark::shared::geometry::{ByteOffset, TextRange};
use rustmark::shared::time::Timestamp;

const T0: Timestamp = Timestamp::from_millis(1_000);

fn doc(text: &str) -> Document {
    Document::from_content(buffer_of(""), text, T0).expect("构造文档失败")
}

// ---------------------------------------------------------------------------
// B-05 / B-06 / B-07 撤销栈（per-Document，根治原型跨文件污染）
// ---------------------------------------------------------------------------

/// 原型场景：在 A 打字 → 切到 B 打字 → 撤销 B，不得改动 A。
#[test]
fn reg_undo_is_scoped_per_document() {
    let mut a = doc("AAA");
    let mut b = doc("BBB");
    a.insert_at(ByteOffset::new(3), "1", T0).unwrap();
    b.insert_at(ByteOffset::new(3), "2", T0).unwrap();

    b.undo(Timestamp::from_millis(2_000)).unwrap();

    assert_eq!(b.text(), "BBB", "B 回到自己的历史");
    assert_eq!(a.text(), "AAA1", "A 的内容绝不能被 B 的撤销影响");
    assert_eq!(a.undo_depth(), 1, "A 的撤销栈不受 B 影响");
}

/// 原型场景：打开 B 后的第一笔输入不得与 A 的最后一次提交合并（全局 lastCommit 缺陷）。
#[test]
fn reg_merge_window_is_per_document() {
    let mut a = doc("");
    let mut b = doc("");
    a.insert_at(ByteOffset::new(0), "a", Timestamp::from_millis(1_000))
        .unwrap();
    b.insert_at(ByteOffset::new(0), "b", Timestamp::from_millis(1_100))
        .unwrap();

    // 各自独立成撤销点（合并窗口是 per-Document 的，不会跨文档误合并）。
    assert_eq!(a.undo_depth(), 1);
    assert_eq!(b.undo_depth(), 1);

    b.undo(Timestamp::from_millis(1_200)).unwrap();
    assert_eq!(b.text(), "");
    assert_eq!(a.text(), "a", "A 不受 B 撤销影响");
}

/// 原型缺陷：undo/redo 不恢复选区，光标被扔到末尾 / 折叠。
#[test]
fn reg_undo_restores_selection() {
    let mut document = doc("hello world");
    let selection = TextRange::new(ByteOffset::new(0), ByteOffset::new(5));
    document.select(selection);
    document.insert("HI", T0).unwrap();
    assert_eq!(document.text(), "HI world");

    document.undo(Timestamp::from_millis(2_000)).unwrap();
    assert_eq!(document.text(), "hello world");
    assert_eq!(
        document.cursor().selection(),
        Some(selection),
        "撤销必须恢复当时的选区"
    );
}

// ---------------------------------------------------------------------------
// B-01 / B-04 快捷键（功能键不被拒 + 冲突检测）
// ---------------------------------------------------------------------------

/// 原型缺陷 B-01：`F8` 无修饰键单键，匹配器拒绝 → 永不触发。
/// Rust 根治：功能键不判为文本输入冲突（`clashes_with_text_input` 只认字符键）。
#[test]
fn reg_function_key_binding_not_flagged_as_text_input_clash() {
    let table = ShortcutTable::new(vec![ShortcutBinding::new(
        ActionId::FocusMode,
        KeyCombo::parse("F8").expect("F8 应可解析"),
    )]);
    let conflicts = table.conflicts();
    assert!(
        conflicts.is_empty(),
        "F8 单键绑定不应产生任何冲突（含 TextInputClash），实际：{conflicts:?}"
    );
}

/// 原型缺陷 B-04：无冲突检测，先注册者静默吞掉后者。
/// Rust 根治：`ShortcutTable::conflicts` 检测重复绑定并返回阻断级 `Duplicate`。
#[test]
fn reg_duplicate_binding_detected_as_blocking_conflict() {
    let table = ShortcutTable::new(vec![
        ShortcutBinding::new(ActionId::Undo, KeyCombo::parse("Ctrl+Z").expect("解析")),
        ShortcutBinding::new(ActionId::Redo, KeyCombo::parse("Ctrl+Z").expect("解析")),
    ]);
    let conflicts = table.conflicts();
    assert!(
        conflicts
            .iter()
            .any(|c| c.kind() == ConflictKind::Duplicate && c.kind().is_blocking()),
        "同一组合绑定到两个动作必须报阻断级 Duplicate 冲突，实际：{conflicts:?}"
    );
}

/// B-02 关联：默认快捷键表必须两两不冲突（注册遗漏在编译期不可发生，且运行期零冲突）。
#[test]
fn reg_default_bindings_have_no_conflict() {
    let table = ShortcutTable::default_bindings().expect("默认表应可构造");
    let has_blocking = table.conflicts().iter().any(|c| c.kind().is_blocking());
    assert!(!has_blocking, "默认快捷键表不得含阻断级冲突");
}

// ---------------------------------------------------------------------------
// B-09 sanitize（递归清洗 + DROP_TAGS）
// ---------------------------------------------------------------------------

/// 原型真实绕过路径：`<svg><script>` 必须整棵子树丢弃。
#[test]
fn reg_sanitize_drops_svg_subtree_and_resanitizes_unwrapped() {
    let out = sanitize_html("<svg><script>alert(1)</script></svg>");
    assert_eq!(out, "", "svg 子树必须整体丢弃，实际：{out}");
    assert!(!out.contains("script"));

    // 未知标签拆壳后子节点仍须清洗：`<foo><img onerror>` 不得残留 onerror。
    let unwrapped = sanitize_html("<foo><img src=x onerror=alert(1)></foo>");
    assert!(
        !unwrapped.contains("onerror"),
        "拆壳后的子节点必须被重新清洗，实际：{unwrapped}"
    );
}

// ---------------------------------------------------------------------------
// B-14 导出 TOC 转义（先转义再清洗，根治注入入口）
// ---------------------------------------------------------------------------

/// 原型缺陷：导出 TOC 未转义且拼接在清洗之后 → 标题含 `<script>` 形成注入。
/// Rust 根治：TOC 在拼接时由 `TocBuilder::escape_title` 转义，再随正文统一清洗。
#[test]
fn reg_toc_title_is_escaped_before_sanitize() {
    let toc = render_toc(&[TocEntry::new(2, "<script>alert(1)</script>")]);
    assert!(
        toc.contains("&lt;script&gt;"),
        "TOC 标题必须被转义，实际：{toc}"
    );
    assert!(
        !toc.contains("<script"),
        "转义后不得残留原始 <script 标签，实际：{toc}"
    );
}
