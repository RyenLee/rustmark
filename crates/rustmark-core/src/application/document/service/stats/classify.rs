//! 文档统计的**行与字符分类判定**（纯函数）：ATX 标题、围栏、任务列表项、
//! 东亚文字。
//!
//! 从 [`super`] 分出来的理由：这些都是 `&str / char → bool / Option` 的纯判定，
//! 既不涉及累加器状态、也不涉及扫描策略。原先它们只能靠 `compute_stats`
//! 端到端间接覆盖——改坏一个边界条件会藏在 1MB 等价性用例的某一个数字里，
//! 报错信息还只是「words 不等」。独立成模块后每条判定有自己的用例，
//! 变异验证也能定位到具体函数。

/// 是否是 ATX 标题（`#` ~ `######` 后必须跟空格）。
pub(super) fn is_atx_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
}

/// 取出行首的围栏标记（` ``` ` 或 `~~~ `，含其后的语言名）。
///
/// 返回 `None` 表示这行不是围栏。**只看行首**：代码块内部的
/// 缩进代码里出现 ```` ``` ```` 才会提前闭合，与 CommonMark 一致。
///
/// 返回值指向静态标记串（` ``` ` / `~~~ `），便于与原已开启的围栏做 `==` 比较，
/// 也省去每行一次 `String` 分配。
pub(super) fn fence_token_of(line: &str) -> Option<&'static str> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    // 超过 3 个空格的缩进是缩进代码块而非围栏。
    if indent > 3 {
        return None;
    }
    let body = line.trim_start_matches([' ', '\t']);
    // 闭合围栏后面不能跟语言名（`` ```rust `` 是开启而非闭合），
    // 但这里不做这个区分：开启与闭合都返回同一个 token，
    // 由调用方用「当前是否已在围栏内」决定语义。
    ["```", "~~~"].into_iter().find(|m| body.starts_with(m))
}

/// **任务标记识别的唯一真源**：判定 `s` **开头**是否是一个任务标记，
/// 返回 `Some(已完成)`。
///
/// 认 `[ ]` / `[x]` / `[X]` 三种，且**要求闭合 `]`**（GFM 的
/// `task list item` 要求 `- [ ] ` 形态的方括号成对；缺 `]` 的 `- [ x`
/// 只是普通文本）。「要求闭合」是 R-11 统一口径时**刻意收紧**的：此前
/// 统计侧不要求、预览回写侧要求，同一行在两个地方得到不同答案。
///
/// # 为什么三个入口而不是两个
///
/// 统计要「整行是不是任务项」（锚定行首、剥子弹），预览回写要「区间里
/// 第一个标记在哪」（任意位置、要偏移），两者输入与输出都不同、无法互相
/// 替代——但**规则**只有一份，就是本函数。两个入口各自只做定位，判定一律
/// 回到这里：任何一边漏改都不会产生第二套口径。
pub fn task_marker_state_at(s: &str) -> Option<bool> {
    let rest = s.strip_prefix('[')?;
    let mut chars = rest.chars();
    let done = match chars.next()? {
        ' ' => false,
        'x' | 'X' => true,
        _ => return None,
    };
    // 闭合 `]` 是硬要求：状态位后面必须是 `]`。
    if chars.next()? != ']' {
        return None;
    }
    Some(done)
}

/// 判定任务列表项；返回 `Some(已完成)`。
///
/// `-` 与 `[` 之间**允许有空白**（`- [ ]` 是最常见写法，也是原型
/// 正则 `^\s*[-*+]\s\[([ xX])\]` 明确允许的）；`[` 与状态位之间
/// 不允许（`[ x]` 不是合法任务项）。
///
/// 只做**定位**（剥列表符号 + 去前导空白），判定委托
/// [`task_marker_state_at`]。
pub fn task_marker_of(line: &str) -> Option<bool> {
    let rest = line.strip_prefix(['-', '*', '+'])?.trim_start();
    task_marker_state_at(rest)
}

/// 在 `s` 的**任意位置**找第一个任务标记，返回 `Some((字节偏移, 已完成))`。
///
/// 偏移是相对 `s` 起点、可直接用于源码替换的**字节**偏移（回写侧的用法：
/// `replace_source(from, from + 3, …)`）。
///
/// 只做**定位**（逐字节找 `[`），判定委托 [`task_marker_state_at`]——
/// 预览回写此前自己用 `w == b"[ ]" || w == b"[x]"` 判，与统计侧口径
/// 分家：大写 `[X]` 点不动（统计认它已完成、回写不认），而正文里的
/// `[x]` 反倒会被改掉。
///
/// 只认**首个**标记：一个列表项里出现多个复选框是极端的写法，
/// 而「改错位置」的代价（把用户别的文字换成 `[x]`）远大于「不生效」。
///
/// `[` 是 ASCII（0x5B），不可能出现在多字节 UTF-8 序列的延续字节里
/// （延续字节恒 ≥ 0x80），故每个命中位置都必然是字符边界、切片不会 panic。
pub fn find_task_marker(s: &str) -> Option<(usize, bool)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(done) = task_marker_state_at(&s[i..])
        {
            return Some((i, done));
        }
        i += 1;
    }
    None
}

/// 是否是「按字计」的东亚文字（CJK 统一表意文字 + 日文假名）。
///
/// 覆盖范围与原型一致。**刻意不含韩文**（`\u{AC00}` 起）：原型的正则没算它，
/// 口径对齐优先于「看起来更完整」。
///
/// 字数口径（与 [`super::compute_stats`] 的词规则配套）：中日韩按字 +
/// 西文按词，对齐原型 `countWords`。`split_whitespace().count()` 会把一整段
/// 没有空格的中文算成 1 个「词」——一篇 800 字的中文稿显示「8 字」，
/// 两者不是精度差异而是**数量级**差异。
pub(super) fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' | '\u{3040}'..='\u{30ff}')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ATX 标题：六个 `#` 是上限，且 `#` 后必须有空格。
    ///
    /// 反向对照同样重要——`#######`（七个）与 `#NoSpace` 都不是标题，
    /// 否则「数 `#` 个数」的写法会假绿。
    #[test]
    fn atx_heading_needs_one_to_six_hashes_and_a_space() {
        assert!(is_atx_heading("# 一"));
        assert!(is_atx_heading("###### 六"));
        assert!(!is_atx_heading("####### 七"), "七个 # 已超出 ATX 上限");
        assert!(!is_atx_heading("#NoSpace"), "# 后无空格不是标题");
        assert!(!is_atx_heading("正文"));
    }

    /// 围栏只看行首；缩进超过 3 个空格是缩进代码块而非围栏。
    #[test]
    fn fence_token_only_counts_unindented_markers() {
        assert_eq!(fence_token_of("```rust"), Some("```"));
        assert_eq!(fence_token_of("~~~"), Some("~~~"));
        assert_eq!(fence_token_of("  ```"), Some("```"), "3 空格缩进仍算围栏");
        assert_eq!(fence_token_of("    ```"), None, "4 空格缩进是缩进代码块");
        assert_eq!(fence_token_of("# 标题"), None);
    }

    /// 任务列表项：三种列表符都认，`-` 与 `[` 之间可有空白，状态位只允许空格 / x / X。
    #[test]
    fn task_marker_accepts_three_bullets_and_whitespace() {
        assert_eq!(task_marker_of("- [ ]"), Some(false));
        assert_eq!(task_marker_of("* [x]"), Some(true));
        assert_eq!(task_marker_of("+ [X]"), Some(true));
        assert_eq!(
            task_marker_of("-  [ ]"),
            Some(false),
            "`-` 与 `[` 间允许空白"
        );
        assert_eq!(task_marker_of("-[ ]"), Some(false));
        assert_eq!(task_marker_of("-[]"), None, "[ 后紧跟 ] 不是任务项");
        assert_eq!(task_marker_of("- [y]"), None, "非法状态位");
        assert_eq!(task_marker_of("-[普通链接]"), None);
    }

    /// 真源自己的边界：认 `[ ]` / `[x]` / `[X]`，**要求闭合 `]`**。
    ///
    /// 「要求闭合」是 R-11 统一口径时**刻意收紧**的行为变更：此前统计侧
    /// 不要求、预览回写侧要求，同一行在两个地方得到不同答案。
    #[test]
    fn task_marker_state_requires_the_closing_bracket() {
        assert_eq!(task_marker_state_at("[ ] 待办"), Some(false));
        assert_eq!(task_marker_state_at("[x] 完成"), Some(true));
        assert_eq!(task_marker_state_at("[X] 完成"), Some(true), "大写也认");
        assert_eq!(task_marker_state_at("[ x"), None, "缺闭合 `]`");
        assert_eq!(task_marker_state_at("[x"), None);
        assert_eq!(task_marker_state_at("[]"), None, "空状态位");
        assert_eq!(task_marker_state_at("[y]"), None, "非法状态位");
        assert_eq!(task_marker_state_at(""), None);
        // 畸形行不再算任务项（GFM 要求 `]`）。
        assert_eq!(task_marker_of("- [ x"), None);
        assert_eq!(find_task_marker("-[x"), None);
    }

    /// **DS-2 的核心不变量**：统计认它是任务项 ⟹ 回写一定能定位到同一个标记。
    ///
    /// 两边此前各写一遍判定（D-1：`- [X]` 统计认、回写不认 → 点击静默
    /// 无反应）。现在两个入口都委托 [`task_marker_state_at`]，这条用例
    /// 逐行交叉对拍：任何一边绕过真源，口径就会在这里分叉。
    #[test]
    fn stats_and_writeback_agree_on_every_task_line() {
        let lines = [
            "- [ ] 待办",
            "* [x] 完成",
            "+ [X] 完成",
            "-  [ ] 宽松空白",
            "-[ ] 无空白",
            "- [X] 见 [x] 说明",
            "- [ ] 第一 [x] 第二",
            "- [ x 畸形",
            "- [] 空状态位",
            "- [y] 非法状态",
            "- 普通列表项",
            "正文里的 [x] 不是任务项",
        ];
        for line in lines {
            let Some(done) = task_marker_of(line.trim_start()) else {
                continue;
            };
            let (off, got) = find_task_marker(line)
                .unwrap_or_else(|| panic!("统计认它是任务项，回写就必须能定位到：{line:?}"));
            assert_eq!(got, done, "任务项状态口径两边必须一致：{line:?}");
            assert!(
                matches!(&line[off..off + 3], "[ ]" | "[x]" | "[X]"),
                "定位到的必须是标记本身：{line:?} → {off}"
            );
        }
    }

    /// D-1：大写 `[X]` 必须**点得动**——此前回写只认小写 `[x]`，
    /// 于是统计显示「已完成」、点下去却毫无反应。
    #[test]
    fn uppercase_checked_marker_is_reachable_by_the_writeback_entry() {
        assert_eq!(find_task_marker("- [X] 已完成"), Some((2, true)));
        assert_eq!(find_task_marker("- [x] 已完成"), Some((2, true)));
        assert_eq!(find_task_marker("- [ ] 待办"), Some((2, false)));
    }

    /// D-2：真标记是 `[X]`、正文里还有 `[x]` 时，必须命中**真标记**。
    ///
    /// 旧回写实现只扫 `b"[ ]" || b"[x]"`，会跳过 `[X]` 去改正文里的
    /// `[x]`——把用户的话换成 `[ ]]`。
    #[test]
    fn find_task_marker_targets_the_real_marker_not_body_text() {
        let line = "- [X] 见 [x] 说明";
        let (off, done) = find_task_marker(line).expect("应找到标记");
        assert_eq!(
            off, 2,
            "必须命中行首的真标记，而不是正文里的 [x]（旧实现给 9）"
        );
        assert!(done);
    }

    /// 偏移是**字节**偏移且相对入参起点：多字节内容在前也不影响。
    #[test]
    fn find_task_marker_offset_is_absolute_bytes_within_the_slice() {
        let s = "中文 [ ] 待办";
        let (off, done) = find_task_marker(s).expect("应找到标记");
        assert_eq!(off, 7, "「中文 」占 7 字节");
        assert!(!done);
        assert_eq!(&s[off..off + 3], "[ ]");
        // 传入子切片时偏移相对子切片起点（回写侧就是这么用的）。
        let (off2, _) = find_task_marker(&s[3..]).expect("子切片里也应找到");
        assert_eq!(off2, 4);
    }

    /// 东亚文字范围：CJK + 假名算，韩文与西文不算（对齐原型口径）。
    #[test]
    fn cjk_range_covers_ideographs_and_kana_only() {
        assert!(is_cjk('中'));
        assert!(is_cjk('\u{3400}'), "扩展 A 区起点");
        assert!(is_cjk('あ'), "日文假名在 3040..30FF");
        assert!(!is_cjk('한'), "韩文刻意不按字计（原型口径）");
        assert!(!is_cjk('A'));
        assert!(!is_cjk(' '));
    }
}
