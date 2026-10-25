# rustmark-core 结构改造任务清单（2026-10-24）

> 上游依据：`docs/core-structure-review-2026-10-24.md`（模块结构评审）。
> 编号延续 v1.1 backlog 约定 `V11-{序号}`（上一号为 V11-55）；优先级沿用
> P0（阻断）/ P1（正确性或结构健康）/ P2（增强）。
> 工作量口径：人日，含门禁迭代与测试写作（本机增量门禁一轮约 2–4 分钟）。
> **本清单是计划，不是执行记录；未经确认不自动开工。**

## 0. 基线快照（每项任务的验收都对着它）

| 指标 | 实测值（2026-10-24，含未提交的 D-6/D-7） |
|---|---|
| 测试 | `cargo test --all-targets` **2131 passed / 0 failed / 4 ignored**（17 个 `test result:` 行逐行求和，不得少算 lib 目标） |
| 架构校验 | `cargo run --bin check-arch` OK；禁止方向 use 边全 0；R-12 字段直写零违规；548 字段白名单 |
| Lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` 0 告警 |
| 构建 | 冷构建 83 s（190 crate）；clippy 36 s；全量测试 81 s；release 121 s |
| 体积 | release exe **10,485,248 B**；红线 10,485,760 B（`scripts/size_gate.sh` 默认）；预警线 8,388,608 B（**已处告警态，余量仅 512 B**） |
| 工作区 | root `rustmark` + `crates/rustmark-core`；拆分落盘 `2f4fd07`（2026-10-23）；D-6/D-7 已合入（`662536b`） |

> **基线勘误（2026-10-24 晚，执行中更新）**：D-6/D-7 已合入 `662536b`；V11-59 审计与
> F-1 修复（`a9f3741`）又新增 5 条用例，**测试基线现为 2136 passed / 0 failed /
> 4 ignored（`--all-targets` 2135 + `--doc` 1）**——`--all-targets` 不含 doc 测试，
> 只跑前者会看到 2135，属正常。下文各任务「2131 基线」的表述一律按 **2136** 执行。
>
> **基线再更新（2026-10-24，V11-57 完成后）：2139 passed / 0 failed / 4 ignored**
> （`--all-targets` 2138 + `--doc` 1；V11-57 新增 fixture 2 例 + 拓扑断言 1 例）。
> 下文「2131 / 2136」表述一律按 **2139** 执行。
>
> **基线再更新（2026-10-24，V11-58 完成后）：2140 passed / 0 failed / 4 ignored**
> （`--all-targets` 2139 + `--doc` 1；净 +1：拓扑断言 icons 依赖面 1 例；
> icons 4 个规格单测从 core lib 迁出为独立 crate 目标，总量守恒）。
> 下文「2131 / 2136 / 2139」表述一律按 **2140** 执行。
> **基线再更新（2026-10-24，V11-62 批次 1 完成后）：2144 passed / 0 failed / 4 ignored**
> （`--all-targets` 2143 + `--doc` 1；净 +4：fixture 第十七节 3 例 + 拓扑断言 ui 依赖面 1 例；
> widgets 7 个内联测试从 core lib 迁 ui lib，总量守恒）。
> 下文「2131 / 2136 / 2139 / 2140」表述一律按 **2144** 执行。
> V11-59 已完成（`2880678`）；V11-60 已关闭（审计 V=0，转 V11-62）；V11-61 已降级为记录；
> **V11-62 已开工——动作 2 批次 1（widgets）+ 批次 2（state）+ 批次 3（shortcuts）+ 批次 4（theme）+ 批次 5（rich）已落地，V11-62 收官**（见 V11-62 节末记录块）。
> 状态表见 `docs/v1.1-backlog.md` §3 的波次登记。

**通用验收（每项必含）**：上表五项全绿 + `cargo fmt --check` 绿 + 所有命令带
`--workspace`（root `Cargo.toml` 注释警告：否则 core 单测静默退出测试集，
测试数下降即事故信号）。

---

## 1. 总览：排序、依赖与并行

| # | ID | 任务 | 优先级 | 工作量 | 依赖 | 并行组 | 触发条件 |
|---|---|---|---|---|---|---|---|
| 1 | V11-56 | 架构文档与模块归属判据（constraints + ADR-013） | **高** | 0.5 | 无 | A | 立即 |
| 2 | V11-65 | workspace 依赖/lint 收敛 | 中 | 0.5 | 无 | A | 立即 |
| 3 | V11-66 | CI 拓扑断言 + 接入成本度量脚本 | 中 | 0.5–1 | 无 | A | 立即 |
| 4 | V11-67 | 过期文档校正 | 低 | 0.25 | 无 | A | 立即 |
| 5 | V11-59 | UI state 字段审计（四问清单） | 中 | 1 | 无 | A | 立即 |
| 6 | V11-57 | 抽 `rustmark-actions`（UiAction 词汇表） | **高** | 0.5–1 | 建议 V11-56 先行 | B | 立即 |
| 7 | V11-58 | 抽 `rustmark-icons` | **高** | 0.5 | **V11-57** | B | 立即 |
| 8 | V11-60 | state 瘦身（分批迁移纯帧内状态） | 中 | 1–2 | V11-59 | C | 立即 |
| 9 | V11-61 | find/replace 引擎上浮 application | 低 | 1 | V11-59 结论（弱） | C | 立即 |
| 10 | V11-62 | presentation 非视图子模块 → `rustmark-ui` | 中 | 2–3 | V11-56；弱依赖 V11-58 | D | **触发式** |
| 11 | V11-63 | infrastructure 去 egui 化（端口化） | 低 | 2–3 | 无（V11-62 后更顺） | D | **触发式** |
| 12 | V11-64 | 抽 `rustmark-infra` | 低 | 1–2 | **V11-63** | D | **触发式** |

**依赖与并行图**：

```
组 A（5 项互不依赖，可同时开工）         组 B（链式）         组 C           组 D（触发式）
V11-56 ─────────────────────┐        V11-57 ──> V11-58    V11-59 ─┬─> V11-60   V11-62（弱依赖 58）
V11-65 / V11-66 / V11-67    │           │                    │       └─> V11-61   V11-63 ──> V11-64
V11-59 ─────────────────────┼───────────┘（判据地基）
                             └─> 触发数据供 D 组判定
```

**关键路径**：V11-56 →（V11-57 → V11-58）→ V11-62；V11-59 → V11-60。
**建议执行顺序**：组 A 五项并行（约 1.5–2 人日合计）→ 组 B（约 1–1.5 人日）
→ 组 C（约 2–3 人日）→ 组 D 待触发数据（V11-66 产出）与用户决策（Q-01~05）
后再拍板。Wave 0–2 合计约 **5–7 人日**，全程领域层零改动、root 公开路径不变。

---

## 2. 任务明细

### V11-56 · 架构文档与模块归属判据 ｜ 优先级：高 ｜ 0.5 人日 ｜ 并行组 A

- **涉及**：`docs/architecture-constraints.md`（架构规则真源）、`docs/adr/ADR-013-*.md`（新）、`docs/README.md`（ADR 索引补 013）。
- **现状问题**：评审三大病灶（core 64% 是适配器叶层、拆分标准是编译便利、可复用性名不副实）只存在于评审报告，未进入规则真源；后续所有拆分动作缺少「模块归属判据」这一共同语言。
- **改造目标**：把「crate 地图 + 模块归属四问判据 + 外移触发条件」写成项目规范，使每个后续 commit 有据可查。
- **具体动作**：
  1. constraints.md 新增「crate 地图」一节：三阶段目标结构图（照评审报告 §4）、当前拓扑一句话（root→core 一条本地边）；
  2. 新增「模块归属判据」：依赖方向 / 变更频率 / 复用范围 / IO·框架耦合四问 + 两条本项目强信号（零内部消费者 = 叶层信号；state 零 serde = 无持久化理由）；
  3. 新增「外移触发条件」量化表（照评审报告 §7）；
  4. 落 ADR-013《core 内适配层驻留与叶层外移策略》：记录决策（保留 root→core 方向规则不变；presentation 外移采用「views 留 root 的中间 crate」形态以保 R-11/R-12 编译保证）、备选方案（搬回 root / 按上下文再拆）与否决理由；
  5. README ADR 索引补 013（现 001–012）。
- **依赖与风险**：无代码依赖。风险：文档五查（目录锚点 ↔ 标题、路径存在性、数字与实测一致）；D-6 的 arch-allow 登记制改动在途，编辑 constraints 前先确认基线版本。
- **验收标准**：constraints 新增章节可通过「任取一模块，按判据表能复现评审结论」抽查；ADR-013 与 ADR-010/011/012 同格式；README 索引 001–013 与 `docs/adr/` 文件数一致。
- **回退**：删文件/还原 diff，零行为影响。

> **完成注记（2026-10-24，`580bdc3`）**：已落地，纯文档零代码改动，`check-arch` 复跑 OK，测试基线 2140 不变。与方案的偏差及处置：
> 1. **「当前拓扑一句话（root→core 一条本地边）」已过时**：该表述写就后 V11-57/58 相继落地，现为 **4 条本地边**（root→core、root→icons、core→actions、icons→actions，两条链在 actions 汇合）。constraints §10.1 按今日实测写：root 82 文件 / 21,049 行、core 240 / 45,280、actions 4 / 687、icons 1 / 613（`find … | xargs wc -l` 口径，只含 `src/`）；并诚实记录与评审快照（46,375 行 / 238 文件）及 git 树基线 `662536b`（244 文件）的口径差，不强行对齐。
> 2. **新章节追加为 §10 而非插入中段**：constraints 的 §3 / §4 / §4.0 / §4.1 / §7 / §9 被 **7 份文档与 7 个源文件**按章节号引用（2026-10-24 实测 `grep -rl`：docs 侧 02-工程骨架 / ADR-010 / refactor-plan / review-2026-10-23 / multi-crate-review / shared-internal-cycle-review / v1.1-backlog；crates 侧 core 的 6 个 shared+domain 源文件与 icons/lib.rs——首轮 grep 曾被 `head -20` 截断少数 4 个，复核后修正），重编号会破坏全部锚点——违背「目录锚点 ↔ 标题双向比对」。故追加 §10（crate 地图 §10.1 / 归属判据 §10.2 / 外移触发 §10.3），§7 文首加交叉引用。
> 3. **触发条件表照评审 §7 六行移植并加「当前状态」列**：数字以 metrics.md §7 为家——core 增量 4.6s→**5.92s**（§7.3）；提交数按模块口径列 views 69 / tests/presentation 47 / state 31 / theme 20 并注明**模块提交数不可去重相加**（避免制造新口径错误）；「state 瘦身叫停」行标注**已由 V11-59 触发并闭环**（V11-60 关闭）。
> 4. **五查结果**：引用路径 6/6 存在；`docs/adr/` 13 文件 = README 索引 13 行；`Cargo.toml:11-13` 注释实测在位但 ADR-013 引用补全为 `crates/rustmark-core/Cargo.toml:11-13` 全路径（root manifest 同行号内容不同）；抽查示例两例（icons 拆出 + shortcuts 子模块内「词汇表拆出 / combo·keys·palette 留 core」的对比）均复现评审结论。
> 5. **ADR-013 与评审的一处对齐**：备选方案「搬回 root」的否决理由采用评审 §3.3② 记载的历史理由（views 与 state 同 crate → R-11/R-12 编译保证降级为测试保证），体积与约定问题列为附带理由；另补录评审变体 A/B 的取舍（icons 不直接依赖 core）。

### V11-65 · workspace 依赖/lint 收敛 ｜ 优先级：中 ｜ 0.5 人日 ｜ 并行组 A

- **涉及**：`Cargo.toml`（root）、`crates/rustmark-core/Cargo.toml`、`src/lib.rs`、`crates/rustmark-core/src/lib.rs`。
- **现状问题**：双 manifest 重复声明同规格依赖（egui 0.37.0 default-features=false、image 0.25 同 feature 集、syntect 5.3.0 同 feature 集（core dep + root dev-dep）、log 0.4、tempfile 3）；lint 三处重复（两个 lib.rs 各 4 行 attribute + 分层 deny）；新增依赖/新 crate 要写两遍——这是「接入成本」中可零风险消除的一块。
- **改造目标**：版本/lint 单点真源；新 crate 少写一遍清单。
- **具体动作**：
  1. root `[workspace.dependencies]` 收 egui / image / syntect / log / tempfile（含各自 feature 注释，注释从两侧 manifest 择优合并保留）；
  2. 两侧改 `{ workspace = true }`；root dev-dependencies 的 syntect/tempfile 同样收敛；
  3. `[workspace.lints.clippy]` 收 `unwrap_used/expect_used/panic = "warn"`，`[workspace.lints.rust]` 收 `missing_docs = "warn"`；两个 lib.rs 删对应 attribute（domain.rs/shared.rs 的更严 `deny` 保留——分层纪律不收敛）；
  4. 两侧 manifest 的选型注释保持（「一个事实一个家」：版本进 manifest 单点，理由留注释）。
- **依赖与风险**：无功能依赖。风险：workspace.lints 的 warn 与 CI `-D warnings` 组合须与原 lib.rs attribute 行为等价（本地不带 -D 时只告警）——用「本地 clippy 有告警/无告警两态各跑一次」验证；`Cargo.lock` 应零变化（有变化即说明规格被改）。
- **验收标准**：`git diff Cargo.lock` 为空；clippy 0 告警；2131 基线；`cargo tree -p rustmark-core` 版本串与收敛前一致。
- **回退**：revert 单 commit。

### V11-66 · CI 拓扑断言 + 接入成本度量脚本 ｜ 优先级：中 ｜ 0.5–1 人日 ｜ 并行组 A

- **涉及**：`.github/workflows/ci.yml`、`docs/workspace-topology.expected.txt`（新）、`tools/onboard_cost.py`（新，沿用 `tools/mut_*.py` 风格）。
- **现状问题**：本地依赖拓扑无机械守卫——新增一条 root→X 边不会触发任何评审；「单功能平均触及 crate 数/文件数」无度量，D 组触发条件只能拍脑袋。
- **改造目标**：拓扑变更是显式评审事件；触发条件有数据。
- **具体动作**：
  1. CI 在 check-arch 步骤后加：`cargo tree --workspace --depth 1 | diff docs/workspace-topology.expected.txt -`（期望文件入库，更新它 = 拓扑评审）；
  2. 度量脚本：git log --stat 聚合「每提交触及 crate 数/文件数/测试行数」+ 各目录 12mo 提交频率，输出 md 表；
  3. 首次运行并把基线写进 `docs/metrics.md`（本会话已有部分数字：root views 70 提交/12mo、core presentation/state 30、theme 19；冷构建 83s、增量 4.6/4.7s）。
- **依赖与风险**：无。风险：CI diff 步骤在多 OS matrix 上输出稳定性（`cargo tree` 输出含路径前缀，Windows/Linux 一致——预跑一遍确认）；度量脚本的目录口径要兼容拆分前后路径（照本会话方法：新旧路径合并计数）。
- **验收标准**：本地可复现 CI 新步骤（故意改期望文件验证会红）；脚本输出与本会话数字一致（误差 ≤1 提交）。
- **回退**：revert；期望文件保留可作后续基线。

### V11-67 · 过期文档校正 ｜ 优先级：低 ｜ 0.25 人日 ｜ 并行组 A

- **涉及**：`crates/rustmark-core/src/domain.rs`（第 3 行注释）、`docs/metrics.md`。
- **现状问题**：① `domain.rs:3` 写「可依赖 shared/ropey/chrono/serde/thiserror」，实测 domain 仅 serde（ropey/chrono 零使用）——允许清单被误读为使用清单，误导后续判断；② metrics.md 的 release 体积记录与实测差 +18,432 B，未 bisect（上轮发现）。
- **改造目标**：文档与代码事实一致。
- **具体动作**：domain.rs 注释改为「允许 serde/thiserror（实测在用）；ropey/chrono 历史上允许但当前零使用」；metrics.md 补一条 2026-10-24 实测基线（10,485,248 B，距红线 512 B），标注差异未 bisect。
- **风险**：纯注释/文档，零行为风险；domain.rs 是 `#![deny]` 门面文件，改注释不动属性。
- **验收标准**：五查通过；`cargo run --bin check-arch` 仍 OK（注释改动不触规）。
- **回退**：revert。

### V11-59 · UI state 字段审计 ｜ 优先级：中 ｜ 1 人日 ｜ 并行组 A

- **涉及**：`crates/rustmark-core/src/presentation/state/` 全部 24 文件 / 6,682 行（13 个 `*_ui_state` 296–478 行/个 + `app_ui_state` + `scroll_sync_state` + `symbol_ui_state/catalog` + `find_replace_ui_state/engine`）。
- **现状问题**：state 是全仓最热模块（30 提交/12mo）且**零 serde**（不持久化）——「UI 状态必须在 core」缺少事实依据；其中多少是纯帧内易失状态未知。
- **改造目标**：产出逐字段四问清单，作为 V11-60 的工作单与 V11-62 的搬迁依据。
- **具体动作**：逐字段打标——① 跨帧保持？② 可被单测断言？③ 经 domain/preferences 持久化？④ 被 application 消费？——每字段给三选一结论（留 core state / 挪 view-local（egui memory 或局部变量）/ 挪 application）；与 field_visibility_guard 的 548 字段白名单交叉核对（哪些白名单条目是 state 的 pub 面）。
- **依赖与风险**：只读，无依赖。风险：结论要可执行——「挪 view-local」须注明目标机制与反证（为什么不能是局部变量）。
- **验收标准**：字段覆盖率 100%（清单行数 = 结构体字段总数）；每条结论有判据引用；产出 `docs/state-field-audit.md`。
- **回退**：不适用（无代码改动）。

### V11-57 · 抽 `rustmark-actions` ｜ 优先级：高 ｜ 0.5–1 人日 ｜ 并行组 B

- **涉及**：新 crate `crates/rustmark-actions/`（`Cargo.toml` + `src/lib.rs` + `src/action.rs` + `src/action/impls.rs` + `src/action/menu_group.rs`，合计 651 行）；`crates/rustmark-core/src/presentation/shortcuts/registry.rs`（改声明）；root `Cargo.toml`（members）；`tools/arch_check.rs`（扫描根/层表）。
- **现状问题**：UiAction 词汇表住在 core 的表现层，却是 icons（待拆）与 root app/views 共用的契约；不抽出它，icons 独立 crate 只能反向依赖整个 core。
- **改造目标**：动作词汇表成为零依赖契约 crate；**25 个使用文件零 diff**。
- **具体动作**：
  1. 预检（开工第一跑）：`grep -rn 'registry::action' src crates tests` 确认无人用深层路径（本会话实测为 0；icons.rs 对 core 的唯一引用是 `use crate::presentation::shortcuts::registry::UiAction`，icons.rs:44）；
  2. `git mv` 三文件进新 crate；`Cargo.toml`：edition 2024 / publish=false / **零 dependencies** / dev 无；lib.rs 加 `#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` + `#![warn(missing_docs)]`（与 core 同款门禁）——注意 arch_check 禁 `mod.rs`（`FORBIDDEN_MOD_FILE`），保持 `action.rs + action/` 布局；
  3. core `registry.rs`：`pub mod action; pub use action::{MenuGroup, UiAction};` → `pub use rustmark_actions::{action, MenuGroup, UiAction};`（连带 re-export `action` 模块，深层路径也保持解析）；
  4. core `Cargo.toml` 加 `rustmark-actions = { path = "../rustmark-actions" }`；root members 加新 crate；
  5. `arch_check.rs`：扫描根加 `crates/rustmark-actions/src`；该路径判 Free 层（零依赖纯词汇，白名单自由位置）；路径归一化（`strip_prefix`）加一条；本地依赖边期望更新为 root→{core}、core→actions；
  6. 随迁三文件中的内联测试（如有）。
- **依赖与风险**：建议 V11-56 先行（判据地基）；强风险点是 arch_check.rs 与在途 D-6/D-7 同文件冲突——**先合入 D-6/D-7 再动**（见 BLK-03）。
- **验收标准**：`cargo tree -p rustmark-actions` 零依赖；`git diff --stat` 中 25 个 UiAction 使用文件零改动；四门禁 + 2131 基线；check-arch OK 且 `tests/arch_violation_fixture.rs` 补一例「actions crate 内引用 core」必报（若 checker 覆盖新根）。
- **回退**：revert 单 commit（新 crate 删除、registry.rs 还原）。

> **完成注记（2026-10-24，`a38ac43`）**：已落地，门禁 2139/0/4。与方案的偏差及处置：
> 1. **守卫方案经用户裁定取「方案 b」**（另两案：a 专用 `Layer::Actions` / a+ 联动 V11-56 文档）：`arch_check.rs` 对 actions 根**强制 Free 层**（`layer_of` 按根内相对路径判定、不感知 crate，不强制会把 `action.rs` 首段报「未知层」——变异 A 实测真实工程 3 条 panic），反向依赖由 **Cargo 循环依赖**结构性挡死，零依赖由 `workspace_topology_test` 的 `actions_crate_declares_no_dependencies` 断言兜底。
> 2. **验收标准中「fixture：actions 内引用 core 必报」按条件豁免**：Free 层下「actions→core」是合法自由引用，层规则管不到（这正是 Cargo 环兜底的原因），原字面 fixture 与方案 b 互斥。实补两例替代：`actions_root_is_judged_free_without_false_unknown_layer`（防「未知层」误报）+ `layer_independent_rules_still_apply_in_actions_root`（帧预约等层无关规则在新根仍生效，变异 B 整根跳过→该例红、零违规用例 vacuous 通过，实证「零违规守卫证明不了牙齿」）。
> 3. **动作 2 的 lib.rs attribute 要求已过时**：V11-65 起 `[workspace.lints]` 单点真源，新 crate 不写顶部 attribute。
> 4. **连带修复**：`tests/app/shortcut_action_guard_test.rs` 路径锚点随迁（`action.rs` / `action/impls.rs` 改 actions 根相对路径，`src_roots()` 加第三根）——不迁则该守卫 `.expect` panic，且 `persisted_key` 兜底扫描**静默丢失**该文件覆盖。

### V11-58 · 抽 `rustmark-icons` ｜ 优先级：高 ｜ 0.5 人日 ｜ 并行组 B

- **涉及**：新 crate `crates/rustmark-icons/`（589 行 + 4 个规格单测）；`crates/rustmark-core/src/presentation.rs`（删 `pub mod icons`）；`src/presentation.rs`（桥接）；root `Cargo.toml`；`tools/arch_check.rs`（`PRESENTATION_ICONS_FORBIDDEN` 表现有对应文件消失——清理或留防回归）；`tests/presentation` 中图标视觉用例归属核对。
- **现状问题**：icons 是零领域逻辑、零 IO 的纯资源（16×16 向量规格数据 + Painter 绘制），core 内零消费者、仅 3 个 root 视图文件使用——典型的「住在 core 里但没有 core 的职责」。
- **改造目标**：资源出 core；root 公开路径 `crate::presentation::icons::*` **零变化**。
- **具体动作**：
  1. `git mv presentation/icons.rs` → `crates/rustmark-icons/src/lib.rs`（或 `icons.rs` + `pub use`）；唯一 core 内部引用 `use crate::presentation::shortcuts::registry::UiAction` → `use rustmark_actions::UiAction`；egui import 不动；
  2. 4 个规格纪律单测随迁；
  3. core `presentation.rs` 删 `pub mod icons;`；
  4. root 桥接改为两行：`pub use rustmark_core::presentation::{rich, shortcuts, state, theme, widgets};` + `pub use rustmark_icons as icons;`（re-export crate 为模块，路径保持）；
  5. root `Cargo.toml`：members + `rustmark-icons` dep；arch_check 扫描根/层表/期望边同步更新（root→icons 新边）。
- **依赖与风险**：**强依赖 V11-57**（UiAction 来源）。风险：`pub use crate as mod` 的路径等价性——验收第 2 条专测；体积零增（同代码搬迁，>+1 KiB 即调查）。
- **验收标准**：`cargo tree -p rustmark-icons` 依赖 = egui + rustmark-actions；`rustmark::presentation::icons::IconOp` 等公开路径可引用（既有测试覆盖即证）；4 个规格单测通过；工具栏/侧栏图标真帧视觉用例全绿；四门禁 + 2131 + size_gate（exe ≤ 10,485,760 B）。
- **回退**：revert 单 commit。

> **完成注记（2026-10-24，`69c8bcc`）**：已落地，门禁 2140/0/4。与方案的偏差及处置：
> 1. **文件布局取 `lib.rs` 单文件**（非「icons.rs + pub use」）：`layer_of` 对 crate 入口白名单自然判 Free，arch_check 第四根**无需** `forced_layer`（actions 根那种强制只在其文件名不在任何层名时才需要）；将来 icons 长出第二个文件，「未知层」警报会逼着定层——缺口要响。
> 2. **field_visibility_guard 范围随扩**（任务清单未点名、但不扩即静默丢覆盖）：白名单 2 条 `presentation/icons.rs::IconColors::*` 改键为 `icons/lib.rs::IconColors::*`（crate 短名前缀防 `lib.rs` 同键碰撞），扫描函数泛化为「core 裸路径 + 叶子 crate 前缀路径」两根；总数仍 548。
> 3. **`PRESENTATION_ICONS_FORBIDDEN` 规则与 fixture 保留防回归**（任务清单「清理或留防回归」取留）：icons 搬出 core 后该子层在 core 内休眠，`leaf_layers_may_not_depend_on_views` 合成用例继续钉住规则本身；约束文档 §4.0 表格行标注「V11-58 起休眠」。
> 4. **fixture 拼接点用例随实桥接扩展**：`shell_reexport_splice_points_are_not_violations` 的合成 `presentation.rs` 补 `pub use rustmark_icons as icons;` 形状（真实桥接已改两行）。
> 5. **视觉用例归属核对结论**：`tests/presentation/toolbar_visual_test.rs` 只用公开路径 `rustmark::presentation::icons::{Icon, spec}`，桥接后零改动，留在 presentation 测试目标。
> 6. clippy 首跑暴露 3 处新问题（collapsible if + 6 处 doc 列表续行缩进——文档注释续行丢了两空格缩进，触发 `doc_lazy_continuation`），已修；复跑全绿。

### V11-60 · state 瘦身（分批） ｜ 优先级：中 ｜ 1–2 人日 ｜ 并行组 C

- **涉及**：按 V11-59 清单定位的 state 文件子集 + 对应 views（root）。
- **现状问题**：纯帧内/易失状态占用 core 领域 crate 的空间与编译面（改它要重编 core → 连带 root），且撑大 R-12 守卫面。
- **改造目标**：命中「纯帧内」判据的状态全部 view-local；core 行数下降；状态写入面进一步收敛。
- **具体动作**：按清单分 2–3 批，每批一个 commit；迁移模式：hover/展开动画/临时 in-flight 编辑 → egui memory（`ctx.data_mut` / `ui.memory`）或视图局部变量；跨帧但易失 → 评估 view-local + `Id`；**不碰**需持久化（走 domain/preferences）与被 application 消费的状态。
- **依赖与风险**：依赖 V11-59。风险：帧内状态改 memory 后有「切标签丢状态」类行为变化——逐批真帧回归；零新增 `arch-allow` 登记。
- **验收标准**：每批四门禁 + 2131 + 真帧回归；`presentation/state` 目录行数较基线下降（目标 ≥15%，约 1,000 行）；R-12 零违规；548 字段白名单只减不增（集合相等断言自然守住）。
- **回退**：分批 revert。

### V11-61 · find/replace 引擎上浮 application ｜ 优先级：低 ｜ 1 人日 ｜ 并行组 C

- **涉及**：`presentation/state/find_replace_ui_state/engine.rs`（351 行）、`application/document/`（新用例）、`shared/pattern`（不变）。
- **现状问题**：模块文档自述「查找引擎：编译后扫描、循环定位、全部替换预览」——匹配扫描算法住在 UI state 层；无 headless 需求时无痛，一旦 CLI 批量替换必须上浮。
- **改造目标**：「扫描 → Match 列表」成为 application 用例；UI state 只留选区索引/循环/预览态。
- **具体动作**：新增 `application::document::find_matches(text, options) -> Vec<Match>`（构建于 shared::pattern，零新依赖）；engine.rs 删扫描主体，保留 UI 编排；`Match` 类型随用例移 application（UI 侧经 DTO 或直接引用——二选一，选直接引用以控 diff）。
- **依赖与风险**：弱依赖 V11-59（若审计已判 engine 留 core 则本项降级为记录）。风险：`Match` 移动触及 `find_replace_ui_state` 字段类型 → 548 白名单联动；替换预览语义不能变（零宽匹配/循环边界须有对拍）。
- **验收标准**：engine.rs ≤100 行或删除；新用例有单测（零宽匹配、循环、大小写、正则子集边界）；查找替换全用例绿；2131。
- **回退**：revert。

### V11-62 · presentation 非视图子模块 → `rustmark-ui` ｜ 优先级：中（触发式）｜ 2–3 人日 ｜ 并行组 D

- **涉及**：新 crate `crates/rustmark-ui/`（约 18.1k 行：state 6,682 + theme 3,482 + rich 5,918 + shortcuts 1,532 + widgets 499；icons 若已拆则不含）；`src/presentation.rs` 桥接；root `Cargo.toml`；`tools/arch_check.rs`（扫描根 + ui 内层判定）；`tests/field_visibility_guard_test.rs`（范围扩展）；`tests/presentation/`（43 文件按归属拆分）；root `Cargo.toml` 的 `[[test]]` 显式声明联动。
- **现状问题**：core 40.6% 是 UI 适配器；UI 高频迭代（state 30 + theme 19 + shortcuts 10 + rich 3 ≈ 62 提交/12mo）全部压在 core 上，改一行 egui 代码重编 core→root 全链。
- **改造目标**：core 名实相符（剩 27.6k 脊柱 + infra）；UI 单 crate 迭代；**R-11/R-12 编译保证不降级**。
- **具体动作**：
  1. 触发条件核验（任一）：UI 并行开发 ≥2 人；core 增量编译 >10 s（现 4.6 s）；presentation 年变更 ≥80 提交且「views↔core state 跨 crate 摩擦」在 ≥3 个功能中复现——用 V11-66 数据判定，不满足则维持现状；
  2. 建 ui crate（egui + rustmark-core（+rustmark-actions/icons 若已拆））；`git mv` 5–6 个子模块，**views 留 root**；
  3. root 桥接：`pub use rustmark_ui::{rich, shortcuts, state, theme, widgets};`——`crate::presentation::<子模块>` 公开路径零变化；
  4. arch_check：扫描根加 `crates/rustmark-ui/src`；ui 内按相对路径判层（ui/src/state → PresentationState 等，层表新增一套或参数化）；`PRESENTATION_*_FORBIDDEN` 词表中的 `crate::presentation::views` 相应清理（views 不在 ui 内，跨 crate 边界天然保证）；
  5. **field_visibility_guard 范围从 `crates/rustmark-core/src` 扩展到 `crates/rustmark-ui/src`**——state 的 pub 面钉死不能松，这是编译保证不降级的关键守卫；
  6. tests/presentation 按被测模块归属拆分：测 ui 模块的随迁（ui crate 内建 tests/ 并显式声明目标，注意 autotests 纪律）；测 views 的留 root；
  7. 若 icons 未先行拆出，则随本次一并搬入 ui（UiAction 缠结在 ui→core 方向内自然消解）。
- **依赖与风险**：V11-56（判据）+ 触发条件；弱依赖 V11-58。风险：① 编译保证降级风险仅当误把 views 搬进 ui——**views 必须留 root**；② 测试搬迁总量大（622 条 presentation 用例按归属拆）——先搬后改路径，每批门禁；③ arch_check 双 roots 归一化逻辑（照 `2f4fd07` 拆分的 `strip_prefix` 模式扩第三例）。
- **验收标准**：`cargo tree --workspace --depth 1` = root→{core,ui,(actions),(icons)} + ui→core（与期望文件一致）；「views 写 ui 私有字段编译失败」抽验 1 例（或 R-12 词法规则 + guard 测试双绿）；2131 基线（逐行求和）；size_gate；release 构建通过；core 目录行数 ≤ 27.6k。
- **回退**：revert 单 commit（搬迁是可逆的机械操作）。

> **触发条件核验（2026-10-24 实测，动作 1 已完成）**。条件为「任一满足即启动评估」：
> - ① UI 并行开发 ≥2 人：**待维护者提供**（git 不可测）。
> - ② core 增量编译 >10s：**不满足**（5.92s，metrics.md §7.3）。
> - ③ ≥80 提交**且**摩擦在 ≥3 个功能复现：**满足**。提交数按 `git log` 去重、新旧路径合并——表现层整体 **88**、含 `tests/presentation` **93**、仅待搬迁的非视图部分 **51**（阈值 80：整体口径过线、待搬迁部分未过线，两种口径如实并存，不替评审裁定）。模块口径另记 views 69 / tests/presentation 47 / state 31 / theme 20（metrics.md §7.2，模块提交数不可去重相加）。
> - **摩擦取证**（先定义后测量，避免事后解释）：F-a = 为跨 crate 读写而必须 `pub` 的 state setter；F-b = 为跨 crate 可见性而登记的 `pub` 字段白名单条目；F-c = views 经桥接访问 state 的面。实测：**F-b 169 条 / 548 条白名单是 state 字段**（settings 25、app 16、sidebar 28、find_replace 19、export 14、editor 12、scroll_sync 8、diff 8、preview 7、status_bar 6、shortcut_overview 6、quick_open 5…）；**F-a 22 个 `pub fn set_/with_`** 分布在 12 个 state 模块；**F-c** 桥接 6 行（`src/presentation.rs`）+ 21 个 view 文件经 `state::` 访问。三个形态在 **8 个功能**复现：设置、侧栏（含搜索/文件树）、查找替换、导出、编辑器（含滚动同步）、预览、diff、命令面板·快速打开·符号。
> - **一项必须直说的结论**：该摩擦是「保住 R-11/R-12 编译保证」的代价，**V11-62 不会消除它**——views 留 root，state 搬到 ui crate 后边界依旧存在，169 条白名单与 22 个 setter 只是换 crate（对应动作 5：field_visibility_guard 范围须扩到 ui crate）。V11-62 的真实收益仍是 core 名实相符（45.3k → 27.6k）、UI 单 crate 迭代、以及 headless 复用的前置条件。
> - **判定**：触发成立（③），可进入动作 2；是否开工由维护者拍板——本项量级 2–3 人日、涉及 622 条用例拆分与 arch_check 改双 roots。

> **动作 2 批次 1 已落地（widgets，2026-10-24，`9ab57ae`）。** 用户已拍板「进入 V11-62 动作 2」，批次策略：widgets → theme → shortcuts → state → rich，每批一个 commit、每批门禁、可 revert。批次 1 实测如下。
>
> - **新 crate 三件套**：`crates/rustmark-ui/`（Cargo.toml + src/lib.rs + src/presentation.rs），**核心设计是 ui crate 保留 `presentation/` 路径前缀**（`crates/rustmark-ui/src/presentation/widgets.rs`）——`layer_of` 按根内相对路径判层零改动（widgets → PresentationWidgets 子层），全部禁令表/豁免表零改动，且 **views 若被误搬进 ui 会自动触发既有禁令**（ADR-013 决策 3 的结构防护）。依赖面 = egui + rustmark-core（本地边 4→6：root→ui、ui→core；ui→core→actions 与 icons→actions 在 actions 汇合，无环）。
> - **桥接路径与 core 同形**：root `pub use rustmark_ui::presentation::widgets;`（后续批次迁入 state/theme/rich/shortcuts 时只改花括号列表）；8 个 view 文件 `use crate::presentation::widgets;` 零改动，公开路径零变化。
> - **每批守卫扩展决策规则（本批确立，防后人重复决策）**：field_visibility_guard 必须扩根（pub 面跨 crate 边界，白名单 548 条零变化——widgets 零结构体零 pub 字段）；四个模式守卫（default_extension / export_format / shortcut_action / theme）**不扩**——按 icons 先例，只在「搬迁代码实际携带被守模式」的批次扩（theme 批次必须扩 theme_guard，其 SOURCE_OF_TRUTH 会搬走）。
> - **归一化边界（坑）**：arch_check 只把 `rustmark_core::` 归一为 `crate::`；`rustmark_ui::` **不归一**（root 拼接 core 全部模块 vs 只拼 ui 的 widgets，两种拼写不等价）。fixture 合成内容同步改三行拼接。
> - **新 crate 必备件（坑）**：lib.rs 须带 `#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]`——workspace lints 的 panic=warn 会打死迁入的带 `panic!` 测试代码。
> - **守卫与 fixture**：arch_check 加第五根（ui 根不强制层，lib.rs 自然 Free）；build.rs 加 rerun-if-changed；field_visibility_guard 扩到 3 根（键格式 `<短名>/<路径>`）；fixture 新增第十七节 3 例（ui 前缀不误判 / ui 根洗不掉子层禁令 / views 误搬 ui 照旧报）。
> - **五门禁实测**：fmt OK；check-arch OK（未发现分层依赖违规）；clippy OK；`--all-targets` **2143 passed / 0 failed / 4 ignored** + `--doc` 1 = 合计 2144（基线 2140，+4 = 3 fixture + 1 拓扑断言）；topology CI 式 diff CLEAN；release exe **10,485,248 B 与基线逐字节相同**（红线 10,485,760 B，余 512 B）。
> - **三组变异验证全捕获**：① 注释掉 arch_check 第五根 → 两个「ui 根禁令」用例双红；② 向 ui/presentation.rs 注入 `pub struct` + `pub` 字段 → guard 报 `ui/presentation.rs::...`；③ 向 ui Cargo.toml 注入第三个依赖 `log` → 依赖面断言红。
> - **文档同步**：constraints §4.1/§7/§10.1/§10.3（含三阶段演进图重算）；topology 期望文件重算；本 backlog 波次表；本节。
> - **批次 2–5 计划（原定）**：theme（3,482 行，**必须扩 theme_guard src_roots**——SOURCE_OF_TRUTH 随迁）→ shortcuts（1,532）→ state（6,682，field guard 白名单重点核对）→ rich（5,918）。views 始终留 root。**【实际执行顺序因依赖环调整见下】**
>
> **动作 2 批次 2 已落地（state，2026-10-25，`c40235c`）。** 原定顺序 theme→shortcuts→state→rich **被依赖分析推翻**，改为按依赖拓扑 **state → shortcuts → theme → rich**（state 为枢纽最先搬）。批次 2 实测如下。
>
> - **顺序调整（阻塞后用户拍板方案 A）**：core 内 `state → {theme, shortcuts}`（3+11 处真实代码依赖），theme/shortcuts/rich 为纯叶。若先搬 theme 会致 core(state) 的 `crate::presentation::theme` 失效 + 形成 core→ui 回边成环（与既有 ui→core 边相撞），Cargo 拒编。故 state 先行（ui→core 引用 theme/shortcuts 合法，纯路径改写）。后续：批次 3 = shortcuts、批次 4 = theme（**必须扩 theme_guard src_roots**）、批次 5 = rich。
> - **搬迁规模**：24 文件 / 6,682 行 git mv 入 `crates/rustmark-ui/src/presentation/state/`（保留历史）；15 文件 import 改写 `crate::{shared,application,domain,infrastructure,presentation::{shortcuts,theme}}` → `rustmark_core::*`（内部兄弟引用 `crate::presentation::state::*` 保持不变）；root 桥接改为 `{state, widgets}`；core `presentation.rs` 删 `pub mod state` + lib.rs 文档清单；theme 内 squiggle.rs 指向 state 的 intra-doc 链接改文字指路（非真实依赖）。
> - **跨 crate 可见性摩擦（R-11 保证的代价，实证 2 处）**：① `ActionEntry`/`ActionTable` 从 `pub(crate)` 升 `pub` 并 re-export；② `ActionEntry::group` 与 `RecentFileView::last_opened` 字段升 `pub`。三处均登记 field_visibility_guard 白名单（548 → **550**，接口面增长留痕）。
> - **白名单改键**：168 条 state 条目 `presentation/state/...` → `ui/presentation/state/...`（ui 根批次 1 已铺，覆盖无断档）；guard 文档注释「暂无 ui 条目」失实表述同步修正；fixture 中「state 现住 core 根」注释同步改「ui 根」。
> - **五门禁实测（代码全绿）**：fmt OK；check-arch OK；clippy OK；`--all-targets` **2143 passed / 0 failed / 4 ignored** + `--doc` 1 = 2144（state 15 个内联测试块 core→ui 总量守恒）；topology CI 式 diff CLEAN（本地边仍 6 条，state 走既有 ui→core 边）。
> - **⚠️ release exe 越旧红线，触发 NFR 红线决策**：批次 2 树 **10,487,808 B**，较批次 1 基线（10,485,248 B，worktree @ `9ab57ae` 共享 target 重建逐字节相同）**+2,560 B**。旧红线 10 MiB（余 512 B）耗尽越线。worktree A/B 实证 +2,560 B 确由跨 crate 搬迁在 `opt-level=z` + fat LTO + `codegen-units=1` 下的 LTO 边界成本产生，无针对性修法。**维护者决策：NFR 红线 10 → 11 MiB、预警 8 → 10 MiB（原红线降级为预警）**，已同步 `scripts/size_gate.sh`、`docs/metrics.md` §2.8 + NFR-PERF-01、`docs/architecture-constraints.md` §预算、TD-01。新红线 11 MiB 下距红线余 **1,046,528 B（0.91 MiB）**，可容纳批次 3–5。
> - **文档同步**：constraints §7（state 移出 core 模块树）/§10.1；metrics.md §2.8 + NFR-PERF-01 + TD-01；本 backlog 波次表与本节。
>
> **动作 2 批次 3 已落地（shortcuts，2026-10-25，`6778f20`）。** 顺序（方案 A）：state → **shortcuts** → theme → rich。批次 3 实测如下。
>
> - **纯叶搬迁**：shortcuts 仅依赖 `application::dto`（1 处 import），不引用 theme/state/rich，无内部依赖边——顺序自由。5 文件 / **892 行** git mv 入 `crates/rustmark-ui/src/presentation/shortcuts/`（保留历史）；1 处 import 改写 `crate::application::dto` → `rustmark_core::application::dto`；自引用 `crate::presentation::shortcuts::registry::normalize` 路径搬后有效不改。
> - **跨 crate 依赖连锁（批次 2 的副效应，本批暴露并修复）**：state 在 ui crate 内原引 `rustmark_core::presentation::shortcuts`（批次 2 改写时 shortcuts 尚在 core），shortcuts 搬入 ui 后该路径失效 → 11 处改回 `crate::presentation::shortcuts`（同 crate 引用）。另 registry 原直引 `rustmark_actions`（ui 不直接依赖该 crate）→ 改经 `rustmark_core::actions`，由 **core re-export** `pub mod actions { pub use rustmark_actions::{MenuGroup, UiAction, action}; }` 间接暴露，保持 ui 依赖面 = egui + core（拓扑断言 `ui_crate_dependency_face_is_egui_plus_core` 不变）。
> - **白名单改键**：10 条 shortcuts 条目 `presentation/shortcuts/...` → `ui/presentation/shortcuts/...`（ui 根批次 1 已铺，覆盖无断档）；白名单总数仍 550。
> - **零守卫扩展**：shortcuts 是定义点，不携带被守的消费模式，四个模式守卫（default_extension / export_format / shortcut_action / theme）均不扩。
> - **五门禁实测**：fmt OK；check-arch OK；clippy OK；`--all-targets` **2143 passed / 0 failed / 4 ignored** + `--doc` 1 = 2144（shortcuts 零内联测试块，总量守恒）；topology CI 式 diff CLEAN（本地边仍 6 条，shortcuts 走既有 ui→core 边）。
> - **release exe 10,488,832 B**：较批次 2（10,487,808 B）+1 KiB，新红线 11 MiB 下 size_gate exit 0（仅预警），距红线余 **1,045,504 B（~1.0 MiB）**，可容纳批次 4–5。
> - **文档同步**：constraints §7（模块树 shortcuts 移出 core / ui 含 widgets+state+shortcuts）+ §10.1（crate 地图 core 206/37,178、ui 36/8,188、规模口径、演进图 2026-10-25 注记）；本 backlog 波次表与本节。

> **动作 2 批次 4 已落地（theme，2026-10-25，`14f7d87`）。** 顺序（方案 A）：state → shortcuts → **theme** → rich。批次 4 实测如下。
>
> - **纯叶搬迁**：theme 仅依赖 `shared::theme`（`ThemeKind`）/ `application::dto`（`CustomThemePaletteView` / `SpanView` / `ThemeView` / `RangeView`）/ `shared::geometry`，不引用 state/shortcuts/rich，无内部依赖边——顺序自由。11 文件 / **3,487 行** git mv 入 `crates/rustmark-ui/src/presentation/theme/`（保留历史，`layer_of` 按根内相对路径判子层零改动，views 若误搬 ui 自动触发既有禁令）；10 文件内 `crate::shared` / `crate::application` 共 ~13 处 import 改写 `rustmark_core::*`，内部兄弟引用 `crate::presentation::theme::*` 保持不变（同 crate 仍有效）。
> - **同 crate 引用回收（批次 2/3 搬迁的副效应，本批收尾）**：ui 内 `widgets/row.rs`、`state/app_ui_state.rs`、`state/preview_ui_state.rs` 原引 `rustmark_core::presentation::theme`（theme 尚在 core 时合法），theme 搬入 ui 后改回 `crate::presentation::theme`（同 crate 引用）。
> - **零跨 crate 可见性摩擦**：`Theme` / `Metrics` / `palette_signature` 等对外符号本就 `pub`（root 视图经 `rustmark::presentation::theme` 公开路径早已消费），无需升 `pub`，白名单总数仍 **550**。
> - **守卫扩展（唯一一处）**：`theme_guard_test.rs` 的 `src_roots()` 由 2 根扩为 3 根（增 `crates/rustmark-ui/src`，与 arch_check 同口径）——SOURCE_OF_TRUTH 为 `shared/theme.rs`，不在 theme 模块内，不受迁移影响；扫描覆盖 theme 迁入 ui 后的文件。field_visibility_guard 白名单 77 条 theme 键 `presentation/theme/...` → `ui/presentation/theme/...`（ui 根批次 1 已铺，覆盖无断档）；两守卫总数仍 550。其余模式守卫（default_extension / export_format / shortcut_action / theme 重复枚举）均不扩。
> - **五门禁实测**：fmt OK；check-arch OK；clippy OK；`--all-targets` **2143 passed / 0 failed / 4 ignored** + `--doc` 1 = 2144（theme 的 `mapper/tests.rs` 内联测试块 core→ui 总量守恒）；topology CI 式 diff CLEAN（本地边仍 6 条，theme 走既有 ui→core 边）。
> - **release exe 10,489,856 B**：较批次 3（10,488,832 B）+1 KiB（1,024 B），新红线 11 MiB 下 size_gate exit 0（仅预警），距红线余 **1,044,480 B（~1.0 MiB）**，可容纳批次 5（rich）。
> - **文档同步**：constraints §7（模块树 core 仅剩 rich / ui 含 widgets+state+shortcuts+theme）+ §10.1（crate 地图 core 195/33,693、ui 47/11,676、规模口径、演进图 2026-10-25 注记）+ R-13 表（rich 禁令 theme 已迁出说明）；metrics.md §2.8 + NFR-PERF-01 + TD-01；本 backlog 波次表与本节。
>
> **动作 2 批次 5 已落地（rich，2026-10-25）。** V11-62 收官批次：rich 是表现层非视图子层中最后一个、且唯一自绘排版的引擎（数学公式 / Mermaid 图表）。顺序自由（纯叶），零守卫扩展（rich 不携带被守模式，arch_check 的 `PresentationRich` 禁令针对四个 workspace 兄弟子层、rich 对它们零引用；egui 属第三方不被拦）。批次 5 实测如下。
>
> - **纯叶搬迁**：rich 仅依赖 egui（自绘排版，零 `crate::shared` / `application` / `domain` / `infrastructure` 依赖，不引用 theme/state/shortcuts/widgets/views），25 文件（含模块根 `rich.rs`）/ **5,918 行**（目录）+ `rich.rs` 53 行 = **5,971 行** git mv 入 `crates/rustmark-ui/src/presentation/rich/`（保留历史，`layer_of` 按根内相对路径判 `PresentationRich` 子层零改动，R-13 仍适用）；rich 内部零真实 `crate::` 依赖，**无需 import 改写**。
> - **模块根随迁**：`core/.../presentation/rich.rs`（声明 `pub mod math; pub mod mermaid;`）一并 git mv 到 `ui/.../presentation/rich.rs`，使 `ui/presentation.rs` 的 `pub mod rich;` 有根文件；core 侧 `presentation/` 目录清空，`crates/rustmark-core/src/presentation.rs` 退化为 doc-only 空壳（保留 `pub mod presentation;` 维持 crate 路径稳定）。
> - **root 桥接对调**：`src/presentation.rs` 把 `pub use rustmark_core::presentation::rich` 移除、rich 加入 ui 行的花括号列表 `{rich, shortcuts, state, theme, widgets}`；`views.rs` 经 `crate::presentation::rich::{math, mermaid}` 透明复用，零改动。
> - **唯一守卫改动**：field_visibility_guard 9 条 `presentation/rich/...` 白名单键（`MathLayout::ascent/width`、`MermaidLayout::height/width`、`MermaidStyle::edge/label_bg/node_fill/node_stroke/text`）改 `ui/presentation/rich/...`（ui 根批次 1 已铺，覆盖无断档）；白名单总数仍 550。arch_check 零改动（`PresentationRich` 禁令词法守门、rich 对兄弟子层零引用）。
> - **五门禁实测**：fmt OK；check-arch OK（未发现分层依赖违规）；clippy OK（CLIPPY_EXIT=0）；`--all-targets` **2143 passed / 0 failed / 4 ignored** + `--doc` 1 = 2144（rich 内联测试 core→ui 守恒）；topology CI 式 diff CLEAN（本地边仍 6 条，`ui_crate_dependency_face_is_egui_plus_core` 不变）。
> - **release exe 10,487,296 B**：较批次 4（10,489,856 B）**-2,560 B**（非单调）——rich 自绘排版零跨 crate 内联，迁移回吐批次 2 引入的 LTO 边界成本；新红线 11 MiB 内、超预警 10 MiB（SIZE_EXIT=0 仅预警），距红线余 **1,047,040 B（~1.0 MiB）**。V11-62 收官，体积窗口稳定。
> - **文档同步**：constraints §7（模块树 core 的 `presentation` 退化为 doc-only 空壳 / ui 含全部五个非视图子模块）+ §10.1（crate 地图 core 169/27,721、ui 73/17,648、规模口径、演进图 2026-10-25 收官注记）+ R-13 表（rich 迁入 ui 后禁令转词法守门说明）；metrics.md §2.8 + TD-01；本 backlog 波次表与本节。
>
> **V11-62 收官**：动作 2 五批次（widgets / state / shortcuts / theme / rich）全部落地，core 表现层非视图子模块清零，`rustmark_core::presentation` 退化为 doc-only 空壳；views 始终留 root（R-11/R-12 编译保证保留）。后续见 V11-63（infrastructure 去 egui 化）/ V11-64（抽 `rustmark-infra`）。

### V11-63 · infrastructure 去 egui 化 ｜ 优先级：低（触发式）｜ 2–3 人日 ｜ 并行组 D

- **涉及**：`infrastructure/fonts.rs` + `fonts/discovery.rs` + `fonts/assembly.rs`（产 `egui::FontDefinitions`）、`infrastructure/export/viewport_capture.rs` + `viewport_capture/image.rs`（`egui::ViewportCommand::Screenshot` / `ColorImage`）；`shared/`（新端口）；`src/app/frame.rs` 组合根（装配点）。
- **现状问题**：core 拖着 egui 的根因就在这 5 个文件——不端口化，「core 可 headless 复用」永远不成立，`rustmark-infra`（V11-64）也无从谈起。
- **改造目标**：`cargo tree -p rustmark-core | grep egui` 为 0；infrastructure 只产字节与端口类型。
- **具体动作**：
  1. 设计预研（本任务第一个交付物）：截图端口化要处理 `ColorImage`/`Color32` 也是 egui 类型——定义自有像素类型（或复用 image crate 的 `RgbaImage`，它已在依赖中）作端口；
  2. fonts：shared 定义 `FontSource`（字节 + 族名 + 样式 + 键）端口；discovery 只产端口类型；`FontDefinitions` 装配移至组合根（或 ui 侧）；
  3. viewport_capture：截图请求经组合根端口（root app 调 `ViewportCommand::Screenshot`），infra 只接像素缓冲；
  4. 端口实现与测试随迁；字体/导出/截图用例改写为对端口编程。
- **依赖与风险**：无强依赖（V11-62 后装配落点更顺，但也可落 root 组合根）。风险：字体装配顺序（选中置顶/内置 Monospace/CJK 追加/math_fallback）是 REL-03 实测逐项对齐过的——端口化后必须有对拍测试锚定顺序；截图链路只能真机验证（CI 无显示）——保留手动回归项。
- **验收标准**：`cargo tree -p rustmark-core | grep -c egui` = 0；字体装配对拍用例绿（族顺序/去重/兜底与端口化前一致）；导出截图相关用例绿；2131；真机手动回归记录（字体渲染、导出图片）。
- **回退**：revert。

### V11-64 · 抽 `rustmark-infra` ｜ 优先级：低（触发式）｜ 1–2 人日 ｜ 并行组 D

- **涉及**：`crates/rustmark-core/src/infrastructure/` 全部 51 文件 / 10,824 行 → 新 crate `crates/rustmark-infra/`；root 组合根装配点；`tools/arch_check.rs`；`tests/infrastructure/`（302 条用例随迁）。
- **现状问题**：infrastructure 层归属正确但 crate 粒度错配——复用 domain 的人被迫拉 syntect/ureq/rfd/pulldown/image。
- **改造目标**：core = 纯六边形 16.7k；infra 独立成右侧适配器 crate。
- **具体动作**：触发条件核验（出现真实非 UI 消费者 + V11-63 完成）；`git mv` 整体搬迁；infra crate 依赖 rustmark-core（端口在 core，实现在 infra——方向合法）；root members/dep 更新；arch_check 第三 roots + 层表；tests/infrastructure 随迁（显式 test 目标声明）。
- **依赖与风险**：**强依赖 V11-63**。风险：装配链（20+ 测试使用的 `assemble_with_clock_and_dialogs` 类参数化装配）跨 crate 后 import 面变化——沿用项目既有模式（新增最深变体 + 旧链转发，调用点零改动）；rfd/syntect 等重依赖的编译缓存收益此时才兑现。
- **验收标准**：`cargo tree -p rustmark-core` 无 syntect/ureq/rfd/pulldown/image/egui；拓扑与期望文件一致；2131；size_gate。
- **回退**：revert。

---

## 3. 技术债登记簿（不阻塞主清单，需跟踪）

| ID | 债务 | 证据 | 处置 | 关联任务 |
|---|---|---|---|---|
| TD-01 | release 体积吃紧（已越旧红线，红线上调后入预警窗口） | 2026-10-24 基线 10,485,248 B（旧红线 10 MiB 余 512 B）；2026-10-25 V11-62 批次 2（state 迁入 `rustmark-ui`）致 +2,560 B → 10,487,808 B，旧 10 MiB 红线越线；批次 3（shortcuts）致 +1 KiB → 10,488,832 B；批次 4（theme）致 +1 KiB → 10,489,856 B；批次 5（rich，~5,918 行，V11-62 收官）致 -2,560 B → 10,487,296 B（非单调：rich 自绘排版零跨 crate 内联，迁移回吐批次 2 的 LTO 边界成本）；新红线 11 MiB 内、超预警 10 MiB。worktree A/B 实证 +2,560 B 确由跨 crate LTO 边界成本产生，无针对性修法。**维护者决策：NFR 红线 10→11 MiB、预警 8→10 MiB（原红线降级为预警）**，见 `scripts/size_gate.sh`、metrics.md §2.8 | V11-62 收官，体积窗口稳定（距新红线 11 MiB 余 1,047,040 B）；后续加依赖动作仍须过 size_gate | V11-62 / 所有 |
| TD-02 | `src/presentation/views/preview_view.rs` 1,421 行 | 上轮评估 | 600 行阈值入 check-arch；V11-62 搬迁窗口顺带拆 | V11-62 |
| TD-03 | `domain.rs:3` 依赖注释宽于实际 | 允许 ropey/chrono，实测零使用 | 注释校正 | V11-67 |
| TD-04 | shared 通用件 ≈900 行且可能继续长 | pattern 494 + lru + text | 阈值：合计 >1,500 行或第 4 个非领域通用模块 → 抽 crate | 监控 |
| TD-05 | D-6/D-7 六文件未提交合入 | git status | 先合入再动 arch_check 的任务 | BLK-03 |
| TD-06 | 双 manifest 重复 | egui/image/syntect/log/tempfile | workspace 收敛 | V11-65 |
| TD-07 | tests/presentation 43 文件将随 V11-62 拆分 | 622 条用例 | 按被测模块归属拆；保持总数 | V11-62 |
| TD-08 | metrics.md 体积记录与实测差 +18,432 B 未 bisect | 上轮发现 | 补记当前基线；差异bisect列为低优先 | V11-67 |

## 4. 阻塞项

| ID | 阻塞 | 影响 | 解除方式 |
|---|---|---|---|
| BLK-01 | 五项用户决策缺失（见 §5 Q-01~05） | V11-62/63/64 无法拍板是否启动 | 用户回答 §5 |
| BLK-02 | V11-62/63/64 触发条件未满足 | 提前启动 = 过早拆分，收益为负 | V11-66 度量数据达标后再启动 |
| BLK-03 | D-6/D-7 未合入，与 V11-57/58/62/63 同改 `tools/arch_check.rs` | 冲突/基线混乱 | 先合入 D-6/D-7，或明确同分支携带并记入 commit message |

## 5. 前置澄清问题（按优先级）

1. **Q-01 core 的复用意图**：是否计划 CLI / headless / 嵌入消费者？——决定 V11-63/64 从「低优先级触发式」升为「必做」，也决定「core 可复用」是否值得端口化成本。
2. **Q-02 icons/rich 是否计划跨项目复用**：——决定 V11-57/58 按「变体 A（actions+icons 两个契约 crate）」还是简化路径执行。
3. **Q-03 团队规模与并行计划**：——V11-62 触发条件①； Solo 开发则 ui crate 继续挂起。
4. **Q-04 mermaid/math 路线图**：还要继续加渲染器吗？——若是，增设 `rustmark-render` 任务（rich 的 5.6k 行独立）。
5. **Q-05 有没有具体「被 core 拖累」的案例**（编译等待/合并冲突/测试难写）：——一个真实案例比统计更能校准排序。
6. **Q-06 是否授权执行 Wave 0**（V11-56/65/66/67/59：纯文档 + 构建卫生 + 只读审计，零行为风险）：——确认后即可开工。

## 6. 附录：验收命令速查

```bash
cargo fmt --check
cargo run --bin check-arch
cargo clippy --workspace --all-targets --all-features -- -D warnings   # 0 告警
cargo test --all-targets        # 2131 passed / 0 failed / 4 ignored
                                 # （grep '^test result:' | awk 逐行求和）
bash scripts/size_gate.sh       # exe ≤ 10,485,760 B
cargo tree --workspace --depth 1   # 拓扑对 docs/workspace-topology.expected.txt
```

> 迁移类任务（V11-57/58/62/64）的额外纪律：git mv 保历史；每项独立 commit；
> 领域层 `domain/` 全程零 diff（对外行为不变的锚）；root 公开路径
> （`crate::presentation::<子模块>` 等）经桥接 re-export 全程保持。
