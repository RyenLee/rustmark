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

/// 判定任务列表项；返回 `Some(已完成)`。
///
/// `-` 与 `[` 之间**允许有空白**（`- [ ]` 是最常见写法，也是原型
/// 正则 `^\s*[-*+]\s\[([ xX])\]` 明确允许的）；`[` 与状态位之间
/// 不允许（`[ x]` 不是合法任务项）。
pub(super) fn task_marker_of(line: &str) -> Option<bool> {
    let rest = line.strip_prefix(['-', '*', '+'])?.trim_start();
    let rest = rest.strip_prefix('[')?;
    match rest.chars().next()? {
        ' ' => Some(false),
        'x' | 'X' => Some(true),
        _ => None,
    }
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
