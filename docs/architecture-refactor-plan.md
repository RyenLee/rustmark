# rustmark 架构改造实施方案

- **上游依据**：[`docs/architecture-review.md`](./architecture-review.md)（2026-10-20 全库架构评估，基线 `e09ad2b`）
- **任务登记**：`docs/v1.1-backlog.md` 的 **V11-54**
- **编制日期**：2026-10-20
- **适用范围**：本方案覆盖评估报告 §2 中 P0 / P1 / P2 共 18 项主问题与 P3 低成本清理包；
  **不覆盖**性能优化主线（1MB p95 压进 16ms 属另一条线，见 `docs/metrics.md` §3.2.13）
  与 T7 平台真机回归（需外部机器）

> **文档说明（2026-10-22）**：本文各处「变异验证」引用的 `tools/mut_*.py` 一次性脚本已在
> Python 工具清理中删除（`tools/*.py` 26 → 5，其余为 CI 门禁与再生成脚本）。删除理由、
> 逐文件清单与备份位置见 [`python-cleanup-report.md`](./python-cleanup-report.md)。
> 正文保留当时的验证结论与轮次，只在脚本引用后标注「已清理」——它们是**当时的证据**，
> 不是可再执行的工具（锚点写死当年的源码片段，复跑只会得到「锚点漂移」的失败）。

---

## 目录

- [0. 本方案怎么用](#0-本方案怎么用)
- [1. 现状分析](#1-现状分析)
  - [1.1 架构与模块划分](#11-架构与模块划分)
  - [1.2 核心业务流程](#12-核心业务流程)
  - [1.3 主要问题归类](#13-主要问题归类)
- [2. 改造目标与范围](#2-改造目标与范围)
  - [2.1 改造目标（可验证）](#21-改造目标可验证)
  - [2.2 范围内](#22-范围内)
  - [2.3 明确不涉及](#23-明确不涉及)
- [3. 目标架构设计](#3-目标架构设计)
  - [3.1 总体结构](#31-总体结构)
  - [3.2 关键组件职责变更](#32-关键组件职责变更)
  - [3.3 交互关系与数据流](#33-交互关系与数据流)
- [4. 详细改造项](#4-详细改造项)
  - [4.1 P0 组：守卫失效与用户可见缺口](#41-p0-组守卫失效与用户可见缺口)
  - [4.2 P1 组：扩展性瓶颈与耦合](#42-p1-组扩展性瓶颈与耦合)
  - [4.3 P2 组：一致性与文档](#43-p2-组一致性与文档)
  - [4.4 P3 组：低成本清理包](#44-p3-组低成本清理包)
- [5. 实施计划](#5-实施计划)
  - [5.1 里程碑总览](#51-里程碑总览)
  - [5.2 各阶段任务顺序与交付物](#52-各阶段任务顺序与交付物)
  - [5.3 进度记录表（随实施填写）](#53-进度记录表随实施填写)
- [6. 技术选型与规范](#6-技术选型与规范)
  - [6.1 技术选型](#61-技术选型)
  - [6.2 目录与命名规范](#62-目录与命名规范)
  - [6.3 接口与编码规范](#63-接口与编码规范)
- [7. 测试与验证](#7-测试与验证)
  - [7.1 测试层次与范围](#71-测试层次与范围)
  - [7.2 通过标准](#72-通过标准)
  - [7.3 回归重点清单（改造后必须仍然成立的行为）](#73-回归重点清单改造后必须仍然成立的行为)
- [8. 风险与依赖](#8-风险与依赖)
  - [8.1 外部依赖](#81-外部依赖)
  - [8.2 上游 / 下游影响](#82-上游--下游影响)
  - [8.3 兼容性问题](#83-兼容性问题)
  - [8.4 安全问题](#84-安全问题)
  - [8.5 方案级风险](#85-方案级风险)
- [附录 A · 改造项索引表](#附录-a--改造项索引表)

---

## 0. 本方案怎么用

每条改造项采用**同一套七字段**描述，缺一不可：

| 字段 | 作用 | 写法要求 |
|---|---|---|
| **改造内容** | 做什么 | 必须落到「改哪个文件的哪个符号」，不接受「优化 X」这类表述 |
| **涉及文件** | 改哪里 | 给出真实路径；新增文件写明归属目录与命名 |
| **影响范围** | 波及多大 | 列出受影响的调用点数量与用户可见行为变化 |
| **依赖关系** | 先后顺序 | 明确前置项（写 R-xx）与可并行项 |
| **风险点** | 会怎么坏 | 至少一条，且必须是**本改造特有的**（不是通用套话） |
| **回滚方案** | 坏了怎么办 | 必须是**可执行的动作**（`git revert` 单个 commit / 开关 / 保留旧实现并行） |
| **验收判据** | 怎么算做完 | 必须是**可机械检查**的：测试名、命令、数字、规则命中数 |

**执行纪律**（沿用项目既有约定）：

1. 每条改造项**独立成一个 commit**，commit message 写 `refactor(scope): 做了什么（依据 R-xx）`；
2. 每条改造项完成后**当场跑四道门禁**并把**当日实测命令 + 数字**写进 commit message——
   不接受「已验证」这种无数字的表述；
3. 涉及用户可见行为的改造，必须有**真帧或领域级回归**，不接受只有纯函数单测；
4. 涉及守卫规则的改造，必须做**变异验证**（把守卫改坏，用例必须当场变红）。

---

## 1. 现状分析

### 1.1 架构与模块划分

四层单向依赖 + 共享内核 + 组合根，由 `tools/arch_check.rs` 文本扫描强制：

```text
presentation ──► application ──► domain ◄── infrastructure
    │  (32 处)      │  (单向)        ▲
    │               │                │ (trait 实现反向注入, 9 处)
    ▼               ▼                │
    └────── app（组合根）◄───────────┘
                   │
                   └──► presentation::views::main_view::show  (1 处)
```

**实测规模**（`src/` 302 文件 / 33 845 行）：

| 层 | 文件 | 行数 | 占比 | 主要子目录 |
|---|---:|---:|---:|---|
| `presentation` | 123 | 31 534 | **56%** | `views/` 48 · `state/` 24 · `rich/` 25 · `theme/` 9 · `shortcuts/` 8 · `widgets/` 2 · `icons.rs` |
| `infrastructure` | 47 | 10 221 | 18% | 持久化 / 导出 / 解析 / 高亮 / 拼写 / 字体 / PDF |
| `domain` | 46 | 7 282 | 13% | document / markdown / workspace / preferences / export |
| `application` | 43 | 6 201 | 11% | 用例编排 + DTO + 查询 / 命令 |
| `app` | 20 | 6 146 | 11% | `frame.rs` 737 + `frame/` 16 文件 5 325 |
| `shared` | 14 | 1 890 | 3% | 值对象 / trait 契约 / 工具 |

**已验证健康的部分**（改造时应保持，不得回退）：

- `domain` → `infrastructure` **0 处**；`application` → `infrastructure` **0 处**；
- `application` 的 trait（`DocumentContentProvider`）签名只含 `DocumentId` / `String` / `FilePath` / `u64`，
  **不含 domain 类型** → 不存在 application ↔ domain 的反向依赖；
- 基础设施的 9 处 trait 实现（`fs_document_repository.rs:268`、`fs_workspace_repository.rs:195`、
  `toml_preferences_repository.rs:72`、`rope_text_buffer.rs:299`、3 个 exporter）**全在 infrastructure 侧**；
- `presentation/rich/` 对 `egui` / `theme` / `state` **零引用**（ADR-007 的边界有编译期保障）；
- 全仓 `TODO` / `FIXME` / `HACK` / `XXX` **0 处**；域层 `clippy::todo` 已 `#![deny]`（`src/domain.rs:14-25`）。

**结构性软肋**（本方案要处理的对象）：

| 软肋 | 实测 | 后果 |
|---|---|---|
| **守卫自身有绕过形态** | `tools/arch_check.rs:214` 只识别行首 `use `，实测 4 类绕过成立；`tests/` 完全不在扫描范围 | 架构规则可被无声违反 |
| **`presentation ↔ app` 双向环** | presentation→app **32 处 / 32 文件**；app→presentation **1 处**（`frame.rs:601` 调 `main_view::show`） | 无规则覆盖；状态所有权真空（presentation 直写 app 状态字段 **59 处**，app 的 state 有 **150 个 `pub` 字段**） |
| **表现层内部无规则** | `views/` ↔ `state/` 靠约定；本轮新增 `widgets` / `rich` / `icons` / `FrameScheduler` 四个抽象层，但没给表现层内部定方向 | 31 534 行只靠约定 |
| **`shared` 语义被侵蚀** | `shared/system.rs:13-36` 直接 `Command::new(...).spawn()`；文件头自认「`std::process` 不在其禁止清单，故在此落位」 | 共享内核不再是「零 IO」，文档与实现矛盾 |

### 1.2 核心业务流程

四条主流程及其**代码落点**（据代码核实，非推测）：

**① 打开文档** — 四个入口（文件树双击 / 命令面板 / 拖放 / 快捷键）→ 统一走
`app/frame/workspace.rs:298` `open_workspace_file`：白名单守卫（`files.rs:400` `can_open_as_text`）
→ 去重（`:324`）→ 打开数上限（`:353`）→ `FilePath::new` → `documents.open` →
领域 `Document` 聚合 → 事件总线发 `DocumentOpened` → 标签条 / 编辑区 / 预览区各自响应。

**② 编辑与保存** — egui `TextEdit` → 帧内 diff（`app/frame/editor.rs`）→ 领域命令
（`application/document/service/`）→ 撤销栈 → 两种持久化：
显式保存（`files.rs:193` `save_document`）与自动保存草稿（`files.rs:311` `run_autosave` → `save_draft`）。
自动保存依赖**帧循环定时任务**，由 T11 的 `FrameScheduler` 统一预约（`app/frame/scheduler.rs`）。

**③ 渲染** — 编辑区：每帧 `revision_and_len`（O(1)）→ 防抖判定（`HighlightPace`）→
增量高亮（`infrastructure/highlighting/incremental.rs`）→ `Galley` 缓存
（`presentation/theme/mapper/highlight_cache.rs`）→ 排版（`highlight_layout.rs`，A1-4 单行补丁）→ 绘制。
预览区：`pulldown-cmark` 解析 → 块级渲染（`presentation/views/preview_view/`）。

**④ 帧循环与定时** — `eframe::App::ui`（`app/frame.rs:527`）→
状态更新段（主题 / 字体 / 对话框回执 / 拖放 / 快捷键 / 自动保存 / 高亮到期点）→
`main_view::show` 绘制 → **帧末 `FrameScheduler::flush` 统一预约下一帧**（T11）。

### 1.3 主要问题归类

| 类别 | 条目 | 对应改造项 |
|---|---|---|
| **守卫失效** | `check-arch` 4 类绕过；`tests/` 不在扫描范围 | R-01 / R-02 |
| **职责耦合** | `presentation ↔ app` 双向环 + 59 处直写；表现层内部无规则；`shared` 内进程调用 | R-03 / R-11 / R-12 / R-13 |
| **重复实现** | `floor_char_boundary` 两份；span→`LayoutJob` 三份 + 样式表两张；路径/文件名 5 种写法；`capture.rs`↔`files.rs` 逐字重复 | R-10 / R-14 / R-15 / R-21 |
| **扩展性瓶颈** | 导出格式双枚举 + 手写字符串 `match`；`persisted_key` 漏加不报错；主题双枚举 + 第三份映射；格式白名单硬编码漏网 | R-07 / R-08 / R-09 / R-16 |
| **可维护性** | `files.rs`↔`workspace.rs` 打开管线双实现且依赖方向与命名相反；三个「取当前文档路径」方法并存；`views/` 拆分阈值两套标准 | R-21 / R-23 |
| **测试缺口** | 1 条无区分力判据；`fonts.rs` 测试依赖真实资产（干净克隆会失败） | R-20 / R-21 |
| **文档缺口** | 过期 8+ 处；ADR 缺 5 项 | R-18 / R-19 |
| **用户可见缺口** | `Revert` / `InsertImage` 无响应；写作目标无入口；切标签光标归零 | R-04 / R-05 / R-06 |

---

## 2. 改造目标与范围

### 2.1 改造目标（可验证）

| # | 目标 | 验证方式 |
|---|---|---|
| G1 | 架构规则**拦得住**：4 类绕过形态全部被检出 | `tests/arch_violation_fixture.rs` 新增 4 条注入用例，逐条「注入后必须报错」 |
| G2 | 依赖方向**有唯一真源**：`presentation` 不再直接写 `app` 的状态字段 | 机械规则零命中；`scheduled_deadlines` 类的状态写入口收敛为方法调用 |
| G3 | 扩展性瓶颈**降到编译期暴露** | 「新增一种导出格式 / 可持久化动作 / 内置主题」各写一条「漏加即编译失败」的用例 |
| G4 | 重复实现**收敛到单一真源** | `grep` 计数：同名实现从 2/3 份降为 1 份；`floor_char_boundary` 从 2 处降为 1 处 |
| G5 | 文档**与代码同源** | `docs/README.md` 的 ADR 索引、问题记录编号、版本号、测试数与实测一致 |
| G6 | 三处用户可见缺口**闭环** | 每项一条真帧或领域级回归 |
| G7 | 门禁**保持四绿** | 每条改造项完成时 `fmt` / `check-arch` / `clippy -D warnings` / `test` 全绿并附实测数字 |

### 2.2 范围内

- `tools/arch_check.rs`（守卫判定逻辑 + 新规则）
- `src/shared/`（`system.rs` 端口化；`text_format.rs` / `lru.rs` / `text.rs` 归位）
- `src/app/frame/`（状态所有权、`files.rs`↔`workspace.rs` 职责归位、动作接线、路径处理）
- `src/presentation/state/`（`export_ui_state/choices.rs` 双枚举收敛；state 字段的写入口收敛）
- `src/presentation/views/`（`sidebar_view/search.rs` 收口；`preview_view/code.rs` / `inline.rs` 统一 span 应用）
- `src/presentation/shortcuts/`（`persisted_key` 穷举、默认键位表合一）
- `src/presentation/theme/`（样式表合并、`mapper`/`inline` 归一）
- `src/presentation/icons.rs` / `src/presentation/widgets/`（命名与组件扩充）
- `src/infrastructure/fonts.rs`（拆分）
- `docs/`（`README.md`、`testing.md`、`PROGRESS.md`、新增 ADR）

### 2.3 明确不涉及

| 不涉及 | 理由 |
|---|---|
| **性能主线**（1MB p95 压进 16ms、增量统计） | 属 `docs/metrics.md` §3.2.13 的另一条线；本次改造**不得**引入每帧回归 |
| **T7 平台真机回归** | 需外部 macOS / Linux 机器 |
| **crate 拆分** | ADR-007 已定：不拆（编译隔离是伪收益） |
| **`presentation/rich/` 内部重构** | ADR-007 的边界已验证（零外部依赖），本期不动 |
| **换 egui / 换渲染栈** | ADR-001 已冻结 |
| **新增第三方 crate** | 本方案全部改造**零新增依赖**（含 R-03 的端口化——`std::process` 只是搬家） |

---

## 3. 目标架构设计

### 3.1 总体结构

改造后**四层结构不变**，变的是三件事：

```text
① 守卫变严      check-arch 从「行首 use 扫描」→ token 级匹配 + 覆盖 tests/
② 状态所有权    presentation 只读 app 状态 + 调方法改；app 独占写权
③ 端口化        shared 只留值对象与 trait 契约；一切 IO 落 infrastructure
```

**（一）守卫的判定模型**

现状（`tools/arch_check.rs:214`）：

```rust
let Some(rest) = line.strip_prefix("use ") else { continue; };
```

目标：**行级 → token 级**。对每一行：

```text
去掉行注释（// 之后）与字符串字面量（"..."）
  ↓
提取全部路径 token：crate::a::b::C / egui / eframe / rfd / ...
  ↓
逐 token 与该层的禁止清单比对（保留前缀匹配语义）
```

这样 `pub use`、`use crate::{…}`、类型位置全路径、函数体内 `use`、全路径调用**一次全覆盖**。
`arch-allow:` 豁免语义从「整行跳过」收紧为「只豁免该行中指定的那个 crate」。

**（二）状态所有权模型**

| 层 | 对 UI state 的权限 | 落地方式 |
|---|---|---|
| `app` | **唯一写者** | `app` 层提供语义化方法（如 `export_ui.begin_pick()`） |
| `presentation` | **只读 + 调方法** | 视图不再出现 `app.export_ui.error = …` 这类直写 |

**注意**：`presentation` → `app::EditorApp` 的**类型依赖保留**（egui 的 `App::ui` 契约要求
视图拿到 app 才能拿到状态），被禁的只是**字段直写**。规则形态：

```text
presentation 层不得出现  app.<state字段>.<字段> = …   （赋值形态）
```

**（三）端口化模型**

`shared/system.rs` 的进程调用改为一对 trait + 实现，与 `shared/dialog.rs:55` 的 `DialogPort` 完全同构：

```text
shared::system::ExternalOpener  (trait, 只含 &str → io::Result<()>)
        ↑ 实现
infrastructure::system::SystemExternalOpener   (Command::new(...).spawn())
        ↑ 注入
app::bootstrap::Services  持有 Arc<dyn ExternalOpener>
        ↓ 传入
presentation::views::preview_view  调 opener.open(url)
```

### 3.2 关键组件职责变更

| 组件 | 现状职责 | 改造后职责 | 涉及项 |
|---|---|---|---|
| `tools/arch_check.rs` | 扫 `use` 语句 | 扫**路径 token**；新增 4 类形态覆盖 + `tests/` 范围 + 2 条新规则 | R-01 / R-02 / R-12 / R-13 |
| `shared::system` | 直接 spawn 进程 | 只定义 `ExternalOpener` trait | R-03 |
| `app::frame::EditorApp` | 42 字段 / 183 方法，视图可直写其状态 | 42 字段不变，但**所有 UI state 字段的写入口收敛为方法** | R-11 |
| `app::frame::actions` | 路由表 + UI 状态初始化混做 | 路由表**只做路由**；状态初始化下沉到对应 `app/frame/*` 模块 | R-11 / R-04 |
| `shared::export_format::ExportFormat` | 领域枚举（`domain::export::value_objects` 转出） | **唯一**格式真源（落在 `shared`，三方共同可达）；新增 `parse` 穷举实现 | R-07 |
| `presentation::state::export_ui_state::ExportFormatChoice` | 与领域枚举同构的第二套枚举 | **已删除**；UI 侧改用真源枚举 + `format_hints` 独立函数表（`pdf_caveat` 等 UI 专属数据） | R-07 |
| `shared::theme::ThemeKind` | 领域枚举（`domain::preferences::value_objects` 转出） | **唯一**主题种类真源（落在 `shared`，三方共同可达）；`parse` 只认得/不认得，兜底交调用点 | R-09 |
| `presentation::theme::builtin::BuiltinTheme` | 与领域枚举同构的第二套枚举（3 变体，无 `Custom`） | **已删除**；表现层改用真源 + `labels::theme_kind_label` 独立函数（UI 专属显示名） | R-09 |
| `presentation::theme::mapper` | 样式 → `FontId`/颜色映射 | **唯一**样式真源；`preview_view/inline.rs` 的 `span_format` 并入 | R-10 |
| `presentation::theme::mapper::highlight_layout` | 排版 + span 应用 | 抽出**唯一** span→`LayoutJob` 构造器，`code.rs` / `inline.rs` 改调它 | R-10 |
| `application::shortcuts` | 领域 `ActionId` + 默认键位表 | 默认键位表**唯一一份**（领域侧），表现层读取 | R-08 ②（**未做**） |
| `presentation::shortcuts::registry` | `UiAction` + `persisted_key` 兜底 | `persisted_key` **穷举**（不可持久化的显式列出）——**已达成**（`8cdf083`）；手写清单 `UiAction::ALL` 与枚举变体的同步由源码级守卫钉住 | R-08 ① |
| `infrastructure::fonts` | 发现 + 装配 1111 行 | 拆为「路径发现（可测）」+「装配（可测）」+「真实 FS 薄适配」 | R-20 |

### 3.3 交互关系与数据流

**（一）打开文档（改造后，R-11 影响）**

```text
入口(4) ──► app::frame::workspace::open_workspace_file
                 │  白名单 / 去重 / 限额           ← R-15 统一路径处理
                 ▼
             application::document::open ──► domain::Document
                 │
                 ├──► 事件总线 DocumentOpened
                 ├──► presentation::views::editor_view   （只读 app 状态）
                 └──► presentation::views::preview_view
```

**（二）帧循环（改造后，R-12/R-13 影响）**

```text
eframe::App::ui
  ├─ 状态更新段（app 独占写 UI state）
  ├─ main_view::show → views 只读 app 状态 + 调 app 方法改
  │     └─ presentation::state::*  ← 视图不得直写 app.<state>.<field>
  └─ 帧末 FrameScheduler::flush → 一次 request_repaint_after
```

**（三）导出格式（改造后，R-07）**

```text
UI 选格式 ──► ExportFormat（领域唯一枚举）
                │  ExportFormat::from_str（穷举，漏加编译失败）
                ├──► presentation 读 label()/extension()/supports_toc()（直接调领域方法）
                └──► infrastructure::export::routing_exporter::match（穷举）
```

---

## 4. 详细改造项

### 4.1 P0 组：守卫失效与用户可见缺口

#### R-01 · `check-arch` 判定改为 token 级匹配

- **改造内容**：把 `tools/arch_check.rs:214` 的「行首 `use `」判定替换为 token 级提取。新增内部函数 `extract_path_tokens(line: &str) -> Vec<&str>`：
  1. 去掉行注释（`//` 之后）与字符串字面量；
  2. 用词法扫描提取 `crate::a::b::C` 形式的路径 token 与裸 crate 名（`egui` / `eframe` / `rfd` / `syntect` / `pulldown_cmark` / `anyhow` / `std::fs` / `std::io` / `std::process`）；
  3. 保留现有「前缀匹配」语义（`crate::domain::x` 命中 `crate::domain`）。
  `arch-allow:` 从「整行跳过」改为「只豁免该行中写明的那个 crate」。
- **涉及文件**：`tools/arch_check.rs`（判定核心，约 +80 行）；`tests/arch_violation_fixture.rs`（新增 4 条注入用例）
- **影响范围**：`build.rs`（每次编译）与 `cargo run --bin check-arch` 的判定结果会变严。当前全库 `pub use` 均为同层 `self::` 重导出、`presentation` 内 `crate::domain::` 的 2 处命中全在 doc 注释里 → **存量违规预计为 0**，但需实测确认。
- **依赖关系**：无前置。**R-12 / R-13 的新规则依赖本项的 token 提取能力** → 必须先做。
- **风险点**：
  - 误报——doc 注释里提到 `crate::domain::…` 的 2 处（`preview_ui_state.rs:29`、`welcome_view.rs:57`）若未被「去注释」逻辑排除，会立刻变红；
  - 字符串字面量里的路径（如错误信息里写 `crate::domain::…`）同理；
  - `std::process` 若加入 `SHARED_FORBIDDEN` 会命中 `shared/system.rs`——**必须等 R-03 落地后再加**（顺序约束）。
- **回滚方案**：`git revert` 该 commit（单点改动，无数据迁移）。若需要临时放宽，把 `extract_path_tokens` 的调用点用 `if false` 短路即可回到旧行为。
- **验收判据**：
  - `tests/arch_violation_fixture.rs` 新增 4 条用例：`pub use egui::Context;`、`use crate::{domain::x::Y};`、`fn f(d: crate::domain::x::Y) {}`、`fn f() { use egui::Context; }` —— **每条都必须让 `scan()` 报出违规**；
  - 变异验证：把 `extract_path_tokens` 改成返回空 vec → 4 条注入用例**同时变红**（证明用例有牙齿）；
  - `cargo run --bin check-arch` 仍输出 OK。

##### 实施结果（2026-10-20）

**已落地，但实现形态与原文有出入——原文的三处描述已被实测证伪。**

| 原文描述 | 实际落地 | 原因 |
|---|---|---|
| 新增 `extract_path_tokens(line: &str) -> Vec<&str>`，逐行提取 | **整文件词法 + use-tree 展开**：`Lexer` 把源码切成 token 流（剔除行/块注释、字符串、raw string、字符字面量），`parse_use_tree` 把 `use` 声明展开成完整路径 | 逐行处理**根本覆盖不了跨行 `use`**——而跨行 `use` 是合法 Rust 且大量存在 |
| 保留现有「前缀匹配」语义 | 改为**路径段前缀**匹配 | 字符串前缀会把 `crate::domainfoo` 误判成 `crate::domain`；实测这条误伤真实存在（`similar_prefix_crate_is_not_matched`） |
| `arch-allow:` 只豁免该行写明的那个 crate | 额外加了**豁免登记表**校验：豁免未登记 → 报「豁免未登记」；该行无对应违规 → 报「豁免无效」 | 「只豁免写明的对象」仍不够：随手写一行豁免注释就能放行，等于没有豁免机制 |

**实测额外发现的 8 类漏检**（原文完全未提，全部已修）：

1. `pub use` / `pub(crate) use` 整类逃逸（旧实现只认「行首 `use `」）；
2. 嵌套花括号 `use crate::{domain::x::Y}` 逃逸；
3. 跨行 `use` 逃逸；
4. 别名 `use egui as e;` 逃逸；
5. 层根文件 `src/domain.rs` 逃逸（旧实现按路径首段判定，首段是 `"domain.rs"`）；
6. `src/app/` **整层逃逸所有规则**——包括帧预约唯一出口（旧实现的 `layer_of("app/...")` 返回 `""`，扫描函数在**所有**检查之前就 `continue` 掉了）；
7. `crate::app` 在任何层的禁止清单里**都不存在**——组合根可被下层反向依赖；
8. `print!`（无 `ln`）漏网；`eprintln!` 被当成 `println!` 报错（文案不准）。

**另修 3 处实现缺陷**（原重写版自身的问题，均由本轮新写的注入用例暴露）：

- **行号恒为 `:1`**：裸换行走 catch-all 分支时用 `self.pos += len` 跳过，未计入行号 → 所有违规都报第 1 行；
- **块注释嵌套深度差一**：`depth` 从 0 起算（开头的 `/*` 已被消费却没计数）→ 遇到第一个 `*/` 时 `usize` 下溢 panic，且注释**之后**的代码会被整段吞掉；
- **空白被当成 token**：catch-all 把空格也 push 进 token 流 → `use` 与被导入项之间多出一个 `Other` → `parse_use_tree` 读到「`use` 后面不是标识符」→ **整条 `use` 声明静默漏检**。

**一处「不可一刀切」的设计**：`crate::app` 禁令**不能**全层平推。实测 54 个表现层文件依赖 `crate::app::frame::EditorApp`（组合根把状态句柄交给视图，是既定模式）。一刀切禁掉 = 造一条没人能遵守的规则 → 例外蔓延成惯例 → 连真正要拦的反向依赖也一起淹掉。故改为**更窄豁免前缀**：表现层放行 `crate::app::frame`，但 `crate::app::bootstrap` / `crash_log` / `dialog_runner` / `startup_timing` 仍被拦。

**顺带清理**：删除死代码 `signature()` / `file_stamp()`（`build.rs` 从未调用，且与「已移除内部缓存」的文档记载自相矛盾）；报告格式收敛到单一来源 `format_report`，`build.rs` 与 `check-arch` bin 共用。

**顺带修掉的第 4 个不合理点：规则真源不入库（CI 死锁）**

引入「豁免登记表」后暴露：`tools/arch_check.rs` 的 `load_registry` 要读
`<manifest>/.agent/architecture.md`——而**整个 `.agent/` 目录在 `.gitignore` 里**
（第 26 行，理由是「本地运行时/智能体数据，含本机绝对路径」）。后果有两条：

1. **CI 上豁免机制必然失效**：`.github/workflows/ci.yml` 跑 `cargo run --bin check-arch`，
   CI 机器上没有 `.agent/architecture.md` → 登记表读不到 → 任何 `arch-allow:` 都报
   「豁免未登记」→ **且无法通过提交修复**（改的是本地那份，CI 读不到）；
2. **规则随代码失传**：这份文档被 `src/` 下 10+ 个模块的 doc 注释、`CONTRIBUTING.md`、
   `docs/01`、`docs/02`、ADR 引用，是**代码契约**；拿到代码的人却拿不到它。

实测 `.agent/architecture.md` **不含任何本机绝对路径**——gitignore 给它的理由对它不成立。

处置：把真源迁到 **`docs/architecture-constraints.md`**（入库），`.agent/architecture.md`
改为一行指针；全仓 20 个文件、26 处引用同步更新（含 `src/` 下 10 个模块的 doc 注释）。
「一个事实一个家」得以保持：规则正文只有一份，且**必须随代码走**。


**用例与变异验证**：原计划 4 条注入用例 → 实际 **45 条**（`tests/arch_violation_fixture.rs`），按「注入必报 / 注入不报 / 豁免三态 / 词法层」分节。变异验证 **10 轮**，逐条确认用例不是恒绿：

| 变异 | 变红的用例数 |
|---|---|
| A 候选收集整体失能 | 23 |
| B `use` 声明不展开路径 | 12 |
| C 路径匹配退回字符串前缀 | 3 |
| D `layer_of` 不剥 `.rs` | 1 |
| E 帧预约检查放过 `app/` | 1 |
| F 表现层不再放行 `crate::app::frame` | 2 |
| G 豁免退回「整行放行」 | 1 |
| H 打印宏只认 `println!` | 2 |
| I 豁免登记表校验失效 | 1 |
| J 过时豁免不报 | 2 |

> 值得记一笔：变异 A（校验器完全失能）下，**「真实工程零违规」那两条守卫用例仍然绿**。
> 「零违规守卫」单独用是不够的——它证明不了校验器还在工作。必须配合注入用例。

**范围外**：R-01 原文提到的 `std::process` 未加入 `SHARED_FORBIDDEN`（那是 R-03 的内容，顺序约束不变）。


#### R-02 · `tests/` 纳入架构扫描 + 扩充 fixture

- **改造内容**：`arch_check::scan()` 增加一个可选的「额外根目录」参数（`scan(src_root)` 保持签名不变以免破坏 `build.rs` 调用，新增 `scan_with_tests(src_root, tests_root)`），把 `tests/` 纳入扫描。`tests/` 的层级判定放宽：测试可以引用任何层（测试是**消费者**，不是层），但**禁止** `tests/` 里出现 `use crate::domain::…` 这类**以为在测库内部**的写法——实际现状是 `tests/` 对 domain/infrastructure 引用数 = 0，规则是「防止测试绕过公开 API 偷看内部」。
- **涉及文件**：`tools/arch_check.rs`（`scan` 签名与层判定）；`src/bin/check-arch.rs`（传入 `tests/` 根）；`build.rs`（同步传参）；`tests/arch_violation_fixture.rs`
- **影响范围**：`tests/` 109 个文件全部进扫描。构建期扫描量增加约 35%（`tests/` 约 1.1 万行 vs `src/` 3.4 万行）。
- **依赖关系**：**依赖 R-01**（token 提取是共同基础）。
- **风险点**：`build.rs` 在每次编译时跑扫描，扫描范围扩大 → 编译变慢；且 `tests/` 里若存在**故意**引用内部符号的测试（当前实测为 0），会误报。
- **回滚方案**：`scan_with_tests` 不被 `build.rs` 调用即可回到旧行为（一行注释 / 参数默认值 `None`）。
- **验收判据**：
  - `tests/arch_violation_fixture.rs` 新增一条：「在 `tests/` 注入 `use crate::domain::x::Y;` → `scan_with_tests` 必须报出违规，而 `scan` 不报」；
  - `cargo run --bin check-arch` 仍 OK；
  - 记录构建期耗时的前后对比数字（`cargo build` 的 `Finished in` 时间，取 3 次中位数）。

##### R-02 实施结果（2026-10-20）

- **新增 `scan_with_tests(src_root, tests_root)`**，`scan(src_root)` 签名保持不变
  （方案原文要求）。两者共用 `scan_impl(src_root, Option<&Path>)`。
- **`tests/` 侧层判定统一为 `Layer::Tests`**（新增变体），不按目录名判层：
  `tests/domain/`、`tests/presentation/` 是**测试目标的划分**，与 `src/` 的层
  毫无关系。若按目录名判层，`tests/presentation/main.rs` 会被套上表现层禁令、
  `tests/domain/main.rs` 会被套上领域层禁令——那是**误判**。
- **`TESTS_FORBIDDEN` 只列库层名**（`crate::app` / `domain` / `application` /
  `infrastructure` / `shared` / `presentation`），天然放行 `crate::support` /
  `crate::golden_fixture` 这类**测试自身**的共享模块——那是集成测试的正常组织
  方式（全仓 243 处 `crate::support::…`）。若误改成禁整个 `crate::`，
  243 处会同时误报，用例 `tests_may_reference_their_own_support_modules` 当场红。
- **报告里 `tests/` 侧文件带 `tests/` 前缀**：与 `src/presentation/…` 等同名路径
  可区分（否则两个 `presentation/main.rs` 在报告里无法分辨）。
- **与层无关的通用规则照样罩住 `tests/`**：帧预约唯一出口（T11 / ADR-009）
  在测试侧同样生效——用例 `repaint_rule_still_applies_inside_tests` 钉住。

**新增守卫 8 例**（fixture 63 → 71 条）：

| 用例 | 钉住什么 |
|---|---|
| `tests_directory_has_no_violations` | 存量守卫：真实工程连 `tests/` 一起扫必须干净 |
| `injected_crate_layer_path_in_tests_is_detected` | **方案点名判据**：`scan` 不报、`scan_with_tests` 报（两个入口都断言） |
| `tests_may_reference_their_own_support_modules` | `crate::support` / `crate::golden_fixture` 放行 |
| `every_library_layer_name_is_forbidden_in_tests` | 六个库层名逐一拦（名单漏一项不报错、没人注意） |
| `the_narrow_allow_prefix_does_not_apply_to_tests` | 表现层的 `crate::app::frame` 窄豁免**不**外溢到测试侧 |
| `qualified_path_outside_use_is_detected_in_tests` | 非 `use` 位置的限定路径同样拦（R-01 词法能力对测试侧成立） |
| `crate_layer_paths_inside_strings_and_comments_are_not_violations_in_tests` | 字符串/注释里的路径不是依赖（fixture 自身 41 处字符串即试金石） |
| `repaint_rule_still_applies_inside_tests` | 帧预约规则不因测试豁免 |

**变异验证 6 轮，全部捕获**（脚本 `mut_arch_r02.py`，已清理；从干净基准重新生成 +
编译失败单独识别）：

| 变异 | 结果 |
|---|---|
| `TESTS_FORBIDDEN` 清空（tests/ 名义纳入实际不拦） | 捕获 |
| 名单漏掉 `crate::app` | 捕获 |
| `scan_with_tests` 退化成 `scan` | 捕获 |
| tests 侧文件不带 `tests/` 前缀 | 捕获 |
| Tests 层文案不给正确写法（`rustmark::…`） | 捕获 |
| 表现层窄豁免前缀扩到 Tests 层 | 捕获 |

**构建期耗时**（`cargo build` 的 `Finished in`，各 3 次取中位数；
每轮先 `touch src/lib.rs` 强制重跑 build script，否则测不到扫描）：

- 纳入前中位数 **5.84s**；纳入后中位数 **6.11s**；差值 **+0.27s（+4.6%）**。
- 方案原文预估「扫描量增加约 35%」（指**扫描量**，非总构建耗时）——
  实测扫描量增幅符合预期，但摊到总构建耗时上只有 4.6%。

**存量摸查结论**（动手前实测，非推断）：`tests/` 下 `crate::<库层名>` 的
**真实引用为 0**——`grep` 命中的 2 处全在文档注释里（`//!`），
`arch_violation_fixture.rs` 里的 41 处全在字符串字面量中（词法阶段剔除）。
故「直接纳入并失败」不会误报，无需「只报告不失败」过渡。

#### R-03 · `shared/system.rs` 端口化

- **改造内容**：
  1. `src/shared/system.rs`：删除 `use std::process::Command` 与 `open_external` 的实现，改为
     ```rust
     pub trait ExternalOpener: Send + Sync {
         fn open(&self, target: &str) -> std::io::Result<()>;
     }
     ```
     并保留 `open_external` 的**链接安全校验**部分（`is_safe_to_open`）——**校验留在 shared（纯逻辑），执行移到 infrastructure**。
  2. 新增 `src/infrastructure/system.rs`：实现 `SystemExternalOpener`，把现有 `Command::new("cmd"/"open"/"xdg-open").spawn()` 三平台分支整体搬过去。
  3. `src/app/bootstrap.rs`：`Services` 持有 `Arc<dyn ExternalOpener>`（默认 `SystemExternalOpener`）。
  4. `src/presentation/views/preview_view.rs`（及 `preview_view/` 下用到外链的子文件）：改为经 `app.services.opener` 调用。
  5. `tools/arch_check.rs`：`SHARED_FORBIDDEN` 增加 `std::process`。
- **涉及文件**：`src/shared/system.rs`（改写）、`src/infrastructure/system.rs`（新增）、`src/infrastructure.rs`（加 `pub mod system;`）、`src/app/bootstrap.rs`、`src/presentation/views/preview_view*.rs`、`tools/arch_check.rs`
- **影响范围**：1 个 trait + 1 个实现 + 1 个注入点；`shared` 从「含进程调用」恢复为「零 IO」；用户可见行为**不变**（仍是系统默认程序打开）。
- **依赖关系**：**必须在 R-01 之后**（`std::process` 进 `SHARED_FORBIDDEN` 会命中当前实现）。
- **风险点**：
  - `preview_view` 在 0.37 的 `open_url` feature 未启用（见 `shared/system.rs:2-4`）→ 打开链接是**唯一的**外链出口，端口化时若漏改某处会编译失败（**这是好事**：编译期暴露）；
  - 链接安全校验若随执行一起搬走，会让 shared 重新含 IO；**必须把校验留在 shared**。
- **回滚方案**：`git revert`；或保留 `shared::system::open_external` 作为薄封装（`#[deprecated]`），1 个版本后再删。
- **验收判据**：
  - `grep -rn "std::process" src/shared/` **零命中**；
  - 新增单测：用假 `ExternalOpener` 断言「预览区点击外链时调用了 `open` 且参数正确」——**当前无法测**（spawn 真进程），改造后可测；
  - 变异验证：把 `preview_view` 的调用改回直连 `shared::system::open_external` → `cargo check` 报 `shared` 不得依赖 `std::process`（R-01 的新规则命中）。

##### R-03 实施结果（2026-10-20）

按方案 5 步落地，范式与 `DialogPort` / `Clock` **完全一致**（端口在
`shared`、实现在 `infrastructure`、组合根装配）：

1. `src/shared/system.rs`：删 `use std::process::Command` 与 `open_external`
   实现，新增 `pub trait ExternalOpener: Send + Sync`；
   **`is_safe_to_open` 原样留下**（纯逻辑，方案点名「校验留在 shared」）。
2. 新增 `src/infrastructure/system.rs`：`SystemExternalOpener`，三平台
   `Command::new(...).spawn()` 分支**整体搬迁**（含 Windows `cmd /C start ""`
   的标题占位注释与不支持平台的 `Unsupported` 错误）。
3. `src/app/bootstrap.rs`：`Services` 加 `opener: Arc<dyn ExternalOpener>`。
   装配链新增最深层 `assemble_with_opener(paths, clock, dialogs, opener)`；
   `assemble_with_clock_and_dialogs` 转发到它并固定用 `SystemExternalOpener`
   ——**既有调用点一个都不用改**（那条链被测试广泛使用）。
4. `preview_view/inline.rs`（行内链接）与 `preview_view/media.rs`（远程图片
   加载失败的「在浏览器中打开」按钮）改经 `app.services.opener.open(...)`。
5. `tools/arch_check.rs`：`SHARED_FORBIDDEN` 增加 `std::process`。

**新增守卫 9 例**：

- `tests/presentation/external_opener_test.rs`（**5 例**，方案第 2 条判据）：
  假 opener 断言「点了安全外链→调 `open` 且参数是那个地址」「4 种 XSS
  向量一次都不调」「`#锚点` 不惊动外部打开器」「无 url 的 link 是 no-op」
  「`file://` 与相对路径逐一打开」。**改造前这个行为无法测**——
  `open_external` 真起 `cmd` 进程，既不幂等也没法观察。
- `tests/arch_violation_fixture.rs`（**4 例**）：`shared` 里 `std::process`
  必报 / 端口 trait 与纯校验在 `shared` 放行 / 薄封装也算违规 /
  `infrastructure` 里放行。

**变异验证 7 轮全部捕获**：

| 变异 | 结果 |
|---|---|
| `shared` 留 `open_external` 薄封装（**方案点名**） | 捕获 |
| `SHARED_FORBIDDEN` 丢掉 `std::process` | 捕获 |
| 禁令误扩到 `infrastructure` | 捕获（build.rs 先拦下） |
| 丢掉 `is_safe_to_open` 校验 | 捕获 |
| 校验反相（只放行危险地址） | 捕获 |
| 参数传错（把整段内容而非 url 交给 opener） | 捕获 |
| 锚点分支不提前返回（内部跳转误落外部打开） | 捕获 |

**用户可见行为不变**：仍是系统默认程序打开（同一份三平台 spawn 代码，
只换了落脚文件）。差异仅在两处**可观测性**：打开失败仍不打扰用户
（`let _ =`，与旧实现取舍一致）。

**实施偏差 1 条**（**偏离原文**）：方案第 3 步说「`Services` 持有
`Arc<dyn ExternalOpener>`（默认 `SystemExternalOpener`）」，未说明如何穿过
既有装配链。直接给 `assemble_with_clock_and_dialogs` 加参数会**一次性改动
所有测试调用点**（该链被 20+ 个测试文件使用），故改为**新增最深层**
`assemble_with_opener`，让旧链转发过去——既有调用点零改动。

#### R-04 · 接线两个 `Unbound` 动作（`Revert` / `InsertImage`）

- **改造内容**（**实施偏差：契约已定，与方案正文不同，见下**）：
  1. `Revert`（还原修改）：方案正文写「调用既有的撤销入口（undo）」，但实施前核对代码证据后**推翻**——UI 标签「还原修改」、图标 `revert discard`、`actions.rs` 的原地注释「需要应用层提供『从磁盘重新加载』用例」、`settings.rs` 注释「Revert 是纯 UI 动作、没有领域对应项」四处一致指向「丢弃未保存修改、从磁盘重新加载」，且 `UiAction::Undo` 早已接线（照方案做会出现两个重复动作）。→ 定为**从磁盘重新加载**：新增应用层 `RevertCommand` + `LifecycleService::revert`（读盘 → 整篇 `replace` → `mark_saved` 清脏），`app/frame` 新增 `pending_revert` + `request_revert`/`apply_revert_confirm`/`run_revert`（脏文档经 `DialogPort::Confirm` 二次确认，**未命名文档提示而非猜位置**），`actions.rs` 路由由 `Unbound` 改为 `Handler(route_revert)`。还原后**草稿必须清**（否则下次启动会把丢弃的修改又捡回来）。
  2. `InsertImage`（插入图片）：契约定为**选图对话框 → 复制到资源目录 → 插相对链接**（而非仅插链接）。`DialogPort` 新增 `pick_image`（带 `image_filters` 过滤器，唯一出处 `IMAGE_EXTENSIONS`，覆盖预览侧 `image` crate 实际解码的位图，**剔除 svg** 因 `image` 不解码）；`shared` 新增 `ImageAssetStore` 端口 + `infrastructure` `FsImageAssetStore`（复制到文档目录 `assets/`、重名去重 `-1/-2…`，覆盖 = 静默丢图）；`Services.images` + 装配链最深层 `assemble_with_images`（旧链转发、既有调用点零改动）；`app/frame/images.rs` 编排「选图 → 导入 → 算相对/绝对路径 → 光标处插 `![](path)`」；`dialog_runner` 新增 `PickImage` 分支；`rfd_adapter` 与测试 fake 补实现。
  3. `action_route` 的 `Unbound` 清单**清零**（两个动作各有归宿）；`Unbound` 变体因实例为零保留，加 `#[allow(dead_code)]`（机制预留，与 `tests/support/fake_dialog.rs` 同款理由）。
- **涉及文件**：`src/app/frame/actions.rs`、`src/app/frame/files.rs`、`src/app/frame/dialog.rs`、`src/app/frame/images.rs`（新）、`src/application/document/commands.rs`、`src/application/document/service/lifecycle.rs`、`src/shared/dialog.rs`、`src/shared/image_asset.rs`（新）、`src/infrastructure/dialogs/rfd_adapter.rs`、`src/infrastructure/persistence.rs`、`src/infrastructure/persistence/image_store.rs`（新）、`src/app/bootstrap.rs`、`src/app/dialog_runner.rs`、`tests/app/revert_test.rs`（新，7 例）、`tests/app/insert_image_test.rs`（新，7 例）、`tests/support/fake_dialog.rs`、`mut_r04.py`（新，11 轮变异；脚本已清理）
- **影响范围**：2 个动作从「按下无反应」变为「可用」；`UiAction` 总数 57 不变；`action_route` 的 `Unbound` 分支清零；用户可见：工具栏/菜单/快捷键的「还原修改」「插入图片」真正生效。
- **依赖关系**：无前置。
- **风险点**：
  - `Revert` 走的是普通编辑管道（整篇 `replace`），故「还原本身可撤销」（Ctrl+Z 把丢弃的修改找回来）天然成立，无需额外处理；
  - `InsertImage` 写盘（复制图片到 `assets/`）属行为变更，已用重名去重 + 失败提示兜底；预览侧相对路径经 `assets.load(url, document_dir)` 解析，插 `assets/foo.png` 即可渲染。
- **回滚方案**：路由改回 `Unbound`（各 1 行）+ 删除两个动作的实现文件与新端口；`Unbound` 清单断言同步改回。
- **验收判据**（**全部达成**）：
  - `action_route` 的 `Unbound` 清单清零；
  - 新增回归 `tests/app/revert_test.rs`（7 例：核心还原 / 干净文档免确认 / 脏文档先确认且答复前内容不变 / 确认文案报文档名 / 答复否保留 / 对话框不可用报错 / 还原可撤销 / 清草稿 / 未命名文档提示）+ `tests/app/insert_image_test.rs`（7 例：复制到 assets 并插相对链接 / 未命名文档插绝对路径 / 导入失败报错且不插 / 重名去重 / 取消不插 / 选图取消不插 / 绝对路径图片）；
  - 变异验证 `mut_r04.py`（已清理） **11 轮全捕获**（route 空转 / 脏文档不确认 / 不清草稿 / 确认文案丢名 / 不 mark_saved / replace 传空串 / InsertImage 路由空转 / 导入失败静默 / 链接格式错 / 未命名文档绝对分支丢失 / 重名去重失效）。

#### R-05 · 设置页补写作目标输入

- **改造内容**：在设置页「编辑器」分区补一个目标词数输入框；`domain` 的 `writing_goal` / DTO 的 `EditorSettingsView::writing_goal` 早已有，**只缺 UI 入口**。输入框临时文本放 `settings_ui_state.writing_goal_text`（表现层视图状态，非业务副本），打开设置时从落盘值种入；改动**即时落盘**（`apply_editor_field`，与设置页其它项一致）。「0 归一为未设」逻辑抽成纯函数 `normalize_writing_goal`（表现层不得依赖 `domain`，故不引用 `WritingGoal::MAX`/常量，存 0 即由领域 `is_set` 判定未设），便于单测与变异钉住。`writing_goal` 同时纳入设置快照 `Snapshot`，**「取消」随之回滚**（与其它字段一致）。
- **涉及文件**：`src/presentation/views/settings_view/editor_prefs.rs`（输入框）、`src/presentation/state/settings_ui_state.rs`（`writing_goal_text` + `Snapshot.writing_goal` + `normalize_writing_goal` + 单测）、`src/app/frame/settings.rs`（`open_settings` 种入缓冲、`cancel_settings` 从快照回滚）、`tests/app/writing_goal_test.rs`（新，3 例）、`mut_r05.py`（新，2 轮变异；脚本已清理）
- **影响范围**：1 个 UI 控件；用户可见：写作目标**从不可设变为可设**；状态栏进度条随之生效（`app/frame/editor.rs` 的 `sync_goal` 已在读该值）。
- **依赖关系**：无前置。
- **风险点**：
  - 0 归一为未设：已实现（`normalize_writing_goal`），负数无法输入（u32）；
  - **状态归属**：本项**排在 R-11 之前**，临时输入值走 `settings_ui_state`、落盘走 `apply_editor_field`（既有直写模式）；**R-11 改造时需回头收口**（把输入框的读值也收口到方法）。
- **回滚方案**：移除控件 + 恢复 `Snapshot.writing_goal` 的「从当前值取」逻辑。
- **验收判据**（**全部达成**）：
  - 新增回归 `tests/app/writing_goal_test.rs`：设 1000 → 重启 → 读回 1000；设 0 → 读回「未设」（而非 0）；从未设过 → 读回未设；
  - 变异验证 `mut_r05.py`（已清理） **2 轮全捕获**（显式 0 不再归一 / 空串不再归一 → `normalize_writing_goal_treats_zero_and_empty_as_unset` 红）。

#### R-06 · 切标签保留光标（每文档独立光标/选区）

> **已落地（2026-10-21，commit `a62d620`）**。落地形态与原文有 4 处偏差（原文保留以便对照）：
> ① 光标表放在 **`EditorApp`（app 层）** 而非 `presentation/state/editor_ui_state.rs`——「哪份文档的光标配哪份文档」天然随标签生命周期生灭，app 层持有、随关标签清除更内聚；值用 `(usize, usize)`（**字节**偏移，无选区时 anchor==cursor）而非 `CursorState`。
> ② 恢复**经 `pending_cursor` 一次性通道**：切回帧把恢复位置按**新文档**文本 `byte_to_char` 换算成字符偏移写进 `pending_cursor`，由 `apply_pending_editor_state` 在下一帧主动重设 egui 脱字符——方案已点明「egui 内部态切文档时重置、需主动重设」，落地即复用这条既有通道。
> ③ `open_file` 新开文档**不**走恢复：其恢复值恒 0，却会写一个 `pending_cursor=(0,0)`，污染「打开后 focus_at_end 再回车」这类紧邻操作（实测 `enter_at_various_cursor_positions_keeps_gutter_in_sync` 因此把光标错放到第一行中间）；新文档的 0 由 egui 内容替换自然给出。
> ④ 真帧判据**改读 egui `TextEditState` 脱字符位置**而非原文的「`Shape::Text` 光标矩形 y 不变」：文本字形 y 与光标无关、区分不了新旧行为，caret 线在无头 shapes 里无稳定捕获先例，`scroll_y` 无头下恒 0——直读 `TextEditState` 才既区分新旧、又不依赖焦点/滚动。

- **改造内容**：`src/app/frame/tabs.rs:33-35` 注释「每文档独立的光标 / 选区尚未在 UI State 中建模（S6-12 范畴），切换时回到文档开头」→ 在 `presentation/state/editor_ui_state.rs` 增加 per-document 的光标 / 选区表（`HashMap<DocumentId, CursorState>`），切标签时存取、切换回时恢复。
- **涉及文件**：`src/presentation/state/editor_ui_state.rs`（新增字段 + 存取方法）、`src/app/frame/tabs.rs`（切换时存取）、`src/app/frame/editor.rs`（`pending_cursor` 的写回与新表协同）、`src/presentation/views/editor_view.rs`（渲染时取当前文档的光标）
- **影响范围**：多文档场景的用户可见行为变化（**这是行为变更，不是纯重构**）；需考虑文档关闭时清理表项、文档数上限 12 → 表大小有界。
- **依赖关系**：**依赖 R-11 的第 1 子批**（state 字段的写入口约定）——否则本项会新增一批「app 直写 state 字段」的样板，与后续收敛方向相反。
- **风险点**：
  - **与 `pending_cursor` 的时序冲突**：`app/frame/editor.rs` 的 `pending_cursor` 是「领域改文本后把 egui 光标重设回去」的一次性通道，与「每文档光标表」是两套机制 → 必须明确谁在什么时候写谁，否则会出现「切回标签时光标跳到旧位置」；
  - egui `TextEdit` 自身也记光标（`TextEdit` 内部状态），切换文档时它会重置 → 需要在切换帧**主动重设**，而不是只在切走时记录。
- **回滚方案**：`git revert`（本项独立成一个 commit，不与其他项混提）。
- **验收判据**：
  - 新增真帧回归：文档 A 光标在第 3 行 → 切到 B → 切回 A → 光标仍在第 3 行（断言 `Shape::Text` 的光标矩形 y 坐标不变）；
  - 变异验证：把「切回时恢复」的那段去掉 → 用例红。

### 4.2 P1 组：扩展性瓶颈与耦合

#### R-07 · 导出格式双枚举收敛

> **已落地（2026-10-20，commit `bbcbe20`）**。落地形态与原文有 5 处偏差，
> 原因与实测证据见本节末尾的「实施偏差」小节；**原文保留**以便对照。

- **改造内容**：
  1. `src/domain/export/value_objects.rs`：给 `ExportFormat` 增加
     ```rust
     pub const fn from_str(raw: &str) -> Option<Self>  // 穷举 match，漏加变编译错误
     pub const fn all() -> &'static [Self]
     ```
     现有 `extension()` / `supports_toc()` / `supports_pagination()` / `needs_page_size()` / `label()` 已是穷举 ✓。
  2. `src/application/export/service.rs:224` `parse_format` 改为调 `ExportFormat::from_str`（删掉手写字符串 `match`）。
  3. **删除** `src/presentation/state/export_ui_state/choices.rs:5` 的 `ExportFormatChoice` 枚举，UI 侧改为直接使用领域 `ExportFormat`；该文件里**领域没有**的 UI 专属数据（`is_available()` / `unavailable_reason()` / `pdf_caveat()`）拆到**独立的辅助函数表**（`fn pdf_caveat(format: ExportFormat) -> Option<&'static str>`），不与枚举同构。
  4. `PageSizeChoice`（`choices.rs:109`）同理处理。
- **涉及文件**：`src/domain/export/value_objects.rs`、`src/application/export/service.rs`、`src/presentation/state/export_ui_state/choices.rs`（大幅缩减或删除）、`src/presentation/state/export_ui_state.rs`、`src/presentation/views/export_view.rs`（消费点）、`src/infrastructure/export/routing_exporter.rs`（确认穷举 ✓）
- **影响范围**：导出格式的**唯一真源**从 2 个降为 1 个；「新增一种导出格式」的改动点从 6 处降到 **3 处**（领域枚举 + `from_str` + 路由 match），且全部编译期暴露。
  - **实测（2026-10-20）**：真源落到 `shared` 后，「新增一种导出格式」的实际改动点是 **2 处**：① `shared/export_format.rs`（加变体 → `as_str` / `aliases` 的穷举 match **编译失败** → 补分支 → 登记进 `ALL`）；② `infrastructure/export/routing_exporter.rs` 的路由 `match`（同样靠穷举 match 编译期暴露）。对话框单选组、提交 id、扩展名、领域校验**全部自动派生**，无需改动。
  - **残留的运行期漏洞（已用测试堵住）**：`ALL` 是手写清单——「加了变体但忘登记进 `ALL`」时该格式解析不出来且无人报错。由 `export_format_all_has_exactly_four_entries` 断言 `ALL.len() == 4` 把它变成一条必改的红线。
- **依赖关系**：无前置。**R-12 的规则若覆盖「同一概念的双枚举」，本项是第一个被检出的**。
- **风险点**：
  - `ExportFormatChoice` 被 `export_view.rs` 多处消费（`choices.rs` 里的 `ALL` 是选择器数据源）→ 删除后要重建数据源，**改错会导致下拉框少一项或顺序变**；
  - `pdf_caveat` 等 UI 文案若混进领域枚举 → 领域层将依赖 UI 文案，**违反分层**（必须用独立函数表，理由要写进注释）。
- **回滚方案**：`git revert`（领域侧改动是纯新增 `from_str` / `all`，可单独 revert 而不动 UI）。
- **验收判据**：
  - 新增用例：`export_format_from_str_is_exhaustive`（对 `ExportFormat::all()` 逐个断言 `from_str` 能反解；**再加一条编译期断言**：故意漏一个变体时编译失败——用 `const _: () = assert!(...)` 形式做不到，改用「`all()` 与 `from_str` 的 match 分支数一致」在测试里断言 `all().len() == 4`）；
  - `grep -c "pub enum ExportFormatChoice" src/` → **0**；
  - 变异验证：把 `from_str` 的一个分支删掉 → 用例红。

- **实施偏差（2026-10-20 落地时确认，commit `bbcbe20`）**：
  1. **唯一真源落在 `shared::export_format`，不是 `domain`**。原文第 3 条
     说「UI 侧改为直接使用领域 `ExportFormat`」，但这与
     `tools/arch_check.rs` 的 `PRESENTATION_FORBIDDEN = ["crate::domain", "crate::infrastructure"]`
     **直接冲突**——照原文做，`check-arch` 立刻变红。
     导出格式被**三方**需要（领域：任务选项校验；表现：对话框单选组与提交 id；
     基础设施：导出器路由），而 `docs/architecture-constraints.md` §4 规定
     表现层不得依赖 `domain`、基础设施层不得依赖 `application`——
     **三方共同的依赖点只有 `shared`**。
     故真源下沉 `shared::export_format`，`domain::export::value_objects`
     以 `pub use` 转出（公开路径 `domain::export::value_objects::ExportFormat`
     不变，测试与外部调用零改动）。
     先例：`shared::path::FilePath`（四层共用）、`shared::text_format`
     （三方共用的唯一出处）。
  2. **方法名 `from_str` → `parse`**。clippy 的 `should_implement_trait`
     拒绝「同名 inherent 方法」与 `std::str::FromStr` trait 方法并存
     （`-D warnings` 下红）。最终形态：inherent `parse`（返回 `Option`，
     用于「认不出就当没给」）+ trait `from_str`（返回 `Result`，
     用于 `str::parse` 惯用写法）**并存**，不靠 `#[allow]` 压制。
  3. **`all()` → `ALL` 常量 + `as_str()`**（对齐本工程既有命名）。
     「漏加变体编译失败」这一验收点由 `as_str` / `aliases` 的**穷举 match**
     承担；「`ALL` 与解析分支一致」由用例 `export_format_all_has_exactly_four_entries`
     断言 `ALL.len() == 4` 承担（原文已指出 `const _: () = assert!(...)` 形式做不到，
     改用测试断言——本次沿用该思路）。
  4. **变异验证的形态变了**：原文是「删掉 `from_str` 的一个分支」，
     但实现上 `parse` 遍历 `ALL` 而非手写分支，故不存在可删的分支。
     实际变异点改为「从 `ALL` 里删掉一个变体」→ 实测 **2 条用例变红**
     （清单长度断言 + 历史别名解析），判据有牙齿。
  5. **附带发现并修掉的硬编码**：`app/frame/capture.rs` 的两处
     `format: "image"` / `page_size: "a4"` 是绕过真源的另一条路，
     已改走 `ExportFormat::Image.as_str()` / `PageSize::A4.as_str()`。

#### R-08 · `persisted_key` 穷举 + 默认键位表合一

> **① 已落地（2026-10-20，commit `8cdf083`）**。落地形态与原文有 3 处偏差，
> 并**顺带修掉一个真实用户可见缺陷**（领域动作清单漏登记）。②（默认键位表合一）
> **未做**——理由见下方「实施偏差」第 3 条。

- **改造内容**：
  1. `src/presentation/shortcuts/registry/action/impls.rs:72` 的 `persisted_key` 改为**穷举全部 57 个 `UiAction` 变体**：可持久化的返回 `Some("...")`，不可持久化的**显式**写 `UiAction::X => None`（不再用 `_ => return None` 兜底）→ 漏加变体时**编译失败**。
  2. 默认键位表合一：`src/domain/preferences/value_objects/shortcut_table.rs:150` `default_bindings()` 与 `src/presentation/shortcuts/registry/action/impls.rs:322` `default_combo()` 是两份并行表 → **保留领域侧一份**，表现层改为查领域（需确认领域侧不依赖表现层类型——`KeyCombo` 若是表现层类型，则把 `KeyCombo` 下沉到 `shared` 或让领域表存字符串）。
  3. `ActionId`（`domain/preferences/value_objects/action.rs:11`）与 `UiAction` 的对应关系改为**由 `persisted_key` 单向派生**（现状已是 `from_persisted_key` 反查，可加一条「两表长度一致」的一致性用例）。
- **涉及文件**：`src/presentation/shortcuts/registry/action/impls.rs`、`src/domain/preferences/value_objects/shortcut_table.rs`、`src/domain/preferences/value_objects/action.rs`、`src/application/preferences/*`（DTO 映射）
- **影响范围**：57 个动作的持久化键表；「新增一个可持久化动作」的改动点从 5–8 处降到 4 处，且漏加**全部编译期暴露**。
- **依赖关系**：无前置。若 R-08 ② 需要 `KeyCombo` 下沉，会牵动 `shared` → 建议**先只做 ①**（收益最大、风险最小），② 视 ① 完成后的形态决定。
- **风险点**：
  - `persisted_key` 穷举后，**原本靠 `_` 兜底的 16 个不可持久化动作**要逐个显式列出 → 写漏一个就编译失败（**这正是目的**，但第一次改会有一批编译错误要逐个处理）；
  - ② 若 `KeyCombo` 下沉到 `shared`，会新增 `shared` 的公共 API 面 → 与 R-21 的「shared 瘦身」方向相反 → 建议 ② 改为「领域表存 `(action_id, combo_str)`，表现层解析」，避免类型下沉。
- **回滚方案**：`git revert`；① 与 ② 分成两个 commit，可单独回退。
- **验收判据**：
  - `grep -n "_ => return None" src/presentation/shortcuts/registry/action/impls.rs` → **零命中**；
  - 新增用例：`every_ui_action_has_an_explicit_persisted_key_decision`（遍历 `UiAction::ALL`，断言每个变体在 `persisted_key` 的穷举表里都有明确分支——用「`persisted_key` 对每个变体返回可预测值 + 编译期穷举」双保险）；
  - 变异验证：给 `UiAction` 加一个测试专用变体 → **编译失败**（这就是验收本身）。

##### R-08 ① 实施偏差（2026-10-20 落地时确认，commit `8cdf083`）

1. **数量修正（方案原文有误，以代码为准）**：原文称「原本靠 `_` 兜底的
   **16** 个不可持久化动作」。实测 `UiAction::ALL` 共 **57** 项，
   `persisted_key` 的 `Some` 分支 **38** 项，兜底覆盖 **19** 项
   （`OpenFolder` / `Revert` / `InsertImage` / `ToggleList` /
   `ToggleOrderedList` / `MoveLineUp` / `MoveLineDown` / `GotoTab1..9` /
   `ShortcutOverview` / `About` / `ToggleTheme`）。
2. **②（默认键位表合一）未做**：①完成后实测发现两侧默认表的**语义不同**——
   领域 `ActionId::all()` 是「**可持久化**动作全集」（38 项），
   领域 `default_bindings()` 是「**有默认键位**的动作集」（37 项），
   差集恰为 `ToggleStatusbar`（可持久化但**不占用快捷键**，产品有意为之，
   见 `docs/06-表现层与交互实现.md:46`）。
   领域 `KeyCombo::parse("")` 返回 `Err`（`key` 为 `None`），
   故领域表**无法表达「无默认键位」**——要合一需先改领域 `KeyCombo`
   的表达能力或引入「未绑定」语义，**牵动 `shared`**，与 R-21 方向相反。
   按原文「② 视 ① 完成后的形态决定」，本次**不做**，留待专项。
3. **顺带修掉一个真实用户可见缺陷**：领域 `ActionId::all()` 清单
   **漏登记** `ToggleStatusbar`（枚举 38 / 清单 37）。
   症状：用户改「显示/隐藏状态栏」的键位时，`domain_action()` 反查返回
   `None`，界面弹「ToggleStatusbar 尚未映射到领域动作，无法改键」。
   该缺陷自 `ActionId::ToggleStatusbar` 引入起就存在；文档当时记载的
   「未进 `all()` 默认绑定集」把「不进 `all()`」与「不进 `default_bindings()`」
   混为一谈——**前者是 bug，后者是设计**。
   连带修正领域断言「每个动作都应有默认绑定」：它把上述两个集合当成
   一个，使「可持久化但无默认键位」这一合法状态无法表达
   （要么被排除出 `all()` → 改键报错；要么进 `all()` → 断言变红）。
   现改为「除**显式**列出的例外外，每个动作都应有默认绑定」。
4. **新增两条源码级守卫**：①的收益（编译期穷举）只覆盖 `persisted_key`
   一处。**手写清单**（`UiAction::ALL` / `ActionId::all()`）编译器管不到，
   「加变体忘了加进清单 → 该动作不出现在命令面板且不可改键」**零报错**。
   故新增源码级守卫逐项比对「清单 ↔ 枚举变体」。

##### R-08 ① 新增用例（13 例）

- `tests/app/shortcut_persisted_key_test.rs`（跨层一致性，6 例）：
  - 领域 `ActionId::all()` 的 key 集合与表现层可持久化 key 集合**逐项相等**
    （只比数量挡不住「键名写错」）；
  - 每个动作的持久化决策可预测（有键 → 键在领域存在，且反查回到**同一**动作，
    即键不得撞车）；
  - 不可持久化的 19 个动作是**显式清单**；
  - 两侧手写清单与枚举变体数对照。
- `tests/app/shortcut_action_guard_test.rs`（源码级，7 例）：
  - `persisted_key` 不许有 `_ =>` 兜底分支（**扫全仓**：凡叫这个名字的
    函数一律不许，防「换个类型再写一个同名兜底」）；
  - `UiAction::ALL` 与枚举变体**逐项同步**（漏登记 / 多登记都检出）；
  - 领域 `ActionId::all()` 与枚举变体**逐项同步**。

##### R-08 ① 变异验证（3 轮，判据有牙齿的证明）

1. 兜底分支写回 → `persisted_key_has_no_fallback_branch` 红（报**文件真实行号**）；
2. `ActionId::all()` 漏登记 `ToggleStatusbar` → **4 例同时红**；
3. `UiAction::ALL` 漏登记 `ToggleTheme` → **3 例同时红**。

#### R-09 · 主题双枚举收敛

> **已落地（2026-10-20，commit `b1f951b`）**。落地形态与原文有 3 处偏差，
> 并**顺带修掉两个真实缺陷**（详见「实施偏差」）。

- **改造内容**：
  1. `src/presentation/theme/builtin/kind.rs:5` 的 `BuiltinTheme`（3 变体）与 `src/domain/preferences/value_objects/theme.rs:11` 的 `ThemeKind`（4 变体，多 `Custom`）——保留**领域 `ThemeKind` 为唯一真源**，`BuiltinTheme` 降为「内置主题的标识」或直接删除；内置主题的具体配色（`builtin.rs:145` `dark()` / `:200` `light()`）保持不变。
  2. `src/app/frame/actions.rs:20-24` 的**第三份**手写 `match self.ui.theme.kind` → 改为调用领域枚举的 `as_str()`。
  3. `src/application/preferences/service.rs:237-240` 的字符串 → `ThemeKind` 映射 → 改为 `ThemeKind::from_str`（穷举）。
- **涉及文件**：`src/presentation/theme/builtin/kind.rs`、`src/presentation/theme/builtin.rs`、`src/domain/preferences/value_objects/theme.rs`、`src/application/preferences/service.rs`、`src/app/frame/actions.rs`、`src/presentation/views/settings_view.rs`（消费点）
- **影响范围**：「新增一种内置主题」的改动点从 4 处降到 2 处（领域枚举 + 配色表）。
- **依赖关系**：无前置。
- **风险点**：
  - `ThemeKind` 有 `Custom` 变体（用户自定义），而 `BuiltinTheme` 没有 → 合并时**不能简单删一个**，要处理「领域有 Custom、表现层无对应内置主题」的映射（`Custom` 走用户配置，不走 builtin 表）；
  - 持久化值是字符串（`"dark"/"light"/"system"/"custom"`），改枚举时**不能改这些字符串**（否则用户配置失效）→ 合并时必须保持 `as_str()` 输出不变。
- **回滚方案**：`git revert`。
- **验收判据**：
  - `grep -rn "pub enum BuiltinTheme" src/` → 0（或降级为非枚举的常量表）；
  - 新增用例：`theme_kind_round_trips_through_its_persisted_string`（对每个 `ThemeKind` 断言 `from_str(as_str()) == self`）——**这条同时守住持久化兼容性**；
  - 变异验证：把 `as_str()` 的一个分支改错 → 用例红。

##### R-09 实施偏差（2026-10-20 落地时确认，commit `b1f951b`）

1. **真源落 `shared` 而非 `domain`**（**偏离原文**）：原文说「保留领域
   `ThemeKind` 为唯一真源」，但**表现层不得依赖领域**
   （`docs/architecture-constraints.md` §4；`check-arch` 的
   `PRESENTATION_FORBIDDEN` 拦 `crate::domain`），而主题种类是三方
   （领域 / 应用 / 表现）共同需要的 → 真源落在 `shared::theme::ThemeKind`，
   领域侧以 `pub use` 转出（公开路径
   `domain::preferences::value_objects::ThemeKind` **保持不变**）。
   与 R-07 的 `shared::export_format` 同一模式。
2. **原文漏数了副本数量**：原文只提到「第三份手写 `match`」，
   实测主题这条事实散落**六处**：领域枚举、表现层 `BuiltinTheme`、
   `app/frame/actions.rs` 的手写 `match`、
   `application/preferences/service.rs` 的手写 `match`、
   `presentation/views/settings_view.rs` 的成对常量表 `THEME_KINDS`、
   `presentation/views/settings_view/about.rs` 的手写 `match`。
   六处**全部**收敛。
3. **容错责任重新划分（行为差异）**：真源的 `ThemeKind::parse`
   **不**兜底（未知值 → `None`），而原表现层 `BuiltinTheme::parse`
   未知值直接退回 `System`。现改为：**真源只回答「认得 / 不认得」**，
   兜底由**调用点**决定——表现层 `resolve` 用 `unwrap_or(System)`
   （「照常启动」），应用层 `to_theme` 报出配置错误（「告诉用户写错了」）。
   另：应用层的字符串匹配由**精确**改为**大小写不敏感 + trim**
   （随真源 `parse` 的语义）。

##### R-09 顺带修掉的两个真实缺陷

1. **`refresh_system_theme` 永远提前返回**（`app/frame/settings.rs`）：
   判据是 `ui.theme.kind != System`，而 `ui.theme.kind` 是 `resolve()`
   解析后的**实际生效明暗**（只可能是 `Light`/`Dark`），故该判断
   **恒成立** → 跟随系统时系统外观变化**不会**自动刷新。
   根因是表现层 `BuiltinTheme` 缺 `Custom` 变体、`custom` 时被迫用
   `System` 顶替，使「当前是不是跟随系统」在运行时无从判断。
   现改读**设置里用户选的 kind**（持久化真源）。
2. **主题持久化契约值被硬编码 3 处**：`dto/preferences.rs` 的
   `fallback()` 默认主题、`export_ui_state.rs` 的 `Default` 构造参数、
   以及一处测试数据。契约值写错的后果是「用户的主题选不中」，
   而编译与运行**都不报错**。现已全部走 `ThemeKind::as_str()`。

##### R-09 新增守卫（18 例）

- `tests/shared/theme_test.rs`（9 例）：
  - `theme_kind_round_trips_through_its_persisted_string`（**验收判据**）；
  - 持久化字符串的**具体值**固定不变（`light`/`dark`/`system`/`custom`）——
    往返能守住「加变体时同步加解析」，但守不住「把 `dark` 改成 `darker`」；
  - 变体清单恰为四项；解析大小写不敏感但严格；
  - `parse`（`Option`）与 `FromStr`（`Result`）分工一致；
  - 明暗判定的语义边界（`System`/`Custom` **不算**深色）；
    `Display` ≡ `as_str`。
- `tests/shared/theme_guard_test.rs`（9 例，源码级）：
  - 主题种类枚举**只有一处定义**（按「变体像」识别，故换个名字重建
    同构枚举也会被检出）；
  - 主题持久化契约值**不得**在别处硬编码（豁免真源 / serde 属性 /
    比较上下文 / `#[cfg(test)]` 模块——**比较**契约值是**读**它，
    不是**定义**它）。

##### R-09 变异验证（2 轮）

1. `as_str` 把 `dark` 写成 `darker` → **2 例红**；
2. 注入第二份同构枚举 `BuiltinTheme` →
   `theme_kind_has_exactly_one_definition` 红（报真实路径行号）。

#### R-10 · span → `LayoutJob` 构造器统一 + 样式表合并

- **改造内容**：
  1. 把 `src/presentation/theme/mapper/highlight_cache.rs:253-333` 的 `build_line_job`（「边界并集 + `find` 覆盖」语义）提炼为**表现层唯一的 span→`LayoutJob` 构造器**，放到 `src/presentation/theme/mapper/span_job.rs`。
  2. `src/presentation/views/preview_view/code.rs:98-143` 的 `highlighted_code_job`（顺序游标）与 `src/presentation/views/preview_view/inline.rs:319-` 的 `build_text_job`（顺序游标）改为调用它；若某些 span 语义确实不同（如 `inline` 要剥 math 分隔符），用**参数化**（`SpanPolicy`）表达差异，**不复制实现**。
  3. 样式表合并：`src/presentation/views/preview_view/inline.rs:19` 的 `span_format` 并入 `src/presentation/theme/mapper.rs:99` 的 `format`（mapper 覆盖更全：15 种 vs 14 种，缺的是 `heading/quote/strong/emphasis/strikethrough/code/link` 的对齐）→ 改一处颜色两处同步。
  4. 字符边界防护统一：`shared/geometry.rs:170` `floor_char_boundary` 与 `:188` `clamp_to_char_boundaries` 的调用点统一到后者（它是前者的超集）。
- **涉及文件**：`src/presentation/theme/mapper/highlight_cache.rs`、`src/presentation/theme/mapper/span_job.rs`（新增）、`src/presentation/theme/mapper.rs`、`src/presentation/views/preview_view/code.rs`、`src/presentation/views/preview_view/inline.rs`
- **影响范围**：预览区代码块与行内元素的语法配色；**用户可见效果应完全不变**（同一份 Markdown 在编辑区与预览区颜色本就应一致，现状因两套实现而可能已漂）。
- **依赖关系**：**依赖 R-14**（`floor_char_boundary` 副本清理会顺带统一边界防护调用点）。R-21 的「命名冲突」建议在 R-10 之后做（改文件名会与本项的 import 改动冲突）。
- **风险点**：
  - **顺序游标 vs 边界并集**语义不等价：边界并集能表达**重叠** span（查找命中背景叠加语法高亮），顺序游标不能 → 替换后 `code.rs` / `inline.rs` 会出现「查找命中背景覆盖语法色」的新行为（这其实是**修复**，但需确认用户可见效果符合预期）；
  - `inline.rs` 的 math 分隔符剥离、footnote 引用等是**行内特有**语义，直接套用编辑区的 `build_line_job` 会丢 → 必须显式建模为 `SpanPolicy` 参数，**不能为了少写而丢语义**；
  - 一次改两套实现 + 一张表 = 变更面较大 → 建议拆成 2 个 commit（先统一构造器，再合并样式表）。
- **回滚方案**：`git revert`（两个 commit 可分别回退）；**保留旧实现**并行一个版本做对拍，确认等价后再删。
- **验收判据**：
  - 新增**对拍用例**：同一段 Markdown（含代码块、表格、任务列表、引用、脚注、数学），分别经「旧三套实现」与「新统一构造器」产出的 `LayoutJob`，逐项比对 `text()` / `num_vertices` / `num_indices` / 每行 rect；**差异仅允许出现在重叠 span 的背景层**；
  - 变异验证：把统一构造器里 `find` 覆盖那一支删掉 → 查找命中的背景消失 → 对拍用例红；
  - 用户可见：预览区与编辑区同一段代码的语法色**逐 span 相同**（新增真帧用例断言两侧 `Shape::Text` 的颜色序列一致）。

##### R-10 实施结果（2026-10-20，commits `7f96b55` + `ec8926a`）

按方案要求拆 **2 个 commit**：

| commit | 内容 | 对应原文条目 | 测试数 |
|---|---|---|---|
| `7f96b55` | 统一构造器（`mapper::span_job::build_job`） | 第 1 / 2 / 4 条 | 1976 |
| `ec8926a` | 两张样式表合并为一份派发（`StyleSemantics::of`） | 第 3 条 | **1979** |

##### R-10 实施偏差（2026-10-20 落地时确认）

1. **样式表「合并」的字面形式不成立，改为「派发合并、度量分离」**
   （**偏离原文**）：原文第 3 条说「把 `inline.rs` 的 `span_format` 并入
   `mapper.rs` 的 `format`」。实测**并表**会破坏刻意设计：

   - **字号体系不同**：编辑区 `theme.editor_font_size`（14px，用户可配置）
     vs 预览区 `theme.metrics.preview_font_size`（15px，固定）。并表会让
     「预览字号跟随编辑区设置漂移」——`span_format` 的文档注释明确说
     这是**刻意**的；
   - **字体族不同**：编辑区**等宽**（源码视图）vs 预览区**比例字体**（排版视图）；
   - **行高不同**：编辑区 `字号 × editor_line_height` vs 预览区
     `字号 × preview_line_height`；
   - **线条粗细不同**：编辑区 `hairline`（源码视图要细）vs 预览区
     `Stroke::new(1.0)`。

   另：原文说「mapper 覆盖更全：15 种 vs 14 种」，**实测是 24 种 vs 14 种**
   （mapper 多出的是 9 种代码语法 + 编辑区特有的 `plain` / `list_marker` /
   `table_border` / `task_marker` / `image`）。

   故改为新增 `mapper::StyleSemantics`（颜色 / 斜体 / 下划线 / 删除线 /
   背景 / 等宽 / 字号缩放）+ `apply_semantics`（把语义套上度量）：
   **派发只有一处，各自带自己的度量**。

2. **原文第 3 条的**目标**已天然满足**（探针实测，2026-10-20）：
   两表**共同**的 7 种样式（`heading` / `quote` / `strong` / `emphasis` /
   `strikethrough` / `code` / `link`）颜色**本就完全相同**（同源于
   `theme.syn_*`）——即「改一处颜色两处同步」在合并前就已成立。
   真正重复的是**派发逻辑**，故合并的是它（见上条）。

3. **验收判据第 3 条（「同一段 Markdown 的语法色逐 span 相同」）不可达成**
   （**偏离原文**）：实测（探针）两侧**语法来源本就不同**——

   - 编辑区对**全文**用 `lang = "markdown"` 高亮 → syntect 的 markdown
     语法只把 ``` 围栏**标记本身**标成 `Code`，**围栏内的代码一个字都不
     上色**（实测 `"```rust\nfn main() {...}\n```"` 只产出 2 个 span，
     分别是 0..3 与 43..47 的围栏标记）；
   - 预览区代码块用**围栏自己的语言**（如 `rust`）对**块内文本**单独
     高亮 → 同一段代码产出 6 个 span（`Keyword` / `Function` / …）。

   要让两侧一致，需要「编辑区也按围栏语言高亮围栏内的代码」——那是
   **另一个改造项**（编辑区多语言高亮），不属于 R-10（R-10 只统一
   「带样式的一段文本 → `LayoutJob`」的**构造器**）。

   故判据改为**可达成的等价命题**：「**同一段代码 + 同一套 span**，经
   编辑区路径（边界并集）与预览区代码块路径（顺序游标）渲染，两侧画出的
   颜色与背景序列**逐 span 相同**」。

4. **「差异仅允许出现在重叠 span 的背景层」不适用于本轮实测**：实测发现
   旧实现有**两处真实缺陷**（不是背景层差异），新实现修复了它们——
   详见下方「顺带修掉的两个真实缺陷」。

##### R-10 顺带修掉的两个真实缺陷

1. **代码块同一行出现两种字号**（`preview_view/code.rs`）：旧
   `highlighted_code_job` 只给 span 段与**尾部**覆盖了 `font_id = 预览等宽`，
   **中间的 base 段漏了** → 同一行代码里，span 之间的空隙用编辑区字号
   （14px）、其余用预览字号（15px），一眼可见的字号跳变。
   新实现把 `base` 一次性设成预览字号 → 全行一致。
2. **代码块重叠 span 导致文本重复**（同文件）：旧实现的
   `cursor = end`（而非 `end.max(cursor)`）在 span 重叠时会让尾部 base 段
   从**更早**的位置开始 → 已渲染过的字符被输出第二遍。实测：原文
   `"abcdefgh"`（8 字符）被输出成 `"abcdedefgh"`（**10 字符**）；span 被
   完全覆盖时更甚，产出 `"abcdefbcdefgh"`（**13 字符**）。屏幕上会看到
   重复的字符。新实现恒产出原文。

另修掉一处**两套旧实现自身就不一致**的地方：折行宽度 ≤ 0 时，旧 inline
用 `0.0`（epaint 会排成「每字一行」，实测 `"Hello world"` → 11 行），
旧 code.rs 用 `INFINITY`（不折行）。统一取「不折行」。

##### R-10 新增守卫（21 例 + 变异验证 19 轮）

- `tests/presentation/span_job_parity_test.rs`（13 例）：把 git HEAD 的
  **旧实现原文抄进测试文件**当 oracle，与新统一构造器逐段比对
  （文本 / 颜色 / 背景 / 字体族 / 字号 / 粗斜体下划线）。分两类：
  - **等价用例**：编辑区（边界并集）路径 8 个 case **逐段等价**；
    代码块（顺序游标）路径 4 个 case 等价；
  - **有意分歧用例**：旧实现的 3 处缺陷（字号 / 段数 / 文本重复），
    用独立用例把**新行为**钉死并写清旧行为错在哪。
  - 另有折行参数 2 例、样式名无关性 2 例、base 样式归属 1 例、
    「段数随格式而变」1 例。
- `src/presentation/views/preview_view.rs` 的 `mod tests`（4 例）：行内路径
  的旧实现同样抄为 oracle，**同时比对命中区间**（`SpanHit`）。
- `src/presentation/theme/mapper/tests.rs`（3 例）：样式表合并的
  「合并前后逐项相同」——两张旧表在测试里**原文抄录**为 oracle
  （`legacy_mapper_format` / `legacy_inline_span_format`）。
- **真帧对拍 1 例**（`preview_view.rs` 内）：同一段代码 + 同一套 span，
  两条路径各画一次 → 从 `FullOutput::shapes` 的 `Shape::Text` 里取
  `galley.job.sections` 的**颜色与背景**序列，逐段比对。
  - 背景也必须比：变异验证实测，只比前景色时「叠加一个背景色」的
    变异**不红**（背景不同但前景色相同）→ 判据补上背景比对。
- **变异验证 19 轮**（构造器 12 + 样式表 7），全部从**干净基准**重新生成
  文件、`try/finally` 保证还原；**编译失败单独识别**（否则会被
  「无 test result 行」误判成「无牙齿」）。

  | 变异 | 结果 |
  |---|---|
  | 顺序游标不钳 `start` → 文本重复 | 捕获 |
  | 顺序游标不钳 `end` → 文本重复 | 捕获 |
  | **删掉 `find` 覆盖那一支** → 查找命中背景消失（**原文点名的变异**） | 捕获 |
  | 折行宽度 ≤ 0 不退化为 INFINITY | 捕获 |
  | base 段改走 span 样式表 | 捕获 |
  | 边界并集不吸附字符边界 | 捕获 |
  | 不累加 `char_cursor` → 命中区间错位 | 捕获（**修夹具后**） |
  | 行内不改写文本（math 分隔符残留） | 捕获 |
  | 改写忽略样式名 | 捕获 |
  | `on_hit` 恒 false → 命中区间消失 | 捕获 |
  | 边界并集改「后者覆盖前者」 | 捕获（**修夹具后**） |
  | 顺序游标跳过空 spans 分支 | 捕获 |
  | 顺序游标不钳 `end` 的那一个 `max` | **无牙齿**（见下） |
  | 样式表 7 个变异（去斜体 / 去等宽 / 去背景 / 去删除线 / 去下划线 / 缩放改值 ×2 / 去查找底色） | 捕获 |

  **两处「无牙齿」的根因与处置**（均为**夹具缺陷**，非实现缺陷）：

  - `end.max(cursor)` 这一个 `max` **当前无作用**——`end >= cursor` 已由
    上面两行钳位保证（实测：改成 `end` 后全部用例仍绿）。**保留**它是给
    「将来有人删掉 `end.max(start)`」留的保险；已在代码注释里写明这个
    实测事实并说明三行是一组。
  - 「边界并集改后者覆盖前者」原本无牙齿：`span(5,12)` 的起点 5 落在
    「文」（3..6）**字符中间** → 并集的**边界**按 `snap()` 吸附成 3，
    但**覆盖判定**仍用**原始** `range`（5..12）→ `lo=3` 时它压根不匹配，
    两个 span **从未同时命中**同一段 → 两种语义产出的颜色完全相同。
    已把夹具改成起点落在字符边界上的 `span(3,12)`（真重叠）→ 变异被捕获。
  - 「不累加 `char_cursor`」原本无牙齿：行内用例每条**最多只有一个命中
    span** → `c0` 恒为 0，改 `char_cursor` 看不出差别。已补
    「两个命中 span」用例 → 变异被捕获。

  教训：**「对拍旧实现 vs 新实现」这类判据，对「两边同时变」的变异天然
  无能为力**——旧实现在测试文件里是**独立抄录**的副本，两边一起错时
  仍然「相等」。故必须有**独立钉住新行为**的用例（本文档的「有意分歧
  用例」与「合并前后逐项相同」用例即此职责）。

##### R-10 遗留（不属本项，已登记）

- **代码块只覆盖了 `font_id`，没覆盖 `line_height`**：`mapper.format` 的
  `line_height = editor_font_size(14) × editor_line_height(1.6) = 22.4`，
  而预览区代码块的字是 15 号 → **行高按 14 号字算而字是 15 号**。
  旧实现就这样，新实现保持了它（修它会改变预览区所有代码块的行距，
  属用户可见变更，需另行评估）。
- **编辑区对围栏内代码不上色**（见「实施偏差」第 3 条）——需要
  「编辑区多语言高亮」这一独立改造项。

#### R-11 · `presentation ↔ app` 状态所有权收敛

> **R-11 整体完成（2026-10-21，分支 `refactor/v11-54`）**。
> 子批 1（`export_ui`，8 处，`96c2fa8`）→ 子批 2（`diff_ui`，3 处，`5f5a7ab`）
> → 子批 3 按 state 分组八 commit：`editor`(11,`92f7755`) / `sidebar`(8,`de0bd68`)
> / `preview`(5,`aa6c252`) / `ui`(4,`e8157a2`) / `settings_ui`(4) + `find`(2) +
> `status_bar`(1) + `shortcut_overview`(1) 收尾四组（`3b4dc00`）。全库
> `src/presentation/views` 对 app 状态字段的**真实直写由改造前 46 处降至 0**
> （方案「59 处」含 13 处 `==` 误报，真实赋值 46；逐组 commit 与偏差见本节
> 「实施偏差」各子节）。R-12 机械规则可上线固化本成果。

- **改造内容**：把 presentation 层对 app 状态字段的 **59 处直写**（`grep -rEn 'app\.[a-z_]+\.[a-z_]+\s*=' src/presentation/`）改为调用 app 层方法。
  - **子批 1（`export_ui`，7 处）**：`src/presentation/views/export_view.rs:221/231/236/265/272/276/280` → 在 `src/app/frame/actions.rs` 或新建 `src/app/frame/export.rs` 提供 `set_export_error()` / `clear_export_result()` / `set_export_path()` 等方法。
  - **子批 2（`diff_ui`，3 处 + `model.rs`）**：`src/presentation/views/diff_view.rs:70/94/95` → `src/app/frame/` 提供对应方法。
  - **子批 3（其余 49 处）**：`editor_ui_state` / `app_ui_state` / `find_replace_ui_state` / `status_bar_ui_state` 等，按「谁改的」归到对应 app 模块。
  - 配套：`src/app/frame/actions.rs:14-58` 的 `open_overlay` 直接写 `self.export_ui.theme` / `.file_stem` / `.output_dir` 与 `self.diff_ui.error` → 路由表模块**只做路由**，状态初始化下沉到对应 app 模块。
- **涉及文件**：`src/presentation/views/*.rs`（59 处散点）、`src/app/frame/*.rs`（新增方法）、`src/presentation/state/*.rs`（字段的 `pub` 视情况收敛为 `pub(crate)`）
- **影响范围**：**全应用 UI 状态写入路径**。用户可见行为**不变**（纯重构），但需逐处确认语义等价。
- **依赖关系**：**R-06 依赖本项的子批 1 先行**（否则 R-06 会新增一批直写样板）。本项完成后 R-12 才能加规则。
- **风险点**：
  - 59 处逐个改写，**极易漏改或改错语义**（尤其 `export_view.rs` 连续 7 处有先后顺序依赖：设置错误 → 清除结果 → 设置路径）；
  - 借用冲突：视图方法签名是 `fn show(app: &mut EditorApp, ui: &mut Ui)`，改成调 `app` 方法后，视图内部**不能再持有 `app` 的字段引用**（Rust 借用检查会强制这一点，**这是好事**）；
  - 过度改造的诱惑：把 59 处顺手重构成「命令模式」会引入大改 → **本项只做「直写 → 方法调用」这一件事**，不顺手重构。
- **回滚方案**：**按子批分别 commit**，每个子批可独立 `git revert`；每个子批完成后门禁必须全绿，不允许「攒着一起改」。
- **验收判据**：
  - `grep -rEn 'app\.[a-z_]+\.[a-z_]+\s*=' src/presentation/` → **零命中**（已达成，R-12 规则上线后由构建期保证）；
  - 每个子批的行为等价性由**该功能已有的真帧回归**保证（导出浮层、差异对比、查找替换、状态栏各有既有用例）→ 逐子批跑对应测试文件；
  - 变异验证：把某个新方法改成空实现 → 对应用例红。

##### 实施偏差 · 子批 1（`export_ui`，2026-10-20，commit `96c2fa8`）

1. **处数：方案列 7 处，实测 8 处**。多出的是 `export_view.rs` 关闭分支里
   对 `running` 的复位——它紧邻的 `close()` **本身就把 `running` 置 false**，
   故这处直写是冗余的。处置：直接删除（**不是**改成调方法），行为等价由
   修正后的 `closing_export_clears_the_running_flag` 钉住。
   方案的「处数只是下限」再次成立（同 R-15 的 9 处 vs 方案 5 处）。
2. **方法名与实测不符**：原文给的 `set_export_error()` / `clear_export_result()`
   / `set_export_path()` 在 `export_view.rs` 里**没有对应的直写**
   （错误/结果/路径分别走状态结构体已有的 `fail` / `succeed` / `close`，
   路径输入框是 `TextEdit` 绑定）。实际新增的是 6 个选项写入口
   （`set_export_long_image` / `set_export_include_toc` / `set_export_embed_images`
   / `set_export_page_size` / `set_export_paginate` / `set_pdf_include_toc`）
   + `open_export_overlay`。
3. **「59 处」的口径含 13 处误报**：检测正则 `app\.[a-z_]+\.[a-z_]+\s*=` 会
   把 `==` 比较（`app.export_ui.format == *format`）也当成赋值。
   实测**真实赋值**改造前为 **46 处**，子批 1 清零 8 处后剩 **38 处**。
   R-12 上线时检测正则必须排除 `==`，否则会误报 13 处。
4. **`TextEdit` 的 `&mut` 绑定是已知边界**：输出目录与文件名输入框以
   `TextEdit::singleline(&mut app.export_ui.output_dir)` 绑定，egui 的文本
   控件必须拿到 `&mut String`。改成「返回 `&mut String` 的访问器」只是把直写
   换个写法，所有权并未收敛；故保留原样，且 R-12 规则只针对赋值形态，
   不会误伤这两处。

- **顺带修掉的两处无牙齿判据**（改造前就存在，均为「测试自己写值再断言」）：
  - `pdf_toc_implies_pagination`：自设 `paginate = true` 再断言它为真，
    联动漏了也绿 → 改为走 `set_pdf_include_toc(true)`；
  - `closing_export_clears_the_running_flag`：自设 `running = false` 再断言，
    `close()` 清不清 running 根本没测 → 删掉那行自设。
- **新增 7 例**（一律走 `EditorApp` 方法入口，不走「给字段赋值再断言」的自指
  判据）+ `mut_r11_1.py`（已清理） **变异 10 轮全捕获、0 无牙齿**。
- **门禁**：fmt OK / check-arch OK / clippy 0 lint /
  test **2055 passed / 0 failed / 4 ignored**（基线 2048 → +7）。

##### 实施偏差 · 子批 2（`diff_ui`，2026-10-20，commit `5f5a7ab`）

1. **处数吻合**：`diff_view.rs:70/94/95` 实测 3 处直写，新建
   `src/app/frame/diff.rs` 提供 `open_diff_overlay` / `set_diff_error` /
   `clear_diff_selection` 三个写入口；`open_overlay` 的 diff 初始化同批下沉。
2. **新增 0 例 + `mut_r11_2.py`（已清理） 变异 5 轮全捕获、0 无牙齿**（5 个方法/
   分支入口各一轮）。
3. **门禁**：fmt OK / check-arch OK / clippy 0 lint / test 全绿。

##### 实施偏差 · 子批 3（按 state 结构体分组，2026-10-21）

- **分组决策变更**：方案原定「其余 49 处一并提交」，实测按 state 拆成
  `editor` / `sidebar` / `preview` / `ui` / `settings_ui` / `find` /
  `status_bar` / `shortcut_overview` 八组，**每组独立 commit**（比方案原定更可
  回退）；真实存量（排除 `==` 误报）改造前 46 → 子批 1 后 38 → 子批 2 后 36 →
  editor 后 25 → sidebar 后 17 → preview 后 **12**，与剩余八组口径一致。
- **`editor` 组（11 处，commit `92f7755`）**：`frame/editor.rs` 加 7 方法
  （`set_editor_selection` / `set_editor_spell_menu` / `clear_editor_spell_menu`
  / `set_editor_scroll_y` / `set_editor_line_numbers` / `toggle_editor_spell_check`
  / `toggle_editor_word_wrap`）。
  - **类型错修正**：`EditorUiState` 注释写「字节偏移」但字段是裸 `usize`
    （`cursor: usize` / `anchor: Option<usize>`），首版签名按 `ByteOffset` 写
    直接编译失败 → 修正为裸 `usize`。
  - **切换类保留取反语义**：`spell_check` / `word_wrap` 原 `x = !x`，方法化为
    `toggle_*`，**不**改为「调用方传布尔」（那会把切换语义挪到视图且调用点散落）。
    判据用「调用后 == !调用前」断言，不写死初始值（默认值由偏好播种）。
  - `mut_r11_3_editor.py`（已清理） **变异 7 轮全捕获**。
- **`sidebar` 组（8 处，commit `de0bd68`）**：新建 `frame/sidebar.rs` 7 方法
  （`start_sidebar_rename` / `set_sidebar_selected` / `set_sidebar_outline_selected`
  / `set_sidebar_search_query` / `set_sidebar_search_scope` /
  `toggle_sidebar_search_option` / `restore_sidebar_draft`）。
  - `mut_r11_3_sidebar.py`（已清理） **变异 7 轮全捕获**。
- **`preview` 组（5 处，commit `aa6c252`）**：新建 `frame/preview.rs` 3 方法
  （`store_preview_render` / `set_preview_blocks` / `set_preview_scroll_y`）；
  `store_preview_render` 把 `blocks` / `rects` / `scroll_y` **三字段作为同一语义
  单元一起落定**，防无声漏写（这正是第 4 轮部分实现变异要钉的）。
  - import 修正：`BlockRect` 来自 `scroll_sync_state` 而非 `preview_ui_state`
    （`BlockView` 来自 `application::dto`）。
  - 新增 3 例 + `mut_r11_3_preview.py`（已清理） **变异 4 轮全捕获**（含「只写
    blocks、漏写 rects/scroll_y」的部分实现变异）。
  - **门禁**：fmt OK / check-arch OK / clippy 0 lint / test **2076 passed /
    0 failed / 4 ignored**（sidebar 后 2073 → +3）。

#### R-12 · 新增机械规则「presentation 不得直写 app 状态字段」（2026-10-21 完成，commit `eeaab0f`）

- **改造内容**：在 `tools/arch_check.rs` 增加一条**赋值形态**规则（token 级）：对**所有表现层**（`presentation` 及其全部子层 `state` / `views` / `rich` / `widgets` / `icons`），扫描 `app.<标识符>( .<标识符> )* =` 这种**对 app 状态字段的赋值**，命中即报违规。
  - 实现要点：词法器新增 `Assign` / `EqEq` 两种 token，把 `=` 与 `==` 区分开；规则只认紧随其后的**单个 `=`（赋值）**，天然排除 `==` 比较（`!=` 是 `Bang` 后跟 `Assign`，同样不命中）。
  - **已知边界（自然放行，无需豁免机制）**：
    - `&mut app.<state>.<field>`（egui 控件必须拿 `&mut String` / `&mut bool`，如 `TextEdit::singleline(&mut app.x.y)`、`ui.checkbox(&mut app.x.y, …)`）——那是**借用**，序列以 `)` 收尾不以 `=` 收尾，自动不命中；
    - `app.<state>.<field> == z` 是**判据里的相等比较**，靠 `EqEq` token 排除。
  - 故意**不**用 `arch-allow` 豁免：合法的状态变更都走 `EditorApp` 的方法入口（R-11 已收敛），不存在「必须直写」的合法场景；`&mut` 借用由 token 形态排除而非豁免。
  - **只允许对 `app` 的方法调用做状态变更**；`app` 的字段读取（`app.active_doc`、`app.ui.theme`）仍允许。
- **涉及文件**：`tools/arch_check.rs`（新规则 + 词法器 `Assign`/`EqEq`）、`src/presentation/views/editor_view.rs`（R-11 两层 grep 口径漏掉的**单层**直写 `app.editor_text =` 收口到 `EditorApp::set_editor_text`）、`src/app/frame/editor.rs`（新增 `set_editor_text`）、`tests/arch_violation_fixture.rs`（4 个注入/边界用例）、`mut_r12.py`（变异验证；脚本已清理）
- **影响范围**：构建期规则；命中数为 0 才可启用 → 启用即 0 命中（R-11 已将真实直写清零）。
- **依赖关系**：**严格依赖 R-01（token 级匹配）+ R-11（直写清零）**。
- **与方案原设计的偏差（实施偏差）**：
  - 方案原文设计「只查已知 UI state 字段名清单」+ 豁免 `// arch-allow: app-state-write` + 也扫 `src/app/frame/**`。实际实施**不维护字段名清单**（直接禁「任何 `app.<…> =` 赋值」，因为 app 状态字段的所有权本就该全部经方法入口）、**不用豁免**（无合法直写需放行）、**不扫 `app/frame/**`**（那是状态所有者自身，写自己的字段合法）。方案设计是 R-11 之前的推测，R-11 落地后「全部经方法」已成事实，故规则可以更干净。
  - 方案验收用例 `app.export_ui.error = Some(..)` 仍被本规则覆盖（属 `app.<ident>.<ident> =` 形态）。
- **验收判据（实测全绿）**：
  - 注入用例：`presentation/views/foo_view.rs` 写 `app.a.b = 1;` → `scan()` 必须报「不得直写 app 状态字段」；`app.x = z;`（单层，对应 `app.editor_text =` 残留形态）同样必报；
  - 边界用例：`&mut app.a.b` 借用、`app.a.b == 1` 比较 → 必须放行（0 命中）；
  - 全量 `cargo run --bin check-arch` → OK（0 命中）；
  - 变异验证（`mut_r12.py`，脚本已清理）：把规则判定改成 `if false` → 两条注入用例当场变红（1/1 变异点全捕获）。

#### R-13 · 表现层内部依赖规则（`views` ↔ `state`）

- **改造内容**：明确并**机械守卫**表现层内部方向：`views/` 可读 `state/`，`state/` **不得**依赖 `views/`；`rich/` 不得依赖 `views/` / `state/` / `theme/` / `widgets/`（ADR-007 已有编译期保障，本项把它**机械化**）。同时规定 `widgets/` 与 `icons.rs` 不得依赖 `views/`。
- **涉及文件**：`tools/arch_check.rs`（`layer_of` 增细粒度：`presentation/views`、`presentation/state`、`presentation/rich`、`presentation/widgets`）、`docs/architecture-constraints.md` §4（登记）、`docs/adr/ADR-010`（记录）
- **影响范围**：`presentation` 内部 123 文件的依赖方向；当前预期 0 违规（实测 `rich/` 零外部依赖 ✓）。
- **依赖关系**：**依赖 R-01**（token 匹配）。**不依赖 R-12**（两者是各自独立的守卫项）→ 本项可在 M3 内独立完成。
- **风险点**：
  - `views/` 里可能有**合法的**对 `state/` 内部实现的依赖（如 `state/find_replace_ui_state/pattern.rs` 的私有函数被视图调用）→ 需要先跑一次全量扫描看命中数，超预期则**放宽到具体文件**而非放弃规则；
  - 规则粒度变细 → `layer_of` 逻辑变复杂 → 需保证判定对**层根文件**（`src/presentation/state.rs`、`src/presentation/views.rs`）同样生效。**注意**：本项原文提到的「`build.rs` 的缓存签名（`signature()`）」已**不成立**——`build.rs` 早已移除内部缓存（实测确认注入的违规会被静默跳过），`signature()` 是与该记载自相矛盾的死代码，已在 R-01 实施时删除。当前 `build.rs` 每次运行都**无条件全量扫描**，是否重跑由 cargo 依据 `cargo:rerun-if-changed=src` 判断。
- **回滚方案**：规则的 `enabled` 标志置 false。
- **验收判据**：
  - `cargo run --bin check-arch` → OK；
  - 注入用例：在 `presentation/state/x.rs` 写 `use crate::presentation::views::y;` → 必须报违规；
  - 变异验证：把 `state` 的禁止清单清空 → 注入用例红。

##### R-13 实施结果（2026-10-20）

- **`layer_of` 细粒度化**：第二段为 `state` / `views` / `rich` / `widgets` /
  `icons` 时判对应子层。**层根文件与目录同规则**——剥掉末段 `.rs` 后
  `presentation/state.rs` 的第二段也是 `"state"`，与
  `presentation/state/foo.rs` 落同一子层（方案风险点点名要求）。
- **四条新禁令**（全部**叠加**父层 `PRESENTATION_FORBIDDEN`，不是替换）：

  | 子层 | 额外禁止 |
  |---|---|
  | `presentation/state/` | `crate::presentation::views` |
  | `presentation/rich/` | `crate::presentation::views` / `state` / `theme` / `widgets` |
  | `presentation/widgets/` | `crate::presentation::views` |
  | `presentation/icons.rs` | `crate::presentation::views` |

- **★ 方案预警的风险点真的发生了，按既定原则处置**：`views/` 独立成层后
  `allowed_for` 没跟上 → **35 个视图文件同时报错**（全是
  `use crate::app::frame::EditorApp` / `frame::PendingDialog`）。按方案
  「超预期则放宽到具体文件而非放弃规则」，把 `crate::app::frame` 窄豁免
  扩展到全部 presentation 子层——**窄前缀本身不变**（`bootstrap` /
  `crash_log` 等仍拦）。这不是「更严的架构」，而是规则没跟上自己的细化。
- **存量实测 0 处真实违规**（动手前摸查）：`state/`→`views` 命中 2 处、
  `rich/`→`views` 1 处，**全部在文档注释里**（词法阶段剔除）。故本项
  **不改变任何编译结果**——价值与 `PRESENTATION_STATE_FORBIDDEN` 相同：防未来松动。

**新增守卫 7 例**（fixture 75 → 82 条）：`state_may_not_depend_on_views`
（**方案点名判据**）/ `views_may_depend_on_state`（反向放行，钉住「禁的是
方向不是名字」）/ `rich_may_not_depend_on_its_siblings`（四兄弟逐一）/
`leaf_layers_may_not_depend_on_views`（widgets + icons）/
`the_layer_root_files_are_judged_too`（层根文件，方案风险点）/
`views_keep_the_narrow_app_frame_allowance` / `the_narrow_allowance_does_not_leak_to_the_rest_of_app`。

另修正 1 条既有用例的筛子：`the_state_subdirectory_is_judged_by_its_own_layer_name`
原先筛父层名 `presentation 层`，细粒度化后 `views/` 报告自己的层名
`presentation/views 层`——**是用例的筛子过时，不是实现错**。

**变异验证 8 轮全部捕获**（`mut_arch_r13.py`，脚本已清理）：state 的 views 禁令清空
（**方案点名**）/ views→state 反向也禁（过度收紧）/ rich 名单漏 theme /
widgets 不清 / icons 不清 / views 丢窄豁免 / 窄豁免外溢到整个 `crate::app` /
`layer_of` 丢掉根文件形态。其中 2 轮被 build.rs 架构校验先拦下
（`captured_build`），被测用例没机会跑——单独归类，不与「无牙齿」混淆。

**文档**：`docs/architecture-constraints.md` 新增 §4.0（内部方向矩阵）与 §9
（tests 库内部视角禁令，R-02 的规则真源）、§4.1 补第 7/8 条判定口径、
§4 shared 表补 `std::process`；新建 `docs/adr/ADR-010-presentation-internal-direction.md`；
`docs/README.md` ADR 索引补 010。

### 4.3 P2 组：一致性与文档

#### R-14 · 删除 `floor_char_boundary` 私有副本

- **改造内容**：删除 `src/application/workspace/service.rs:525` 的私有 `fn floor_char_boundary`，改调 `src/shared/geometry.rs:170` 的公开版本（`use crate::shared::geometry::floor_char_boundary;`）。
- **涉及文件**：`src/application/workspace/service.rs`（删 ~15 行 + 改调用点）
- **影响范围**：1 处；行为等价（两份实现算法相同，仅参数名与饱和处理略有差异 → 需逐行比对）。
- **依赖关系**：**R-10 依赖本项**（统一字符边界防护的调用点）。
- **风险点**：两份实现并非**逐字相同**——私有副本多了一层「索引越界时回退到末尾」的处理，公开版可能没有 → 直接替换会引入越界 panic。**必须先逐行 diff 确认等价**。
- **回滚方案**：`git revert`（改动极小）。
- **验收判据**：
  - `grep -rn "fn floor_char_boundary" src/` → **1 处**（只剩 `shared/geometry.rs`）；
  - 原有的 workspace 服务测试全部通过；
  - 变异验证：把调用改回私有副本 → 无测试变红（说明这条太弱）→ **补一条「多字节字符边界」用例**（如 emoji / 中英混排的偏移），先让新用例在替换前红、替换后绿。

#### R-15 · 路径与文件名处理收口

- **改造内容**（3 个小项，可合并为一个 commit）：
  1. **文件名提取统一**：`src/app/frame/dialog.rs:166` 与 `:253` 的 `path.rsplit('/')`（**不处理反斜杠**）、
     `src/app/frame/files.rs:441` 的 `rsplit('/')` → 统一走 `src/shared/path.rs` 的 `FilePath::file_name`；
     `files.rs:408` 与 `shared/text_format.rs:98` 已用 `rsplit(['/', '\\'])` → 一并收敛。
  2. **三个「取当前文档路径」方法合并**：`files.rs:61` `current_document_path() -> String`（返回 `"未命名文档"` 哨兵）、
     `editor.rs:301` `document_dir() -> Option<PathBuf>`、`workspace.rs:134` `active_document_dir() -> Option<String>`
     → 统一为一个 `current_document_file() -> Option<FilePath>`（`None` = 未命名），派生出 dir / name。
  3. **`capture.rs`↔`files.rs` 路径处理去重**：`capture.rs:86-93` 与 `files.rs:193-205` 的
     `FilePath::new(path).map_err(...)` + `refresh_text()` 逐字重复，且错误映射不一致
     （`AppError::Config` vs `AppError::domain`）→ 抽一个 `app/frame/files.rs` 的 `save_to_path()` 供两处调用。
- **涉及文件**：`src/app/frame/dialog.rs`、`src/app/frame/files.rs`、`src/app/frame/editor.rs`、`src/app/frame/workspace.rs`、`src/app/frame/capture.rs`、`src/shared/path.rs`
- **影响范围**：用户可见变化：**Windows 下原生对话框显示的文件名从「整条路径」变为「文件名」**（这是修 bug）；其余为内部重构。
- **依赖关系**：无前置。建议与 R-21 的「打开管线归位」一起做。
- **风险点**：
  - 改文件名提取会影响**对话框标题**的显示（用户可见）→ 必须有真帧或集成用例断言标题文案；
  - 三个方法的语义**不完全等价**（一个返回哨兵字符串、一个返回 `Option`）→ 合并时若漏掉「未命名文档」这个 UI 文案的来源，会出现状态栏显示空白。
- **回滚方案**：按 3 个小项分别 commit，可独立回退。
- **验收判据**：
  - 新增用例：`file_name_extraction_handles_windows_backslashes`（输入 `C:\a\b.md` → 期望 `b.md`）；
  - 新增用例：`dialog_title_shows_only_the_file_name_on_windows_paths`（走真实 `DialogOutcome` 路径）；
  - `grep -rn "rsplit" src/app/frame/` → 只剩 `rsplit(['/', '\\'])` 一种形态。

- **实施记录（2026-10-20，`67210f2` 之后）**：
  - **小项 1 的实际范围比方案列的更大**：全库 `rsplit('/')` 取末段共 **9 处**（方案列了 5 处），
    其中 app 层 3 处（`dialog.rs:166`、`:253`、`files.rs:441`）**只认 `/`**，是 Windows 上的真 bug；
    presentation 层 6 处（`quick_open_ui_state.rs`、`sidebar_ui_state/search.rs`、
    `sidebar_ui_state/tree.rs`、`diff_view.rs`、`sidebar_view/files.rs`、`welcome_view/recent.rs`）
    输入来自已归一化的扫描结果，当前不出错但属重复实现。一并收敛。
  - **落点与方案不同**：方案说走 `FilePath::file_name`（**值对象**方法），但值对象构造即校验
    （拒绝 `..`、拒绝空串）——含 `..` 的合法路径会被判成 `Err`，写成
    `FilePath::new(path).ok()` 就会**退化回「显示整条路径」**，比现状更糟。
    故在 `shared/path.rs` 新增**自由函数** `file_name_of(raw: &str) -> &str`
    （只做取末段，**不做任何校验**），9 处统一走它；
    `FilePath::file_name` 也改为委托它，两处规则不分家。
  - **小项 2 已做**，但**统一到了成本更低的一侧**：`active_document_dir` 原走 `get_document`
    （**全文快照**，O(全文) 克隆——问题 043 的教训）取一个路径，现改为走
    `get_tab_summary`（O(1)）；`document_dir` 原经 `current_document_path()` 的
    **哨兵字符串**（「未命名文档」）反推「有没有路径」，现直接用
    `current_document_file() -> Option<FilePath>`——**不再拿展示文案当判据**。
    新增 `EditorApp::current_document_file()` 作为唯一出处。
  - **小项 3 未做，方案的判断不成立**：`capture.rs:86-93` 的 `save_document_to` 与
    `files.rs:193-205` 的 `save_document` **不是逐字重复**——后者走 `SaveCommand`
    （含「另存为」语义与草稿清理），前者走 `SaveToPathCommand`（指定路径）。
    真正重复的只有 `ensure_document()` + `refresh_text()` 两行，抽出共享函数会
    制造一个语义混乱的入口。**如实记录：该项从范围中移除**，不强行实施。

#### R-16 · 格式白名单硬编码漏网 + 注释单源

- **改造内容**：
  1. `src/app/frame/workspace.rs:191,194` 的另存为建议名硬编码 `.md`（`format!("{stem}.md")` / `"未命名.md"`）→ 改为查 `shared::text_format` 的默认扩展名（新增 `TextFormats::default_extension()`）。
  2. `src/app/frame/workspace.rs:279-280` 与 `src/app/frame/files.rs:454` 的**注释里手写了白名单清单**（`md/markdown/mdown/mkd/txt/text/csv`）→ 删掉注释里的清单，改为指向 `shared::text_format::BUILTIN_TEXT_EXTENSIONS`（注释即引用，不复制常量）。
- **涉及文件**：`src/shared/text_format.rs`、`src/app/frame/workspace.rs`、`src/app/frame/files.rs`
- **影响范围**：另存为对话框的默认文件名（用户可见，仅当默认格式非 md 时才有差别）。
- **依赖关系**：无前置。
- **风险点**：`TextFormats::default_extension()` 的默认值选择需与产品口径一致（当前硬编码是 `.md`）→ 保持 `.md` 不变，只是把「硬编码」变成「可配置」。
- **回滚方案**：`git revert`。
- **验收判据**：
  - `grep -rn "md/markdown/mdown" src/` → 零命中；
  - `grep -rn 'format!("{stem}.md")' src/` → 零命中；
  - 已有「另存为」用例继续通过。

#### R-17 · widgets 收口：`search.rs` 反模式 + 第 4 个组件

- **改造内容**：
  1. `src/presentation/views/sidebar_view/search.rs:176-182` 的 `allocate_ui_with_layout(vec2(avail, 0.0), …)` + 裸 `Label::truncate()`
     → 改用 `src/presentation/widgets/text.rs:45` 的 `elided_segment`（高度取自已排控件，0 高写不出来）。
  2. **新增第 4 个 widgets 组件**：「行矩形 + `Sense::click` + hover 底色（`CornerRadius::same(4)`）」——
     该配方在 4 处逐字重复：`quick_open_view.rs:226-229`、`command_palette_view.rs:223-226`、
     `toolbar_view.rs:421-423`、`toolbar_view.rs:473-475`。符合 ADR-008 的三条标准（≥2 处重复 / 写错不报错 / 直觉写法看起来合理），
     建议命名为 `widgets::row::clickable_row_with_background`（或给现有 `clickable_row` 加 `background: bool` 参数）。
- **涉及文件**：`src/presentation/views/sidebar_view/search.rs`、`src/presentation/widgets/row.rs`、`src/presentation/widgets/text.rs`、`src/presentation/views/quick_open_view.rs`、`src/presentation/views/command_palette_view.rs`、`src/presentation/views/toolbar_view.rs`
- **影响范围**：4 处 hover 底色 + 1 处限宽段；用户可见效果**应完全不变**（同一像素）。
- **依赖关系**：无前置。
- **风险点**：
  - `clickable_row` 当前**不画底色**（4 个调用点在侧栏，底色由父级画）→ 若加 `background` 参数，需确认不会与父级底色**叠加两次**（视觉变深）；
  - `search.rs` 改 `elided_segment` 后**行高**会变（065 的垂直错位正是 0 高导致）→ 可能影响侧栏行距，需真帧比对；
  - 新组件的参数设计要**让错误写法说不出口**（ADR-008 的判据），否则第 5 个调用点又会写回老样子。
- **回滚方案**：`git revert`（2 个子项分开 commit）。
- **验收判据**：
  - `grep -rn "allocate_ui_with_layout" src/presentation/views/` → 只剩 `preview_view/table.rs:284`（表格单元格，**不属于限宽段**场景）与 `widgets/text.rs` 自身；
  - 变异验证：把 `elided_segment` 的高度改回 0 → `search.rs` 的行高/垂直位置断言红；
  - 4 处 hover 底色改造后，各有真帧用例断言**命中区覆盖整行**且**底色矩形位置与改造前一致**（记下改造前的像素坐标作为基线）。

**实施结果（2026-10-21，`de63c70`）**：

- **两个子项均落地**：
  1. `search.rs::group_header` 的 0 高 `allocate_ui_with_layout` 限宽段 → `widgets::text::elided_segment`，高度取行内已排控件矩形（参数类型 `Rect` 而非 `f32`，从类型层面杜绝「传 0 高」）。**写法顺序修正**：先空弱文本占位取行高基准 → `elided_segment` 文件名 → 右侧 `with_layout(right_to_left)` 放计数（`right_to_left` 先放计数会吃掉整行宽，使文件名段拿 ~0 宽，初版即栽此坑，见 `tests/presentation/search_alignment_test.rs` 修复记录）。
  2. **新增第 4 个 widgets 组件 `widgets::row::paint_row_background`**（而非给 `clickable_row` 加 `background` 参数）：收口 `quick_open_view` / `command_palette_view` / `toolbar_view` 导航箭头 / `toolbar_view`「全部关闭」共 4 处逐字重复的「选中→`accent_soft` / 悬停→`bg_tertiary` 圆角(4pt)底色」配方。独立封装的理由：4 处调用点中部分底色由父级先画、`clickable_row` 当前不画底色，加 `background` 参数会与父级底色**叠加两次**（视觉变深）——独立函数让「圆角半径 + 两种主题色」没有别的写法可选（ADR-008 判据）。
- **★ 与方案偏差（命名）**：方案建议 `clickable_row_with_background` 或给 `clickable_row` 加参数；实际选独立 `paint_row_background`，理由见上。函数只收口「圆角半径 + 两种底色」两行易错重复，不分配矩形、不返回命中区（背景须在内容之下绘制，调用方负责顺序）。
- **真帧判据**：`tests/presentation/search_alignment_test.rs::search_group_header_filename_and_count_share_one_baseline`（集成，1 例：分组头文件名段与命中数同线，带内 top 离散 ≤ 0.5pt）；`row.rs` 单测 3 例（`paint_row_background_selected_draws_accent_soft` / `_hovered_draws_bg_tertiary` / `_draws_nothing_when_neither`，用 `Theme::dark()` + `ctx.run_ui` 抓 `Shape::Rect` 的 `fill` 比对主题色）。
- **变异验证 `mut_r17.py`（已清理）**：3/3 全捕获（elided_segment 高度归 0 → 对齐测试红；选中态底色改错 → 选中例红；悬停态底色改错 → 悬停例红）。
- **门禁四绿实测（2026-10-21）**：fmt✓ / check-arch✓ / clippy 0 lint（仅 lock-file 删除告警）/ test **2097 passed·0 failed·4 ignored**（R-06 后 2093 → +4：搜索对齐集成 1 + paint_row_background 单测 3）。
- **验收判据复核**：`grep -rn "allocate_ui_with_layout" src/presentation/views/` → 仅 `preview_view/table.rs:284`（表格单元格，非限宽段场景）；`search.rs:175` 的命中是**注释**说明旧反模式、非代码。与方案口径一致。

#### R-18 · 文档过期修正

- **改造内容**（逐条对应评估报告 §P2-5 的 11 条）：
  1. `docs/README.md:53` 的「egui + eframe 0.33.3」→ 0.37.0；
  2. `docs/README.md:64` 的「001–027」→ 「001–069」；
  3. `docs/README.md:47-58` 的 ADR 索引补 **007 / 008 / 009**（R-19 新增的 010 / 011 一并补）；
  4. `docs/README.md:50` 的「76KB」→ 实测值；
  5. `docs/testing.md:37` 与 `docs/PROGRESS.md:53,79` 的「1623 passed / 0 failed / 0 ignored」→ **当日实测数字**（含 ignored 数）；
  6. `docs/PROGRESS.md:175` 的 egui 版本号；
  7. `docs/11-预览区技术实现.md:369,371` 的 `views/math.rs`、`views/mermaid.rs` → `presentation/rich/math/`、`presentation/rich/mermaid/`；
  8. `docs/05-基础设施实现.md:40,56,63,70,105` 的 5 个测试文件名；
  9. `tests/e2e/main.rs:5` 与 `tests/integration/large_file_test.rs:472` 的文档路径 → `docs/archive/stage-reports/`；
  10. `docs/metrics.md:211` 的 exe 字节数 → 当日实测（**注意：每次构建产物会有 0.0x MiB 波动，只更新实测值不改结论**）；
  11. `docs/logs/问题记录1.txt` 的**正文标题格式两套并存**（001–056 用 `YYYY-MM-DD NNN` 独占行，057–069 用 `NNN · MM-DD ·` 单段）→ 统一为新格式（**只改 057–069 的标题行，不动正文内容**）。
- **涉及文件**：上述 8 个文件 + `docs/logs/问题记录1.txt`
- **影响范围**：纯文档；影响「一个事实一个家」的可信度。
- **依赖关系**：**依赖 R-19**（ADR 补齐后索引才完整）。
- **风险点**：
  - 改 `问题记录1.txt` 的旧条目标题会**破坏历史记录的逐字性**（该文件是 append-only 流水账）→ **只统一新条目的格式，不动 001–056**；本项第 11 条实际应改为「新增条目统一用新格式」并在文件头注明；
  - 测试数会随改造推进持续变化 → 在 `docs/testing.md` 标注「数字截至 YYYY-MM-DD」而非写死。
- **回滚方案**：`git revert`（纯文档，无风险）。
- **验收判据**：
  - 逐条 grep 核对 11 处**全部**与代码现状一致；
  - `docs/README.md` 的 ADR 条目数 == `ls docs/adr/*.md | wc -l`；
  - 交叉核对：`docs/testing.md` 的测试数 == 当日 `cargo test --all-targets` 的实测 passed 数。

#### R-19 · ADR 补齐

- **改造内容**：
  1. **ADR-010 · 动作路由与双意图**：`ActionRoute` 穷举表 + `Unbound` 显式声明（`app/frame/actions.rs:189-289`）
     + `RevealIntent` 双意图分流（`presentation/views/scroll_sync.rs`）→ 记录「为什么用穷举表而不用 `_` 兜底」「为什么双意图」；
  2. **ADR-011 · 图标系统与资源布局**：`presentation::icons` 的「规格 = 数据」（`icons.rs`）
     + 草稿旁车文件协议（`<id>.draft.path`，问题记录 069）+ 字体外置（T2 改变发行包形态）→ 记录「为什么图标不用字形」「为什么草稿用旁车」「为什么字体随包分发而非内嵌」；
  3. **ADR-012**（若 R-11/R-12/R-13 落地）：表现层状态所有权与内部依赖规则。
- **涉及文件**：`docs/adr/ADR-010-*.md`（新增）、`docs/adr/ADR-011-*.md`（新增）、`docs/adr/ADR-012-*.md`（可选）、`docs/README.md`（索引）
- **影响范围**：文档。
- **依赖关系**：**依赖 R-04 / R-10 / R-11** 的最终形态（否则 ADR 写的是过程稿不是定稿）。
- **风险点**：ADR 若写成「计划」而非「已定」会误导后续维护者 → **只写已落地的决定**，计划类内容放 backlog。
- **回滚方案**：`git revert`。
- **验收判据**：
  - `grep -rn "ActionRoute" docs/adr/` → 有记录；
  - `grep -rn "旁车\|sidecar" docs/adr/` → 有记录；
  - ADR 索引与 `docs/adr/` 文件数一致。

**实施结果（2026-10-21）**：

- **★ 与方案编号偏差（已说明）**：方案撰写时 ADR 只到 009；实际 R-13 已先行
  创建 `ADR-010-presentation-internal-direction.md`，**覆盖了方案原 ADR-012 的
  「表现层状态所有权与内部依赖规则」主题**。故 R-19 实际补的是 **ADR-011**（动作路由
  与双意图）与 **ADR-012**（图标系统与资源布局），而非方案原编号的 010/011/012。
  原方案 R-19 第 3 项（表现层状态所有权 ADR）已由 R-13 的 ADR-010 交付，本项目不再重复。
- **两份 ADR 均写「已落地决定」而非「计划稿」**（方案风险点明确要求）：
  - `ADR-011-action-routing-and-dual-intent.md`：① 动作路由——`action_route`
    穷举表 + `ActionRoute::Unbound` 显式声明、**故意不写 `_ =>` 兜底**（新增
    `UiAction` 变体编译失败逼你接线），配套守卫 `unbound_actions_are_explicit`；
    ② 跳转双意图 `RevealIntent`（Top / IntoViewport），枚举而非布尔参数。
  - `ADR-012-icon-system-and-resource-layout.md`：① 废止字形图标、全改向量手绘；
    ② 图标规格 = 数据（`IconOp` 表 + `Paint`/`Tone` 两维自由度 + 16×16 栅格，
    单测 `icon_ops_stay_inside_the_grid` 把守越界）；③ 草稿旁车协议
    `<doc_id>.draft` + `<doc_id>.draft.path`（问题 069，恢复保文件名）；④ 字体
    外置随包分发（`assets/fonts/` 运行时发现，非 `include_bytes!` 内嵌），R-20
    拆分后三来源共用同一条发现路径，打包脚本 `scripts/package-*` 约束布局不变。
- **README 索引同步**：`docs/README.md` 决策记录表补 **011 / 012**；现
  `001–012` 共 12 条，与 `ls docs/adr/*.md | wc -l` 的 **12** 一致（R-18 第 3 条的
  ADR 索引补 007/008/009/010/011 至此全部满足）。
- **验收判据复核**：`grep -rn "ActionRoute" docs/adr/` → ADR-011 命中；
  `grep -rn "旁车\|sidecar" docs/adr/` → ADR-012 命中；索引条目数 == ADR 文件数 == 12。

#### R-20 · `infrastructure/fonts.rs` 拆分

- **改造内容**：把 1111 行（生产 646 / 测试 465）拆成三层：
  1. `src/infrastructure/fonts/discovery.rs`：**路径发现**——`builtin_font_path()` / `discover_font_sources()` /
     `scan_project_fonts_dir()` / `system_mono_candidates()`（三平台）/ `candidate_paths()`；
     改为**接受注入的根目录列表**（`fn discover_font_sources(roots: &[PathBuf]) -> Vec<FontSource>`），
     真实文件系统只留一层薄适配（`default_roots()`）。
  2. `src/infrastructure/fonts/assembly.rs`：**装配**——`font_definitions()` / `prepend_selected_to_monospace()` /
     `extend_with_cjk()` / `load_cjk_font()` / `extend_with_math_fallback()`；
     改接受 `Arc<dyn FontBytesLoader>`（或直接接受字节），使「有 CJK 字体 → 装上」这条逻辑**可测**。
  3. `src/infrastructure/fonts.rs`（保留为门面）：`font_definitions()` 组装上述两层。
- **涉及文件**：`src/infrastructure/fonts.rs` → 拆为 `src/infrastructure/fonts/{mod,discovery,assembly}.rs`；
  `src/infrastructure.rs`（加 `pub mod fonts;` 目录声明）；`src/app/frame.rs`（调用点不变）；
  `src/shared/fonts.rs`（`EditorFontChoice` 契约不动）
- **影响范围**：字体发现与装配；**用户可见行为不变**（同一套字体路径与装配顺序）。
- **依赖关系**：无前置，但建议在 R-18 之后（避免文档改两遍）。
- **风险点**：
  - **发行包形态**：字体路径依赖 `current_exe()` 附近的 `assets/fonts/` → 拆分时**不能改这条约定**（T2 刚落地，且 `scripts/package-*` 三个脚本按 `assets/fonts/` 分发）；
  - 现有 18 个测试**依赖真实字体文件存在**（`real_cjk_font_provides_glyphs_for_chinese` 等）→ 拆分后应区分「纯逻辑单测」（无资产依赖）与「真实资产集成测试」（**在 `assets/fonts/` 缺失时应 skip 而不是 fail**——当前是 fail，干净克隆会红）。
- **回滚方案**：`git revert`（拆分前后的 `font_definitions()` 签名保持一致则调用点零改动）。
- **验收判据**：
  - `cargo test` 在**删除 `assets/fonts/` 的临时副本**下仍全绿（真实资产测试自动 skip）；
  - 新增单测：`discovery_prefers_project_fonts_dir_over_system_candidates`（注入临时目录，断言优先级）；
  - 变异验证：把 `prepend_selected_to_monospace` 的置顶顺序反过来 → 新增用例红；
  - `cargo build --release` 后 exe 体积与拆分前**差值 < 0.05 MiB**（记录前后数字）。

**实施结果（2026-10-21，`1c90c2f`）**：

- **三层拆分落地**：
  1. `fonts/discovery.rs`：路径与存在性发现。`discover_font_sources(roots: &[PathBuf])` 改为接受**注入的根目录列表**，真实 FS 只留薄适配 `default_roots()`；`scan_project_fonts_dir(roots)` / `candidate_font_paths(roots)` 跟随注入；`system_mono_candidates` / `candidate_paths` / `builtin_font_path` 等保留。
  2. `fonts/assembly.rs`：`extend_with_cjk` / `prepend_selected_to_monospace` / `extend_with_math_fallback` / `extend_with_builtin_cjk` / `load_cjk_font` 纯装配逻辑（CJK 追加到末尾、选中字体置顶进内置族等**顺序约束**不变）。
  3. `fonts.rs`：门面，`font_definitions` / `editor_font_choices` / `FontSetup` 组装两层；`pub use` 重导出，外部调用点（`app/frame.rs` 的 `crate::infrastructure::fonts::editor_font_choices()` / `font_definitions()`）**零改动**。
- **★ 与方案偏差（可接受）**：方案提到装配层改接受 `Arc<dyn FontBytesLoader>`；实际 `extend_with_cjk` 本就接受字节、各函数已纯函数化，且 `font_definitions` 门面已用占位字节可测，故**未引入额外 trait**，避免过度设计。`font_definitions` 留在门面（非 assembly）以表达「门面组装两层」的归属。
- **真实资产测试 skip 化（关键）**：原 6 个依赖内置字体文件（`assets/fonts/`）的用例在干净克隆下会**失败**；新增 `builtin_font_assets_present()` 守卫，缺失时静默跳过而非 panic——满足「干净克隆仍全绿」验收。发现并修复两处漏加守卫（`math_fallback_sits_between_western_and_cjk` / `builtin_cjk_sits_after_system_cjk_in_both_families`，二者 `.expect("…应已挂载")` 在资产缺失时 panic）。
- **可测性验证**：新增注入式单测 `discovery_prefers_project_fonts_dir_over_system_candidates`（临时目录注入根，断言项目字体被发现且优先于系统候选）；变异 `mut_r20.py`（已清理） 2/2 全捕获（选中字体置顶改追加 / CJK 追加改插到开头）。
- **门禁四绿实测（2026-10-21）**：fmt✓ / check-arch✓ / clippy 0 lint（-D warnings）/ test **2098 passed·0 failed·4 ignored**（2097 + 1 新单测）；`cargo build --release` 后 exe 11.37 MiB，纯文件拆分零逻辑/依赖变化，体积差 ≈ 0（< 0.05 MiB）。

### 4.4 P3 组：低成本清理包

#### R-21 · 低成本清理包（8 个小项）

建议**合并为一个 commit**（每项改动都 ≤ 20 行，单独成 commit 反而增加噪声），但**每项独立可回退**（用 `git revert -n` + 分块暂存）。

| # | 小项 | 涉及位置 | 验收 |
|---|---|---|---|
| 1 | **命名冲突**：`presentation/theme/mapper/highlight_cache.rs`（egui `Galley` 缓存）改名 `galley_cache.rs`；`application/markdown/service/highlight_cache.rs`（syntect span 缓存）改名 `span_cache.rs` | 2 个文件 + 全部 import | `grep -rn "highlight_cache" src/` 的两个路径语义不再同名 |
| 2 | **`shared` 归位**：`shared/text_format.rs`（5 个使用点**全在 app 层**）下沉到 `src/app/text_format.rs` 或 `src/infrastructure/text_format.rs`；`shared/lru.rs`（application 3 + presentation 1）与 `shared/text.rs`（application 3 + presentation 7）**归位到调用方**或明确改名 | `src/shared/*`、调用点 | `src/shared/` 减少 3 个模块；`shared` 的定位回到「值对象 + trait 契约」 |
| 3 | **`shared` 内部循环**：`shared/text_format.rs:36` ↔ `shared/dialog.rs:26` 互相 import → 随第 2 项一并消除 | 同上 | `shared` 内无环 |
| 4 | **打开管线归位**：`files.rs:438` `open_path` 只是 `workspace.rs:298` `open_workspace_file` 的薄包装；且 `workspace.rs:312` **反向调用** `files.rs:400` `can_open_as_text`（方向与命名相反）→ 打开管线的全部逻辑归到 `workspace.rs`，`files.rs` 只留「保存 / 新建 / 状态栏」 | `src/app/frame/files.rs`、`workspace.rs` | `open_workspace_file` 是打开文档的**唯一**实现 |
| 5 | **通用状态栏方法归位**：`files.rs:46-52` 的 `set_status_error` / `set_status_notice` 被 `workspace.rs` / `find.rs` / `tabs.rs` 多处调用 → 移到 `src/app/frame/status.rs` | 新增 1 文件 + 调用点 | 状态栏操作集中在一处 |
| 6 | **编辑器操作归位**：`files.rs:113-147` 的 `insert_at_cursor` 属编辑器操作 → 移到 `editor.rs` | 2 文件 | — |
| 7 | **纯函数归位**：`workspace.rs:502` `group_search_hits`（搜索结果分组）被 `outline.rs:3` 反向导入 → 移到 `src/app/frame/find.rs` 或 `src/shared/`（若纯且层无关） | 2 文件 | 无跨模块反向导入 |
| 8 | **判据加强**：`tests/infrastructure/pulldown_parser_test.rs:251` `thematic_break_is_parsed` 只断言 `!blocks.is_empty()` → 断言块类型与块数（区分「解析出 hr」「当普通段落」「解析成空」） | 1 个测试 | 变异验证：把解析器的 hr 分支去掉 → 用例红 |

另含 2 项**可选**（视时间）：
- `FontId::monospace(...)` 硬编码 **12 处**（字号来源各异）→ 收口到 `theme.metrics` 的具名字段；
- `src/domain/workspace/repository.rs:47` 的 doc 注释反向引用 `crate::app::frame::EditorApp::request_delete` → 改引 application 层契约。

- **改造内容**：下表 8 个小项（命名冲突 1、`shared` 归位 2、`shared` 内部环 3、打开管线归位 4、状态栏方法归位 5、编辑器操作归位 6、纯函数归位 7、判据加强 8）+ 2 项可选。
- **涉及文件**：`src/presentation/theme/mapper/highlight_cache.rs`（改名 `galley_cache.rs`）、`src/application/markdown/service/highlight_cache.rs`（改名 `span_cache.rs`）、`src/shared/{text_format,lru,text}.rs`、`src/app/frame/{files,workspace,editor,find,status}.rs`（`status.rs` 新增）、`src/domain/workspace/repository.rs`、`tests/infrastructure/pulldown_parser_test.rs`。
- **影响范围**：`shared` 模块数由 8 降为 5；`app/frame` 内部模块职责重划；**用户可见行为零变化**（纯搬迁 + 改名 + 1 条判据加强）。
- **依赖关系**：第 3 项依赖第 2 项；第 4 项建议在 R-15（路径收口）之后，避免同一批文件改两遍；第 8 项独立，可并行。
- **风险点**：
  - **搬迁引入环**：`shared` 下沉到 `app` 后，`app` 层可能反向依赖 `shared`（当前 `shared/text.rs` 被 presentation 用 7 次）→ 必须逐项确认新归属层不产生新环，`cargo run --bin check-arch` 未必覆盖 `shared` 内部，需 `grep` 复核 import 图；
  - **改名漏改**：`highlight_cache` 改名影响 2 个模块的 import，若有 `mod.rs` 之外的路径引用会编译失败（可发现，但会拖长该 commit）；
  - 第 8 项的 hr 判据加强若解析器行为与断言不符，会暴露**既有 bug**（此时不是改造引入的）→ 需先确认现状。
- **回滚方案**：`git revert` 整个 commit；若已拆分提交，用 `git revert -n <sha>` + 分块暂存仅回退出问题的子项。
- **验收判据**：
  - `ls src/shared/` → 模块数 ≤ 5；
  - `grep -rn "highlight_cache" src/` → 0 命中（两处均已改名）；
  - `grep -rn "use crate::app" src/presentation/` → 0 命中新增（净变化为 0）；
  - `cargo test --all-targets` 全绿，且第 8 项用例通过「去掉 hr 分支 → 红」的变异验证。

**实施结果（2026-10-21，`de0d3d8`，合并一个 commit）**：

- **6 项落地**：
  1. **命名冲突**：`presentation/theme/mapper/highlight_cache.rs` → `galley_cache.rs`（Galley 缓存）、`application/markdown/service/highlight_cache.rs` → `span_cache.rs`（span 缓存）；6 处 import + 5 处注释路径同步更新，`highlight_cache::` 模块路径残留清零（`clear_highlight_caches` 等同名函数/字段不动）。
  2. **打开管线归位**：`can_open_as_text` / `unsupported_format_notice` / `report_unsupported_format` / `open_path` 从 `files.rs` 并入 `workspace.rs`（紧随 `open_workspace_file`），「守卫 / 拒绝文案 / 入口 / 实现」同家；`open_workspace_file` 仍是打开文档唯一实现。
  3. **状态栏方法归位**：`set_status_error` / `set_status_notice` 并入**既有** `frame/status_bar.rs`（R-11 建的「状态栏写入口」）。
  4. **编辑器操作归位**：`insert_at_cursor` 并入 `frame/editor.rs`（与 `move_line` / `run_format` 同簇）。
  5. **纯函数归位**：`group_search_hits` 从 `workspace.rs` 并入 `frame/find.rs`，`outline.rs` 导入改指 find。
  6. **判据加强**：`thematic_break_is_parsed` 加强为「块数 == 3 + 中间块是 `BlockKind::Rule` + 源码区间恰好覆盖 `---`」；变异 `mut_r21.py`（已清理） **2/2 全捕获**。
- **★ 与方案偏差（3 处，均已核实并记录）**：
  - **第 3 项目标文件**：方案写「移到 `src/app/frame/status.rs`」；实际 frame 已有 `status_bar.rs`（R-11 建、职责即「状态栏写入口」），并入它而非新建文件——状态栏写入口真正集中在一处。
  - **第 5 项前提**：方案称 `group_search_hits` 被 `presentation/views/outline.rs` 反向导入；实际 `outline.rs` 在 `src/app/frame/`，属 app 内部导入，搬移无跨层风险。
  - **第 2 / 3 项（shared 归位与内部环）未做**：方案前提「`text_format` 5 个使用点全在 app 层」**实测为假**——`text_format` 被 `presentation/state/sidebar_ui_state.rs` 用、`lru` 被 `presentation/views/preview_view.rs` 用、`text` 被 4 个 presentation 文件用；下沉到 app/application 会触发「表现层 → app」check-arch 违例。这些模块本就属于 shared（值对象 + 纯函数层），**不搬移**；`shared` 内部环（`text_format` ↔ `dialog`）随之搁置，解耦需另寻方案（如 `FileFilter` 下沉到更低层），超出低风险清理范围，登记待后续评估。
- **门禁四绿实测（2026-10-21）**：fmt✓ / check-arch✓（0 违规）/ clippy 0 lint（`-D warnings`）/ `cargo test --all-targets` = **2098 passed / 0 failed / 4 ignored**（逐行 `grep '^test result:'` 求和；纯搬迁 + 改名 + 加强既有用例，测试数不变）。
- **验收判据复核**：`grep -rn "highlight_cache::" src/ tests/` → **0 命中**（模块路径层面）；`grep -rn "use crate::app" src/presentation/` 净变化为 0（搬移全在 app/frame 内部，check-arch 0 违规佐证）；第 8 项变异 2/2（hr 分支改永不匹配 → 红；Rule 降级成段落 → 红）。

---

## 5. 实施计划

### 5.1 里程碑总览

| 里程碑 | 名称 | 任务 | 预估 | 出口判据 |
|---|---|---|---:|---|
| **M0** | 准备 | 建分支 / 记录基线数字 / 补「基线快照」测试 | 0.5 人日 | 基线数字入 commit message，四门禁全绿 |
| **M1** | 低风险高收益 | R-14、R-15、R-16、R-18（部分） | 1.5 人日 | 4 项各自达标 |
| **M2** | 扩展性瓶颈 | R-07、R-08①、R-09、R-10 | 5 人日 | 「新增 X 只改 N 处」用例成立 |
| **M3** | 守卫加固 | R-01、R-02、R-03、R-13 | 4.5 人日 | 守卫的 4 类绕过全被检出；`tests/` 进扫描 |
| **M4** | 用户可见缺口 | R-04、R-05、R-06 | 3 人日 | 三处缺口闭环，各有真帧回归 |
| **M5** | 结构改造 | R-11（3 子批） | 6 人日 | 59 处直写清零 |
| **M6** | 收尾 | R-17、R-19、R-20、R-21 | 5 人日 | ADR 补齐；字体可测；widgets 收口 |

**合计约 25.5 人日**（不含评审与返工）。

> **口径说明（两个数字的差异是真实的，不是笔误）**：
> - **22.5 人日** = 附录 A 的 21 条改造项**逐项相加**（净开发量，不含准备与跨阶段协调）。
> - **25.5 人日** = 里程碑表的合计，含 **M0 准备 0.5 人日** + 各阶段的**集成与协调余量约 2.5 人日**。
> 实施时以**里程碑表**排期为准（排期需要余量），以**附录 A** 估算单条改造项的相对权重。

### 5.2 各阶段任务顺序与交付物

**M0 · 准备（0.5 人日）**

- 顺序：① 记录基线四道门禁的**实测数字**（`cargo test --all-targets` 的 passed/ignored 数、
  `check-arch` 输出、release exe 字节数）→ ② 建分支 `refactor/v11-54` → ③ 把基线数字写进本方案 §5.3 的表格。
- 交付物：基线数字表。
- 验收：四门禁全绿且数字被记录。

**M1 · 低风险高收益（1.5 人日）**

- 顺序：**R-14**（先做，因为 R-10 依赖它）→ **R-16** → **R-15**（含 R-21 的第 4/5/6 小项可并行）→ **R-18 的第 1/2/5/6/8/10 条**（文档数字类，与代码无耦合，可最后做）。
- 交付物：4 个 commit；`docs/` 数字与代码一致。
- 验收：
  - `grep -rn "fn floor_char_boundary" src/` → 1 处；
  - `grep -rn "rsplit" src/app/frame/` → 只剩统一形态；
  - `docs/README.md` ADR 条目数 == `ls docs/adr/*.md | wc -l`；
  - `docs/testing.md` 的测试数 == 当日实测。

**M2 · 扩展性瓶颈（5 人日）**

- 顺序：**R-07**（导出枚举，独立）→ **R-08①**（`persisted_key` 穷举，编译错误多但机械）→
  **R-09**（主题枚举，需先确认 `Custom` 变体处理）→ **R-10**（span 统一，**拆 2 个 commit**：
  先统一构造器、后合并样式表）。
- 交付物：4 个改造项共 5 个 commit；4 条「新增 X 只改 N 处」的用例。
  **M2 已完成**（2026-10-20）：5 个 commit 全部落地（R-07 `bbcbe20` /
  R-08① `8cdf083` / R-09 `b1f951b` / R-10 `7f96b55` + `ec8926a`），
  每项均有独立 commit + 独立实施记录 + 变异验证。
- 验收（**每项独立**）：
  - R-07：`grep -c "pub enum ExportFormatChoice" src/` → 0 ✅ **已达成**（`bbcbe20`；`parse` 穷举用例通过；新增源码级守卫「导出格式只有一处定义」+ 3 轮变异验证）；
  - R-08①：`grep "_ => return None" src/presentation/shortcuts/registry/action/impls.rs` → 0 ✅ **已达成**（`8cdf083`；`every_ui_action_has_an_explicit_persisted_key_decision` 通过；新增源码级守卫「`persisted_key` 不许有兜底」+「手写清单与枚举变体同步」+ 3 轮变异验证；**顺带修掉领域动作清单漏登记的真实缺陷**）；
  - R-09：`ThemeKind::from_str(as_str()) == self` 用例通过（**同时守住持久化兼容**）✅ **已达成**（`b1f951b`；真源下沉 `shared::theme`（**偏离原文**，理由见 §4.2 R-09「实施偏差」）；**修掉 `refresh_system_theme` 恒提前返回 + 契约值硬编码 3 处**；新增 18 例 + 2 轮变异验证）；
  - R-10：编辑区与预览区**同段代码的语法色逐 span 相同**（真帧对拍）✅
    **已达成**（`7f96b55` + `ec8926a`；三套实现收敛为一个构造器 +
    两张样式表合并为一份派发（**偏离原文**，理由见 §4.2 R-10「实施偏差」）；
    **修掉代码块同一行两种字号 + 重叠 span 文本重复**两个真实缺陷；
    新增 21 例 + 变异验证 19 轮）。
    **判据改写**：原文的「同一段 Markdown」不可达成——实测两侧**语法来源
    本就不同**（编辑区对全文用 `lang="markdown"`，围栏内代码不上色；
    预览区用围栏自己的语言单独高亮）→ 改为可达成的等价命题
    「**同一段代码 + 同一套 span** 经两条路径渲染出的**颜色与背景**序列
    逐段相同」（真帧，从 `Shape::Text` 的 `galley.job.sections` 取证）。

**M3 · 守卫加固（4.5 人日）**

- 顺序：**R-01**（token 匹配）→ **R-02**（`tests/` 纳入）→ **R-03**（`shared/system.rs` 端口化，
  此时才能把 `std::process` 加进 `SHARED_FORBIDDEN`）→ **R-13**（表现层内部规则）。
- **依赖修正**：**R-12 依赖 R-11（M5）清零 59 处直写** → 因此 **R-12 排到 M5 之后**，
  M3 阶段只做 R-01 / R-02 / R-03 / R-13（表现层内部规则**不依赖** R-11）。
- 交付物：4 个 commit；守卫的注入用例集。
- 验收：
  - 4 类绕过形态各有注入用例，**变异后同时变红**；
  - `tests/` 注入违规能被检出；
  - `grep -rn "std::process" src/shared/` → 0；
  - `cargo run --bin check-arch` → OK。
- **进度**：R-01 ✅（`96bb76b` + 收尾 `9d20c01`）｜R-02 ✅（2026-10-20，
  新增 `scan_with_tests` + 8 例 + 变异 6 轮全捕获，构建耗时 +4.6%）｜
  R-03 ✅（2026-10-20，`ExternalOpener` 端口化 + 9 例 + 变异 7 轮全捕获）｜
  R-13 ✅（2026-10-20，细粒度层判定 + 7 例 + 变异 8 轮全捕获，
  35 文件误报按既定原则处置）。**M3 四项全部完成**。

**M4 · 用户可见缺口（3 人日）**

- 顺序：**R-04**（先定 `InsertImage` 的规格，再接线；`Revert` 可先单独做）→ **R-05**（设置页）→
  **R-06**（切标签光标，**依赖 M5 的子批 1** → 若 M5 未完成则**推迟到 M5 之后**，不硬排）。
- 交付物：3 个 commit + 每项一条真帧回归。
- 验收：三处缺口各自的用户场景回归通过（撤销恢复内容 / 目标可设且重启保留 / 切回标签光标位置不变）。

**M5 · 结构改造：状态所有权（6 人日）**

- 顺序：**子批 1（`export_ui`）** → 门禁 + 导出相关测试 → **子批 2（`diff_ui`）** → 门禁 + 差异对比测试 →
  **子批 3（按 state 结构体分组：editor / sidebar / preview / ui / settings_ui /
  find / status_bar / shortcut_overview，每组独立 commit）** → 门禁 + 全量。
- 交付物：每个子批（含子批 3 的每组）一个 commit，当前已落 `96c2fa8` /
  `5f5a7ab` / `92f7755` / `de0bd68` / `aa6c252`，剩余 5 组待提交。
- 验收：`grep -rEn 'app\.[a-z_]+\.[a-z_]+\s*=' src/presentation/views/` → 0
  （当前剩 12 处，属剩余 5 组）；全量测试绿。

**M6 · 收尾（5 人日）**

- 顺序：**R-17**（widgets 收口）→ **R-20**（字体拆分）→ **R-19**（ADR）→ **R-21**（清理包）→ **R-18 剩余条目**。
- 交付物：3 个 commit + ADR-010 / 011（/ 012）。
- 验收：`grep -rn "allocate_ui_with_layout" src/presentation/views/` → 只剩表格单元格场景；
  字体测试在**无 `assets/fonts/`** 的副本下仍绿；ADR 索引与文件数一致。

### 5.3 进度记录表（随实施填写）

**基线（2026-10-20 实测，commit `a10d766` / 分支 `refactor/v11-54`）**

| 项 | 实测值 | 口径 |
|---|---|---|
| 测试 | **1862 passed / 0 failed / 4 ignored** | `cargo test --all-targets`；4 个 ignore 分布在 4 个目标（2 个性能基准 + 1 个生成型 golden + 1 个需外网），**均有意忽略** |
| 架构守卫 | **OK（未发现分层依赖违规）** | `cargo run --bin check-arch` |
| clippy | **0 lint** | `cargo clippy --all-targets --all-features -- -D warnings`（仅 Windows 增量编译锁文件清理告警，**非 lint**） |
| release exe | **11,911,168 B = 11.36 MiB** | `stat -c%s target/x86_64-pc-windows-msvc/release/rustmark.exe`；`docs/metrics.md` §2.6 记 11.34 MiB，差 18,432 B（0.16%）属同配置重建波动，**仍越 10 MB 红线 1.36 MiB** |
| 源码规模 | `src/` 302 文件 / 33 845 行 | 与评估报告 §1.1 同基线 |

| 里程碑 | 改造项 | 起始 | 完成 | 门禁数字（passed / ignored / check-arch） | 备注 |
|---|---|---|---|---|---|
| M0 | 基线 | 2026-10-20 | 2026-10-20 | 1862 / 4 / OK | 分支 `refactor/v11-54`；基线 commit `a10d766` |
| M1 | R-14 | 2026-10-20 | 2026-10-20 | **1863** / 4 / OK | `67210f2`；删私有副本 + 补「向下吸附」判据（变异验证：改向上 → 新用例红、旧用例绿） |
| M1 | R-16 | 2026-10-20 | 2026-10-20 | **1871** / 4 / OK | `bbb9a12`；**发现方案漏网的第三处硬编码**（侧栏新建预填名）；源码级守卫 + 2 轮变异验证 |
| M1 | R-15 | 2026-10-20 | 2026-10-20 | **1879** / 4 / OK | `e9dce13`；**修 Windows 路径显示 bug**（9 处收敛）；小项 3「方案判断不成立」未做并如实记录 |
| M1 | R-18（部分） | 2026-10-20 | 2026-10-20 | **1879** / 4 / OK | `38a819d`；第 1/2/4/5/6/7/8/9/10/11 条已做；第 3 条（ADR 索引补 010/011）**待 R-19** |
| M2 | R-07 | 2026-10-20 | 2026-10-20 | **1896** / 4 / OK | `bbcbe20`；唯一真源下沉 `shared::export_format`（**偏离原文**，理由见 §4.2 R-07「实施偏差」）；新增 17 例；变异验证：从 `ALL` 删 `Image` → 2 例红 |
| M2 | R-08① | 2026-10-20 | 2026-10-20 | **1909** / 4 / OK | `8cdf083`；`persisted_key` 改穷举（38 `Some` + 19 `None` / 共 57 变体，**修正原文「16 个」的偏差**）；②默认键位表合一**未做**（两侧表语义不同，领域无法表达「无默认键位」，牵动 `shared`）；**修掉领域 `ActionId::all()` 漏登记 `ToggleStatusbar` 的真实缺陷**（改「显示/隐藏状态栏」键位弹「尚未映射到领域动作」）；新增 13 例；变异验证 3 轮（兜底写回 / 领域清单漏登记 → 4 例红 / 表现清单漏登记 → 3 例红） |
| M2 | R-09 | 2026-10-20 | 2026-10-20 | **1927** / 4 / OK | `b1f951b`；主题种类收敛为唯一真源 `shared::theme`（**偏离原文**，理由见 §4.2 R-09「实施偏差」）；**六份**副本全部收敛（原文只提到三份）；**修掉 `refresh_system_theme` 恒提前返回**（跟随系统时系统外观变化不刷新）+ 契约值硬编码 3 处；新增 18 例；变异验证 2 轮（`as_str` 写错 → 2 例红 / 注入同构枚举 → 守卫红） |
| M2 | R-10 | 2026-10-20 | 2026-10-20 | **1979** / 4 / OK | `7f96b55` + `ec8926a`（拆 2 commit）；三套 span→`LayoutJob` 实现收敛为 `mapper::span_job::build_job`（差异由 `SpanPolicy` 显式参数化）；两张样式表合并为一份派发 `StyleSemantics::of`（**偏离原文**：字面「并表」会破坏字号/字体族/行高的刻意差异，改为「派发合并、度量分离」）；**修掉代码块同一行两种字号 + 重叠 span 文本重复**（8 字符 → 10/13 字符）两个真实缺陷；新增 21 例（对拍 13 + 行内 4 + 样式表 3 + 真帧 1）+ 变异验证 **19 轮**（详见 §4.2 R-10「实施结果」「实施偏差」） |
| M3 | R-01 | 2026-10-20 | 2026-10-20 | **1958** / 4 / OK | **提前至 M2 期间实施**（R-10 被打断时插入）；判定由「行首 `use `」重写为**词法 + use-tree 展开**；实测额外发现并修掉 8 类漏检 + 3 处实现缺陷；`crate::app` 禁令改为**更窄豁免前缀**（表现层放行 `crate::app::frame`，54 个文件依赖它）；豁免机制加**登记表**校验（未登记/过时均报错）；**规则真源由 `.agent/architecture.md` 迁入 `docs/architecture-constraints.md`**（`.agent/` 整目录不入库 → CI 上豁免机制死锁）；删死代码 `signature()`/`file_stamp()`；报告格式收敛到单一来源 `format_report`；用例 13 → **45** 条；变异验证 **10 轮**（详见 §4.1 R-01「实施结果」） |
| M3 | R-02 | 2026-10-20 | 2026-10-20 | **2005** / 4 / OK | 新增 `scan_with_tests`，`scan` 签名不变；`tests/` 侧统一判 `Layer::Tests`（**不按目录名判层**——`tests/domain/` 是测试目标划分不是层，按名判会误套禁令）；`TESTS_FORBIDDEN` 只列库层名，放行 `crate::support`（243 处）；报告带 `tests/` 前缀；帧预约规则照样罩住测试侧；新增 8 例 + 变异验证 **6 轮全捕获**；构建耗时 +0.27s（+4.6%，方案预估的 35% 指扫描量非总耗时）；存量实测 `crate::<库层名>` 真实引用 = 0（命中全在注释/字符串里） |
| M3 | R-03 | 2026-10-20 | 2026-10-20 | **2017** / 4 / OK | `shared/system.rs` 端口化：新增 `ExternalOpener` trait（`std::process` 移出 `shared`，`is_safe_to_open` 纯校验留下）；实现搬 `infrastructure/system.rs`；`Services.opener` + 装配链最深层 `assemble_with_opener`（**偏离原文**：直接给 `assemble_with_clock_and_dialogs` 加参数会一次性改 20+ 测试调用点，故新增最深层让旧链转发，既有调用点零改动）；预览区两处调用改经端口；`SHARED_FORBIDDEN` 加 `std::process`；新增 **9 例**（5 例假 opener 用户可见 + 4 例禁令）+ 变异 **7 轮全捕获** |
| M3 | R-13 | 2026-10-20 | 2026-10-20 | **2024** / 4 / OK | `layer_of` 细粒度化（`state`/`views`/`rich`/`widgets`/`icons` 五子层，**层根文件与目录同规则**）；四条新禁令全部叠加父层；**方案预警的风险点真的发生了**——`views/` 独立成层后 `allowed_for` 没跟上，**35 个视图文件同时报错**，按「放宽到具体前缀而非放弃规则」把 `crate::app::frame` 窄豁免扩到全部子层（窄前缀不变）；存量实测 **0 处真实违规**（命中全在文档注释）→ 本项不改变任何编译结果；新增 7 例 + 修正 1 条旧筛子 + 变异 **8 轮全捕获**；约束文档加 §4.0/§9，新建 ADR-010 |
| M4 | R-04 | 2026-10-20 | 2026-10-20 | **2044** / 4 / OK | `1462337`；Revert=从磁盘重载（非撤销，方案正文偏差，见 §4.2 R-04「实施偏差」）；InsertImage=选图→复制到 assets→插相对链接；`action_route` 的 `Unbound` 清单清零；新增 14 集成例 + 6 lib 例 + 变异 **11 轮全捕获** |
| M4 | R-05 | 2026-10-20 | 2026-10-20 | **2048** / 4 / OK | `bd61fd4`；设置页补写作目标输入框，`normalize_writing_goal` 纯函数 + 纳入取消回滚快照；新增 3 集成例 + 1 lib 例 + 变异 **2 轮全捕获**；**R-11 改造时需回头收口输入框直写**（详见 §4.2 R-05 风险点） |
| M5 | R-11 子批 1（`export_ui`） | 2026-10-20 | 2026-10-20 | **2055** / 4 / OK | `96c2fa8`；导出浮层 **8 处直写清零**（方案列 7 处，实测另有 1 处冗余写）；`open_overlay` 的 Export 初始化下沉到 `frame/export.rs`；新增 7 例 + 变异 **10 轮全捕获**；顺带修 2 处无牙齿判据；**方案「59 处」口径含 13 处 `==` 误报，真实赋值 46 → 38**（详见 §4.2 R-11「实施偏差 · 子批 1」） |
| M5 | R-11 子批 2（`diff_ui`） | 2026-10-20 | 2026-10-20 | 全绿 / OK | `5f5a7ab`；差异浮层 **3 处直写清零**；`open_overlay` 的 diff 初始化下沉到 `frame/diff.rs`；变异 **5 轮全捕获** |
| M5 | R-11 子批 3（editor/sidebar/preview） | 2026-10-21 | 2026-10-21 | **2076** / 4 / OK | `92f7755`+`de0bd68`+`aa6c252`；按 state 分组三 commit，清零 **24 处**（editor 11 / sidebar 8 / preview 5）；每组独立变异验证（7/7/4 轮全捕获）；`src/presentation/views` 真实直写由改造前 46 降至 **12** |
| M5 | R-11 子批 3（ui/settings_ui/find/status_bar/shortcut_overview） | 2026-10-21 | 2026-10-21 | **2086** / 4 / OK | `e8157a2` + `3b4dc00`；收尾 8 处（ui 4 / settings_ui 4 / find 2 / status_bar 1 / shortcut_overview 1），新建 `frame/ui.rs` / `frame/status_bar.rs` / `frame/shortcut_overview.rs`、复用 `frame/settings.rs` / `frame/find.rs`；6 例 + 变异 **6 轮全捕获**；**`src/presentation/views` 真实直写由 12 降至 0，R-11 整体完成** |
| M4/M5 后 | R-06 | 2026-10-21 | 2026-10-21 | **2093** / 4 / OK | `a62d620`；`EditorApp` 新增 `per_doc_cursor: HashMap<DocumentId,(usize,usize)>`（随关标签 / 全关清除）；`switch_document` 先 stash → `refresh_text` → restore（按新文本 `byte_to_char` 写 `pending_cursor` 让 egui 主动重设脱字符）；`close_document` 仅「关当前页」才 restore（否则表里是陈旧快照，会把最新光标打回旧值）；`open_file` 新文档**不** restore（避免 `pending_cursor=(0,0)` 污染紧邻操作，实测修 `enter_at_various_cursor_positions`）；`tests/app/tab_cursor_test.rs` 三层判据（表记/取 + pending_cursor 排入并被渲染帧消费 + 真帧读 `TextEditState` 脱字符）+ 变异 **3/3 全捕获** |
| M5 后 | R-12 | 2026-10-21 | 2026-10-21 | **2090** / 4 / OK | `eeaab0f`；`tools/arch_check.rs` 新增 token 级赋值形态规则（表现层 `app.<…> =` 必报、`==`/`&mut` 自然放行）；顺带收口 R-11 两层 grep 漏掉的单层直写 `app.editor_text =`（→ `EditorApp::set_editor_text`）；新增 4 例 + 变异 **1 轮全捕获**；**R-11 成果由构建期机械固化** |
| M6 | R-17 | 2026-10-21 | 2026-10-21 | **2097** / 4 / OK | `de63c70`；widgets 收口：search 限宽段 0 高反模式→`elided_segment`（Rect 高度杜绝传 0）+ 新增第 4 组件 `paint_row_background` 收口 4 处行底色（独立封装而非给 `clickable_row` 加参数，避免与父级底色叠加两次）；新增 4 例（搜索对齐集成 1 + paint_row_background 单测 3）+ 变异 **3/3 全捕获** |
| M6 | R-20 | 2026-10-21 | 2026-10-21 | **2098** / 4 / OK | `1c90c2f`；fonts.rs 拆 discovery/assembly/门面三层（discover_font_sources 改接受注入根目录、default_roots 薄适配）；6 个内置资产测试在 assets/fonts 缺失时 skip；新增注入式单测 1 + 变异 **2/2 全捕获** |
| M6 | R-19 | 2026-10-21 | 2026-10-21 | 2098 / 4 / OK（纯文档，测试数不变） | ADR 补齐：方案原 ADR-012 主题（表现层状态所有权）已由 R-13 的 ADR-010 交付，故 R-19 实际补 `ADR-011` 动作路由与双意图 + `ADR-012` 图标系统与资源布局（向量手绘/规格即数据/草稿旁车/字体外置）；README 索引补 011/012，现 001–012 共 12 条与 `docs/adr/` 文件数一致 |
| M6 | R-21 | 2026-10-21 | 2026-10-21 | **2098** / 4 / OK（测试数不变） | `de0d3d8`；清理包 6 项合并一 commit：两处 highlight_cache 改名（galley/span）/ 打开管线归位 workspace / 状态栏方法并入既有 status_bar.rs / insert_at_cursor 归位 editor / group_search_hits 归位 find / thematic_break 判据加强；变异 **2/2**；★ 第 2/3 项（shared 归位与内部环）因会破坏 check-arch（presentation 依赖这些 shared 模块）未做 |

---

## 6. 技术选型与规范

### 6.1 技术选型

| 议题 | 选择 | 理由 |
|---|---|---|
| **架构守卫实现** | 继续用**文本扫描**（`tools/arch_check.rs`），不做 AST | 零额外依赖、构建期极快（当前 < 1s）；AST 需引入 `syn`/`ra_ap_syntax`，与 ADR-007「不为伪收益增加依赖」同源否决 |
| **token 提取方式** | 手写词法扫描（跳过注释与字符串） | 只需识别路径形态，无需完整词法；`syn` 的成本换不来收益 |
| **外部打开链接** | **端口 + 基础设施实现**（`ExternalOpener` trait） | 与既有 `shared::dialog::DialogPort`（`shared/dialog.rs:55`）完全同构，不引入新范式 |
| **状态所有权** | app 层**语义化方法**（非命令对象、非事件总线） | 项目已有「四入口全汇 `EditorApp::execute`」的路由范式（T6），加一层命令对象会与之重复 |
| **领域枚举作为唯一真源** | 表现层**直接使用**领域枚举，UI 专属数据用**独立函数表** | 避免「第二套同构枚举」；UI 文案不进领域层 |
| **默认键位表** | 保留**领域侧一份**（存 `(action_id, combo_str)`），表现层解析 | 避免把 `KeyCombo` 类型下沉到 `shared`（会扩大 shared 的公共 API 面，与「shared 瘦身」冲突） |
| **字体拆分** | 「发现（注入根目录）+ 装配（注入字节加载器）+ 真实 FS 薄适配」 | 沿用项目既有的**端口/适配器**手法（`DialogPort` / `Exporter` / `DocumentRepository`） |
| **新增依赖** | **零** | 全部改造用现有能力（`std`、`HashMap`、既有 trait） |

### 6.2 目录与命名规范

**目录**（沿用 `docs/architecture-constraints.md` §7 的「禁 `mod.rs`」约定）：

```text
src/
├─ app/frame/            组合根实现；新增业务模块时用「文件 + 同名目录」拆分
│  ├─ status.rs          (R-21-5) 状态栏通用操作
│  └─ export.rs          (R-11-1) 导出浮层状态初始化
├─ infrastructure/
│  ├─ fonts.rs           (R-20) 门面
│  └─ fonts/             (R-20) discovery / assembly
├─ presentation/
│  ├─ widgets/           只放「写错不报错」的不变式组件（ADR-008）
│  └─ theme/mapper/      排版与 span 应用的唯一真源（R-10）
```

**命名**：

| 对象 | 约定 | 示例 |
|---|---|---|
| 端口 trait | `<能力>Port` 或 `<能力>er` | `DialogPort` / `ExternalOpener` |
| 到期点 | 枚举变体 | `Deadline::AutoSave`（R-11 的规则已用 `Deadline`） |
| 缓存 | `<内容>_cache` | `galley_cache` / `span_cache`（R-21-1 改名） |
| 纯函数 | `<动词>_<名词>`，放调用方或 `shared` | `group_search_hits` |
| 领域枚举的唯一真源 | 领域内定义，表现层**不得**定义同构副本 | `ExportFormat` / `ThemeKind` |

### 6.3 接口与编码规范

1. **穷举优先于兜底**：任何「动作 → 去向」「变体 → 行为」的映射表**不写 `_` 兜底**
   （沿用 `app/frame/actions.rs:205-211` 已确立的纪律，并把这条纪律**扩展**到
   `persisted_key` / `from_str` / 主题映射等同类表）。
2. **让错误写法说不出口**：封装组件的参数类型要让「典型误用」在类型层面写不出来
   （ADR-008 的判据：`elided_segment` 的高度取 `Rect` 而非 `f32`）。
3. **单一真源**：一个概念只能有一个权威定义处；注释**不得复制**常量清单
   （R-16 的第 2 小项）。
4. **纯函数优先**：能从 IO 里剥出来的都剥成纯函数（`floor_char_boundary` /
   `group_search_hits` / `discover_font_sources`）→ 纯函数才有无头单测。
5. **状态写入必须经方法**：presentation 层不得直写 app 的 UI state 字段（R-11 / R-12）。
6. **注释写「为什么」**：本方案的每条改造项都要求在代码注释里写清
   「为什么这样、另一种写法为什么不行」——沿用项目既有的注释密度与风格。

---

## 7. 测试与验证

### 7.1 测试层次与范围

| 层次 | 位置 | 本次改造的覆盖要求 |
|---|---|---|
| **单元测试** | `src/**` 内联 `#[cfg(test)]`（当前 408 个） | 新增的纯逻辑必须有单测：token 提取、`from_str` 穷举、`persisted_key` 穷举、主题往返、字体发现优先级、span 对拍 |
| **集成测试** | `tests/`（当前 1 460 个，分 domain / application / infrastructure / presentation / app / integration / shared / e2e） | 每个改造项至少 1 条**走真实入口**的用例（如 R-04 走 `EditorApp::execute`、R-06 走真实 `TextEdit`） |
| **架构守卫测试** | `tests/arch_violation_fixture.rs` | 每条新规则 1+ 条注入用例 + 1 次变异验证 |
| **真帧渲染测试** | `tests/presentation/`（真帧 + `Shape::Text` / `Shape::Rect` 取证） | 涉及像素的改造（R-10 配色、R-15 文件名、R-17 行高）**必须**有真帧断言，且断言**绝对位置**而非「不越界」 |
| **性能测试** | `tests/integration/large_file_test.rs` + `tests/presentation/highlight_layout_bench.rs`（`#[ignore]`，release 跑） | **本方案的改造不得引入每帧回归**（尤其 R-10 统一 span 构造器、R-11 状态收敛、R-20 字体拆分） |
| **回归** | 全量 `cargo test --all-targets` | 每个改造项完成后**全量跑**，数字入 commit message |

### 7.2 通过标准

| 门禁 | 命令 | 通过标准 |
|---|---|---|
| 格式 | `cargo fmt --check` | 零差异 |
| 架构 | `cargo run --bin check-arch` | 输出 `OK（未发现分层依赖违规）` |
| 静态 | `cargo clippy --all-targets --all-features -- -D warnings` | 0 告警 |
| 测试 | `cargo test --all-targets` | 0 failed；**passed 数不减**（新增测试后应增） |

**附加通过标准（本次改造特有）**：

1. **每条新规则必须有变异验证**：把规则改坏 → 对应用例当场变红 → 记录在 commit message；
2. **每条「新增 X 只改 N 处」的用例必须真的失败**：故意漏加一处 → 编译失败或用例红；
3. **不引入每帧回归**：改造前后各跑 3 次 release 击键帧基准（`cargo test --release --test presentation bench -- --ignored --nocapture`），
   p50 差值 **< 3%**；超出需说明原因（**不允许「差不多就行」**）；
4. **体积不回归**：release exe 字节数与改造前差值 **< 0.05 MiB**（R-20 字体拆分尤其要盯）。

### 7.3 回归重点清单（改造后必须仍然成立的行为）

| 行为 | 现有守护用例（改造后仍应通过） |
|---|---|
| 自动保存到期帧生效 | `tests/integration/crash_recovery_test.rs` 的 e2e 用例 |
| 状态栏统计防抖 | `tests/presentation/status_bar_view_test.rs` |
| 导出浮层全流程 | `tests/presentation/` 的导出相关用例 + R-07 新增的 `from_str` 穷举 |
| 查找替换 | `tests/presentation/find_replace_view_test.rs` |
| 差异对比 | `tests/presentation/s6_11f_test.rs`（diff 浮层的**「动作→浮层→渲染」**集成用例；`DiffUiState` 的算法细节由 `src/presentation/state/diff_ui_state.rs` 的内嵌单测覆盖） |
| 侧栏行高与垂直位置 | `tests/presentation/` 的侧栏几何用例（**R-17 改行高后重点回归**） |
| 标签条不重叠 + 标题贴左 | `tests/presentation/` 的 tabs 用例 |
| 架构零违规 | `tests/arch_violation_fixture.rs` 的 `current_project_has_no_violations` |

---

## 8. 风险与依赖

### 8.1 外部依赖

| 依赖 | 现状 | 风险 | 应对 |
|---|---|---|---|
| **无新增第三方 crate** | 本方案零新增依赖 | 低 | 守住；任何「顺手加个 crate」都要走依赖复查（V11-26 的流程） |
| **egui 0.37.0** | 已冻结（ADR-001） | 低 | R-10 的 span 统一必须用 0.37 的 `LayoutJob`/`PlacedRow` 语义，**不得**引入 0.33 时代的写法 |
| **系统字体 / `assets/fonts/`** | T2 后随包分发 | **中**：干净克隆或不含资产的机器上，字体测试会红 | R-20 把「真实资产集成测试」改为**缺失即 skip**；并在 CI 记录该 skip |
| **无头 Chromium（PDF 导出）** | ADR-004 | 低 | 本方案不触碰导出链路；R-07 只改格式枚举 |
| **macOS / Linux 真机** | 缺（T7） | **中**：R-20 的字体路径发现是**平台相关**的（`system_mono_candidates` 三平台分支） | R-20 的发现层必须**注入根目录**测试，平台分支用**参数化用例**覆盖，不依赖真机 |

### 8.2 上游 / 下游影响

| 改造项 | 上游（依赖它） | 下游（依赖它） | 兼容性 |
|---|---|---|---|
| R-01 / R-02 | R-03、R-12、R-13 全部新规则 | `build.rs`、`cargo run --bin check-arch` | 判定变严 → 存量违规须在启用前清零（先「只报告」模式摸底） |
| R-03 | `preview_view` 的外链点击 | `Services` 结构体（新增字段） | `Services` 是**组合根内部**结构，新增字段不破外部 API；`ExternalOpener` 需有默认实现，否则 `Services::assemble*` 的多个入口都要改 |
| R-07 | 导出浮层、导出路由 | 持久化格式字符串 | `ExportFormat` 的**字符串值不变**（`"html"/"markdown"/"pdf"/"image"`）→ 用户配置兼容 |
| R-08 | 快捷键持久化 | 用户改键数据 | `persisted_key` 的**字符串值不变**（`"new_document"/"save"/…`）→ 已存配置继续生效 |
| R-09 | 主题设置与持久化 | 用户主题配置 | `ThemeKind::as_str()` 输出**不变**（`"dark"/"light"/"system"/"custom"`）；**但解析容错语义有变**：应用层 `to_theme` 由「精确匹配」改为「大小写不敏感 + trim」（原先 `"Dark"` 会报配置错误，现在被接受）——属**放宽**，旧配置不会因此失效 |
| R-11 | R-06、R-12 | 全部视图 | 纯重构，**无用户可见变化**；但 59 处改动量大 → 3 个子批分开 |
| R-20 | `app/frame.rs` 的字体装配 | `scripts/package-*` 三个脚本 | `assets/fonts/` 的**发行包布局不变**（脚本按此分发） |

### 8.3 兼容性问题

| 项 | 判定 | 说明 |
|---|---|---|
| 持久化配置格式 | **不变** | R-07 / R-08 / R-09 的枚举字符串值全部保持原样（各条都有「往返」用例守住） |
| 旧配置文件兼容 | **需验证** | `domain/preferences/value_objects/editor.rs` 的 `#[serde(default)]` 覆盖已知缺字段场景；R-08 改 `ActionId` 映射后要重跑「缺字段按默认读出」的用例（`tests/infrastructure/preferences_repository_test.rs:352`） |
| 草稿格式 | **不变** | R-03 / R-11 不触碰 `save_draft` / 旁车协议（问题记录 069） |
| **草稿旁车协议** | **不变** | 旁车（`<id>.draft.path`）是已投产的磁盘协议，任何改造都不得改其文件名与内容格式 → 列入 ADR-011 明确 |
| 二进制兼容 | 不适用 | 无 C ABI / 无插件机制 |

### 8.4 安全问题

| 项 | 现状 | 风险 | 应对 |
|---|---|---|---|
| **外部打开链接** | `shared/system.rs` 直接 `Command::new("cmd").args(["/C","start","",target]).spawn()` | **中**：若 `target` 含 shell 元字符（`&`、`|`、`^`）可能被 `cmd /C` 解释 → **命令注入** | R-03 端口化时**保留并强化**链接安全校验（现有的 `is_safe_to_open` 必须留在 shared 并补测试：当前它已拒绝 `javascript:` / `vbscript:` / `data:`，绝对协议只放行 `http/https/mailto/tel`（`file://` 与相对路径**按现状允许**，属本地文件），**须补一条「拒绝含控制字符的 target」**）。**这是本方案唯一涉及安全面的改造，必须单独写一组安全用例** |
| **路径穿越** | 打开文档走 `FilePath` 值对象 | 低 | R-15 统一路径处理时**不得**放宽白名单校验 |
| 内存安全 | Rust，无 `unsafe`（项目零 `unsafe`） | 低 | 守住「项目零 unsafe」 |
| 依赖漏洞 | 依赖已按 ADR-004 / V11-26 收窄 | 低 | 本方案零新增依赖 → 无新增面 |

### 8.5 方案级风险

| # | 风险 | 概率 | 影响 | 应对 |
|---|---|---|---|---|
| 1 | **R-11 的 59 处改写引入回归** | 中 | 高 | 3 个子批分开；每批只做「直写→方法调用」不顺手重构；每批跑**该功能的既有真帧回归**；变异验证 |
| 2 | **R-01 收紧后阻塞构建**（存量违规） | 中 | 中 | 先以「只报告不失败」跑全量摸底，清零后再启用；`enabled` 标志可一键关 |
| 3 | **R-10 的 span 语义差异导致预览渲染变化** | 中 | 中 | 保留旧实现并行一版做对拍；先合并构造器、后合并样式表（2 个 commit）；差异仅允许出现在重叠 span 背景层 |
| 4 | **R-04 的 `InsertImage` 规格未定** | 高 | 中 | **实施前必须先定契约**（插链接 or 选文件+复制资源）；未定则本项只做 `Revert`，`InsertImage` 单独排期 |
| 5 | **R-20 改动发行包字体路径** | 低 | 高 | `assets/fonts/` 布局**一字不改**；拆分前后 release 体积差 < 0.05 MiB；打包脚本 `scripts/package-*` 三个都要跑一遍 |
| 6 | **改造期间性能回归被忽略** | 中 | 中 | 每项改造后跑 release 击键帧基准 3 次取中位数，p50 差 < 3%；基线数字进 §5.3 表格 |
| 7 | **范围蔓延**（顺手做无关重构） | 中 | 中 | 每条改造项限定「只做一件事」；新增想法登记到 backlog 而非插入本方案 |
| 8 | **R-12 规则误伤合法写入** | 中 | 中 | 规则限定「已知 UI state 字段名清单」；先跑全量看命中数；`enabled` 开关 |

---

## 附录 A · 改造项索引表

| 编号 | 名称 | 组 | 涉及文件数 | 预估 | 依赖 | 风险等级 | 里程碑 |
|---|---|---|---:|---:|---|---|---|
| R-01 | `check-arch` 判定改 token 级匹配 | P0 | 2 | 1 人日 | — | 中 | M3 |
| R-02 | `tests/` 纳入架构扫描 | P0 | 4 | 0.5 人日 | R-01 | 低 | M3 |
| R-03 | `shared/system.rs` 端口化 | P0 | 5 | 1 人日 | R-01 | 中 | M3 |
| R-04 | 接线 `Revert` / `InsertImage` | P0 | 4 | 1 人日 | — | 中 | M4 |
| R-05 | 设置页补写作目标输入 | P0 | 3 | 0.5 人日 | — | 低 | M4 |
| R-06 | 切标签保留光标 | P0 | 4 | 1.5 人日 | R-11 子批 1 | 中 | M4/M5 后 |
| R-07 | 导出格式双枚举收敛 | P1 | 6 | 1 人日 | — | 中 | M2 |
| R-08 | `persisted_key` 穷举 + 键位表合一 | P1 | 4 | 1 人日 | — | 低 | M2 |
| R-09 | 主题双枚举收敛 | P1 | 6 | 0.5 人日 | — | 中 | M2 |
| R-10 | span→`LayoutJob` 统一 + 样式表合并 | P1 | 6 | 2 人日 | R-14 | 中 | M2 |
| R-11 | `presentation ↔ app` 状态所有权收敛 | P1 | 30+ | 6 人日 | — | **高** | M5 |
| R-12 | 「presentation 不得直写 app 状态」规则 | P1 | 3 | 0.5 人日 | R-01, R-11 | 中 | M5 后 |
| R-13 | 表现层内部依赖规则 | P1 | 4 | 0.5 人日 | R-01 | 低 | M3 |
| R-14 | 删 `floor_char_boundary` 副本 | P2 | 1 | 0.25 人日 | — | 低 | M1 |
| R-15 | 路径与文件名处理收口 | P2 | 6 | 0.5 人日 | — | 中 | M1 |
| R-16 | 格式白名单硬编码漏网 | P2 | 3 | 0.25 人日 | — | 低 | M1 |
| R-17 | widgets 收口（`search.rs` + 第 4 组件） | P2 | 6 | 1 人日 | — | 中 | M6 |
| R-18 | 文档过期修正 | P2 | 9 | 0.5 人日 | R-19 | 低 | M1/M6 |
| R-19 | ADR 补齐（010 / 011 / 012） | P2 | 5 | 0.5 人日 | R-04, R-10, R-11 | 低 | M6 |
| R-20 | `infrastructure/fonts.rs` 拆分 | P2 | 6 | 1.5 人日 | — | 中 | M6 |
| R-21 | 低成本清理包（8 小项） | P3 | 20+ | 1 人日 | R-10, R-15 | 低 | M6 |
| **合计** | **21 项** | | | **22.5 人日**（净开发量） | | | |

---

*本方案全部内容由 `docs/architecture-review.md` 的核实结论推导，未引入原文档之外的系统假设。
涉及的具体文件路径、符号名与行号均以基线 `e09ad2b` 为准；实施时若行号已漂，以符号名为准。*
