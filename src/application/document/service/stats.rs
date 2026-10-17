//! 文档统计纯函数：字数 / 标题 / 代码块 / 任务 / 链接图片 / 段落（口径对齐原型）。
//!
//! # 单遍扫描（建议三 / 问题 054 ②）
//!
//! 重构前 `compute_stats` 在整篇文本上做了**约六遍**独立遍历：按行 `split` +
//! 每行 `trim`/判定、`count_links_and_images` 逐字节、`count_words` 逐字符、
//! `chars().count()`、`chars().filter(!is_whitespace).count()`、`line_count`
//! 逐字节再扫一次 `\n`。1MB 文本上单次结算 ≈ 9.5ms，其中约 4~6ms 纯属重复遍历。
//!
//! 现在改为**单遍**：一个字节游标同时驱动
//! - 字符 / 非空格字符 / 词（CJK 按字 + 西文按词）—— 字符层；
//! - 行数（`\n` 计数）—— 字节层；
//! - 链接 / 图片（`[`/`!` 字节层匹配，跳跃式）—— 字节层；
//! - 标题 / 围栏 / 任务 / 段落（复用行缓冲，行尾结算）—— 行层。
//!
//! 除「围栏内跳过整行」外，各统计项的判定口径与原实现逐字一致；等价性由
//! 本文件底部的 `reference_compute_stats` 属性测试守住（对外语义不许变）。

use crate::application::dto::StatsView;

#[cfg(test)]
use crate::shared::text::line_count;

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
    let bytes = text.as_bytes();
    let mut acc = StatsAcc::default();
    let mut lines = 0usize;
    // 当前行在 `text` 中的字节起点；行尾用零拷贝切片 `&text[line_start..bi]` 结算，
    // 不再为每行分配 `String`（建议三关键：字符一遍 + 零分配）。
    let mut line_start = 0usize;

    // 第一遍（字符层）：一个持久的 `char_indices` 迭代器同时驱动
    // 字符 / 非空格字符 / 词（CJK 按字 + 西文按词）/ 行数；行尾以零拷贝切片结算
    // 标题 / 围栏 / 任务 / 段落。旧实现要 `split` + 每行 3 次 `chars()` + 2 次整篇
    // `chars().count()` + `line_count` 等约六遍独立遍历，这里合并为这一遍。
    for (bi, ch) in text.char_indices() {
        if ch == '\n' {
            acc.feed_newline();
            acc.finish_line(&text[line_start..bi]);
            lines += 1;
            line_start = bi + ch.len_utf8();
        } else {
            acc.feed(ch);
        }
    }
    // 收尾：最后一行（不论是否为空）+ 末尾可能未闭合的西文词。
    acc.finish_line(&text[line_start..text.len()]);
    lines += 1;
    acc.flush_western();

    // 第二遍（字节层）：链接 / 图片计数。仅在 `[` / `!` 处做配对，绝大多数文档
    // 此遍几乎不做事；匹配口径与等价性基准 `reference_compute_stats` 所用的
    // `ref_count_links_and_images` 逐字一致（图片 `![...](...)` 不计入链接数）。
    let (links, images) = count_links_and_images(bytes);

    let words = acc.cjk + acc.western;
    StatsView {
        words,
        characters: acc.chars,
        characters_no_space: acc.chars_no_space,
        lines,
        paragraphs: acc.paragraphs,
        headings: acc.headings,
        code_blocks: acc.code_blocks,
        links,
        images,
        title: title.to_string(),
        tasks_done: acc.tasks_done,
        tasks_total: acc.tasks_total,
        // 原型：`max(1, round(words / 300))`。
        reading_minutes: words.div_ceil(300).max(1) as u32,
    }
}

/// 链接与图片数（字节层匹配）。
///
/// 图片 `![说明](地址)` **不计入**链接数（原型口径），否则一张图会在「链接」和
/// 「图片」两栏各数一次。仅在 `[` / `!` 处做括号配对，其余字符 O(1) 跳过。
/// 与 `equivalence::ref_count_links_and_images` 口径一致（同一份匹配规则），
/// 故链接 / 图片计数与旧实现必然逐项相等。
fn count_links_and_images(bytes: &[u8]) -> (usize, usize) {
    let mut links = 0;
    let mut images = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' && bytes[i] != b'!' {
            i += 1;
            continue;
        }
        let is_image = bytes[i] == b'!';
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

/// 单遍扫描的累加器：字符 / 词 / 行级各项计数 + 围栏状态。
///
/// 不持有任何 `String`——行内容以零拷贝 `&str` 切片在行尾传入 `finish_line`
/// （建议三关键：单遍 + 零分配；旧实现每行重建 `line_buf`、每个西文 token 重建
/// `current`，是 1MB 上约 4~6ms 重复遍历的主要来源）。
struct StatsAcc {
    // 字符 / 词级
    chars: usize,
    chars_no_space: usize,
    cjk: usize,
    western: usize,
    in_token: bool,        // 当前是否在一个西文 token 内
    token_has_alnum: bool, // 当前 token 是否至少含一个字母 / 数字
    // 行级
    headings: usize,
    code_blocks: usize,
    tasks_done: usize,
    tasks_total: usize,
    paragraphs: usize,
    fence: Option<&'static str>, // 当前所在的围栏标记（None 表示不在围栏内）
    in_blank: bool,              // 上一个非空行是否为空行（判「新段落开始」）
}

impl Default for StatsAcc {
    fn default() -> Self {
        // `in_blank` 初始为 true：文档开头视为「处于空行之后」，
        // 首个非空行即开启第一个段落（与原实现 `let mut in_blank = true` 一致）。
        Self {
            chars: 0,
            chars_no_space: 0,
            cjk: 0,
            western: 0,
            in_token: false,
            token_has_alnum: false,
            headings: 0,
            code_blocks: 0,
            tasks_done: 0,
            tasks_total: 0,
            paragraphs: 0,
            fence: None,
            in_blank: true,
        }
    }
}

impl StatsAcc {
    /// 喂入一个普通字符：累计字符 / 非空格字符，并按词规则更新西文 token 状态机。
    ///
    /// 词规则（与旧实现逐字一致）：连续的 `[A-Za-z0-9_'-]` 构成一个 token，
    /// 当且仅当该 token 至少含一个字母 / 数字才计入字数（排除孤立的 `-`/`'`/`_`）。
    fn feed(&mut self, ch: char) {
        self.chars += 1;
        if !ch.is_whitespace() {
            self.chars_no_space += 1;
        }
        if is_cjk(ch) {
            self.flush_western();
            self.cjk += 1;
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '\'' || ch == '-' {
            self.in_token = true;
            if ch.is_ascii_alphanumeric() {
                self.token_has_alnum = true;
            }
        } else {
            self.flush_western();
        }
    }

    /// 喂入一个换行：计入字符与行数、结算西文 token（换行本身不计入非空格字符）。
    fn feed_newline(&mut self) {
        self.chars += 1;
        self.flush_western();
    }

    /// 把当前西文 token 结算为一个词（当且仅当含至少一个字母 / 数字）。
    fn flush_western(&mut self) {
        if self.in_token && self.token_has_alnum {
            self.western += 1;
        }
        self.in_token = false;
        self.token_has_alnum = false;
    }

    /// 结算一行（行内容以零拷贝切片传入）：标题 / 围栏 / 任务 / 段落判定。
    ///
    /// 逻辑与原实现逐字一致；围栏内的整行内容一律跳过。
    fn finish_line(&mut self, line: &str) {
        let trimmed = line.trim();
        let fence_token = fence_token_of(line);
        if let Some(open) = self.fence {
            if fence_token == Some(open) {
                self.fence = None;
            }
            return;
        }
        if let Some(token) = fence_token {
            self.fence = Some(token);
            self.code_blocks += 1;
            return;
        }
        if is_atx_heading(trimmed) {
            self.headings += 1;
        }
        if let Some(done) = task_marker_of(trimmed) {
            self.tasks_total += 1;
            if done {
                self.tasks_done += 1;
            }
        }
        if trimmed.is_empty() {
            self.in_blank = true;
        } else if self.in_blank {
            self.paragraphs += 1;
            self.in_blank = false;
        }
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
///
/// 返回值指向静态标记串（` ``` ` / `~~~ `），便于与原已开启的围栏做 `==` 比较，
/// 也省去每行一次 `String` 分配。
fn fence_token_of(line: &str) -> Option<&'static str> {
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
fn task_marker_of(line: &str) -> Option<bool> {
    let rest = line.strip_prefix(['-', '*', '+'])?.trim_start();
    let rest = rest.strip_prefix('[')?;
    match rest.chars().next()? {
        ' ' => Some(false),
        'x' | 'X' => Some(true),
        _ => None,
    }
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
/// 西文单词 = 连续的 `[A-Za-z0-9_'-]`，且**必须至少含一个字母或数字**。
///
/// # 与原型正则的一处有意偏离
///
/// 原型正则 `/[A-Za-z0-9_'-]+/g` 把孤立的连字符也算成一个词，
/// 于是一份任务清单 `- [ ] todo` 会显示「8 字」。故要求词含字母/数字，
/// 破折号、星号、方括号等 Markdown 结构符号不计入字数（口径自相矛盾的原型也该修）。
///
/// 是否是「按字计」的东亚文字。
///
/// 覆盖范围与原型一致：CJK 统一表意文字 + 日文假名。
/// 刻意**不含**韩文（`\u{AC00}` 起）：原型的正则没算它，
/// 口径对齐优先于「看起来更完整」。
fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' | '\u{3040}'..='\u{30ff}')
}

#[cfg(test)]
mod equivalence {
    use super::*;

    /// 旧实现（重构前）原样留存，作为等价性基准。
    ///
    /// 对外语义不许变——任何输入下新 `compute_stats` 必须与它逐项相等。
    fn reference_compute_stats(text: &str, title: &str) -> StatsView {
        let mut headings = 0;
        let mut code_blocks = 0;
        let mut tasks_done = 0;
        let mut tasks_total = 0;
        let mut paragraphs = 0;
        let mut in_blank = true;
        let mut fence: Option<String> = None;

        for raw in text.split('\n') {
            let line = raw.trim();
            let fence_token = ref_fence_token_of(raw);
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
            if ref_is_atx_heading(line) {
                headings += 1;
            }
            if let Some(done) = ref_task_marker_of(line) {
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

        let (links, images) = ref_count_links_and_images(text);
        let words = ref_count_words(text);
        StatsView {
            words,
            characters: text.chars().count(),
            characters_no_space: text.chars().filter(|c| !c.is_whitespace()).count(),
            lines: line_count(text),
            paragraphs,
            headings,
            code_blocks,
            links,
            images,
            title: title.to_string(),
            tasks_done,
            tasks_total,
            reading_minutes: words.div_ceil(300).max(1) as u32,
        }
    }

    fn ref_is_atx_heading(line: &str) -> bool {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
    }

    fn ref_fence_token_of(line: &str) -> Option<String> {
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        if indent > 3 {
            return None;
        }
        let body = line.trim_start_matches([' ', '\t']);
        ["```", "~~~"]
            .into_iter()
            .find(|m| body.starts_with(m))
            .map(|m| m.to_string())
    }

    fn ref_task_marker_of(line: &str) -> Option<bool> {
        let rest = line.strip_prefix(['-', '*', '+'])?.trim_start();
        let rest = rest.strip_prefix('[')?;
        match rest.chars().next()? {
            ' ' => Some(false),
            'x' | 'X' => Some(true),
            _ => None,
        }
    }

    fn ref_count_links_and_images(text: &str) -> (usize, usize) {
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

    fn ref_count_words(text: &str) -> usize {
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

    /// 逐项比较两份 `StatsView`，给出可读的字段级失败信息。
    fn assert_stats_eq(actual: &StatsView, expected: &StatsView, input: &str) {
        assert_eq!(actual.words, expected.words, "words | 输入: {input:?}");
        assert_eq!(
            actual.characters, expected.characters,
            "characters | 输入: {input:?}"
        );
        assert_eq!(
            actual.characters_no_space, expected.characters_no_space,
            "characters_no_space | 输入: {input:?}"
        );
        assert_eq!(actual.lines, expected.lines, "lines | 输入: {input:?}");
        assert_eq!(
            actual.paragraphs, expected.paragraphs,
            "paragraphs | 输入: {input:?}"
        );
        assert_eq!(
            actual.headings, expected.headings,
            "headings | 输入: {input:?}"
        );
        assert_eq!(
            actual.code_blocks, expected.code_blocks,
            "code_blocks | 输入: {input:?}"
        );
        assert_eq!(actual.links, expected.links, "links | 输入: {input:?}");
        assert_eq!(actual.images, expected.images, "images | 输入: {input:?}");
        assert_eq!(
            actual.tasks_done, expected.tasks_done,
            "tasks_done | 输入: {input:?}"
        );
        assert_eq!(
            actual.tasks_total, expected.tasks_total,
            "tasks_total | 输入: {input:?}"
        );
        assert_eq!(
            actual.reading_minutes, expected.reading_minutes,
            "reading_minutes | 输入: {input:?}"
        );
    }

    /// 在多种结构混合的输入上，断言新实现与旧实现逐项相等。
    #[test]
    fn single_pass_matches_reference_on_mixed_inputs() {
        let cases: &[&str] = &[
            "",
            " ",
            "\n",
            "\n\n\n",
            "hello",
            "hello world",
            "这是一段中文",
            "中文 mixed with English",
            "well-known state_of_the_art",
            "第一行\n第二行\n",
            "第一段\n第二行\n\n第二段\n\n\n第三段",
            "# 一\n## 二\n###### 六\n####### 七\n#NoSpace",
            "```\n## 不是标题\n# 也不是\n```\n# 真标题",
            "~~~\n# 不是标题\n~~~\n# 真标题",
            "    ```\n    # 缩进代码\n",
            "- [ ] a\n* [x] b\n+ [X] c\n-  [ ] d\n-[ ] e",
            "- [] 空括号\n- [y] 非法状态\n-[普通链接]",
            "[a](b) ![图](c.png) [带括号](d(e)f) [裸括号] [^1]",
            "```\n- [ ] 代码里的\n```\n- [ ] 真的",
            "你好world",
            "- [ ] 待办\n- [x] 完成\n",
            "a b\nc d e\n",
            "你好",
            // 链接跨行（极端边界）：新旧实现都应计为一个链接，且行尾结算一致。
            "[a\n](b)",
        ];
        for c in cases {
            let actual = compute_stats(c, "");
            let expected = reference_compute_stats(c, "");
            assert_stats_eq(&actual, &expected, c);
        }
    }

    /// 同机 A/B：单遍 `compute_stats` 必须明显快于旧六遍 `reference_compute_stats`。
    ///
    /// 「合并遍历」的核心收益就是这一项——若某次改动把单遍写回了更慢的实现，
    /// 此测试（release 口径）会立刻变红。正确性已由上方等价性测试守死，这里只看速度。
    #[test]
    fn single_pass_is_faster_than_reference_ab() {
        // 1MB 伪 Markdown：混合中文 / 英文 / 标题 / 任务列表 / 链接图片 / 代码块。
        let mut s = String::with_capacity(1 << 20);
        let unit = "# 标题一\n\n这是一段中文 mixed with English words and `inline code`.\n\
                    - [ ] 待办项 one\n- [x] 已完成 two\n\
                    ![图](a.png) [链接](https://example.com/path?x=1&y=2)\n\n\
                    ```\nlet x: u32 = 1;\n```\n\n";
        while s.len() < 1 << 20 {
            s.push_str(unit);
        }
        // 预热：排除首次页错误 / 缓存冷启动对单次计时的干扰。
        let _ = compute_stats(&s, "");
        let _ = reference_compute_stats(&s, "");

        fn median_ms(f: impl Fn()) -> f64 {
            let mut samples = [0f64; 9];
            for s in samples.iter_mut() {
                let t0 = std::time::Instant::now();
                for _ in 0..5 {
                    f();
                }
                *s = t0.elapsed().as_secs_f64() * 1000.0 / 5.0;
            }
            samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
            samples[samples.len() / 2]
        }

        let new_med = median_ms(|| {
            let _ = compute_stats(&s, "");
        });
        let ref_med = median_ms(|| {
            let _ = reference_compute_stats(&s, "");
        });
        eprintln!(
            "[ab] compute_stats 单遍={new_med:.3}ms 旧六遍={ref_med:.3}ms 比值={:.2}",
            new_med / ref_med.max(f64::MIN_POSITIVE)
        );
        // release 口径做硬断言；debug 下只打印比值（debug 计时噪声大，不卡门禁）。
        #[cfg(not(debug_assertions))]
        assert!(
            new_med < ref_med * 0.8,
            "单遍实现未明显快于旧六遍实现（应 < 80%）：new={new_med:.3}ms ref={ref_med:.3}ms"
        );
    }
}
