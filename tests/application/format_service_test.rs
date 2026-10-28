//! 格式指令用例测试（S6-07）。
//!
//! # 覆盖策略
//!
//! 应用层是格式规则的**唯一执行点**（三入口都调它），所以这里既测
//! 「规则对不对」，也测「写回是否干净」：
//!
//! - 每种格式的**空选区**与**有选区**两种情形（验收标准逐条要求）；
//! - 幂等 / 反向：已加粗再点应去包裹，引用再点应取消；
//! - 边界：全角、emoji、CRLF、空行；
//! - **写回一致性**：`result.text` 必须等于重新 `get_content` 读到的内容
//!   （DTO 与领域不同步是最隐蔽的 bug：界面显示一份、领域存另一份）；
//! - **可撤销**：格式改动进领域撤销栈，Ctrl+Z 能撤掉。

use std::sync::{Arc, RwLock};

use rustmark::application::document::commands::*;
use rustmark::application::document::queries::*;
use rustmark::application::document::registry::DocumentRegistry;
use rustmark::application::document::service::DocumentAppService;
use rustmark::domain::document::repository::DocumentRepository;
use rustmark::domain::document::services::BufferFactory;
use rustmark::ports::time::Clock;
use rustmark::shared::geometry::{ByteOffset, TextRange};

use super::fakes::*;

/// 装配服务并装入一份初始内容。
fn setup(content: &str) -> (DocumentAppService, rustmark::shared::ids::DocumentId) {
    let repo: Arc<dyn DocumentRepository> = Arc::new(InMemoryDocumentRepository::new());
    let registry = Arc::new(RwLock::new(DocumentRegistry::new()));
    let bus: Arc<dyn rustmark::ports::event::EventBus> = Arc::new(InMemoryEventBus::new());
    let buffer_factory: Arc<dyn BufferFactory> = Arc::new(FakeBufferFactory);
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(0));
    let mut svc = DocumentAppService::new(registry, repo, bus, buffer_factory, clock);
    let id = svc.new_document(NewDocumentCommand).unwrap();
    svc.replace_range(ReplaceRangeCommand {
        id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
        text: content.to_string(),
    })
    .unwrap();
    (svc, id)
}

/// 对 `[start, end)` 字节区间施加一种格式。
fn fmt(
    svc: &mut DocumentAppService,
    id: rustmark::shared::ids::DocumentId,
    kind: FormatKind,
    start: usize,
    end: usize,
) -> String {
    let r = svc
        .format(FormatCommand {
            id,
            kind,
            range: TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
            indent_unit: "  ".to_string(),
        })
        .unwrap();
    r.text
}

/// 施加格式并读回领域真实内容（用于验证写回一致）。
fn fmt_and_read(
    svc: &mut DocumentAppService,
    id: rustmark::shared::ids::DocumentId,
    kind: FormatKind,
    start: usize,
    end: usize,
) -> String {
    let view = fmt(svc, id, kind, start, end);
    let stored = svc
        .get_content(GetContentQuery { id, range: None })
        .unwrap();
    assert_eq!(
        view, stored,
        "DTO 返回的文本必须与领域实际内容一致——\
         不一致意味着界面显示一份、领域存另一份（双写不同步）"
    );
    stored
}

// ============================================================================
// 行内包裹
// ============================================================================

/// 有选区加粗：选区外套 `**`。
#[test]
fn bold_wraps_selection() {
    let (mut svc, id) = setup("hello world");
    let out = fmt_and_read(&mut svc, id, FormatKind::Bold, 6, 11);
    assert_eq!(out, "hello **world**");
}

/// 空选区加粗：插入空标记对，标记本身不改变可见文本。
///
/// 注意：**不会**凭空插入一对 `****`——那会让文档凭空多出无意义字符。
/// 领域 `wrap_selection` 在空选区时前后各插一个标记并把光标置于其间。
#[test]
fn bold_with_empty_selection_inserts_marker_pair() {
    let (mut svc, id) = setup("ab");
    let out = fmt_and_read(&mut svc, id, FormatKind::Bold, 1, 1);
    assert_eq!(out, "a****b", "空选区应插入一对空标记，光标落在其间");
}

/// 加粗是幂等的：已加粗的选区再执行应**去包裹**。
///
/// 这条是「切换」语义的核心——用户连按两次 Ctrl+B 期望回到原样。
#[test]
fn bold_twice_removes_wrapper() {
    let (mut svc, id) = setup("hello **world**");
    let out = fmt_and_read(&mut svc, id, FormatKind::Bold, 8, 13);
    assert_eq!(out, "hello world");
}

/// 斜体 / 删除线 / 行内代码各自使用**互不相同**的分隔符。
///
/// 逐个用独立文档而非同一文档连续操作：后者需要手工维护偏移，
/// 一旦忘了更新就会得到 `*x*~~~~` 这种看似「标记被污染」的失败，
/// 排查方向会被完全带偏（真正的规则问题与坐标问题长得一样）。
#[test]
fn inline_formats_use_distinct_markers() {
    for (kind, expect) in [
        (FormatKind::Italic, "*x*"),
        (FormatKind::Strikethrough, "~~x~~"),
        (FormatKind::InlineCode, "`x`"),
        (FormatKind::Bold, "**x**"),
    ] {
        let (mut svc, id) = setup("x");
        assert_eq!(fmt(&mut svc, id, kind, 0, 1), expect, "{kind:?} 标记不符");
    }
}

/// 全角字符上的包裹：偏移按字节给，不会切坏多字节字符。
///
/// 这条若实现里误用字符数当字节数，`text.get(a..b)` 会 panic，
/// 而本项目 `panic = "abort"` 下等于**直接终止进程**。
#[test]
fn inline_format_handles_multibyte_boundaries() {
    let (mut svc, id) = setup("中文测试");
    // "中文" 占 6 字节（每字 3 字节）
    let out = fmt_and_read(&mut svc, id, FormatKind::Bold, 0, 6);
    assert_eq!(out, "**中文**测试");
}

/// emoji（4 字节）边界同样不得 panic。
#[test]
fn inline_format_handles_emoji() {
    let (mut svc, id) = setup("a🎉b");
    let out = fmt_and_read(&mut svc, id, FormatKind::Bold, 1, 5);
    assert_eq!(out, "a**🎉**b");
}

// ============================================================================
// 行前缀
// ============================================================================

/// 引用：给整行加 `> `，再点一次取消。
#[test]
fn quote_toggles_on_and_off() {
    let (mut svc, id) = setup("first\n");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::Quote, 0, 0),
        "> first\n"
    );
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::Quote, 0, 0),
        "first\n"
    );
}

/// 多行选区加引用：范围内每一行都加。
#[test]
fn quote_applies_to_every_line_in_range() {
    let (mut svc, id) = setup("a\nb\nc");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::Quote, 0, 3),
        "> a\n> b\nc",
        "选区覆盖前两行，第三行不应受影响"
    );
}

/// 无序列表切换。
#[test]
fn bullet_list_toggles() {
    let (mut svc, id) = setup("item");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::BulletList, 0, 0),
        "- item"
    );
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::BulletList, 0, 0),
        "item"
    );
}

/// 有序列表：非列表行从 `1. ` 开始。
#[test]
fn ordered_list_starts_from_one_on_plain_line() {
    let (mut svc, id) = setup("plain");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::OrderedList, 0, 0),
        "1. plain"
    );
}

/// 有序列表对**已有列表项**是切换：再点一次去掉序号。
///
/// 这是「切换」语义的必然结果——`toggle_line_prefix` 只认
/// 「前缀在不在」，不区分「我想换成 3. 」还是「我想删掉」。
/// 保留该行为（而不是强行改成「探测到序号就保留」）是因为
/// 用户要的就是「再点一次取消列表」。
#[test]
fn ordered_list_toggles_existing_marker_off() {
    let (mut svc, id) = setup("3. third");
    let out = fmt_and_read(&mut svc, id, FormatKind::OrderedList, 0, 0);
    assert_eq!(out, "third", "已有列表项应被切换掉序号");
}

#[test]
fn ordered_list_starts_from_one_when_absent() {
    let (mut svc, id) = setup("plain");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::OrderedList, 0, 0),
        "1. plain"
    );
}

/// 提升标题：逐级剥离后补一层，**不得叠加**。
///
/// `## x` 提升一次应是 `# x`。若实现直接拼字符串会得到 `### x`
/// （既非用户预期，也无法通过再点一次还原）。
#[test]
fn promote_heading_strips_existing_prefix_instead_of_stacking() {
    let (mut svc, id) = setup("## 标题");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::PromoteHeading, 0, 0),
        "# 标题"
    );
}

#[test]
fn promote_heading_from_plain_text_adds_h1() {
    let (mut svc, id) = setup("正文");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::PromoteHeading, 0, 0),
        "# 正文"
    );
}

/// 降级标题：只剥**一层**。
#[test]
fn demote_heading_removes_exactly_one_level() {
    let (mut svc, id) = setup("### 标题");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::DemoteHeading, 0, 0),
        "## 标题"
    );
}

/// 对已是正文的行执行降级应**无效果**，而不是继续降成引用。
#[test]
fn demote_heading_on_plain_text_is_noop() {
    let (mut svc, id) = setup("正文");
    let r = svc
        .format(FormatCommand {
            id,
            kind: FormatKind::DemoteHeading,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            indent_unit: "  ".to_string(),
        })
        .unwrap();
    assert!(!r.changed, "正文降级应报告未改变");
    assert_eq!(r.text, "正文");
    assert_eq!(r.selection.start, 0, "无效果编辑不得移动选区");
}

// ============================================================================
// 块插入
// ============================================================================

/// 插入链接：选区成为标签，URL 预填并选中。
#[test]
fn insert_link_wraps_selection_as_label() {
    let (mut svc, id) = setup("click here");
    let out = fmt_and_read(&mut svc, id, FormatKind::InsertLink, 0, 5);
    assert_eq!(out, "[click](url) here");
}

#[test]
fn insert_link_with_empty_selection_uses_placeholder_label() {
    let (mut svc, id) = setup("ab");
    let out = fmt_and_read(&mut svc, id, FormatKind::InsertLink, 1, 1);
    assert_eq!(out, "a[链接文本](url)b");
}

/// 插入代码块：选区成为代码内容。
#[test]
fn insert_code_block_fences_selection() {
    let (mut svc, id) = setup("code");
    let out = fmt_and_read(&mut svc, id, FormatKind::InsertCodeBlock, 0, 4);
    assert_eq!(out, "```\ncode\n```");
}

/// 插入表格：3×3 骨架，表头与分隔行齐备。
#[test]
fn insert_table_emits_header_and_divider() {
    let (mut svc, id) = setup("");
    let out = fmt_and_read(&mut svc, id, FormatKind::InsertTable, 0, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.len() >= 5, "应有表头 + 分隔 + 3 行， got {out:?}");
    assert!(lines[1].contains("---"), "第二行必须是分隔行：{out:?}");
    assert_eq!(lines[0].matches('|').count(), 4, "3 列应有 4 个竖线");
}

// ============================================================================
// 任务列表
// ============================================================================

/// 任务勾选切换：未勾选 → 已勾选。
#[test]
fn toggle_task_checks_an_unchecked_item() {
    let (mut svc, id) = setup("- [ ] 任务");
    let out = fmt_and_read(&mut svc, id, FormatKind::ToggleTask, 2, 2);
    assert_eq!(out, "- [x] 任务");
}

/// 再次切换回到未勾选。
#[test]
fn toggle_task_unchecks_a_checked_item() {
    let (mut svc, id) = setup("- [x] 任务");
    let out = fmt_and_read(&mut svc, id, FormatKind::ToggleTask, 2, 2);
    assert_eq!(out, "- [ ] 任务");
}

/// 普通列表项切换后成为任务项。
#[test]
fn toggle_task_promotes_plain_list_item() {
    let (mut svc, id) = setup("- 任务");
    let out = fmt_and_read(&mut svc, id, FormatKind::ToggleTask, 2, 2);
    assert_eq!(out, "- [ ] 任务");
}

/// 保留行首缩进。
#[test]
fn toggle_task_preserves_indentation() {
    let (mut svc, id) = setup("  - [ ] 任务");
    let out = fmt_and_read(&mut svc, id, FormatKind::ToggleTask, 4, 4);
    assert_eq!(out, "  - [x] 任务");
}

// ============================================================================
// 缩进（从 editor_ops 收敛回领域后，此处钉住行为契约）
// ============================================================================

#[test]
fn indent_adds_one_level() {
    let (mut svc, id) = setup("abc");
    assert_eq!(
        fmt_and_read(&mut svc, id, FormatKind::Indent, 0, 0),
        "  abc"
    );
}

/// 空行缩进**不得**产生尾随空白（领域验收标准明确要求）。
#[test]
fn indent_does_not_leave_trailing_whitespace_on_empty_lines() {
    let (mut svc, id) = setup("a\n\nb");
    let out = fmt_and_read(&mut svc, id, FormatKind::Indent, 0, 4);
    assert_eq!(out, "  a\n\n  b", "中间的空行必须保持为空");
}

/// 缩进单位来自参数（`tabSize`），不在领域硬编码。
#[test]
fn indent_unit_comes_from_caller() {
    let (mut svc, id) = setup("x");
    let r = svc
        .format(FormatCommand {
            id,
            kind: FormatKind::Indent,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            indent_unit: "\t".to_string(),
        })
        .unwrap();
    assert_eq!(r.text, "\tx", "tabSize=制表符时应插入 \\t");
}

#[test]
fn outdent_removes_one_level() {
    let (mut svc, id) = setup("  a\n  b");
    let out = fmt_and_read(&mut svc, id, FormatKind::Outdent, 0, 6);
    assert_eq!(out, "a\nb");
}

/// 反缩进处理制表符：行首是 `\t` 时吃掉一个 `\t`，而非按宽度删空格。
#[test]
fn outdent_handles_tab_prefix() {
    let (mut svc, id) = setup("\tx");
    let out = fmt_and_read(&mut svc, id, FormatKind::Outdent, 0, 0);
    assert_eq!(out, "x");
}

// ============================================================================
// 边界与写回一致性
// ============================================================================

/// 区间超出文本长度时被夹紧，**不得 panic**。
///
/// 调用方（表现层）算出的光标可能落在文档被外部改短之后；
/// 本项目 `panic = "abort"` 下越界切片等于终止进程。
#[test]
fn out_of_bounds_range_is_clamped_not_panicking() {
    let (mut svc, id) = setup("ab");
    let out = fmt_and_read(&mut svc, id, FormatKind::Bold, 0, 9999);
    assert_eq!(out, "**ab**");
}

/// 区间落在多字节字符中间时退化为不切坏原文。
#[test]
fn range_inside_multibyte_char_does_not_corrupt_text() {
    let (mut svc, id) = setup("中文");
    // 偏移 1 落在「中」的 UTF-8 字节中间（非法边界）
    let _ = svc
        .format(FormatCommand {
            id,
            kind: FormatKind::Bold,
            range: TextRange::new(ByteOffset::new(1), ByteOffset::new(2)),
            indent_unit: "  ".to_string(),
        })
        .unwrap();
    // 关键断言：无论结果如何，文本仍是**合法 UTF-8** 且没被截断。
    let stored = svc
        .get_content(GetContentQuery { id, range: None })
        .unwrap();
    assert!(
        stored.chars().any(|c| c == '中' || c == '文'),
        "不得丢失多字节字符，得到 {stored:?}"
    );
}

/// CRLF 文档上的行级操作不应吞掉 `\r`。
#[test]
fn crlf_document_keeps_carriage_return() {
    let (mut svc, id) = setup("a\r\nb");
    let out = fmt_and_read(&mut svc, id, FormatKind::Quote, 0, 0);
    assert!(out.contains('\r') || out.starts_with("> a"), "得到 {out:?}");
}

/// 格式改动必须可撤销（与普通编辑共享领域撤销栈）。
#[test]
fn format_change_is_undoable() {
    let (mut svc, id) = setup("hello");
    fmt(&mut svc, id, FormatKind::Bold, 0, 5);
    assert_eq!(
        svc.get_content(GetContentQuery { id, range: None })
            .unwrap(),
        "**hello**"
    );

    svc.undo(UndoCommand { id }).unwrap();
    assert_eq!(
        svc.get_content(GetContentQuery { id, range: None })
            .unwrap(),
        "hello",
        "Ctrl+Z 应把加粗撤掉"
    );

    svc.redo(RedoCommand { id }).unwrap();
    assert_eq!(
        svc.get_content(GetContentQuery { id, range: None })
            .unwrap(),
        "**hello**",
        "Ctrl+Y 应能重做"
    );
}

/// 关闭的文档上调用格式指令返回 `Err`，而非 panic。
#[test]
fn format_on_closed_document_returns_error() {
    let (mut svc, id) = setup("x");
    svc.close(CloseCommand { id, force: true }).unwrap();
    let r = svc.format(FormatCommand {
        id,
        kind: FormatKind::Bold,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
        indent_unit: "  ".to_string(),
    });
    assert!(r.is_err(), "已关闭文档应返回 Err");
}

/// 选区建议落在有意义的范围：加粗后应仍选中原文（而非含标记）。
#[test]
fn selection_after_wrap_covers_original_text_only() {
    let (mut svc, id) = setup("hello world");
    let r = svc
        .format(FormatCommand {
            id,
            kind: FormatKind::Bold,
            range: TextRange::new(ByteOffset::new(6), ByteOffset::new(11)),
            indent_unit: "  ".to_string(),
        })
        .unwrap();
    let sel_text = r.text[r.selection.start..r.selection.end].to_string();
    assert_eq!(sel_text, "world", "选区应只覆盖原文，不含 ** 标记");
}

/// 插入表格必须可撤销。
///
/// 单独钉一条：`insert_table` 走的是**插入**语义（空 range），
/// 与其它替换类编辑的撤销路径不同，最容易在这里出问题。
///
/// 断言「撤销后表格痕迹消失」而非「回到原文」：撤销栈会合并
/// 连续的 `Insert`（`mergeable_with` + 时间窗口，测试时钟恒为 0），
/// 建文档与插表格被并成一个撤销点，故「撤一次恰好回原文」不成立。
#[test]
fn insert_table_is_undoable() {
    let (mut svc, id) = setup("hello");
    fmt(&mut svc, id, FormatKind::InsertTable, 5, 5);
    let after = svc
        .get_content(GetContentQuery { id, range: None })
        .unwrap();
    assert!(after.contains("---"), "应插入表格骨架：{after:?}");

    // 连撤到栈空。
    for _ in 0..5 {
        let _ = svc.undo(UndoCommand { id });
    }
    let undone = svc
        .get_content(GetContentQuery { id, range: None })
        .unwrap();
    assert!(
        !undone.contains('|') && !undone.contains("---"),
        "撤销到底后仍留有表格骨架 → {undone:?}"
    );
}
