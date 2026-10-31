//! 零依赖英文拼写检查器（路线①）。
//!
//! # 算法：分词 + 近邻门槛
//!
//! 1. **分词**：按非字母数字切分（等价于「空白 + 标点」）；连续 `[A-Za-z]` 构成一个候选词。
//! 2. **跳过规则**（误报防线，见下）：含数字、含内部大写（驼峰 / 专有名词）、
//!    长度 ≤ 2、URL / 路径残片等一律不判错。
//! 3. **门槛**：**只有存在编辑距离 ≤ 1 的词表近邻时才判错**。
//!
//! # 为什么要有「近邻门槛」
//!
//! 词表只有 3210 词，裸判定（不在词表 → 错）会把这个Markdown 编辑器变成
//! 圣诞树：`Kubernetes`、`serde`、`egui`、人名、地名全部标红。
//! 加上门槛后，`teh` / `recieve` / `seperate` 这类**真错词**因贴近 `the` /
//! `receive` / `separate` 被抓出；而**生僻词与专有名词因找不到近邻而放行**。
//!
//! 这正是 hunspell 的核心思想（用「有候选替换」而非「不在词典」来判错），
//! 只是这里退化成了**编辑距离 1 + 小词表**的形态——精度有限，但**误报率极低**，
//! 而误报率才是这类「辅助提示」功能的生命线。
//!
//! 代价：漏报（生僻范围内的错词抓不到）。这是刻意的取舍。

use rustmark_core::domain::markdown::errors::SpellCheckError;
use rustmark_core::domain::markdown::services::SpellChecker;
use rustmark_core::domain::markdown::value_objects::Misspelling;
use rustmark_core::shared::geometry::{ByteOffset, TextRange};

use super::dictionary;

/// 短于该长度的词不判错（`a` / `an` / `is` 之外的短 token 多为噪声或缩写）。
const MIN_CHECK_LEN: usize = 3;

/// 启用近邻门槛时的最大编辑距离。
const MAX_EDIT_DIST: usize = 1;

/// 建议的**最大**编辑距离（V11-07）。
///
/// 取 2 而判错仍取 1：判错放宽会把整篇技术文档标红（误报是这类功能的生命线），
/// 而建议放宽只是让候选多一些——用户看不到想选的词时顶多放弃替换，
/// 不会因此被误导。两条门槛取不同值正是「判错的代价 ≫ 漏建议的代价」。
const MAX_SUGGEST_DIST: usize = 2;

/// 基于内置词表的拼写检查器。
pub struct VendoredSpellChecker {
    /// 预先算好的「词的删除变体」索引：`word` → `删除任一字符后的形态`集合。
    ///
    /// 有了它，判断「是否存在编辑距离 ≤ 1 的词表词」无需扫描全词表：
    /// 对候选词生成其删除变体，查表命中即说明存在近邻。由O(词长) 降为 O(词长) 哈希查。
    deletions: std::collections::HashSet<String>,
}

impl Default for VendoredSpellChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl VendoredSpellChecker {
    /// 构建检查器并建立近邻索引。
    pub fn new() -> Self {
        let mut deletions = std::collections::HashSet::new();
        for word in dictionary::iter_words() {
            // 词长 ≤ MAX_EDIT_DIST 的词，其删除变体可能与空串混淆，不入索引。
            if word.len() <= MAX_EDIT_DIST {
                continue;
            }
            for i in 0..word.len() {
                if !word.is_char_boundary(i) {
                    continue;
                }
                let mut s = word.to_string();
                s.remove(i);
                deletions.insert(s);
            }
        }
        Self { deletions }
    }

    /// 候选词是否存在词表近邻（编辑距离 ≤ [`MAX_EDIT_DIST`]）。
    fn has_near_neighbor(&self, lower: &str) -> bool {
        if is_known(lower) {
            return false; // 本身就在词表里（或其词干在），不是错词
        }
        // 编辑距离 1 有三种形态，各对应一条判定（缺一条就漏掉该形态）：
        // - **多字母**（`thee` → `the`）：候选删一字符后直接是词表词。
        // - **少字母**（`mesage` → `message`）：候选本身就是索引中的删除变体。
        // - **替换 / 易位**（`teh`→`the`、`reciev`→`receive`）：
        //   两者删掉同一位置后相等，故「候选的某个删除变体也在索引中」即命中。
        //
        // 索引 `deletions` 存的是**词表词的全部删除变体**，故上面三条只需
        // 查两个集合，无需枚举字母表。
        //
        // 形态一：候选删一字符后即词表词。
        for i in 0..lower.len() {
            if !lower.is_char_boundary(i) {
                continue;
            }
            let mut s = lower.to_string();
            s.remove(i);
            if is_known(&s) {
                return true;
            }
        }
        // 形态二：候选本身是某词表词的删除变体（少了一个字母）。
        if self.deletions.contains(lower) {
            return true;
        }
        // 形态三：候选的某个删除变体也在索引里（与某词表词仅差一次替换/易位）。
        (0..lower.len())
            .filter(|i| lower.is_char_boundary(*i))
            .any(|i| {
                let mut s = lower.to_string();
                s.remove(i);
                self.deletions.contains(&s)
            })
    }
}

/// 在词表里找与 `word` 距离 ≤ [`MAX_SUGGEST_DIST`] 的词（**纯函数**，便于单测）。
///
/// # 边界
///
/// - `word` 为空 / `limit` 为 0 → 空集；
/// - `word` 不是纯 ASCII 字母（中文、数字、标点）→ 空集：
///   检查器本来就只判 ASCII 词，给中文提英文建议毫无意义；
/// - 词本身在词表里（或其词干在）→ 空集：它不是错词，不该有建议；
/// - 找不到候选 → 空集。宁可不给建议，也不给离谱的。
///
/// # 排序：距离优先，字典序次之
///
/// 距离相同时按字典序排，保证结果**稳定**：否则 `HashSet` 的迭代顺序
/// 会让同一个词每次给出不同顺序的建议，菜单看起来在抖动。
pub fn suggest_candidates(word: &str, limit: usize) -> Vec<String> {
    if word.is_empty()
        || limit == 0
        || !word.bytes().all(|b| b.is_ascii_alphabetic())
        || is_known(word)
    {
        return Vec::new();
    }
    // 长度差超过门槛的词不可能在距离内（编辑距离 ≥ 长度差），先筛掉——
    // 这一条把绝大多数词表词挡在距离计算之外。
    let len = word.chars().count();
    let mut scored: Vec<(usize, &str)> = dictionary::iter_words()
        .filter(|w| {
            let d = w.chars().count();
            d.saturating_sub(len).min(len.saturating_sub(d)) <= MAX_SUGGEST_DIST
        })
        .map(|w| (typo_distance(word, w), w))
        .filter(|(d, _)| *d <= MAX_SUGGEST_DIST && *d > 0)
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, w)| w.to_string())
        .collect()
}

/// 「打字错误距离」：Levenshtein，但**相邻字符易位记 1 步**。
///
/// # 为什么必须把易位单独算作 1
///
/// 纯 Levenshtein 把 `teh → the` 记成 **2**（换两个字符），
/// 而它其实是「手比脑快」打反了一次——人类最常见的错词形态之一
/// （Damerau 早就指出：80% 的拼写错误是「替换 / 增删 / 易位」四类）。
/// 按 2 记会让 `the` 排在一堆距离 1 的无关词（`tea` / `tel`）后面，
/// 用户翻完五条也看不到自己想打的那个词。
///
/// 只处理**相邻**易位：非相邻交换（`abcd → badc`）在真实打字里几乎不出现，
/// 为它付出复杂度不划算。
pub(crate) fn typo_distance(a: &str, b: &str) -> usize {
    if is_adjacent_transposition(a, b) {
        return 1;
    }
    edit_distance(a, b)
}

/// 两串是否**恰好**由一次相邻字符易位得到。
fn is_adjacent_transposition(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len() != b.len() || a.len() < 2 {
        return false;
    }
    let mut diffs = (0..a.len()).filter(|i| a[*i] != b[*i]);
    let Some(i) = diffs.next() else {
        return false; // 完全相同，不是「错词」
    };
    let Some(j) = diffs.next() else {
        return false; // 只差一个字符 = 替换，交给 Levenshtein
    };
    if diffs.next().is_some() {
        return false; // 差三处以上
    }
    j == i + 1 && a[i] == b[j] && a[j] == b[i]
}

/// 词是否可识别：**原形**在词表中，或**任一词干**在词表中。
///
/// # 为什么需要词干回退
///
/// 词表存的是原形（`color` / `drop`），而文本里出现的是各种屈折形态
/// （`colors` / `dropped` / `dropping` / `boxes`）。若无词干回退，
/// 这些**完全正确**的词会因「找不到近邻」被标红——实测一份技术文档里
/// 53 个误报绝大多数正是此类。
///
/// 这正是 hunspell 用「词缀规则」解决的问题；此处用一组保守的英语后缀规则近似，
/// 只覆盖高频形态，避免过度生成把真错词也放进白名单。
fn is_known(word: &str) -> bool {
    if dictionary::contains(word) {
        return true;
    }
    stems(word).any(|s| dictionary::contains(&s))
}

/// 枚举可能的英语词干（**纯函数**，供测试直接断言）。
///
/// 规则刻意保守——每条都必须「脱掉后仍是常见词」，否则宁可漏判不可误判：
/// - 复数 / 第三人称：`-s` / `-es` / `-ies`→`y`；
/// - 过去式 / 过去分词：`-ed` / `-ied`→`y` / `-d`；
/// - 进行时：`-ing`，含「双写辅音还原」（`running`→`run`）与「去 e 还原」（`moving`→`move`）。
fn stems(word: &str) -> impl Iterator<Item = String> {
    let mut out: Vec<String> = Vec::new();
    let w = word;

    // `-ies` → `y`（`stories` → `story`）
    if let Some(base) = w.strip_suffix("ies") {
        out.push(format!("{base}y"));
    }
    // `-es` → 去尾（`boxes` → `box`）
    if let Some(base) = w.strip_suffix("es") {
        out.push(base.to_string());
    }
    // `-s` → 去尾（`colors` → `color`）。要求去尾后至少 3 字母，
    // 否则 `is` / `as` 会被剥成 `i` / `a` 之类噪声。
    if w.len() > 3
        && let Some(base) = w.strip_suffix('s')
    {
        out.push(base.to_string());
    }
    // `-ied` → `y`（`cried` → `cry`）
    if let Some(base) = w.strip_suffix("ied") {
        out.push(format!("{base}y"));
    }
    // `-ed` → 去尾（`dropped` → `dropped`→`dropp` 需再还原双写）
    if w.len() > 3
        && let Some(base) = w.strip_suffix("ed")
    {
        out.push(base.to_string());
        // 双写辅音还原：`running` → `runn` → `run`
        let mut chars = base.chars();
        if let (Some(last), Some(prev)) = (chars.next_back(), chars.next_back())
            && last == prev
            && !"aeiou".contains(last)
        {
            let shortened: String = base[..base.len() - last.len_utf8()].to_string();
            out.push(shortened);
        }
    }
    // `-ing` → 去尾（`moving` → `mov` → 补回 `e` 得 `move`）
    if w.len() > 4
        && let Some(base) = w.strip_suffix("ing")
    {
        out.push(base.to_string());
        let mut chars = base.chars();
        if let (Some(last), Some(prev)) = (chars.next_back(), chars.next_back()) {
            // 双写辅音还原：`running` → `runn` → `run`
            if last == prev && !"aeiou".contains(last) {
                out.push(base[..base.len() - last.len_utf8()].to_string());
            }
            // 去 e 还原：`moving` → `move`
            if !"aeiou".contains(last) {
                out.push(format!("{base}{last}"));
            }
        }
    }
    out.into_iter()
}

impl SpellChecker for VendoredSpellChecker {
    fn check(&self, text: &str) -> Result<Vec<Misspelling>, SpellCheckError> {
        if !self.is_available() {
            return Err(SpellCheckError::new("内置词表为空，拼写检查已跳过"));
        }
        Ok(check_english(text, |word| self.has_near_neighbor(word)))
    }

    fn is_available(&self) -> bool {
        dictionary::is_available()
    }

    fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        if !self.is_available() {
            return Vec::new();
        }
        // 建议按「用户看到的形态」算：判错用的是小写词，
        // 而菜单里要显示原拼写，故在此归一化。
        suggest_candidates(&word.to_ascii_lowercase(), limit)
    }
}

/// 分词并收集疑似错词区间（**纯函数**，便于无头测试）。
///
/// `is_typo` 接收**已转小写**的纯字母词，返回是否判错。抽成参数是为了让测试
/// 能用「全判错 / 全不判错」的极端实现验证分词与跳过规则本身。
pub fn check_english(text: &str, mut is_typo: impl FnMut(&str) -> bool) -> Vec<Misspelling> {
    let mut out = Vec::new();
    for (start, candidate) in tokens(text) {
        let Some(word) = normalize_candidate(candidate) else {
            continue;
        };
        if is_typo(&word) {
            out.push(Misspelling::new(TextRange::new(
                ByteOffset::new(start),
                ByteOffset::new(start + candidate.len()),
            )));
        }
    }
    out
}

/// 产出 `(字节起点, 候选词)` 对：连续的 ASCII 字母构成一个候选。
///
/// 非 ASCII（中文等）天然被切断——按字节扫描时每个非字母字节都是分隔符，
/// 故中文文本不会产出任何候选，无需额外分支。
fn tokens(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.push((start, &text[start..i]));
        } else {
            i += 1;
        }
    }
    out.into_iter()
}

/// 候选词 → 可判定的小写词；不该判错则返回 `None`。
///
/// 这里是**误报防线集中地**，每条规则都对应一类真实噪声：
/// - 内部含大写（`camelCase`、`ParseError`）：标识符与复合词；
/// - 首字母大写（`The`、`Rust`）：句首 / 专有名词，小写形态无法区分；
/// - 长度 < [`MIN_CHECK_LEN`]：噪声与短缩写。
///
/// **注意**：本函数不做「词长上限」过滤。超长 token 由近邻门槛自然放过
/// （`Kubernetes` 找不到近邻），无需再添一条可能误伤的规则。
fn normalize_candidate(candidate: &str) -> Option<String> {
    if candidate.len() < MIN_CHECK_LEN {
        return None;
    }
    let first_upper = candidate.as_bytes()[0].is_ascii_uppercase();
    let rest_has_upper = candidate.as_bytes()[1..]
        .iter()
        .any(|b| b.is_ascii_uppercase());
    if rest_has_upper {
        // 首字母大写 + 其余含大写 → 驼峰标识符（camelCase / PascalCase）。
        return None;
    }
    if first_upper && candidate.len() > 1 {
        // 纯首字母大写：可能是句首，也可能是专有名词，一律放过。
        // 代价：`Teh` 这类句首错词抓不到——比满屏误报可接受得多。
        return None;
    }
    Some(candidate.to_ascii_lowercase())
}

/// Levenshtein 距离（**测试与诊断用**，检查热路径不调用）。
///
/// 保留为`pub` 是因为「近邻门槛」的正确性最终要靠「距离确实是 1」来断言，
/// 而词表近邻索引做的是**结构化判定**，需要一个独立的距离实现来交叉验证。
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}
