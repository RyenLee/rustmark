# docs/ 文档地图

> 最近整理：2026-10-14（归档 + 去重 + 调试经验沉淀）。本页是 `docs/` 的导航入口：
> 每篇文档的定位、面向读者、什么时候读。已完成的一次性文档在 [`archive/`](./archive/README.md)。

## 怎么用这份地图

| 你是谁 | 先读 |
|---|---|
| 新接手的开发者 | 本页 → `PROGRESS.md`（进度索引）→ `debug-guide.md`（跑起来 + 排障） |
| 领任务开工 | `v1.1-backlog.md`（唯一任务入口）→ 对应阶段文档 → `.agent/conventions.md` |
| 用户 / 报缺陷 | `troubleshooting.md`（配置目录 / 草稿恢复 / 崩溃报告） |
| 准备发布 | `release.md` → `CHANGELOG.md` → `testing.md` |
| 关心体积 / 性能数字 | `metrics.md`（唯一数字来源）→ `dependencies.md` |

## 现行文档（根目录）

| 文档 | 定位 | 读者 | 什么时候读 |
|---|---|---|---|
| [`PROGRESS.md`](./PROGRESS.md) | 九阶段实施计划索引 + 当前进度 | 全体 | 了解「做了什么、做到哪」；阶段总览 / 依赖顺序 / 硬约束 |
| [`v1.1-backlog.md`](./v1.1-backlog.md) | **v1.1 唯一任务入口**（V11-01~26 总览 + 逐项说明） | 维护者 | 领任务、查任务状态与完成记录 |
| [`debug-guide.md`](./debug-guide.md) | 运行 / 调试命令手册 + **GUI 缺陷定位方法论**（§6） | 开发者 | 跑程序、写测试、**排障前** |
| [`troubleshooting.md`](./troubleshooting.md) | 终端用户故障排查（配置目录 / 重置 / 草稿恢复 / 崩溃报告） | 用户 | 应用出问题、要提 issue |
| [`metrics.md`](./metrics.md) | 七项硬指标口径与实测值（**唯一数字来源**） | 维护者 | 关心体积 / 启动 / 内存 / 性能数字时 |
| [`dependencies.md`](./dependencies.md) | 依赖清单、锁定版本、许可证、复查报告（§6） | 维护者 | 增删依赖、评审体积 |
| [`testing.md`](./testing.md) | 测试金字塔 / 覆盖率门禁 / 当前测试状态 | 贡献者 | 写测试、跑 CI 等价命令 |
| [`release.md`](./release.md) | 发布流程（tag 驱动、CI 全自动、回滚） | 维护者 | 发版前 |
| [`manual-qa-windows.md`](./manual-qa-windows.md) / [`manual-qa-macos.md`](./manual-qa-macos.md) / [`manual-qa-linux.md`](./manual-qa-linux.md) | 三平台手工回归清单（V11-02 待执行） | 测试者 | 真机回归、验收 GUI 行为 |

## 架构评估与改造（2026-10-20）

| 文档 | 定位 | 读者 | 什么时候读 |
|---|---|---|---|
| [`architecture-review.md`](./architecture-review.md) | 全库架构评估报告：模块职责边界、依赖关系、P0~P3 问题清单（18 项主问题 + 11 项清理） | 维护者 | 想知道「现状哪里有问题、为什么这么判」 |
| [`architecture-refactor-plan.md`](./architecture-refactor-plan.md) | 上述评估的**可执行改造方案**：21 条改造项（R-01~R-21，各含七字段）+ M0~M6 里程碑 + 验收判据 | 维护者 | 领改造任务、查某一条怎么做与怎么算做完 |

> 两份文档构成一对：**review 回答「问题是什么」，plan 回答「怎么改、怎么验」**。
> 任务状态仍只在 [`v1.1-backlog.md`](./v1.1-backlog.md) 的 V11-54 / V11-54a 登记。
> 具体文件路径与行号以基线 `e09ad2b` 为准，**行号漂移时以符号名为准**。

## 阶段文档（01–11）

| 文档 | 内容 |
|---|---|
| `01-需求基线与技术决策.md` | 需求基线、ADR 决策记录、指标口径 |
| `02-工程骨架与分层边界.md` | 模块系统、分层约束、`check-arch` |
| `03-领域模型实现.md` | 5 个限界上下文的领域层落地 |
| `04-应用服务与用例编排.md` | 5 个 AppService、事件订阅链 |
| `05-基础设施实现.md` | ropey / pulldown / syntect 适配、fs 仓储、导出器 |
| `06-表现层与交互实现.md` | egui 视图、快捷键单一数据源、主题（77.1 KB，阶段文档中最大单篇） |
| `07-集成联调与原型对齐.md` | 集成联调、原型契约对齐、手工回归 |
| `08-性能优化与质量加固.md` | 体积 / 启动 / 内存优化过程 |
| `09-测试体系打包与发布.md` | 测试金字塔、覆盖率门禁、三平台打包 |
| `10-编辑区技术实现.md` | 编辑区专项（行高 / 高亮 / IME / 行号栏） |
| `11-预览区技术实现.md` | 预览区专项（渲染管线 / 滚动同步 / 图表） |

> 阶段文档中的「任务清单 / 验收记录」是各阶段收尾时的快照；**当前状态一律以
> `PROGRESS.md` §1.1 与 `v1.1-backlog.md` 为准**（历史快照与新结论冲突时，新结论胜）。

## 决策记录（adr/）

| ADR | 决策 |
|---|---|
| [ADR-001](./adr/ADR-001-ui-framework.md) | UI 框架：egui + eframe（wgpu 后端），不采用 Tauri（**当前锁定 0.37.0**——ADR 正文记的是当初的 0.33.3，升级后的现行版本见 `Cargo.toml`） |
| [ADR-002](./adr/ADR-002-parser-highlighter.md) | 解析与高亮：pulldown-cmark + syntect（feature 收窄） |
| [ADR-003](./adr/ADR-003-persistence-export.md) | 持久化与导出：ropey trait 隔离、原子写 |
| [ADR-004](./adr/ADR-004-pdf-export.md) | PDF 导出：不内嵌引擎 → 系统打印 → **无头 Chromium**（修订） |
| [ADR-005](./adr/ADR-005-prototype-relationship.md) | 原型关系：行为契约来源，非代码来源 |
| [ADR-006](./adr/ADR-006-memory-threshold-revision.md) | 内存阈值修订：按平台各自口径（Windows Private WS ≤150 MB） |
| [ADR-007](./adr/ADR-007-rich-renderer-boundary.md) | `presentation/rich/` 的边界：零 egui 依赖，与 `views` / `state` / `theme` 隔离 |
| [ADR-008](./adr/ADR-008-widgets-layer-boundary.md) | `presentation/widgets/` 的收口标准（≥2 处重复 + 写错不报错 + 直觉写法看起来合理） |
| [ADR-009](./adr/ADR-009-frame-scheduler.md) | 帧循环定时任务的统一调度：登记表 + 唯一预约出口（`request_repaint_after` 只许出现在 `app/frame/scheduler.rs`） |

## 流水与素材（logs/ 与 pic/）

| 路径 | 内容 |
|---|---|
| [`logs/问题记录1.txt`](./logs/问题记录1.txt) | 用户反馈与缺陷修复的**原始流水记录**（001–056 + 066，**倒序** append-only）——调试经验已提炼至 `debug-guide.md` §6，细节追根溯源查这里 |
| `logs/示例文档.md` | 应用内置欢迎页 / 测试用 Markdown 样本 |
| `pic/` | 文档引用的截图与调试探针图（问题记录按编号引用） |

## 归档（archive/）

已完成的一次性文档（阶段报告 / 修复方案 / 实施日志 / 审计快照）在 [`archive/`](./archive/README.md)，
结论仍有效、可追溯，但不再维护。收录标准与逐篇索引见其 README。

---

_整理约定：文档只保留一个「家」——任务状态只进 backlog，数字只进 metrics，命令与排障只进
debug-guide，用户故障只进 troubleshooting；其余位置引用，不复制。_
