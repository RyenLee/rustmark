# rustmark Workspace 拆分技术评估报告（2026-10-24）

- **评估对象**：D-3 阶段 4a/4b「原单体 crate 按 DDD 拆分」的落地现状（root `rustmark` + 成员 `crates/rustmark-core`）
- **评估基线**：工作区当前状态（v0.2.0）。注意：评估时工作区有**未提交改动**（D-6 shared 登记制入校验器、D-7 scheduler debug 断言，涉及 `tools/arch_check.rs`、`src/app/frame/scheduler.rs`、`tests/arch_violation_fixture.rs`、两份文档，共 6 文件 +321 行），本报告结论已包含这批改动
- **评估方式**：只读静态分析（grep / 自写依赖矩阵脚本 / 文件统计）+ 实跑构建与测试计时（`cargo build` / `cargo test` / `cargo clippy` / `cargo run --bin check-arch` / `cargo build --release`）。**未修改任何源代码**；计时需要构建产物，使用了 `cargo clean` 与独立 target 目录（见 §3.3 说明）
- **环境**：Windows / x86_64-pc-windows-msvc / rustc 1.99.0 / 2026-10-24
- **前次评审**：`docs/architecture-review-2026-10-23.md`（层内问题清单 D-1~D-12）。本报告聚焦 **workspace 拆分本身**，不重复其层内结论，仅在交叉处引用

---

## 零、结论先行

| 判据 | 实测 | 判定 |
|---|---|:---:|
| crate 依赖方向（root → core 单边） | `cargo tree` 实测，core 零本地依赖，无环 | ✓ |
| 层间依赖矩阵（5 层 × 禁止方向） | 自写脚本统计 use 边 + `check-arch` 当日实跑：**禁止方向边数全部为 0** | ✓ |
| optional 依赖 / feature 开关 | 两份 Cargo.toml 均无 `[features]`、无 `optional`（grep 实测） | ✓ |
| 全量测试 | **2131 passed / 0 failed / 4 ignored**（17 个 `test result:` 行逐行求和） | ✓ |
| 冷构建 / 增量 / 测试 / clippy / release | 83s / 4.6–4.7s / 81s（含编译）/ 36s / 121s | 见 §3.3 |
| 「按 DDD 拆分为多个 crate」这一前提 | **仅部分成立**：DDD 分层是 core **内部模块级**分层；crate 级只有 2 个（root + core），限界上下文未独立成 crate | ⚠ |
| 综合技术债等级 | **低**（无 P0；2 项中等结构性关注点；若干低债务与必要拆分代价） | 见 §3.2 |

**核心判断**：这次拆分是一次**目标明确、执行完整、有机械守卫**的架构改造。它买到的最有价值的东西不是"编译快"，而是**把"表现层不得碰领域/基础设施"从文档规则变成了 cargo 结构事实**（core 物理上无法引用 root，反向依赖不可能存在）。遗留问题集中在：crate 粒度仍粗（core 一个 crate 装五层 46 375 行）、表现层被 crate 边界切成两半（views 在 root、state 在 core）导致 UI 功能天然跨 crate 改动、以及双清单并行带来的 manifest 级重复。

---

## 一、落地效果总结

### 1.1 事实基线（结构实测）

| 项 | 实测值 | 证据 |
|---|---|---|
| workspace 成员 | `members = ["crates/rustmark-core"]`（+ root 自身） | 根 `Cargo.toml:7` |
| crate 数 | 2（`rustmark` 0.2.0 / `rustmark-core` 0.2.0） | 根 `Cargo.toml:10-12`；core `Cargo.toml:1-4` |
| 源码规模 | 326 个 `.rs` / 67 501 行 | `find src crates -name '*.rs' \| xargs wc -l` |
| root 构成 | `app`（组合根，29 文件 / 7 027 行）+ `presentation::views`（50 文件 / 13 799 行）+ 2 个 re-export 拼接点 | `src/lib.rs:35-41`；`src/presentation.rs:3-7` |
| core 构成 | domain 47 文件 / 7 180 行；application 49 / 6 578；infrastructure 52 / 10 824；shared 19 / 2 975；presentation 76 / 18 818（state/theme/rich/widgets/icons/shortcuts，**无 views**） | core `src/lib.rs:25-29`；`find + wc` 实测 |
| 测试布局 | 11 个显式 `[[test]]` 目标（`autotests = false`）+ 130 个测试文件 + 432 个 src 内联 `#[test]` | 根 `Cargo.toml:171-230`；grep 实测 |
| 拼接点规模 | root 全部 326 个文件中**只有 2 个**引用 `rustmark_core`（`src/lib.rs`、`src/presentation.rs` 各 1 条 `pub use`）——存量 `crate::` 路径零改动迁移的证据 | grep `rustmark_core` in `src/` = 2 文件 / 2 行 |

### 1.2 核心收益（每条附证据与影响范围）

| # | 收益 | 证据（路径 / 符号 / 行号） | 影响范围 | 验证方式 |
|---|---|---|---|:---|
| B-1 | **反向依赖被 cargo 结构锁死**：core 不可能引用 `crate::app`，"下层反向依赖组合根"从"靠 review 发现"变成"编译上不可能" | core `Cargo.toml` 依赖表（`:10-68`）无任何本地依赖；`cargo tree -p rustmark-core` 无 root 条目 | 全部 326 个源文件 | `cargo tree` 实跑 |
| B-2 | **层间矩阵零违规且有机械执行**：domain→application/infrastructure/presentation、application→infrastructure/presentation、infrastructure→application/presentation、presentation→domain/infrastructure、shared→业务层，**禁止方向 use 边全部为 0** | 自写脚本统计 326 文件 use 边（§2.4 矩阵）；`check-arch` 当日实跑输出 `OK（未发现分层依赖违规）` | 全部 5 层 + 组合根 | 脚本 + 实跑 |
| B-3 | **视图零改动迁移**：2 条 `pub use` 拼回全部 `crate::<层>::` 路径，root 现存 `crate::domain::` 10 处 / `crate::application::` 46 处 / `crate::infrastructure::` 23 处 / `crate::shared::` 48 处 / `crate::presentation::` 161 处均走拼接点 | `src/lib.rs:41`；`src/presentation.rs:7` | root 全部 82 文件 | grep 计数 |
| B-4 | **core 内部实现对 root 编译不可见**：4b 可见性收紧后模块级 pub 项 1 664→1 383、字段 pub 606→547，548 条已批准 pub 字段由白名单双向断言钉死 | `tests/field_visibility_guard_test.rs:61-62`（`APPROVED_PUB_FIELDS`，注释 `total: 548`）；约束文档 §2.1 | core 46 375 行 | 读测试 + 文档 |
| B-5 | **测试金字塔随拆分显式化**：`tests/` 按层分目录（domain 10 / application 9 / infrastructure 16 / presentation 43 / shared 9 / e2e 1 / app 25 / integration 7 个用例文件），11 个 `[[test]]` 目标显式声明，"删入口文件 = 有意识地从构建移除该层" | 根 `Cargo.toml:171-230` 注释块 | 130 个测试文件 | 读 Cargo.toml |
| B-6 | **root 侧改动的编译隔离**：只碰 views/组合根时只重编 1 个 crate（实测 4.7s）；core 侧改动才波及 2 个 crate（4.6s） | §3.3 实测 | 开发者日常循环 | 实测 |
| B-7 | **校验器跟随 workspace 化**：三根扫描（shell src + core src + tests）、`rustmark_core::X` 归一为 `crate::X` 匹配同一禁令表（防改写前缀绕过）、`mod.rs` 禁令、帧预约唯一出口均不设豁免 | `tools/arch_check.rs`（`scan_project`）；约束文档 §4.1/§4.3/§7 | 全仓 | 读代码 + 实跑 |
| B-8 | **命令面纪律随拆分固化**：`--workspace` 必带写入约束文档 §6（不带则 core 单测静默缺席），CI 四处（clippy/check/test/llvm-cov）同口径 | 约束文档 §6；`.github/workflows/ci.yml` | 团队协作 | 读文档 + CI 配置 |

### 1.3 现存不足（每条附证据、等级与影响范围）

| # | 不足 | 证据 | 等级 | 影响范围 |
|---|---|---|:---:|---|
| S-1 | **"DDD 拆分为多个 crate"前提仅部分成立**：限界上下文（Document/Markdown/Workspace/Preferences/Export）是 core 内部模块，不是独立 crate；crate 级粒度只有 2 个，core 单 crate 承载五层 46 375 行 | `crates/rustmark-core/src/{domain,application,infrastructure,shared,presentation}.rs`；约束文档 §5 上下文表 vs §7 文件树 | 中 | 所有权划分、并行开发、core 内任何改动的重编范围 |
| S-2 | **表现层被 crate 边界切成两半**：views（root，50 文件/13 799 行）与 state/theme/rich/widgets/icons/shortcuts（core，76 文件/18 818 行）。新增一个 UI 功能要同时改两个 crate（视图 + 状态 + 应用服务），跨 crate 摩擦是结构性的 | `src/presentation/views/` vs `crates/rustmark-core/src/presentation/`；`src/presentation.rs:3-7` 注释自述"views 依赖组合根 app，只能留 root" | 中 | 全部 UI 功能迭代 |
| S-3 | **双 manifest 版本/feature/lint 重复**：无 `[workspace.dependencies]` 表——`egui 0.37.0`（根 `:71` / core `:18`）、`image 0.25`（根 `:93` / core `:34`）、`syntect 5.3.0` 及同一 4 项 feature 列表（根 dev `:155-160` / core `:24-29`）、`tempfile`（根 `:152` / core `:71`）各声明两份；4 条 lint 属性在 `src/lib.rs:26-27,33` 与 core `src/lib.rs:21-23` 各写一份（core 注释自述"两处必须保持一致"） | 两份 Cargo.toml + 两份 lib.rs | 低 | 依赖升级、lint 策略调整 |
| S-4 | **release 产物体积贴红线**：实测 exe 10 485 248 B，门禁上限 10 485 760 B（`scripts/size_gate.sh` `LIMIT`），**余量 512 B（0.005%）**；8 MiB 预警线早已越线（仅告警）。`docs/metrics.md` 记录的 10 466 816 B 与实测差 +18 432 B（工作区含未提交改动） | 本报告实跑 `cargo build --release --bin rustmark`；`scripts/size_gate.sh:24` | 中（风险） | 任何新增代码/依赖都可能触发门禁 |
| S-5 | **最大文件未拆且阈值无机械校验**：`preview_view.rs` 1 421 行（全仓最大），`toolbar_view.rs` 695 行；600 行拆分阈值 2026-10-24 才入约束文档 §7，且明确为"评审纪律、当前未机械校验" | `wc -l` 实测；约束文档 §7 末段 | 低-中 | 表现层可维护性（前次评审 D-4 未闭合项） |
| S-6 | **组合根内部残余反向导入**：`group_search_hits` 定义在 `src/app/frame/find.rs:203`，被 `src/app/frame/outline.rs:3` 反向 import（app 内纯函数互导）；另有三条并存的"当前文档路径"方法（返回类型各异） | grep 实测；前次评审 D-8 | 低 | 组合根 29 文件 / 7 027 行的局部导航 |
| S-7 | **`crate::app::frame` 下行通道有 38 个使用文件**（33 个 views + 5 个 app 内部）：这是获认可的窄豁免，但它是全仓最大的单一"层规则例外"；裸字段赋值已归零（state_borrow_guard 白名单为空双向断言），egui 控件 `&mut` 借用属文档化已知边界 | grep 实测 38 文件；`tests/presentation/state_borrow_guard_test.rs`（3 用例） | 低（受控） | 表现层 50 个视图文件 |
| S-8 | **shared 内核 19 文件 / 2 975 行**，含 494 行自制正则引擎（`shared/pattern.rs`）等偏"应用契约"的模块；准入三问 + 登记表刚入校验器（**未提交，在途**） | core `src/shared/`；约束文档 §4.5；`git diff tools/arch_check.rs`（`SHARED_REGISTRY_BEGIN` 新区） | 低（已设防） | 全部层均可依赖 shared |
| S-9 | **领域层携带渲染结果值对象**：`RenderedBlock` 含 `spans: Vec<InlineSpan>` 与 `source_range`，是"解析产物面向渲染"的形状（FR-02 滚动同步 / FR-03 增量高亮的性能依据，模块头有论证） | `crates/rustmark-core/src/domain/markdown/value_objects/render.rs:13-34` | 低（灰色地带） | domain/markdown 上下文 1 909 行 |
| S-10 | **应用层持有 IO 旁路表**：`DocumentAppService` 的编码记忆表（读盘时才能知道的信息），模块注释明确论证"不进聚合，防领域层被 IO 细节污染" | `crates/rustmark-core/src/application/document/service.rs:54-61` | 低（文档化取舍） | application/document |

---

## 二、DDD 专项评审

### 2.1 依赖关系图（crate 级）

```
                    ┌───────────────────────────────┐
                    │  rustmark (root, 0.2.0)       │
                    │  lib + 2 bins (rustmark/      │
                    │  check-arch)                  │
                    │  · app/            组合根      │
                    │  · presentation::views        │
                    └──────────────┬────────────────┘
                                   │ path 依赖（根 Cargo.toml:82）
                                   │ rustmark-core = { path = "crates/rustmark-core" }
                                   ▼
                    ┌───────────────────────────────┐
                    │  rustmark-core (0.2.0)        │
                    │  domain / application /       │
                    │  infrastructure / shared /    │
                    │  presentation(无 views)        │
                    └───────────────────────────────┘
                    （零本地依赖 → 反向依赖结构上不可能）
```

| 核查项 | 结论 | 证据 |
|---|---|---|
| 依赖方向 | 严格单向 root → core；core 无任何 path/workspace 依赖 | `cargo tree -p rustmark-core --depth 1` 输出 13 个外部 crate、零本地 crate |
| 可选依赖 | **无**。两份 manifest 无 `optional = true` | grep `optional` 双 Cargo.toml = 0 匹配 |
| feature 开关 | **无**。两份 manifest 均无 `[features]` 段——core 全量编译，不存在"按 feature 裁剪层"的可能 | grep `\[features\]` = 0 匹配 |
| 循环依赖 | **无**。crate 级 core→root 被 cargo 锁死；模块级环历史上有过（shared dialog↔text_format，2026-10-21 已断，现仅 `dialog → text_format` 单向） | 前次评审 §3.2；`cargo tree` |
| 外部依赖重叠 | egui/image/syntect/tempfile 双清单声明（S-3），版本字符串一致但无单点真源 | 两份 Cargo.toml |

### 2.2 限界上下文逐一体检

| 上下文 | 类型（约束文档 §5） | 聚合根 | 实测规模 | 分层角色 | 判定 |
|---|---|---|---|---|---|
| Document | 核心域 | `Document` | domain 2 196 行（aggregate 489 / services/ / undo / value_objects / events / repository / errors） | domain + application/document + infrastructure/persistence | ✓ 清晰 |
| Markdown | 核心域 | `RenderedDocument`（值对象聚合） | domain 1 909 行（aggregate / value_objects{block,inline,outline,render,table} / lint/rules） | domain + application/markdown + infrastructure/parsing+highlighting | ✓ 清晰（S-9 灰色地带） |
| Preferences | 通用域 | `Preferences` | domain 1 855 行（value_objects{action,editor,keys,shortcut_table,theme}） | domain + application/preferences + infrastructure/toml_preferences_repository | ✓ 清晰 |
| Workspace | 支撑域 | `Workspace` | domain 689 行 | domain + application/workspace + infrastructure/fs_workspace_repository | ✓ 清晰 |
| Export | 支撑域 | `ExportJob` | domain 531 行 | domain + application/export + infrastructure/export（html/pdf/viewport） | ✓ 清晰 |
| （横切）infrastructure | — | — | 10 824 行 / 52 文件，按**技术能力**组织（persistence/parsing/highlighting/export/fonts/spell/system/text/events/dialogs），不按上下文 | 适配器层 | ✓ 符合 DDD 惯例（端口-适配器） |
| （横切）shared | — | — | 19 文件 / 2 975 行，三问登记制（在途） | 共享内核 | ✓ 有准入守卫 |
| （横切）app | — | — | 29 文件 / 7 027 行，`frame.rs` 778 行 | 组合根 | ✓ 职责单一（S-6 残余） |

**结论**：五个上下文全部对应"aggregate + value_objects + events + services + repository + errors"的标准六件套形状，端口 trait 在 domain（`domain/document/repository.rs:131` 行只定义契约、零 IO），实现在 infrastructure（`fs_document_repository.rs:437` 行），应用层是薄编排（`DocumentAppService` facade 125 行 + 7 个私有子模块挂载 inherent impl）。**上下文与分层角色的对应关系清晰，未发现边界错配到需要重构的程度。**

### 2.3 边界错配 / 领域模型泄露 / 实现重复

| 类别 | 发现 | 证据 | 判定 |
|---|---|---|---|
| 领域模型泄露 | ① `RenderedBlock.spans` 渲染形状入 domain（S-9）；② application 持编码旁路表（S-10）；③ `shared` 含 `text_format`/`fonts`/`theme` 等偏应用契约模块（S-8，已设登记制守卫） | §1.3 各条 | 均有文档化论证，非失控泄露；①② 是有依据的取舍 |
| 实现重复 | 代码级：**抽样未见跨 crate 重复实现**——root 的 `app/frame/export.rs`、`preview.rs` 等是薄写入口（文件头明确"刻意不复刻 ExportUiState 已有行为"）；span→LayoutJob 已收敛为唯一构造器（前次评审 C-1/R-10）。manifest 级：版本/feature/lint 三处双写（S-3） | `src/app/frame/export.rs:1-23`；`src/app/frame/preview.rs:1-20` | 代码级重复低；manifest 级重复是真实的小额债务 |
| 边界错配 | 表现层跨 crate 分裂（S-2）是最大的结构性错配：`presentation::views` 因依赖组合根而必须留 root，与 core 的 presentation 六子模块被人为切开。代价是 UI 功能必跨 crate；收益是 views 能拿 `&EditorApp` | `src/presentation.rs:3-7` | 有明确理由，属"必要拆分代价"与"真实摩擦"的混合体 |

### 2.4 内聚度与耦合度（层间 use 边矩阵，实测）

统计口径：326 个 `.rs` 的顶层 `use` 语句（单行口径，多行 use 组与 `super::` 相对导入不计，故为**下界**）；"自由位置"（lib.rs/main.rs/check-arch.rs）单列。

| 从＼到 | shared | domain | application | infrastructure | presentation | app |
|---|---:|---:|---:|---:|---:|---:|
| shared | 10 | **0** | **0** | **0** | **0** | **0** |
| domain | 54 | 2（上下文内） | **0** | **0** | **0** | **0** |
| application | 90 | 34 | 26（上下文内） | **0** | **0** | **0** |
| infrastructure | 33 | 33 | **0** | 6（适配器间） | **0** | **0** |
| presentation（core+root 合并计） | 23 | **0** | 28 | **0** | 100（层内） | 32（仅 `app::frame` 窄豁免） |
| app（组合根） | 22 | 8 | 12 | 13 | 23 | 5（内部） |

- **禁止方向全部为 0**（加粗列），与 `check-arch` 当日实跑 `OK` 互相独立地印证。
- **耦合形态健康**：耦合集中在 shared（被 4 层引用，是枢纽但有登记制防膨胀）与 domain（被 application/infrastructure 依赖，方向单向）。
- **内聚度**：层内按上下文/能力二次切分，跨上下文 import 仅 domain/preferences→domain/document（`UndoPolicy` 共享，属 Published Language 模式，约束文档 §5 有登记）。
- **crate 耦合面**：root→core 一条边；core 对外暴露面经 4b 收紧后由 548 条白名单钉死。

---

## 三、技术债判定

### 3.1 四维度评分

| 维度 | 分（10 制） | 代码依据 |
|---|---|:---:|
| 架构合理性 | **9.0** | 禁止方向 use 边 0（§2.4 矩阵）；check-arch 当日 OK；domain `#![deny(unwrap/panic/print)]`（`crates/rustmark-core/src/domain.rs:19-29`）；扣分项：S-1/S-2 两条结构性灰色（crate 粒度粗 + 表现层跨 crate） |
| 长期可维护性 | **8.5** | 2 131 测试 / 4 ignored / 0 failed；94 条架构注入用例（`tests/arch_violation_fixture.rs`）；548 字段白名单 + 3 注入用例；state_borrow 白名单为空 + 2 注入用例；12 篇 ADR；扣分项：S-5（1 421 行最大文件、阈值无机械校验）、S-8（shared 增长，守卫在途） |
| 团队协作效率 | **6.5** | 并行开发：core 是单一争点——任何 core 改动重编全部自有 crate（实测 core-touch = 2 crate）；所有权只能按"core vs shell"二分，无法按上下文认领；发布粒度：两 crate 均 `publish = false` 且同版本 0.2.0，拆分不带来独立发布能力（当前单人维护，此项按"若团队扩大"的场景评估） |
| 后续迭代成本 | **7.5** | 新上下文 = core 内新模块（廉价，六件套模板成熟）；新 UI 功能 = 2 crate 改动（S-2）；新依赖 = 2 份 manifest + 体积预算（S-3/S-4）；扩展点有编译期护栏（穷举 match、`ActionRoute` 无 `_` 兜底，前次评审 §4.1） |

### 3.2 综合等级：**低**

- **无 P0/阻断级债务**：门禁全绿（fmt/clippy 0 告警/check-arch OK/2 131 测试 0 失败）、零 TODO/FIXME（前次评审实测 0，本轮未复核全部但抽样一致）、零层间违规。
- **两项中等结构性关注点**（不构成"高"，因其有明确理由与缓解）：S-1 crate 粒度、S-2 表现层跨 crate。前次评审 D-11 已给出再评估触发条件（行数逼近 10 万或团队 >1 人），本报告 §4.2 将其量化。
- **必要的拆分代价**（不应记为债务，但应有意识地管理）：
  1. 2 条 re-export 拼接点（`src/lib.rs:41`、`src/presentation.rs:7`）——换来 21 000+ 行 views/app 代码零改动；
  2. 双清单并行（版本/feature/lint 三处，S-3）——换来 core 不依赖 root 的结构事实；
  3. 校验器三根扫描 + `rustmark_core::` 前缀归一（`tools/arch_check.rs`）——换来"改前缀绕不过禁令"；
  4. `--workspace` 命令面纪律——换来的是一次没漏的 core 测试出席；
  5. UI 功能跨 crate 改动（S-2）——换来 views 可持 `&EditorApp` 的 Immediate Mode 形态。
- **真实技术债**（可执行清偿，见 §4.1）：S-3 清单无单点真源、S-5 大文件阈值未机械校验、S-6 组合根反向导入、S-4 体积余量 512 B（风险项）、S-1/S-2 的触发条件未量化（本报告补上）。

### 3.3 可量化指标（本会话实测 + 待测量）

| 指标 | 实测值（2026-10-24，Windows/MSVC） | 口径说明 |
|---|---|---|
| 全量冷构建（debug，`cargo build --workspace`） | **83 s** / 190 个 crate / 3.0 GiB 产物 | `cargo clean` 后计时；含全部外部依赖 |
| 冷 clippy（`--workspace --all-targets`，0 告警） | **36 s** | clippy = check 口径无 codegen，可作 IDE 冷检查代理 |
| 无操作增量构建 | **2.5–3 s** | 热缓存下 `cargo build --workspace` |
| 增量：改 core 文件（`domain/document/aggregate.rs`） | **4.6 s**，波及 **2/2 crate**（rustmark-core + rustmark 全重编） | `touch` 后计时；外部依赖不重编 |
| 增量：改 root 视图文件（`presentation/views/main_view.rs`） | **4.7 s**，波及 **1/2 crate**（仅 rustmark） | 同上 |
| 单次跨模块改动平均波及 crate 数 | root 侧 1；core 侧 2；按文件数加权（root 82 / core 244）**≈1.75**（推断，等权假设） | 见左；crate 级拆分只隔离 root 侧改动 |
| 全量测试（`cargo test --workspace`，热依赖） | **81 s**（编译 ≈40 s + 执行 ≈41 s）；**2 131 passed / 0 failed / 4 ignored** | 17 个 `test result:` 行逐行求和；cargo 顺序执行各测试二进制，执行时间为各行 `finished in` 累加 |
| release 构建（`cargo build --release --bin rustmark`，LTO+z+cgu=1） | **121 s**；exe **10 485 248 B（9.998 MiB）** | 门禁上限 10 485 760 B → 余量 **512 B** |
| IDE 体验损耗 | **部分待测量**：冷检查代理 36 s（clippy 口径）；rust-analyzer 首次索引/检查耗时、内存占用、跳转延迟**待测量**——获取方式：VS Code 命令面板执行 `rust-analyzer: Status` 查看索引与检查耗时；或 `cargo check --workspace --all-targets` 计时（冷 ≈36 s，热 ≈3 s）。可测事实：2 crate / 326 文件 / 67 501 行 / 最大单文件 1 421 行 | — |
| 测试执行时长分布 | 最重三个目标：621 例 / 13.8 s、356 例 / 3.7 s、300 例 / 8.5 s | 由 test result 行直接得出 |

> 说明：为取得冷构建数字，本会话执行了 2 次 `cargo clean`（用户构建缓存被重建；属 `cargo clean` 原生操作，未用任何直接删除）。测量用的独立 target 目录 `target-eval/`、`target-eval-b/` 已/将以 `cargo clean --target-dir` 清理。**所有计时均在未提交改动（D-6/D-7 在途）存在的工作区上取得。**

---

## 四、优化与风控建议

### 4.1 改进项（P0/P1/P2）

**P0（阻断级）：无。** 门禁全绿、零层间违规、测试 0 失败——没有需要立即处理的结构性问题。若把"防体积越线"视为发布阻断项，见 P1-3。

| # | 等级 | 改进项 | 目标 | 改动范围 | 预期收益 | 回退风险 |
|---|---|---|---|---|---|---|
| P1-1 | P1 | **引入 `[workspace.dependencies]` 单点真源** | 消除 S-3 的版本/feature 双写（egui/image/syntect/tempfile） | 2 个 Cargo.toml（root 改 `workspace.dependencies` + `dep.workspace = true`；core 同） | egui 0.37→0.38 升级单点修改；杜绝"syntect feature 两侧漂移导致测试与产品不同源"（根 dev-deps 注释自述的风险） | 低：path 依赖关系不变，`Cargo.lock` 重解析；回退 = 还原两份清单 |
| P1-2 | P1 | **`preview_view.rs`（1 421 行）按既有先例拆分**，并把 600 行阈值并入 `check-arch` 文本扫描（不设豁免） | 消除最大维护盲区（S-5），把"评审纪律"升级为机械门禁 | 1 文件 → `preview_view.rs` 入口 + `preview_view/{media,overlay,toc}.rs`（子目录已有 3 000+ 行先例）；`tools/arch_check.rs` 加一条文件名规则 | 同目录 `sidebar_view/`、`settings_view/` 拆后评审定位成本更低的经验复现；阈值机械化后新增大文件必红 | 低：纯文件移动 + use 调整；风险在渲染路径回归，由现有 43 个 presentation 测试文件兜底 |
| P1-3 | P1 | **体积红线余量预警前移**：`scripts/size_gate.sh` 增加"距红线 <2% 即 warn 升级"或将强预警线提到 9.5 MiB | 当前余量 512 B（0.005%），任何新增代码/依赖即越线（S-4） | `scripts/size_gate.sh` + `ci.yml` 摘要输出 | 越线从"CI 红"变成"提前可见的渐进告警"；配合 `docs/metrics.md` 唯一数值真源纪律 | 低：只动告警阈值不动判定红线 |
| P2-1 | P2 | **D-8 收尾**：`group_search_hits` 下沉 core（`presentation/state/find_replace_ui_state` 或 workspace 应用服务）；三条"当前文档路径"方法收敛为一个 `Option<FilePath>` 返回 | 消除组合根内反向导入（S-6） | `src/app/frame/find.rs`、`outline.rs` + core 侧落点 | 组合根 7 027 行的局部导航成本下降 | 低-中：涉及 outline 调用点，需回归大纲/搜索测试 |
| P2-2 | P2 | **`[workspace.lints]` 收敛 4 条 lint 属性**（unwrap/expect/panic/missing_docs，根 `lib.rs:26-27,33` 与 core `lib.rs:21-23`） | 消除"两处必须保持一致"的隐性耦合（S-3 的 lint 部分） | 2 个 Cargo.toml + 2 个 lib.rs | lint 策略单点修改；新 crate 自动继承 | 低：cargo ≥1.51 支持；回退 = 还原属性行 |
| P2-3 | P2 | **合并 D-6/D-7 在途改动并发布基线**（shared 登记制、scheduler debug 断言当前未提交） | 工作区回到干净基线，避免评估结论与代码状态漂移 | 6 个已修改文件（+321 行） | 守卫生效；`git status` 干净 | 低：改动已在 CI 口径验证过（check-arch OK、clippy 0 告警均含这批改动实测通过） |

### 4.2 守卫机制建议

**（a）依赖方向校验（已有，建议保持 + 补一处）**
- 现状：`check-arch`（token 级、三根扫描、`rustmark_core::` 归一、豁免登记表零项、shared 登记制在途）+ `build.rs` 编译期 panic + CI。**当日实跑 OK，建议原样保持。**
- 补充：在 CI 加一条 `cargo tree --workspace --depth 1` 输出快照断言（或写入 Job Summary），把"本地 crate 依赖图只有 root→core 一条边"变成可见事实——防止未来引入 optional path 依赖或第三 crate 时静默改变拓扑。

**（b）分层测试策略（已有，建议补一条口径）**
- 现状：11 个显式 `[[test]]` 目标、目录即分层、`tests/` 禁库内部视角（R-02，编译期强制）、领域覆盖率 ≥90% 门禁、2 131 测试。
- 补充：把本报告的 §2.4 矩阵脚本固化为 CI 一步（或并入 `check-arch` 的只读报告模式）——"禁止方向 use 边计数 = 0"作为可回归的数字，而不只是"校验器没报错"（零违规证明不了校验器在工作，必须配注入用例——本项目已有 94 条 fixture，纪律正确）。

**（c）crate 合并或再拆分的触发条件（量化，补 D-11 的定性表述）**

| 方向 | 触发条件（任一满足即启动评估） | 建议动作 |
|---|---|---|
| **再拆分**（core → 多 crate） | ① 总行数 ≥ 100 k（当前 67.5 k）；② 团队 ≥ 2 人并行开发且 core 成 merge 冲突热点（可观测指标：core 目录占月度改动文件数 >60%）；③ 单上下文（Document 先行：domain 2 196 + application/document + infrastructure/persistence）需要独立所有权或独立 CI 子集；④ core 单 crate 增量重编 >15 s（当前 4.6 s） | 按 Document 上下文拆 `rustmark-document`（application 层随上下文拆或保持 facade crate）；循环风险由 cargo 锁死，`cargo tree -i` 断言防回归 |
| **合并**（core 表现层 → root 或反之） | ① `arch-allow` 豁免表因表现层边界增长 >5 条；② core 内出现 shared↔某层环；③ views 与 state 的"一对功能跨两 crate"成为迭代主要摩擦（可观测：单功能 PR 平均触碰 crate 数 >1.5 且 views/state 成对出现） | 评估把 `presentation::views` 整体移入 core、组合根 `app` 单独成 crate（即表现层完整入 core，root 只剩组合根 + bins） |
| **不动作** | 行数 <100 k、单人维护、UI 功能跨 crate 摩擦可忍受 | 维持现状，按 P1/P2 清单清偿小额债务 |

---

## 五、事实与推断清单

**已验证事实**（本会话直接运行/读取）：
1. 依赖拓扑：root→core 单边、零 optional、零 feature、零环（`cargo tree` + grep 双 Cargo.toml）。
2. 层间矩阵禁止方向 use 边全 0（自写脚本，326 文件）+ `check-arch` 当日实跑 OK（含未提交改动）。
3. root 仅 2 文件引用 `rustmark_core`（re-export 拼接点）；拼回路径使用量 presentation 161 / shared 48 / application 46 / infrastructure 23 / domain 10。
4. 规模数字：67 501 行 / 326 文件 / 分层与上下文行数（§1.1、§2.2）。
5. 全部计时与测试数字（§3.3）：冷构建 83 s、clippy 36 s、增量 4.6/4.7 s、测试 81 s（2 131/0/4）、release 121 s 与 exe 10 485 248 B。
6. 守卫资产存在性与规模：field guard 548 条白名单、arch fixture 94 用例、state_borrow 白名单为空。
7. `crate::app::frame` 通道 38 个使用文件；`group_search_hits` 跨文件反向导入（D-8 现存）。
8. 工作区评估时点有 6 文件未提交改动（D-6/D-7 在途）。

**推断**（标注了推断性质）：
1. "按文件数加权的平均波及 crate 数 ≈1.75"——等权假设，非改动频率加权。
2. "clippy 36 s 可作 IDE 冷检查代理"——口径近似（clippy = check + lints，无 codegen），rust-analyzer 实测待测量。
3. S-4 中 `docs/metrics.md` 记录值（10 466 816 B）与本次实测（10 485 248 B）差异归因于未提交改动——**未逐 commit  bisect，属推断**。
4. §2.3"代码级重复低"为抽样判定（root frame 写入口 + span 构造器收敛点），未做全量重复检测。
5. 团队协作维度评分基于"若团队扩大"的情景推演——当前为单人维护项目。

---

## 附：关键复现命令

```bash
# 结构事实
cargo tree -p rustmark-core --depth 1     # core 零本地依赖
cargo metadata --no-deps --format-version 1 | head   # workspace 成员
grep -rn 'rustmark_core' src/ --include='*.rs'        # 仅 2 处（拼接点）

# 守卫
cargo run --bin check-arch                # 当日实跑 OK
cargo clippy --workspace --all-targets    # 0 告警

# 指标
cargo clean && time cargo build --workspace           # 冷构建 ~83 s
touch crates/rustmark-core/src/domain/document/aggregate.rs && time cargo build --workspace   # core 增量 ~4.6 s
time cargo test --workspace                            # ~81 s；grep '^test result:' | awk 求和
time cargo build --release --bin rustmark              # ~121 s；exe 10 485 248 B
bash scripts/size_gate.sh                              # 红线 10 485 760 B，余量 512 B
```
