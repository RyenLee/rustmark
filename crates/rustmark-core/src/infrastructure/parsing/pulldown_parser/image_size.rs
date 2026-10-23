//! 源码预处理：剥离 CSDN 私有扩展的图片尺寸后缀 `=宽x高`，
//! 并维护「预处理后偏移 → 原文偏移」映射。
//!
//! 脱糖只负责「让 pulldown 认得出图片」；被剥掉的 `#pic_center` /
//! ` =30x30` 语义并未丢弃——[`parse_image_display`] 与
//! [`image_display_at_source`] 在**同一套识别规则**上把它们解析成
//! [`ImageDisplay`]，随块上行到渲染层（问题 3：图片必须按参数区分
//! 尺寸与对齐，而不是所有图片一个样）。
//!
//! # 参数规则表（自定义参数的入口）
//!
//! 图片目标地址末尾可带两类私有扩展参数，**顺序无关、可叠加**：
//!
//! | 规则 | 形态 | 识别位置 |
//! |------|------|----------|
//! | 对齐 | `#pic_center` / `#pic_left` / `#pic_right` / `#pic_float` | 地址内任意位置 |
//! | 尺寸 | ` =宽x高` | 仅地址结尾（其后只允许再有对齐令牌） |
//!
//! 于是 `url#pic_center =30x30`（先对齐后尺寸，CSDN 工具栏生成顺序）与
//! `url =30x30#pic_center`（先尺寸后对齐）解析结果完全一致。
//!
//! 脱糖（剥除）与解析（取值）共用同一组规则函数：**新增一条参数规则只需
//! 改本文件一处**（对齐令牌加一行到 [`PIC_ALIGN_TOKENS`]，尺寸形态在
//! [`size_suffix_len`] 扩展），两条路径同时生效，不存在「剥了却没解析」
//! 的漂移——不变式由 `desugar_and_parse_agree_on_the_same_rules` 测试守住。

use crate::domain::markdown::value_objects::{ImageAlign, ImageDisplay};

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

/// 对齐规则表：地址内**任意位置**出现 `token` 即按 `align` 摆放。
///
/// # 自定义参数规则
///
/// 新增一条对齐规则 = 往本表追加一行（如 `("#pic_center_small", ...)`）：
/// 脱糖的剥除与解析的取值都遍历本表，自动同时生效。注意表项**按顺序**
/// 匹配，具体令牌要排在互为前缀的泛化令牌之前。
const PIC_ALIGN_TOKENS: &[(&str, ImageAlign)] = &[
    ("#pic_center", ImageAlign::Center),
    ("#pic_left", ImageAlign::Left),
    ("#pic_right", ImageAlign::Right),
    // 浮动（文字环绕）在 egui 里没有对应版式，降为左对齐：
    // 图片贴在左边，至少「不居中」的位置语义不失真。
    ("#pic_float", ImageAlign::Left),
];

/// 若 `s` 以某个 Typora 对齐后缀开头，返回其长度（含前导 `#`）。
fn pic_align_suffix_len(s: &str) -> Option<usize> {
    PIC_ALIGN_TOKENS
        .iter()
        .find_map(|(t, _)| s.starts_with(t).then_some(t.len()))
}

/// `pos` 处是否位于某个链接 / 图片的**无引号地址**之内。
///
/// 判据：向前找最近的 `](`，且它与 `pos` 之间不含空白或括号——
/// 若含空白，说明 `pos` 已经越出了地址（比如落在正文里），
/// 那就绝不能改写它，否则会把正常文字改坏。
///
/// 唯一例外：地址尾部带尺寸后缀（`url =30x30`）。先设尺寸再设对齐时，
/// 对齐令牌跟在尺寸后面（`url =30x30#pic_center`），回看这段必须放行——
/// 此时的空白只允许出现在尺寸前缀处，且尺寸必须完整（`=宽x高`）。
fn in_link_destination(text: &str, pos: usize) -> bool {
    let Some(open) = text[..pos].rfind("](") else {
        return false;
    };
    let dest = &text[open + 2..pos];
    if !dest.contains([' ', '\t', '\n', '\r', '(', ')']) {
        return true;
    }
    dest.rsplit_once(' ').is_some_and(|(head, size)| {
        !head.is_empty()
            && !head.contains([' ', '\t', '\n', '\r', '(', ')'])
            && size_suffix_len(size).is_some_and(|len| len == size.len())
    })
}

/// 尺寸后缀之后是否紧跟「地址结束」：`)`，或一个**完整的**对齐令牌
/// （先设尺寸再设对齐时 `#pic_center` 跟在 ` =30x30` 后面，
/// 如 `![a](url =30x30#pic_center)`）。其余形态一律不放行——与解析侧
/// 「尺寸后缀只认地址结尾」的判据同源。
fn closes_destination(text: &str, pos: usize) -> bool {
    match text.as_bytes().get(pos) {
        Some(b')') => true,
        Some(b'#') => pic_align_suffix_len(&text[pos..])
            .is_some_and(|len| text.as_bytes().get(pos + len) == Some(&b')')),
        _ => false,
    }
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
                && closes_destination(text, i + 1 + len)
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
    pub(crate) fn to_original(&self, offset: usize) -> usize {
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

/// 从**原文**里图片语法的目标地址（`](` 与 `)` 之间的文本）提取显示参数。
///
/// 与 [`desugar_image_sizes`] 同源：脱糖剥掉什么，这里就解析出什么——
/// 对齐标记取地址里出现的 Typora 令牌，尺寸后缀只认**贴在地址结尾**的
/// ` =宽x高`（与脱糖的「后随 `)` 或完整对齐令牌」判据一致）。
///
/// 两类参数**顺序无关、可叠加**：`url#pic_center =30x30`（先对齐后尺寸，
/// CSDN 工具栏生成顺序）与 `url =30x30#pic_center`（先尺寸后对齐，
/// 「位置标记加在最后」）解析结果完全一致。
///
/// 输入示例：`https://x/y.png#pic_center =30x30` →
/// `ImageDisplay { width: Some(30), height: Some(30), align: Center }`。
pub(crate) fn parse_image_display(dest: &str) -> ImageDisplay {
    let mut display = ImageDisplay::default();
    // 对齐：地址内任意位置的 `#pic_*` 令牌（脱糖同款 starts_with 识别）。
    for (token, align) in PIC_ALIGN_TOKENS {
        if dest.contains(*token) {
            display.align = *align;
            break;
        }
    }
    // 尺寸：只认贴在地址结尾的 ` =宽x高`，其后只允许再有**一个完整的**
    // 对齐令牌（反向顺序 ` =30x30#pic_center`）。从左往右取第一个满足
    // 条件的 ` =`——与脱糖从左往右扫、只有后随 `)`/令牌那段才剥等价；
    // ` =30x30 more` 这种中间形态不是尺寸声明。
    let mut from = 0;
    while let Some(off) = dest[from..].find(" =") {
        let idx = from + off;
        let after = &dest[idx + 1..];
        if let Some(len) = size_suffix_len(after) {
            let rest = &after[len..];
            let closes =
                rest.is_empty() || pic_align_suffix_len(rest).is_some_and(|l| l == rest.len());
            if closes {
                // `宽x高`：任一段不是正整数则整体忽略——半组尺寸没有意义，
                // 不如按纹理实际尺寸渲染。
                let spec = &after[1..len];
                if let Some((w, h)) = spec.split_once('x')
                    && let (Ok(width), Ok(height)) = (w.parse::<u32>(), h.parse::<u32>())
                {
                    display.width = Some(width);
                    display.height = Some(height);
                }
                break;
            }
        }
        from = idx + 1;
    }
    display
}

/// 在**原文**里提取一张图的显示参数（问题 3）。
///
/// # 锚点为什么精确
///
/// pulldown 的 `OffsetIter` 对行内节点发出的 Start / End 事件携带**同一个**
/// span（`parse.rs`：`item.start..item.end`），而 Image 节点的 `end` 在
/// `scan_inline_link` 成功时被设为 `)` 之后的位置。该 span 经
/// [`SourceOffsets::to_original`] 还原后仍指向**原文**中本图 `)` 之后，
/// 故回退一个字节即是右括号，`](` 到它之间就是完整的目标地址
/// （含 `#pic_center` / ` =30x30` 后缀——脱糖只动了副本，原文完好）。
///
/// 任何一步对不上（源码残缺、偏移异常）都退回 [`ImageDisplay::default`]：
/// 丢参数只会退化成「按实际尺寸、居中渲染」，不会把别的图的参数
/// 安到本图头上。
pub fn image_display_at_source(text: &str, close_paren: usize) -> ImageDisplay {
    // `get(..)` 而非 `[..]`：偏移越界或不在字符边界时安全返回 None。
    let Some(before_paren) = text.get(..close_paren).and_then(|s| s.strip_suffix(')')) else {
        return ImageDisplay::default();
    };
    let Some(open) = before_paren.rfind("](") else {
        return ImageDisplay::default();
    };
    let dest = &before_paren[open + 2..];
    parse_image_display(dest)
}

#[cfg(test)]
mod tests {
    use super::{desugar_image_sizes, image_display_at_source, parse_image_display};
    use crate::domain::markdown::value_objects::{ImageAlign, ImageDisplay};

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

    /// 反向顺序（先尺寸后对齐）：` =30x30#pic_center` 两段都要剥掉，
    /// 否则无引号地址一遇空白即终止，pulldown 直接把整段当普通文本。
    #[test]
    fn strips_size_then_align_in_reverse_order() {
        let (src, _) = desugar_image_sizes("![a](http://x/y.png =30x30#pic_center)");
        assert_eq!(src, "![a](http://x/y.png)");
        // 段落里夹一个，前后的正文与另一张图都不能被误伤。
        let (src2, _) =
            desugar_image_sizes("见图 ![a](x.png =1x1#pic_left) 和 ![b](y.png#pic_right) 结束");
        assert_eq!(src2, "见图 ![a](x.png) 和 ![b](y.png) 结束");
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

    // ---- 显示参数提取（问题 3）----

    /// CSDN / Typora 全套语法：对齐令牌 + 尺寸后缀同时出现。
    #[test]
    fn parses_full_csdn_syntax() {
        let d = parse_image_display("https://imgconvert.csdnimg.cn/aHR0cHM#pic_center =30x30");
        assert_eq!(
            d,
            ImageDisplay {
                width: Some(30),
                height: Some(30),
                align: ImageAlign::Center,
            }
        );
    }

    /// 只写对齐标记（无尺寸）→ 只影响对齐。
    #[test]
    fn parses_align_only() {
        assert_eq!(
            parse_image_display("a.png#pic_left").align,
            ImageAlign::Left
        );
        assert_eq!(
            parse_image_display("a.png#pic_right").align,
            ImageAlign::Right
        );
        assert_eq!(
            parse_image_display("a.png#pic_center").align,
            ImageAlign::Center
        );
        // 尺寸字段保持 None。
        assert_eq!(parse_image_display("a.png#pic_left").width, None);
    }

    /// 只写尺寸（无对齐）→ 默认**左**对齐 + 尺寸生效（效果截图：
    /// 无标记图片贴在左缘，要居中必须显式写 `#pic_center`）。
    #[test]
    fn parses_size_only() {
        let d = parse_image_display("images/a.png =50x100");
        assert_eq!(d.width, Some(50));
        assert_eq!(d.height, Some(100));
        assert_eq!(d.align, ImageAlign::Left);
    }

    /// 反向顺序（先尺寸后对齐，「位置标记加在最后」）与 canonical 顺序
    /// 解析结果完全一致。
    #[test]
    fn parses_reverse_order_size_then_align() {
        assert_eq!(
            parse_image_display("images/a.png =30x30#pic_center"),
            ImageDisplay {
                width: Some(30),
                height: Some(30),
                align: ImageAlign::Center,
            }
        );
        assert_eq!(
            parse_image_display("images/a.png =50x100#pic_right"),
            ImageDisplay {
                width: Some(50),
                height: Some(100),
                align: ImageAlign::Right,
            }
        );
        // 与 canonical 顺序等价。
        assert_eq!(
            parse_image_display("images/a.png#pic_center =30x30"),
            parse_image_display("images/a.png =30x30#pic_center"),
        );
    }

    /// 尺寸后面跟**多个**令牌不是合法形态（解析不认，脱糖也不剥）。
    #[test]
    fn size_followed_by_two_tokens_is_rejected() {
        assert_eq!(
            parse_image_display("a.png =30x30#pic_left#pic_right").width,
            None
        );
    }

    /// 干净地址（无任何扩展）→ 全默认。
    #[test]
    fn parses_plain_destination() {
        assert_eq!(parse_image_display("images/a.png"), ImageDisplay::default());
        assert_eq!(
            parse_image_display("https://x.test/y.png"),
            ImageDisplay::default()
        );
    }

    /// 真实 URL 片段（`#section`）不是对齐令牌，不得改变对齐。
    #[test]
    fn real_fragment_is_not_align_token() {
        assert_eq!(
            parse_image_display("https://x.test/y.png#section"),
            ImageDisplay::default()
        );
    }

    /// 尺寸后缀必须贴在地址**结尾**：中间的 ` =30x30` 不是尺寸声明
    /// （脱糖同款判据：后随 `)`）。
    #[test]
    fn size_suffix_must_be_at_the_end() {
        assert_eq!(parse_image_display("a.png =30x30 more").width, None);
    }

    /// 半组尺寸（`=30x` / `=x30`）整体忽略，不落半组进字段。
    #[test]
    fn half_size_spec_is_ignored() {
        assert_eq!(parse_image_display("a.png =30x").width, None);
        assert_eq!(parse_image_display("a.png =x30").width, None);
        assert_eq!(parse_image_display("a.png =abc").width, None);
    }

    /// 锚点提取：`close_paren` 指向原文中 `)` 之后，回推即得完整地址。
    #[test]
    fn anchor_extracts_params_from_original_text() {
        let text = "带尺寸的图片: ![Alt](images/a.png#pic_center =30x30)\n";
        // `)` 位于 text 末端（不含换行）之前。
        let close = text.trim_end().len();
        assert_eq!(
            image_display_at_source(text, close),
            ImageDisplay {
                width: Some(30),
                height: Some(30),
                align: ImageAlign::Center,
            }
        );
    }

    /// 多图段落：每张图各按自己的 `)` 锚点提取，互不串扰。
    #[test]
    fn anchor_is_per_image_in_multi_image_paragraph() {
        let text = "![a](x.png =1x1) 中间文字 ![b](y.png#pic_left =2x2)";
        // 第一张图 `![a](x.png =1x1)` 长 16 字节，`)` 在索引 15，
        // 锚点取其**之后**的位置（与 `End(Image)` 区间终点同义）。
        assert_eq!(image_display_at_source(text, 16).width, Some(1));
        // 第二张图的 `)` 之后：整段末尾。
        let second = image_display_at_source(text, text.len());
        assert_eq!(second.width, Some(2));
        assert_eq!(second.height, Some(2));
        assert_eq!(second.align, ImageAlign::Left);
    }

    /// 反向顺序（先尺寸后对齐）的锚点提取同样精确。
    #[test]
    fn anchor_extracts_reverse_order_params() {
        let text = "![a](x.png =2x2#pic_left)";
        assert_eq!(
            image_display_at_source(text, text.len()),
            ImageDisplay {
                width: Some(2),
                height: Some(2),
                align: ImageAlign::Left,
            }
        );
    }

    /// 干净图片（无扩展）提取出默认参数，不影响渲染。
    #[test]
    fn anchor_on_plain_image_yields_default() {
        let text = "![a](images/a.png)";
        assert_eq!(
            image_display_at_source(text, text.len()),
            ImageDisplay::default()
        );
    }

    /// 源码残缺（无右括号 / 无地址）→ 默认参数，绝不 panic。
    #[test]
    fn anchor_on_broken_source_yields_default() {
        assert_eq!(
            image_display_at_source("![a](images/a.png", 19),
            ImageDisplay::default()
        );
        assert_eq!(
            image_display_at_source("没有图片", 12),
            ImageDisplay::default()
        );
        // 越界偏移同样安全。
        assert_eq!(
            image_display_at_source("![a](x.png)", 999),
            ImageDisplay::default()
        );
    }

    /// 同源不变式：脱糖剥掉的后缀，正好是解析认出的参数——两条路径共用
    /// 一套规则。任何新规则（自定义参数）只改一边，就会在这里挂。
    #[test]
    fn desugar_and_parse_agree_on_the_same_rules() {
        // (目标地址, 期望的洗净地址, 期望的显示参数)
        let cases: [(&str, &str, ImageDisplay); 7] = [
            ("images/a.png", "images/a.png", ImageDisplay::default()),
            (
                "images/a.png#pic_center",
                "images/a.png",
                ImageDisplay {
                    width: None,
                    height: None,
                    align: ImageAlign::Center,
                },
            ),
            (
                "images/a.png#pic_left =50x100",
                "images/a.png",
                ImageDisplay {
                    width: Some(50),
                    height: Some(100),
                    align: ImageAlign::Left,
                },
            ),
            (
                "images/a.png =50x100#pic_right",
                "images/a.png",
                ImageDisplay {
                    width: Some(50),
                    height: Some(100),
                    align: ImageAlign::Right,
                },
            ),
            (
                "images/a.png =30x30",
                "images/a.png",
                ImageDisplay {
                    width: Some(30),
                    height: Some(30),
                    align: ImageAlign::Left,
                },
            ),
            // 真实 URL 片段不是参数：原样保留。
            (
                "https://x.test/y.png#section",
                "https://x.test/y.png#section",
                ImageDisplay::default(),
            ),
            // 尺寸不在地址结尾：不是尺寸声明，不剥也不认。
            (
                "images/a.png =30x30 more",
                "images/a.png =30x30 more",
                ImageDisplay::default(),
            ),
        ];
        for (dest, clean, display) in cases {
            let text = format!("![a]({dest})");
            let (src, _) = desugar_image_sizes(&text);
            assert_eq!(src, format!("![a]({clean})"), "脱糖结果与规则不符：{dest}");
            assert_eq!(
                parse_image_display(dest),
                display,
                "解析结果与规则不符：{dest}"
            );
            // 剥干净之后不应再剩任何参数（否则脱糖不完整）。
            assert_eq!(
                parse_image_display(clean),
                ImageDisplay::default(),
                "洗净地址仍能解析出参数：{dest}"
            );
        }
    }
}
