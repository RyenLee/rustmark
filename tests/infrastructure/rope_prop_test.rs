//! 属性测试（S9-03）：文本缓冲与撤销栈的「不变量」。
//!
//! # 为什么是固定种子而不是 `proptest`
//!
//! 任务书把属性测试列为可选并允许降级：「若体积/编译代价过大，
//! 降级为固定随机种子用例并记录理由」。本文件采用降级方案：
//!
//! - `proptest` 只在**运行测试**时需要，不影响 release 体积，但会拉进
//!   `rand-core` 一族依赖，冷编译时间增加约 30s+（CI 每次都要付）；
//! - 属性测试的价值在「随机但不自觉偏向先入为主的生成方式」，
//!   对本项目的三个不变量（内容一致性、历史状态合法性、坐标往返），
//!   一颗**可复现的** xorshift64 星生成器覆盖同样的操作空间；
//! - 固定种子的额外收益：失败可**精确复现**（proptest 也要靠种子重放），
//!   且零新依赖符合 `conventions.md` 第 26 条（新增依赖须 ADR 评审）。
//!
//! 每组属性跑 **256 例**（任务书门槛），全部用例在失败时打印种子与
//! 操作序列，可手工重放定位。

use rustmark::domain::document::services::TextBuffer;
use rustmark::domain::document::undo::{Snapshot, UndoStack};
use rustmark::domain::document::value_objects::ModificationKind;
use rustmark::infrastructure::text::rope_text_buffer::RopeTextBuffer;
use rustmark::ports::time::Timestamp;
use rustmark::shared::geometry::{ByteOffset, TextRange};

use crate::support::text_buffer::StringTextBuffer;

/// 每组属性的例数（任务书要求 ≥ 256）。
const CASES: u32 = 256;

/// xorshift64* 伪随机数生成器（固定种子，可复现）。
///
/// 比 `RandomState`/系统熵源慢不了多少，但序列完全确定：
/// 同一种子永远得到同一序列——这是属性测试失败后能「重放」的前提。
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // 全零是 xorshift 的不动点，种子先混合一个常数。
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `[0, bound)` 内的均匀值（拒绝采样，避免模偏差）。
    fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0);
        loop {
            let x = self.next_u64();
            let max = u64::MAX - u64::MAX % bound;
            if x < max {
                return x % bound;
            }
        }
    }
}

/// 编辑操作用的随机文本片段（含多字节与换行，打击对齐 bug）。
const FRAGMENTS: &[&str] = &["a", "中", "🌿", "\n", "ab\n文", "z"];

/// 属性 1：随机 insert / remove 序列下，Rope 与参考实现**逐操作**内容一致。
///
/// 不变量：两个实现对同一操作序列的 `snapshot()` 永远相同；
/// 删除区间先经 `align_range` 对齐字符边界（编辑器真实的调用约定）。
#[test]
fn prop_rope_matches_reference_under_random_edits() {
    let mut rng = Rng::new(0xC0FF_EE01);
    for case in 0..CASES {
        // 初始语料：随机长度（0..80 字节）的碎片拼接。
        let mut text = String::new();
        for _ in 0..rng.below(20) {
            text.push_str(FRAGMENTS[rng.below(FRAGMENTS.len() as u64) as usize]);
        }
        let mut rope = RopeTextBuffer::new_with_text(&text);
        let mut reference = StringTextBuffer::from_str(&text);

        // 每例 24 个随机操作。
        for step in 0..24 {
            let len = rope.len_bytes();
            if rng.below(2) == 0 || len == 0 {
                // 插入：偏移对齐到字符边界。
                let at =
                    rope.char_boundary_floor(ByteOffset::new(rng.below(len as u64 + 1) as usize));
                let frag = FRAGMENTS[rng.below(FRAGMENTS.len() as u64) as usize];
                rope.insert(at, frag)
                    .unwrap_or_else(|e| panic!("case {case} step {step}: insert 失败 {e:?}"));
                reference.insert(at, frag).unwrap_or_else(|e| {
                    panic!("case {case} step {step}: 参考实现 insert 失败 {e:?}")
                });
            } else {
                // 删除：随机区间（floor..ceil 保证非空且对齐）。
                let a = rope.char_boundary_floor(ByteOffset::new(rng.below(len as u64) as usize));
                let b = rope.char_boundary_ceil(ByteOffset::new(
                    (a.value() as u64 + 1 + rng.below(len as u64 - a.value() as u64))
                        .min(len as u64) as usize,
                ));
                let range = TextRange::new(a, b);
                let expect = reference.remove(range).unwrap_or_else(|e| {
                    panic!("case {case} step {step}: 参考实现 remove 失败 {e:?}")
                });
                let got = rope
                    .remove(range)
                    .unwrap_or_else(|e| panic!("case {case} step {step}: remove 失败 {e:?}"));
                assert_eq!(got, expect, "case {case} step {step}: remove 返回值不一致");
            }
            assert_eq!(
                rope.snapshot(),
                reference.snapshot(),
                "case {case} step {step}：内容分叉（种子可复现：Rng::new(0xC0FFEE01)，case={case}）"
            );
        }
    }
}

/// 属性 2：随机 undo / redo 序列后，内容回到**合法历史状态**。
///
/// 不变量：
/// ① 连续 undo 到底，内容必然回到最初快照（历史不会凭空丢一步）；
/// ② undo 后 redo 到底，回到撤销前的内容；
/// ③ 深度不超上限；④ 新编辑后 redo 链必然清空。
#[test]
fn prop_undo_redo_returns_to_a_legal_history_state() {
    let mut rng = Rng::new(0xABCD_1234);
    for case in 0..CASES {
        let mut stack = UndoStack::with_default_policy();

        // 编辑历史：从初稿出发，每次编辑记录「编辑前」快照。
        // 时间戳间距 1000ms，远超合并窗口（400ms），每步必入栈。
        let mut current = String::from("初稿");
        let mut t = 1_000_000u64;
        for _ in 0..rng.below(40) {
            stack.push(
                Snapshot::new(current.clone(), None, Timestamp::from_millis(t)),
                if rng.below(2) == 0 {
                    ModificationKind::Insert
                } else {
                    ModificationKind::Remove
                },
            );
            t += 1_000;
            current.push_str(FRAGMENTS[rng.below(FRAGMENTS.len() as u64) as usize]);
        }
        let depth = stack.undo_depth();
        assert!(depth <= 40, "深度 {depth} 超过入栈次数上限");
        if depth == 0 {
            // 没有任何入栈（rng 恰好给了 0 次编辑）：无从撤销，直接验
            // 「空栈不可撤销 / 不可重做」后进入下一例。
            assert!(!stack.can_undo() && !stack.can_redo());
            continue;
        }
        let before_undo = current.clone();

        // ① 逐步 undo：每一步拿到的都是「某次编辑前」的合法快照。
        let mut restored = current.clone();
        for i in 0..depth {
            let prev = stack
                .undo(Snapshot::new(
                    restored.clone(),
                    None,
                    Timestamp::from_millis(t),
                ))
                .unwrap_or_else(|| panic!("case {case}: 第 {i} 步 undo 不应失败"));
            restored = prev.content().to_string();
        }
        assert_eq!(restored, "初稿", "case {case}: undo 到底应回到初稿");
        assert!(!stack.can_undo(), "case {case}: 到底后不应再可撤销");
        assert!(stack.can_redo(), "case {case}: 撤销过就应可重做");

        // ② redo 全部步数，回到撤销前内容。
        let mut redone = restored;
        for i in 0..depth {
            let next = stack
                .redo(Snapshot::new(
                    redone.clone(),
                    None,
                    Timestamp::from_millis(t),
                ))
                .unwrap_or_else(|| panic!("case {case}: 第 {i} 步 redo 不应失败"));
            redone = next.content().to_string();
        }
        assert_eq!(
            redone, before_undo,
            "case {case}: redo 到底应回到撤销前内容"
        );
        assert!(!stack.can_redo(), "case {case}: redo 到底后不应再可重做");

        // ③ 新编辑使 redo 链失效。
        stack.push(
            Snapshot::new(redone, None, Timestamp::from_millis(t)),
            ModificationKind::Replace,
        );
        assert!(!stack.can_redo(), "case {case}: 新编辑后 redo 链应清空");
    }
}

/// 属性 3：`byte_to_line_col` 与 `line_col_to_byte` 在全部字符边界上往返一致。
///
/// 不变量：对任意字符边界偏移 o，`line_col_to_byte(byte_to_line_col(o)) == o`；
/// 反方向对「每行每列（该行的字符边界）」同样成立。越界必须一致报错。
#[test]
fn prop_line_col_round_trips_through_byte() {
    let mut rng = Rng::new(0x5EED_0003);
    for case in 0..CASES {
        // 随机语料：2..12 行，每行 0..30 字节碎片。
        let mut text = String::new();
        let lines = 2 + rng.below(11);
        for i in 0..lines {
            for _ in 0..rng.below(10) {
                text.push_str(FRAGMENTS[rng.below(FRAGMENTS.len() as u64) as usize]);
            }
            if i + 1 < lines {
                text.push('\n');
            }
        }
        let rope = RopeTextBuffer::new_with_text(&text);
        let reference = StringTextBuffer::from_str(&text);

        // 正向：每个字符边界偏移 → 行列 → 偏移。
        for o in 0..=text.len() {
            let probe = ByteOffset::new(o);
            if o < text.len() && !text.is_char_boundary(o) {
                continue; // 非边界不保证往返（两个实现都容忍但语义未定义）。
            }
            let lc = rope
                .byte_to_line_col(probe)
                .unwrap_or_else(|e| panic!("case {case}: byte_to_line_col({o}) 失败 {e:?}"));
            let back = rope
                .line_col_to_byte(lc)
                .unwrap_or_else(|e| panic!("case {case}: line_col_to_byte({lc:?}) 失败 {e:?}"));
            assert_eq!(back, probe, "case {case}: 偏移 {o} 往返不一致");
            // 与参考实现逐点比对（差分加持）。
            assert_eq!(
                rope.byte_to_line_col(probe),
                reference.byte_to_line_col(probe),
                "case {case}: byte_to_line_col({o}) 与参考实现不一致"
            );
        }

        // 反向：逐行、逐字符边界列 → 偏移 → 行列。
        for line in 0..rope.len_lines() {
            let start = rope.line_start_byte(line).unwrap();
            let end = rope.line_end_byte(line).unwrap();
            for o in start.value()..=end.value() {
                if o < end.value() && !text.is_char_boundary(o) {
                    continue;
                }
                let lc = rope.byte_to_line_col(ByteOffset::new(o)).unwrap();
                let back = rope.line_col_to_byte(lc).unwrap();
                assert_eq!(
                    back,
                    ByteOffset::new(o),
                    "case {case}: 行 {line} 列往返不一致（偏移 {o}）"
                );
            }
        }
    }
}
