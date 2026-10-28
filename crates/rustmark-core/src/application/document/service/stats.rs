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
//! [`equivalence`] 里的 `reference_compute_stats` 对拍守住（对外语义不许变）。
//!
//! # 模块结构
//!
//! 本文件只留**编排**：[`compute_stats`] 驱动那一遍扫描，把字符喂给累加器、
//! 字节喂给链接计数器；各子判定分在私有子模块里（纯逻辑外提，可直接测）：
//! - `classify`：行级（ATX 标题 / 围栏 / 任务项）与字符级（是否按字计的东亚文字）判定；
//! - `links`：链接 / 图片计数（字节层，括号嵌套配对）；
//! - `accumulator`：单遍累加器 `StatsAcc`（字符 / 词 / 行级计数 + 围栏状态）；
//! - `equivalence`：旧实现原样留存的等价性基准（仅 `cfg(test)`）。

use crate::application::dto::StatsView;

#[cfg(test)]
use crate::shared::text::line_count;

use accumulator::StatsAcc;
use links::count_links_and_images;

// 任务标记识别的三个入口（`classify`）：R-11 起是**跨 crate 唯一真源**——
// 预览区的勾选回写（root `preview_view::list`）此前自写一套判定，与统计
// 侧口径分家（大写 `[X]` 点不动、正文里的 `[x]` 反被改）。三个入口同族
// 导出，规则本身只在 `task_marker_state_at` 里写了一遍。
pub use classify::{find_task_marker, task_marker_of, task_marker_state_at};

mod accumulator;
mod classify;
mod links;

#[cfg(test)]
mod equivalence;

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
