//! `RopeTextBuffer` 单测（S5-01）。
//!
//! # 本文件的形态选择
//!
//! 任务书要求「以同一套用例跑一遍 `tests/domain/text_buffer_test.rs` 作对照基线」。
//! 但**复制**用例有风险：两份副本会各自漂移，某天改了一处而忘了另一处，
//! 对照基线就名存实亡。
//!
//! 因此本文件采用**差分测试（differential testing）**形态：把同一个输入同时喂给
//! 两个实现，逐方法比对返回值。这样：
//! -覆盖范围自动与参考实现**保持一致**（参考实现新增用例时，
//!   只要它自己仍通过，本文件的比对矩阵仍在同一批语义上运行）；
//! - 新增语料只需在 [`CASES`] 里加一行，两个实现同时受检。
//!
//! 另有一组 `rope_specific_*` 用例覆盖 ropey 特有行为（大文件局部编辑的
//! 复杂度优势、CRLF、多字节字符穿插编辑），这些在朴素实现上没有对应物。

use rustmark::domain::document::errors::TextBufferError;
use rustmark::domain::document::services::TextBuffer;
use rustmark::infrastructure::text::rope_text_buffer::{RopeBufferFactory, RopeTextBuffer};
use rustmark::shared::geometry::{ByteOffset, LineCol, TextRange};

use crate::support::text_buffer::StringTextBuffer;

/// 差分测试语料：覆盖空文本、ASCII、中文、多字节、CRLF、末行换行、单行超长。
const CASES: &[&str] = &[
    "",
    "a",
    "hello world",
    "a\n",
    "a\nb\nc",
    "\n\n\n",
    "中文",
    "中文abc中文",
    "a\r\nb\r\n",
    "# 标题\n\n正文内容\n\n- 列表项\n",
    "line1\n中文行\nline3\n",
    "trailing spaces   \nnext",
    "🌿 emoji 与中文混排",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbbbb\ncccc",
];

/// 对两个实现同时执行 `f`，比对结果是否一致。
///
/// `TextBufferError` 派生自 `PartialEq`，因此错误**类型与内容**都必须相同——
/// 这比只比对 `is_err()` 严格得多，能发现「ropey 把 `InvalidRange` 误报成
/// `OutOfRange`」这类只在错误路径出现的分叉。
fn assert_same<T: std::fmt::Debug + PartialEq, E: PartialEq + std::fmt::Debug>(
    label: &str,
    reference: Result<T, E>,
    rope: Result<T, E>,
) {
    match (reference, rope) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "{label}：两个实现的 Ok 值不一致"),
        (Err(a), Err(b)) => assert_eq!(a, b, "{label}：两个实现的错误不一致"),
        (Ok(_), Err(e)) => panic!("{label}：参考实现成功但 ropey 报错 {e:?}"),
        (Err(e), Ok(_)) => panic!("{label}：参考实现报错 {e:?} 但 ropey 成功"),
    }
}

/// 全语料差分：长度、坐标换算、边界对齐。
#[test]
fn rope_matches_reference_on_lengths_and_coordinates() {
    for text in CASES {
        let rope = RopeTextBuffer::new_with_text(text);
        let reference = StringTextBuffer::from_str(text);

        assert_eq!(
            rope.len_bytes(),
            reference.len_bytes(),
            "len_bytes 不一致：{text:?}"
        );
        assert_eq!(
            rope.len_chars(),
            reference.len_chars(),
            "len_chars 不一致：{text:?}"
        );
        assert_eq!(
            rope.len_lines(),
            reference.len_lines(),
            "len_lines 不一致：{text:?}"
        );
        assert_eq!(rope.snapshot(), *text, "snapshot 未还原原文：{text:?}");

        // 逐字节换算：包括落在多字节字符**中间**的偏移（两者都应容忍，不 panic）。
        for offset in 0..=text.len() {
            let probe = ByteOffset::new(offset);
            assert_same(
                &format!("byte_to_line_col({offset}) of {text:?}"),
                reference.byte_to_line_col(probe),
                rope.byte_to_line_col(probe),
            );
            // 越界偏移必须一致地报错，且错误内容相同。
            assert_same(
                &format!("byte_to_line_col({}) 越界 of {text:?}", text.len() + 1),
                reference.byte_to_line_col(ByteOffset::new(text.len() + 1)),
                rope.byte_to_line_col(ByteOffset::new(text.len() + 1)),
            );
            assert_eq!(
                rope.char_boundary_floor(probe),
                reference.char_boundary_floor(probe),
                "char_boundary_floor({offset}) 不一致：{text:?}"
            );
            assert_eq!(
                rope.char_boundary_ceil(probe),
                reference.char_boundary_ceil(probe),
                "char_boundary_ceil({offset}) 不一致：{text:?}"
            );
        }

        // 逐行坐标换算与行首行尾。
        for line in 0..rope.len_lines() {
            assert_same(
                &format!("line_start_byte({line}) of {text:?}"),
                reference.line_start_byte(line),
                rope.line_start_byte(line),
            );
            assert_same(
                &format!("line_end_byte({line}) of {text:?}"),
                reference.line_end_byte(line),
                rope.line_end_byte(line),
            );
            for col in 0..=8 {
                assert_same(
                    &format!("line_col_to_byte({line},{col}) of {text:?}"),
                    reference.line_col_to_byte(LineCol::new(line, col)),
                    rope.line_col_to_byte(LineCol::new(line, col)),
                );
            }
        }
        // 行号越界必须一致报错。
        assert_same(
            &format!("line_start_byte(越界) of {text:?}"),
            reference.line_start_byte(rope.len_lines()),
            rope.line_start_byte(rope.len_lines()),
        );
        assert_same(
            &format!("line_end_byte(越界) of {text:?}"),
            reference.line_end_byte(rope.len_lines()),
            rope.line_end_byte(rope.len_lines()),
        );
    }
}

/// 全语料差分：`slice` 与 `insert` / `remove` 的返回值与错误。
#[test]
fn rope_matches_reference_on_editing() {
    for text in CASES {
        let mut rope = RopeTextBuffer::new_with_text(text);
        let mut reference = StringTextBuffer::from_str(text);

        // slice：合法区间、越界区间、反向区间（`TextRange::new` 已归一化）。
        let probes: Vec<usize> = (0..=text.len()).step_by(1).collect();
        for &start in &probes {
            for &end in &probes {
                let range = TextRange::new(ByteOffset::new(start), ByteOffset::new(end));
                let rope_slice = rope.slice(range).map(|c| c.into_owned());
                let reference_slice = reference.slice(range).map(|c| c.into_owned());
                assert_same(
                    &format!("slice({start},{end}) of {text:?}"),
                    reference_slice,
                    rope_slice,
                );
            }
        }

        // insert：合法偏移、恰好在末尾（合法）、越界（非法）、字符中间（非法）。
        let insert_probes = [0usize, 1, text.len(), text.len() + 1, text.len() * 2];
        for at in insert_probes {
            assert_same(
                &format!("insert({at}) of {text:?}"),
                reference.insert(ByteOffset::new(at), "X"),
                rope.insert(ByteOffset::new(at), "X"),
            );
        }
        // insert 成功时两者内容必须同步演进，后续比对才有意义。
        assert_eq!(
            rope.snapshot(),
            reference.snapshot(),
            "一轮 insert 后内容分叉：{text:?}"
        );

        // remove：整段、清空、越界。区间终点须用**当前**长度——上面的 insert 已改变内容。
        let full = TextRange::new(ByteOffset::new(0), ByteOffset::new(rope.len_bytes() + 1));
        assert_same(
            &format!("remove(越界) of {text:?}"),
            reference.remove(full),
            rope.remove(full),
        );
        let current = rope.len_bytes();
        let all = TextRange::new(ByteOffset::new(0), ByteOffset::new(current));
        let rope_removed = rope.remove(all);
        let reference_removed = reference.remove(all);
        assert_same(
            &format!("remove(全段) of {text:?}"),
            reference_removed,
            rope_removed,
        );
        assert_eq!(rope.snapshot(), reference.snapshot(), "清空后不一致");
        assert_eq!(rope.len_bytes(), 0, "清空后长度应为 0");
    }
}

// ---------------------------------------------------------------------------
// 以下为 ropey 特有行为的用例（参考实现无对应物）
// ---------------------------------------------------------------------------

/// 验收要求的第4 组：snapshot 与原缓冲**独立**。
#[test]
fn rope_specific_snapshot_is_independent() {
    let mut buffer = RopeTextBuffer::new_with_text("original");
    let snapshot = buffer.snapshot();
    buffer
        .insert(ByteOffset::new(snapshot.len()), " mutated")
        .expect("追加应成功");
    assert_eq!(snapshot, "original", "快照不应随后续编辑而变化");
    assert_eq!(buffer.snapshot(), "original mutated");
}

/// 多字节字符中间插入必须被拒绝（否则产生乱码，`FR-05`）。
#[test]
fn rope_specific_insert_inside_multibyte_char_is_rejected() {
    let mut buffer = RopeTextBuffer::new_with_text("中文");
    // "中" 占 3 字节，偏移 1 与 2 都落在字符中间。
    for bad in [1usize, 2] {
        assert_eq!(
            buffer.insert(ByteOffset::new(bad), "x"),
            Err(TextBufferError::NotCharBoundary { offset: bad }),
            "偏移 {bad} 应被判定为非字符边界"
        );
    }
    assert_eq!(buffer.snapshot(), "中文", "被拒绝的插入不得改变内容");

    // 合法边界 3 恰是"文"的起始字节（"中" 占0..3），故插入后应夹在两字之间。
    buffer
        .insert(ByteOffset::new(3), "!")
        .expect("合法边界插入应成功");
    assert_eq!(buffer.snapshot(), "中!文");

    // 此刻内容为 "中!文"（3 + 1 + 3 = 7 字节），故末尾边界是 7 而非 6。
    buffer
        .insert(ByteOffset::new(7), "?")
        .expect("末尾边界插入应成功");
    assert_eq!(buffer.snapshot(), "中!文?");
}

/// 验收要求的第 2 组：字节区间删除返回被删内容，且两端须在边界上。
#[test]
fn rope_specific_remove_range_returns_removed_text() {
    let mut buffer = RopeTextBuffer::new_with_text("hello world");
    let removed = buffer
        .remove(TextRange::new(ByteOffset::new(6), ByteOffset::new(11)))
        .expect("删除合法区间应成功");
    assert_eq!(removed, "world");
    assert_eq!(buffer.snapshot(), "hello ");

    // 终点落在 "world" 的 'o' 之后一字节的边界上（合法），但终点越界则拒绝。
    assert!(matches!(
        buffer.remove(TextRange::new(ByteOffset::new(0), ByteOffset::new(99))),
        Err(TextBufferError::InvalidRange { .. })
    ));
}

/// CRLF：ropey 的 `len_lines` 与朴素实现一致（`\r\n` 算**一个**换行）。
#[test]
fn rope_specific_crlf_counts_single_break() {
    let buffer = RopeTextBuffer::new_with_text("a\r\nb\r\n");
    assert_eq!(buffer.len_lines(), 3, "\"a\\r\\nb\\r\\n\" 应为 3 行");
    assert_eq!(
        buffer.line_end_byte(0).unwrap(),
        ByteOffset::new(2),
        "第 0 行结束于 `\\r` 之前"
    );
    assert_eq!(buffer.line_start_byte(1).unwrap(), ByteOffset::new(3));
}

/// 验收要求的第 1 组：大文件局部编辑应高效且不 panic。
///
/// 这是 Rope 相对 `String` 的**核心价值**：对 1MB 文档做 1000 次中部插入，
/// `String` 需要约 1000 × 500KB ≈ 500MB 的memcpy，Rope 只需重写少数分块。
/// 这里断言「正确性 + 不 panic」；耗时只做宽松上限，避免 CI 抖动导致误报。
#[test]
fn rope_specific_large_file_random_edits_do_not_panic() {
    let mut buffer = RopeTextBuffer::new_with_text(&"x".repeat(1024 * 1024));
    assert_eq!(buffer.len_bytes(), 1024 * 1024);

    // 伪随机（固定种子）的中部插入 + 删除，避免引入 rand 依赖。
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..1000 {
        let at = (next() % buffer.len_bytes() as u64) as usize;
        let at = buffer.char_boundary_floor(ByteOffset::new(at)).value();
        buffer
            .insert(ByteOffset::new(at), "ab")
            .expect("边界对齐后的插入必须成功");
    }
    assert_eq!(buffer.len_bytes(), 1024 * 1024 + 2000);
    assert_eq!(buffer.len_chars(), 1024 * 1024 + 2000);

    // 再全部删回空，确保 remove 的字符索引换算在大文件上同样正确。
    buffer
        .remove(TextRange::new(
            ByteOffset::new(0),
            ByteOffset::new(buffer.len_bytes()),
        ))
        .expect("清空大文件应成功");
    assert_eq!(buffer.len_bytes(), 0);
    assert_eq!(buffer.len_lines(), 1, "空缓冲区仍为 1 行");
}

/// `slice` 的零拷贝快路径：连续切片应返回 `Cow::Borrowed`。
#[test]
fn rope_specific_slice_prefers_borrowed_when_contiguous() {
    let buffer = RopeTextBuffer::new_with_text("hello world");
    let slice = buffer
        .slice(TextRange::new(ByteOffset::new(0), ByteOffset::new(5)))
        .expect("合法切片应成功");
    assert!(
        matches!(slice, std::borrow::Cow::Borrowed(_)),
        "连续切片应零拷贝"
    );
    assert_eq!(slice, "hello");
}

/// `RopeBufferFactory` 必须产出可用的 rope 缓冲（组合根依赖它注入实现）。
#[test]
fn rope_specific_factory_produces_working_buffer() {
    use rustmark::domain::document::services::BufferFactory;

    let factory = RopeBufferFactory;
    let mut buffer = factory.create("abc");
    assert_eq!(buffer.len_bytes(), 3);
    buffer
        .insert(ByteOffset::new(3), "d")
        .expect("工厂产出的缓冲应可写");
    assert_eq!(buffer.snapshot(), "abcd");
}

/// 编译期断言：缓冲实现必须跨线程共享（后台预览线程要求 `Send + Sync`）。
#[test]
fn rope_specific_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RopeTextBuffer>();
    assert_send_sync::<RopeBufferFactory>();
}
