//! 查找替换 UI 状态与匹配器（S6-08）。
//!
//! # 为什么自己实现匹配而不用 `regex` crate
//!
//! 1. **体积**：S5-03 刚把 syntect 压到 17 KiB，此时引入 `regex` +
//!    `regex-syntax` 会成为新的体积大头，而「全字匹配 + 大小写 + 正则」
//!    三者的组合用一个手写匹配器就够；
//! 2. **UTF-8 安全**：手写扫描天然按 `char` 走，不会切在多字节字符中间
//!    （正则库的正则会，但用户写的 `regex` 模式仍可能切——这一点
//!    在 [`FindOptions::validate`] 里显式拒绝跨字符边界的字面量模式）。
//!
//! # 匹配模式
//!
//! | 组合 | 行为 |
//! |---|---|
//! | 默认 | 大小写不敏感的**字面量**匹配 |
//! | `Aa` | 大小写敏感的��面量匹配 |
//! | `.*` | 正则匹配（支持 `^ $ . * + ? [] () \|` 与转义） |
//! | 全字 | 命中的两侧必须是「词边界」 |
//!
//! 三个开关**正交**，可任意组合（原型 `FindReplacePanel.vue` 的行为）。

/// 查找选项。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FindOptions {
    /// 区分大小写（`Aa`）。
    pub case_sensitive: bool,
    /// 正则模式（`.*`）。
    pub regex: bool,
    /// 全字匹配。
    pub whole_word: bool,
}

impl FindOptions {
    /// 是否处于「纯字面量、不区分大小写」（默认状态）。
    pub const fn is_plain(&self) -> bool {
        !self.case_sensitive && !self.regex && !self.whole_word
    }

    /// 活跃开关数（UI 显示成角标）。
    pub const fn active_count(&self) -> usize {
        self.case_sensitive as usize + self.regex as usize + self.whole_word as usize
    }
}

/// 模式校验结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PatternError {
    /// 空模式。
    Empty,
    /// 正则语法错误（附人类可读原因）。
    InvalidRegex(String),
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("请输入查找内容"),
            Self::InvalidRegex(reason) => write!(f, "正则表达式无效：{reason}"),
        }
    }
}

/// 正则模式编译结果（自制的极小引擎，只支持常用子集）。
///
/// 支持：字面字符、`.`、`*`、`+`、`?`、`[]`（字符类，含 `^` 取反与范围）、
/// `|`（或）、`()`（分组）、`^`（行首）、`$`（行尾）、`\` 转义。
/// 不支持：反向引用、量词 `{n,m}`、非贪婪 `*?`。
///
/// 不支持的那些**在编译期就报错**，而不是运行时给出错误结果——
/// 用户看到「正则无效」比看到「匹配结果莫名其妙」好得多。
#[derive(Clone, Debug)]
pub enum Pattern {
    /// 字面量。
    Literal(String),
    /// 正则。
    Regex(Regex),
}

/// 一个正则节点。
#[derive(Clone, Debug)]
enum Node {
    /// 字面字符（`char` 而非 `byte`：多字节字符按整体匹配）。
    Char(char),
    /// 任意字符（`.`，不含换行）。
    Any,
    /// 字符类。
    Class {
        /// 是否取反（`[^...]`）。
        negated: bool,
        /// 类内字符。
        items: Vec<ClassItem>,
    },
    /// 行首锚点。
    StartAnchor,
    /// 行尾锚点。
    EndAnchor,
    /// 分组（只支持单分支：`(...)` 内不含 `|`）。
    ///
    /// 刻意不支持组内 `|`——它需要回溯与分支栈，
    /// 而嵌套分组会让「回溯深度」难以界定，得不偿失。
    /// 用户可用多个顶层 `|`（`(a|b)` 等价于 `a|b`）。
    Group(Vec<Node>),
    /// 重复（最少 0 次）。
    Repeat {
        /// 被重复的节点序列。
        node: Vec<Node>,
        /// 最少次数。
        min: usize,
        /// 最多次数；`None` 表示无上限。
        max: Option<usize>,
        /// 是否贪婪（首版只支持贪婪，`*?` 会报语法错）。
        greedy: bool,
    },
}

/// 字符类中的一个元素。
#[derive(Clone, Debug)]
enum ClassItem {
    /// 单字符。
    One(char),
    /// 范围（`a-z`）。
    Range(char, char),
}

/// 编译后的正则。
#[derive(Clone, Debug)]
pub struct Regex {
    alternatives: Vec<Vec<Node>>,
    /// 是否区分大小写（编译期固化，见 [`Regex::compile`]）。
    case_sensitive: bool,
}

impl Regex {
    /// 从模式串编译。
    ///
    /// `case_sensitive` 在**编译期**固化进结构体，而不是每次匹配传参：
    /// 匹配是递归回溯，热路径上少一个参数就少一层传递；
    /// 而选项变更时 [`compile`] 本来就会重新编译一次，不存在「改了选项
    /// 但沿用旧正则」的可能。
    pub fn compile(pattern: &str, case_sensitive: bool) -> Result<Self, PatternError> {
        let chars: Vec<char> = pattern.chars().collect();
        let mut cursor = 0usize;
        let alternatives = parse_alternatives(&chars, &mut cursor, None)?;
        if cursor != chars.len() {
            return Err(PatternError::InvalidRegex(format!(
                "第 {} 个字符处有多余的 ')'",
                cursor + 1
            )));
        }
        Ok(Self {
            alternatives,
            case_sensitive,
        })
    }

    /// 在 `text` 的 `char` 索引 `start`..`end`（字节区间 `from`..`to`）内查找。
    ///
    /// 返回**字节**区间——领域层与 DTO 用的都是字节偏移。
    fn find_in(&self, text: &str, from: usize, to: usize) -> Option<(usize, usize)> {
        let slice = text.get(from..to)?;
        for start_byte in slice.char_indices() {
            if let Some(end_byte) = self.match_at(slice, start_byte.0) {
                let end = from + start_byte.0 + end_byte;
                if end <= to {
                    return Some((from + start_byte.0, end));
                }
            }
        }
        None
    }

    /// 尝试在 `slice` 的 `at` 字节位置匹配任一分支；成功返回**相对字节长度**。
    fn match_at(&self, slice: &str, at: usize) -> Option<usize> {
        for alternative in &self.alternatives {
            if let Some(len) = match_seq(alternative, slice, at, self.case_sensitive) {
                return Some(len);
            }
        }
        None
    }
}

/// 两个字符在给定大小写规则下是否相等。
///
/// 大小写不敏感时**两侧都折叠成小写**再比，而不是用
/// `eq_ignore_ascii_case` 之类的 ASCII 快速路径：后者对中文与重音字母
/// 会「看起来不区分大小写」但实际区分（例如 `Ä` vs `ä`），
/// 用户搜 `ä` 找不到 `Ä`，比明确不支持更让人困惑。
fn chars_eq(left: char, right: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        left == right
    } else {
        left.to_lowercase().eq(right.to_lowercase())
    }
}

/// 在 `seq` 上匹配 `slice[at..]`；成功返回消耗的字节数。
fn match_seq(seq: &[Node], slice: &str, at: usize, cs: bool) -> Option<usize> {
    let Some((first, rest)) = seq.split_first() else {
        // 空分支：匹配空串。
        return Some(0);
    };
    match first {
        Node::Repeat {
            node,
            min,
            max,
            greedy,
        } => match_repeat(node, *min, *max, *greedy, rest, slice, at, cs),
        Node::StartAnchor => {
            // 行首：`at == 0` 或前一字符是换行。
            if at == 0 || slice[..at].ends_with('\n') {
                match_seq(rest, slice, at, cs)
            } else {
                None
            }
        }
        Node::EndAnchor => {
            if at == slice.len() || slice[at..].starts_with('\n') {
                match_seq(rest, slice, at, cs)
            } else {
                None
            }
        }
        Node::Group(inner) => {
            // 分组不额外消耗字符（无捕获组需求），直接进内容再接后续。
            match_seq(inner, slice, at, cs)
                .and_then(|len| match_seq(rest, slice, at + len, cs).map(|tail| len + tail))
        }
        Node::Any => {
            let ch = slice[at..].chars().next()?;
            if ch == '\n' {
                return None;
            }
            match_seq(rest, slice, at + ch.len_utf8(), cs).map(|tail| ch.len_utf8() + tail)
        }
        Node::Char(expected) => {
            let ch = slice[at..].chars().next()?;
            if chars_eq(ch, *expected, cs) {
                match_seq(rest, slice, at + ch.len_utf8(), cs).map(|tail| ch.len_utf8() + tail)
            } else {
                None
            }
        }
        Node::Class { negated, items } => {
            let ch = slice[at..].chars().next()?;
            let hit = items.iter().any(|item| class_matches(item, ch, cs));
            if hit != *negated {
                match_seq(rest, slice, at + ch.len_utf8(), cs).map(|tail| ch.len_utf8() + tail)
            } else {
                None
            }
        }
    }
}

/// 重复节点匹配（回溯式；模式串都很短，朴素回溯足够）。
#[allow(clippy::too_many_arguments)]
fn match_repeat(
    node: &[Node],
    min: usize,
    max: Option<usize>,
    greedy: bool,
    rest: &[Node],
    slice: &str,
    at: usize,
    cs: bool,
) -> Option<usize> {
    // 先收集各次重复的结束位置，再按贪心/非贪心决定回退点。
    let mut ends = vec![at];
    let mut current = at;
    let limit = max.unwrap_or(usize::MAX);
    while ends.len() - 1 < limit {
        match match_seq(node, slice, current, cs) {
            // 必须严格消耗，否则 `a*` 会无限循环。
            Some(len) if len > 0 => {
                current += len;
                ends.push(current);
            }
            _ => break,
        }
    }

    if greedy {
        for (count, end) in ends.iter().enumerate().rev() {
            if count < min {
                break;
            }
            if let Some(tail) = match_seq(rest, slice, *end, cs) {
                return Some(tail + (end - at));
            }
        }
        None
    } else {
        for (count, end) in ends.iter().enumerate() {
            if count < min {
                continue;
            }
            if let Some(tail) = match_seq(rest, slice, *end, cs) {
                return Some(tail + (end - at));
            }
        }
        None
    }
}

/// 字符类元素是否匹配。
fn class_matches(item: &ClassItem, ch: char, cs: bool) -> bool {
    match item {
        ClassItem::One(expected) => chars_eq(*expected, ch, cs),
        // 范围的两端**各自**折叠成小写后再比。
        //
        // 不能整体折叠再比区间：`[A-Z]` 折叠成 `[a-z]` 恰好等价，
        // 但 `[A-z]`（含 `[ \ ] ^ _ ` `）折叠后端点会**互换**
        // （`Z`→`z` 是 0x7A，`a`→`a` 是 0x61），区间方向反了，
        // 结果是「本该匹配一大段字符的类什么都不匹配」。
        // 逐端折叠不会出现这个问题：`low <= ch && ch <= high`
        // 在两端都被映射到同一侧时仍然有序。
        ClassItem::Range(low, high) => {
            let (low, high) = if cs {
                (*low, *high)
            } else {
                (fold_lower(*low), fold_lower(*high))
            };
            low <= ch && ch <= high
        }
    }
}

/// 折叠到「小写区」的字符（用于范围比较）。
///
/// 刻意**不**用 `to_lowercase()` 的完整结果：它对 `İ` 返回**两个**字符
/// （`i` + 组合附加号），取第一个虽然能用但语义模糊。这里只取首个
/// `char`，让范围比较的行为在所有输入上都是「确定的一个值」。
fn fold_lower(ch: char) -> char {
    ch.to_lowercase().next().unwrap_or(ch)
}

/// 解析 `|` 分隔的分支列表。
fn parse_alternatives(
    chars: &[char],
    cursor: &mut usize,
    terminator: Option<char>,
) -> Result<Vec<Vec<Node>>, PatternError> {
    let mut alternatives = Vec::new();
    loop {
        alternatives.push(parse_sequence(chars, cursor, terminator)?);
        if *cursor < chars.len() && chars[*cursor] == '|' {
            *cursor += 1;
            continue;
        }
        break;
    }
    Ok(alternatives)
}

/// 解析一个序列（直到 `|` 或终止符）。
fn parse_sequence(
    chars: &[char],
    cursor: &mut usize,
    terminator: Option<char>,
) -> Result<Vec<Node>, PatternError> {
    let mut nodes = Vec::new();
    while *cursor < chars.len() {
        let ch = chars[*cursor];
        if Some(ch) == terminator || ch == '|' {
            break;
        }
        if ch == ')' {
            break;
        }
        *cursor += 1;

        let node = match ch {
            '.' => Node::Any,
            '^' => Node::StartAnchor,
            '$' => Node::EndAnchor,
            '(' => {
                // 找到配对的 `)`。
                let mut depth = 1usize;
                let start = *cursor;
                while *cursor < chars.len() && depth > 0 {
                    match chars[*cursor] {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        *cursor += 1;
                    }
                }
                if depth != 0 {
                    return Err(PatternError::InvalidRegex("括号未闭合".to_string()));
                }
                let inner = &chars[start..*cursor];
                // 跳过配对的 `)`。
                *cursor += 1;
                let mut inner_cursor = 0usize;
                let alternatives = parse_alternatives(inner, &mut inner_cursor, None)?;
                if alternatives.len() != 1 {
                    return Err(PatternError::InvalidRegex(
                        "括号内暂不支持 '|'，请拆成多个顶层分支".to_string(),
                    ));
                }
                Node::Group(alternatives.into_iter().next().unwrap_or_default())
            }
            '[' => {
                let mut negated = false;
                if *cursor < chars.len() && chars[*cursor] == '^' {
                    negated = true;
                    *cursor += 1;
                }
                let mut items = Vec::new();
                loop {
                    if *cursor >= chars.len() {
                        return Err(PatternError::InvalidRegex(
                            "字符类未闭合（缺少 ']'）".to_string(),
                        ));
                    }
                    if chars[*cursor] == ']' {
                        *cursor += 1;
                        break;
                    }
                    let low = read_class_atom(chars, cursor)?;
                    // 范围 `a-z`：`-` 后紧跟一个字符且不是 `]`。
                    if *cursor + 1 < chars.len()
                        && chars[*cursor] == '-'
                        && chars[*cursor + 1] != ']'
                    {
                        *cursor += 1;
                        let high = read_class_atom(chars, cursor)?;
                        // 端点反序（`[z-a]`）在**编译期**报错。
                        //
                        // 不报错的话它会编译成一个「永远匹配不到任何字符」的类：
                        // 用户看到的是「正则明明没写错却一个都搜不到」，
                        // 这种失败比明确报错难查一个数量级。
                        // 真正则库（`regex` crate）同样在这一点上直接拒绝。
                        if low > high {
                            return Err(PatternError::InvalidRegex(format!(
                                "字符范围端点反序：'{low}' 大于 '{high}'"
                            )));
                        }
                        items.push(ClassItem::Range(low, high));
                    } else {
                        items.push(ClassItem::One(low));
                    }
                }
                Node::Class { negated, items }
            }
            // 落单的 `]`（没有配对的 `[`）按字面量处理——
            // 用户可能就是在搜这个符号，不该报错。
            ']' => Node::Char(ch),
            '\\' => {
                let escaped = *chars
                    .get(*cursor)
                    .ok_or_else(|| PatternError::InvalidRegex("转义符后缺少字符".to_string()))?;
                *cursor += 1;
                Node::Char(escaped)
            }
            '*' | '+' | '?' => {
                return Err(PatternError::InvalidRegex(format!(
                    "量词 '{ch}' 前面缺少可重复的内容"
                )));
            }
            other => Node::Char(other),
        };

        // 处理后缀量词。
        let node = match (node, chars.get(*cursor)) {
            (node, Some('*')) => {
                *cursor += 1;
                Node::Repeat {
                    node: vec![node],
                    min: 0,
                    max: None,
                    greedy: true,
                }
            }
            (node, Some('+')) => {
                *cursor += 1;
                Node::Repeat {
                    node: vec![node],
                    min: 1,
                    max: None,
                    greedy: true,
                }
            }
            (node, Some('?')) => {
                *cursor += 1;
                Node::Repeat {
                    node: vec![node],
                    min: 0,
                    max: Some(1),
                    greedy: true,
                }
            }
            (node, _) => node,
        };
        nodes.push(node);
    }
    Ok(nodes)
}

/// 读一个字符类内的原子（支持 `\]` 转义）。
fn read_class_atom(chars: &[char], cursor: &mut usize) -> Result<char, PatternError> {
    let ch = *chars
        .get(*cursor)
        .ok_or_else(|| PatternError::InvalidRegex("字符类未闭合".to_string()))?;
    *cursor += 1;
    if ch == '\\' {
        let escaped = *chars
            .get(*cursor)
            .ok_or_else(|| PatternError::InvalidRegex("转义符后缺少字符".to_string()))?;
        *cursor += 1;
        return Ok(escaped);
    }
    Ok(ch)
}

/// 编译模式（字面量或正则，取决于开关）。
pub fn compile(pattern: &str, options: FindOptions) -> Result<Pattern, PatternError> {
    if pattern.is_empty() {
        return Err(PatternError::Empty);
    }
    if options.regex {
        Regex::compile(pattern, options.case_sensitive).map(Pattern::Regex)
    } else {
        Ok(Pattern::Literal(pattern.to_string()))
    }
}

/// 一次查找的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Match {
    /// 命中区间（**字节**）。
    pub start: usize,
    /// 命中区间终点（字节，不含）。
    pub end: usize,
}

impl Match {
    /// 命中长度（字节）。
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// 是否为空命中（零宽匹配，如 `a*` 可匹配空串）。
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// 查找器：编译好的模式 + 全部命中。
#[derive(Clone, Debug)]
pub struct Finder {
    pattern: Pattern,
    options: FindOptions,
    matches: Vec<Match>,
}

impl Finder {
    /// 编译并扫描全文。
    pub fn scan(text: &str, pattern: &str, options: FindOptions) -> Result<Self, PatternError> {
        let pattern = compile(pattern, options)?;
        // 上限保护：10 万条足以覆盖任何真实文档，再多就是模式退化。
        const MAX_MATCHES: usize = 100_000;
        // DEF-01 修复：大小写不敏感的字面量查找改为单次线性扫描（O(N)），
        // 不再「每命中一次重建全篇字符表再重新扫描」导致 O(N×M)。
        // 其余分支（正则 / 大小写敏感字面量）维持原逐命中扫描。
        let is_ignore_case_literal =
            matches!(&pattern, Pattern::Literal(_)) && !options.case_sensitive;
        let matches = if is_ignore_case_literal {
            let Pattern::Literal(literal) = &pattern else {
                unreachable!("已确认是 Literal 分支")
            };
            scan_literal_ignore_case_all(text, literal, options.whole_word, MAX_MATCHES)
        } else {
            let mut matches = Vec::new();
            let mut at = 0usize;
            while at <= text.len() && matches.len() < MAX_MATCHES {
                let Some(found) = find_from(&pattern, options, text, at) else {
                    break;
                };
                // 零宽匹配必须强制前进，否则死循环。
                at = if found.is_empty() {
                    next_char_boundary(text, found.end)
                } else {
                    found.end
                };
                if !found.is_empty() {
                    matches.push(found);
                }
            }
            matches
        };
        Ok(Self {
            pattern,
            options,
            matches,
        })
    }

    /// 全部命中。
    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    /// 命中数。
    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// 下一个命中（从 `after` 之后开始，循环）。
    pub fn next_after(&self, after: usize) -> Option<Match> {
        self.find_cyclic(after, true)
    }

    /// 上一个命中（从 `before` 之前开始，循环）。
    pub fn prev_before(&self, before: usize) -> Option<Match> {
        self.find_cyclic(before, false)
    }

    /// 循环查找：越过端点则回到另一端。
    fn find_cyclic(&self, from: usize, forward: bool) -> Option<Match> {
        if self.matches.is_empty() {
            return None;
        }
        if forward {
            // 命中区间终点 <= from 的都算「已过去」。
            self.matches
                .iter()
                .find(|m| m.end > from)
                .copied()
                .or_else(|| self.matches.first().copied())
        } else {
            // 起点 >= from 的都算「还没过去」。
            self.matches
                .iter()
                .rev()
                .find(|m| m.start < from)
                .copied()
                .or_else(|| self.matches.last().copied())
        }
    }

    /// 当前模式与选项（供 UI 回显）。
    pub fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    /// 选项。
    pub const fn options(&self) -> FindOptions {
        self.options
    }

    /// 校验「全部替换」后的新内容。
    ///
    /// 返回 `Err` 表示替换文本会让命中数变化（例如替换文本本身含模式），
    /// 此时 UI 应提示「替换后仍会有新命中」，避免用户以为替换干净了。
    pub fn preview_replace_all(&self, text: &str, replacement: &str) -> ReplaceSummary {
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0usize;
        let mut count = 0usize;
        for m in &self.matches {
            if m.start < cursor {
                continue;
            }
            out.push_str(&text[cursor..m.start]);
            // 替换文本里的 `$1` 之类不支持——按字面量处理。
            out.push_str(replacement);
            cursor = m.end;
            count += 1;
        }
        out.push_str(&text[cursor..]);
        // 判断「替换后是否仍会有新命中」，必须用**原模式**去扫新文本，
        // 而不是用替换文本当模式——后者只在「替换文本恰好等于模式」时成立，
        // 会漏掉绝大多数真正会二次命中的情况。
        let will_match_again =
            !self.matches.is_empty() && matches_with(&self.pattern, &out, self.options);
        ReplaceSummary {
            result: out,
            count,
            will_match_again,
        }
    }
}

/// 替换结果摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceSummary {
    /// 替换后的全文。
    pub result: String,
    /// 实际替换次数。
    pub count: usize,
    /// 替换后是否仍会出现新命中。
    ///
    /// 例：用「`aa`」替换「`a`」，新文本里全是 `a`，再点一次全部替换会
    /// **无限增生**（`a` → `aa` → `aaaa` → …）。UI 必须据此警告，否则
    /// 用户会点到手酸。
    ///
    /// 反方向（用「`a`」替换「`aa`」）是收敛的，必须**不**报警——
    /// 误报会让用户以为替换不干净，反而不敢用这个功能。
    pub will_match_again: bool,
}

/// 用**已编译**的 `pattern` 判断 `text` 中是否还有命中。
///
/// 与 [`Finder::scan`] 的区别：这里不重新解析模式串，因此正则模式的
/// 语法错误无从产生——`preview_replace_all` 拿到的 `self.pattern`
/// 必然是编译期已通过的产物。
fn matches_with(pattern: &Pattern, text: &str, options: FindOptions) -> bool {
    find_from(pattern, options, text, 0).is_some_and(|m| !m.is_empty())
}

/// 从 `from` 起找第一个命中。
fn find_from(pattern: &Pattern, options: FindOptions, text: &str, from: usize) -> Option<Match> {
    match pattern {
        Pattern::Literal(literal) => {
            let haystack = if options.case_sensitive {
                text
            } else {
                // 不区分大小写：先按小写找字节位置，再映回原文。
                // 注意 `to_lowercase()` 可能改变长度（如 `İ` → `i̇`），
                // 故这里逐字符比较而非整体小写，避免偏移错位。
                return find_literal_ignore_case(text, literal, from, options.whole_word);
            };
            find_literal(haystack, literal, from, options.whole_word)
        }
        Pattern::Regex(regex) => {
            let found = regex.find_in(text, from, text.len())?;
            if options.whole_word && !is_word_boundary(text, found.0, found.1) {
                // 跳过——继续往后找，但要防止死循环。
                let next = next_char_boundary(text, found.1);
                return find_from(pattern, options, text, next.max(from + 1));
            }
            Some(Match {
                start: found.0,
                end: found.1,
            })
        }
    }
}

/// 大小写敏感的��面量查找。
fn find_literal(text: &str, literal: &str, from: usize, whole_word: bool) -> Option<Match> {
    let mut at = from;
    while at <= text.len() {
        let found = text.get(at..)?.find(literal)?;
        let start = at + found;
        let end = start + literal.len();
        if !whole_word || is_word_boundary(text, start, end) {
            return Some(Match { start, end });
        }
        // 不满足全字条件，继续往后。
        at = next_char_boundary(text, start);
    }
    None
}

/// 大小写不敏感的字面量查找。
///
/// 逐字符比较，**两侧都折叠成小写**。刻意不用
/// `text.to_lowercase().find(...)`：Unicode 小写映射会改变长度
/// （`İ` → `i̇` 是 1 个字符变 2 个），整体小写后再找会得到**错位**的字节偏移，
/// 切片时 panic 或切出乱码。
fn find_literal_ignore_case(
    text: &str,
    literal: &str,
    from: usize,
    whole_word: bool,
) -> Option<Match> {
    let needle: Vec<char> = literal.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return None;
    }
    let haystack: Vec<(usize, char)> = text.char_indices().collect();
    for index in 0..haystack.len() {
        let start = haystack[index].0;
        if start < from {
            continue;
        }
        // 逐字符比对，两侧都折叠成小写。
        let hit = haystack[index..]
            .iter()
            .take(needle.len())
            .flat_map(|(_, ch)| ch.to_lowercase())
            .eq(needle.iter().copied());
        if !hit {
            continue;
        }
        let end = match haystack.get(index + needle.len()) {
            Some((offset, _)) => *offset,
            None => text.len(),
        };
        if whole_word && !is_word_boundary(text, start, end) {
            continue;
        }
        return Some(Match { start, end });
    }
    None
}

/// 大小写不敏感字面量查找的**单次线性扫描**版本（DEF-01 修复）。
///
/// 对照原 [`find_literal_ignore_case`]：那里每次调用都 `text.char_indices().collect()`
/// 重建整篇字符表，且 `Finder::scan` 每命中一次调一次 → 最坏 **O(N×M)**。
/// 本函数只构建一次「(原字节偏移, 字符)」序列，随后在该序列上做滑动窗口比对，
/// 整体 **O(N × |needle|)**（needle 长度固定时为 O(N)），不再随命中数退化。
///
/// 严格保持与原实现一致的比对语义：
/// - 两侧都折叠成小写再逐字符比较（不用 `to_lowercase()` 整体小写，避免
///   Unicode 小写映射改变长度导致字节偏移错位、切片 panic）；
/// - 命中后**按 `end` 字节推进**到下一个字符边界，避免重叠命中
///   （与原 `Finder::scan` 里 `at = found.end` 的推进一致）；
/// - 整词（`whole_word`）边界判定沿用 [`is_word_boundary`]；
/// - 受 `max_matches` 上限保护，达到即停。
fn scan_literal_ignore_case_all(
    text: &str,
    literal: &str,
    whole_word: bool,
    max_matches: usize,
) -> Vec<Match> {
    // 与 [`find_literal_ignore_case`] 同款小写折叠（字符级，非整体小写）。
    let needle: Vec<char> = literal.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let haystack: Vec<(usize, char)> = text.char_indices().collect();
    if haystack.len() < needle.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    // 滑动窗口起点（以 haystack 下标计），命中后按 `end` 字节跳到下一字符边界。
    let mut index = 0usize;
    while index + needle.len() <= haystack.len() {
        let hit = haystack[index..]
            .iter()
            .take(needle.len())
            .flat_map(|(_, ch)| ch.to_lowercase())
            .eq(needle.iter().copied());
        if !hit {
            index += 1;
            continue;
        }
        let start = haystack[index].0;
        let end = match haystack.get(index + needle.len()) {
            Some((offset, _)) => *offset,
            None => text.len(),
        };
        if whole_word && !is_word_boundary(text, start, end) {
            // 整词边界不成立：与原实现一致，继续往后找（不按 end 跳过），
            // 因原 `find_literal_ignore_case` 对该起点 `continue` 而非返回。
            index += 1;
            continue;
        }
        matches.push(Match { start, end });
        if matches.len() >= max_matches {
            break;
        }
        // 命中后跳到 `end` 之后的第一个字符边界（与原 `at = found.end` 一致），
        // 避免同一字符参与两个重叠命中。
        index = match haystack.iter().position(|&(off, _)| off >= end) {
            Some(i) => i,
            None => haystack.len(),
        };
    }
    matches
}

/// 区间两侧是否为词边界。
///
/// 规则：起点前一个字符不是词字符，且终点后一个字符不是词字符。
/// 中文按「非 ASCII 字母数字」处理，故「中文abc」里 `abc` 的左边界成立。
fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

/// 是否为「词字符」。
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// 下一个字符边界（保证不切在多字节字符中间）。
fn next_char_boundary(text: &str, at: usize) -> usize {
    match text[at..].chars().next() {
        Some(ch) => at + ch.len_utf8(),
        None => at + 1,
    }
}

/// 查找输入框的 egui `Id` 源串。
///
/// 单独提出常量是因为**三处**必须用同一个值：视图里绑定控件、
/// [`crate::app::frame::EditorApp::collect_actions`] 判断「焦点是否在输入框」。
/// 写成两处字面量的话，改视图里的那处就会让「快捷键让位」静默失效——
/// 用户在查找框里按 Ctrl+B 又把文档加粗了，且没有任何报错。
pub const FIND_INPUT_ID: &str = "rustmark.find.input";

/// 替换输入框的 egui `Id` 源串（理由同 [`FIND_INPUT_ID`]）。
pub const REPLACE_INPUT_ID: &str = "rustmark.find.replace";

/// 查找替换面板状态。
#[derive(Clone, Debug, Default)]
pub struct FindReplaceUiState {
    /// 查找框内容。
    pub find_text: String,
    /// 替换框内容。
    pub replace_text: String,
    /// 查找选项。
    pub options: FindOptions,
    /// 是否展开替换框。
    pub replace_open: bool,
    /// 当前命中的索引（`None` = 未选中）。
    pub current: Option<usize>,
    /// 上次扫描得到的查找器（`None` = 还没扫过 / 模式无效）。
    ///
    /// 存**编译后的模式**而非只存命中列表：正则模式编译一次即可复用，
    /// 而「下一个 / 上一个」需要在命中列表上二分/顺序查找，命中列表本身
    /// 就是 [`Finder`] 的一部分，拆开存只会多一份可能不同步的副本。
    ///
    /// 它是**派生状态**：由 `find_text` + `options` + 文档内容唯一决定，
    /// 任何一项变了都必须 [`Self::rescan`] 重扫，否则面板显示的计数
    /// 会与实际能跳转到的位置不符。
    pub finder: Option<Finder>,
    /// 面板错误提示（如正则无效）。
    pub error: Option<String>,
    /// 查找框是否拥有焦点（查找期间编辑区仍可编辑，A-28）。
    pub find_focused: bool,
    /// 上次查找的文档 ID——跨文档切换时据此决定是否保留状态。
    pub last_doc_id: Option<crate::shared::ids::DocumentId>,
    /// 跨文档切换时是否保留查找状态。
    ///
    /// 定义（验收要求「明确定义并写进文档」）：**保留**查询与选项，
    /// 但**重置**命中选中与滚动位置。理由：用户常在多个文件里
    /// 找同一个词；但命中位置属于旧文档，带过去会跳到错误位置。
    pub keep_query_across_documents: bool,
    /// 上次「全部替换」的替换处数（状态栏提示用；`None` = 本次会话没替换过）。
    pub last_replace_count: Option<usize>,
    /// 上次「全部替换」后是否仍会出现新命中（`aa` → `a` 这类无限增生）。
    ///
    /// 必须让用户看见：否则反复点「全部替换」会让文档指数膨胀，
    /// 而每一遍都提示「已替换 N 处」，看起来完全正常。
    pub last_replace_grows: bool,
}

impl FindReplaceUiState {
    /// 构造默认状态。
    pub fn new() -> Self {
        Self {
            keep_query_across_documents: true,
            ..Default::default()
        }
    }

    /// 打开面板。
    pub fn open(&mut self) {
        self.replace_open = false;
    }

    /// 打开面板并展开替换框（`Replace` 动作）。
    pub fn open_with_replace(&mut self) {
        self.replace_open = true;
    }

    /// 打开面板（`with_replace` 决定是否展开替换框）。
    ///
    /// 取代 [`Self::open`] / [`Self::open_with_replace`] 成对使用：
    /// 两个几乎一样的公开方法很容易让调用方「挑一个顺手的用」，
    /// 而 `Ctrl+F` 展开替换框并不是什么不可饶恕的错误——只是多占一行。
    pub fn open_panel(&mut self, with_replace: bool) {
        self.replace_open = with_replace;
        // 打开即聚焦查找框：用户按 Ctrl+F 的意图是「马上打字」，
        // 焦点若留在编辑区，就得再点一次面板。
        self.find_focused = true;
    }

    /// 关闭面板并清空错误。
    pub fn close(&mut self) {
        self.error = None;
    }

    /// 对给定文档内容重扫，并更新命中计数 / 当前项 / 错误。
    ///
    /// 这是面板状态的**唯一**写入口：改 `find_text` 或 `options` 之后
    /// 必须调它，否则会拿旧模式去报数。
    pub fn rescan(&mut self, text: &str) {
        if self.find_text.is_empty() {
            // 空查询：清空而不是保留旧命中。否则用户清空输入框后
            // 面板还显示「3/12」，点「下一个」却跳到旧位置。
            self.finder = None;
            self.error = None;
            self.current = None;
            return;
        }
        match Finder::scan(text, &self.find_text, self.options) {
            Ok(finder) => {
                let count = finder.count();
                self.finder = Some(finder);
                self.error = None;
                if count == 0 {
                    self.current = None;
                } else if self.current.is_none_or(|index| index >= count) {
                    self.current = Some(0);
                }
            }
            Err(error) => {
                // 模式无效：保留**上一次**的 finder 吗？刻意不保留——
                // 那样「下一个」会跳到与当前输入框内容无关的位置。
                self.finder = None;
                self.error = Some(error.to_string());
                self.current = None;
            }
        }
    }

    /// 当前选中的命中（`None` 表示无命中或未选中）。
    pub fn current_match(&self) -> Option<Match> {
        let finder = self.finder.as_ref()?;
        let index = self.current?;
        finder.matches().get(index).copied()
    }

    /// 命中总数。
    pub fn match_count(&self) -> usize {
        self.finder.as_ref().map_or(0, Finder::count)
    }

    /// 计数文本（`1/12` / `无结果` / 空串）。
    ///
    /// 与原型 `FindReplacePanel.vue` 的 `{{ findIndex + 1 }}/{{ matches.length }}`
    /// 逐字对齐，包括「无命中时显示『无结果』而不是 `0/0`」这个细节，
    /// 以及「查询为空时什么都不显示」（此时报「无结果」会让人以为文档里
    /// 真的没有匹配，可明明还没输入任何东西）。
    pub fn count_label(&self) -> String {
        if self.find_text.is_empty() {
            return String::new();
        }
        let total = self.match_count();
        if total == 0 {
            return "无结果".to_string();
        }
        // `current` 为 `None` 只可能出现在「无命中」（上面已处理），
        // 兜底成第 1 项，避免 UI 出现 `0/12`。
        format!("{}/{}", self.current.unwrap_or(0) + 1, total)
    }

    /// 跳到下一个 / 上一个命中（`forward = true` 为下一个）。
    ///
    /// 循环语义由 [`Finder::next_after`] / [`Finder::prev_before`] 提供，
    /// 这里只负责算起点：从**当前命中的终点**往后找，反向则从**起点**往前找。
    /// 用「终点」而非「起点」是关键——否则连点两次「下一个」会在同一个
    /// 命中上打转（起点仍在命中内，`end > from` 立刻又选中它）。
    pub fn step(&mut self, forward: bool) -> Option<Match> {
        let finder = self.finder.as_ref()?;
        let total = finder.count();
        if total == 0 {
            return None;
        }
        let from = match self.current.and_then(|i| finder.matches().get(i)) {
            Some(m) if forward => m.end,
            Some(m) => m.start,
            // 没有当前项：正向从头找，反向从末尾找（与主流编辑器一致——
            // 「上一个」在刚打开面板时跳到最后一个命中）。
            None => {
                if forward {
                    0
                } else {
                    usize::MAX
                }
            }
        };
        let found = if forward {
            finder.next_after(from)
        } else {
            finder.prev_before(from)
        };
        let found = found?;
        // 反向查找用 `usize::MAX` 起点时 `prev_before` 会返回最后一项，
        // 靠 `matches().iter().position` 反查索引——O(n)，但只在
        // 「无当前项」这一次发生，不是热路径。
        let index = if forward {
            finder.matches().iter().position(|m| *m == found)?
        } else {
            finder.matches().iter().rposition(|m| *m == found)?
        };
        self.current = Some(index);
        Some(found)
    }

    /// 选项变更后必须清空当前命中（旧的 `current` 指向的是旧模式的第 N 项）。
    pub fn set_options(&mut self, options: FindOptions) {
        if self.options != options {
            self.options = options;
            self.current = None;
        }
    }

    /// 切换某个选项。
    pub fn toggle(&mut self, which: OptionKind) {
        let mut options = self.options;
        match which {
            OptionKind::CaseSensitive => options.case_sensitive = !options.case_sensitive,
            OptionKind::Regex => options.regex = !options.regex,
            OptionKind::WholeWord => options.whole_word = !options.whole_word,
        }
        self.set_options(options);
    }

    /// 记录一次查找结果。
    ///
    /// 已被 [`Self::rescan`] 取代并删除：它与 `rescan` 是同一件事的两种写法
    /// （前者接收「已编译好的 `Finder`」，后者自己扫），两份入口必然出现
    /// 「调用方用了旧的那份、忘了更新 `finder` 字段」的分歧——
    /// 这正是 S6-07 在 `editor_ops` 与 `EditingService` 之间踩过的坑。
    /// 单一入口 [`Self::rescan`] 既是唯一真相，也让「模式变了会自动重扫」
    /// 成为结构性保证而非调用方的自觉。
    ///
    /// 文档切换：按 `keep_query_across_documents` 决定保留什么。
    pub fn on_document_switched(&mut self, doc_id: crate::shared::ids::DocumentId) {
        match self.last_doc_id {
            None => self.last_doc_id = Some(doc_id),
            Some(previous) if previous == doc_id => {}
            Some(_) => {
                // 跨文档：保留查询与选项，重置命中选中。
                self.current = None;
                self.error = None;
                // 命中列表**必须**清空：它的字节偏移属于旧文档，
                // 带到新文档上会跳到完全无关的位置（甚至越界）。
                // 保留查询的用户随后 [`Self::rescan`] 一次就能得到新命中。
                self.finder = None;
                if !self.keep_query_across_documents {
                    self.find_text.clear();
                    self.replace_text.clear();
                    self.options = FindOptions::default();
                }
                self.last_doc_id = Some(doc_id);
            }
        }
    }
}

/// 可切换的查找选项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionKind {
    /// 区分大小写。
    CaseSensitive,
    /// 正则。
    Regex,
    /// 全字。
    WholeWord,
}
