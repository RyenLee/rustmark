//! 文档统计的**链接 / 图片计数**（字节层纯函数）。
//!
//! 从 [`super`] 分出来的理由：整篇统计的第二遍扫描只做这一件事，且是纯函数
//! （`&[u8] -> (usize, usize)`）。它与字符层的累加器**只共享结果不共享状态**，
//! 分开后各自都能单独测——尤其是括号嵌套配对那条容易静默退化的规则。
//!
//! 图片 `![说明](地址)` **不计入**链接数（原型口径），否则一张图会在「链接」
//! 和「图片」两栏各数一次。

/// 链接与图片数（字节层匹配）。
///
/// 仅在 `[` / `!` 处做括号配对，其余字符 O(1) 跳过。与
/// `equivalence::ref_count_links_and_images` 口径一致（同一份匹配规则），
/// 故链接 / 图片计数与旧实现必然逐项相等。
pub(super) fn count_links_and_images(bytes: &[u8]) -> (usize, usize) {
    let mut links = 0;
    let mut images = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' && bytes[i] != b'!' {
            i += 1;
            continue;
        }
        let is_image = bytes[i] == b'!';
        let open = if is_image {
            if bytes.get(i + 1) != Some(&b'[') {
                i += 1;
                continue;
            }
            i + 1
        } else {
            i
        };
        match link_tail_len(&bytes[open..]) {
            Some(len) => {
                if is_image {
                    images += 1;
                } else {
                    links += 1;
                }
                i = open + len;
            }
            None => i += 1,
        }
    }
    (links, images)
}

/// 从 `[` 开始匹配到配对的 `)`，返回**含** `[` 与 `)` 的总长度。
///
/// 括号可嵌套：`[文字](a(b)c)` 是合法链接，扫到第一个 `)` 就停会把它
/// 误拆成「链接 + 残余」两段。链接地址里的 `)` 不转义时按嵌套配对处理。
pub(super) fn link_tail_len(bytes: &[u8]) -> Option<usize> {
    debug_assert_eq!(bytes.first(), Some(&b'['));
    // 找到配对的 `]`（`]` 可出现在链接文字里，故记深度）。
    let mut i = 1;
    let mut bracket_depth = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => bracket_depth += 1,
            b']' => {
                bracket_depth -= 1;
                if bracket_depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if i >= bytes.len() {
        return None; // 没有 `]`
    }
    if bytes.get(i + 1) != Some(&b'(') {
        return None; // `[]` 后面不是 `(` → 不是链接（可能是引用式 `[^1]`）
    }
    // 从 `(` 起算括号深度。
    i += 2;
    let mut paren_depth = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => paren_depth += 1,
            b')' => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None // 括号没闭合
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 链接尾长度：含 `[` 与 `)` 的总长，括号可嵌套。
    ///
    /// `[带括号](d(e)f)` 是最容易退化的一条——不记括号深度就会在
    /// 第一个 `)` 处收尾，把它拆成「链接 + 残余」。
    #[test]
    fn link_tail_len_counts_to_the_matching_paren() {
        assert_eq!(link_tail_len(b"[a](b)"), Some(6));
        // 多字节的链接文字：返回值是**字节**长度，含 `[` 与配对的 `)`。
        let nested = "[带括号](d(e)f)";
        assert_eq!(link_tail_len(nested.as_bytes()), Some(nested.len()));
        // 链接文字里的 `[` 必须记深度：`[a[b]](c)` 配对的 `]` 是第二个，
        // 不记深度会在第一个 `]` 收尾，把整条判成非链接。
        let nested_bracket = "[a[b]](c)";
        assert_eq!(
            link_tail_len(nested_bracket.as_bytes()),
            Some(nested_bracket.len())
        );
        assert_eq!(
            link_tail_len("[裸括号]".as_bytes()),
            None,
            "] 后没有 ( 不是链接"
        );
        assert_eq!(link_tail_len(b"[^1]"), None, "引用式脚注不是链接");
        assert_eq!(link_tail_len("[未闭合](a".as_bytes()), None, "括号没闭合");
    }

    /// 图片不计入链接；`!` 后不是 `[` 时按普通字符跳过。
    #[test]
    fn images_are_not_counted_as_links() {
        let (links, images) = count_links_and_images("[a](b) ![图](c.png)".as_bytes());
        assert_eq!((links, images), (1, 1), "同一张图不得两栏各数一次");
        assert_eq!(count_links_and_images("![图](c.png)".as_bytes()), (0, 1));
        assert_eq!(
            count_links_and_images("! 感叹号后不是中括号".as_bytes()),
            (0, 0)
        );
        assert_eq!(count_links_and_images("[裸括号] [^1]".as_bytes()), (0, 0));
    }
}
