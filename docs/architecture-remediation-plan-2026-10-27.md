# rustmark 架构整改细化实施方案

> 文档编号：ADR-REM-2026-10-27　|　作者：架构评审（专家视角）　|　状态：进行中（R-01、R-02、R-03 B2 已落地；R-03 B1 首批 4 视图已落地——均为 2026-10-27；其余批次待排期）
> 综合来源：架构评审 `architecture-review-2026-10-23.md`、评估报告 `rustmark-evaluation-2026-10-25.md`、合规差额报告、多轮 refactor 波次（V11-56~67）、「去双源」批次（2026-10-26）、专家架构评审（2026-10-27）。
> 本文件是上述所有讨论的**可执行汇总**。任何一条改造项落地后，请在 `docs/PROGRESS.md` / `docs/v1.1-backlog.md` §3 回填实测数字，并同步本文件的「状态」列。

---

## 0. 当前基线（实测，落地前快照）

| 指标 | 数值 | 说明 |
|---|---|---|
| workspace crate 数 | **7** | root + 6 成员 |
| 本地依赖边 | **9 条，无环** | 编译期无循环依赖 |
| 测试基线 | **2227**（all-targets 2226 + doc 1；4 ignored 有意） | 引用旧数字前必须重测 |
| release 体积 | **10,550,272 B = 10.062 MiB** | 红线 11 MiB / 预警 10.5 MiB，`size_gate` exit 0 |
| 各 crate 规模 | root 91/21,988 · core 99/14,670 · ui 71/17,286 · infra 55/11,011 · shared 21/3,226 · icons 1/613 · actions 4/687 | 行数/文件数 |
| 门禁状态 | fmt / check-arch / clippy(-D warnings) / 全量测试 / doc / thresholds(121 条) / size_gate / release_dryrun(15/15) | 全绿 |
| 远端 | 本地 `main` 领先 `origin/main` ~80 提交 | agent 侧不可达，**push 由维护者本机执行** |

---

## 1. 背景与现状概述

### 1.1 演进脉络
项目从单体逐步重构为 7-crate workspace：抽离 `rustmark-actions`（动作词汇表）、`rustmark-icons`（资源）、`rustmark-shared`（工具+端口）、`rustmark-infra`（基础设施）、`rustmark-ui`（表现层 widgets/state/shortcuts/theme/rich）、`rustmark-core`（domain+application，已端口化、零 egui）。架构评审 12 项、评估报告 34 项、合规 P1/P2、去双源 5 处均已收口。

### 1.2 已解决的问题（简述）
- **硬伤清零**：编译期无环、core 内部 domain→application 反向引用 0 处、core 零 egui 依赖。
- **静默失效收口 5 处**：`thousands` 三份 → `shared::text`；`diff_view` 行号栏 → `gutter_numbers`/`paint_gutter`；`search` 增量行号 → `LineCursor`；`capture` 截图登记 → `capture_target`/`begin_capture_job`；配色默认 → 派生自领域。
- **门禁固化**：`default-members` 拓扑守卫、`[[test]]` 登记守卫（D-6）、`thresholds` 基线、体积门禁、发布预演。

### 1.3 仍存在的问题（两类维度）
**维度一 · root 与 crates 的依赖与交互**
- **A（P0）**：root 经 `pub use rustmark_core::{application, domain, shared}`（`src/lib.rs:49`）把 core 的整层转成 `crate::domain / crate::shared / crate::application`，并把 ui 子模块整体转发（`src/presentation.rs:11`）。边界不可见、层纪律被绕过、逻辑依赖未声明。
- **B（P1）**：表现层被劈成两半——views 留在 root、widgets/state/shortcuts/theme/rich 在 ui crate。根因：43 个视图文件依赖 `crate::app`（`EditorApp`），ui 不能依赖 root（否则环），views 被锁在 root。
- **C（P2）**：`rustmark-icons` 直接 `use rustmark_actions::UiAction`（`crates/rustmark-icons/src/lib.rs:68`），资源方绑定动作域；root 经 ui 间接才能看到 `UiAction`，链路脆弱。
- **D（P2）**：组合根 `src/app/bootstrap.rs` 直连领域仓储与端口接口；端口（架构契约）定义在 `rustmark-shared` 工具箱里，违背「契约归领域、实现归 infra」。

**维度二 · 各 crate 内部职责边界**
- **E（P1）**：`rustmark-shared`（21/3,226）三合一——通用工具（`text`/`lru`/`pattern`…）、架构端口（`dialog`/`event`/`system`/`image_asset`/`time`）、领域值对象（`theme`/`fonts`/`export_format`）混居。
- **F（P2）**：`rustmark-ui`（71/17,286）混装「应用外壳 UI」与「富内容渲染引擎」（`rich/{math,mermaid}` ~3k 行）。
- **G（P3）**：`rustmark-core`（14,670）单 crate 承载 domain+application，个别服务过大（`stats.rs` 639 / `search.rs` 626 / `find.rs` 535 行）。
- **H（P3）**：`rustmark-infra` 高复杂度算法单体（`highlighting/incremental.rs` 853 / `fonts.rs` 659 行）。
- **I（低）**：`actions` / `icons` 体量小、单一职责清晰，仅监控。

---

## 2. 改造目标（SMART）

| 编号 | 目标 | 可衡量判据 |
|---|---|---|
| O1 | 边界可见、依赖可声明 | root 仅依赖在 `Cargo.toml` 显式声明的 crate；`crate::domain` 在 `src/` 出现 0 处；`check-arch` 新增断言通过 |
| O2 | 共享契约与工具分离 | `rustmark-shared` 内部拆出 `kernel`（端口+值对象）与 `support`（工具）；契约与工具物理可分 |
| O3 | 表现层合一、root 瘦身 | views 迁入 ui crate 或经窄接口解耦；root 行数显著下降（目标 ≤ 组合根合理体量） |
| O4 | 富内容渲染可独立复用 | `rich/{math,mermaid}` 抽为 `rustmark-render`，端口化输出，不绑 egui 像素 |
| O5 | 消除脆弱间接依赖 | `icons` 不再依赖 `actions`；`actions` 在 root 显式声明（如需） |
| O6 | 超大服务/文件可维护 | `search`/`stats`/`find` 抽纯函数；`incremental` 等模块内拆分 |
| O7 | 门禁/文档纪律固化 | 跨 crate 搬迁的 §10 crate 地图、thresholds `source` 同步由守卫或脚本强制 |

---

## 3. 逐条细化改造项

> 角色缩写：Arch=架构师/技术负责人；Core=核心开发(core/infra/shared)；UI=前端(root views/ui)；Root=组合根/装配；QA=质量/门禁/变异；Docs=文档同步。

### R-01 · 收敛 root 命名空间联邦 + 消除未声明逻辑依赖　【P0，对应问题 A】

- **具体操作步骤**
  1. 删除 `src/lib.rs:49` 的整层转发 `pub use rustmark_core::{application, domain, shared}`，改为只保留对外稳定的少量类型（如需暴露，逐名 `pub use rustmark_core::application::X`）。
  2. `src/` 内部所有 `crate::domain` / `crate::shared` / `crate::application` 调用改为显式 `rustmark_core::domain::…`，使真实依赖在 `Cargo.toml` 可见。
  3. 若 root 确需 `shared` / `actions`，在 root 的 `[dependencies]` 显式声明；否则统一经 core/ui 取用，弃用 `crate::shared` 别名。
  4. 在 `tools/arch_check.rs` 新增断言：**root（src 根）不得出现 `crate::domain` 路径**，并配 fixture + 变异（删断言→红）。
  5. 同步 `docs/architecture-constraints.md` §10 crate 地图与 §3 定位声明。
- **负责角色**：Root（主）＋ Arch（守卫设计）＋ Docs
- **优先级**：P0（影响全局、是后续拆库绊脚石、靠词法守卫兜底）
- **预期产出**：`src/lib.rs` 收敛；`crate::domain` 0 处；新增 arch 断言 + 变异用例；依赖图在 `Cargo.toml` 完全可见。
- **验收点**：`cargo tree -p rustmark` 显示 root 仅依赖声明项；`check-arch` 新断言绿；全量门禁绿；体积逐字节不变（纯重构）。
- **状态**：✅ **已落地（2026-10-27）**——`src/` 37 文件共 109 处 `crate::domain` / `crate::shared` / `crate::application` → 显式 `rustmark_core::<层>::…`；`src/lib.rs` 的 `pub use` 标注为**公开 API shim**（仅维持对外/历史 `rustmark::<层>` 路径，根源码禁走）；`tools/arch_check.rs` 新增 `check_root_federated_layers` 守卫（root 仅拦 `crate::domain|shared|application` 字面三连，绕过 `rustmark_core::→crate::` 归一化，不影响 `crate::infrastructure` 装配路径、不误伤显式 `rustmark_core::<层>`）+ 3 个单元/变异用例（正向捕获 / 负向放行 / 注释豁免全绿）；全量门禁绿（fmt / check-arch / clippy `-D warnings` / `test --all-targets` 2241 passed / `test --doc` 1 / `thresholds` 121 / `size_gate` 10,548,736 B）。注：方案原文步骤 1 的「删除整层转发」改为**保留 re-export 作 shim + 守卫禁源码直取**的混合收敛，规避 524 处 `rustmark::<层>` 集成测试引用的大面积破坏性改动，收益（消除未声明逻辑依赖、依赖图可见）相同。

### R-02 · 拆分 `rustmark-shared` 为 kernel + support　【P1，对应问题 E】

- **具体操作步骤**
  1. 先 **internal 重排（零行为改动）**：在 `rustmark-shared/src/` 下建 `kernel/`（端口 `dialog`/`event`/`system`/`image_asset`/`time` + 值对象 `theme`/`fonts`/`export_format`）与 `support/`（工具 `text`/`text_format`/`lru`/`pattern`/`encoding`/`geometry`/`path`/`ids`/`error`/`result`/`constants`）。
  2. 保留原 `shared::*` 路径（`pub use kernel::*;` / `pub use support::*;`），使调用点零改动。
  3. 跑 `check_thresholds.py --fix`（注意 `--fix` 只回行号，路径搬迁要手改 `source`）。
  4. 评估是否升 crate：`kernel` 升 `rustmark-kernel`（端口真源）收益高；`support` 维持子模块即可。
  5. `constraints` §4.5 登记表随新文件更新（build.rs 机械拦截）。
- **负责角色**：Core（主）＋ Arch ＋ Docs
- **优先级**：P1（波及全部 6 crate、是端口来源混乱根、根因性强）
- **预期产出**：shared 内部职责二分；可选升 `rustmark-kernel`；调用点零改动。
- **验收点**：`tests/shared/internal_cycle_guard_test.rs` 无环断言绿；thresholds 121 条 OK；门禁绿；体积实测（拆 crate 各 +2~3KB 属预期，需记录）。
- **状态**：✅ **已落地（2026-10-27）**——内部二分完成（步骤 1~3、5）：19 个模块按职责迁入 `shared/kernel/`（端口 + 值对象：`dialog` `event` `system` `image_asset` `time` `theme` `fonts` `export_format`，8 个）与 `shared/support/`（工具：`constants` `encoding` `error` `geometry` `ids` `lru` `path` `pattern` `result` `text` `text_format`，11 个）；`shared.rs` 以 `pub mod kernel; pub mod support; pub use kernel::*; pub use support::*` 拼回原 `shared::<名>` 路径，公开路径零改动；`tests/shared/internal_cycle_guard_test.rs` 的扁平性断言扩展为递归收集 `kernel/`/`support/` 两个已知子目录（跳过伞形模块）；`constraints` §4.5 登记表 21 行路径键同步（含 2 个伞形模块 `kernel.rs`/`support.rs`）；`check_thresholds.py` 8 处 `via:` 路径手改；4 个测试级守卫（`export_format_guard` / `theme_guard` / `field_visibility_guard` / `default_extension_guard`）的硬编码扁平路径同步；全量门禁绿（fmt / check-arch / clippy `-D warnings` / `test --all-targets` 全目标 0 失败 / `test --doc` 1 / `thresholds` 121 / `size_gate` 见 metrics §2.8）。**步骤 4「升 crate（`rustmark-kernel`）」未随本次落地**：当前 `kernel`（端口/值对象）反而依赖 `support`（`dialog→error/path/text_format` 等），若升 crate 会让 `rustmark-kernel` 反向依赖 `rustmark-shared`，方向别扭；且方案标为「评估/可选」，留作独立评估项。**模块布局遵循 `CONSTR-03`（禁 `mod.rs`）**：`kernel.rs` + `kernel/`、`support.rs` + `support/` 属「`xxx.rs` + `xxx/`」的多层嵌套合法形态，全程未引入 `mod.rs`；`arch_violation_fixture::mod_rs_is_rejected_in_src_and_tests` 已补 `shared/kernel/mod.rs` / `shared/support/mod.rs` 两个嵌套用例，钉住 R-02 新子目录永不回退到 `mod.rs`。

### R-03 · 表现层合一（视图解耦 EditorApp）　【P1，对应问题 B】

- **短期 B2（低成本，必做）**
  1. `src/presentation.rs:11` 的 `presentation::{rich, shortcuts, state, theme, widgets}` 整体转发改为**显式逐项** `pub use`。
  2. 在 `check-arch` 新增守卫：「新增 ui 子模块须同步 root re-export + `docs/architecture-constraints.md` §10」；配 fixture。
  3. 把「views 留 root 是已知约束」写进 §10 与 backlog，消除人记。
- **中期 B1（根治）**
  1. 定义窄接口 `EditorViewContext`（trait 或 view-model 结构体），只暴露该视图所需的数据与命令，**替代整个 `EditorApp`** 传入视图。
  2. 43 个视图文件逐一改为接收 `&EditorViewContext`（按依赖拓扑分批，先叶子视图后聚合视图）。
  3. 视图不再 `use crate::app` 后，root 的 `src/presentation/views/` **只留视图**（装配与状态在别处）——注意 views 按 ADR-013 决策 3 **留 root**，**不**整体迁入 `rustmark-ui`（见下方「B1 进展」的接口形态条）。
- **负责角色**：UI（主）＋ Arch（接口设计）＋ QA
- **优先级**：P1（root 臃肿直接原因、视图不可独立测；短期 B2 成本低先做）
- **预期产出**：B2=显式 re-export + 守卫；B1=视图窄接口 + views 迁 ui + root 瘦身。
- **验收点**：B2——re-export 显式、守卫绿；B1——`src/presentation/views` 清空或仅留装配、`crate::app` 在视图 0 处、root 行数下降、视图可独立单测。
- **状态**：**B2 ✅ 已落地（2026-10-27）**——`src/presentation.rs:11` 的整组 `pub use rustmark_ui::presentation::{rich, shortcuts, state, theme, widgets};` 改为 5 行**逐项显式**；`tools/arch_check.rs` 新增 `pub fn check_ui_submodule_sync(manifest)` 并接入 `scan_project`：以 ui 侧 `pub mod X;` 为真源，对账 root 逐项 re-export 与 constraints §10.1，出现整组 `::{…}` 即报（与 `mod.rs` 同型：整组会让新子模块被静默带入，编译测试全绿而清单失配）；fixture `ui_submodule_must_be_synced_to_root_and_crate_map` 覆盖三场景（缺 root / 缺 §10.1 / 整组形式）+ 变异验证（关掉逐项断言 → 红）；constraints §10.1 补硬约束 4 并引用既有「views 留 root」条款。门禁全绿（fmt / check-arch / thresholds 121 / clippy `-D warnings` / `test --all-targets` 2242 passed / `test --doc` 1 / `size_gate` 10,548,736 B 逐字节不变——纯 re-export + 构建期守卫，不进 release 产物）。**B1 首批 ✅ 已落地（2026-10-27）**——详见下方「B1 进展」。**B1 未收官**（余 32 个视图，需按端口扩张分批推进）。

#### R-03 B1 进展（2026-10-27 首批）

- **实测口径更正**：方案原写「43 个视图」，实测 **36 个文件**引用 `crate::app`（`views/` 共 55 个 `.rs` / 14,138 行，其余不碰组合根）。以实测替代沿用。
- **接口形态**：`rustmark-ui::presentation::view_context::EditorViewContext`（trait）。
  - **不是**「一堆字段引用的结构体」：视图既要读写状态又要调命令，而命令实现在 `EditorApp` 上，调用点会同时需要 `&mut self.field` 与 `&mut self`——借用检查器直接拒绝。trait 只给**一个** `&mut`，状态访问与命令都是它的方法。
  - **放 ui 而非 root**：**契约要独立于组合根**——trait 描述「视图能对应用状态做什么」，而应用状态住在 ui 的 `state/` 下，契约与状态同侧才说得清；root 侧只提供实现。**注意**：views 按 ADR-013 决策 3 **留 root**（constraints §10.1 硬约束 2），故「为将来搬迁铺路」这条理由**不成立**，不可据此决策。
  - **端口收 `UiAction`（实测零新增依赖边）**：`UiAction` 属 `rustmark-actions`，但 `rustmark-core/src/lib.rs` 已有 `pub mod actions { pub use rustmark_actions::{MenuGroup, UiAction, action}; }`，ui 侧 `presentation/shortcuts/registry.rs` 已在 re-export ⇒ trait 里引用 `UiAction` **不产生**新的 crate 边，`rustmark-ui/Cargo.toml` 的依赖面仍恰为 `egui + rustmark-core`（`workspace_topology_test` 断言不变）。收它的理由是**不变量**：菜单 / 快捷键 / 命令面板三处必须走**同一条**执行路径（`src/app/frame.rs`：「视图层只负责**画**按钮，点击后同样回到 `EditorApp::execute`」）；`execute` 不进 trait 就得在 root 侧另留分支，等于在视图里复制一份分派。
  - **形态：5 个能力 trait + 1 个 bundle**：`ThemeAccess`（3）/ `UiStateAccess`（15）/ `DocumentAccess`（13）/ `CommandPort`（61）/ `ServiceQueries`（4，批 2 用），`EditorViewContext` 是纯 bundle（无自有方法）。摊平成一个大 trait ≈ `EditorApp`（183 方法 + 40 字段）的一半（96 / 183），正是「trait 上照抄 `EditorApp` 全部成员」的假解耦。Rust 1.86+ 的 trait object upcasting（1.99 已实测）保证将来再拆细粒度窄 trait 时**每视图只改一行签名**。
  - **`ServiceQueries` 交出服务本身，而非 N 个语义包装**（v2 修正）：实测 16 处 `services.` 触点**全部是只读调用**（`now()` / `map_scroll()` / `highlight()` / `spell_check()` / `spell_suggestions()` / `encoding_of()` / `has_writing_goal()` / `goal_progress()` / `open()`），故直接给**服务的只读借用**：`markdown_service()` / `document_service()` / `preferences_service()` / `external_opener()`。视图拿得到能力、**拿不到所有权**（所有权即装配权），故不能在视图里装配新依赖。**刻意不给 `assets`**：`AssetLoader` 住在 infra，本 crate 不可名，且唯一使用者 `preview_view/image_cache.rs` 因三重阻塞留 root（见下方「批 1 遗留」）。
  - **object-safety 三条硬约束**（trait 方法**不得含泛型参数**）：`set_diff_error(impl Into<String>)` → `String`；`apply_theme_field(impl FnOnce(&mut ThemeView))` → `&mut dyn FnMut(&mut ThemeView)`；`apply_editor_field` 同理。
  - **访问器统一 `_state` 后缀**：与 `EditorApp` 同名 inherent 方法永不撞名。撞名时 `fn theme(&self) { self.theme() }` 会按「inherent 优先」解析，今天恰好正确、明天 inherent 一删就静默递归。实现侧**一律**写 `EditorApp::x(self, …)` 全路径（名字不同的也写，保持一致），把这个隐患在结构上掐掉。
- **首批迁移（4 个文件 / 812 行）**：`about_view` / `stats_view` / `symbol_view` / `shortcut_overview_view`——正是 `overlays::show` 的四个 `match` 分支，内聚且可独立验收。`EditorApp` 实现该 trait，故 `&mut EditorApp` 在调用点自动强转 `&mut dyn EditorViewContext`，**已迁移与未迁移视图共存于同一次渲染**（`overlays.rs` 即混合态），这是渐进迁移的关键机制。
- **批 1 准备项 ✅（2026-10-27）**：**P-1** 3 个 root 私有类型搬进 `rustmark-ui/src/presentation/state/`
  （`PendingDialog` → `state/pending_dialog.rs`、`WorkspaceEntries` → `state/workspace_entries.rs`、
  `WorkspaceSearch` → `state/sidebar_ui_state/search.rs`），root 侧 `pub use` 转发，
  **公开路径逐字不变**（`rustmark::app::frame::PendingDialog` 等 6 处调用点零改动）；
  **P-2** `EditorApp::editor_id()`（关联函数、无 `self`，trait 方法没法在 `dyn` 上调）落为 ui 侧自由函数
  `editor_ui_state::editor_id()`，root 保留转发保旧路径；**P-3** 纠偏两处错误前提文档
  （「塞进 `UiAction` 就要新增 ui→actions 边」不成立 /「视图最终整体迁进 ui」与 ADR-013 决策 3 冲突）。
  迁移进度 **7/7 ✅ 批 1 收官**——步 1.1 `welcome_view` 树（3 个文件：`welcome_view.rs` /
  `welcome_view/actions.rs` / `welcome_view/hints.rs`）、步 1.2 `settings_view`
  树（5 个文件：`settings_view.rs` + `about` / `appearance` / `editor_prefs` /
  `shortcuts`）、步 1.3 `diff_view` / `find_replace_view` / `export_view` /
  `quick_open_view`（4 个文件）、步 1.4 `command_palette_view`（1 个文件，
  **`execute` 进 trait 的第一条**）、步 1.5 `overlays`（1 个文件，10 个分支全通）、
  步 1.6 `toolbar_view` 树（4 个文件：`toolbar_view.rs` + `controls` / `menu_bar`
  / `tab_strip`）、步 1.7 `sidebar_view` 树（5 个文件：`sidebar_view.rs` + `files` /
  `lint` / `outline` / `search`）+ `preview_view/inline/math.rs`（1 个文件）。
  **批 1 的 24 个零 services 视图全部解耦**，`PENDING_DECOUPLED_VIEWS` 余 **8** 条
  （全为 `services` 视图 + 6 个未登记子模块，留批 2 / 批 3）。
- **批 2 进展（`services` 视图解耦）**：按「状态栏 → 编辑区 → 预览区 → 主视图 +
  滚动同步」四步推进。
  - **步 2.1 `status_bar_view.rs`（已收官）**：契约新增 12 个方法
    （`UiStateAccess` +3：`editor_state_read` / `status_bar_state` /
    `status_bar_state_read`；`DocumentAccess` +5：`current_line_count` /
    `cursor_line_col` / `selection_line_col` / `current_document_dirty` /
    `current_document_kind`；`CommandPort` +4：`current_stats_for_status_bar` /
    `set_status_bar_encoding` / `arm_status_message_deadline` / `save_document`），
    trait 总数 96 → **108**。
    **`build_model` 保持只读**（收 `&dyn EditorViewContext`）——它是公开的纯读
    函数，集成测试有 **19 处**调用点直接断言它的输出；改成可变收��就得让这 19 处
    全部去拿独占借用，为一个纯读断言编造可变签名。
    **只读变体因此是 3 个而非拍板的 2 个**：`editor_state_read` 是被代码事实
    逼出来的第三条（`build_model` 要读 `word_wrap` / `spell_check` /
    `load_remote_images` 三个开关，而 trait 上通往 `EditorUiState` 的唯一出口
    是 `&mut` 的）。
    **行索引的性能不变量未破**：`LineIndex` 不搬（视图要的是「行数」一个数，
    不是 58k 项的偏移表），三处行相关文案改走 `current_line_count()` /
    `cursor_line_col()` / `selection_line_col()` 三个独立调用——扫描仍只有
    **一次**，因为行索引在应用层有缓存（键 = 文档 + 修订版本 + 文本长度），
    一帧内文本没变就命中（问题 053 的结论不变，只是「共用」从共用一个局部
    变量变成共用缓存）。
  - **步 2.2 `editor_view.rs` + `editor_view/spell.rs`（已收官）**：契约新增
    **10** 个方法（`UiStateAccess` +1 `editor_text_mut`；`CommandPort` +9：
    `apply_pending_editor_state` / `ensure_document` / `set_editor_selection` /
    `set_editor_text` / `flush_edit` / `close_spell_menu` / `set_spell_menu` /
    `set_editor_scroll_y` / `replace_spelling`），trait 总数 108 → **118**。
    **契约的参数类型必须与契约同侧**：`editor_ops::TextEdit`（一次编辑的区间
    替换描述）搬进 `rustmark_ui::presentation::view_context::TextEdit`，
    root 侧 `editor_ops` 以 `pub use` 转发 ⇒ 公开路径
    `rustmark::presentation::views::editor_ops::TextEdit` **逐字不变**；
    否则 trait 要反向依赖组合根（`editor_ops` 住在 root 的 `views/` 下）。
    **形态 B 的极端形态（`TextEdit::multiline`）解法**：egui 0.37 的
    `TextEditOutput` 不带生命周期参数（`galley: Arc<Galley>`），借用在
    `.show(ui)` 返回即结束 ⇒ 把独占借用关进**显式块**，块内不得调上下文的
    其它方法（含 `&self` 的 `markdown_service()`），需要的数据块外预取。
    **签名收窄波及的测试调用点**：`tests/presentation/spell_menu_test.rs`
    4 处 `spell_menu_for(&mut app, …)` → `(&app, …)`（函数变只读后 `&mut`
    成了多余的 mut，clippy `unnecessary_mut_passed` 报出），另 3 处
    `let (_dir, mut app)` 的 `mut` 随之失效——**方向与 R13 相反：这次是
    放宽**。
    **`flush_edit` 的钳制基准口径（易错点，已钉死）**：`domain_len` 必须是
    **变换前**的文本长度，不是 `editor_text.len()`。编辑区先把 `editor_text`
    拼成变换后的新串再调 `flush_edit`，若按新串长度钳制，「回车退出列表」
    这类**变短**的变换会把相对旧文本的 `byte_end` 砍短，领域层只删掉标记的
    前半个字符（空列表项 `4. ` 上回车残留成 `. `）。专项回归：
    `top_menu_test::enter_key_creates_lines_and_keeps_list_numbers_intact`（三步
    断言「续行 → 退出列表 → 空行」）与
    `enter_ime_test::list_enter_right_after_commit_does_not_pile_markers`
    （中文提交后同一时刻连按回车，零时钟推进）**均通过**。
  - **步 2.3 预览区（已收官）**：`preview_view.rs` + 7 个子模块
    （`blocks` / `code` / `list` / `media` / `table` / `toc` +
    `inline/text_render.rs`）。这些子模块原先经 `use super::*` 从 facade
    拿到 `EditorApp`（**glob 导入父模块的私有 `use` 别名**），故守卫的
    `first_crate_app_line` 只看见 facade 那一条 `crate::app`——**连带改形参
    不是可选项**：`show` 的签名一换，`draw_block` 与其下 18 个绘图函数的
    `&mut EditorApp` 全部编译不过。
    契约新增 **7** 个方法（trait 总数 118 → **125**）：
    `UiStateAccess` +2（`preview_state` / `preview_state_read`）；
    `DocumentAccess` +1（`document_revision`——只要一个 `u64`，绝不为它做
    全文快照，问题 043）；`CommandPort` +4（`set_preview_blocks` /
    `store_preview_render` / `replace_source` / `preview_image_fetcher`）。
    **`image_cache.rs` 的三重阻塞被一条闭包契约解开**（推翻了批 1 的评估，
    详见下条「批 1 遗留」的更正）：文件**不搬家**（仍住 root，故 `image`
    crate 与 `decode()` 原样可用），只把 `image_entry` 的形参从
    `&EditorApp` 换成 `&dyn EditorViewContext`；`AssetLoader`（infra，ui
    不可名）与文档目录（`pub(crate)`）封进
    `preview_image_fetcher(&self) -> ImageFetcher` 返回的
    `Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>`——**必须能跨线程**，
    否则远程图走不了后台下载（F-3），UI 线程会被网络阻塞。
    于是 `image_cache.rs` 也不再引用 `crate::app`，`EXEMPT_VIEWS` 在本轮
    **没有任何真实成员**。
    **形态 B 的两处新形态**：① `markdown_service()` 是 `&self`，写进
    `match` 判据会让借用覆盖整块 → 先把 `render_preview` 落到 owned 结果；
    ② `if vctx.preview_state_read().degraded` 同理 → 先落成 `bool`。
    `&str::clone()` 陷阱在 `list::toggle_task` 再次出现
    （`vctx.editor_text().clone()` 得到 `&str` 而非 `String`），同样改
    `to_string()` 并就地注释钉死。
    顺带把 `media.rs` 的外链打开从 `app.services.opener.open`
    改走 `external_opener()`（`ServiceQueries` 早已给出，此前只是没人用）。
  - **步 2.4 `scroll_sync.rs` + `main_view.rs`（已收官）**：两者同一步做，契约新增
    **8** 个方法（trait 总数 125 → **133**）：`DocumentAccess` +1
    （`should_show_welcome`）；`CommandPort` +7（`set_preview_scroll_y` /
    `set_preview_rect` / `set_split_dragging` / `set_sidebar_dragging` /
    `persist_split_ratio` / `persist_sidebar_visibility` / `persist_sidebar_width`）。
    **`ScrollSync` 一个 trait 方法都没加**：它的全部方法本就 `pub` 且住在 ui crate，
    经 `ui_state().sync.*` 全可达——为它加转发只会在契约上多出十几个
    「组合根转手一次」的空壳。
    **`main_view` 的 Panel 顺序一行未动**（egui 的 Panel 是命令式切分，
    顺序就是布局本身）：`toolbar → status_bar → sidebar → sidebar_divider →
    (welcome | editor → divider → preview) → overlays`，`scroll_sync::apply`
    仍在两侧都画完之后、`overlays` 之前。
    **形态 A 的新落点**：`ScrollSync::observe(now_ms, editor_y, preview_y)` 要
    独占借用 `ui_state()`，而两个实参分别住 `EditorUiState` / `PreviewUiState`
    ——同一表达式里就是两次借用。先把两个 `f32` 读出来（`Copy`）再调。
    同理 `sync.enabled && preview_visible && !focus_mode` 与
    `preview.degraded` 分属两个容器，`&&` 前半段关进块、后半段在外层求值。
    **白名单归零**：`PENDING_DECOUPLED_VIEWS = &[]`，常量与双向校验保留
    （批 3 新增视图时「零容忍」直接生效）。
- **批 1 遗留（移交给批 2 / 批 3）**：
  - ~~`preview_view/image_cache.rs` **本轮不迁**，留 root + 批 3 加
    `EXEMPT_VIEWS` 永久豁免；三重阻塞：`image` crate 属 root 独有依赖 /
    `AssetLoader` 在 infra / `document_dir()` 是 `pub(crate)`。~~
    **更正（步 2.3 实测）**：三重阻塞里只有第 2 条是真的，而它恰好可以用
    「返回 `'static + Send` 闭包」的契约封住：
    - 第 1 条只在「文件搬到 ui crate」时成立——本轮**不搬家**，
      `image` crate 与 `decode()` 照旧在 root 编译；
    - 第 3 条同理：文档目录由 root 侧的闭包内部取，ui 不需要名它。
    故 `image_cache.rs` 已随步 2.3 解耦，`PENDING_DECOUPLED_VIEWS` 只剩
    `main_view.rs` / `scroll_sync.rs` 两条（步 2.4 清空）。
  - `*_state()` 的**只读变体**按需补，只给真实需要的两个容器补：
    `status_bar_state_read()`（`status_bar_view.rs` 的读/写交替必然踩独占借用）
    与 `preview_state_read()`；其余 12 个容器纯写，不补（对称美学不值 14 个方法）。
  - `flush_edit` 的 `domain_len` 必须是**旧文本长度**，不是当前 `editor_text.len()`
    ——「退出列表」分支会把文本改短，按新长度钳制会把区间砍短，领域只删掉半个
    标记（已知症状：`` ` `` 残留成 `. `）。
  - `editor_text` 的独占借用：egui 0.37 的 `TextEditOutput` **不带生命周期参数**
    （`galley: Arc<Galley>`），借用在 `.show(ui)` 返回后立即结束 ⇒ 把借用关进
    显式块即可；但**块内**不得调 `vctx` 其它方法（含 `&self` 的
    `markdown_service()`），块外预取所需值。
- **守卫 `check_view_decoupling`**：白名单 `PENDING_DECOUPLED_VIEWS`（32 条）**双向**校验——名单外的 `crate::app` 引用报违规；名单内已解耦的条目也报（防名单腐烂）。词法级判定（`first_crate_app_line`），注释里提到旧路径不误伤。已接入 `scan_project`，故 `check-arch` bin 与 `build.rs` 构建期同时生效。
- **变异验证**：① 废掉 `first_crate_app_line` → 构建期 panic，列出 32 条陈旧；② 关掉反向陈旧检查 → fixture 红（「名单陈旧应报 1 条，实际：[]」）。
- **门禁**：fmt / check-arch / thresholds 121 / clippy `-D warnings` / `test --all-targets` **2243** passed（+1 新用例）/ `test --doc` 1 / `size_gate` 见 §metrics。
- **下一步**：按「零 `app.services` 的独立视图树」分批推进——批 1 = 24 个视图（`welcome_view` 树 / `settings_view` 树 / `diff_view` / `find_replace_view` / `export_view` / `quick_open_view` / `command_palette_view` / `overlays` / `toolbar_view` 树 / `sidebar_view` 树）**✅ 已完成**；批 2 = 8 个 `services` 视图 + 6 个未登记子模块 **✅ 已完成（白名单归零、`views/**` 零 `crate::app`）**；批 3 = 守卫收官（「视图不得出现 `EditorApp` 记号」新守卫 + `PENDING_DECOUPLED_VIEWS` 常量退役评估；`EXEMPT_VIEWS` 因 `image_cache.rs` 已解耦而**无成员**，是否保留该机制待裁决）。

### R-04 · 抽离 `rustmark-render`（富内容渲染引擎）　【P2，对应问题 F】

- **具体操作步骤**
  1. 在 `rustmark-ui/src/presentation/rich/` 内先做 **internal 端口化**：`math`/`mermaid` 产出「布局/绘制指令」而非直接绑 egui 像素。
  2. 待规模触发（既有规划：5.6k 行或 10 提交/年）升为 `rustmark-render` crate，ui 依赖它。
  3. 导出/headless 复用渲染时不再背 egui。
- **负责角色**：UI ＋ Core
- **优先级**：P2（影响 ui 体量 + 导出复用；对接既有规划，可与前序并行准备）
- **预期产出**：`rich/` 端口化；可选升 crate；导出路径复用渲染。
- **验收点**：`cargo tree -p rustmark-render` 无 egui（若升 crate）；渲染输出契约单测绿；门禁绿。

### R-05 · 解耦 icons 与 actions　【P2，对应问题 C/D 的 icons 部分】

- **具体操作步骤**
  1. `rustmark-icons` 定义独立小契约 `IconKey`（枚举或字符串），删除 `use rustmark_actions::UiAction`。
  2. `UiAction → IconKey` 的映射移到调用方（ui）。
  3. 若 root 确需直用 `actions`，在 root `[dependencies]` 显式声明，消除「经 ui 转发才可见」的脆弱链。
- **负责角色**：Core（icons）＋ UI ＋ Root
- **优先级**：P2（改造成本小，可早做）
- **预期产出**：icons 不再依赖 actions；映射上移；root 依赖显式。
- **验收点**：`cargo tree -p rustmark-icons` 无 `rustmark-actions`；icons 单测绿；门禁绿。

### R-06 · core 超大服务拆分（纯函数提取）　【P3，对应问题 G】

- **具体操作步骤**
  1. `application/workspace/service/search.rs`（626）：已抽 `LineCursor`，继续把 snippet 窗口、高亮区间提取为可独立测纯函数。
  2. `application/document/service/stats.rs`（639）、`application/document/find.rs`（535）：按子功能拆模块，纯逻辑外提。
  3. 每抽一处补不变量单测 + 变异验证（参照去双源批次纪律）。
- **负责角色**：Core ＋ QA
- **优先级**：P3（层方向正确，纯可维护性，非紧急）
- **预期产出**：超大服务降行数、纯函数可测；公开路径零变化（重导出）。
- **验收点**：目标文件 ≤ ~400 行；新增纯函数单测绿；体积逐字节不变。

### R-07 · infra 高复杂度大文件模块内拆分　【P3，对应问题 H】

- **具体操作步骤**
  1. `highlighting/incremental.rs`（853）按「diff 计算 / 区间合并 / 重排」分段为子模块。
  2. `fonts.rs`（659）、`parsing/pulldown_parser/image_size.rs`（584）、`stack.rs`（548）、`persistence/asset_loader.rs`（441）同理模块内拆分。
  3. 拆后补单测，确认行为等价。
- **负责角色**：Core（infra）
- **优先级**：P3（常规重构，最低）
- **预期产出**：单体文件降复杂度；行为等价。
- **验收点**：目标文件 ≤ ~400 行；单测绿；体积实测记录。

### R-08 · 监控 actions / icons 单一职责　【低，对应问题 I】

- **具体操作步骤**：仅监控，确认 `rustmark-actions`（词汇表）、`rustmark-icons`（资源）维持单一职责；R-05 完成后核查 icons 不再膨胀。
- **负责角色**：Arch
- **优先级**：低（确认项）
- **预期产出**：职责确认记录。
- **验收点**：无新增跨职责依赖。

### R-09 · 门禁/文档纪律固化（贯穿所有项）　【贯穿】

- **具体操作步骤**
  1. 每改造项提交必带五门禁 + thresholds + size_gate + release_dryrun 实测数字入 message。
  2. 守卫类必做变异验证（删断言/断路径 → 红）。
  3. 跨 crate 搬迁：同步 `field_visibility_guard` 白名单键 + `thresholds.baseline.yaml` 的 `source:`（手改路径 + `--fix` 行号）+ `constraints` §10 crate 地图 + `check_thresholds.py --fix`。
  4. `cargo test --doc` 单独跑。
- **负责角色**：QA ＋ Docs（全员遵守）
- **预期产出**：零回归漂移、文档与代码一致。
- **验收点**：所有门禁绿；文档五查（目录锚点↔标题、路径存在、数字一致）通过。

---

## 4. 实施阶段与排期建议

> 排期以「工作单元」与「进入/退出判据」给出（solo 项目不强制日历日期；每项完成判据触发下一项）。

| 阶段 | 包含项 | 建议周期（参考） | 进入判据 | 退出判据 |
|---|---|---|---|---|
| **阶段 1 · 地基（P0）** | R-01 | 1 个工作单元 | 五门禁全绿快照 | R-01 验收点全过；arch 新断言 + 变异绿；体积逐字节不变 |
| **阶段 2 · 契约分离（P1）** | R-02、R-03(B2) | 1~2 个工作单元 | 阶段 1 完成 | shared 内部二分 + re-export 显式 + 守卫绿 |
| **阶段 3 · 表现层合一（P1 根治）** | R-03(B1) | 2~3 个工作单元（43 文件分批） | 阶段 2 完成、窄接口设计评审通过 | views 迁 ui、root 瘦身、视图可独立测 |
| **阶段 4 · 子域抽离（P2）** | R-04、R-05 | 2 个工作单元（可并行） | 阶段 2/3 稳定 | render 端口化/升 crate；icons 不依赖 actions |
| **阶段 5 · 常规瘦身（P3）** | R-06、R-07、R-08、R-09 | 持续，随功能提交穿插 | 无阻塞 | 大文件降复杂度、纪律固化 |

**排序依据串讲**：先「让边界可见、依赖可声明」（R-01）→ 再「把混在一起的契约与工具分开」（R-02）→ 同时「解耦视图与运行时、为拆库松绑」（R-03）——这三步构成「可拆分架构」的地基；其后才是子域抽离（R-04/R-05）与常规瘦身（R-06/R-07）。优先级加权因子：**影响半径 > 腐化概率（是否静默失效/靠人记） > 改造成本/是否阻断其他改造**。

---

## 5. 所需资源与潜在风险

### 5.1 资源
- **人力/角色**：Arch（边界决策+守卫）、Core（core/infra/shared）、UI（ui/root views）、Root（bootstrap/lib.rs）、QA（门禁+变异）、Docs（约束/backlog/metrics 同步）。solo 项目可一人兼任，但 RACI 列清 accountability。
- **工具链**：`cargo`（fmt/clippy/test/doc）、`tools/check-arch`（`arch_check.rs` + 变异脚本 `tools/mut_*.py`）、`tools/check_thresholds.py`、`scripts/size_gate.sh`、`tools/release_dryrun.py`、`cargo tree`（依赖图核验）、`cargo metadata`（定位外部库真实路径）。
- **CI**：thresholds 校验已接入（Linux 单平台）；建议把 arch 新断言、size_gate、release_dryrun 也纳入 CI 门禁。
- **远端**：push 由维护者本机执行（agent 侧不可达）。

### 5.2 风险与缓解
| 风险 | 影响 | 缓解 |
|---|---|---|
| **静默失效（双源）** | 抽函数/迁模块时两处实现分家，不编译不红 | 抽纯函数时立即补不变量单测 + 变异（去双源批次实证）；契约测试逐字段比较，不双走同一 `From` |
| **文档漂移** | §10 crate 地图 / thresholds `source` 路径随搬迁失配 | `--fix` 只回行号，路径手改；R-09 固化同步步骤；`check_thresholds.py` 每轮必跑 |
| **体积/编译代价** | 端口化 +17KB、拆 crate 各 +2~3KB（LTO 边界） | 每次 release 实测记录；纯重构应逐字节不变，变了必解释 |
| **可见性/搬迁坑** | 结构体搬子模块时原私有字段全要 `pub(super)`；CRLF 行号差 1 | 字节级归一 LF，行号以 `wc` 为准；按既有搬迁三件套（扫描根+layer_of+依赖边）执行 |
| **远端不可达** | 本地领先 ~80 提交未 push，长期分叉 | 维护者本机定期 push；agent 不主动 push |
| **回归（门禁假绿）** | `autotests=false` 下未登记 `[[test]]` 不编译却全绿 | D-6 守卫拦截；新增 `[[test]]` 必带 lint 头 |

---

## 6. 验收标准

### 6.1 通用门禁（每项改造后必过）
- `cargo fmt --check` 干净
- `cargo run --bin check-arch` 0 违规（含本方案新增断言）
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` 0 告警
- `cargo test --workspace --all-targets --no-fail-fast`：passed 数 ≥ 当前基线 2227（功能项只增不减），0 failed
- `cargo test --workspace --doc` +1 计入基线
- `python tools/check_thresholds.py` 121 条 OK（路径/行号正确）
- `bash scripts/size_gate.sh` exit 0（纯重构应 **10,550,272 B** 逐字节不变；拆 crate 允许 +2~3KB 并需记录）
- `python tools/release_dryrun.py` 15/15

### 6.2 逐项验收（映射到 R-01~R-09）
| 项 | 关键验收信号 |
|---|---|
| R-01 | `crate::domain` 在 `src/` 0 处；`cargo tree -p rustmark` 仅含声明依赖；arch 新断言 + 变异绿 |
| R-02 | shared 内部 kernel/support 二分；`internal_cycle_guard_test` 绿；调用点零改动 |
| R-03 | B2：re-export 显式 + 守卫绿；B1：`src/presentation/views` 清空/仅装配、`crate::app` 在视图 0 处、root 行数下降、视图可独立测 |
| R-04 | `rich/` 端口化；若升 crate 则 `cargo tree -p rustmark-render` 无 egui |
| R-05 | `cargo tree -p rustmark-icons` 无 `rustmark-actions`；icons 单测绿 |
| R-06 | `search`/`stats`/`find` 目标文件 ≤ ~400 行；纯函数单测绿；体积逐字节不变 |
| R-07 | `incremental` 等目标文件 ≤ ~400 行；单测绿 |
| R-08 | actions/icons 无新增跨职责依赖 |
| R-09 | 文档五查通过；无体积/阈值漂移 |

### 6.3 总体达成判据（对应目标 O1~O7）
- O1：依赖图在 `Cargo.toml` 完全可见，root 无 `crate::domain`。
- O2：`rustmark-shared` 内部契约与工具物理可分。
- O3：表现层合一，root 行数显著下降。
- O4：`rustmark-render` 可独立复用、不绑 egui。
- O5：icons 不依赖 actions，root 依赖显式。
- O6/O7：大文件降复杂度、门禁/文档纪律固化、零回归漂移。

---

## 7. 执行纪律（铁律，全员遵守）
1. **增量提交**：每改造一个 commit，message 写文件 + `git commit -F`，门禁数字进 message。
2. **守卫必变异**：新增/修改 `check-arch` 断言必须做变异验证（删断言/断路径 → 红）。
3. **不主动 push**：远端由维护者本机执行。
4. **文档同步**：约束 §10、backlog §3、metrics、PROGRESS 随代码回填实测数字。
5. **体积/基线不得"沿用"**：必须实测；纯重构逐字节不变是正常证据。

> 本文件与 `docs/architecture-constraints.md`、`docs/v1.1-backlog.md`、`docs/PROGRESS.md` 联动维护。改造项状态变更请同步回填。
