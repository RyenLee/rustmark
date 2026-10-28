# rustmark 架构审查报告（2026-10-23）

- **审查基线**：工作区当前状态（v0.2.0，未提交改动前）
- **审查方法**：静态扫描（grep/统计复算）+ `check-arch` 实际运行 + 文档与实现交叉核对 + 三个维度的独立子调查（耦合/非功能/选型与规范）
- **前次评估**：[architecture-review.md](./architecture-review.md)（2026-10-20，基线 `e09ad2b`）——本报告核对了其问题清单的修复状态，并按用户要求的 8 个维度重新全面审查
- **规模实测**：`src/` 324 个 `.rs` / 63 301 行（含 432 个内联 `#[test]`）；`tests/` 1 686 个 `#[test]`（`docs/testing.md` 记 2026-10-22 实测 **2101 passed / 0 failed / 4 ignored**）

---

## 零、结论先行

**架构整体健康，属于同类项目中少见的「规则被机械执行」状态。** 三层判定：

| 判据 | 实测 | 判定 |
|---|---|:---:|
| `check-arch` 分层依赖校验（2026-10-23 实跑） | `OK（未发现分层依赖违规）` | ✓ |
| 生产代码 `TODO/FIXME/HACK/XXX` | **0** | ✓ |
| `unwrap/expect/panic` 守卫 | `lib.rs` warn 级 lint + CI `-D warnings` | ✓ |
| CI 门禁 | fmt / clippy / check / check-arch / 三平台测试 / 体积红线 / release 性能冒烟 / 覆盖率（整体≥70%、领域≥90%）/ cargo-deny / 发布演练，10 项 | ✓ |
| 前次评估 P0 问题（4 项） | **全部已修复**（§一） | ✓ |
| 表现层占比 | **49%**（30 942 / 63 301 行）；~~`preview_view.rs` 1 339 行未拆~~ **已拆（D-4 闭合）**：入口 458 行 + `image_cache.rs` 319 + `tests.rs` 664（TD-02，2026-10-25），`toolbar_view.rs` 695 → 211 + `tab_strip.rs` 498（TD-09）；两文件现均低于 600 行阈值 | ✓ |
| 预览区 span→`LayoutJob` 构造器 | **已由 R-10 收敛**：唯一构造器 `span_job::build_job` + 唯一语义派发 `StyleSemantics::of`；双样式表为有意设计并有快照测试守卫（勘误见 §六 D-1） | ✓ |
| 可观测性 | ~~仅 `log::warn` + 启动埋点；`debug-guide.md` §2.4 与实现自相矛盾~~ 已纠错（2026-10-24，D-2）；~~帧级常态指标仍缺~~ **已补（D-5，2026-10-26）**：超 16ms 的帧自带 `warn`（分阶段归因 + 本帧到期任务 + 限流），`RUST_LOG=…=debug` 每 600 帧吐平均/最大/超预算占比；排查手册见 `debug-guide.md` §5.6 | ✓ |

**问题分布**：无 P0（阻断级）问题；原 3 项中等（D-4/D-5/D-6）与 7 项低（D-1/D-2/D-3/D-7/D-8/D-10/D-12 中的
可执行项）**已全部闭合或明确降级为记录**（逐项结论见 §六「2026-10-26 收口」列）。
最值得投入的三件事全部落地：① ~~`debug-guide` 日志章节纠错~~ **已完成（2026-10-24）**；
② 表现层「状态写入」机械守卫 **已随 D-3 全阶段闭合**；
③ ~~`preview_view.rs` 拆分与帧级耗时可观测性~~ **已完成（D-4 = TD-02/TD-09，D-5 = 2026-10-26）**。

---

## 一、架构设计合理性（分层 / 模块 / 边界）

### 1.1 分层结构

依赖方向 `presentation → application → domain ← infrastructure`，`shared` 为零业务依赖内核，`app` 为唯一组合根。规则真源 [architecture-constraints.md](../../architecture-constraints.md)（依赖矩阵 §3、各层禁 crate §4、判定口径 §4.1），执行者为词法/token 级的 [arch_check.rs](../../../tools/arch_check.rs)（`build.rs` 编译期 panic + `check-arch` bin 双入口）+ CI。

各层规模（实测）：

| 层 | 文件 | 行数 | 占比 | 职责边界 |
|---|---:|---:|---:|---|
| `presentation` | 125 | 30 942 | **49%** | views / state / rich / theme / shortcuts / widgets / icons，内部单向（R-13，ADR-010） |
| `infrastructure` | 52 | 10 105 | 16% | 持久化 / 解析 / 高亮 / 导出 / 字体 / 拼写 / 对话框适配 |
| `domain` | 47 | 6 603 | 10% | 5 个限界上下文，零 IO 零 UI |
| `app` | 29 | 6 636 | 10% | 组合根 + 帧循环 + 动作路由 |
| `application` | 49 | 6 149 | 10% | 用例编排 + DTO |
| `shared` | 19 | 2 767 | 4% | 错误 / 事件 / ID / 几何 / 跨层共享类型 |

**评估**：
- ✓ 依赖矩阵清晰且**有机械执行**（词法级 token 扫描、豁免登记表、`tests/` 纳入、`mod.rs` 禁令、帧预约唯一出口均不设豁免）。这是本项目架构治理最大的亮点：规则「写进文档 → 编译期可拦 → CI 强制」三段闭环。
- ✓ 服务边界与需求对齐：单机本地编辑器、无后端进程，ADR-003 显式废弃了原型的 SQLite/IPC 假设——边界定义没有照搬过时方案。
- ⚠ **`presentation` 占比接近一半**，是典型的「UI 富客户端」形态但同时是维护成本最高的一层；`views` 内部拆分标准不统一（`preview_view.rs` 1 339 行、`toolbar_view.rs` 683 行未拆，而 `sidebar_view/`、`settings_view/` 已按子目录拆干净）。
- ⚠ **`shared` 存在职责漂移风险**：从约束文档设想的 5 个文件（error/result/event/ids/geometry）长到 19 个文件，新增了 `export_format.rs`、`theme.rs`、`pattern.rs`（474 行）、`fonts.rs`、`encoding.rs`、`image_asset.rs` 等。每一步都有文档论证（如 R-07 对 `ExportFormat` 的归属裁决），但**缺少一条「什么可以进 shared」的准入标准**，长期会退化成杂物间。

### 1.2 限界上下文

5 个上下文（Document / Markdown / Workspace / Preferences / Export）+ 聚合根 + 上下文映射在约束文档 §5 显式登记，聚合拆分与目录结构一致。**合理，无越界**。

---

## 二、技术选型适配性

场景锚点：单人本地 Markdown 编辑器、三平台分发、**exe ≤ 10 MB 红线**、无后端、无 async 运行时。

> 历史口径注记（2026-10-26 补）：本文写作时（2026-10-23）体积红线确为 10 MiB；该红线已于
> 2026-10-24 上调至 **11 MiB（预警 10.5 MiB）**，当前口径见 `docs/metrics.md` §2.8 与
> `scripts/size_gate.sh`。本段作为历史快照保留，勿按旧值判当前门禁。

| 依赖 | 用途 | 适配判定 |
|---|---|---|
| `egui/eframe 0.37`（wgpu 单后端按平台开启） | 即时模式 UI | **适配**。即时模式契合「每帧重绘 + 状态在外」的编辑器；体积通过关 default/字体外置压进红线。代价是 CPU 布局压力，项目用缓存 + 增量 + 帧调度系统性缓解（见 §四-3） |
| `pulldown-cmark 0.13` | GFM 解析 | **适配**。Options 运行期配置（`pulldown_parser.rs`），与依赖 feature 解耦 |
| `syntect 5.3`（regex-fancy、无主题） | 语法高亮 | **适配**。纯 Rust 正则避开 onig 的 C 依赖；关主题省数百 KB。代价是语法集首次加载，已有测量门禁 |
| `ropey` | 文本缓冲 | **适配**。大文件（1 MB 级）有专项集成测试 |
| `ureq 3.5`（rustls + `rustls-native-certs`） | 远程图片下载 | **适配**。同步 API 放后台线程（`preview_view.rs:267` `thread::spawn`），平台信任库替代内置根证书省 ~400 KB——「不用 async 运行时」是被文档论证的取舍，不是遗漏 |
| `rfd` | 原生对话框 | **适配**。同步阻塞 API 由 `app/dialog_runner.rs:147` 后台线程承载，不卡 UI 线程 |
| `thiserror` + `anyhow` | 错误 | **适配**。领域错误枚举用 thiserror，应用编排边界用 anyhow，分工常规 |
| `toml + serde` / `ahash(no-rng)` / `log + env_logger` | 配置 / 缓存键 / 日志 | 前两者适配；**日志是选型链的最弱一环**（见 §四-5） |
| **无数据库、无 web 框架、无 tokio** | — | **正确**。本地编辑器引入这些只会增加体积与复杂度；ADR-001/003 已显式否决 Tauri/SQLite 路线 |

**依赖治理**：`docs/dependencies.md` 登记制度 + `deny.toml`（CI 单独跑 licenses/bans/sources/advisories，且 cargo-deny **锁版本**避免口径漂移）+ 体积预算表（约束文档 §8）。`Cargo.toml` 每个依赖都有「为什么开/关这个 feature」的注释。**这是超出项目体量的规范化程度，与体积红线匹配。**

**版本策略**：`edition 2024 / rust-version 1.99 / rust-toolchain.toml 唯一真源`；egui 0.33→0.37 升级有专门复盘（`archive/stage-reports/egui-0.37-upgrade.md`）。风险可控。

---

## 三、耦合度与依赖关系

### 3.1 层间：健康

- `check-arch` 今日实跑零违规；前次评估的 4 类绕过形态（`pub use` / 组花括号 / 全路径 / impl-for）已由 token 级判定收口，`tests/` 已纳入扫描，豁免表当前**零登记**（期望状态）。
- `domain → infrastructure`、`application → infrastructure`、`presentation → domain` 等反向引用为 0；trait 实现全部落在 infrastructure 侧反向注入。

### 3.2 层内与跨层接口：三个残余耦合点

| # | 耦合点 | 现状实测 | 等级 |
|---|---|---|:---:|
| C-1 | ~~预览区 span→`LayoutJob` 三份实现~~ **已由 R-10 收敛（勘误）** | 切段唯一构造器 `span_job::build_job`（三处均为薄封装 + 显式 `SpanPolicy`）；语义派发唯一 `StyleSemantics::of`；两张样式表**有意保留**（编辑区 26 样式/编辑字号 vs 预览 14 样式/预览字号，共同 7 色同源 `theme.syn_*`，论证见 `span_job.rs` 模块头），`style_tables_keep_their_look` 快照钉住 | **低**（残余：跨表颜色一致性缺「直接对拍」断言，见 D-1） |
| C-2 | **`presentation → app` 的窄豁免通道** | 32 个视图文件 `use crate::app::frame`（获认可的下行通道）；视图经 `&mut <X>UiState` 句柄改状态 **118 处/12 文件**。直接写 `app.<field> =` 的裸字段赋值实测 **0 处**（前次为 59 处——已有实质收敛），但**「赋值/穿透」形态尚无机械守卫** | **中** |
| C-3 | **`app/frame` 组合根胶合** | 29 文件 / 6 636 行；`frame.rs` 737 行。`workspace.rs`↔`files.rs`↔`outline.rs` 间仍有纯函数互相导入（`group_search_hits` 被 `outline.rs` 反向导入） | **低** |

前次评估发现的循环依赖已核实修复：`shared/dialog` ↔ `shared/text_format` 环已断（现仅 `dialog → text_format` 单向）；`shared/system.rs` 的 `std::process` 已按 R-03 移到 `infrastructure/system.rs`（端口留在 shared）。**全仓无层间环、无层内环。**

### 3.3 双枚举类反模式：大部分已根治

- `ExportFormat`：已收敛为 `shared/export_format.rs` **唯一真源**（R-07），领域层 re-export 保路径，穷举 `match` + `export_format_guard_test` 守卫 → 新增格式从「改 6 处含 1 处运行期错误」降为「改 2 处全编译期」。✓
- `persisted_key()`：已改为**逐变体穷举**（`impls.rs:108-150`，无 `_ =>` 兜底），配合两条跨层契约测试。✓
- `ThemeKind/BuiltinTheme`：真源已入 `shared/theme.rs`。✓
- **残余**：C-1 的 span 构造器/样式表是同类问题中**唯一未根治**的一项。

---

## 四、非功能性指标支撑

### 4.1 可扩展性
- 新增导出格式/快捷键动作/主题均已收敛到「穷举 match = 漏加编译失败」的模式；`ActionRoute` 无 `_` 兜底（ADR-011），`Unbound` 清单由断言锁定（`actions.rs:439`），R-04 后当前**零未接线动作**。
- 大文档：`tests/integration/large_file_test.rs` 覆盖 1 MB 解析/首屏/滚动映射/全文查找 + 降级路径；10 MB 体积红线同样是**扩展性约束**（新依赖需登记 + ADR 追加评审）。
- 判定：**扩展点有编译期护栏，主要扩展压力在表现层**（新增一个侧边栏页签/浮层仍要碰 views + state + 路由三处，但均在层内）。

### 4.2 可维护性
- 测试金字塔显式分层（9 个 `[[test]]` 目标 + 目录即分层），2 100+ 测试，含 golden 语料、变异注入 fixture（`arch_violation_fixture`）、性能回归门禁。
- 文档体系完整：架构约束（唯一真源）+ 12 篇 ADR + 依赖登记 + 测试/调试/发布文档 + 度量基线；`docs/README.md` 索引与 `testing.md` 实测数已更新到位。
- 判定：**强**。唯二短板是 §六 D-2（~~`debug-guide` 日志章节过期~~ 已于 2026-10-24 纠错）与大文件拆分不一致。

### 4.3 性能
- 帧调度：ADR-009 `FrameScheduler` 单一出口（`request_repaint_after` 全仓只许出现在 `scheduler.rs`，无豁免、构建期拦截）——把「定时功能静默失效」这类历史 bug 类别机械消灭。
- 缓存：应用层真 LRU（`highlight_cache/render_cache/spell_cache`，键含 revision）+ 表现层 Galley 缓存，设计文档 [cache-design.md](../../cache-design.md)。
- 门禁：CI release 性能冒烟（1 MB 全文查找/解析/首屏阈值断言，仅 Linux 必过）+ 体积门禁 10 MB + 近期搜索优化有独立报告（切搜索标签首帧 250ms → **0.69ms**）。
- 判定：**强，且有回归防线**。风险点：性能阈值仅 Linux 单平台（跨平台数字不可比，已显式裁定）；帧预算依赖 `begin/flush` 成对契约（ADR-009 自认的软肋，~~现仅 `repaint_delay` 测试兜底~~ **D-7 已补 debug 断言（2026-10-24）**：漏调 `begin` 在 debug 构建即 panic，含「下一帧漏调」的消费语义用例）。

### 4.4 容错
- 写入原子性：同目录临时文件 → `sync` → `rename` 覆盖（`fs_document_repository.rs`）；草稿旁车 `<id>.draft.path` 不覆盖原文件。
- 崩溃链路：crash log + 恢复提示 + 专项集成测试；`panic = "abort"` 下靠旁车与 crash log 兜底（与「恢复而非 unwind」策略一致）。
- 生产代码禁 `unwrap/expect/panic`（lint + CI），错误有统一目录（`shared/error.rs` + 领域 errors + `error_catalog` 测试）。
- 外部资源：图片下载失败有降级策略（`asset_loader/policy.rs`），https 失败路径是缺陷 025 修复过的历史热点。
- 判定：**良好**。桌面单进程场景下没有「服务可用性」问题，主要风险（数据丢失）已被原子写 + 草稿覆盖。

### 4.5 可观测性 —— 曾是最弱维度（**2026-10-26 D-5 已补帧级常态指标**）
- 现状：`env_logger`（默认 warn，`main.rs:23`）+ 零散 `log::warn!` + `RUSTMARK_STARTUP_TIMING` 启动埋点 + panic backtrace；无结构化日志、无运行时指标、无帧耗时的用户可见归因（曾靠临时归因测试排查 TextEdit 开销）。
- **且文档与实现矛盾**（~~已纠错~~ **2026-10-24 D-2 闭合**）：[debug-guide.md](../../debug-guide.md) §2.4/§400 旧版声称「本项目没有 logger 后端，`RUST_LOG` 无效」——F-2 已注册 env_logger，`frame.rs:717` 注释也已同步，**当时只有 debug-guide 没改**；现已按 `main.rs` 实况改写（默认 warn、`RUST_LOG=rustmark::app=debug` 模块过滤、无正则 feature 的边界注明）。
- 判定：对当前规模「够用」，但每次性能/行为排障都依赖临时插桩（近期两次排障均为事后复盘），**缺一层常态化的帧级/搜索级耗时指标**。
- **补记（2026-10-26，D-5 闭合）**：帧级缺口已补——`FrameScheduler` 在 `flush` 结账本帧耗时，
  超 16ms（`NFR-PERF-04`）即 `log::warn!`（状态更新 / 绘制 / 未归因三段归因 + 本帧到期任务，
  120 帧限流），每 600 帧再给一条 `debug` 常态指标（平均 / 最大 / 超预算占比）。仍是「粗粒度、
  无结构化日志」的水准：**不做**指标上报、不做 p95 分位数、不落文件——这三项都要引入依赖或常驻 IO，
  与「单人本地工具 + 体积红线」不匹配。判据见 `scheduler.rs` 的 8 条单测，使用手册见 `debug-guide.md` §5.6。

---

## 五、规范与最佳实践落地

| 规范 | 落地情况 | 判定 |
|---|---|:---:|
| 禁 `mod.rs`（CONSTR-03） | 校验器逐文件判名，无豁免 | ✓ |
| 依赖矩阵 + 各层禁 crate | token 级扫描 + build.rs + CI，实跑零违规 | ✓ |
| 表现层内部单向（R-13） | ADR-010 + 子层禁令已入校验器 | ✓ |
| 帧预约唯一出口 | 文本扫描、不设豁免 | ✓ |
| 豁免可见（arch-allow 登记表） | 当前零豁免 | ✓ |
| 生产禁 unwrap / `missing_docs` | lint warn + CI `-D warnings` | ✓ |
| 验证链（fmt/clippy/check/test/check-arch） | 本地 5 步 + CI 三平台，job 职责单一、step 可复制 | ✓ |
| ADR 制度 | 12 篇，结构性决定（含 R-03/R-04/R-07）均有编号引用 | ✓ |
| 测试分层规范 | `tests/` 禁库内部视角（R-02），显式 `[[test]]` 声明 | ✓ |
| 数据流转（视图 → DTO → 应用 → 领域） | `mapper.rs` 用 `SpanView` 而非领域类型；DTO 隔离有效 | ✓ |
| **文档与实现同步** | ~~`debug-guide.md` §2.4 与 logger 现状矛盾（见 D-2）~~ 已纠错（2026-10-24） | ✓ |
| **ADR-009 `begin/flush` 成对契约** | ~~仅测试兜底，无 debug 断言~~ `flush` debug 断言 + `begin` 消费语义（D-7，2026-10-24），2 条 `should_panic` 用例 | ✓ |

**判定：规范落地水平显著高于项目规模的通常水准——「规则 → 机械检查 → CI」的闭环是全项目最值钱的资产。** 残余缺口都是执行尾部（过期文档 D-2 已于 2026-10-24 纠错；软契约 D-7 已于 2026-10-24 补 debug 断言）。

---

## 六、问题清单（按等级）

> 分级口径：**高** = 会在正常演进中造成用户可见缺陷或显著维护成本；**中** = 明确债务、应在近期处理；**低** = 记录在案、择机处理。全部结论结合「单人维护、本地桌面、体积红线」的实际场景。
>
> **2026-10-26 收口**：12 项全部处置——**9 项闭合**（D-1/D-2/D-3/D-4/D-5/D-6/D-7/D-8/D-11）、
> **2 项明确记录为不修**（D-9 单平台门禁、D-10 rich 不额外拆）、**1 项是持续纪律**（D-12 数值真源）。
> 逐项结论与提交号见各行；本轮新落地的两项是 D-4（拆分，TD-02 + TD-09）与 D-5（帧耗时观测）。

| ID | 等级 | 问题 | 影响范围 | 可落地改进 |
|---|:---:|---|---|---|
| D-1 | ~~低~~ **已闭合（2026-10-23）** | **原描述「3 份并行实现 + 2 张样式表」不成立**——已由 R-10 收敛：切段唯一构造器 `span_job::build_job`、语义派发唯一 `StyleSemantics::of`，三处均为薄封装；双样式表为**有意保留**（字号体系与覆盖面不同，论证见 `span_job.rs` 模块头）。二次核查发现对拍用例也已存在（`shared_styles_have_identical_colors_in_both_tables`），但仅测 dark 主题、且注释承诺的反方向断言缺失 | 表现层（已闭合） | **已完成**：① 对拍用例参数化双主题（dark+light）；② 新增反方向用例 `exclusive_styles_stay_distinct_between_tables`（3 对特有样式不同色，防「无脑转发」退化）。`cargo test --lib mapper::tests` 17/17、fmt/clippy 全绿 |
| D-2 | ~~中~~ **已闭合（2026-10-24）** | ~~`docs/debug-guide.md` §2.4/§400 声称「无 logger 后端、`RUST_LOG` 无效」，与 `main.rs:23` 已注册 env_logger 矛盾~~ 已按实况改写 | 全体排障者 | **已完成**：§2.4 改为「env_logger 已注册、默认 warn、`RUST_LOG` 调级别」并注明无 `regex` feature 的模块过滤边界、启动埋点独立通路；§400 脚注同步；全库排查无其他过期说法 |
| D-3 | ~~中~~ **已闭合（2026-10-23，阶段 1/2/3/4a/4b 全部完成）** | `presentation` 状态写入守卫：裸字段赋值已收敛至 0；egui 控件借用 `&mut app.<state>.<field>` 穿透字段（原 7 处 / 5 文件，全在 views/）。~~单 crate 下字段私有化对 views 无编译期约束力~~ workspace 拆分 + 4b 可见性收紧后，core 内部面已对 views 编译不可见 | 表现层 125 文件；新增 UI 状态字段时的心智负担 | **阶段 1**（核实已存在）：R-12 赋值规则 + fixture 四条注入用例。**阶段 2 已完成**：`tests/presentation/state_borrow_guard_test.rs`——词法级（`lex_dump`）扫描全表现层，字段借用白名单双向断言 + 2 条匹配注入用例；词法补 `TokKind::Amp`（零行为变化，fixture 87/87 回归）。**阶段 3 已完成（2026-10-23）**：7 处穿透全部方法化——6 处 TextEdit/checkbox 走「clone（或读出）→ 就地编辑 → 变化才值写回」+ 7 个 state setter（`set_only_changes`/`set_output_dir`/`set_file_stem`/`set_find_text`/`set_replace_text`/`set_writing_goal_text` + `take_blocks`），**清单归零**（`APPROVED_BORROWS = &[]`，集合相等断言双向钉死，新增穿透必红）。**阶段 4a 已完成（2026-10-23）**：拆 workspace——新建 `crates/rustmark-core`（domain/application/infrastructure/shared 四层 + presentation 的 icons/rich/shortcuts/state/theme/widgets 六子模块），shell（root）只留组合根 `app` 与 `presentation::views`；root 经 `pub use rustmark_core::{…}` re-export 拼接，存量 `crate::` 路径**零改动**；依赖方向被 cargo 锁死（`cargo tree -p rustmark-core` 无反向依赖）；校验器改 `scan_project` 三根扫描（shell src + core src + tests，core 违规带 `crates/rustmark-core/src/` 前缀）并把 `rustmark_core::X` 归一为 `crate::X` 匹配同一禁令表（拼接点天然豁免）；命令面（ci clippy/check/test/llvm-cov）加 `--workspace`；全链验证绿（2125 测试 0 失败、注入 core `crate::app` 必红、release 10 485 248 B 启动冒烟通过）。**阶段 4b（原后置待办）**：可见性收紧——~~core 内字段/方法从 `pub` 收窄（`pub(crate)` / 私有 + 受控接口），让 D-11 的编译期强制对 views 真正生效；**后置条件**：4a 合入稳定后单独批次执行，逐字段核对存量读点（约 340 处读点当期零改动），不得与 4a 混批~~ **阶段 4b 已完成（2026-10-23，五批 2995d68 / 15e9020 / a24b5ca / e21ad8c / 2952872）**：模块级 pub 项 1 664 → 1 383、字段 pub 606 → 547（58 收紧为 pub(crate) + 1 死字段删除），E0616/E0451 实测 547 对读点逐条核对；新增 `tests/field_visibility_guard_test.rs` 白名单双向断言（548 条 + 3 注入用例）钉死放宽方向。全链验证 2 125 通过 0 失败。**D-3 全阶段（1/2/3/4a/4b）闭合** |
| D-4 | ~~中~~ **已闭合（2026-10-25，TD-02 + TD-09）** | `presentation/views` 拆分标准不一：`preview_view.rs` 1 339 行（全仓最大）+ `toolbar_view.rs` 683 行未拆，同目录 `sidebar_view/`、`settings_view/` 已拆干净 | 表现层可维护性；大文件的评审与定位成本 | 按已有先例拆：`preview_view.rs` → 保留 `preview_view.rs` 入口 + `preview_view/{media,overlay,…}.rs`（其子目录已 3 000+ 行，壳比内容大即信号）；~~在约束文档 §7 记录阈值~~ **已完成（2026-10-24）**。**拆分已落地（2026-10-25）**：`preview_view.rs` 1,421 → 入口 **458** + `image_cache.rs` 319 + `tests.rs` 664（TD-02，`999e63c`）；`toolbar_view.rs` 695 → 入口 **211** + `tab_strip.rs` 498（TD-09，`22d4550`）。两者均无 `pub` 项外泄、公开路径零变化；D-4 的行数阈值已由 `tools/check_thresholds.py` 机械校验（E-24 起），不再是「靠自觉」 |
| D-5 | ~~中~~ **已闭合（2026-10-26，`1c58052`）** | 可观测性薄弱（§四-5）：无帧级耗时常态指标，排障靠临时插桩 + 事后复盘 | 性能回归的发现时机（近期两次卡顿均为用户反馈后排查） | **已完成（2026-10-26）**：`FrameScheduler` 兼作帧耗时观测点——它是唯一每帧首尾都会被调到的地方，挂在这里不需要新增调用点、也不会漏帧。`begin` 记墙钟，`loop.rs` 在两个阶段边界各 `lap` 一次（状态更新 / 绘制），`flush` 结账：超 `FRAME_BUDGET`（**16ms**，`NFR-PERF-04` 的一帧口径）时 `log::warn!` 一条（**仅 release 构建输出**——debug 的帧耗时不代表真实性能，全超门槛的日志
等于没有告警），带**分阶段归因**、**未归因余量**、**本帧到期的定时任务**与待命数，并按 120 帧限流（压掉的帧数累计到下一条，持续慢不刷屏）；另每 600 帧 `log::debug!` 一条常态指标（平均 / 最大 / 超预算占比），`RUST_LOG=rustmark::app::frame::scheduler=debug` 可取。**零新增依赖**（`log` 本就在 root）。8 条单测钉住阈值边界、归因算术、清表顺序、限流会计、输出开关与接线；两处变异验证：删掉 `flush` 的结账 → 2 例红；把结账挪到清表之后 → 归因恒空的用例红。排查手册见 `debug-guide.md` §5.6 |
| D-6 | ~~中~~ **已闭合（2026-10-24）** | ~~`shared` 无准入标准（§一-1）：文件数持续增长，`pattern.rs` 474 行、`fonts.rs`/`encoding.rs` 等逐次加入~~ 已改为登记制 | 一旦松口，`shared` 会成为绕过依赖矩阵的通道（历史上 `std::process` 就是这么进来的） | **已完成（2026-10-24）**：① 约束 §4.5 新增「shared 准入三问」（≥2 层需要 / 零 IO / 无可归属的层）+ `<!-- shared:registry -->` 登记表（存量 18 文件全部带理由入表）；② `check-arch` 机械核对——`shared/` 下未登记文件在 `build.rs` 构建期与 CI 报文件级违规；③ fixture 三用例钉住（未登记必报 / 已登记放行且键写法归一 / 合成扫描 `scan` 不套用登记制） |
| D-7 | ~~低~~ **已闭合（2026-10-24）** | ~~ADR-009 `begin/flush` 成对契约仅测试兜底~~ 已加 debug 断言 | 漏调 `begin` 的表现是定时功能静默失效 | **已完成（2026-10-24）**：`FrameScheduler::flush` 入口 `debug_assert`（本帧未 `begin` 即 panic），断言后**消费** `begun` 状态位使「下一帧漏调」也报得出；2 条 `should_panic` 用例钉住断言与消费语义；release 不编译断言、体积预算不受影响；ADR-009「后果」节已回写 |
| D-8 | ~~低~~ **已闭合（2026-10-26，`1c58052`）** | ~~`app/frame` 内部互相导入的纯函数（`group_search_hits` 被 `outline.rs` 反向导入）与三条并存的「当前文档路径」方法~~ 两项均已消解，见右列 | 组合根内 6 636 行的局部导航成本 | **完成两半**：① 路径方法在 V11-54a / R-15 已收敛——`current_document_file() -> Option<FilePath>` 是唯一出处（轻量摘要，不付全文快照），`current_document_path()` 降为面包屑用的显示包装（无路径时给「未命名文档」哨兵），`document_dir` / `active_document_title_stem` 都由它派生；② `group_search_hits` 于 2026-10-26 从 `app/frame/find.rs` 下沉到 `crates/rustmark-ui/src/presentation/state/sidebar_ui_state/search.rs`，与 `group_hits` / `locate_in_snippet` 同住一处（原先「组合函数在组合根、零件在 ui」导致 `outline.rs` 跨模块反向导入）。公开路径经 root 的 re-export 不变，端到端覆盖仍在（`tests/presentation/sidebar_view_test.rs` 的 `search_workspace` 分组与高亮断言） |
| D-9 | 低 **（已记录，不修）** | 性能/测试门禁单平台（perf 仅 Linux）与 `panic=abort` 后无 unwind | 平台特有性能退化只在本地手测发现 | 按建议**记录为已知取舍**：`docs/PROGRESS.md` 的剩余环境项已含 macOS/Linux 真机回归（V11-02）；触发条件写明——真做 macOS 分发时补 macOS perf job，在此之前不预建 |
| D-10 | 低 **（维持 + 已核）** | 富文本渲染子系统（`presentation/rich/`：math + mermaid 手绘）复杂度集中 | 该子系统是未来 bug 密度最高的候选区 | 建议已落实为现状：ADR-007 的边界由 `check-arch` 编译期保障（rich 不得回指 views/state），mermaid 图元已有部分 golden 测试；**不额外拆**——rich 现已随 V11-62 批次 5 整体住 `rustmark-ui`，再拆只会把「一个子系统」切成两个 crate 的耦合 |
| D-11 | ~~低~~ **core 侧已闭合（2026-10-23，D-3 阶段 4a + 4b）** | ~~单 crate 63 301 行~~已拆 workspace：core（`crates/rustmark-core`：四层 + presentation 六子模块）与 shell（root：`app` + `views`）两 crate，**core → shell 反向依赖已被 cargo 锁死**，**core 内部实现面已对 shell 编译不可见**（4b：模块级 pub 项 1 664 → 1 383、字段 606 → 547 + 白名单守卫）；shell 内 `app↔views` 及 core 内层间边界仍靠词法校验 | 增量编译时间；跨层误引用的检查层级 | core 侧随 4a/4b 闭合；shell 内 `app↔views` 剩余部分归 D-3 轨道（阶段 1/2/3 已闭合借用穿透，编译期强制待评估）；行数逼近 10 万或团队 >1 人时再评估拆更多 crate |
| D-12 | 低 **（纪律，持续）** | 性能阈值、体积数值散落在 `metrics.md`/`ci.yml` 注释/ADR 多处 | 数字更新时多点漂移 | 保持 `metrics.md` 为唯一数值真源，其余处只引用不复写；2026-10-26 起阈值一半以上已**机械校验**（`docs/thresholds.baseline.yaml` + `tools/check_thresholds.py`，121 条 / 9 模块），「散落」的后果从「漂移」降为「CI 红」 |

---

## 七、前次评估（2026-10-20）问题修复核对

| 前次编号 | 问题 | 现状 |
|---|---|:---:|
| P0-1 | `check-arch` 4 类绕过形态 | **已修复**：token 级判定 + `tests/` 纳入 + 豁免登记表（§一实测） |
| P0-2 | `shared/system.rs` 进程调用 | **已修复**（R-03）：端口留 shared，`std::process` 移 `infrastructure/system.rs` |
| P0-3 | Revert/InsertImage 未接线等功能缺口 | **已修复**（R-04）：`actions.rs:235/237` 均为 Handler，Unbound 清单清零 |
| P1-1 | `presentation↔app` 双向环 + 59 处裸字段写 | **大幅收敛**：裸字段写 = 0；类型依赖保留为获认可的窄豁免；**赋值形态机械守卫未做**（→ 本轮 D-3） |
| P1-2 | 导出格式双枚举 | **已修复**（R-07）：`shared/export_format.rs` 唯一真源 |
| P1-3 | span 3 份实现 + 2 张样式表 | **已修复**（R-10）：`span_job` 唯一构造器 + `StyleSemantics` 唯一派发；样式表合并经论证**否决**（度量不同、颜色同源），快照测试钉住（本轮勘误确认） |
| P1-4 | 主题双枚举 + 第三份映射 | **已修复**：真源入 `shared/theme.rs` |
| P1-5 | `persisted_key` 兜底静默失效 | **已修复**：穷举 match（无 `_ =>`）+ 双向契约测试 |
| P2-1/2/3 | 重复实现 / 路径反斜杠 / 白名单硬编码 | 未逐条复核（不在本轮重点），建议按前次批次清单核销 |
| P2-5 | 文档过期 8+ 处 | **已修复**：README ADR 索引（010–012 已列）、testing.md 已改当日实测；~~遗留 `debug-guide.md` §2.4~~ **D-2 已于 2026-10-24 闭合** |
| P2-7 | `fonts.rs` 1 111 行依赖真实资产 | **已缓解**：730 行，但仍含路径发现逻辑（择机拆分） |

---

## 八、改进路线（按投入产出排序）

**第 1 批 · 立即（零风险，文档级）**
1. ~~修 `debug-guide.md` §2.4/§400 日志章节（D-2）~~ **已完成（2026-10-24）**。
2. ~~在 `architecture-constraints.md` §7 补「视图文件 >600 行须拆」阈值（D-4 的规则化）~~ **已完成（2026-10-24）**（阈值已入 §7，评审纪律、暂未机械校验）。

**第 2 批 · 结构收口（表现层内聚性）**
3. ~~跨表共同样式「颜色恒等」直接对拍断言~~ **已完成（2026-10-23，见 D-1 闭合记录）**。
4. ~~拆 `preview_view.rs`（D-4）~~ **已完成（2026-10-25，TD-02 + TD-09）**：`preview_view.rs` 与 `toolbar_view.rs` 均已拆分到阈值以下。

**第 3 批 · 守卫补全**
5. ~~`check-arch` 增加表现层状态赋值形态检查（先报告模式摸存量再启用，D-3）~~ **已被 D-3 阶段 2 取代**（2026-10-23）：`tests/presentation/state_borrow_guard_test.rs` 的词法级白名单双向断言覆盖了同一判据，且比「报告模式」更严（存量已归零、新增穿透必红）。
6. ~~`FrameScheduler` debug 断言（D-7）~~ **已完成（2026-10-24）**。
7. ~~`shared` 准入三问入约束文档（D-6）~~ **已完成（2026-10-24）**（三问 + 登记表入约束 §4.5，且由 `check-arch` 机械核对，非仅文档）。

**第 4 批 · 可观测性**
8. ~~帧耗时超预算 `log::warn` + 归因~~ **已完成（2026-10-26，D-5）**：`FrameScheduler` 兼作观测点，超 16ms 告警带分阶段归因 + 到期任务（限流 120 帧/条），另每 600 帧一条 `debug` 常态指标；排查手册 `debug-guide.md` §5.6。

> 所有批次沿用项目既有纪律：**每步留下可执行判据**（测试 / 机械规则 / CI 门禁），不留「靠自觉」。
>
> **2026-10-26 收口**：八条全部落地——第 1 批（D-2 / D-4 规则化）、第 2 批（D-1 对拍 + D-4 拆分）、
> 第 3 批（D-3 阶段 2 / D-7 断言 / D-6 准入三问）、第 4 批（D-5 帧耗时观测）。
> 本报告此后只作**历史记录**：新问题请登记到 `docs/v1.1-backlog.md` 或新的评审报告，
> 不在此表追加行。

---

## 九、方法与局限

| 局限 | 说明 | 缓解 |
|---|---|---|
| 未运行完整测试套件与 clippy | 本轮实跑仅 `check-arch`（通过）；测试数引用 `testing.md` 2026-10-22 实测 | 结论未依赖「全绿」假设；结构性结论均为静态实测 |
| 静态分析 ≠ 运行时 | 依赖统计为文本/token 级 | 与 `check-arch` 同口径；关键 trait 签名已人工核对 |
| 行号会漂移 | 引用附文件 + 符号名，可 grep 复核 | — |
| 子调查截断 | 三个维度的子代理输出部分截断，关键数字已由主查复算（层规模/测试数/豁免/枚举归属等） | 报告内所有「实测」字样均为本会话直接运行所得 |
