# docs/ 文档地图

> 最近整理：**2026-10-31**（补 `reviews/`、`diagrams/` 索引并校正计数；此前 2026-10-28 第三批按用途分目录）。
> 本页是 `docs/` 的**唯一导航入口**。分区规则：
> **`spec/` 是契约（改要同步跑门禁）、`guides/` 是手册（按动作查）、`plans/` 是进度（领活看这里）、
> `stages/` `design/` `reports/` 是可读证据（只读）。**

## 目录一览

| 目录 | 类别 | 收录 | 文件数 |
|---|---|---|---|
| [`spec/`](./spec/) | **全局契约与基线**（机器读取，路径即契约） | 架构规则 / 指标数字 / 阈值基线 / 拓扑基线 / 依赖图 | 5 |
| [`plans/`](./plans/) | **计划与进度** | 九阶段索引 + v1.1 任务入口 | 2 |
| [`guides/`](./guides/) | **操作手册**（面向具体动作） | 调试 / 排障 / 配置 / 测试 / 发布 / 依赖 / 三平台回归 | 9 |
| [`stages/`](./stages/) | **阶段技术文档** | 01–11 各层实现说明 | 11 |
| [`design/`](./design/) | **技术设计** | 缓存子系统设计 | 1 |
| [`diagrams/`](./diagrams/) | **图示** | 项目文件 / 模块级关系图 | 1 |
| [`reports/`](./reports/) | **专项报告与评审** | 合规差距 / 阈值审计与裁定 / shared 内部环评审 | 4 |
| [`reviews/`](./reviews/) | **评审与审计** | 代码评审 / 安全健壮性评审 / 发布卫生审计 | 2 |
| [`adr/`](./adr/) | **决策记录**（不可改写） | ADR-001 ~ ADR-013 | 13 |
| [`remediation/`](./remediation/README.md) | **整改执行期档案**（按批次分文件夹） | R-01~R-11 总案 + R-10 专项评估 | 5 |
| [`samples/`](./samples/) | **样本与夹具** | `示例文档.md`（mermaid 测试取样源） | 1 |
| [`logs/`](./logs/) | **流水**（append-only） | 问题记录 001–069 | 1 |
| [`pic/`](./pic/) | **素材** | 问题记录按编号引用的截图 | 15 |

合计 **71 个 tracked 文件**（口径：`git ls-files docs`；`docs/reviews/` 内新增的发布卫生审计文档尚待入库）。顶层只留本页——新增文档**必须**进上表某个目录，不许再平铺。

## 怎么用这份地图

| 你是谁 | 先读 |
|---|---|
| 新接手的开发者 | 本页 → [`plans/PROGRESS.md`](./plans/PROGRESS.md)（做到哪了）→ [`guides/debug-guide.md`](./guides/debug-guide.md)（跑起来 + 排障）→ [`spec/architecture-constraints.md`](./spec/architecture-constraints.md)（不许做什么） |
| 领任务开工 | [`plans/v1.1-backlog.md`](./plans/v1.1-backlog.md)（**唯一任务入口**）→ 对应阶段文档 → [`spec/architecture-constraints.md`](./spec/architecture-constraints.md) |
| 要改架构 / 加 crate | [`spec/architecture-constraints.md`](./spec/architecture-constraints.md) §10（crate 地图与归属四问）→ [`remediation/`](./remediation/README.md)（为什么是现在这个样子）→ [`spec/current_dependency_graph.html`](./spec/current_dependency_graph.html) → [`diagrams/project-relationship-map.md`](./diagrams/project-relationship-map.md)（文件 / 模块级关联图） |
| 用户 / 报缺陷 | [`guides/troubleshooting.md`](./guides/troubleshooting.md)（配置目录 / 草稿恢复 / 崩溃报告） |
| 准备发布 | [`guides/release.md`](./guides/release.md) → [`reports/release-compliance-gap-report.md`](./reports/release-compliance-gap-report.md)（还剩什么没闭环）→ [`guides/testing.md`](./guides/testing.md) |
| 关心体积 / 性能数字 | [`spec/metrics.md`](./spec/metrics.md)（**唯一数字来源**）→ [`guides/dependencies.md`](./guides/dependencies.md) |

---

## 1. `spec/` · 全局契约与基线

> 这五份是**机器或流程直接读取**的判据。移动/改名会让门禁变红或**静默失效**，
> 所以它们单独成区且路径被代码写死。改内容必须同步跑门禁。

| 文档 | 定位 | 被谁读 |
|---|---|---|
| [`architecture-constraints.md`](./spec/architecture-constraints.md) | **架构规则唯一真源**：分层 / 依赖矩阵 / 各层禁令 / `shared` 准入三问 + 登记表 / §10 crate 地图与模块归属四问 | `cargo run --bin check-arch`（按区段标记解析「豁免登记表」「`shared` 准入表」，未登记即违规） |
| [`metrics.md`](./spec/metrics.md) | **七项硬指标唯一数字来源**（体积 / 启动 / 内存 / 帧耗时 / 解析 / 高亮 / 搜索） | 人工 + CI 注释口径；`size_gate.sh` 默认值与 §2.8 同源 |
| [`thresholds.baseline.yaml`](./spec/thresholds.baseline.yaml) | 121 条阈值的取值 / 单位 / 源码行号基线（运行时**不加载**） | `python3 tools/check_thresholds.py`（CI `Threshold baseline` 步骤） |
| [`workspace-topology.expected.txt`](./spec/workspace-topology.expected.txt) | 16 条本地依赖边的期望快照（含手写注释头） | CI 拓扑 diff + `scripts/gen_dep_graph.sh` 回写 |
| [`current_dependency_graph.html`](./spec/current_dependency_graph.html) | **10 crate / 16 条本地依赖边**的实测依赖图（SVG，分层布局 + 每 crate 规模） | 人工维护；`gen_dep_graph.sh` 只写 `target/` 下的机器图，**不覆盖本文** |

> 原 `target_dependency_graph.html`（2026-10-27 目标态）已归档：
> 当前依赖图见 [`spec/current_dependency_graph.html`](./spec/current_dependency_graph.html)。
> 它预测的 5 项中 2 项达成、**2 项经决议不做**（`shared` 拆两 crate、`views` 迁 ui，见 ADR-013 决策 3）、
> 且未预见 `rustmark-ports`。目标态与现状已合一，故合并为单图。

## 2. `plans/` · 计划与进度

| 文档 | 定位 | 什么时候读 |
|---|---|---|
| [`PROGRESS.md`](./plans/PROGRESS.md) | 九阶段实施计划索引 + 当前进度 + v1.1 状态摘要 | 了解「做了什么、做到哪」；阶段总览 / 依赖顺序 / 硬约束 |
| [`v1.1-backlog.md`](./plans/v1.1-backlog.md) | **v1.1 唯一任务入口**（V11-01~67 总览 + 逐项说明 + 完成记录） | 领任务、查任务状态与完成记录 |

## 3. `guides/` · 操作手册

| 文档 | 定位 | 什么时候读 |
|---|---|---|
| [`debug-guide.md`](./guides/debug-guide.md) | 运行 / 调试命令手册 + **GUI 缺陷定位方法论**（§6） | 跑程序、写测试、**排障前** |
| [`troubleshooting.md`](./guides/troubleshooting.md) | 终端用户故障排查（配置目录 / 重置 / 草稿恢复 / 崩溃报告） | 应用出问题、要提 issue |
| [`config-reference.md`](./guides/config-reference.md) | **配置面参考**：`preferences.toml` 全字段 + 自定义主题色板 + 快捷键语法与默认键位 | 手改配置、写主题、查字段含义 |
| [`testing.md`](./guides/testing.md) | 测试金字塔 / 覆盖率门禁 / 排除清单与理由 | 写测试、跑 CI 等价命令 |
| [`dependencies.md`](./guides/dependencies.md) | 依赖清单、锁定版本、许可证、复查报告（§6） | 增删依赖、评审体积 |
| [`release.md`](./guides/release.md) | 发布流程（tag 驱动、CI 全自动、回滚三步） | 发版前；**`release_dryrun.py` 校验其回滚三步齐备** |
| [`manual-qa-windows.md`](./guides/manual-qa-windows.md) / [`macos`](./guides/manual-qa-macos.md) / [`linux`](./guides/manual-qa-linux.md) | 三平台手工回归清单（V11-02 **待执行**） | 真机回归、验收 GUI 行为 |

## 4. `stages/` · 阶段技术文档（01–11）

| 文档 | 内容 | 状态 |
|---|---|---|
| `01-需求基线与技术决策.md` | 需求基线、ADR 决策记录、指标口径 | 阶段快照 |
| `02-工程骨架与分层边界.md` | 模块系统、分层约束、`check-arch` | 阶段快照 |
| `03-领域模型实现.md` | 5 个限界上下文的领域层落地 | 阶段快照 |
| `04-应用服务与用例编排.md` | 5 个 AppService、事件订阅链 | 阶段快照 |
| `05-基础设施实现.md` | ropey / pulldown / syntect 适配、fs 仓储、导出器 | 阶段快照 |
| `06-表现层与交互实现.md` | egui 视图、快捷键单一数据源、主题 | **仍被 `widgets.rs` 引用**（五条纪律条目出处） |
| `07-集成联调与原型对齐.md` | 集成联调、原型契约对齐、手工回归 | 阶段快照 |
| `08-性能优化与质量加固.md` | 体积 / 启动 / 内存优化过程 | 阶段快照 |
| `09-测试体系打包与发布.md` | 测试金字塔、覆盖率门禁、三平台打包 | 阶段快照 |
| `10-编辑区技术实现.md` | 编辑区专项（行高 / 高亮 / IME / 行号栏） | **仍指导改代码** |
| `11-预览区技术实现.md` | 预览区专项（渲染管线 / 滚动同步 / 图表） | **仍指导改代码** |

> 01–09 的「任务清单 / 验收记录」是各阶段收尾时的快照。**当前状态一律以
> `plans/PROGRESS.md` §1.2 与 `plans/v1.1-backlog.md` 为准**（历史快照与新结论冲突时，新结论胜）。

## 5. `design/` · 技术设计

| 文档 | 内容 |
|---|---|
| [`cache-design.md`](./design/cache-design.md) | 缓存子系统设计：`revision` 语义失效 / 防抖节拍 / LruMap 收敛 / 淘汰策略。改缓存前先读 |

## 6. `reports/` · 专项报告与评审

> 结论仍有效，但**不作为现行判据**——判据在 `spec/`。唯一例外是合规差距报告 §6（发布前必读）。

| 文档 | 内容 | 现状 |
|---|---|---|
| [`release-compliance-gap-report.md`](./reports/release-compliance-gap-report.md) | **发布阻断项判定依据** + 未闭环待办（§6：三平台产物 / 真机回归 / 证书） | **活文档**：§6 未闭环项持续维护；§0–§5 实测数字为 2026-10-22 快照（`0.2.0` / 186 crate / 388 组件），现行口径见 `Cargo.toml` 与 SBOM |
| [`threshold-audit-2026-10-25.md`](./reports/threshold-audit-2026-10-25.md) | 115 项阈值的全量审计（取值 / 依据 / 判定） | `spec/thresholds.baseline.yaml:8` 的 `audit_report:` 指向本篇 |
| [`threshold-review-2026-10-25.md`](./reports/threshold-review-2026-10-25.md) | 审计的裁定结论（6 个语义簇 / 3 项阻断 / P0-P1 落地记录） | 同上 `review_report:`；121 条阈值的取值依据 |
| [`shared-internal-cycle-review.md`](./reports/shared-internal-cycle-review.md) | `shared` 内部环评审（E-27 / V11-55） | 结论已固化为守卫；**`ports/dialog.rs:107` 直接引用为成环判据出处** |

## 7. `adr/` · 决策记录（13 条，不可改写）

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
| [ADR-011](./adr/ADR-011-action-routing-and-dual-intent.md) | 动作路由：穷举路由表 + `Unbound` 显式声明（无 `_` 兜底） |
| [ADR-012](./adr/ADR-012-icon-system-and-resource-layout.md) | 图标系统：向量手绘废止字形 + 规格即数据（16×16）；字体外置随包分发 |
| [ADR-013](./adr/ADR-013-core-adapter-residency-and-leaf-extraction.md) | core 适配层驻留与叶层外移：模块归属四问 + 外移触发条件量化表 |

## 8. `remediation/` · 整改执行期档案

| 路径 | 批次 | 内容 | 状态 |
|---|---|---|---|
| [`r01-r11/remediation-plan-2026-10-27.md`](./remediation/r01-r11/remediation-plan-2026-10-27.md) | R-01~R-11 | 架构整改细化实施方案（问题画像 A~I → 目标 O1~O7 → 逐条改造项 + 落地记录） | ✅ 全量收官 |
| [`r10/evaluation.md`](./remediation/r10/evaluation.md) | R-10 | 拆库 vs 端口升 crate；结论「**强最大解不可达**」 | ✅ 项 A 落地 `0f20bc8` |
| [`r10/max-solution.md`](./remediation/r10/max-solution.md) | R-10 | 最大解 M0~M3 的设计与不可达论证（M1 成环禁 / M2 伪收敛） | ✅ 项 A + 项 B 落地 |

> 这三篇**不归档**：总方案是全仓唯一记载「每条改造具体怎么改、偏差为何」的文档；
> R-10 两篇承载**否定性结论**——它不会变成代码也不会变成规则，丢了会有人重走一遍。

## 9. `samples/` · 样本与夹具

| 文档 | 内容 |
|---|---|
| [`示例文档.md`](./samples/示例文档.md) | Markdown 样本：`rustmark-render` 的 mermaid / flowchart.js 测试**逐字取自本篇**对应小节。**不是日志**，改动会牵动渲染测试 |

## 10. `logs/` 与 `pic/` · 流水与素材

| 路径 | 内容 |
|---|---|
| [`logs/问题记录1.txt`](./logs/问题记录1.txt) | 用户反馈与缺陷修复的**原始流水**（001~069，**倒序** append-only）——调试经验已提炼至 `guides/debug-guide.md` §6 |
| [`pic/`](./pic/) | 文档引用的截图（问题记录按编号引用） |


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
- **新增 UI 模块（`rustmark-ui`）**：按 `spec/architecture-constraints.md` §10 四问判据判断模块归属；
  `views` 留 root，子层（`state/theme/shortcuts/widgets`）在 ui crate，富内容渲染在 `rustmark-render`；
  跨 crate 可见字段须进白名单 + 对应 `pub` setter（见 `tests/field_visibility_guard_test.rs`）。
- **新增端口（`rustmark-ports`）**：只放「领域需要、实现归 infra」的 trait，`ports → shared` 单向；
  **`shared` 绝不反向 `pub use rustmark_ports::*`**（会成环），调用点经 `rustmark_core::ports::*` 取用。

---

## 维护纪律（改文档前先读）

1. **一个「家」**：任务状态只进 `plans/v1.1-backlog.md`，数字只进 `spec/metrics.md`，
   命令与排障只进 `guides/debug-guide.md`，用户故障只进 `guides/troubleshooting.md`；
   其余位置**引用，不复制**。
2. **按目录归属**：新增文档**必须**进本页「目录一览」里某个目录，**不许平铺进 `docs/` 顶层**；
   归属存疑时按「谁读它」判断，而不是按「它讲什么」。
3. **`spec/` 是路径契约**：动这 5 个文件等于改代码契约，必须同步 `tools/arch_check.rs`、
   `tools/check_thresholds.py`、`tools/release_dryrun.py`、`.github/workflows/ci.yml`、
   `scripts/gen_dep_graph.sh` 里的路径，并跑全套门禁。
4. **历史不改写**：`adr/` 的论断、`CHANGELOG.md`、`plans/v1.1-backlog.md` 的已完成条目、
   `logs/问题记录1.txt` 一律不回改（含其中的旧路径）。
5. **三档归属**：仍在指引后续工作的 → `spec/` `plans/` `guides/` `stages/` `design/`；
   记录「当时做了什么」的 → `reports/` `remediation/` `adr/`；
   任务已收官且结论被吸收的 → 不在 docs/ 留存（归档区已于 2026-10-31 移除）。
6. **数字必须当日实测**：跨快照引用「此前全绿」而不复跑，会积累出「文档声称全绿、实际为红」的
   失实（教训见 `plans/PROGRESS.md` §1.1.1）。
