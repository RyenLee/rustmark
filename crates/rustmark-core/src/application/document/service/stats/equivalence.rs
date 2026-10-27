//! 单遍统计的**等价性基准**：旧实现原样留存，逐项对拍。
//!
//! 只在本模块被 `cargo test` 编译时存在（`super` 里以 `#[cfg(test)]` 挂载），
//! 因此不影响 release 体积——它存在的唯一意义是给「合并六遍为单遍」这场重构
//! 留下可执行的反证：任何输入下新 [`super::compute_stats`] 必须与旧实现
//! 逐项相等，且（release 口径）明显更快。

use super::classify::is_cjk;
use super::links::link_tail_len;
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
