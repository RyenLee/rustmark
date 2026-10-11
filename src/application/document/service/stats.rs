//! 文档统计纯函数：字数 / 标题 / 代码块 / 任务 / 链接图片 / 段落（口径对齐原型）。

use crate::application::dto::StatsView;

/// 取一行开头的有序列表序号（`3. ` → `Some(3)`）。
///
/// 只认「数字 + `. `」，避免把正文里的 `2024. 年计划` 误判为列表项。
pub(super) fn leading_list_number(line: &str) -> Option<u32> {
    let t = line.trim_start();
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 9 {
        return None;
    }
    t[digits.len()..]
        .starts_with(". ")
        .then(|| digits.parse().ok())?
}

/// 由文档全文与标题计算统计信息（口径逐项对齐原型 `computeStats`）。
///
/// # 为什么不按空白切分
///
/// 原型的口径是**中日韩按字、西文按词**（`countWords`）。
/// `split_whitespace().count()` 把一整段没有空格的中文算成 1 个「词」，
/// 中文文档的字数会因此少到离谱（一篇 800 字的中文稿显示「8 字」）。
/// 两者不是精度差异，是**数量级**差异。
pub fn compute_stats(text: &str, title: &str) -> StatsView {
    let mut headings = 0;
    let mut code_blocks = 0;
    let mut tasks_done = 0;
    let mut tasks_total = 0;
    let mut paragraphs = 0;
    // 上一个字符是不是空白（或在文首）——用来判「新的非空段落开始了」。
    let mut in_blank = true;
    // 当前所在的围栏（` ``` ` 或 `~~~ `）；非空表示在代码块**内**。
    //
    // 围栏内的内容必须整体跳过：代码示例里写 `## 不是标题`
    // 不该被计成标题，写 `- [ ] 不是任务` 不该被计成任务。
    let mut fence: Option<String> = None;

    for raw in text.split('\n') {
        let line = raw.trim();
        let fence_token = fence_token_of(raw);
        if let Some(open) = fence.as_deref() {
            if fence_token.as_deref() == Some(open) {
                fence = None;
            }
            continue;
        }
        if let Some(token) = fence_token {
            fence = Some(token);
            code_blocks += 1;
            continue;
        }
        if is_atx_heading(line) {
            headings += 1;
        }
        if let Some(done) = task_marker_of(line) {
            tasks_total += 1;
            if done {
                tasks_done += 1;
            }
        }
        if line.is_empty() {
            in_blank = true;
        } else if in_blank {
            paragraphs += 1;
            in_blank = false;
        }
    }

    let (links, images) = count_links_and_images(text);
    let words = count_words(text);
    StatsView {
        words,
        characters: text.chars().count(),
        characters_no_space: text.chars().filter(|c| !c.is_whitespace()).count(),
        lines: text.split('\n').count().max(1),
        paragraphs,
        headings,
        code_blocks,
        links,
        images,
        title: title.to_string(),
        tasks_done,
        tasks_total,
        // 原型：`max(1, round(words / 300))`。
        reading_minutes: words.div_ceil(300).max(1) as u32,
    }
}

/// 是否是 ATX 标题（`#` ~ `######` 后必须跟空格）。
fn is_atx_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
}

/// 取出行首的围栏标记（` ``` ` 或 `~~~ `，含其后的语言名）。
///
/// 返回 `None` 表示这行不是围栏。**只看行首**：代码块内部的
/// 缩进代码里出现 ```` ``` ```` 才会提前闭合，与 CommonMark 一致。
fn fence_token_of(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    // 超过 3 个空格的缩进是缩进代码块而非围栏。
    if indent > 3 {
        return None;
    }
    let body = line.trim_start_matches([' ', '\t']);
    for marker in ["```", "~~~"] {
        if let Some(rest) = body.strip_prefix(marker) {
            // 闭合围栏后面不能跟语言名（`` ```rust `` 是开启而非闭合），
            // 但这里不做这个区分：开启与闭合都返回同一个 token，
            // 由调用方用「当前是否已在围栏内」决定语义。
            let _ = rest;
            return Some(marker.to_string());
        }
    }
    None
}

/// 判定任务列表项；返回 `Some(已完成)`。
///
/// `-` 与 `[` 之间**允许有空白**（`- [ ]` 是最常见写法，也是原型
/// 正则 `^\s*[-*+]\s\[([ xX])\]` 明确允许的）；`[` 与状态位之间
/// 不允许（`[ x]` 不是合法任务项）。
fn task_marker_of(line: &str) -> Option<bool> {
    let rest = line.strip_prefix(['-', '*', '+'])?.trim_start();
    let rest = rest.strip_prefix('[')?;
    match rest.chars().next()? {
        ' ' => Some(false),
        'x' | 'X' => Some(true),
        _ => None,
    }
}

/// 统计链接与图片数。
///
/// 图片 `![说明](地址)` **不计入**链接数（原型口径），
/// 否则一张图会在「链接」和「图片」两栏各数一次。
fn count_links_and_images(text: &str) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut links = 0;
    let mut images = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' && bytes[i] != b'!' {
            i += 1;
            continue;
        }
        let is_image = bytes[i] == b'!';
        // `!` 后面必须紧跟 `[`，否则 `!` 是普通字符。
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
fn link_tail_len(bytes: &[u8]) -> Option<usize> {
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

/// 字数：中日韩按字 + 西文按词（对齐原型 `countWords`）。
///
/// 「西文单词」= 连续的 `[A-Za-z0-9_'-]`。连字符与下划线算单词内字符，
/// 因为 `well-known` 是一个词，拆成两个会虚高。
///
/// # 与原型正则的一处有意偏离
///
/// 原型的正则是 `/[A-Za-z0-9_'-]+/g`，它把**孤立的连字符**也算成一个词：
/// `- [ ] todo` 里的 `-` 命中一次、`[x]` 里的 `x` 命中一次，
/// 于是一份 5 词的任务清单会显示「8 字」。
///
/// 故这里加一条判据：单词必须**至少含一个字母或数字**才算。
/// 破折号、星号、方括号都是 Markdown 的结构符号而非词，
/// 把它们计进字数只会让「列表越多字数越虚高」。
/// 这也是原型该修的地方（它自己的 `charsNoSpace` 也没排除它们，口径自相矛盾）。
fn count_words(text: &str) -> usize {
    let mut cjk = 0;
    let mut western = 0;
    let mut current = String::new();
    let flush = |token: &mut String, western: &mut usize| {
        if token.chars().any(|c| c.is_ascii_alphanumeric()) {
            *western += 1;
        }
        token.clear();
    };
    for ch in text.chars() {
        if is_cjk(ch) {
            flush(&mut current, &mut western);
            cjk += 1;
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '\'' || ch == '-' {
            current.push(ch);
        } else {
            flush(&mut current, &mut western);
        }
    }
    flush(&mut current, &mut western);
    cjk + western
}

/// 是否是「按字计」的东亚文字。
///
/// 覆盖范围与原型一致：CJK 统一表意文字 + 日文假名。
/// 刻意**不含**韩文（`\u{AC00}` 起）：原型的正则没算它，
/// 口径对齐优先于「看起来更完整」。
fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' | '\u{3040}'..='\u{30ff}')
}
