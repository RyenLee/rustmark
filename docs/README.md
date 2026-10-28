# docs/ 文档地图

> 最近整理：**2026-10-28**（第二批归档 + 索引重建：14 篇收官文档与 9 张零引用截图入库，整改文档按阶段建档）。
> 本页是 `docs/` 的**唯一导航入口**。分区规则只有一条：
> **「现行真源」是活的，要改就改它；「决策与证据」是只读的，改不了也不该改；「归档」是一次性报告，只查不维护。**

## 怎么用这份地图

| 你是谁 | 先读 |
|---|---|
| 新接手的开发者 | 本页 → [`PROGRESS.md`](./PROGRESS.md)（做到哪了）→ [`debug-guide.md`](./debug-guide.md)（跑起来 + 排障）→ [`architecture-constraints.md`](./architecture-constraints.md)（不许做什么） |
| 领任务开工 | [`v1.1-backlog.md`](./v1.1-backlog.md)（**唯一任务入口**）→ 对应阶段文档 → `.agent/conventions.md` |
| 要改架构 / 加 crate | [`architecture-constraints.md`](./architecture-constraints.md) §10（crate 地图与归属四问）→ [`remediation/`](./remediation/README.md)（为什么是现在这个样子） |
| 用户 / 报缺陷 | [`troubleshooting.md`](./troubleshooting.md)（配置目录 / 草稿恢复 / 崩溃报告） |
| 准备发布 | [`release.md`](./release.md) → [`release-compliance-gap-report.md`](./release-compliance-gap-report.md)（还剩什么没闭环）→ [`testing.md`](./testing.md) |
| 关心体积 / 性能数字 | [`metrics.md`](./metrics.md)（**唯一数字来源**）→ [`dependencies.md`](./dependencies.md) |

---

## 1. 现行真源（唯一的「家」，其余位置只引用不复写）

> 这五份是**机器或流程直接读取**的判据，移动/改名会让门禁变红。改内容必须同步跑门禁。

| 文档 | 定位 | 被谁读 |
|---|---|---|
| [`architecture-constraints.md`](./architecture-constraints.md) | **架构规则唯一真源**：分层 / 依赖矩阵 / 各层禁令 / `shared` 准入三问 + 登记表 / §10 crate 地图与模块归属四问 | `cargo run --bin check-arch`（按区段标记解析「豁免登记表」「`shared` 准入表」，未登记即违规） |
| [`metrics.md`](./metrics.md) | **七项硬指标唯一数字来源**（体积 / 启动 / 内存 / 帧耗时 / 解析 / 高亮 / 搜索） | 人工 + CI 注释口径；`size_gate.sh` 的默认值与 §2.8 同源 |
| [`thresholds.baseline.yaml`](./thresholds.baseline.yaml) | 121 条阈值的取值 / 单位 / 源码行号基线（运行时**不加载**） | `python3 tools/check_thresholds.py`（CI `Threshold baseline` 步骤） |
| [`workspace-topology.expected.txt`](./workspace-topology.expected.txt) | 12 条本地依赖边的期望快照（含手写注释头） | `tests/workspace_topology_test.rs` + CI diff + `scripts/gen_dep_graph.sh` 回写 |
| [`PROGRESS.md`](./PROGRESS.md) | 九阶段实施计划索引 + 当前进度 + v1.1 状态摘要 | 全体；阶段文档的历史快照与此冲突时**以本文 §1.2 为准** |
| [`v1.1-backlog.md`](./v1.1-backlog.md) | **v1.1 唯一任务入口**（V11-01~67 总览 + 逐项说明 + 完成记录） | 维护者；任务状态只在这里登记 |

## 2. 操作手册（面向具体动作）

| 文档 | 定位 | 什么时候读 |
|---|---|---|
| [`debug-guide.md`](./debug-guide.md) | 运行 / 调试命令手册 + **GUI 缺陷定位方法论**（§6） | 跑程序、写测试、**排障前** |
| [`troubleshooting.md`](./troubleshooting.md) | 终端用户故障排查（配置目录 / 重置 / 草稿恢复 / 崩溃报告） | 应用出问题、要提 issue |
| [`config-reference.md`](./config-reference.md) | **配置面参考**：`preferences.toml` 全字段（类型 / 默认 / 约束）+ 自定义主题色板 + 快捷键语法与默认键位 | 手改配置、写主题、查字段含义 |
| [`testing.md`](./testing.md) | 测试金字塔 / 覆盖率门禁 / 排除清单与理由 | 写测试、跑 CI 等价命令 |
| [`dependencies.md`](./dependencies.md) | 依赖清单、锁定版本、许可证、复查报告（§6） | 增删依赖、评审体积 |
| [`release.md`](./release.md) | 发布流程（tag 驱动、CI 全自动、回滚三步） | 发版前 |
| [`release-compliance-gap-report.md`](./release-compliance-gap-report.md) | **发布阻断项判定依据** + 未闭环待办（§6：三平台产物 / 真机回归 / 证书） | 发版前查未闭环合规项 |
| [`manual-qa-windows.md`](./manual-qa-windows.md) / [`manual-qa-macos.md`](./manual-qa-macos.md) / [`manual-qa-linux.md`](./manual-qa-linux.md) | 三平台手工回归清单（V11-02 **待执行**） | 真机回归、验收 GUI 行为 |
| [`cache-design.md`](./cache-design.md) | 缓存子系统设计（`revision` 语义失效 / 防抖节拍 / LruMap 收敛） | 改缓存、查淘汰策略 |

## 3. 决策与证据（**只读**：记录「当时为什么这么定」，不回改）

### 3.1 决策记录 [`adr/`](./adr/)（13 条，不可改写）

| ADR | 决策 |
|---|---|
| [ADR-001](./adr/ADR-001-ui-framework.md) | UI 框架：egui + eframe（wgpu 后端），不采用 Tauri（正文记 0.33.3，**现行版本见 `Cargo.toml`**） |
| [ADR-002](./adr/ADR-002-parser-highlighter.md) | 解析与高亮：pulldown-cmark + syntect（feature 收窄） |
| [ADR-003](./adr/ADR-003-persistence-export.md) | 持久化与导出：ropey trait 隔离、原子写 |
| [ADR-004](./adr/ADR-004-pdf-export.md) | PDF 导出：不内嵌引擎 → 系统打印 → **无头 Chromium**（修订） |
| [ADR-005](./adr/ADR-005-prototype-relationship.md) | 原型关系：行为契约来源，非代码来源 |
| [ADR-006](./adr/ADR-006-memory-threshold-revision.md) | 内存阈值修订：按平台各自口径（Windows Private WS ≤150 MB） |
| [ADR-007](./adr/ADR-007-rich-renderer-boundary.md) | `presentation/rich/` 边界：零 egui 依赖，与 `views` / `state` / `theme` 隔离 |
| [ADR-008](./adr/ADR-008-widgets-layer-boundary.md) | `presentation/widgets/` 收口标准（≥2 处重复 + 写错不报错 + 直觉写法看起来合理） |
| [ADR-009](./adr/ADR-009-frame-scheduler.md) | 帧循环定时任务统一调度：登记表 + 唯一预约出口 |
| [ADR-010](./adr/ADR-010-presentation-internal-direction.md) | `presentation` 内部依赖单向化 + `tests/` 禁库内部视角 |
| [ADR-011](./adr/ADR-011-action-routing-and-dual-intent.md) | 动作路由：穷举路由表 + `Unbound` 显式声明（无 `_` 兜底）；跳转双意图 |
| [ADR-012](./adr/ADR-012-icon-system-and-resource-layout.md) | 图标系统：向量手绘废止字形 + 规格即数据（16×16）；字体外置随包分发 |
| [ADR-013](./adr/ADR-013-core-adapter-residency-and-leaf-extraction.md) | core 适配层驻留与叶层外移：模块归属四问 + 外移触发条件量化表 |

### 3.2 整改执行期档案 [`remediation/`](./remediation/README.md)（3 篇，按批次分文件夹）

| 路径 | 批次 | 内容 | 状态 |
|---|---|---|---|
| [`r01-r11/remediation-plan-2026-10-27.md`](./remediation/r01-r11/remediation-plan-2026-10-27.md) | R-01~R-11 | 架构整改细化实施方案（问题画像 A~I → 目标 O1~O7 → 逐条改造项 + 落地记录） | ✅ 全量收官 |
| [`r10/evaluation.md`](./remediation/r10/evaluation.md) | R-10 | 拆库 vs 端口升 crate；结论「**强最大解不可达**」 | ✅ 项 A 落地 `0f20bc8` |
| [`r10/max-solution.md`](./remediation/r10/max-solution.md) | R-10 | 最大解 M0~M3 的设计与不可达论证（M1 成环禁 / M2 伪收敛） | ✅ 项 A + 项 B 落地 |

> 这三篇为什么**不归档**：总方案是全仓唯一记载「每条改造具体怎么改、偏差为何」的文档；
> R-10 两篇承载**否定性结论**——它不会变成代码也不会变成规则，丢了会有人重走一遍。
> 同类的 R-04 / R-07 方案已随批次收官归档，见 [`archive/remediation-plans/`](./archive/remediation-plans/)。

### 3.3 阈值治理证据（2 篇）

| 文档 | 内容 | 为什么留 |
|---|---|---|
| [`threshold-audit-2026-10-25.md`](./threshold-audit-2026-10-25.md) | 115 项阈值的全量审计（取值 / 依据 / 判定） | `thresholds.baseline.yaml:8` 的 `audit_report:` 指向本篇 |
| [`threshold-review-2026-10-25.md`](./threshold-review-2026-10-25.md) | 审计的裁定结论（6 个语义簇 / 3 项阻断 / P0-P1 落地记录） | 同上 `review_report:`；121 条阈值的取值依据 |

### 3.4 阶段技术文档（01–11）

| 文档 | 内容 | 状态 |
|---|---|---|
| `01-需求基线与技术决策.md` | 需求基线、ADR 决策记录、指标口径 | 阶段快照 |
| `02-工程骨架与分层边界.md` | 模块系统、分层约束、`check-arch` | 阶段快照 |
| `03-领域模型实现.md` | 5 个限界上下文的领域层落地 | 阶段快照 |
| `04-应用服务与用例编排.md` | 5 个 AppService、事件订阅链 | 阶段快照 |
| `05-基础设施实现.md` | ropey / pulldown / syntect 适配、fs 仓储、导出器 | 阶段快照 |
| `06-表现层与交互实现.md` | egui 视图、快捷键单一数据源、主题（阶段文档中最大单篇） | **仍被 `widgets.rs` 引用**（五条纪律条目出处） |
| `07-集成联调与原型对齐.md` | 集成联调、原型契约对齐、手工回归 | 阶段快照 |
| `08-性能优化与质量加固.md` | 体积 / 启动 / 内存优化过程 | 阶段快照 |
| `09-测试体系打包与发布.md` | 测试金字塔、覆盖率门禁、三平台打包 | 阶段快照 |
| `10-编辑区技术实现.md` | 编辑区专项（行高 / 高亮 / IME / 行号栏） | **仍指导改代码** |
| `11-预览区技术实现.md` | 预览区专项（渲染管线 / 滚动同步 / 图表） | **仍指导改代码** |

> 01–09 的「任务清单 / 验收记录」是各阶段收尾时的快照。**当前状态一律以 `PROGRESS.md` §1.2 与
> `v1.1-backlog.md` 为准**（历史快照与新结论冲突时，新结论胜）。

### 3.5 依赖图与拓扑快照（1 图 + 1 文本基线）

| 路径 | 内容 | 维护方式 |
|---|---|---|
| [`current_dependency_graph.html`](./current_dependency_graph.html) | **9 crate / 12 条本地依赖边**的实测依赖图（SVG，含分层布局与每 crate 规模） | `scripts/gen_dep_graph.sh` 只重写 `workspace-topology.expected.txt` 与 `target/` 下的机器图，**不覆盖本文**——本图由人工维护。2026-10-28 依实测重绘 |
| [`workspace-topology.expected.txt`](./workspace-topology.expected.txt) | 12 条本地依赖边的机器基线（含手写注释头） | `scripts/gen_dep_graph.sh` 重写主体、保留注释头；与 `tests/workspace_topology_test.rs`、CI diff 对账 |

> 原 `target_dependency_graph.html`（2026-10-27 目标态）已归档：
> [archive/architecture-reviews/target-dependency-graph-2026-10-27.html](./archive/architecture-reviews/target-dependency-graph-2026-10-27.html)。
> 它预测的 5 项中 2 项达成（icons 解耦 / render 抽出）、**2 项经决议不做**
> （`shared` 拆 kernel+support 两 crate、`views` 迁入 ui，见 ADR-013 决策 3）、
> 且未预见 `rustmark-ports` 独立成 crate。目标态与现状已合一，故合并为单图。

### 3.6 流水与素材（`logs/` 与 `pic/`）

| 路径 | 内容 |
|---|---|
| [`logs/问题记录1.txt`](./logs/问题记录1.txt) | 用户反馈与缺陷修复的**原始流水**（001~069，**倒序** append-only）——调试经验已提炼至 `debug-guide.md` §6，追根溯源查这里 |
| `logs/示例文档.md` | 应用内置欢迎页 / 测试用 Markdown 样本 |
| `pic/` | 文档引用的截图（问题记录按编号引用）；无引用的已随第二批归档移入 `archive/pic/` |

---

## 4. 归档 [`archive/`](./archive/README.md)（一次性报告，只查不维护）

两批归档，逐篇索引与「现状」见其 README：

| 批次 | 目录 | 收录 |
|---|---|---|
| 2026-10-14（第一批） | `stage-reports/` `audits-2026-10-12/` `crash-reports/` + 3 篇 10-17 报告 | 阶段收尾报告 / 一次性修复方案 / 审计快照 / 崩溃转储 |
| **2026-10-28（第二批）** | **`architecture-reviews/`**（9 篇 + 1 图）**`incident-reports/`**（2）**`remediation-plans/`**（2）**`tooling/`**（2）**`pic/`**（+9 张） | R-01~R-11 整改收官后的评审 / 方案 / 复盘 / 工具治理记录 |

---

## 扩展 crate 的实操清单（贡献者）

> 拆出的 `rustmark-actions` / `rustmark-icons` / `rustmark-ui` / `rustmark-render` / `rustmark-ports`
> 各有契约，扩展前先读对应 ADR（[ADR-011](./adr/ADR-011-action-routing-and-dual-intent.md) /
> [ADR-012](./adr/ADR-012-icon-system-and-resource-layout.md) /
> [ADR-013](./adr/ADR-013-core-adapter-residency-and-leaf-extraction.md)）。

- **新增可持久化动作（`rustmark-actions`）**：必须在 `UiAction` 与领域 `ActionId` **两侧同步加**变体；
  快捷键若要持久化，必须显式声明 `persisted_key`，**不得用 `_` 兜底**
  （`rustmark-actions/src/action/impls.rs` 要求两侧 `ActionId::all()` 与本表逐项比对，缺一项即构建失败）。
- **新增图标（`rustmark-icons`）**：规格即数据——按 16×16 栅格手绘向量，草稿旁车协议随包分发；
  不要引入字体字形或位图。
- **新增 UI 模块（`rustmark-ui`）**：按 `architecture-constraints.md` §10 四问判据判断模块归属；
  `views` 留 root，子层（`state/theme/shortcuts/widgets`）在 ui crate，富内容渲染在 `rustmark-render`；
  跨 crate 可见字段须进白名单 + 对应 `pub` setter（见 `tests/field_visibility_guard_test.rs`）。
- **新增端口（`rustmark-ports`）**：只放「领域需要、实现归 infra」的 trait，`ports → shared` 单向；
  **`shared` 绝不反向 `pub use rustmark_ports::*`**（会成环），调用点经 `rustmark_core::ports::*` 取用。

---

## 维护纪律（改文档前先读）

1. **文档只保留一个「家」**：任务状态只进 `v1.1-backlog.md`，数字只进 `metrics.md`，
   命令与排障只进 `debug-guide.md`，用户故障只进 `troubleshooting.md`；其余位置**引用，不复制**。
2. **历史不改写**：`adr/`、`CHANGELOG.md`、`v1.1-backlog.md` 的已完成条目、`logs/问题记录1.txt`
   一律不回改（含其中的旧路径）。归档迁移造成的老链接失效由 `archive/README.md` 索引兜底。
3. **三档归属**：仍在指引后续工作的 → 本页 §1/§2；记录「当时为什么」的 → §3；
   任务已收官且结论被吸收的 → §4 归档。**新文档不要平铺进 `docs/` 顶层**，按上表分档。
4. **数字必须当日实测**：跨快照引用「此前全绿」而不复跑，会积累出「文档声称全绿、实际为红」的失实
   （教训见 `PROGRESS.md` §1.1.1）。
