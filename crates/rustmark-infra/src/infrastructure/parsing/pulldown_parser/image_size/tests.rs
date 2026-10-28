//! `image_size` 门面测试（从主文件内联 `mod tests` 迁出；R-07 Batch 1，零生产改动）。

use super::{desugar_image_sizes, image_display_at_source, parse_image_display};
use rustmark_core::domain::markdown::value_objects::{ImageAlign, ImageDisplay};

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
