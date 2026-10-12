//! 编辑区纯文本变换逻辑（S6-03）。
//!
//! # 为什么这些是纯函数
//!
//! 编辑区的「缩进 / 反缩进 / 移动行 / 列表续行 / 括号配对」都是**对一段文本的可预测变换**，
//! 不依赖 egui、不依赖领域层。把它们抽成纯函数有三个好处：
//!
//! 1. **可测**：无需 GUI 即可逐条验证多字节边界、选区跨行等易错点（见本文件末尾 `#[cfg(test)]`）。
//! 2. **可移植**：若将来把编辑面从 egui 换成 `cosmic-text`，这些变换一行都不用改
//!    （S6-03 风险预案要求的「输入法/编辑器隔离层」的底层支撑）。
//! 3. **单一真相**：视图只负责「把旧文本变成新文本」的意图，真正的写入永远走领域命令。
//!
//! 所有偏移语义统一为：**选区/光标用字符索引，命令用字节偏移**。
//! 原因见 [`byte_to_char`] / [`char_to_byte`]——`İ`（1 字符 → 2 字节）、
//! emoji、组合字符都会让「字符数 ≠ 字节数」，直接用字节切文本会 panic。
//!
//! # 哪些规则**不该**出现在这里（S6-07 之后的收敛）
//!
//! 本文件曾自带 `indent` / `outdent` 的实现，与领域
//! `EditingService::indent_lines` / `outdent_lines` 形成**双份真相**——
//! 而领域版更完备（处理制表符、缩进不给空行留尾随空白）。
//! S6-07 已把这两处收敛回领域，本文件只保留：
//!
//! - **坐标系换算**（`char_to_byte` / `byte_to_char`）：egui 只认字符索引，
//!   领域只认字节偏移，这个鸿沟只能由表现层填；
//! - **egui 特有逻辑**（`diff_replace` / `transform_insert`）：与 `TextEdit`
//!   的撤销栈、括号配对、列表续行走同一条 `keypress` 通路，搬不进领域。
//!
//! # 模块结构
//!
//! 坐标换算与邻近文本工具在 `cursor`，行级变换（移动行 / 列表标记）在 `line`，
//! 编辑差异与插入自动变换在 `transform`。facade 原路径 re-export 全部公开项
//! 并保留单元测试。

mod cursor;
mod line;
mod transform;

pub use cursor::*;
pub use line::*;
pub use transform::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_char_roundtrip_multibyte() {
        // 字符索引：你(0) 好(1) a(2) 𝄞(3) b(4)
        // 字节偏移：好=3，a=6，𝄞=7（4 字节），b=11，总长 12
        let t = "你好a𝄞b";
        assert_eq!(char_to_byte(t, 0), 0);
        assert_eq!(byte_to_char(t, 0), 0);
        assert_eq!(char_to_byte(t, 1), 3); // 好
        assert_eq!(byte_to_char(t, 3), 1);
        assert_eq!(char_to_byte(t, 2), 6); // a
        assert_eq!(byte_to_char(t, 6), 2);
        assert_eq!(char_to_byte(t, 3), 7); // 𝄞（4 字节）
        assert_eq!(byte_to_char(t, 7), 3);
        assert_eq!(char_to_byte(t, 4), 11); // b
        assert_eq!(byte_to_char(t, 11), 4);
        assert_eq!(char_to_byte(t, t.chars().count()), t.len());
    }

    #[test]
    fn diff_pure_insert_at_middle() {
        let old = "ab";
        let new = "aXb";
        let e = diff_replace(old, new).unwrap();
        assert_eq!(e.byte_start, 1);
        assert_eq!(e.byte_end, 1);
        assert_eq!(e.inserted, "X");
        assert_eq!(e.cursor_char, 2);
    }

    #[test]
    fn diff_multibyte_insert_keeps_byte_boundaries() {
        // 在「你」与「好」之间插入 X（不能把中文字节切开）
        let old = "你好";
        let new = "你X好";
        let e = diff_replace(old, new).unwrap();
        assert_eq!(e.byte_start, 3);
        assert_eq!(e.byte_end, 3);
        assert_eq!(e.inserted, "X");
        // 用字节区间切片 old 必须合法且恰好是空
        assert_eq!(&old[e.byte_start..e.byte_end], "");
        // 替换后拼回应等于 new
        let rebuilt = format!(
            "{}{}{}",
            &old[..e.byte_start],
            e.inserted,
            &old[e.byte_end..]
        );
        assert_eq!(rebuilt, new);
    }

    #[test]
    fn diff_replace_across_multibyte() {
        let old = "你好世界";
        let new = "你X界"; // 删掉「好世」，插入 X
        let e = diff_replace(old, new).unwrap();
        let rebuilt = format!(
            "{}{}{}",
            &old[..e.byte_start],
            e.inserted,
            &old[e.byte_end..]
        );
        assert_eq!(rebuilt, new);
    }

    #[test]
    fn move_line_down_swaps() {
        let text = "a\nb\nc";
        let (out, cur) = move_line(text, 0, 1).unwrap();
        assert_eq!(out, "b\na\nc");
        assert_eq!(cur, 2); // 原 'a' 现位于第 2 行起点
    }

    #[test]
    fn move_line_up_at_top_is_noop() {
        let text = "a\nb";
        assert!(move_line(text, 0, -1).is_none());
    }

    #[test]
    fn list_marker_unordered() {
        let (indent_len, marker) = list_marker("  - hello").unwrap();
        assert_eq!(indent_len, 2);
        assert_eq!(marker, "  - ");
    }

    #[test]
    fn list_marker_ordered_increments() {
        let (_, marker) = list_marker("1. first").unwrap();
        assert_eq!(marker, "2. ");
    }

    #[test]
    fn list_marker_multi_digit_increments() {
        // 两位数序号：整段数字都要剥出来。只剥一位的话 `10. ` 会被看成
        // `0. `，`. ` 前缀匹配不上 → 回车续不出 `11. `（V11-13 实测）。
        let (mlen, marker) = list_marker("10. 第十条").unwrap();
        assert_eq!(marker, "11. ");
        assert_eq!(mlen, 4); // 缩进 0 + "10" + "." + " "
        let (mlen2, marker2) = list_marker("123) deep").unwrap();
        assert_eq!(marker2, "124) ");
        assert_eq!(mlen2, 5);
    }

    #[test]
    fn matching_close_pairs() {
        assert_eq!(matching_close('('), Some(')'));
        assert_eq!(matching_close('{'), Some('}'));
        assert_eq!(matching_close('"'), Some('"'));
        assert_eq!(matching_close('x'), None);
    }

    #[test]
    fn transform_bracket_pair_inserts_close_and_places_cursor() {
        // 用户在「ab」光标 1 处输入 '('
        let old = "ab";
        let mut edit = TextEdit {
            byte_start: 1,
            byte_end: 1,
            inserted: "(".to_string(),
            cursor_char: 2,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "()");
        // 光标停在 '(' 之后（索引 2，字符层面）
        assert_eq!(edit.cursor_char, 2);
    }

    #[test]
    fn transform_list_continuation_appends_marker() {
        // 在一行无序列表项末尾回车
        let old = "- hello";
        let mut edit = TextEdit {
            byte_start: 7,
            byte_end: 7,
            inserted: "\n".to_string(),
            cursor_char: 8,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "\n- ");
        assert_eq!(edit.cursor_char, 8 + 2);
    }

    #[test]
    fn transform_noop_for_plain_insert() {
        let old = "ab";
        let mut edit = TextEdit {
            byte_start: 1,
            byte_end: 1,
            inserted: "x".to_string(),
            cursor_char: 2,
            anchor_char: None,
        };
        assert!(!transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "x");
    }

    #[test]
    fn transform_empty_list_item_exits_list() {
        // 光标点在一个空无序列表项 "- " 的末尾（字符 2 / 字节 2）。
        // 回车应「退出列表」：把 "- " 整段删掉、只留一个空行，光标落到该空行。
        let old = "- ";
        let mut edit = TextEdit {
            byte_start: 2,
            byte_end: 2,
            inserted: "\n".to_string(),
            cursor_char: 3,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        // 标记被移除、不再续出新的 "- "
        assert_eq!(edit.inserted, "\n");
        // 替换区间起点被拉到行首（标记起始），删掉整段 "- "
        assert_eq!(edit.byte_start, 0);
        // 光标落在空行上：行首字符索引 0 + 换行 1
        assert_eq!(edit.cursor_char, 1);
    }

    #[test]
    fn transform_indented_empty_list_item_exits() {
        // 缩进的空无序列表项 "  - "（2 空格 + "- " = 4 字节），光标在末尾。
        let old = "  - ";
        let mut edit = TextEdit {
            byte_start: 4,
            byte_end: 4,
            inserted: "\n".to_string(),
            cursor_char: 5,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        // 标记被移除、不再续出新的 "- "
        assert_eq!(edit.inserted, "\n");
        // 替换区间起点被拉到行首（标记起始），删掉整段 "  - "
        assert_eq!(edit.byte_start, 0);
        // 光标落在空行上：行首字符索引 0 + 换行 1
        assert_eq!(edit.cursor_char, 1);
    }

    #[test]
    fn transform_empty_ordered_list_item_exits() {
        // 空有序列表项 "1. "（光标在末尾，字节 3），回车退出列表。
        let old = "1. ";
        let mut edit = TextEdit {
            byte_start: 3,
            byte_end: 3,
            inserted: "\n".to_string(),
            cursor_char: 4,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "\n");
        assert_eq!(edit.byte_start, 0);
        assert_eq!(edit.cursor_char, 1);
    }

    #[test]
    fn transform_nonempty_list_item_continues() {
        // 非空列表项仍续行：回车补出下一行标记。
        let old = "- hello";
        let mut edit = TextEdit {
            byte_start: 7,
            byte_end: 7,
            inserted: "\n".to_string(),
            cursor_char: 8,
            anchor_char: None,
        };
        assert!(transform_insert(old, &mut edit));
        assert_eq!(edit.inserted, "\n- ");
        // 起点不变（只续行，不删已有内容）
        assert_eq!(edit.byte_start, 7);
    }
}
