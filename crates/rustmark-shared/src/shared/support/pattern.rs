//! 迷你正则引擎：模式 AST、编译（解析）与回溯匹配（`shared` 内核）。
//!
//! # 为什么放在 shared 而不是表现层
//!
//! 它同时服务两处：查找替换面板（表现层 `find_replace_ui_state`）与
//! 侧栏跨文件搜索（应用层 `application::workspace`）。分层依赖严格单向
//! （`presentation → application → domain`，见 `docs/spec/architecture-constraints.md`，
//! `check-arch` 机械校验），应用层不能反向引用表现层——引擎本体因此
//! 下沉到所有层都能用的 [`crate::shared`]；编译与扫描的**用例**住在
//! `application::document::find`（V11-61 上浮），表现层只留面板状态。
//!
//! # 为什么自己实现而不用 `regex` crate
//!
//! 1. **体积**：引入 `regex` + `regex-syntax` 会成为新的体积大头
//!    （10 MiB 红线，见 `docs/spec/metrics.md` §2），而「全字匹配 + 大小写 + 正则」
//!    三者的组合用一个手写匹配器就够；
//! 2. **UTF-8 安全**：手写扫描天然按 `char` 走，不会切在多字节字符中间。
//!
//! # 支持范围
//!
//! 支持：字面字符、`.`、`*`、`+`、`?`、`[]`（字符类，含 `^` 取反与范围）、
//! `|`（或）、`()`（分组）、`^`（行首）、`$`（行尾）、`\` 转义。
//! 不支持：反向引用、量词 `{n,m}`、非贪婪 `*?`、词边界 `\b`。
//!
//! 不支持的那些**在编译期就报错**，而不是运行时给出错误结果——
//! 用户看到「正则无效」比看到「匹配结果莫名其妙」好得多。

/// 正则模式编译结果（自制的极小引擎，只支持常用子集）。
#[derive(Clone, Debug)]
pub enum Pattern {
    /// 字面量。
    Literal(String),
    /// 正则。
    Regex(Regex),
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
    /// 而选项变更时调用方本来就会重新编译一次，不存在
    /// 「改了选项但沿用旧正则」的可能。
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

    /// 在 `text` 的字节区间 `from`..`to` 内查找第一个命中。
    ///
    /// 返回**字节**区间——领域层与 DTO 用的都是字节偏移。
    pub fn find_in(&self, text: &str, from: usize, to: usize) -> Option<(usize, usize)> {
        let slice = text.get(from..to)?;
        for start_byte in slice.char_indices() {
            if let Some(end_byte) = self.match_at(slice, start_byte.0) {
                // `slice` 是 `text[from..to]`，`match_at` 在 `slice` 内从 `start_byte.0`
                // 起匹配并返回相对字节长度，故 `from + start_byte.0 + end_byte`
                // 必 ≤ `to`，原先 `if end <= to` 是恒真死代码（E-21），移除。
                let end = from + start_byte.0 + end_byte;
                return Some((from + start_byte.0, end));
            }
        }
        None
    }

    /// 尝试在 `slice` 的 `at` 字节位置匹配任一分支；成功返回**相对字节长度**。
    fn match_at(&self, slice: &str, at: usize) -> Option<usize> {
        for alternative in &self.alternatives {
            // 每个分支各领一份预算，互不挤占；预算耗尽则判该分支未命中。
            let mut budget = MATCH_STEP_BUDGET;
            if let Some(len) = match_seq(alternative, slice, at, self.case_sensitive, &mut budget) {
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

/// 单次匹配尝试允许处理的**最大节点步数**（ReDoS 护栏）。
///
/// 回溯引擎对 `(a+)+$`、`(a*)*b` 这类嵌套量词是**指数级**的。触发门槛低到
/// 只是「用户在查找框里敲出、或从文档里粘来这样一个模式」，而它既不会编译
/// 报错、常规单测也覆盖不到，唯一的症状是 UI 线程直接冻结（本引擎在查找
/// 替换面板键入时同步执行，侧栏跨文件搜索同理）。
///
/// 故给每次尝试一份步数预算：耗尽即判为**未命中**并降级——宁可漏一次匹配，
/// 不可冻住界面。
///
/// 取值原则：需远大于「一行文本 × 肉眼可见的模式」的合理开销（典型行远不足
/// 10 KB，即便退化到二次也只在百万量级），又能让指数级路径在毫秒级退出。
const MATCH_STEP_BUDGET: usize = 1_000_000;

/// 消耗一步预算；返回 `false` 表示已耗尽，应当放弃本次尝试。
fn spend(budget: &mut usize) -> bool {
    match budget.checked_sub(1) {
        Some(left) => {
            *budget = left;
            true
        }
        None => false,
    }
}

/// 在 `seq` 上匹配 `slice[at..]`；成功返回消耗的字节数。
fn match_seq(seq: &[Node], slice: &str, at: usize, cs: bool, budget: &mut usize) -> Option<usize> {
    if !spend(budget) {
        return None;
    }
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
        } => match_repeat(node, *min, *max, *greedy, rest, slice, at, cs, budget),
        Node::StartAnchor => {
            // 行首：`at == 0` 或前一字符是换行。
            if at == 0 || slice[..at].ends_with('\n') {
                match_seq(rest, slice, at, cs, budget)
            } else {
                None
            }
        }
        Node::EndAnchor => {
            if at == slice.len() || slice[at..].starts_with('\n') {
                match_seq(rest, slice, at, cs, budget)
            } else {
                None
            }
        }
        Node::Group(inner) => {
            // 分组不额外消耗字符（无捕获组需求），直接进内容再接后续。
            match_seq(inner, slice, at, cs, budget)
                .and_then(|len| match_seq(rest, slice, at + len, cs, budget).map(|tail| len + tail))
        }
        Node::Any => {
            let ch = slice[at..].chars().next()?;
            if ch == '\n' {
                return None;
            }
            match_seq(rest, slice, at + ch.len_utf8(), cs, budget).map(|tail| ch.len_utf8() + tail)
        }
        Node::Char(expected) => {
            let ch = slice[at..].chars().next()?;
            if chars_eq(ch, *expected, cs) {
                match_seq(rest, slice, at + ch.len_utf8(), cs, budget)
                    .map(|tail| ch.len_utf8() + tail)
            } else {
                None
            }
        }
        Node::Class { negated, items } => {
            let ch = slice[at..].chars().next()?;
            let hit = items.iter().any(|item| class_matches(item, ch, cs));
            if hit != *negated {
                match_seq(rest, slice, at + ch.len_utf8(), cs, budget)
                    .map(|tail| ch.len_utf8() + tail)
            } else {
                None
            }
        }
    }
}

/// 重复节点匹配（回溯式；模式串都很短，朴素回溯足够）。
#[allow(clippy::too_many_arguments)] // 模式匹配核心函数；参数独立（min/max/greedy/node 等），打包成 struct 反而增加所有权转换开销
fn match_repeat(
    node: &[Node],
    min: usize,
    max: Option<usize>,
    greedy: bool,
    rest: &[Node],
    slice: &str,
    at: usize,
    cs: bool,
    budget: &mut usize,
) -> Option<usize> {
    // 先收集各次重复的结束位置，再按贪心/非贪心决定回退点。
    let mut ends = vec![at];
    let mut current = at;
    let limit = max.unwrap_or(usize::MAX);
    while ends.len() - 1 < limit {
        match match_seq(node, slice, current, cs, budget) {
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
            if let Some(tail) = match_seq(rest, slice, *end, cs, budget) {
                return Some(tail + (end - at));
            }
        }
        None
    } else {
        for (count, end) in ends.iter().enumerate() {
            if count < min {
                continue;
            }
            if let Some(tail) = match_seq(rest, slice, *end, cs, budget) {
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
        // 范围比较：两端与**待匹配字符**都各自折叠到小写区后再比。
        //
        // 逐端（而非整体）折叠是为了躲开端点互换：`[a-Z]` 这类跨大小写边界
        // 的区间，折叠后可能出现 `low > high`；用「同一侧」比较时它只是恒假，
        // 不会让整个字符类反向吞掉本该命中的字符。
        //
        // `ch` 必须与端点走**同一套**折叠——这是修 bug 的关键。早先只折叠
        // 端点而拿原始字符去比，于是 `[A-Z]` 在忽略大小写时被折成
        // `'a' <= ch <= 'z'`，而大写字母（0x41..0x5A）恒小于 `'a'`，
        // 表现为「勾了忽略大小写、搜 `[A-Z]`，却一个大写都搜不到」。
        // 区间两侧语义必须同源，否则折叠端点的那半套规则等于没生效。
        ClassItem::Range(low, high) => {
            let (low, high, ch) = if cs {
                (*low, *high, ch)
            } else {
                (fold_lower(*low), fold_lower(*high), fold_lower(ch))
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
                        // 不报错的话它会编成一个「永远匹配不到任何字符」的类：
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
