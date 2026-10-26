//! 行内分隔符剥离：把 `$…$` / `$$…$$` 与 `[^…]` 的「语法外壳」在渲染前去掉。
//!
//! 这两条剥离只发生在渲染侧：内容里保留的仍是 `[^1]` / `$E=mc^2$` 原文，
//! 搜索 / 字数不受影响。保守策略——只在**两端成对**时才剥，剥不动就原样返回。

/// 剥掉行内公式两端的 `$` / `$$` 分隔符（两侧各最多两个）。
///
/// 只在**两端成对**时才剥：`a $x$ 与 b` 的片段是 `$x$`，剥成 `x`；
/// 而片段本身若只剩一个 `$`（解析边界与渲染边界不一致时可能发生），
/// 原样返回比剥空更安全——剥空会让那一个字符凭空消失。
///
/// `pub` 是为了让表现层用例直接断言（切片越界会 panic，而本项目
/// `panic = "abort"`；这条规则不能只靠肉眼检查）。
pub fn strip_math_delimiters(s: &str) -> &str {
    let lead = s.len() - s.trim_start().len();
    let trail = s.trim_end().len();
    if trail <= lead {
        return s;
    }
    let body = &s[lead..trail];
    let open = body.len() - body.trim_start_matches('$').len();
    let close = body.len() - body.trim_end_matches('$').len();
    if open == 0 || close == 0 {
        return s;
    }
    let k = open.min(close).min(2);
    let (start, end) = (lead + k, trail - k);
    if end <= start { s } else { &s[start..end] }
}

/// 把脚注引用 `[^1]` 的显示文本换成**标号** `1`。
///
/// `[^` 与 `]` 是语法不是内容：正文里显示 `[^1]` 看起来就像「脚注没渲染」。
/// 主流渲染器（GitHub 等）把它画成上标数字，这里同样只留标号，字号缩小、
/// 用链接色（见 `super::text_format::span_format` 的 `"footnote_ref"`），与脚注定义区的 `[1]`
/// 标签对应。内容里保留的仍是 `[^1]` 原文，搜索 / 字数不受影响。
///
/// 只在**两端成对**时才剥（与 [`strip_math_delimiters`] 同一保守策略）：
/// 片段若不是完整的 `[^…]`（解析边界与渲染边界不一致时可能发生），
/// 原样返回比剥错更安全。
///
/// `pub` 是为了让表现层用例直接断言。
pub fn footnote_ref_mark(s: &str) -> &str {
    let Some(body) = s.strip_prefix("[^") else {
        return s;
    };
    let Some(label) = body.strip_suffix(']') else {
        return s;
    };
    if label.is_empty() { s } else { label }
}
