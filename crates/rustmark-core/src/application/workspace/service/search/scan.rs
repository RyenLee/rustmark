//! 跨文件搜索的**文本扫描纯函数**：字符比较、行号游标、字面量 / 正则两条
//! 分支与命中片段窗口。
//!
//! 从用例编排（[`super`]）里分出来的理由只有一条：这些都是 `&str → Vec`
//! 的纯计算，不碰服务状态、不碰 IO。单独成模块后，它们可以在**不构造**
//! `WorkspaceAppService`（仓储 / 事件总线 / 时钟 / 注册表全套依赖）的前提下
//! 直接测——这是把「上限保护」「行号口径」这类容易静默退化的不变量钉住的
//! 前提。
//!
//! 两条分支此前**各写一遍**行号推进逻辑，抽到同一处后由
//! [`tests::literal_and_regex_paths_report_the_same_lines`] 与全仓行号真源
//! [`crate::shared::text::line_index_at`] 双重对拍。抽纯函数最大的风险是
//! 两处实现静默分家（不编译也不报错），交叉对拍是唯一的解药。
//!
//! # 片段窗口与高亮区间
//!
//! [`snippet_window`] 只回答「该截哪一段」；命中在片段里的**高亮区间**由
//! 窗口边界与命中偏移共同决定（`start_off - window.start`）。窗口两端都向下
//! 对齐到 UTF-8 字符边界且**不跨行**，故高亮区间必然落在窗口内、且不会切在
//! 多字节字符中间——这两条不变量分别由 [`snippet_bounds`] 的 `debug_assert`、
//! [`tests::snippet_bounds_cover_the_match`] 与
//! [`tests::snippet_bounds_never_split_a_utf8_char`] 守住。

use crate::shared::geometry::floor_char_boundary;
use crate::shared::pattern::Regex;

/// 单次命中的中间结果（字节偏移 + 行列 + 上下文片段）。
pub(super) struct RawHit {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) line: usize,
    pub(super) col: usize,
    pub(super) snippet: String,
}

/// 大小写不敏感的单字符比较，不分配。
///
/// `char::to_lowercase()` 是迭代器，`Iterator::eq` 直接逐项比较。原实现
/// `tc.to_lowercase().collect::<Vec<char>>() == pc.to_lowercase().collect::<Vec<char>>()`
/// 会在**每一次**字符试配时申请两个小 Vec——1MB 文档按字符逐一试配即
/// 数十万次堆分配，独占单次搜索的大头（见 `tests/presentation/search_perf_test.rs`）。
pub(super) fn char_eq_ignore_case(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    // 只要有一侧是 ASCII，等值判定就退化为 ASCII 忽略大小写比较：
    // 非 ASCII 字符的小写永远不可能等于 ASCII 字符。
    if a.is_ascii() || b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    a.to_lowercase().eq(b.to_lowercase())
}

/// 增量行号游标：把命中的字节偏移换算成 0-based 行号与列偏移。
///
/// # 为什么是游标而不是每次从头数
///
/// 命中位置在单文件内**单调递增**，跨命中累计扫过的字节只需 O(n) 一遍。
/// 若每条命中都重新 `text[..start]` 数换行再 `rfind`，单文件 50 条命中
/// 就是 50 遍全文扫描（问题 048 的原始形态）。
///
/// # 为什么抽出来
///
/// 字面量路径与正则路径各写过一遍这三行推进逻辑。两处的行号口径必须
/// **完全一致**——同一个文件用不同搜索模式显示不同行号是明确的 bug——
/// 此前靠「改一处记得改另一处」维持。抽出后只有一处，并由
/// [`tests::cursor_agrees_with_the_single_source_line_index`] 与
/// [`crate::shared::text::line_index_at`]（全仓行号口径的唯一出处）对拍。
#[derive(Default)]
pub(super) struct LineCursor {
    /// 已数到的行号（0-based）。
    line: usize,
    /// [`Self::counted_to`] 所在行的行首字节偏移。
    line_start: usize,
    /// 已数过换行的右边界（恒为字符边界）。
    counted_to: usize,
}

impl LineCursor {
    /// 把 `[counted_to, to)` 之间的新字节数进去。
    ///
    /// `to` 必须是字符边界且 ≥ 已计边界；不满足时（命中位置回退）不做任何事，
    /// 保证游标只前进不后退——回退会让行号变小，比不精确更糟。
    pub(super) fn advance_to(&mut self, text: &str, to: usize) {
        if to <= self.counted_to {
            return;
        }
        let chunk = &text[self.counted_to..to];
        self.line += chunk.bytes().filter(|&b| b == b'\n').count();
        if let Some(pos) = chunk.rfind('\n') {
            self.line_start = self.counted_to + pos + 1;
        }
        self.counted_to = to;
    }

    /// 命中起点在当前行内的列偏移（字节）。
    pub(super) fn col_at(&self, start: usize) -> usize {
        start - self.line_start
    }
}

/// 命中片段窗口：一段**不跨行、两端对齐字符边界**的字节区间。
///
/// 单独成为类型而不是让 [`snippet_window`] 直接返回 `String`，是为了把
/// 「窗口边界」这一个纯判定从「切片 + 分配」里分出来：边界（行内截断、
/// 上下文半径、UTF-8 对齐）是能静默退化的部分，切片只是搬运。
pub(super) struct SnippetWindow {
    /// 窗口首字节：不早于行首，已向下对齐字符边界。
    start: usize,
    /// 窗口尾字节：不晚于行尾，已向下对齐字符边界。
    end: usize,
}

impl SnippetWindow {
    /// 取出窗口内的文本切片。
    pub(super) fn slice<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }
}

/// 在文本中搜索：`regex` 为 `Some` 走正则分支，否则按字面量
/// （支持大小写不敏感与整词匹配）。
///
/// 字面量路径字符级扫描：以字符为单位比较，大小写不敏感走 Unicode 小写归一化，
/// 整词匹配检查命中两侧的字母数字边界。**仅返回前 `max_per_file` 个命中**
/// （上限保护留给调用方，此处只限制单文件上限以避免超大文件阻塞 UI 帧）。
pub(super) fn search_text(
    text: &str,
    pattern: &str,
    case_sensitive: bool,
    whole_word: bool,
    regex: Option<&Regex>,
    max_per_file: usize,
) -> Vec<RawHit> {
    if let Some(regex) = regex {
        return search_text_regex(text, regex, max_per_file);
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let pat: Vec<char> = pattern.chars().collect();
    if pat.is_empty() || chars.len() < pat.len() {
        return Vec::new();
    }

    let mut hits = Vec::new();
    // 行号与行首**增量**维护：命中位置按 `i` 单调递增，跨命中累计扫过的
    // 字节只需 O(n) 一遍（见 [`LineCursor`]）。
    let mut cursor = LineCursor::default();
    for i in 0..=(chars.len() - pat.len()) {
        let mut matched = true;
        for (j, &pc) in pat.iter().enumerate() {
            let tc = chars[i + j].1;
            let eq = if case_sensitive {
                tc == pc
            } else {
                char_eq_ignore_case(tc, pc)
            };
            if !eq {
                matched = false;
                break;
            }
        }
        if !matched {
            continue;
        }
        if whole_word && !is_whole_word(&chars, i, pat.len()) {
            continue;
        }
        let (start_off, _) = chars[i];
        let (end_off, end_char) = chars[i + pat.len() - 1];
        let end_off = end_off + end_char.len_utf8();

        // 把到命中起点为止的新字节数进游标（两端都是字符边界）。
        cursor.advance_to(text, start_off);
        let col = cursor.col_at(start_off);
        let snippet = snippet_window(text, cursor.line_start, start_off, end_off);

        hits.push(RawHit {
            start: start_off,
            end: end_off,
            line: cursor.line,
            col,
            snippet,
        });
        if hits.len() >= max_per_file {
            break;
        }
    }
    hits
}

/// 命中的左右两侧是否都不是字母数字（整词匹配的边界判定）。
///
/// 抽出来是因为「整词」与 [`crate::application::document::find`] 的
/// `is_word_boundary` 是两套**有意不同**的口径（那边认 `_`，这边不认），
/// 独立成函数后各自的判定才看得清，不会被误合并。
fn is_whole_word(chars: &[(usize, char)], start: usize, len: usize) -> bool {
    let before = start.checked_sub(1).map(|k| chars[k].1);
    let after = chars.get(start + len).map(|&(_, c)| c);
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

/// 正则模式的单文件搜索（[`search_text`] 的正则分支）。
///
/// 与字面量路径同一套约定：命中按字节偏移递增、行号/行首增量维护
/// （正则命中顺序由 `find_in` 从 `from` 向后扫保证单调）、单文件上限
/// `max_per_file` 兜底。大小写敏感已固化在编译期的 [`Regex`] 里，
/// 整词开关不参与（理由见 [`crate::application::workspace::service::WorkspaceAppService::search`]
/// 的文档）。
pub(super) fn search_text_regex(text: &str, regex: &Regex, max_per_file: usize) -> Vec<RawHit> {
    let mut hits = Vec::new();
    let mut cursor = LineCursor::default();
    let mut from = 0usize;
    while hits.len() < max_per_file {
        let Some((start_off, end_off)) = regex.find_in(text, from, text.len()) else {
            break;
        };
        // 与字面量路径同一段增量行号逻辑（同一个 [`LineCursor`]）。
        cursor.advance_to(text, start_off);
        let col = cursor.col_at(start_off);
        let snippet = snippet_window(text, cursor.line_start, start_off, end_off);

        hits.push(RawHit {
            start: start_off,
            end: end_off,
            line: cursor.line,
            col,
            snippet,
        });
        // 前进：普通命中跳到终点；零宽命中（`a*` 可匹配空串）向前推进
        // 一个字符——原地不动会在同一位置无限重配（`find_in` 只扫
        // `from` 之后，零宽命中的起点原样返回）。
        from = if end_off > start_off {
            end_off
        } else {
            let Some(step) = text[start_off..].chars().next().map(char::len_utf8) else {
                break;
            };
            start_off + step
        };
    }
    hits
}

/// 命中片段的字节窗口：命中所在行内、前后各 40 字节，两端对齐字符边界。
///
/// 摘要窗口按**字节**取，而 `±40` 后的落点完全可能停在多字节 UTF-8
/// 字符中间：中文 3 字节、emoji 4 字节，混合排版时 40 的偏移仍可能
/// 切进字符内部，直接切片会 panic（崩溃记录
/// docs/logs/crash-20261014135941-29300.txt：`end byte index 305 is not
/// a char boundary; it is inside '行'`）。切片前把两端都向下对齐到字符
/// 边界：最多少取半个字符，摘要语义不受影响；`find('\n')` 那条路径
/// 本就是边界，对齐是恒等的。
///
/// 返回的窗口必然覆盖 `[start_off, end_off]`——高亮区间
/// （[`SnippetWindow::highlight`]）由此成立。
pub(super) fn snippet_bounds(
    text: &str,
    line_start: usize,
    start_off: usize,
    end_off: usize,
) -> SnippetWindow {
    let start = floor_char_boundary(text, line_start.max(start_off.saturating_sub(40)));
    let end = floor_char_boundary(
        text,
        (text[end_off..]
            .find('\n')
            .map(|x| end_off + x)
            .unwrap_or(text.len()))
        .min(end_off + 40),
    );
    // 窗口必须**完整覆盖**命中——下游按 `start_off - start` 定位片段内高亮，
    // 窗口一旦没盖住，高亮区间就会是负数或越界。
    debug_assert!(
        start <= start_off && end_off <= end,
        "片段窗口 [{start}, {end}) 未覆盖命中 [{start_off}, {end_off})"
    );
    SnippetWindow { start, end }
}

/// 取命中上下文片段（前后各 40 字节，行内截断）。
///
/// 边界计算见 [`snippet_bounds`]；本函数只负责切片与分配。
pub(super) fn snippet_window(
    text: &str,
    line_start: usize,
    start_off: usize,
    end_off: usize,
) -> String {
    snippet_bounds(text, line_start, start_off, end_off)
        .slice(text)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 游标与全仓行号真源 [`crate::shared::text::line_index_at`] 逐点相等。
    ///
    /// 交叉对拍而不是自设期望值：`line_index_at` 是编辑区行号栏、状态栏与
    /// 文档统计共用的唯一口径（[`crate::shared::text::line_count`] 的姊妹函数）。
    /// 搜索结果的「行号」若与它分家，用户点搜索结果就会跳到别处。
    #[test]
    fn cursor_agrees_with_the_single_source_line_index() {
        let text = "第一行\nsecond line\n\n第四行\nlast";
        let mut cursor = LineCursor::default();
        for (offset, ch) in text.char_indices() {
            // 逐字符推进（含行首、换行符本身、文末）。
            cursor.advance_to(text, offset);
            assert_eq!(
                cursor.line,
                crate::shared::text::line_index_at(text, offset),
                "offset {offset}（{ch:?}）行号不一致"
            );
            assert_eq!(
                cursor.col_at(offset),
                offset - text[..offset].rfind('\n').map_or(0, |p| p + 1),
                "offset {offset} 列偏移不一致"
            );
        }
        // 文末也算一次（`char_indices` 不含 `text.len()` 之后的位置）。
        cursor.advance_to(text, text.len());
        assert_eq!(
            cursor.line,
            crate::shared::text::line_index_at(text, text.len())
        );
    }

    /// 同一行多次命中：列偏移按行首算，不累计。
    #[test]
    fn repeated_hits_stay_on_the_same_line() {
        let text = "aa aa aa\nbb";
        let mut cursor = LineCursor::default();
        let mut cols = Vec::new();
        for start in [0usize, 3, 6] {
            cursor.advance_to(text, start);
            cols.push((cursor.line, cursor.col_at(start)));
        }
        assert_eq!(cols, vec![(0, 0), (0, 3), (0, 6)]);
    }

    /// 游标只前进不后退：回退的偏移不得让行号变小。
    ///
    /// 正则分支的零宽命中会原地重配（`from` 前推一个字符），若哪天有调用点
    /// 传了更小的偏移，宁可保持当前位置也不能把行号倒回去。
    #[test]
    fn cursor_never_moves_backwards() {
        let text = "a\nb\nc";
        let mut cursor = LineCursor::default();
        cursor.advance_to(text, 4); // 第 2 行（0-based）的 'c'
        assert_eq!(cursor.line, 2);
        cursor.advance_to(text, 1); // 回退请求
        assert_eq!(cursor.line, 2, "回退请求必须被忽略");
        assert_eq!(cursor.line_start, 4);
    }

    /// 两条分支共用同一游标：同一个文件、同一种模式，行号与片段必须一致。
    ///
    /// 字面量分支与正则分支此前各写一遍推进逻辑——这条用例直接比较两条
    /// 路径的产出，任何一处偏移口径改动都会立刻在这里分叉。
    #[test]
    fn literal_and_regex_paths_report_the_same_lines() {
        let text = "alpha\nbeta\nalpha beta\nalpha";
        let literal = search_text(text, "alpha", true, false, None, 100);
        let regex = Regex::compile("alpha", true).expect("编译正则");
        let via_regex = search_text(text, "alpha", true, false, Some(&regex), 100);
        let shape = |hits: &[RawHit]| {
            hits.iter()
                .map(|h| (h.start, h.end, h.line, h.col, h.snippet.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shape(&literal),
            shape(&via_regex),
            "两条分支的产出必须逐项相等"
        );
        assert_eq!(
            literal.iter().map(|h| (h.line, h.col)).collect::<Vec<_>>(),
            vec![(0, 0), (2, 0), (3, 0)]
        );
    }

    /// 字面量分支的整词开关：`category` 里的 `cat` 不算独立词。
    ///
    /// 与 [`crate::application::document::find`] 的整词判定是两套口径
    /// （那边把 `_` 当词字符），这条用例钉住本分支自己的口径。
    #[test]
    fn literal_scan_respects_whole_word() {
        let text = "cat category cat";
        let loose = search_text(text, "cat", true, false, None, 100);
        assert_eq!(loose.len(), 3, "不加整词开关时三处都命中");
        let strict = search_text(text, "cat", true, true, None, 100);
        let starts: Vec<usize> = strict.iter().map(|h| h.start).collect();
        assert_eq!(starts, vec![0, 13], "整词开关必须滤掉 category 里的 cat");
    }

    /// 大小写不敏感比较：ASCII 走快速路径，非 ASCII 走 Unicode 小写迭代。
    ///
    /// 反向对照同样重要——`A` 与 `ä` 必须不等，否则「只要不相等就折叠」
    /// 的写法会假绿。
    #[test]
    fn char_eq_ignore_case_folds_ascii_and_unicode() {
        assert!(char_eq_ignore_case('A', 'a'));
        assert!(char_eq_ignore_case('Ä', 'ä'));
        assert!(char_eq_ignore_case('中', '中'));
        assert!(!char_eq_ignore_case('A', 'ä'));
        assert!(!char_eq_ignore_case('a', 'b'));
    }

    /// 片段窗口必须**完整覆盖**命中：高亮区间（`SnippetWindow::highlight`）
    /// 落在窗口内，且窗口内该区间的切片逐字节等于命中原文。
    ///
    /// 这是「高亮区间」的唯一不变量：UI 侧按 `start_off - window.start`
    /// 定位高亮，窗口一旦没盖住命中，高亮就会错位或越界。
    #[test]
    fn snippet_bounds_cover_the_match() {
        // 20 个中文（60 字节）+ "ab" + 20 个中文：命中落在多字节字符之间。
        let filler = "一二三四五六七八九十壹贰叁肆伍陆柒捌玖拾";
        let text = format!("{filler}ab{filler}");
        let start_off = filler.len();
        let end_off = start_off + 2;
        let window = snippet_bounds(&text, 0, start_off, end_off);
        assert!(
            window.start <= start_off && end_off <= window.end,
            "窗口 [{}, {}) 必须覆盖命中 [{start_off}, {end_off})",
            window.start,
            window.end
        );
        // 高亮区间 = 命中偏移相对窗口首字节（UI 侧 `locate_in_snippet` 的等价物）。
        let hi = start_off - window.start;
        let hj = end_off - window.start;
        assert_eq!(
            &window.slice(&text)[hi..hj],
            "ab",
            "高亮区间内的切片必须等于命中原文"
        );
    }

    /// 窗口两端必须落在 UTF-8 字符边界上。
    ///
    /// 直接对拍崩溃现场（docs/logs/crash-20261014135941-29300.txt）：
    /// 按字节截取后不对齐，切片就会 panic。这里用 floor 后的落点反证——
    /// 真实的落点（20 / 102）都不是字符边界，对齐后才成立。
    #[test]
    fn snippet_bounds_never_split_a_utf8_char() {
        let filler = "一二三四五六七八九十壹贰叁肆伍陆柒捌玖拾";
        let text = format!("{filler}ab{filler}");
        let start_off = filler.len(); // 60
        let end_off = start_off + 2; // 62
        // 未对齐的落点：60-40=20（字符 6..9 中间）、62+40=102（字符中间）。
        assert!(
            !text.is_char_boundary(20) && !text.is_char_boundary(102),
            "用例前提：原始落点确实不在字符边界上"
        );
        let window = snippet_bounds(&text, 0, start_off, end_off);
        assert!(text.is_char_boundary(window.start), "左端必须是字符边界");
        assert!(text.is_char_boundary(window.end), "右端必须是字符边界");
        assert_eq!(window.start, 18);
        assert_eq!(window.end, 101);
    }

    /// 窗口**不跨行**：命中所在行的行尾 `\n` 之后一律不取。
    #[test]
    fn snippet_bounds_stay_inside_the_line() {
        let text = "aaaa\nbbbb\ncccc";
        let window = snippet_bounds(text, 5, 5, 9);
        assert_eq!(window.slice(text), "bbbb", "不得越过行尾吃到下一行");
        assert!(!window.slice(text).contains('\n'));
    }

    /// 上下文半径：前后各 40 字节（行足够长时精确生效）。
    ///
    /// 阈值登记见 `docs/thresholds.baseline.yaml` 的 `search_snippet_context`；
    /// 改了常量而没改基线，这条用例与阈值脚本会同时红。
    #[test]
    fn snippet_window_uses_the_context_radius() {
        let text = "a".repeat(100);
        let window = snippet_bounds(&text, 0, 50, 52);
        assert_eq!(window.start, 10, "左侧 40 字节");
        assert_eq!(window.end, 92, "右侧 40 字节");
        assert_eq!(snippet_window(&text, 0, 50, 52), &text[10..92]);
    }
}
