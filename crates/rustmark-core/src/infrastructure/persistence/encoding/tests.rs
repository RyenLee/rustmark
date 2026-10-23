use super::gb18030::*;
use super::gbk::*;
use super::writer::gbk_reverse;
use super::*;

/// 把字符编成 GBK 字节，供测试构造输入。
///
/// ASCII 直接取字节（GBK 与之完全兼容）；其余字符在表里反查 (首, 尾)。
/// 查不到的字符会被跳过——测试文本应只含 GBK 可表示的字符。
fn gbk_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in text.chars() {
        if (ch as u32) < 0x80 {
            out.push(ch as u8);
            continue;
        }
        for lead in 0x81u8..=0xFE {
            let mut found = None;
            for trail in (0x40u8..=0x7E).chain(0x80..=0xFE) {
                if gbk_lookup(lead, trail) == Some(ch) {
                    found = Some(trail);
                    break;
                }
            }
            if let Some(trail) = found {
                out.push(lead);
                out.push(trail);
                break;
            }
        }
    }
    out
}

#[test]
fn plain_utf8_is_detected_and_decoded() {
    let (text, enc) = decode("中文与 English".as_bytes());
    assert_eq!(text, "中文与 English");
    assert_eq!(enc, Encoding::Utf8);
    assert!(!enc.keeps_bom());
}

#[test]
fn utf8_bom_is_stripped_and_remembered() {
    let mut bytes = UTF8_BOM.to_vec();
    bytes.extend_from_slice("带 BOM 的内容".as_bytes());
    let (text, enc) = decode(&bytes);
    assert_eq!(text, "带 BOM 的内容", "BOM 不应残留在正文里");
    assert_eq!(enc, Encoding::Utf8Bom);
    assert!(enc.keeps_bom(), "原本带 BOM 的文件保存时应补回 BOM");
}

/// 核心用例：GBK 文件不再产生替换字符。
#[test]
fn gbk_chinese_decodes_without_replacement_chars() {
    let bytes = gbk_bytes("中文测试，编辑器");
    assert!(!bytes.is_empty(), "构造的 GBK 字节不应为空（前提检查）");
    // 前提：这些字节确实不是合法 UTF-8，否则测的就不是 GBK 分支。
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "构造的字节恰好是合法 UTF-8，无法验证 GBK 分支"
    );

    let (text, enc) = decode(&bytes);
    assert_eq!(enc, Encoding::Gbk);
    assert_eq!(text, "中文测试，编辑器");
    assert!(
        !text.contains('\u{fffd}'),
        "GBK 解码不应产生替换字符，实际得到：{text}"
    );
}

/// 中英混排：GBK 的 ASCII 区与 UTF-8 兼容，必须原样保留。
#[test]
fn gbk_mixed_with_ascii_keeps_ascii_intact() {
    let bytes = gbk_bytes("文档 README 说明");
    let (text, _) = decode(&bytes);
    assert_eq!(text, "文档 README 说明");
}

#[test]
fn utf16_little_endian_bom_is_decoded() {
    let text = "记事本另存为 Unicode";
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let (decoded, enc) = decode(&bytes);
    assert_eq!(decoded, text);
    assert_eq!(enc, Encoding::Utf16Le);
}

#[test]
fn utf16_big_endian_bom_is_decoded() {
    let text = "大端序";
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    let (decoded, enc) = decode(&bytes);
    assert_eq!(decoded, text);
    assert_eq!(enc, Encoding::Utf16Be);
}

/// 奇数长度的 UTF-16 不能 panic，且要露出替换字符提示尾部损坏。
#[test]
fn truncated_utf16_does_not_panic() {
    let bytes = vec![0xFF, 0xFE, 0x41, 0x00, 0x42];
    let (text, enc) = decode(&bytes);
    assert_eq!(enc, Encoding::Utf16Le);
    assert!(text.starts_with('A'));
    assert!(text.contains('\u{fffd}'), "截断的半个码元应显式提示");
}

/// 任意字节输入都不能 panic——这是编辑器"什么文件都要能打开"的底线。
#[test]
fn arbitrary_bytes_never_panic() {
    let cases: &[&[u8]] = &[
        &[],
        &[0x00],
        &[0xFF],
        &[0xFF, 0xFF, 0xFF],
        &[0x81],             // 落单的 GBK 首字节
        &[0x81, 0x20],       // 非法尾字节
        &[0xB2, 0xE2, 0x81], // 正常汉字后跟落单首字节
        &[0x80],             // CP936 欧元符号
    ];
    for case in cases {
        let (text, _) = decode(case);
        // 只要求不 panic；空输入应得到空串。
        if case.is_empty() {
            assert!(text.is_empty());
        }
    }
}

/// 未定义但尾字节合法的组合，整对作废且只产出**一个**替换字符。
#[test]
fn undefined_gbk_pair_yields_one_replacement_char() {
    // 动态找一处"尾字节合法、但组合未定义"的空洞，而非硬编码——
    // 硬编码会随映射表重新生成而失效（如 0x81 0x40 实际是「丂」）。
    let (lead, trail) = (0x81u8..=0xFE)
        .find_map(|l| {
            (0x40u8..=0x7E)
                .chain(0x80..=0xFE)
                .find(|&t| gbk_lookup(l, t).is_none())
                .map(|t| (l, t))
        })
        .expect("GBK 表中应存在未定义组合（前提检查）");
    assert!(is_gbk_trail(trail), "找到的尾字节应在合法范围内");

    let (text, enc) = decode(&[lead, trail, b'A']);
    assert_eq!(enc, Encoding::Gbk);
    assert_eq!(
        text, "\u{fffd}A",
        "整对应作废成一个替换字符，且不影响其后的 ASCII"
    );
}

/// 表规模与实现假设一致：改了生成脚本而忘了改常量，这里会失败。
#[test]
fn table_size_matches_declared_dimensions() {
    assert_eq!(
        GBK_TABLE.len(),
        GBK_ROWS * GBK_COLS * 2,
        "映射表尺寸与 GBK_ROWS × GBK_COLS 不符"
    );
}

/// **回归防线**：大部分合法、仅零星坏字节的 UTF-8，必须仍按 UTF-8 处理。
///
/// 早先的实现会直接把这类文件整体交给 GBK 解码，后果是把**原本正确的中文
/// 二次解码**成满篇看似正常却全错的汉字——比替换字符危险得多，
/// 因为用户看不出问题，一保存就毁掉原文。
#[test]
fn mostly_valid_utf8_with_few_bad_bytes_keeps_its_chinese() {
    let mut bytes = "# 中文".as_bytes().to_vec();
    bytes.extend_from_slice(&[0xFF, 0xFE, 0x80]); // 零星坏字节
    bytes.extend_from_slice("\n\ntail\n".as_bytes());

    let (text, enc) = decode(&bytes);
    assert_eq!(enc, Encoding::Utf8, "应判为（损坏的）UTF-8，而不是 GBK");
    assert!(
        text.contains("中文"),
        "坏字节之前的中文必须原样保留，实际：{text:?}"
    );
    assert!(text.contains("tail"), "坏字节之后的正文必须保留");
    assert!(
        text.contains('\u{fffd}'),
        "坏字节应显式标记为替换字符，而不是被悄悄吞掉"
    );
}

/// 判据分档：真 UTF-8 的三字节序列多于双字节；GBK 反之。
///
/// 这是"UTF-8 还是 GBK"的分叉点，直接决定文件是否被正确打开。
#[test]
fn utf8_multibyte_counts_discriminates_gbk_from_utf8() {
    // 真中文 UTF-8：汉字全是三字节序列。
    let (two, three) = utf8_multibyte_counts("中文测试文档".as_bytes());
    assert_eq!((two, three), (0, 6), "六个汉字应为六个三字节序列");
    assert!(three > two, "→ 应判为 UTF-8");

    // GBK 汉字：可能凑出合法双字节，但三字节不会多于双字节。
    let gbk = gbk_bytes("中文测试文档");
    let (two, three) = utf8_multibyte_counts(&gbk);
    assert!(
        three <= two,
        "GBK 文本若出现三字节多于双字节就会被误判为 UTF-8，实际 two={two} three={three}"
    );

    // GBK 中英混排是最容易被误判的一类，单独锁住。
    let mixed = gbk_bytes("文档 README 说明");
    let (two, three) = utf8_multibyte_counts(&mixed);
    assert!(
        three <= two,
        "GBK 中英混排不得被误判为 UTF-8，实际 two={two} three={three}"
    );

    // 含中文的损坏 UTF-8：三字节序列仍占多数 → 判 UTF-8。
    let mut damaged = "# 中文".as_bytes().to_vec();
    damaged.extend_from_slice(&[0xFF, 0xFE, 0x80]);
    let (two, three) = utf8_multibyte_counts(&damaged);
    assert!(three > two, "含中文的损坏 UTF-8 应判为 UTF-8");

    // 纯 ASCII 与空输入：两个计数都是 0（它们走严格 UTF-8 分支，到不了这里）。
    assert_eq!(utf8_multibyte_counts(b"hello world"), (0, 0));
    assert_eq!(utf8_multibyte_counts(&[]), (0, 0));
}

// ------------------------------------------------------------------
// GB18030 四字节扩展
// ------------------------------------------------------------------

/// 把字符编成 GB18030 字节：优先双字节 GBK，否则四字节。
fn gb18030_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in text.chars() {
        let cp = ch as u32;
        if cp < 0x80 {
            out.push(cp as u8);
            continue;
        }
        if let Some((lead, trail)) = gbk_reverse().get(&cp) {
            out.push(*lead);
            out.push(*trail);
            continue;
        }
        let pointer =
            gb18030_cp_to_pointer(cp).unwrap_or_else(|| panic!("U+{cp:04X} 应可编码为 GB18030"));
        out.extend_from_slice(&gb18030_pointer_to_bytes(pointer));
    }
    out
}

/// 四字节序列正确解码（此前会产生 `\u{FFFD}` + 3 个垃圾 ASCII 字符）。
///
/// 字节值由 WHATWG 合规实现独立验证（U+20000 → `95 32 82 36`）。
#[test]
fn gb18030_four_byte_sequence_decodes() {
    // 𠀀 (U+20000)：纯四字节文件不是合法 UTF-8，落到 GBK 分支。
    // （分支本身由下面的 `assert_eq!(enc, Encoding::Gbk)` 钉住，无需前置断言——
    //   静态字节数组传给 `str::from_utf8` 会触发 `invalid_from_utf8` lint。）
    let bytes = [0x95u8, 0x32, 0x82, 0x36];
    let (text, enc) = decode(&bytes);
    assert_eq!(enc, Encoding::Gbk);
    assert_eq!(text, "\u{20000}");
    assert!(!text.contains('\u{fffd}'), "四字节不应产生替换字符");
}

/// GBK 双字节与 GB18030 四字节混排：各自正确解码，互不干扰。
///
/// 四字节的第 2/4 字节是 `0x30..=0x39`（ASCII 数字区）——若误按 GBK
/// 双字节处理，这些数字会被吞进序列或泄漏到输出里。
#[test]
fn gb18030_four_byte_and_gbk_two_byte_interleave() {
    // "中"（GBK 双字节 D6 D0）+ 𠀀（四字节）+ "文"（GBK 双字节 CE C4）。
    let bytes = gb18030_bytes("中\u{20000}文");
    let (text, _) = decode(&bytes);
    assert_eq!(text, "中\u{20000}文");
}

/// 表的双向映射必须互逆：抽全空间样本，encode ∘ decode 恒等。
#[test]
fn gb18030_range_table_round_trips() {
    // 均匀抽样 + 边界锚点。任何一条不互逆都会让保存后的文件读回来变字。
    let mut samples = Vec::new();
    for i in 0..2000u32 {
        // Knuth 乘法散列布满全空间（模 1_587_600 个四字节指针）。
        samples.push(i.wrapping_mul(2_654_435_761) % 1_587_600);
    }
    samples.extend([0, 1, 35, 36, 18_999, 19_043, 1_237_575, 1_587_599]);
    for pointer in samples {
        match gb18030_pointer_to_cp(pointer) {
            Some(ch) => {
                let back = gb18030_cp_to_pointer(ch as u32);
                assert_eq!(
                    back,
                    Some(pointer),
                    "U+{:04X} 经表往返应回到指针 {pointer}",
                    ch as u32
                );
            }
            None => continue, // 未赋值指针无需互逆
        }
    }
}

/// 编码回写：GBK + 四字节混合文本经 encode → decode 往返无损。
#[test]
fn gb18030_encode_round_trips_through_decode() {
    let text = "中文与 English 混排，含扩展汉字 𠀀 与符号 ✓。";
    let bytes = encode(text, Encoding::Gbk);
    let (decoded, enc) = decode(&bytes);
    assert_eq!(enc, Encoding::Gbk);
    assert_eq!(decoded, text, "GBK/GB18030 编码回写必须无损往返");
}

/// 截断的四字节序列（缺第 4 字节）不得 panic；其后的字节按 WHATWG
/// 「作废首字节、回退重读」的语义处理——后续若凑成合法 GBK 双字节，
/// 按双字节解码是正确行为，不算"吞"。
#[test]
fn truncated_gb18030_sequence_does_not_panic() {
    // 95 32 82 = 𠀀 的前三个字节，缺第 4 字节；后面跟控制字符 0x07
    // （它既当不了 GBK 尾字节，也不是数字，必然原样保留）。
    let bytes = [0x95u8, 0x32, 0x82, 0x07];
    let (text, _) = decode(&bytes);
    assert!(text.contains('\u{fffd}'), "截断序列应显式替换：{text:?}");
    assert!(
        text.ends_with('\u{07}'),
        "其后的非序列字节不得被吞：{text:?}"
    );
}

/// UTF-16 编码回写：小端 / 大端各自补回 BOM 且往返无损。
#[test]
fn utf16_encode_round_trips_with_bom() {
    let text = "记事本 Unicode";
    for enc in [Encoding::Utf16Le, Encoding::Utf16Be] {
        let bytes = encode(text, enc);
        let (decoded, detected) = decode(&bytes);
        assert_eq!(detected, enc);
        assert_eq!(decoded, text, "{enc:?} 编码回写应往返无损");
    }
}

/// UTF-8 BOM 编码回写：`keeps_bom` 真正接到 `encode` 上。
#[test]
fn utf8_bom_encode_writes_bom_back() {
    let bytes = encode("正文", Encoding::Utf8Bom);
    assert!(bytes.starts_with(&UTF8_BOM), "BOM 应被写回");
    let (decoded, detected) = decode(&bytes);
    assert_eq!(detected, Encoding::Utf8Bom);
    assert_eq!(decoded, "正文");
}
