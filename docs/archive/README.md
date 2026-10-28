# 归档区索引（archive）

> **第一批归档**：2026-10-14 ｜ 动因：文档整理（去重 + 归档 + 调试经验沉淀）——`stage-reports/` / `audits-2026-10-12/` / 2026-10-17 三篇
> **第二批归档**：2026-10-28 ｜ 动因：R-01~R-11 整改收官，评审 / 方案 / 复盘类一次性文档集中入库——`architecture-reviews/` / `incident-reports/` / `remediation-plans/` / `tooling/` / `pic/`
> 定位：**已完成的一次性文档**。结论与数据仍有效、可追溯，但不再随版本维护；
> 现行文档见 [`docs/README.md`](../README.md)（文档地图），整改执行期档案见
> [`docs/remediation/`](../remediation/README.md)。

## 收录标准（什么进归档、什么留现行）

| 进归档 | 留现行 |
|---|---|
| 阶段收尾时的一次性报告 / 验证记录 | 仍在指引后续工作的规范、口径、清单 |
| 已完成的修复方案与实施步骤记录（结论已被现行文档吸收或指向） | 活跃任务的唯一入口（`v1.1-backlog.md`） |
| 历史审计快照（建议已处置完毕） | 决策记录（`adr/`）、唯一数字来源（`metrics.md`） |
| 被更新版取代的旧版文档 | 面向用户/开发者的操作手册（`troubleshooting.md` / `debug-guide.md`） |

**读归档文档前先知道**：归档文档中的「当前 / 待办 / 未修复」表述只在其**写作日期**成立。
后续演进以现行文档（`docs/README.md` 列出的各篇）与 `docs/logs/问题记录1.txt` 为准。

## stage-reports/ · 阶段报告与方案（13 篇）

| 文档 | 原路径 | 写作日期 | 内容 | 现状（2026-10-16） |
|---|---|---|---|---|
| [textedit-lag-audit.md](./stage-reports/textedit-lag-audit.md) | 本处新建 | 2026-10-16 | `docs/logs/T.txt`（egui `TextEdit` 卡顿通用分析）逐条审计：三条成因 / 三条方案 / 诊断建议在本项目的实际状态，以及本项目真正的每帧浪费与修复 | **审计已闭合**（问题 046）；数字进 `metrics.md` §3.2.2、行数规则进 `shared::text`；方案①在本期不可用（0.37 `TextEdit` 无虚拟化 API） |
| [egui-0.37-upgrade.md](./stage-reports/egui-0.37-upgrade.md) | 本处新建 | 2026-10-16 | egui/eframe/epaint/wgpu 升级评估：版本与兼容性、API 变更清单及代码位置、TextEdit 卡顿成因与 0.37 针对性改进、后续排查方向 | **升级已完成并交付**（四道门禁全绿）；版本事实已进 `dependencies.md` §1、数字已进 `metrics.md` §3.2.0 |

| [prototype-contract.md](./stage-reports/prototype-contract.md) | `docs/prototype-contract.md` | 2026-10-07 | 原型行为契约 A 29 条 + 缺陷免疫清单 B 17 项（S1-06） | B 清单 17 项已全部验证免疫，见 regression-checklist |
| [prototype-alignment.md](./stage-reports/prototype-alignment.md) | `docs/prototype-alignment.md` | 2026-10-07 | 原型 ↔ Rust 行为对齐矩阵 45 行（S7-02） | 差异项均已处置；渲染/语法差异的裁决原则仍适用 |
| [e2e-cases.md](./stage-reports/e2e-cases.md) | `docs/e2e-cases.md` | 2026-10-07 | 端到端用例集 32 条 + 覆盖矩阵（S7-01） | 自动化部分在 `tests/e2e/`，本文是用例目录存档 |
| [regression-checklist.md](./stage-reports/regression-checklist.md) | `docs/regression-checklist.md` | 2026-10-07 | 原型 17 项缺陷回归验证（S7-03） | 17/17 已免疫，断言在 `tests/integration/regression_test.rs` |
| [large-file-report.md](./stage-reports/large-file-report.md) | `docs/large-file-report.md` | 2026-10-08 | S7-04 大文件验证报告；登记 DEF-01 查找 O(N×M) | **DEF-01 已修复**（S6-08，O(N) 重写）；报告留存取证过程 |
| [crash-recovery-report.md](./stage-reports/crash-recovery-report.md) | `docs/crash-recovery-report.md` | 2026-10-08 | S7-06 异常退出与草稿恢复验证报告 | DEF-02 已修复并接线；自动保存/恢复现为生效功能 |
| [font-encoding-fix-plan.md](./stage-reports/font-encoding-fix-plan.md) | `docs/font-encoding-fix-plan.md` | 2026-10-08 | 中文乱码定位与修复方案（字体 + GBK 解码） | 已实施；GBK/UTF-16 解码判据仍被 `docs/05` 引用 |
| [p1p2-remediation-plan.md](./stage-reports/p1p2-remediation-plan.md) | `docs/p1p2-remediation-plan.md` | 2026-10-08 | P1/P2 五项未交付功能的分析与路线 | 5/5 已实施（零依赖），另补整篇长图 |
| [p1p2-implementation-log.md](./stage-reports/p1p2-implementation-log.md) | `docs/p1p2-implementation-log.md` | 2026-10-08~09 | 上述五项 + 四项遗留缺陷的逐步实施记录 | 已完成；记录含每项「修改文件清单/验证/已知问题」 |
| [optimization-log.md](./stage-reports/optimization-log.md) | `docs/optimization-log.md` | 2026-10-08 | 阶段 08 优化决策日志 OPT-001~014（含放弃项） | 阶段已收尾；「放弃项及原因」对后续优化仍有参考价值 |
| [textedit-cost-attribution.md](./stage-reports/textedit-cost-attribution.md) | `docs/logs/T.txt` 审计延伸（新建） | 2026-10-17 | 编辑区 1MB 击键帧成本归因（三轮测量 + diff_replace 落地 + 建议二/四/一/三 落地） | **归因已闭环、四项建议（052/053/054/055）全部落地**；现行数字见 `docs/spec/metrics.md` §3.2.7/§3.2.8/§3.2.9/§3.2.10 |

## audits-2026-10-12/ · 全项目审计快照（2 篇）

| 文档 | 原路径 | 内容 | 现状 |
|---|---|---|---|
| [optimization-report.md](./audits-2026-10-12/optimization-report.md) | `docs/audit/optimization-report.md` | 2026-10-12 五维度静态审计（错误处理/性能/架构/测试/安全），R-001~R-0xx | 建议项已逐条处置，落地记录见下一篇 |
| [optimization-report-execution.md](./audits-2026-10-12/optimization-report-execution.md) | `docs/audit/optimization-report-execution.md` | 上述审计的执行报告：7 个 commit 清单 + 逐项落地详情 | 已执行完毕（P0/P1/P2 全落地） |

## 2026-10-17 · 上线前体检与代码清理（3 篇）

| 文档 | 内容 | 现状 |
|---|---|---|
| [上线前体检报告-2026-10-17.md](./上线前体检报告-2026-10-17.md) | 代码审查 + 安全审计 + QA 门禁三联结论（上线前全检） | 已闭合；门禁基线数字仍有效 |
| [代码清理审计-2026-10-17.md](./代码清理审计-2026-10-17.md) | 全仓冗余/低性能代码审计：F-1~F-4 功能缺陷、D-1~D-4 死代码、C-1~C-8 微优化，按风险分批 | 四批次已全部实施，落地情况见下一篇 |
| [代码清理实施记录-2026-10-17.md](./代码清理实施记录-2026-10-17.md) | 上述审计的执行记录：逐项落地、16 个守卫测试、与审计建议的三处偏差及理由 | **实施完毕**；测试 1774→1790，四道门禁 + release 冒烟全绿 |

## crash-reports/ · 用户提交的崩溃转储（1 份）

| 文档 | 内容 | 现状 |
|---|---|---|
| [crash-20261014135941-29300.txt](./crash-reports/crash-20261014135941-29300.txt) | 2026-10-14 用户报告「搜索功能崩溃」时附的崩溃报告（panic at workspace/service.rs:468，UTF-8 字符边界） | 对应缺陷 **028 已修复**（`floor_char_boundary` + 回归 `search_snippet_never_splits_utf8_chars`）；原件从 `docs/logs/` 移至此处（诊断工件不进流水账），问题记录 028 的路径引用已同步 |

## architecture-reviews/ · 架构评审与改造收官（第二批，9 篇 + 2 图）

> 共同特征：任务 **100% 收官**、结论已被 `architecture-constraints.md` / ADR-010~013 吸收、
> 文中数字为**写作时点快照**（多数已漂移）。**不要**按这里的数字判断现状——现行口径看
> [`metrics.md`](../spec/metrics.md) 与 [`PROGRESS.md`](../plans/PROGRESS.md) §1.2。

| 文档 | 原路径 | 写作日期 | 内容 | 现状 |
|---|---|---|---|---|
| [architecture-review.md](./architecture-reviews/architecture-review.md) | `docs/architecture-review.md` | 2026-10-20 | 全库架构评估：3 P0 + 8 P1 + 7 P2 + 6 项改造遗留；含对 `check-arch` 判定逻辑的复刻验证 | **18 项已 100% 转为 R-01~R-21 并落地**（backlog V11-54 ✅）；`src/ 302 文件 / 33,845 行`等规模数字已失效 |
| [architecture-layers.svg](./architecture-reviews/architecture-layers.svg) | `docs/arc/architecture-layers.svg` | 2026-10-20 | 上篇的「改造后分层依赖 + 四处违规边」示意图 | 随上篇归档；7-crate 时代图形，现状见 [`current_dependency_graph.html`](../spec/current_dependency_graph.html) |
| [target-dependency-graph-2026-10-27.html](./architecture-reviews/target-dependency-graph-2026-10-27.html) | `docs/target_dependency_graph.html` | 2026-10-27 | 目标态依赖图：5 项外移计划（shared 拆 kernel+support / icons 解耦 / views 迁 ui / rich 抽出 render / ports） | **预测已被现实修正**：2 项达成（icons 解耦、render 抽出）、**2 项经决议不做**（shared 不再拆 crate、views 留 root，见 ADR-013 决策 3）、ports 已独立成 crate 而原图未画。目标态与现状合一，故与 current 图合并为单图 |
| [architecture-refactor-plan.md](./architecture-reviews/architecture-refactor-plan.md) | `docs/architecture-refactor-plan.md` | 2026-10-20 | **21 条改造项（R-01~R-21）+ M0~M6 里程碑**，每条七字段 + 验收判据 | **§5.3 进度表 22 行全 OK**（2026-10-21 收官，`2099 passed`） |
| [architecture-review-2026-10-23.md](./architecture-reviews/architecture-review-2026-10-23.md) | `docs/architecture-review-2026-10-23.md` | 2026-10-23 | 八维度复审：前次问题修复核对 + D-1~D-12 | D-1~D-8 全闭合；D-9 / D-12 记为**已知取舍**（不修）；D-11 core 侧闭合 |
| [multi-crate-review-2026-10-23.md](./architecture-reviews/multi-crate-review-2026-10-23.md) | `docs/multi-crate-review-2026-10-23.md` | 2026-10-23 | 多 crate 拆分专项评估（DDD 边界 / 依赖 / 内聚 / DX / 构建） | 结论已由 D-3 阶段 4a/4b 落地，量化触发表沉淀为 [ADR-013](../adr/ADR-013-core-adapter-residency-and-leaf-extraction.md) |
| [workspace-split-assessment-2026-10-24.md](./architecture-reviews/workspace-split-assessment-2026-10-24.md) | `docs/workspace-split-assessment-2026-10-24.md` | 2026-10-24 | 同上，D-3 4a 延续（「是否再拆」决策） | 「再拆与否」的判据已由 ADR-013 触发条件取代 |
| [core-structure-review-2026-10-24.md](./architecture-reviews/core-structure-review-2026-10-24.md) | `docs/core-structure-review-2026-10-24.md` | 2026-10-24 | core 结构复审：三大病灶（大文件 / 状态过宽 / 服务过大） | 对应 V11-56~67 全部收官（G/H 已闭项、V11-59/60/61 已闭环） |
| [core-refactor-tasklist-2026-10-24.md](./architecture-reviews/core-refactor-tasklist-2026-10-24.md) | `docs/core-refactor-tasklist-2026-10-24.md` | 2026-10-24 | core 改造逐项任务清单（V11-56~67，含完成记录） | 全部落地/闭项；基线 `2131 passed` 已过期 |
| [state-field-audit.md](./architecture-reviews/state-field-audit.md) | `docs/state-field-audit.md` | 2026-10-24 | UI state 24 文件 / 39 结构体 / 201 字段四问审计（K/V/A 判定） | 结论 K=201 / V=0 / A=0 已固化进守卫；白名单数字 **168 → 594**，本篇数字已全线过期 |
| [rustmark-evaluation-2026-10-25.md](./architecture-reviews/rustmark-evaluation-2026-10-25.md) | `docs/rustmark-evaluation-2026-10-25.md` | 2026-10-25 | 全方位评估 34 项（架构 / 代码 / 文档三维度） | 34 项已全部收口（M2 明确不做、E-11~13 二次核验、TD-04 触发已记） |

## incident-reports/ · 缺陷技术复盘（第二批，2 篇）

| 文档 | 原路径 | 内容 | 现状 |
|---|---|---|---|
| [search-tab-performance-report.md](./incident-reports/search-tab-performance-report.md) | `docs/search-tab-performance-report.md` | 侧栏「搜索」标签卡顿专项：全链路 → 根因 → 实测 → 修复 → 门禁 | **缺陷已修复**并固化为回归；性能数字锚定 2026-10-23，现行值见 [`metrics.md`](../spec/metrics.md) §3.2 |
| [search-tab-postmortem.md](./incident-reports/search-tab-postmortem.md) | `docs/search-tab-postmortem.md` | 同事件的 5-Why 复盘：修复方案、性能对比、经验教训 | 方法论已被 [`debug-guide.md`](../guides/debug-guide.md) §6 吸收 |

## remediation-plans/ · 已收官的整改方案（第二批，2 篇）

> 与 [`docs/remediation/`](../remediation/README.md)（执行期档案）的区别：**这里的批次已收官且判据作用已消失**；
> R-04 / R-07 的落地记录分别见 `22e8a86` / `aea1665`，规则沉淀见 `architecture-constraints.md` §10。

| 文档 | 原路径 | 内容 | 现状 |
|---|---|---|---|
| [r04-render-crate-promotion-plan.md](./remediation-plans/r04-render-crate-promotion-plan.md) | `docs/r04-render-crate-promotion-plan.md` | `presentation::rich` → `rustmark-render` 升 crate 方案（egui-free 默认 + feature 门控） | **已落地 `22e8a86`**（第 9 个 crate 之外的 render 独立）；注意本篇头部仍写「草案待拍板」，属历史时点表述 |
| [R-07-infra-split-plan.md](./remediation-plans/R-07-infra-split-plan.md) | `docs/R-07-infra-split-plan.md` | `rustmark-infra` 9 个 ≥400 行大文件的拆分设计（生产/测试比例测算） | **R-07 闭项**：Batch 0 `aea1665` 外迁测试内联模块；`incremental` / `stack` / `end` 三个有状态强内聚文件显式豁免 |

## tooling/ · 工具治理（第二批，2 份）

| 文档 | 原路径 | 内容 | 现状 |
|---|---|---|---|
| [python-cleanup-report.md](./tooling/python-cleanup-report.md) | `docs/python-cleanup-report.md` | Python 工具清理报告（`tools/*.py` 26 → 5），含删除依据与恢复路径 | 一次性操作记录；备份目录在仓库外，追责价值已归零 |
| [缓存改进.txt](./tooling/缓存改进.txt) | `docs/logs/缓存改进.txt` | 外部 AI 生成的 egui `TextEdit` 卡顿通用建议（0.33 时代三段成因 + 三套方案） | **已被更权威的两篇完全覆盖**：[textedit-lag-audit.md](./stage-reports/textedit-lag-audit.md)（逐条核对本项目实际状态）+ [textedit-cost-attribution.md](./stage-reports/textedit-cost-attribution.md)（三轮实测归因） |

## 第二批归档的图片（9 张，进 pic/）

`docs/pic/` 下 9 张**全仓零引用**的截图（无标题、无文档指向、无问题记录编号），随第二批移入
[pic/](./pic/)。合计 3.76 MB。原文未作任何标注，**恢复检索只能靠文件名**——
`PixPin_2026-10-18_20-25-09` / `_20-26-57` / `_20-32-28` / `_22-05-13` / `_23-06-07` /
`PixPin_2026-10-19_15-06-53` / `右键前.png` / `右键后.png` / `重置失效.png`。

## 与现行文档的衔接

- 体积数字的现行口径：`docs/spec/metrics.md`（§2.4 / §2.5 为 v1.1 最新实测）；
- 依赖清单与复查结论：`docs/guides/dependencies.md`（§6 为 2026-10-14 复查报告）；
- 缺陷修复的流水记录：`docs/logs/问题记录1.txt`（001~046，倒序）；
- 调试方法论沉淀：`docs/guides/debug-guide.md` §7（源自本归档区各篇的实战教训）；
- 整改执行期档案（**未归档**）：`docs/remediation/`（R-01~R-11 总方案 + R-10 两份评估）。
