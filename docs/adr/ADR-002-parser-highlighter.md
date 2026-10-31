# ADR-002 · Markdown 解析与语法高亮

- **状态**：已接受（2026-10-06）
- **关联需求**：§5.1、§5.2、§8.1、§8.2（`FUNC-P0-05`~`P0-10`、`NFR-PERF-05/06`）
- **关联任务**：S1-03、S5-02、S5-03、S8-02

## 背景

需求文档 §5.1 已选定 pulldown-cmark 0.13（440 KiB，10KB 文档约 2ms）与 syntect 5.3.0，§5.2 否决 comrak（2,767 KiB，为 pulldown 的 6 倍）。本 ADR 补充**实施层面的具体参数**：feature 裁剪、语言集范围、语法集加载方式、降级预案。

## 决策

### 1. 解析器

```toml
pulldown-cmark = { version = "0.13", default-features = false, features = ["html", "simd"] }
```

**已核实（crates.io API，2026-10-06）**：0.13 线最新为 `0.13.4`；feature 全集仅 `default = ["getopts","html"]`、`html`、`simd`、`gen-tests`。

- **GFM 特性（表格 / 任务列表 / 删除线 / 脚注）不是 feature，而是运行期 `Options` 开关**（`ENABLE_TABLES` 等）。故关闭默认 feature 不会丢失 GFM 能力，只需在 S5-02 显式启用所需 `Options` 并逐项加 golden 用例。
- `default-features = false` 的实际收益是**去掉 `getopts`**（pulldown 自带 `cmark` 命令行二进制的依赖，本项目不需要）；`html` 因 HTML 导出（S5-09）必须保留；`simd` 可选，需在 CI 三平台验证后再决定是否启用。
- 必须使用 `OffsetIter::into_offset_iter()` 产出带源码区间的事件流——这是 `FR-02`（双向滚动同步）与 `FR-03`（增量高亮）的实现前提；若某版本不支持则需重新评估。
- 数据模型：事件流 → `Vec<RenderedBlock>`，每块携带 `source_range: TextRange`（S3-09）。

### 2. 语法高亮

```toml
syntect = { version = "5.3.0", default-features = false, features = ["default-fancy"] }
```

- `default-fancy` 为**纯 Rust** 实现（不带 onig 的 C 依赖），满足需求 §10「依赖维护风险」的应对项。
- **语言集裁剪**：首版保留 20 种，与原型 `CodeDialog.vue` 的语言清单对齐。

| # | 原型标识 | syntect 语法名 | # | 原型标识 | syntect 语法名 |
|---|---|---|---|---|---|
| 1 | rust | Rust | 11 | go | Go |
| 2 | python | Python | 12 | java | Java |
| 3 | javascript / js | JavaScript | 13 | c | C |
| 4 | typescript / ts | TypeScript | 14 | cpp / c++ | C++ |
| 5 | json | JSON | 15 | sql | SQL |
| 6 | html | HTML | 16 | xml | XML |
| 7 | css | CSS | 17 | dockerfile | Dockerfile |
| 8 | markdown / md | Markdown | 18 | ini / toml | TOML |
| 9 | bash / sh | Bash | 19 | yaml | YAML |
| 10 | toml | TOML | 20 | diff | Diff |

> 上表为**待验证映射**：syntect 的实际语法名需在 S5-03 用 `SyntaxSet::find_syntax_by_token()` 逐项核对，核对结果回写本表。

- **加载方式**：构建期 dump 为二进制（`.bin`），运行期 `from_dump` 加载（预期约 23ms，§8.2）。dump 生成失败时回退到源码语法集加载（慢但可用）并在启动日志提示。

### 2.1 语法集来源与体积（已核实，关键决策点）

**已核实（crates.io API，2026-10-06）**：syntect 5.3.0 的 feature 关系中，`default-fancy` = `parsing + default-syntaxes + default-themes + html + plist-load + yaml-load + dump-load + dump-create + regex-fancy`，且 `default-syntaxes` 依赖 `dump-load`。

由此得出一个必须正视的结论：**`default-syntaxes` 会把 syntect 内置的全量语法集（数百种语言）编译进二进制**。因此"裁剪语言集"若只是运行时过滤，对体积毫无收益。三选一：

| 方案 | 做法 | 体积 | 工作量 | 适用 |
|---|---|---|---|---|
| A · 全量内置 | `default-fancy` 原样使用 | 最大，需实测是否 ≤1.5 MB | 最小 | 若实测达标则采用 |
| B · 自建 curated 语法源 | 关闭 `default-syntaxes` / `default-themes`，自备 20 种 `.sublime-syntax` + 1 套主题，`build.rs` 生成 dump | 最小 | 中（需引入语法源数据与版权核查） | 实测超预算时采用 |
| C · 运行时过滤 | 从全量语法集过滤后 dump | 无收益（全量数据仍在二进制内），仅启动收益 | 小 | **不作为体积手段** |

**执行顺序**：S5-03 先按方案 A 实测 `cargo bloat`；≤1.5 MB 则保持 A；超出则转方案 B，并把语法源的许可证核查结果补入 `deny.toml` 与 `NOTICE`。

### 3. 体积红线与降级预案

| 组件 | 预算 | 触发动作 |
|---|---|---|
| pulldown-cmark | ≤ 0.5 MB | 超出则核查是否有未关闭的 feature |
| syntect | ≤ 1.5 MB | 超出则语言集裁剪 20 → 10 → 5 分级执行 |
| syntect（最终手段） | — | 仍超预算则改用自研 regex 高亮，仅覆盖 Markdown 代码围栏的高频 token（字符串、注释、关键字、数字） |

降级一旦触发，须追加 ADR-002 修订版并说明覆盖范围损失。

### 4. P1 特性的延后（2026-10-08 修订）

**原决策**：数学公式（LaTeX）与 Mermaid 图表（均为 P1）在 v1.0 **不实施**。

- 二者均需引入额外渲染依赖（如 `typst` / `katex` 替代方案或 JS 运行时），与 10 MB 预算冲突；
- 领域层 `BlockKind` 预留 `Raw` 扩展位（S3-09），确保后续可增量加入而不改架构；
- 渲染时对公式/图表语法降级为「等宽原文块」并给出提示，不报错。

**修订结论**：**依赖层面的延后决定维持不变**——不引入 wasm / node / JS 运行时，体积预算不被击穿。但**功能层面已交付零依赖版本**：

- 解析器开启 `ENABLE_MATH`（零成本，本就在 pulldown 内），行内 `$...$` 补回分隔符并入段落文本，整段 `$$...$$` 升级为独立 `BlockKind::Math { display }`；
- ` ```mermaid ` 围栏（大小写不敏感）落到 `BlockKind::Diagram`，不再当普通代码块；
- 预览区给出结构化卡片：行内公式随正文基线、块级公式居中成卡 + 复制、图表卡标注引擎名；像素级排版与出图交由导出 HTML 后的浏览器运行时；
- 导出的 `Options` 同步开启 `ENABLE_MATH`，公式不再被当普通文本静默丢失（已加回归测试）。

**未覆盖**：导出 HTML 尚未注入 KaTeX / Mermaid 资源，浏览器打开时公式仍是源码形态。若后续要做，需另立ADR 讨论「资源注入策略（CDN 外链 vs 可选离线包）」——这会引入外链与体积决策，不在本次零依赖范围内。

## 影响范围

- S5-02 / S5-03：按本 ADR 配置依赖与语言集；
- S3-09 / S3-10：领域值对象与 trait 须承载 `source_range` 与增量 `highlight_range`；
- S8-02：体积实测与分级裁剪；
- S7-02：与原型 highlight.js 的 token/配色对齐表；
- P1 延后项（公式 / Mermaid）：零依赖结构化方案已交付，见上文修订。

## 复盘触发条件

`cargo bloat` 实测 syntect > 1.5 MB 且裁剪到 5 种语言仍超预算。
