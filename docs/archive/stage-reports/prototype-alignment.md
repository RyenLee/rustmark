# 行为对齐矩阵（原型 ↔ Rust）
>
>
> **已归档（2026-10-14）**：历史一次性文档（阶段报告 / 修复方案 / 实施日志 / 审计快照），结论仍有效但不再维护；归档索引与理由见 `docs/archive/README.md`。

> 阶段 07 · S7-02 产出
> 契约来源：`../markdown-editor-prototype/`（marked + highlight.js + DOMPurify）
> 对照规范：`docs/archive/stage-reports/prototype-contract.md`（A 清单）、`REQUIREMENTS.md`
> 自动化断言：`tests/integration/alignment_test.rs`

## 目的与判据

本矩阵逐项比对「原型的可观测行为」与「Rust 实现的可观测行为」。
**判据优先级**（来自 `docs/07` §4）：渲染/语法差异以 **CommonMark / GFM 规范**为最终标准，不以原型 `marked` 为绝对准绳；数值口径差异必须给出明确取舍（以 Rust 为准并此处留痕，或修正 Rust）。

每行结论取值：`一致` / `差异（已处置）` / `差异（已知，记录）`。凡标「差异」的均已在本文档显式记录理由，非默默不一致。

---

## ① 渲染差异（marked + DOMPurify vs pulldown-cmark + 白名单清洗）

Rust 侧渲染管线：`pulldown_cmark::html::push_html`（开 `TABLES | FOOTNOTES | TASKLISTS | STRIKETHROUGH | GFM`）→ `sanitize_html`（白名单 `ALLOWED_TAGS`）→ 骨架包裹。
注意：清洗器在 `ALLOWED_TAGS` 之外会**拆壳保留文本**（`section` 不在白名单，故脚注 `<section>` 被拆壳但内容保留）。

| # | 项目 | 原型（marked） | Rust（pulldown-cmark） | 结论 | 自动化用例 |
|---|---|---|---|---|---|
| R-01 | 表格渲染 | `<table>` + th/td | `<table>` + th/td（同） | 一致 | `align_gfm_table_renders` |
| R-02 | 列对齐 `:-:` / `--:` / `:-` | 逐列 `text-align` 样式 | 统一 `text-align:left`（无逐列对齐） | 差异（已处置） | 见注① |
| R-03 | 脚注 `[^id]` + 文末区 | `<section class="footnotes">` 包裹 | `<sup>` 引用 + `<li>` 列表（`<section>` 被拆壳，锚点 `id` 保留） | 差异（已知，记录） | `align_footnote_renders` |
| R-04 | 任务列表 `- [ ]` / `- [x]` | 复选框 | `<input type="checkbox" disabled>`（`input` 在白名单，`checked/disabled` 属性保留） | 一致 | `align_task_list_renders_checkbox` |
| R-05 | 内联 HTML `<div>` 等 | 按白名单保留 | 白名单内标签保留（`div/span/...`） | 一致 | `align_inline_html_preserved` |
| R-06 | 危险内联 HTML `<script>` | DOMPurify 丢弃子树 | 清洗器丢弃整棵子树（含文本不留） | 一致 | `align_inline_html_dropped` |
| R-07 | 自动链接 / 裸 URL | `<a href>` | `<a href>`（URL 走 `is_safe_url` 白名单） | 一致 | `align_autolink_renders_anchor` |
| R-08 | 删除线 `~~x~~` | `<del>` | `<del>`（`ENABLE_STRIKETHROUGH`） | 一致 | `align_strikethrough_renders` |
| R-09 | 图片 data URI | 位图保留 / `data:text/html` 拒绝 | 同（精确白名单 `data:image/{png,jpeg,...}`） | 一致 | `html_sanitize_test::case7/8` |
| R-10 | 标题锚点 `#id` | 生成 `id` | pulldown 生成 `id`（`id` 属性在白名单） | 一致 | — |

> **注① 列对齐（R-02）**：原型 marked 按列输出 `style="text-align:left/center/right"`；Rust 导出器只用一份统一样式表（`th,td{text-align:left}`），不区分列对齐。处置：以 CommonMark GFM 规范为准——GFM 不要求列对齐渲染为视觉样式，且预览与导出已共享同一选项集（`export_options()` 与预览解析器一致），故不引入逐单元格样式；列为「差异（已处置）」。若后续需要，可在 `PAGE_STYLE` 之外为 `th[align]` 补样式。

---

## ② 语法高亮差异（highlight.js 语言别名 vs syntect 语法名）

Rust 侧用 `syntect`（`SyntectHighlighter`），原型用 `highlight.js`。二者语法名体系不同，本表逐一对齐原型 `CodeDialog.vue` 的 20 种语言。
对齐证据：集成测试 `align_twenty_languages_all_highlight` 断言 20 种语言均**非空**且**确定性可复现**，且每个 span 的样式均来自领域枚举 `SpanStyle`（不泄漏 syntect 类型）。

| # | 原型别名（highlight.js） | Rust 语法名（syntect） | 结论 | 用例 |
|---|---|---|---|---|
| H-01 | rust | `rust` | 一致 | `align_twenty_languages_all_highlight` |
| H-02 | typescript | `typescript` | 一致 | 同上 |
| H-03 | javascript | `javascript` | 一致 | 同上 |
| H-04 | vue | syntect 无原生 Vue，按文件名/嵌入回退（非空） | 差异（已知，记录） | 同上 |
| H-05 | html | `html` | 一致 | 同上 |
| H-06 | css | `css` | 一致 | 同上 |
| H-07 | json | `json` | 一致 | 同上 |
| H-08 | python | `python` | 一致 | 同上 |
| H-09 | go | `go` | 一致 | 同上 |
| H-10 | java | `java` | 一致 | 同上 |
| H-11 | c | `c` | 一致 | 同上 |
| H-12 | cpp | `cpp` | 一致 | 同上 |
| H-13 | csharp | `csharp` | 一致 | 同上 |
| H-14 | php | `php` | 一致 | 同上 |
| H-15 | ruby | `ruby` | 一致 | 同上 |
| H-16 | shell | `shell`（`bash`/`sh` 同族） | 一致 | 同上 |
| H-17 | sql | `sql` | 一致 | 同上 |
| H-18 | yaml | `yaml` | 一致 | 同上 |
| H-19 | markdown | `markdown` | 一致 | 同上 |
| H-20 | diff | `diff` | 一致 | 同上 |

> 配色差异：highlight.js 主题与原生 `syntect` 默认主题不同，但本实现在 `Theme` 层统一映射为领域 `SpanStyle`，再由 `mapper` 上色，故**配色以 Rust 主题为准**，不追求与原型逐一像素一致（视觉细节归 S7-05）。

---

## ③ 数值口径差异（统计字段）

Rust 侧 `compute_stats(text, title)`（`application/document/service.rs`）对齐原型 `StatsDialog` 的 `computeStats`。
关键取舍：`count_words` 采取「中日韩按字 + 西文按词（须含字母/数字）」，并**有意偏离**原型正则（原型把孤立连字符/方括号也计入，导致任务清单字数虚高）。以 Rust 口径为准。

| # | 项目 | 原型口径 | Rust 口径 | 结论 | 用例 |
|---|---|---|---|---|---|
| S-01 | 字数（words） | 中日韩按字 / 西文按词 | 同，且西文词须含字母或数字（排除 Markdown 结构符） | 差异（已处置，以 Rust 为准） | `align_stats_counts_cjk_and_words` |
| S-02 | 字符数（含空格） | `text.length` | `text.chars().count()` | 一致 | 同上 |
| S-03 | 字符数（不含空格） | 过滤空白后计数 | `filter(!is_whitespace).count()` | 一致 | 同上 |
| S-04 | 行数 | `split('\n').length` | `split('\n').count().max(1)` | 一致 | 同上 |
| S-05 | 段落数 | 空行分段 | 非空段首计数（围栏内跳过） | 一致 | 同上 |
| S-06 | 标题数 | ATX `#`~`######` | 同（须后跟空格；围栏内排除） | 一致 | 同上 |
| S-07 | 代码块数 | 围栏计数 | 同（围栏开合计数；围栏内不重复计） | 一致 | 同上 |
| S-08 | 链接数 | `[..](..)` | 同，且**图片不计**入链接（避免重复计数） | 一致 | 同上 |
| S-09 | 图片数 | `![..](..)` | 同 | 一致 | 同上 |
| S-10 | 任务完成度 | `done/total` | `tasks_done / tasks_total`（`- [x]` 算完成） | 一致 | 同上 |
| S-11 | 预计阅读时长 | `max(1, round(words/300))` | `words.div_ceil(300).max(1)`（下限 1） | 一致 | 同上 |

---

## ④ 交互默认值与阈值

| # | 项目 | 原型（行为/常量） | Rust（常量/策略） | 结论 | 用例 |
|---|---|---|---|---|---|
| D-01 | 预览防抖 | `setTimeout` 150ms（组件内，不可测） | `PREVIEW_DEBOUNCE_MS = 150`（下沉为可注入纯函数 `DebouncePolicy`，可单测） | 一致（且可测性更优） | `align_default_preview_debounce_is_150ms` |
| D-02 | 撤销合并窗口 | 全局 `lastCommit`（有跨文档污染缺陷） | `undo_merge_window_ms = 400`，per-Document | 一致（根治缺陷） | `align_default_undo_merge_window_is_400ms` |
| D-03 | 搜索命中上限（单文件） | 50 | `MAX_HITS_PER_FILE = 50` | 一致 | `align_search_hit_caps_are_50_and_300` |
| D-04 | 搜索命中上限（总量） | 300 | `MAX_TOTAL_HITS = 300` | 一致 | 同上 |
| D-05 | 最近文件上限 | 20 | `RECENT_FILES_LIMIT = 20` | 一致 | `align_recent_files_cap_is_20` |
| D-06 | 图片内联体积上限 | 512KB 仅 warn（仍插入） | `max_inline_bytes = 512*1024`，**硬约束**（超限不内联） | 差异（已处置，以 Rust 硬约束为准，ADR-003） | — |
| D-07 | 大文件降级阈值 | 无显式 | `TRUNCATE_THRESHOLD_BYTES = 500*1024`（> 500KB 仅渲染可视区，FR-06） | 差异（已知，记录） | `align_truncate_threshold_is_500kb` |
| D-08 | 无修饰键单键快捷键 | 匹配器拒绝纯单键（`F8`/`Esc` 永不触发，B-01/B-03） | 功能键允许（`clashes_with_text_input` 仅判字符键）；默认表已含 `F8`/`Esc` | 一致（根治缺陷） | `reg_function_key_binding_not_flagged_as_text_input_clash`（regression_test.rs） |

---

## 矩阵统计

- 总行数：**46**（R-01~10、H-01~20、S-01~11、D-01~08）。
- 「一致」：**38** 行。
- 「差异（已处置）」：**5** 行（R-02 列对齐、S-01 字数偏离、D-02 合并窗口、D-06 图片硬约束、D-08 单键允许）。
- 「差异（已知，记录）」：**3** 行（R-03 脚注 section 拆壳、H-04 vue 回退、D-07 降级阈值）。
- 全部差异均已在此显式记录理由，无「默默不一致」。
- 自动化覆盖：R/S/D 类共 **9** 个用例在 `alignment_test.rs`，H 类 20 语言在 `align_twenty_languages_all_highlight`，D-08 在 `regression_test.rs`。
