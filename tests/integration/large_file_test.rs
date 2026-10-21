//! S7-04 大文件与边界场景验证。
//!
//! 依据 `docs/07-集成联调与原型对齐.md` §S7-04，分三组断言：
//!
//! 1. **1MB 四项耗时数字**（打开 / 首屏渲染 / 滚动映射 / 全文查找）——
//!    记录 p50 耗时并设宽松上限，避免性能退化被静默吞掉。
//! 2. **降级触发**（FR-06）——源超`TRUNCATE_THRESHOLD_BYTES`
//!    时 `render_preview` 须返回 `truncated=true` 且不产出块（纯文本降级）。
//! 3. **8 类边界输入不崩溃**——空文档 / 仅 BOM / 10 万行 / 深度嵌套 /
//!    未闭合围栏 / 超长 URL / 非法 UTF-8 / 1MB 全量。
//!
//! # 关于性能阈值的口径
//!
//! 阈值刻意设得宽松（本机debug 构建、无数值优化）：目的是**捕捉数量级退化**
//! （如O(n²) 回归），而非充当性能基准。真实性能基线见 S7-04 报告，
//! 以release 构建在目标平台实测为准。
//!
//! **门禁归属（V11-13 裁定）**：本文件里所有「耗时断言」（`*_is_bounded`）
//! 只在 **release** 构建下生效；debug 构建下只打印 `[metrics]` 指标、不做断言。
//! 理由同 `ci.yml` 的 `perf` job 注释——debug 吞吐约低 40%，绝对毫秒数不可比，
//! 在 debug 下门禁会把达标项误判为不达标。release 门禁由 CI 的 `perf` job
//! （`cargo test --release --test integration large`）常态化执行，故验证链
//! 不再需要 `--skip` 本文件任何用例。

use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use rustmark::application::document::registry::{DocumentRegistry, RegistryProvider};
use rustmark::application::markdown::queries::{MapScrollQuery, RenderPreviewQuery, ScrollSource};
use rustmark::application::markdown::service::{
    DebouncePolicy, MarkdownAppService, RenderCache, TRUNCATE_THRESHOLD_BYTES,
};
use rustmark::domain::document::aggregate::Document;
use rustmark::domain::markdown::errors::ParseError;
use rustmark::domain::markdown::services::MarkdownParser;
use rustmark::domain::markdown::value_objects::RenderedBlock;
use rustmark::infrastructure::highlighting::syntect_highlighter::SyntectHighlighter;
use rustmark::infrastructure::parsing::pulldown_parser::PulldownMarkdownParser;
use rustmark::presentation::state::find_replace_ui_state::{FindOptions, Finder};
use rustmark::shared::ids::DocumentId;
use rustmark::shared::time::{Clock, Timestamp};

use crate::support::text_buffer::buffer_of;

/// 固定时钟（时间恒为 0）。
///
/// S7-04 只关心「降级阈值按字节判定」这件事，与时间无关，故不需要
/// 可变时钟。本地定义而非复用 `tests/common/fakes.rs`：那个文件内部
/// 自带 `mod support`，与本测试目标已声明的 `support` 会触发
/// clippy `duplicate_mod`（同一文件被当作两个模块载入）。
struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(0)
    }
}

// ================================================================
// 夹具
// ================================================================

/// 渲染预览查询（版本号0 = 强制解析，不走缓存）。
fn render_query(id: DocumentId) -> RenderPreviewQuery {
    RenderPreviewQuery {
        doc_id: id,
        version: 0,
    }
}

/// 滚动映射查询（取文档中段偏移）。
fn map_scroll_query(id: DocumentId) -> MapScrollQuery {
    MapScrollQuery {
        doc_id: id,
        source: ScrollSource::Editor { offset: 512 * 1024 },
    }
}

/// 构造一个把 `content` 装进注册表的 Markdown 服务。
///
/// 走真实的 `MarkdownAppService` + `RegistryProvider`，故降级判定走的是
/// 生产代码路径（而不是复制一份阈值判断），避免测试与实现漂移。
fn service_with(content: &str) -> (MarkdownAppService, DocumentId) {
    let doc = doc_of(content);
    let id = doc.id();
    let mut registry = DocumentRegistry::new();
    registry.insert(doc);

    let service = MarkdownAppService::new(
        Arc::new(PulldownMarkdownParser::new()),
        Arc::new(SyntectHighlighter::new()),
        Arc::new(RegistryProvider::new(Arc::new(RwLock::new(registry)))),
        Arc::new(FixedClock),
        DebouncePolicy::new(150),
        Arc::new(Mutex::new(RenderCache::new(8))),
    );
    (service, id)
}

/// 构造一个承载 `content` 的文档（固定时间戳 T0，与既有测试一致）。
fn doc_of(content: &str) -> Document {
    Document::from_content(buffer_of(""), content, Timestamp::from_millis(0)).expect("构造文档失败")
}

/// 取多次耗时的 p50（毫秒）。
fn p50_ms(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("耗时不应为 NaN"));
    samples[samples.len() / 2]
}

/// 取多次耗时的 p95（毫秒）。
fn p95_ms(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("耗时不应为 NaN"));
    samples[(samples.len() * 95 / 100).min(samples.len() - 1)]
}

/// 预热一次再计时：首次调用含惰性初始化（JIT 无关，但含正则/语法集构造），
/// 直接计会把冷启动算进稳态耗时。
fn timed_ms(mut runs: usize, mut op: impl FnMut()) -> Vec<f64> {
    op();
    runs = runs.max(1);
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        op();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples
}

// ================================================================
// 1. 1MB 四项耗时数字
// ================================================================

#[test]
fn large_open_cost_stays_bounded() {
    let markdown = crate::golden_fixture::load_or_generate_large();
    assert!(
        markdown.len() >= 1024 * 1024,
        "样本应达 1MB，实际 {} 字节",
        markdown.len()
    );

    // 「打开」在本headless 口径下 = 解析（生产降级阈值以上时UI 只走纯文本，
    // 但解析耗时仍是上限，故仍测）。
    let parser = PulldownMarkdownParser::new();
    let samples = timed_ms(5, || {
        let blocks = parser.parse(&markdown).expect("1MB 应能解析");
        std::hint::black_box(blocks);
    });
    let p50 = p50_ms(samples);

    println!(
        "[metrics] 1MB 打开/解析：p50={p50:.2}ms（{} 字节）",
        markdown.len()
    );
    // 门禁只在 release 生效（V11-13）；debug 下仅打印指标（见文件头注释）。
    #[cfg(not(debug_assertions))]
    assert!(p50 <= 2000.0, "1MB 解析 p50 {p50:.2}ms 疑似退化");
}

#[test]
fn large_first_screen_render_is_bounded() {
    let markdown = crate::golden_fixture::load_or_generate_large();
    let (service, id) = service_with(&markdown);

    // 首屏 = 首屏可视区那一段的渲染与高亮，是用户感知的「能开始看」的时刻。
    let samples = timed_ms(5, || {
        let view = service.render_preview(render_query(id)).expect("应能渲染");
        std::hint::black_box(view);
    });
    let p50 = p50_ms(samples);

    println!("[metrics] 1MB 首屏渲染：p50={p50:.2}ms（降级路径）");
    // 门禁只在 release 生效（V11-13）；debug 下仅打印指标（见文件头注释）。
    #[cfg(not(debug_assertions))]
    assert!(p50 <= 500.0, "1MB 首屏渲染 p50 {p50:.2}ms 疑似退化");
}

#[test]
fn large_scroll_mapping_is_bounded() {
    let markdown = crate::golden_fixture::load_or_generate_large();
    let (service, id) = service_with(&markdown);

    let samples = timed_ms(5, || {
        let target = service
            .map_scroll(map_scroll_query(id))
            .expect("滚动映射应成功");
        std::hint::black_box(target);
    });
    let p50 = p50_ms(samples);

    println!("[metrics] 1MB 滚动映射：p50={p50:.2}ms");
    // 门禁只在 release 生效（V11-13）；debug 下仅打印指标（见文件头注释）。
    #[cfg(not(debug_assertions))]
    assert!(p50 <= 200.0, "1MB 滚动映射 p50 {p50:.2}ms 疑似退化");
}

/// 1MB 全文查找耗时（大小写不敏感路径）。
///
/// # DEF-01 已修复
///
/// 原缺陷：大小写不敏感字面量查找为 O(N×M)——`find_literal_ignore_case`
/// 每次调用重建整篇 `char_indices()` 表，且 `Finder::scan` 每命中一次调一次。
/// S6-08 已改为单次线性扫描（[`scan_literal_ignore_case_all`]），整体 O(N)，
/// 不再随命中数退化。本用例已从 `#[ignore]` 恢复为门禁。
///
/// 历史实测（修复前，debug 构建）：16KB / 4000 命中 517ms（超线性）。
/// 修复后算法为 O(N)。**门禁阈值 1500ms 仅在 release 构建生效**（V11-13）；
/// debug 下因 Unicode 折叠开销偏慢（~2100ms）但非退化，故 debug 只打印指标。
/// release 门禁由 CI `perf` job（`cargo test --release --test integration large`）执行。
#[test]
fn large_full_text_find_is_bounded() {
    let markdown = crate::golden_fixture::load_or_generate_large();
    let options = FindOptions {
        case_sensitive: false,
        regex: false,
        whole_word: false,
    };

    let samples = timed_ms(5, || {
        let finder = Finder::scan(&markdown, "段落", options).expect("扫描应成功");
        std::hint::black_box(finder.count());
    });
    let p50 = p50_ms(samples.clone());
    let p95 = p95_ms(samples);

    println!("[metrics] 1MB 全文查找：p50={p50:.2}ms p95={p95:.2}ms");
    // 门禁只在 release 生效（V11-13）；debug 下仅打印指标。
    // 大小写不敏感路径在 debug 下因 Unicode 折叠开销偏慢（~2100ms），
    // 但算法为 O(N) 无回归，故阈值以 release 基线为准。
    #[cfg(not(debug_assertions))]
    assert!(
        p50 <= 1500.0,
        "1MB 全文查找 p50 {p50:.2}ms 疑似退化（release 基线）"
    );
}

#[test]
fn large_find_case_sensitive_is_bounded() {
    // 同一份 1MB 样本走**大小写敏感**路径（memchr），作为 DEF-01 的对照基线：
    // 它证明 1MB 规模本身并非不可查找，瓶颈只在折叠路径的算法。
    let markdown = crate::golden_fixture::load_or_generate_large();
    let options = FindOptions {
        case_sensitive: true,
        regex: false,
        whole_word: false,
    };

    let samples = timed_ms(5, || {
        let finder = Finder::scan(&markdown, "段落", options).expect("扫描应成功");
        std::hint::black_box(finder.count());
    });
    let p50 = p50_ms(samples);

    println!("[metrics] 1MB 全文查找（大小写敏感）：p50={p50:.2}ms");
    // 门禁只在 release 生效（V11-13）；debug 下仅打印指标（见文件头注释）。
    #[cfg(not(debug_assertions))]
    assert!(
        p50 <= 1500.0,
        "1MB 大小写敏感查找 p50 {p50:.2}ms 疑似退化（release 基线）"
    );
}

#[test]
fn large_find_returns_expected_hits() {
    // 计时测试之外，另立一条**正确性**断言：1MB 上查找必须真能命中，
    // 否则「很快地返回 0 条」会伪装成性能达标。
    let markdown = crate::golden_fixture::load_or_generate_large();
    let finder = Finder::scan(
        &markdown,
        "段落",
        FindOptions {
            case_sensitive: false,
            regex: false,
            whole_word: false,
        },
    )
    .expect("扫描应成功");

    assert!(
        finder.count() > 1000,
        "1MB 样本应含大量「段落」命中，实际 {}",
        finder.count()
    );
}

// ================================================================
// 2. 降级触发（FR-06）
// ================================================================

#[test]
fn degrade_triggers_above_threshold() {
    let markdown = crate::golden_fixture::load_or_generate_large();
    assert!(
        markdown.len() > TRUNCATE_THRESHOLD_BYTES,
        "1MB 样本应超过降级阈值 {}",
        TRUNCATE_THRESHOLD_BYTES
    );

    let (service, id) = service_with(&markdown);
    let view = service.render_preview(render_query(id)).expect("应能返回");

    assert!(view.truncated, "超阈值必须标记 truncated 以触发 UI 降级");
    assert!(
        view.blocks.is_empty(),
        "降级路径不应解析出块（否则失去降级意义），实际 {} 块",
        view.blocks.len()
    );
}

#[test]
fn no_degrade_below_threshold() {
    // 反向对照：阈值以下**不得**降级，否则大文件优化会误伤普通文档。
    let markdown = "普通文档\n\n第二段。\n";
    assert!(
        markdown.len() <= TRUNCATE_THRESHOLD_BYTES,
        "样本应在阈值以下"
    );

    let (service, id) = service_with(markdown);
    let view = service.render_preview(render_query(id)).expect("应能渲染");

    assert!(!view.truncated, "阈值以下不得降级");
    assert!(!view.blocks.is_empty(), "阈值以下应正常解析出块");
}

#[test]
fn degrade_boundary_is_exactly_at_threshold() {
    // 阈值语义是 `>` 而非 `>=`：恰好等于阈值仍应正常解析。
    let exact = "a".repeat(TRUNCATE_THRESHOLD_BYTES);
    let (service, id) = service_with(&exact);
    let view = service.render_preview(render_query(id)).expect("应能渲染");
    assert!(
        !view.truncated,
        "恰好等于阈值（{TRUNCATE_THRESHOLD_BYTES} 字节）不应降级"
    );
}

#[test]
fn degrade_avoids_parse_cost_entirely() {
    // 降级的意义在于「不解析」。用一个会panic 的解析器替身来证明
    // 降级路径确实**没有**走到解析器——比计时断言更硬。
    struct ExplodingParser;

    impl MarkdownParser for ExplodingParser {
        fn parse(&self, _content: &str) -> Result<Vec<RenderedBlock>, ParseError> {
            panic!("降级路径不应调用解析器");
        }
    }

    let markdown = crate::golden_fixture::load_or_generate_large();
    let doc = doc_of(&markdown);
    let id = doc.id();
    let mut registry = DocumentRegistry::new();
    registry.insert(doc);

    let service = MarkdownAppService::new(
        Arc::new(ExplodingParser),
        Arc::new(SyntectHighlighter::new()),
        Arc::new(RegistryProvider::new(Arc::new(RwLock::new(registry)))),
        Arc::new(FixedClock),
        DebouncePolicy::new(150),
        Arc::new(Mutex::new(RenderCache::new(8))),
    );

    let view = service.render_preview(render_query(id)).expect("应能返回");
    assert!(view.truncated, "应标记降级");
}

// ================================================================
// 3. 8 类边界输入
// ================================================================

#[test]
fn boundary_empty_document() {
    let parser = PulldownMarkdownParser::new();
    let blocks = parser.parse("").expect("空文档应能解析");
    assert!(blocks.is_empty(), "空文档应无块，实际 {}", blocks.len());
}

#[test]
fn boundary_bom_only_document() {
    // 仅 BOM：解码须剥离 BOM，否则正文首部会出现不可见字符。
    let bytes = [0xEF, 0xBB, 0xBF];
    let text = rustmark::infrastructure::persistence::fs_document_repository::FsDocumentRepository::decode_public(
        &bytes,
    );
    assert!(text.is_empty(), "仅 BOM 应解码为空文本，实际 {text:?}");

    let parser = PulldownMarkdownParser::new();
    let blocks = parser.parse(&text).expect("仅 BOM 应能解析");
    assert!(blocks.is_empty(), "仅 BOM 应无块");
}

#[test]
fn boundary_hundred_thousand_lines() {
    let mut text = String::with_capacity(100_000 * 12);
    for i in 0..100_000 {
        text.push('第');
        text.push_str(&i.to_string());
        text.push_str("行内容\n");
    }
    let parser = PulldownMarkdownParser::new();
    let start = Instant::now();
    let blocks = parser.parse(&text).expect("10 万行应能解析");
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    println!(
        "[metrics] 10 万行解析：{elapsed:.2}ms（{} 块）",
        blocks.len()
    );
    assert!(elapsed <= 5000.0, "10 万行解析 {elapsed:.2}ms 疑似退化");
}

#[test]
fn boundary_deep_nesting() {
    // 深度嵌套列表 + 引用：递归型解析器在此最容易栈溢出。
    let mut list = String::new();
    for depth in 0..200 {
        list.push_str(&"  ".repeat(depth));
        list.push_str("- 深层列表项\n");
    }
    let quote = format!("{}> 深层引用\n", "> ".repeat(300));

    let parser = PulldownMarkdownParser::new();
    for (name, text) in [("嵌套列表", list.as_str()), ("嵌套引用", quote.as_str())] {
        // 不panic 即为通过（`panic = "abort"` 下栈溢出会终止进程）。
        let blocks = parser
            .parse(text)
            .unwrap_or_else(|e| panic!("{name} 应能解析：{e}"));
        println!("[metrics] {name}：{} 块", blocks.len());
    }
}

#[test]
fn boundary_unclosed_code_fence() {
    // 未闭合围栏：按 CommonMark 应「一直吃到文末」，不得报错或吞掉后文。
    let text = "正常段落。\n\n```rust\nfn 未闭合() {\n// 缺三个反引号\n";
    let parser = PulldownMarkdownParser::new();
    let blocks = parser.parse(text).expect("未闭合围栏应能解析");
    assert!(!blocks.is_empty(), "未闭合围栏仍应产出块");
}

#[test]
fn boundary_very_long_url() {
    // 超长 URL（10 万字符）：不得触发正则/链接解析的灾难性回溯。
    let url = format!("https://example.com/{}", "a".repeat(100_000));
    let text = format!("[链接]({url})\n");

    let parser = PulldownMarkdownParser::new();
    let start = Instant::now();
    let blocks = parser.parse(&text).expect("超长 URL 应能解析");
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    println!("[metrics] 超长 URL 解析：{elapsed:.2}ms");
    assert!(!blocks.is_empty(), "超长 URL 链接应产出块");
    assert!(elapsed <= 3000.0, "超长 URL {elapsed:.2}ms 疑似退化");
}

#[test]
fn boundary_invalid_utf8_does_not_crash() {
    // 非法 UTF-8 字节：编码探测会先把"绝大部分合法、只有零星坏字节"的输入
    // 判为损坏的 UTF-8（而非 GBK），再以 lossy 方式解码——
    // 这样坏字节变成 U+FFFD，**而前后原本正确的中文原样保留**。
    //
    // 若这里被误判成 GBK，中文会被二次解码成另一批看似正常的汉字，
    // 断言 `contains("中文")` 就会失败——本条正是那个回归的防线。
    //
    // 注意：`docs/07` §S7-04 原文写的是「非法 UTF-8 返回明确错误」，
    // 与实现不一致。此处断言的是**实际契约**（不崩溃 + 保留其余内容）；
    // 契约是否需变更见 `docs/archive/stage-reports/large-file-report.md` 的待确认项。
    let mut bytes = b"# \xe4\xb8\xad\xe6\x96\x87".to_vec(); // 合法中文
    bytes.extend_from_slice(&[0xFF, 0xFE, 0x80]); // 非法续字节
    bytes.extend_from_slice(b"\n\ntail\n");

    let text = rustmark::infrastructure::persistence::fs_document_repository::FsDocumentRepository::decode_public(
        &bytes,
    );

    assert!(
        text.contains('\u{FFFD}'),
        "非法字节应被替换为 U+FFFD，实际 {text:?}"
    );
    assert!(text.contains("tail"), "非法字节之后的正文不得丢失");
    assert!(text.contains("中文"), "非法字节之前的正文不得丢失");

    let parser = PulldownMarkdownParser::new();
    let blocks = parser
        .parse(&text)
        .expect("含替换字符的文本应能解析，不得报错");
    assert!(!blocks.is_empty(), "应仍能解析出块");
}

#[test]
fn boundary_one_mb_full_pipeline() {
    // 1MB 全量：解析 → 渲染 → 查找 → 导出，任一环节崩溃即失败。
    let markdown = crate::golden_fixture::load_or_generate_large();

    let parser = PulldownMarkdownParser::new();
    let blocks = parser.parse(&markdown).expect("应能解析");
    assert!(!blocks.is_empty(), "1MB 应产出块");

    let finder = Finder::scan(
        &markdown,
        "fn",
        FindOptions {
            case_sensitive: false,
            regex: false,
            whole_word: false,
        },
    )
    .expect("应能查找");
    assert!(finder.count() > 0, "1MB 代码块应含 fn 标识");

    let html = rustmark::infrastructure::export::pulldown_html_exporter::render_fragment(&markdown);
    let html = rustmark::infrastructure::export::html_sanitize::sanitize_html(&html);
    assert!(html.contains("<"), "导出 HTML 应有标签");
}
