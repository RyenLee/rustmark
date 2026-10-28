# Rust Markdown 编辑器 · 开发实施计划（索引）

> 项目代号：`rustmark`  
> 计划目录：`D:/data/WorkBuddyCache/workspace/rustmark/`  
> 计划版本：v1.0（2026-10-06）  
> 需求基线：《Rust Markdown 编辑器 · 需求文档 v2.0 整合版》  
> 执行方法：《Rust Markdown 编辑器 · Agent 开发流程》（任务单元制）

---

## 1. 本索引的用途

本目录是 `rustmark`（Rust 原生 Markdown 编辑器）的**实施计划与工程规范载体**，不是原型工程。

| 目录                              | 定位                                          | 是否写入代码              |
| ------------------------------- | ------------------------------------------- | ------------------- |
| `../markdown-editor-prototype/` | 已交付的 Web 高保真原型（Vue 3 + Vite + Pinia + mock） | ❌ 只读，仅作行为契约来源       |
| `./`（`rustmark/`）               | 目标工程的计划、规范、ADR；后续 Rust crate 直接在**本目录根**初始化 | ✅ 后续在此 `cargo init` |

`docs/` 下按「阶段序号-阶段名称」命名，每篇包含：阶段目标、输入/输出、任务清单（编号 / 描述 / 前置依赖 / 涉及文件 / 验收标准）、技术方案要点、风险与注意事项、完成标准（DoD）。

---

## 1.1 当前进度（2026-10-14）

**九个阶段全部完成（v1.0 发布就绪）**。判定依据：`.agent/tasks/01..09-*.md` 状态段全部勾选并附验收命令输出；阶段 09 出口检查 5/6 闭合——唯一未闭合项「预发布环境演练」已拆为两半：**不依赖远端的一半（版本守卫 / 校验和 / Notes 归组 / 无私钥 / 回滚步骤）于 2026-10-09 离线演练完毕，13/13 通过并接入 CI**；依赖远端与三平台真机的一半步骤已文档化于 `docs/guides/release.md` §4.2，待 fork 仓库首跑。

**主线九阶段状态**

| 阶段            | 状态    | 关键依据                                                                                                                                                                                                                                                                                                 |
| ------------- | ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 01 需求基线与技术决策  | ✅ 已完成 | `REQUIREMENTS.md`、ADR-001~006、`.agent/` 全套；原「剩余 3 项」已随阶段 02+ 开工与 S8-01（cargo-bloat 实测）消化                                                                                                                                                                                                             |
| 02 工程骨架与分层边界  | ✅ 已完成 | `tasks/02` 状态段勾选；`check-arch` 双通道（build.rs + bin）持续把关                                                                                                                                                                                                                                                |
| 03 领域模型实现     | ✅ 已完成 | `tasks/03` 全部勾选；5 上下文聚合 + 领域层零 unwrap 编译期强制                                                                                                                                                                                                                                                          |
| 04 应用服务与用例编排  | ✅ 已完成 | 5 个 AppService + 事件订阅链；应用层 57 用例绿                                                                                                                                                                                                                                                                    |
| 05 基础设施实现     | ✅ 已完成 | 出口 6/6 通过（`tasks/05` 收尾记录）                                                                                                                                                                                                                                                                           |
| 06 表现层与交互实现   | ✅ 已完成 | `tasks/06`：全部任务 + S6-12 收尾三处修复均验证                                                                                                                                                                                                                                                                    |
| 07 集成联调与原型对齐  | ✅ 已完成 | S7-01~06 全勾；DEF-01/02 已修复；三平台手工回归**清单**已产出（10-09 更新）                                                                                                                                                                                                                                                 |
| 08 性能优化与质量加固  | ✅ 已完成 | S8-01~~08 全勾；出口 7 项对照全过；DEF-03/04 已修复；OPT-001~~014                                                                                                                                                                                                                                                   |
| 09 测试体系、打包与发布 | ✅ 已完成 | `tasks/09` 全勾；覆盖率整体 85.27% / 领域 91.69% 门禁生效（红/绿演练均过）；Windows 打包实测 3.9MB；S9-03 固定种子方案 / S9-05 未签名路径；S9-06 补离线演练工具 `tools/release_dryrun.py`（13/13 通过、红路径 exit 1、已入 CI）与 `tools/gen_release_notes.py`（Notes 逻辑可离线验证）；**遗留**：预发布演练的远端 / 真机部分、macOS/Linux 真机回归（见 `tasks/09` 状态段与 `docs/guides/release.md` §4.2） |

**主线之外的增补工作（均已完成并有实施记录）**

- **P1/P2 未交付功能补救**（`docs/archive/stage-reports/p1p2-remediation-plan.md` 总览表 5/5 ✅，逐步记录见 `docs/archive/stage-reports/p1p2-implementation-log.md`）：拖拽打开、自定义主题（8 核心语义色）、数学公式/Mermaid（结构化卡片 + 导出资源注入）、拼写检查（3779 词 + 近邻门槛 + 波浪线）、图片导出（视口截图 + **整篇长图**，10-09 补齐）。
- **中文文本与图标显示修复**（`docs/archive/stage-reports/font-encoding-fix-plan.md` §六，2026-10-08 完成）：CJK 字体注入（本机命中 msyh.ttc）、9 个无字形符号替换、GBK/UTF-16 解码（内嵌映射表 47 KB，零新依赖）、导出 HTML 中文字体族。
- **P2 遗留缺陷五项修复**（2026-10-09）：拖拽目录误开、GB18030 四字节解码、保存写回原编码、长图导出、导出 HTML 注入 KaTeX/Mermaid 资源。
- **v1.1 P1 四项收官**（2026-10-09，按「先排除阶段 9 任务」口径的执行顺序 V11-12 → V11-03 → V11-04 → V11-05）：
  - **V11-12 规范 15 机械门禁**：`lib.rs` 开启 `clippy::unwrap_used`/`expect_used`/`panic` 门禁 + 测试豁免，14 处非测试 `unwrap`/`expect` 全部重写为非 panic 形态（锁损坏 `PoisonError::into_inner`、选项 `?`/`and_then`/`let…else`），零 `#[allow]` 残留，红路径演练通过。
  - **V11-03 状态栏编码接线（DEF-05）**：共享内核 `TextEncoding` + 三层接线 + 每帧同步，未落盘显示 `—` 不谎称 UTF-8，GBK/UTF-8 用例通过。
  - **V11-04 PDF 开放（方案 B）**：开放入口并诚实提示「经系统打印对话框」，打印副作用抽为可注入 `PrintDispatcher`，CI 不弹框。
  - **V11-05 写作目标进度条**：领域 `WritingGoal` + 应用 `goal_progress` + 状态栏百分比/细进度条/达成去重提示，旧 TOML 配置缺字段按「未设置」兼容。

**验证链记录**（2026-10-20 实测，v1.0 收尾口径；**v1.1 现行基线见 §1.2 = 2187**）：`fmt --check` 零差异 · `clippy --all-targets --all-features -- -D warnings` 0 告警 · `check-arch` OK · 全量 `cargo test --all-targets` **1879 passed / 0 failed / 4 ignored**（有界性能断言只在 release 生效，debug 仅打印指标，见 `docs/guides/testing.md`）。2026-10-09~10 的各轮历史实测快照（含 7 条红的发现与处置）已归档于 `docs/plans/v1.1-backlog.md` §2（V11-14/15）与 `docs/guides/testing.md` 状态块。

**离线预发布演练记录**（2026-10-09，`tools/release_dryrun.py`）：绿路径 **13/13 通过**（tag v0.1.0 ↔ Cargo.toml 0.1.0 一致、产物 sha256 重算一致、Notes 按阶段聚合、无私钥入仓、回滚步骤齐备）；红路径 `--tag v9.9.9` → 11/12 且 **exit 1**，证明门禁会红。

### 1.1.1 历史审计轮次（2026-10-09 ~ 10-10，已归档摘要）

> 2026-10-09 ~ 10-10 曾进行四轮滚动审计（§1.1.1「第二轮 · 先排除阶段 9」、§1.1.2「第三轮 · 全量口径」、
> §1.1.3「第四轮 · 用户反馈修复批次」、§1.1.4「第五轮 · V11-06~09 一次性完成」）。其结论均已沉淀至现行文档，
> 原文随本次文档整理（2026-10-14）移除，不再重复维护：
>
> - 轮次发现的 V11-12 / V11-13 及 7 条测试红（V11-14 / V11-15）的完整分析与处置记录 → `docs/plans/v1.1-backlog.md` §2；
> - 「7 条红」的三类归因与关闭状态 → `docs/guides/testing.md` 当前状态块；
> - 各轮次的「完成度主体」快照 → 本文 §1.1 当前进度与阶段总览表（§2）。
>
> **教训保留**：那些轮次反复验证了一条纪律——「文档声称的全绿」必须附**当日实测命令与数字**，
> 跨快照引用「此前全绿」而不复跑，就会积累出 §1.1.3 自己发现的「验证链实际为红」这类文档失实。

---

## 1.2 下一阶段（v1.1）

**阶段 01–09 已全部完成，无进行中的阶段。** 下一阶段为 **v1.1**，
任务规划、逐项说明与完成记录以 [`docs/plans/v1.1-backlog.md`](./v1.1-backlog.md) 为**唯一入口**（本文不重复其总览表）。

截至 2026-10-28 的状态摘要：

- **功能项已全部完成**：V11-03~09、V11-12~26 均 ✅；测试基线 **2299 passed / 0 failed / 4 ignored**
  （`--all-targets`，2026-10-28 实测；再加 `--doc` 的 1 passed / 1 ignored 合计 **2300 passed / 5 ignored**；
  分层构成见 `docs/spec/metrics.md` §1.1，命令构成见 backlog §3）。
  较 2187 的 **+40**：+8 来自架构评审 D-5（帧耗时观测的单测）、+8 来自合规差额报告
  PRIV-01/02（远程图片开关 6 条 + 导出 CDN 披露 2 条）、+1 来自 `default-members` 拓扑守卫、
  +8 来自 V11-61（find 引擎上浮的内联单测）、+5 来自 D-6（测试目标登记守卫）、
  +10 来自「去双源」批次（千分位 2 / 行号栏 3 / 搜索游标 4 / 配色派生 1）。
- **剩余仅环境前置项**：V11-02（macOS / Linux 真机回归，P0）、V11-01（远端发布演练）、
  V11-10 / V11-11（证书预算 / 痛点驱动）——见 backlog §3。
- **架构改造已收官**：V11-54 / V11-54a（全库评估 + 21 条改造项）2026-10-21 完成；
  V11-56~67 波次中 V11-56/57/58/59/62/63/64/65/66/67 已落地，V11-60 关闭、V11-61 降级为记录。
  **workspace 现为 9 crate**：`rustmark`（组合根 + views）/ `rustmark-core`（六边形脊柱，零 egui）/
  `rustmark-infra`（V11-64，`7b9b6ba`）/ `rustmark-shared`（TD-04，`2da434e`）/ `rustmark-ui`（V11-62）/
  `rustmark-render`（R-04 升 crate，`22e8a86`：`presentation::rich` 搬出，默认 egui-free、egui 后端经 feature 门控）/
  `rustmark-ports`（R-10 项 A，`0f20bc8`：5 个架构端口契约搬出 `shared/kernel`，实现仍在 infra）/
  `rustmark-icons` / `rustmark-actions`，本地依赖边 **12 条**、无环（`docs/spec/workspace-topology.expected.txt`）。
  两个新 crate 的直接收益是**依赖面收窄**：core 从 12 个外部依赖降为 4 个（serde/thiserror/log/ahash），
  复用 domain/application 的消费者不再被 syntect/ureq/rfd/image/pulldown 拖进编译图。
- **R-03 B1 视图窄接口解耦（批 1，2026-10-27 起）**：视图签名由 `&mut EditorApp`
  改为 `&mut dyn EditorViewContext`（契约在 `rustmark_ui::presentation::view_context`），
  契约拆 5 个能力 trait（`ThemeAccess` / `UiStateAccess` / `DocumentAccess` /
  `CommandPort` / `ServiceQueries`）+ 1 个 bundle，避免「trait 照抄 `EditorApp`」的假解耦。
  **视图按 ADR-013 决策 3 留 root**，trait 住 ui 只为「契约独立于组合根」。
  进度：**准备项 P-1/P-2/P-3 已完成**（`PendingDialog` / `WorkspaceEntries` /
  `WorkspaceSearch` 搬 ui 且公开路径逐字不变；`editor_id()` 落为自由函数；
  两处错误前提文档已纠偏），**批 1 迁移 7/7 收官**：`welcome_view` 树 3 +
  `settings_view` 树 5 + `diff_view` / `find_replace_view` / `export_view` /
  `quick_open_view` 4 + `command_palette_view` 1 + `overlays` 1 +
  `toolbar_view` 树 4 + `sidebar_view` 树 5 + `preview_view/inline/math.rs` 1
  = **24 个零 services 视图全部解耦**。守卫 `PENDING_DECOUPLED_VIEWS` 只减不增，
  余 8 条（`services` 视图 + 6 个未登记子模块，留批 2 / 批 3）。契约最终 **96 个
  方法**（ThemeAccess 3 / UiStateAccess 15 / DocumentAccess 13 / CommandPort 61 /
  ServiceQueries 4）——`ServiceQueries` 交出**服务本身的只读借用**（视图拿得到
  能力、拿不到所有权），`assets` 刻意不给（`AssetLoader` 在 infra，ui 不可名）。
- **R-03 B1 批 2（`services` 视图解耦）· 步 2.1 `status_bar_view`（已收官）**：契约
  **96 → 108 个方法**（`UiStateAccess` +3 / `DocumentAccess` +5 / `CommandPort` +4），
  白名单 8 → **7 条**。`build_model` 保持**只读**签名（收 `&dyn EditorViewContext`），
  集成测试 **19 处**调用点**零改动**；为此把 `*_state()` 只读变体从拍板的 2 个
  补到 **3 个**（多出 `editor_state_read`——`build_model` 要读三个编辑器开关，
  而通往 `EditorUiState` 的唯一出口是 `&mut` 的）。`LineIndex` 不搬，行索引靠
  应用层缓存保证仍只扫一次全文（问题 053 的性能不变量未破）。
- **R-03 B1 批 2 · 步 2.2 `editor_view` 树（已收官，`e9024b6`）**：契约 **108 → 118**
  个方法（`UiStateAccess` +1 `editor_text_mut` / `CommandPort` +9）。`editor_ops::TextEdit`
  搬进契约同侧（root 侧 `pub use` 转发，公开路径逐字不变，实测 **±0 B**）。
  `flush_edit` 的 `domain_len` 口径钉死为**旧文本长度**，专项回归
  （`enter_key_creates_lines_and_keeps_list_numbers_intact` /
  `list_enter_right_after_commit_does_not_pile_markers`）通过。
- **R-03 B1 批 2 · 步 2.3–2.4（已收官，`d988edf` + 本步）**：`preview_view` 树（含
  `image_cache.rs`）/ `main_view` / `scroll_sync` 全部解耦，契约 **118 → 133** 个方法，
  `PENDING_DECOUPLED_VIEWS` **归零**、`views/**` 零 `crate::app`。
  **`image_cache.rs` 的「三重阻塞」被一条闭包契约解开**——文件不搬家（`image` crate
  与 `decode()` 原样可用），`AssetLoader`（infra）与文档目录（`pub(crate)`）封进
  `preview_image_fetcher(&self) -> ImageFetcher` 返回的
  `Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>`，远程图的后台下载（F-3）
  因此不受影响。原计划的 `EXEMPT_VIEWS` 永久豁免**没有实际成员**。
- **R-03 B1 批 3（守卫收官，2026-10-27）**：**R-03 B1 全部三批完成**。
  ① 白名单 `PENDING_DECOUPLED_VIEWS` **整条删除**——它已无成员，保留一张空表会把
  「一名都不许有」读成「暂时没人管」；生产入口 `check_view_decoupling(manifest)`
  直接传 `&[]`，带参版本留给夹具（双向校验逻辑本身要能被测）。
  ② 新增 `check_view_no_composition_root_marker`：`views/**` 递归覆盖全部 `.rs`，
  出现 `EditorApp` **标识符**即报。补的是路径判据的结构性盲区——子模块经
  `use super::*` 拿到父模块私有 `use` 别名后，本文件零 `crate::app` 却能写
  `app: &mut EditorApp`（`preview_view/{blocks,code,list,media,table,toc}.rs`
  六个文件从未进过白名单、也从未被报过）。
  ③ **P0 修复（验收方一手复现）**：路径判据原用「`crate` `::` `app` 相邻三连」
  字面匹配，`use crate::{app::frame::EditorApp};` 的第三连是 `{`，**必然不命中**
  ——视图可真实持有组合根而 `build.rs` 与 `check-arch` **两层全绿**。改走
  `parse_use_tree` 展开（同一个文件里早有这个展平器，只有这条守卫没用它），
  旧函数删除，从根上不给「两套扫描器分叉」留第二次机会。
  ④ `allowed_for` 的 `crate::app::frame` 窄豁免**从 `views/` 撤回**（其余表现层
  子层照旧）——撤回条件「全部视图迁完」已兑现，留下的豁免只剩坏处。
  ⑤ 变异验证 4 项全捕获：关记号守卫 → fixture 114/2 failed；关 `use` 展开分支 →
  115/1 failed（红在花括号那条）；关无参入口 → 115/1 failed；真实工程注入
  `fn __mutation_marker(_: &EditorApp) {}` → 报 `stats_view.rs`，回退后注释里的
  **20 处 `EditorApp`（14 文件）** 仍全部不触发。
  **取证姿势**：变异态下直接 `cargo build --bin check-arch` 会让 `build.rs` 先 panic、
  bin 不重建 → 跑旧 exe 假绿；必须 `SKIP_ARCH_CHECK=1` 构建后再独立运行。
- **R-05 解耦 icons 与 actions（2026-10-27）**：**问题 C 收口**——`rustmark-icons` 不再依赖 `rustmark-actions`。
  ① icons 侧定义自有小契约 `IconKey` 枚举（`Bold / Italic / Strikethrough / InlineCode / InsertLink`），`Icon::Action(UiAction)` 改为 `Icon::Action(IconKey)`，`build_specs` 与 icons 单测同步；删除 `use rustmark_actions::UiAction` 与 `rustmark-icons` `[dependencies]` 里的 `rustmark-actions`。`cargo tree -p rustmark-icons` 已确认无 `rustmark-actions`。
  ② `UiAction → IconKey` 映射上移到 **root** 的 `src/presentation.rs::icon_key_for`（非方案字面写的「ui crate」）：唯一生产调用点 `toolbar_view/controls.rs:261` 本就在 root，且 `rustmark-ui` 的拓扑守卫 `ui_crate_dependency_face_is_egui_plus_core` 严禁其获第三依赖，加 `rustmark-icons` 边会破该守卫；root 已同时依赖 icons 与（经 re-export 的）actions，上移映射不引入任何新跨 crate 边，最小侵入。
  ③ 方案步骤 3「root 显式依赖 actions」**未触发**：root 经 `crate::presentation::shortcuts::registry::UiAction` 间接引用，非直连，符合「若无则不动 root 依赖」。
  ④ 门禁全绿：fmt / check-arch / clippy -D warnings / `test --all-targets` 2250 passed·0 failed·4 ignored / `test --doc` / thresholds 121 / size_gate **10,558,464 B（±0 B，与 R-03 收官基线一致）**。拓扑期望 `workspace-topology.expected.txt` 同步（本地依赖边 9 → 8，icons 依赖面收敛为纯 egui）。
- **R-11「去双源」收口（2026-10-28，`3e89332` + `a90f29e`）**：DS-2 任务项识别、DS-1 搜索命中高亮两处双源各收口到一处真源。
  - **DS-2 任务项识别（真双源，且口径已分歧）**：core 的 `stats::classify::task_marker_of`
    与 root `preview_view/list::task_marker_offset` 各自写一遍判定（前者要行首子弹、认大写
    `[X]`、**不要求闭合 `]`**；后者不要子弹、不认 `[X]`、要求 `]`），坐实两个活缺陷：
    **D-1** `- [X]` 点击**静默无反应**（统计认它已完成、回写不认 → `toggle_task` 直接 return）；
    **D-2** 真标记是 `[X]` 而正文里有 `[x]` 时，会把**正文里的 `[x]` 换成 `[ ]]`**。
    收口方式：抽出唯一真源 `task_marker_state_at`（锚定开头、认 `[ ]`/`[x]`/`[X]`、要求闭合 `]`），
    两个入口各只做定位（`task_marker_of` 剥子弹、`find_task_marker` 找首个并返回字节偏移），
    root 侧改为委托。三个函数提为 `pub` 并从 `document::service` 导出（root 已依赖 core，
    **不新增跨 crate 依赖边**）。
    - **行为变更（须显式接受）**：统一后**要求闭合 `]`**，`- [ x` / `-[x` 这类畸形行在统计侧
      从「算作任务项」变为「不算」。这更正确（GFM 要求 `]`），但属行为变更，已由
      `task_marker_state_requires_the_closing_bracket` 钉死；等价性基准
      `stats/equivalence::ref_task_marker_of` **同步跟随真源**（留一份更旧口径的副本，
      等价性用例就只是在「输入没碰上差异」时碰巧绿），并往语料里补了畸形行。
    - 变异 4 项全捕获：真源去掉闭合要求 → core 3 红（含 equivalence 对拍）；真源丢掉大写 `X`
      → core 5 红 + root 2 红；`find_task_marker` 改取最后一个 → core 2 红 + root 2 红；
      root 退回本地判定（旧双源形态）→ root 2 红。
  - **DS-1 搜索命中高亮（真双源）**：core 有 `SearchHitView::match_range`（`pub(crate)`、
    **实测零消费**），UI 却在 `locate_in_snippet` 里拿片段**再搜一遍** query，返回字符区间。
    成因化石：`scan.rs` 两处文档写着「高亮区间见 `[SnippetWindow::highlight]`」，而
    `SnippetWindow` 实际只有 `start`/`end`——文档里留痕说 core 会带出高亮、代码没落地，
    UI 才只能自己重搜。收口方式：给 `SnippetWindow` 补上 `highlight` 方法
    （绝对字节命中 + 窗口起点 → 片段内**字符**区间），DTO 换成 `pub highlight`，UI 删除
    `locate_in_snippet` 直接取 DTO；两处文档引用随之变真。**顺带删** `RawHit::{start,end}`
    与 `snippet_window`（同样算完即弃 / 只做切片）。UI 侧四处分歧一并消除：① 上下文抢高亮
    ② 忽略 `whole_word` ③ 大小写折叠走 `to_lowercase` 而非 `char_eq_ignore_case` ④ 命中被窗口
    截断时返回 `(0,0)`。
    - 变异 3 项全捕获：`highlight` 恒返回 `(0,0)` → core 6 红 + app 2 红；改从片段开头起
      （旧 UI 行为退化）→ core 6 红 + app 2 红；**`search.rs` 漏搬运 highlight → core 全绿、
      仅 app 2 红**，证明新增的服务级用例（`search_highlight_points_at_the_real_match` /
      `search_highlight_honors_whole_word`）是管线的唯一守卫。
  - 门禁：fmt / check-arch / clippy `-D warnings` 全绿；`test --all-targets`
    **2295 passed / 0 failed / 4 ignored**（基线 2284，+11：core 8 / root 3；
    删 1 条 `locate_in_snippet` 用例）；`test --doc`；thresholds 121 OK
    （`stats.rs` 112→118、`scan.rs` 262→296 两处行号漂移已同步）；
    size_gate **10,560,512 B**（DS-2 ±0 B，DS-1 −512 B，端点见 metrics §2.8）。
- **可观测性（D-5，2026-10-26，`1c58052`）**：`FrameScheduler` 兼作帧耗时观测点——超 16ms 的帧打一条
  `warn`（状态更新 / 绘制 / 未归因三段归因 + 本帧到期任务，120 帧限流；**仅 release 构建输出**），
  `RUST_LOG=…=debug`
  时每 600 帧给平均 / 最大 / 超预算占比。`docs/archive/architecture-reviews/architecture-review-2026-10-23.md` 的 12 项问题
  至此**全部收口**（9 项闭合、2 项记录为不修、1 项持续纪律）。
- **合规差额报告 P1 收尾（2026-10-26，`ee8acfb`）**：`PRIV-01` 远程图片开关（设置页 +
  关闭后零外联 + 状态栏常驻提示 + 跨重启持久化）与 `PRIV-02` 导出 CDN 披露（对话框常驻
  说明 + 导出件 `<head>` 注释）已闭环，`PRIVACY.md` 里「不勾选则不注入」的错误说法一并改正。
  剩余 P1 只有 `SEC-03`（Release Notes 三段式，发布时动作）。
- **`--workspace` 纪律机制化（2026-10-26，`9c85129`）**：根 manifest 的 `default-members`
  覆盖全部 9 个包（root `.` + 8 个 crate），漏带 flag 不再漏测子 crate；两份清单的同步由
  `workspace_topology_test::default_members_cover_every_workspace_member` 集合相等断言钉死。
- **体积**：`NFR-PERF-01` release exe **10,567,168 B = 10.078 MiB**（**2026-10-28 R-10 项 B 后实测**，
  当日 `bash scripts/size_gate.sh` 输出；2026-10-27 R-03 B1 批 2 收官时点为 10,558,464 B = 10.069 MiB），
  低于 10.5 MiB 预警线与 11 MiB 红线（`size_gate` exit 0，距预警线（11,010,048 B）余 442,880 B、
  距红线（11,534,336 B）余 967,168 B）。
  2026-10-20 的
  「11.36 MiB 超 10 MB 红线」已解除：V11-26 依赖裁剪先把 11.36 → 10.93 MiB（仍高于旧红线），
  随后维护者把红线 10 → 11 MiB（2026-10-24，`scripts/size_gate.sh` 与 metrics §2.8 同步），
  当前值再降至 10.07 MiB。**R-03 B1 的 +9,728 B 拆两段**：批 1 **+7,680 B**（其中 **+7,168 B 全部落在
  准备项提交 `9f8b735`**——trait 32 → 92 方法的一次性扩张 + 3 个 root 私有类型搬进 `rustmark-ui`；
  其后步 1.1–1.7 七个纯签名迁移步合计**净 +512 B**，单步在 −2,048…+1,024 之间跳，全部是
  **PE 节对齐粒度**的抖动）、批 2 **+2,048 B**（步 2.1 +512 / 步 2.2 ±0 / 步 2.3 +1,024 / 步 2.4 +512）。
  归因口径见 metrics §2.8：主因是**一次性元数据 + 薄转发桩**（批 2 的 40 个新函数平均只吃 57.6 B
  代码），不是「trait 越大派发越贵」（给 96 方法的 trait 再加 4 方法 + 4 impl 实测 **±0 B**）。

---

### 历史进度（2026-10-06 快照，摘要保留）

> 逐阶段的完整落地记录（测试数、验证链输出、暴露并修复的缺陷）见 `docs/01`~`docs/09` 各阶段文档
> 与 `.agent/tasks/01..09-*.md` 的状态段，此处只留索引级摘要。

- **阶段 01 产出已落地**：`REQUIREMENTS.md`、ADR-001~005、`.agent/` 全套、`docs/archive/stage-reports/prototype-contract.md`（A 契约 29 条 + B 免疫清单 17 项）、`docs/spec/metrics.md`、`docs/guides/dependencies.md`、`deny.toml`、`rust-toolchain.toml`（固定 1.99.0）。
- **阶段 02~05**：工程骨架与分层边界（`check-arch` 双通道）、5 个限界上下文（领域层零 unwrap 编译期强制）、5 个 AppService + 事件订阅链、基础设施适配器（ropey / pulldown / syntect / fs 仓储 / 导出器）——各阶段出口检查全过。
- **阶段 06~07**：egui 视图与快捷键单一数据源；三平台手工回归清单产出；P1/P2 五项补救（拖拽打开 / 自定义主题 / 公式与 Mermaid 结构化卡片 / 拼写检查 / 图片导出，均零依赖）——实施记录已归档至 `docs/archive/stage-reports/`。
- **阶段 08~09**：体积 / 启动 / 内存达标（Windows 10.93 MiB 超限见 metrics §2.4；冷启动 704 ms；空闲内存 125.3 MB）；覆盖率门禁生效（整体 85.27% / 领域 91.69%）；离线发布演练 13/13 入 CI。

---

## 2. 阶段总览与进度

| 序号 | 阶段         | 文档                                             | 对应需求文档章节                   | 对应里程碑 | 预估工期  | 状态                                           |
| -- | ---------- | ---------------------------------------------- | -------------------------- | ----- | ----- | -------------------------------------------- |
| 01 | 需求基线与技术决策  | [docs/stages/01-需求基线与技术决策.md](../stages/01-需求基线与技术决策.md) | §1–§2、§5、§10               | 前置    | 0.5 周 | ✅ 已完成                                        |
| 02 | 工程骨架与分层边界  | [docs/stages/02-工程骨架与分层边界.md](../stages/02-工程骨架与分层边界.md) | §5.3、§6.2、§7.1             | M1 前段 | 1 周   | ✅ 已完成                                        |
| 03 | 领域模型实现     | [docs/stages/03-领域模型实现.md](../stages/03-领域模型实现.md)       | §6.1、§6.4、§7.1（domain/）    | M1    | 2 周   | ✅ 已完成                                        |
| 04 | 应用服务与用例编排  | [docs/stages/04-应用服务与用例编排.md](../stages/04-应用服务与用例编排.md) | §6.2、§7.1（application/）    | M1–M2 | 1.5 周 | ✅ 已完成                                        |
| 05 | 基础设施实现     | [docs/stages/05-基础设施实现.md](../stages/05-基础设施实现.md)       | §5.1、§7.1（infrastructure/） | M2    | 2 周   | ✅ 已完成（出口 6/6 通过）                             |
| 06 | 表现层与交互实现   | [docs/stages/06-表现层与交互实现.md](../stages/06-表现层与交互实现.md)   | §3、§7.1（presentation/）     | M2–M3 | 2.5 周 | ✅ 已完成（含 S6-12 收尾修复）                          |
| 07 | 集成联调与原型对齐  | [docs/stages/07-集成联调与原型对齐.md](../stages/07-集成联调与原型对齐.md) | §3.2、§4.1                  | M3    | 1 周   | ✅ 已完成（DEF-01/02 已修；macOS/Linux 真机回归待阶段 09）   |
| 08 | 性能优化与质量加固  | [docs/stages/08-性能优化与质量加固.md](../stages/08-性能优化与质量加固.md) | §4.1、§8                    | M4    | 1.5 周 | ✅ 已完成（DEF-03/04 已修；出口 7 项对照全过）               |
| 09 | 测试体系、打包与发布 | [docs/stages/09-测试体系打包与发布.md](../stages/09-测试体系打包与发布.md) | §4.3、§4.4、§11              | M4    | 1.5 周 | ✅ 已完成（覆盖率门禁生效；预发布演练离线部分 13/13，远端 / 真机部分待 CI） |


**总计约 13.5 周**，与需求文档 §9.1 的「12 周」存在差异：差异来自阶段 01（需求基线/ADR 冻结）与阶段 07（原型行为对齐）两项在需求文档中未单列、但本工程实际存在的必要工作。若需压缩回 12 周，可选项见 §6。

---

## 3. 阶段依赖顺序

```
01 需求基线与技术决策
   │  产出：REQUIREMENTS.md / ADR-001~003 / .agent 规范 / 指标基线口径
   ▼
02 工程骨架与分层边界 ──────┐
   │  产出：Cargo.toml / 模块树 / build.rs 边界校验 / shared / CI
   ▼                        │（边界校验贯穿 03–09，每次提交必跑）
03 领域模型实现              │
   │  产出：5 个上下文聚合根 / 值对象 / 事件 / trait
   ▼                        │
04 应用服务与用例编排        │
   │  产出：Command / Query / DTO / AppService / 事件订阅
   ▼                        │
05 基础设施实现 ─────────────┘
   │  产出：ropey / pulldown / syntect / fs 仓储 / 事件总线 / 导出器
   ▼
06 表现层与交互实现
   │  产出：egui 视图 / UI State / 快捷键单一数据源 / 主题
   ▼
07 集成联调与原型对齐
   │  产出：端到端用例 / 行为对齐矩阵 / 缺陷回归清单 / 三平台手工回归
   ▼
08 性能优化与质量加固
   │  产出：体积 / 启动 / 内存达标数据 + 增量高亮 + 大文件降级
   ▼
09 测试体系、打包与发布
      产出：测试金字塔 + 覆盖率门禁 + 三平台产物 + 发布流程
```

**并行可行性**：05 的 S5-01/S5-02/S5-03（ropey / pulldown / syntect 适配器）只依赖 03 的 trait 定义，可与 04 并行开工；06 的 S6-09（快捷键注册表）依赖 03 的 `Preferences` 聚合，可在 04 完成前提早启动。

---

## 4. 三条贯穿全程的硬约束

1. **分层依赖单向**：`presentation → application → domain ← infrastructure`。每层允许/禁止的依赖见需求文档 §6.4，由 `build.rs` / `check-arch` 在每次提交前强制校验（阶段 02 落地）。
2. **模块系统**：Rust 2024 Edition，禁止 `mod.rs`，路径统一 `crate::` 开头（需求文档 §5.3）。
3. **任务单元制**：一次只做一个任务单元，每单元必须可编译、可测试、可提交；单元完成后重置 Agent 上下文（Agent 流程文档 §3、§5.3）。

**每单元收尾验证命令**（阶段 02 起生效）：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo check
cargo test                     # 本单元测试 + 全量回归
cargo run --bin check-arch     # 架构边界校验
```

---

## 5. 关键决策摘要（详见阶段 01）

| 编号      | 决策     | 结论                                                                                            | 依据                                                                   |
| ------- | ------ | --------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| ADR-001 | UI 框架  | **egui + eframe**（wgpu 后端），不采用 Tauri（**当前锁定 0.37.0**，见 `Cargo.toml`；本表记的是当初决策） | 需求文档 §5.1 / §5.2；与原型 `DESIGN-OVERVIEW.md` §1「目标态 Tauri v2」冲突，以需求文档为准 |
| ADR-002 | 解析与高亮  | pulldown-cmark 0.13（`default-features = false`）+ syntect 5.3（`default-fancy`，语言集裁剪 + dump 加载） | 需求文档 §5.1、§8.1                                                       |
| ADR-003 | 文本缓冲   | ropey 1.6.1 实现领域层 `TextBuffer` trait，领域层不直接依赖 ropey 类型                                        | 需求文档 §6.4 允许 domain → ropey，但建议以 trait 隔离以便测试                        |
| ADR-004 | 导出 PDF | P1，采用「系统打印/导出对话框」外置方案，不内嵌 PDF 引擎                                                              | 体积预算 5–7 MB，内嵌引擎会击穿 ≤10 MB 目标                                        |
| ADR-005 | 原型关系   | 原型为**行为契约来源**，非代码来源；其 8 项失效缺陷在 Rust 侧从设计上根治                                                   | `markdown-editor-prototype/docs/REMAINING-WORK.md`                   |

---

## 6. 工期压缩选项（如需回到 12 周）

| 选项                             | 可省     | 代价         |
| ------------------------------ | ------ | ---------- |
| 阶段 03 与 04 部分并行（领域 + 应用接口同批设计） | ~0.5 周 | 接口返工风险上升   |
| 阶段 07 的端到端用例改为冒烟级（每模块 3 条主路径）  | ~0.5 周 | 长尾交互缺陷暴露延后 |
| Export 的 PDF（P1）延后到 v1.1       | ~0.5 周 | 首版缺 PDF 导出 |

不建议压缩阶段 01（ADR 未冻结会导致阶段 06 返工）与阶段 08（性能指标是本项目核心差异点）。

---

## 7. 进度更新约定

每完成一个任务单元，在本索引阶段表的「状态」列更新为 `🟨 进行中` / `✅ 已完成`，并在对应阶段文档的「任务清单」勾选框打勾、记录完成日期与验收命令输出摘要。阶段完成标准（DoD）全部满足后方可进入下一阶段。
