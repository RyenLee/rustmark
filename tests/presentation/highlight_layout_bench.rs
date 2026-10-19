//! A1-4 排版层单帧成本基准（1MB / 58k 行，单行编辑）。
//!
//! # 测什么
//!
//! `highlight_layout_with_find` 在一次「行内改一个字符」后的**整段耗时**，
//! 以及这一帧是否走了「单行原地补丁」快路径。
//!
//! 与 `highlight_perf_test.rs` 的分工：那里测 TextEdit **整帧**（用户感知），
//! 这里把排版层单独拎出来（优化落点）。两者不可互相替代。
//!
//! 数字必须 release 下取（debug 差约一个数量级）：
//!
//! ```text
//! cargo test --release --test presentation bench_layout_phases -- --ignored --nocapture
//! ```

use std::time::Instant;

use rustmark::presentation::theme::builtin::Theme;
use rustmark::presentation::theme::mapper::{
    clear_highlight_caches, highlight_layout_with_find, last_highlight_stats,
};

/// 1MB 档的语料规模。
const MIB: usize = 1024 * 1024;
/// 参与取样的连续编辑轮数。
const ROUNDS: usize = 30;

/// 合成接近 `target_bytes` 的中文 Markdown 语料（整块拼接，不会切断 UTF-8）。
fn markdown_sample(target_bytes: usize) -> String {
    let unit = "# 标题 H1\n\n\
                这是一段中文正文，用于模拟真实编辑场景：包含 **加粗**、`行内代码` \
                与 [链接](https://example.com)。\n\n\
                - 列表项一\n\
                - 列表项二\n\n\
                > 引用行\n\n\
                ```rust\nfn main() { println!(\"hi\"); }\n```\n\n";
    let mut s = String::new();
    while s.len() < target_bytes {
        s.push_str(unit);
    }
    s
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// 在 `from` 之后找下一个可安全替换（不切断 UTF-8）的 ASCII 位置。
fn next_ascii_pos(text: &str, from: usize) -> usize {
    // `from` 来自字节算术，可能落在多字节字符中间；先向下吸附到字符边界。
    let mut start = from.min(text.len());
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    text[start..]
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphanumeric())
        .map(|(i, _)| start + i)
        .expect("样本里应有 ASCII 字符")
}

#[test]
#[ignore = "性能基准：需 release 下显式运行"]
fn bench_layout_phases_on_1mb() {
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});
    let theme = Theme::dark();
    let text = markdown_sample(MIB);

    // 预热：字体装载等一次性成本不计入稳态。
    let _ = highlight_layout_with_find(&ctx, &theme, &text, None, 900.0, None);

    let mut samples = Vec::with_capacity(ROUNDS);
    let mut patched = 0usize;
    let mut current = text.clone();

    for round in 0..ROUNDS {
        // 在文档中部附近改一个 ASCII 字符（等长替换，行数与行高都不变）。
        let pivot = current.len() / 3 + round * 7;
        let at = next_ascii_pos(&current, pivot);
        let old_char = current[at..].chars().next().expect("位置有效");
        let new_char = if old_char == 'a' { 'b' } else { 'a' };
        let mut next = current.clone();
        next.replace_range(at..at + old_char.len_utf8(), &new_char.to_string());

        let started = Instant::now();
        let galley = highlight_layout_with_find(&ctx, &theme, &next, None, 900.0, None);
        samples.push(started.elapsed().as_secs_f64() * 1e3);
        if last_highlight_stats().patched {
            patched += 1;
        }
        // 文本必须真的变了，否则测的是整篇缓存命中路径（不是 miss 路径）。
        assert_eq!(galley.text(), next.as_str());
        current = next;
    }

    println!(
        "[metrics] 1MB 排版层单行编辑 {ROUNDS} 轮：p50 {:.3} ms ｜ min {:.3} ms ｜ 补丁命中 {patched}/{ROUNDS}",
        median(samples.clone()),
        samples.iter().cloned().fold(f64::INFINITY, f64::min),
    );
    clear_highlight_caches();
}
