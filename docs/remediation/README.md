# 整改阶段档案（remediation）

> 建档：2026-10-28　｜　定位：**架构整改的执行期档案**——按整改批次分文件夹存放，
> 只收录「结论仍在指导后续判断」的方案与评估；已收官且结论被吸收的同类文档在
> [`../archive/remediation-plans/`](../archive/remediation-plans/)。
> 现行规范一律看 [`../architecture-constraints.md`](../spec/architecture-constraints.md)；
> 任务状态一律看 [`../v1.1-backlog.md`](../plans/v1.1-backlog.md)。

## 为什么单独成区

2026-10-27 ~ 10-28 的 R-01~R-11 整改产生了三类文档：**总方案**（跨批次）、**专项评估**（单批次）、
**专项方案**（单批次的实施设计）。后者在批次收官后即失去判据作用（R-04 / R-07 两份已归档），
但**总方案与 R-10 的评估不是**：

- 总方案是全仓唯一记载「R-01~R-11 每条具体怎么改、改了哪些文件、偏差为何」的文档；
- R-10 的两份评估承载的是**否定性结论**（强最大解不可达 / M1 成环禁 / M2 伪收敛）。
  否定性结论没有天然归宿——它不会变成代码、不会变成规则，一旦丢失，下一个人会重走一遍。

因此这三篇留在 `docs/` 侧，但**按整改批次分文件夹**，避免与「现行规范」混在同一层。

## 目录

| 路径 | 批次 | 内容 | 状态 |
|---|---|---|---|
| [`r01-r11/remediation-plan-2026-10-27.md`](./r01-r11/remediation-plan-2026-10-27.md) | **R-01 ~ R-11** | 架构整改细化实施方案：问题画像（root 维度 A~D / crate 内部 E~I）→ 目标 O1~O7 → 逐条改造项（每项含步骤 / 角色 / 验收 / 落地记录） | ✅ 全量收官（R-01~R-09 落地或闭项，R-10 项 A/B 于 10-28 落地，R-11 于 `7201e11`） |
| [`r10/evaluation.md`](./r10/evaluation.md) | **R-10** | 拆库 vs 端口升 crate：现状违规画像（文件:行号级）→ 四条候选路 → 结论「强最大解不可达」 | ✅ 已落地为 `rustmark-ports`（项 A，`0f20bc8`） |
| [`r10/max-solution.md`](./r10/max-solution.md) | **R-10** | 最大解（M0 端口 crate / M1 core→infra / M2 装配 crate / M3 core DI 边界）的设计与不可达论证 | ✅ M0 + M3 已落地（项 A `0f20bc8` / 项 B `a3b5160`）；M1 成环禁、M2 待第二消费者 |

## 阅读顺序

- **想知道当前架构为什么长这样** → `r10/evaluation.md` §1（违规画像）→ `r10/max-solution.md` §结论 → `../architecture-constraints.md` §10（crate 地图与归属四问）
- **想知道某条改造当初怎么做的** → `r01-r11/remediation-plan-2026-10-27.md` §3 对应条目
- **想领新任务** → 不要读这里，去 [`../v1.1-backlog.md`](../plans/v1.1-backlog.md)

## 纪律

1. 本区**只增不改**：批次收官后不再回改执行记录，纠正以新增段落显式标注（与 `../logs/问题记录1.txt` 同规）。
2. 新批次立项时按 `r{编号}/` 或 `r{起}-r{止}/` 建文件夹，**不要平铺进 `docs/` 顶层**。
3. 批次收官满一个评审周期、且结论已被 `architecture-constraints.md` / ADR 吸收后，
   移入 [`../archive/remediation-plans/`](../archive/remediation-plans/)。
