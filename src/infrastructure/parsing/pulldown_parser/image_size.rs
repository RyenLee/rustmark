//! 源码预处理：剥离 CSDN 私有扩展的图片尺寸后缀 `=宽x高`，
//! 并维护「预处理后偏移 → 原文偏移」映射。

/// `=宽x高` 的长度（含前导 `=`，不含尾随的 `)`）；不是该形状时返回 `None`。
///
/// 尺寸后缀是 CSDN 的私有扩展（`![alt](url =30x30)`），不是 Markdown。
fn size_suffix_len(s: &str) -> Option<usize> {
    let rest = s.strip_prefix('=')?;
    let digits = |t: &str| t.bytes().take_while(u8::is_ascii_digit).count();
    let w = digits(rest);
    if w == 0 {
        return None;
    }
    let rest = rest[w..].strip_prefix('x')?;
    let h = digits(rest);
    if h == 0 {
        return None;
    }
    Some(1 + w + 1 + h)
}

/// Typora 私有扩展的图片对齐后缀（`#pic_center` / `#pic_left` /
/// `#pic_right` / `#pic_float`）。这些不是 Markdown，会让 `url#pic_center`
/// 把 `pic_center` 当成 URL 片段带进加载器，本地图片因此「文件不存在」、
/// 远程图片则把片段拼进请求路径，两种都加载失败。
const PIC_ALIGN_TOKENS: &[&str] = &["#pic_center", "#pic_left", "#pic_right", "#pic_float"];

/// 若 `s` 以某个 Typora 对齐后缀开头，返回其长度（含前导 `#`）。
fn pic_align_suffix_len(s: &str) -> Option<usize> {
    PIC_ALIGN_TOKENS
        .iter()
        .find_map(|t| s.starts_with(t).then_some(t.len()))
}

/// `pos` 处是否位于某个链接 / 图片的**无引号地址**之内。
///
/// 判据：向前找最近的 `](`，且它与 `pos` 之间不含空白或括号——
/// 若含空白，说明 `pos` 已经越出了地址（比如落在正文里），
/// 那就绝不能改写它，否则会把正常文字改坏。
fn in_link_destination(text: &str, pos: usize) -> bool {
    let Some(open) = text[..pos].rfind("](") else {
        return false;
    };
    let dest = &text[open + 2..pos];
    !dest.contains([' ', '\t', '\n', '\r', '(', ')'])
}

/// 源码预处理：删掉 CSDN 风格的图片尺寸后缀 ` =宽x高`，并给出偏移映射表。
///
/// # 为什么必须处理
///
/// `![alt](url =30x30)` 不是合法 Markdown：无引号的链接地址**一遇空白即终止**，
/// `=30x30` 于是落进括号里成为语法错误，pulldown 索性把整段按**普通文本**吐出——
/// 用户在预览里看到的就是「一长串原始 Markdown，还带着 base64 图片地址」。
/// 删掉这段后缀后，`![alt](url)` 是标准图片语法，pulldown 正常产出 `Tag::Image`，
/// 预览与导出都能显示图片。
///
/// # 为什么同时返回映射表
///
/// 删除会改变字节偏移，而每个块自带的 `source_range` 必须指向**原文**
/// （点击跳转、滚动同步、增量重解析全靠它）。调用方用 [`SourceOffsets`]
/// 把 pulldown 给出的偏移还原回原文坐标即可。没有可删时返回借用，
/// 不做任何分配。
pub fn desugar_image_sizes(text: &str) -> (std::borrow::Cow<'_, str>, SourceOffsets) {
    let mut removals: Vec<(usize, usize)> = Vec::new();
    if text.contains(" =") || text.contains("#pic_") {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        // `copied` 是原文中已拷进 `out` 的位置。
        let mut copied = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            let byte = bytes[i];
            // ` =宽x高`：空格 + `=` 触发，仅限链接地址内。
            if byte == b' '
                && in_link_destination(text, i)
                && let Some(len) = size_suffix_len(&text[i + 1..])
                && bytes.get(i + 1 + len) == Some(&b')')
            {
                out.push_str(&text[copied..i]);
                // 记的是「处理后」的偏移，供还原用。
                removals.push((out.len(), 1 + len));
                i += 1 + len;
                copied = i;
                continue;
            }
            // `#pic_center` 等对齐后缀：直接接在 URL 之后（无空格），同样只在
            // 链接地址内才剥，避免误伤正文里的 `#`。其后是 `)`，无需特判。
            if byte == b'#'
                && in_link_destination(text, i)
                && let Some(len) = pic_align_suffix_len(&text[i..])
            {
                out.push_str(&text[copied..i]);
                removals.push((out.len(), len));
                i += len;
                copied = i;
                continue;
            }
            i += 1;
        }
        if !removals.is_empty() {
            out.push_str(&text[copied..]);
            return (std::borrow::Cow::Owned(out), SourceOffsets { removals });
        }
    }
    (
        std::borrow::Cow::Borrowed(text),
        SourceOffsets {
            removals: Vec::new(),
        },
    )
}

/// [`desugar_image_sizes`] 的偏移映射表。
pub struct SourceOffsets {
    /// `(预处理后文本中的偏移, 该处被删掉的字节数)`，按偏移升序。
    removals: Vec<(usize, usize)>,
}

impl SourceOffsets {
    /// 把「预处理后文本」的偏移还原成「原文」偏移。
    pub fn to_original(&self, offset: usize) -> usize {
        offset
            + self
                .removals
                .iter()
                .take_while(|(at, _)| *at <= offset)
                .map(|(_, len)| *len)
                .sum::<usize>()
    }

    /// 是否发生过预处理（没发生时可跳过一切还原开销）。
    pub fn is_identity(&self) -> bool {
        self.removals.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::desugar_image_sizes;

    /// `=宽x高` 后缀被剥除，且保留正常的图片语法。
    #[test]
    fn strips_size_suffix() {
        let (src, off) = desugar_image_sizes("![a](http://x/y.png =30x30)");
        assert_eq!(src, "![a](http://x/y.png)");
        assert!(!off.is_identity());
    }

    /// Typora 对齐后缀 `#pic_center` 等被剥除，URL 还原干净。
    #[test]
    fn strips_pic_align_suffix() {
        let (src, _) = desugar_image_sizes("![a](http://x/y.png#pic_center)");
        assert_eq!(src, "![a](http://x/y.png)");
        let (src2, _) = desugar_image_sizes("![a](http://x/y.png#pic_left)");
        assert_eq!(src2, "![a](http://x/y.png)");
    }

    /// 真实 URL 片段（非对齐令牌）不得被误删。
    #[test]
    fn keeps_real_fragment() {
        let (src, off) = desugar_image_sizes("![a](http://x/y.png#section)");
        assert_eq!(src, "![a](http://x/y.png#section)");
        assert!(off.is_identity());
    }

    /// 正文里的 `=数字` 不在链接地址内，绝不可改写。
    #[test]
    fn does_not_touch_body_text() {
        let (src, off) = desugar_image_sizes("尺寸 =30x30 不是图片后缀");
        assert_eq!(src, "尺寸 =30x30 不是图片后缀");
        assert!(off.is_identity());
    }
}
