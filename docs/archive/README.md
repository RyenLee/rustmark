# 归档区索引（archive）

> 归档日期：2026-10-14 ｜ 归档动因：文档整理（去重 + 归档 + 调试经验沉淀）
> 定位：**已完成的一次性文档**。结论与数据仍有效、可追溯，但不再随版本维护；
> 现行文档见 [`docs/README.md`](../README.md)（文档地图）。

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
| [textedit-cost-attribution.md](./stage-reports/textedit-cost-attribution.md) | `docs/logs/T.txt` 审计延伸（新建） | 2026-10-17 | 编辑区 1MB 击键帧成本归因（三轮测量 + diff_replace 落地 + 建议二/四/一/三 落地） | **归因已闭环、四项建议（052/053/054/055）全部落地**；现行数字见 `docs/metrics.md` §3.2.7/§3.2.8/§3.2.9/§3.2.10 |

## audits-2026-10-12/ · 全项目审计快照（2 篇）

| 文档 | 原路径 | 内容 | 现状 |
|---|---|---|---|
| [optimization-report.md](./audits-2026-10-12/optimization-report.md) | `docs/audit/optimization-report.md` | 2026-10-12 五维度静态审计（错误处理/性能/架构/测试/安全），R-001~R-0xx | 建议项已逐条处置，落地记录见下一篇 |
| [optimization-report-execution.md](./audits-2026-10-12/optimization-report-execution.md) | `docs/audit/optimization-report-execution.md` | 上述审计的执行报告：7 个 commit 清单 + 逐项落地详情 | 已执行完毕（P0/P1/P2 全落地） |

## 与现行文档的衔接

- 体积数字的现行口径：`docs/metrics.md`（§2.4 / §2.5 为 v1.1 最新实测）；
- 依赖清单与复查结论：`docs/dependencies.md`（§6 为 2026-10-14 复查报告）；
- 缺陷修复的流水记录：`docs/logs/问题记录1.txt`（001~046，倒序）；
- 调试方法论沉淀：`docs/debug-guide.md` §7（源自本归档区各篇的实战教训）。
