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

## 1.1 当前进度（2026-10-09）

**九个阶段全部完成（v1.0 发布就绪）**。判定依据：`.agent/tasks/01..09-*.md` 状态段全部勾选并附验收命令输出；阶段 09 出口检查 5/6 闭合——唯一未闭合项「预发布环境演练」已拆为两半：**不依赖远端的一半（版本守卫 / 校验和 / Notes 归组 / 无私钥 / 回滚步骤）于 2026-10-09 离线演练完毕，13/13 通过并接入 CI**；依赖远端与三平台真机的一半步骤已文档化于 `docs/release.md` §4.2，待 fork 仓库首跑。

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
| 09 测试体系、打包与发布 | ✅ 已完成 | `tasks/09` 全勾；覆盖率整体 85.27% / 领域 91.69% 门禁生效（红/绿演练均过）；Windows 打包实测 3.9MB；S9-03 固定种子方案 / S9-05 未签名路径；S9-06 补离线演练工具 `tools/release_dryrun.py`（13/13 通过、红路径 exit 1、已入 CI）与 `tools/gen_release_notes.py`（Notes 逻辑可离线验证）；**遗留**：预发布演练的远端 / 真机部分、macOS/Linux 真机回归（见 `tasks/09` 状态段与 `docs/release.md` §4.2） |

**主线之外的增补工作（均已完成并有实施记录）**

- **P1/P2 未交付功能补救**（`docs/p1p2-remediation-plan.md` 总览表 5/5 ✅，逐步记录见 `docs/p1p2-implementation-log.md`）：拖拽打开、自定义主题（8 核心语义色）、数学公式/Mermaid（结构化卡片 + 导出资源注入）、拼写检查（3779 词 + 近邻门槛 + 波浪线）、图片导出（视口截图 + **整篇长图**，10-09 补齐）。
- **中文文本与图标显示修复**（`docs/font-encoding-fix-plan.md` §六，2026-10-08 完成）：CJK 字体注入（本机命中 msyh.ttc）、9 个无字形符号替换、GBK/UTF-16 解码（内嵌映射表 47 KB，零新依赖）、导出 HTML 中文字体族。
- **P2 遗留缺陷五项修复**（2026-10-09）：拖拽目录误开、GB18030 四字节解码、保存写回原编码、长图导出、导出 HTML 注入 KaTeX/Mermaid 资源。
- **v1.1 P1 四项收官**（2026-10-09，按「先排除阶段 9 任务」口径的执行顺序 V11-12 → V11-03 → V11-04 → V11-05）：
  - **V11-12 规范 15 机械门禁**：`lib.rs` 开启 `clippy::unwrap_used`/`expect_used`/`panic` 门禁 + 测试豁免，14 处非测试 `unwrap`/`expect` 全部重写为非 panic 形态（锁损坏 `PoisonError::into_inner`、选项 `?`/`and_then`/`let…else`），零 `#[allow]` 残留，红路径演练通过。
  - **V11-03 状态栏编码接线（DEF-05）**：共享内核 `TextEncoding` + 三层接线 + 每帧同步，未落盘显示 `—` 不谎称 UTF-8，GBK/UTF-8 用例通过。
  - **V11-04 PDF 开放（方案 B）**：开放入口并诚实提示「经系统打印对话框」，打印副作用抽为可注入 `PrintDispatcher`，CI 不弹框。
  - **V11-05 写作目标进度条**：领域 `WritingGoal` + 应用 `goal_progress` + 状态栏百分比/细进度条/达成去重提示，旧 TOML 配置缺字段按「未设置」兼容。

**验证链最新记录**（2026-10-09，v1.1 P1 收官后；2026-10-09 晚间复测一致）：`fmt --check` 零差异 · `clippy --workspace --all-targets -D warnings` 0 告警 · `check-arch` OK · 全量 `cargo test --all-targets -- --skip large_full_text_find_is_bounded` 全绿（**注意**：性能阈值用例 `large_full_text_find_is_bounded` 在 debug 构建下**稳定**超过 1500ms 上限、3/3 次失败，被 `--skip` 掩去，并非真全绿；已登记为 **V11-13**，见 §1.1.2；其大小写敏感对照 `large_find_case_sensitive_is_bounded` 始终通过）。v1.1 P1 四项（V11-12/03/04/05）新增用例并入基线。

**离线预发布演练记录**（2026-10-09，`tools/release_dryrun.py`）：绿路径 **13/13 通过**（tag v0.1.0 ↔ Cargo.toml 0.1.0 一致、产物 sha256 重算一致、Notes 按阶段聚合、无私钥入仓、回滚步骤齐备）；红路径 `--tag v9.9.9` → 11/12 且 **exit 1**，证明门禁会红。

### 1.1.1 第二轮梳理结论（2026-10-09 · 按「先排除阶段 9 任务」口径）

| 类别   | 项                                              | 状态 | 判断依据                                                                                                                        |
| ---- | ---------------------------------------------- | -- | --------------------------------------------------------------------------------------------------------------------------- |
| 已完成  | 阶段 01–08 全部任务单元                                | ✅  | `.agent/tasks/01..08` **无任何未勾选项**；`docs/01..08` 出口检查全勾                                                                      |
| 已完成  | 阶段 09 · S9-01~S9-08                            | ✅  | `.agent/tasks/09` 8/8 勾选；覆盖率 85.27% / 91.69%；Windows zip 3.9 MB ≤ 10 MB                                                     |
| 部分完成 | 阶段 09 出口检查 5/6                                 | ⚠️ | 唯一未闭合项「发布演练」的**离线部分已演练 13/13 并入 CI**，远端 / 真机部分待环境                                                                           |
| 已完成  | 主线外增补：P1/P2 五项补救、字体与编码修复、P2 五项缺陷修复、S9-06 演练工具化 | ✅  | `docs/p1p2-implementation-log.md`、`docs/font-encoding-fix-plan.md`、`tools/release_dryrun.py`                                |
| 已完成  | 源码分层与规模                                        | ✅  | `src/` 6 层 136 个 `.rs` 约 3.7 万行；10 个显式 `[[test]]` 目标；`check-arch` OK                                                        |
| 未完成  | V11-02 macOS / Linux 真机回归                      | ⬜  | 本机仅 Windows；`docs/manual-qa-{macos,linux}.md` 未执行；`docs/metrics.md` 三平台指标空白                                                 |
| 已完成  | V11-12 规范 15 机械门禁（P1）                          | ✅  | `lib.rs` 已开启 `clippy::unwrap_used`/`expect_used`/`panic` + `#[cfg_attr(test, allow)]`；14 处非测试命中全部重写，零 `#[allow]` 残留；红路径演练通过 |
| 已完成  | V11-03 状态栏编码接线（DEF-05，P1）                      | ✅  | 共享内核 `TextEncoding` + 三层接线 + 每帧同步；GBK/UTF-8 状态栏用例通过                                                                         |
| 已完成  | V11-04 PDF 开放（P1，方案 B）                         | ✅  | 开放 PDF 入口并诚实提示「经系统打印对话框」；打印副作用抽为可注入 `PrintDispatcher`，CI 不弹框                                                                |
| 已完成  | V11-05 写作目标进度条（P1）                             | ✅  | 领域 `WritingGoal` + 应用 `goal_progress` + 状态栏百分比/细条/达成去重提示；TOML 兼容旧配置                                                         |
| 未完成  | V11-06 ~ V11-09（P2）                            | ⬜  | 见 `docs/v1.1-backlog.md` 逐项说明                                                                                               |
| 已排除  | V11-01 / V11-10 / V11-11（阶段 9 相关）              | ⏸  | 按维护者要求移入 backlog 附录 A，编号保留以便追溯                                                                                              |

**本轮新发现的规范缺口**：`.agent/conventions.md` §四-15 与 `CONTRIBUTING.md` §4 规定「生产代码禁用 `unwrap()` / `expect()` / `panic!`」，但**没有任何机械检查**——`clippy` 默认不开 `unwrap_used` / `expect_used`，`check-arch` 只校验分层依赖。实测 `src/` 下 61 处 `unwrap()` / `expect()` 中 **14 处不在 `#[cfg(test)]` 上下文**，均依赖局部不变式保证（如 `frame.rs:1925` 前方有 `else { return }` 守卫；`markdown/service.rs` 的 6 处 `Mutex::lock().expect` 在 `panic = "abort"` 下不可达），无一是当前已知缺陷，但规范与实现存在灰区 → 登记为 **V11-12（P1）**。领域层 `src/domain/**` 命中的 3 处经核对**全是文档注释中的文字**（提及 `std::fs`），非真实 `use`，合规。

### 1.1.2 第三轮梳理结论（2026-10-09 晚间 · 全量口径，不预先排除阶段 9）

> 本轮审计**未**预先排除阶段 9 任务，故 V11-01 / V11-10 / V11-11 重新进入候选（仍受「远端 / CI / 证书 / 预算」前置约束）。另新增一项已知问题。

| 类别      | 项                                        | 状态   | 判断依据                                                                                                                                                                     |
| ------- | ---------------------------------------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 已完成     | V11-12 规范 15 机械门禁（P1）                    | ✅    | 重跑 `cargo clippy --workspace --all-targets -- -D warnings` 退出码 **0**；`src/lib.rs` 26–27 行 gate 在位                                                                        |
| 已完成     | V11-03 状态栏编码接线（DEF-05，P1）                | ✅    | `src/shared/encoding.rs` 枚举 + 三层接线；GBK / UTF-8 状态栏用例通过                                                                                                                   |
| 已完成     | V11-04 PDF 开放（P1，方案 B）                   | ✅    | `PrintDispatcher` 可注入端口就位；导出入口开放；CI 不弹打印框                                                                                                                                |
| 已完成     | V11-05 写作目标进度条（P1）                       | ✅    | `WritingGoal` 值对象 + `goal_progress` + 状态栏百分比 / 细条 / 达成去重                                                                                                                 |
| 已完成     | 验证链（本轮重跑）                                | ✅    | `fmt --check` OK · `clippy -D warnings` 0 告警 · `check-arch` OK · 全量测试（跳过被掩的性能用例）**1304 passed / 0 failed**                                                               |
| ⚠️ 已知问题 | `large_full_text_find_is_bounded` 性能阈值用例 | ⬜    | debug 构建下 p50 稳定 **~2100ms**，超 1500ms 上限，**3/3 次复现失败**；大小写敏感对照 `large_find_case_sensitive_is_bounded` 通过；本会话及既往均未改动 find 逻辑，属环境 / 阈值标定问题。当前被 `--skip` 掩去 → 登记 **V11-13** |
| 未完成     | V11-02 macOS / Linux 真机回归                | ⬜    | 需真机 / 远端；`docs/metrics.md` 三平台指标空白                                                                                                                                       |
| 未完成     | V11-06 ~ V11-09（P2）                      | ⬜    | 见 `docs/v1.1-backlog.md`                                                                                                                                                 |
| 候选恢复    | V11-01 / V11-10 / V11-11（阶段 9 相关）        | ⏸→候选 | 本轮全量口径不再预先排除；恢复为下一阶段候选，仍受环境 / 预算前置约束（见 backlog 附录 A）                                                                                                                     |


**与 §1.1.1 的差异**：① 不再预排除阶段 9，V11-01/10/11 重新计入候选；② 新增「性能用例漂移」这一已知问题（V11-13），其失败被 `--skip` 掩去，**不能算作测试全绿**。

### 1.1.3 第四轮梳理结论（2026-10-10 · 用户反馈修复批次 + 验证链实测）

> 本轮由**用户实测反馈**驱动（原始问题记录：`docs/logs/问题记录1.txt`，2026-10-10 08:32），随后扩展为一整批 UI / 编辑器 / 预览 / 导出修复。全部改动已于 2026-10-10 提交（`git log` 共 **11 笔**，其中首笔为初始化提交）。**注意：本轮审计发现验证链当前为红（7 项失败），与 §1.1.1/§1.1.2 声称的「全绿」不符——后者只对 2026-10-09 的快照有效，10-10 批次未复跑全量。**

**触发问题（`docs/logs/问题记录1.txt` 四项）与处置**

| # | 用户报告                           | 处置落点（提交）                                         | 状态                                          |
| - | ------------------------------ | ------------------------------------------------ | ------------------------------------------- |
| 1 | 打开文件后编辑区看不到内容（深浅主题均如此）         | `57b407e` 编辑区对齐/字体族、`46e0e2d` 统一行高               | ✅ 已修（见下「验证链」对编辑区用例的实测）                      |
| 2 | 顶部菜单层级不严谨（下拉应属应用级，加粗等应属编辑区工具栏） | `cf2450a`/`4838d22` 分层重构：编辑区工具栏回到编辑区顶部，格式组平铺     | ⚠️ 代码已改，**4 条 `top_menu_test` 未同步 → 红**（见下） |
| 3 | 顶部菜单下拉的文案与快捷键显示重叠              | `cf2450a` 菜单行高等高同排                               | ✅ 已修                                        |
| 4 | 多标签切换后预览区只显示第一篇文档              | 复用既有 `switch_document` 路径（`preview_ui_state.rs`） | ⚠️ **未找到专用回归用例**，需补验证                       |

**本轮新增能力（超出「修 bug」范围的增量）**

- **编辑器内 LaTeX / Mermaid 真实渲染**：新增 `src/presentation/views/math.rs`（2068 行，自研 LaTeX 子集排版引擎）与 `mermaid.rs`（约 2968 行，自研 graph/flowchart 分层布局引擎）；解析 → 纯函数排版 → 绘制的三段分离，子集外写法**明确回退源码卡而非猜测**。零新依赖（ADR-002 体积预算不破）。
- **PDF 导出改走无头 Chromium**（ADR-004 修订，2026-10-10）：Windows 路径由 `powershell Start-Process -Verb PrintTo` 改为无头 Edge/Chrome `--print-to-pdf`，可**静默落盘真实 PDF**；`printto` 降为兜底并由 `PdfBackend::writes_pdf` 向界面暴露差别；顺带修复「纸张选项从未注入 `@page`」。
- **预览区四批次修复**：`053fbc6`（滚动/行内样式/公式图表/列表层级/脚注表格）、`4838d22`（图表出图、链接可点击、双向滚动、自动换行、工具栏平铺）、`46e0e2d`（目录/键帽/高亮/上下标）、`399d14f`（表格列宽自适应 + 统一间距刻度 `space_*`）。
- **交互与状态**：`f2c151c` 浮层不再每帧抢焦点（新增 `dialog_ui_state.rs`）；`e4b3237` 行号栏默认隐藏并接入设置开关；`1147357` 回车退出列表 / 去除每键整篇回读 / 程序性滚动不再驱动预览。

**验证链实测（2026-10-10 22:2x，本机 debug，`cargo test --all-targets --no-fail-fast`）**

```
cargo fmt --check                              → exit 0  ✅
cargo clippy --workspace --all-targets -D warnings → exit 0  ✅（0 告警）
cargo run --bin check-arch                     → OK（未发现分层依赖违规）  ✅
cargo test --all-targets --no-fail-fast        → 1195 passed / 7 failed / 1 ignored  ❌
```

规模：`src/` **148** 个 `.rs` / **46,678** 行（较 §1.1.1 的 136 个 / 3.7 万行增长，主要来自 `math.rs`/`mermaid.rs`）；`tests/` **82** 个 `.rs` / **30,067** 行。

**7 项失败分类（证据 → 性质 → 处置方向）**

| 目标             | 用例                                                                                     | 证据                                                                             | 性质                                                                                                                                        | 处置方向                                                                                  |
| -------------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| presentation   | `top_menu_test::editor_toolbar_lives_in_editor_pane_not_top_region` 等 **4 条**          | `panicked: 编辑区工具栏缺少分组「格式」`（`top_menu_test.rs:224/438/584/722`）                 | **测试滞后于代码**：`4838d22` 已把「格式」组**平铺为按钮**（`toolbar_view.rs:301` 有 `if title == "格式"` 分支，不再绘制分组标题），测试仍按「分组下拉」结构断言                             | 同步测试（改为按按钮文案定位）或恢复分组标题，二选一                                                            |
| integration    | `chinese_corpus_parses_to_stable_snapshot`、`gfm_full_corpus_parses_to_stable_snapshot` | 快照期望缺 `header_spans`/`rows_spans`/`depth`/`ordinal`/`spans` 新字段（`main.rs:126`） | **golden 期望未随解析器结构演进更新**：`053fbc6`/`46e0e2d` 给 `TableData`/`ListItem` 加了字段，`{:?}` 快照文本随之变化，而 `tests/data/golden/*.expected`（10-07 生成）未重生成 | 重生成 golden（`BLESS`/等价机制）并复核新增字段语义；顺带评估「快照是否应打印内部 `Debug`」的脆性                          |
| infrastructure | `pdf_exporter_test::real_export_writes_a_real_pdf_to_the_chosen_path`                  | `IoUnavailable: 调用系统打印失败：msedge.exe 未产出 PDF`（`pdf_exporter_test.rs:546`）       | **环境限制**：本会话内**手工执行**无头 Edge `--print-to-pdf` 同样 exit 0 但零产物（`--headless=old/new`/无模式均如此），故非代码缺陷，属无头浏览器在本会话不可用                            | 在普通终端 / CI 复跑确认；若真机也复现，则 **ADR-004 修订需复盘**（新 Edge 可能已移除旧 headless 的 `--print-to-pdf`） |

**与 §1.1.2 的差异**：① 完成度主体未变（九阶段 + v1.1 P1 四项仍全绿于其自证口径）；② **新增实质增量**：编辑器内公式/图表渲染、PDF 无头 Chromium 路径、四批预览修复；③ **新增 7 项红**：4 条工具栏测试（测试滞后）+ 2 条 golden 快照（期望滞后）+ 1 条 PDF 真实链路（环境）；④ 三处文档失实已同步（README §1/§4、backlog V11-04、`docs/06` 编码段——见文件头「本轮同步记录」）。

### 1.1.4 第五轮梳理结论（2026-10-10 深夜 · V11-06 ~ V11-09 一次性完成）

> 本轮执行 `docs/v1.1-backlog.md` 中 **V11-06 / V11-07 / V11-08 / V11-09** 四项 P2 任务（用户指令：「执行 V11-06~09」）。四项均属**本机可完成、无环境前置**，故一次并行推进；严格遵循「不触碰既有红项」的纪律，§1.1.3 登记的 7 条红**原样保留、未修**。

| 任务 | 交付物 | 关键落点 |
|---|---|---|
| **V11-06** 侧栏删除二次确认 | 异步确认 + 递归删除 | `DialogRequest::Confirm` 复用异步 `DialogRunner`；`EditorApp::request_delete` 登记待确认目标、帧尾回执后 `apply_delete_confirm`；`fs_workspace_repository::remove` 放宽为递归 `remove_dir_all`（由确认兜底） |
| **V11-07** 拼写检查增强 | 词表扩充 + 建议 + 一键替换 | 领域 `SpellChecker::suggest`（距离 ≤2，稳定排序）；应用层 `SpellSuggestQuery`；编辑区右键错词弹「你可能想输入」菜单（`replace_spelling`）；词表补高频词与英式拼写 |
| **V11-08** Markdown lint | 4 规则 + 开关 + 侧栏标签 | 新领域模块 `src/domain/markdown/lint.rs`；偏好 `LintSettings`；应用层 `LintQuery`/`lint_document`；侧栏新增第四标签「检查」（`SidebarTab::Lint`）；设置页规则开关 |
| **V11-09** 自定义主题细粒度配色 | +10 语法色（可选覆盖） | `CustomThemePalette` 增 10 项 `syn_*`（`Option<String>`，`None` = 跟随主题）；`from_custom_palette` 改为按背景亮度选内置基线（**修掉「浅色自定义主题沿用深色语法色」缺陷**）；签名纳入全部语法色；设置页新增折叠区 |

**验证链实测（2026-10-10 深夜，本机 debug，`cargo test --all-targets --no-fail-fast`）**

```
cargo fmt --check                                   → exit 0  ✅
cargo clippy --all-targets -- -D warnings           → exit 0  ✅（0 告警）
cargo run --bin check-arch                          → OK（未发现分层依赖违规）  ✅
cargo test --all-targets --no-fail-fast             → 1519 passed / 7 failed / 1 ignored
```

7 项失败**与 §1.1.3 完全一致**（4 条 `top_menu_test` + 2 条 golden 快照 + 1 条 PDF 真实链路），均为既有红（V11-14 ×6 / V11-15 ×1），本轮未引入任何新红。新增用例约 **32 条**（V11-06 10 + V11-07 ~11 + V11-08 13 + V11-09 8，另改写既有用例若干）全部通过。规模：`src/` **150** 个 `.rs`（较 §1.1.3 增 `domain/markdown/lint.rs`、`shared/text.rs`）；`tests/` **87** 个 `.rs`（新增 `app/delete_confirm_test.rs`、`application/spell_suggestion_test.rs`、`infrastructure/spell_suggest_test.rs`、`presentation/spell_menu_test.rs`、`presentation/lint_tab_test.rs`）。

**遗留提示**：本轮四项完成后，v1.1 P2 主清单（V11-06~09）清空；剩余可开工项仅 **V11-14**（P0，纯本机，恢复全绿基线的前提）与 **V11-15 / V11-02**（环境前置）。

---

## 1.2 下一阶段（v1.1）

**阶段 01–09 已全部完成，无进行中的阶段。** 下一阶段为 **v1.1**，任务规划见 [`docs/v1.1-backlog.md`](./v1.1-backlog.md)。本轮审计为**全量口径**（不预先排除阶段 9），故下一阶段候选含：已完成的 4 项 P1、待处置的已知问题 V11-13、待开工的 P2 四项，以及恢复为候选的阶段 9 相关三项（V11-01/10/11，受环境 / 预算前置约束）。

概要（详细内容 / 验收依据见 backlog）：

| 优先级    | 任务                         | 一句话目标                                                                                           | 状态    |
| ------ | -------------------------- | ----------------------------------------------------------------------------------------------- | ----- |
| **P0** | V11-02 macOS / Linux 真机回归  | 补齐三平台指标与手工回归，消除平台覆盖盲区（唯一需真机 / 远端环境的主清单项）                                                        | ⬜ 待真机 |
| ✅ P1   | V11-12 规范 15 机械门禁          | 把「生产代码禁用 unwrap」升级为机械检查，14 处灰区逐处裁决                                                              | ✅ 已完成 |
| ✅ P1   | V11-03 状态栏编码显示接线（DEF-05）   | 状态栏显示真实编码名                                                                                      | ✅ 已完成 |
| ✅ P1   | V11-04 PDF 导出开放（方案 B）      | 开放入口并诚实提示系统打印路径                                                                                 | ✅ 已完成 |
| ✅ P1   | V11-05 写作目标进度条             | 可设目标 + 状态栏进度 + 达成提示                                                                             | ✅ 已完成 |
| ⚠️ 已知  | V11-13 全文查找性能阈值用例漂移        | `large_full_text_find_is_bounded` debug 下稳定超 1500ms，被 `--skip` 掩去；需标定为 debug 友好阈值或重 `#[ignore]` | ⬜ 待处置 |
| ✅ P2   | V11-06 侧栏删除二次确认            | 异步确认对话框 + 目录递归删除                                                                                | ✅ 已完成 |
| ✅ P2   | V11-07 拼写检查增强              | 词表扩充 + 「你可能想输入」建议 + 一键替换                                                                         | ✅ 已完成 |
| ✅ P2   | V11-08 Markdown 格式检查（lint） | 4 规则 + 规则开关 + 侧栏「检查」标签跳转                                                                         | ✅ 已完成 |
| ✅ P2   | V11-09 自定义主题细粒度配色          | +10 语法高亮色可选覆盖（`None` = 跟随主题）                                                                      | ✅ 已完成 |
| **P0** | V11-14 测试期望同步            | 同步工具栏 + golden 快照期望，消除 6 条滞后红（**P2 清空后唯一纯本机可开工项**）                                            | ⬜ 待开工 |
| **P1** | V11-15 PDF 真实链路回归         | 复核无头 Chromium 出 PDF 在普通终端 / CI 通过                                                                            | ⬜ 待环境 |
| 环境前置   | V11-01 发布预发布演练（远端 / 真机部分）  | 完整发布链路 + 回滚（离线半已 13/13）                                                                         | ⏸ 待环境 |
| 环境前置   | V11-10 代码签名与安装器            | 证书决策 + notarize / 签名                                                                            | ⏸ 待预算 |
| 环境前置   | V11-11 测试提速与自动更新评估         | nextest / 自动更新调研                                                                                | ⏸ 待痛点 |

> **说明（2026-10-09 晚间全量口径）**：本轮审计未预先排除阶段 9 任务，故 V11-01 / V11-10 / V11-11 由 backlog 附录 A 恢复为下一阶段候选（仍受「远端 / CI / 证书 / 预算」前置约束，非功能阻塞）。V11-13 为本轮新发现的已知问题（详见 §1.1.2）。

> **本轮审计（2026-10-09）**：核对文档与代码实际状态时发现并修正下列漂移——  
> ① `docs/{01,02,03,07,08,09}-*.md` 的「阶段出口检查」此前全部未勾选，与 `.agent/tasks/NN-*.md` 状态段的实际结果冲突，已按真实完成情况回填（平台限定项以括注标明）；  
> ② `.agent/tasks/01` 的 3 项遗留（cargo-deny/bloat 安装、模板试跑、ADR-002 体积回填）均早已由后续阶段消化，已勾选并注明依据；`.agent/tasks/03` 的 S3-14（领域覆盖率 ≥90%）已由 S9-02 实测落地；  
> ③ 代码层：`status_bar_ui_state.rs` 的 `TextEncoding` 旧注释声称「本应用只读写 UTF-8」与 `encoding.rs` 的五编码读写能力矛盾，且 `goal_notified` 是零读写的死字段（写作目标未实现）——已改为诚实文案并移除死字段；状态栏「未接线到实际编码」登记为 **DEF-05**（V11-03）。

---

### 历史进度（2026-10-06，保留备查）

**阶段 04 代码已落地并跑通验证链**（详见 `docs/04-应用服务与用例编排.md`）：

- 5 个 AppService 全部落地，**签名只认 trait、只返 DTO，不暴露聚合根**：`DocumentAppService`（`DocumentRepository` + `BufferFactory` + `EventBus` + `Clock`）、`MarkdownAppService`（`MarkdownParser` + `SyntaxHighlighter`）、`WorkspaceAppService`、`PreferencesAppService`、`ExportAppService`
- 命令 / 查询 / DTO 三件套按上下文拆分（`commands.rs` / `queries.rs` / `dto.rs`），`registry.rs` 以 `Arc<RwLock<DocumentRegistry>>` 提供共享只读视图
- 跨上下文事件编排 `event_subscriptions.rs`：`AutoSaveTracker`（纯逻辑、可单测）+ 四个订阅者（`TextModified`→自动保存计时、`DocumentSaved`→最近文件、`PreferencesChanged`→缓存失效、`DocumentClosed`→停止追踪）
- 应用层集成测试 **57 项全绿**（6 个测试文件），含缓存命中不重复解析、防抖纯函数、大文件截断、草稿覆盖磁盘搜索、保存失败保留脏标记、导出失败不误报成功、无重入事件链
- **测试暴露并修复 3 个真实缺陷**：① 持锁发布事件导致订阅者重入 `RwLock` **死锁**（已改为「锁内变更 → 锁外发布」）；② `compute_stats` 漏统计 `- [ ]` 未勾选任务；③ `EventBus::subscribe` 的 `&mut self` 签名阻塞共享后注册
- 验证链实测：`cargo fmt --check` ✅ ｜ `clippy --all-targets -D warnings` 零告警 ✅ ｜ `cargo test` **269 项全绿** ✅ ｜ `check-arch` OK ✅

**阶段 03 代码已落地并跑通验证链**（详见 `.agent/tasks/03-领域模型实现.md`）：

- 5 个限界上下文全部实现：`document`（值对象 / `TextBuffer` trait / `EditingService` / 事件 / 仓储 trait / 聚合根 / `UndoStack`）、`markdown`（值对象含 `source_range` / 三个 trait / `BlockMapper` / `RenderedDocument`）、`workspace`（条目规则 / 最近文件 / 标签页 / 关闭决策）、`preferences`（`KeyCombo` 归一化 / 冲突检测 / `TabSize`）、`export`（`ExportJob::validate` 判定表 / TOC 转义标记）
- 共享内核新增 `shared/time.rs`（`Timestamp` + 可注入 `Clock`，满足规范第 21 条）与 `shared/path.rs`（`FilePath` 值对象，替代 `std::path::PathBuf`）
- 测试 **212 项全绿**（领域层 166 + shared 37 + 架构 9）；领域层**零 `unwrap()`、零 `println!`**（由 `src/domain.rs` 的 `#![deny(clippy::unwrap_used, clippy::print_stdout, ...)]` 在编译期强制）
- 验证链实测：`cargo fmt --check` ✅ ｜ `clippy --all-targets -D warnings` 零告警 ✅ ｜ `cargo test` 212 项全绿 ✅ ｜ `check-arch` OK ✅

**阶段 02 代码已落地并跑通验证链**（详见 `.agent/tasks/02-工程骨架与分层边界.md`）：

- `Cargo.toml`（lib + bin 双目标、`profile.release` 五项）、118 个 `.rs` 模块文件（**0 个 `mod.rs`**）、`shared` 四模块（error/result/ids/geometry/event）
- 架构边界校验器 `tools/arch_check.rs`，由 `build.rs` 与 `src/bin/check-arch.rs` 共用；注入违规实测拦截（退出码 101 + 精确行号）
- CI 三平台矩阵 + deny job；`.rustfmt.toml` / `.gitignore` / `CONTRIBUTING.md`
- 验证链实测：`cargo fmt --check` ✅ ｜ `clippy --all-targets -D warnings` 零告警 ✅ ｜ `cargo test` 32 项全绿（shared 23 + 架构 9）✅ ｜ `check-arch` OK ✅

**阶段 01 产出已落地**：`REQUIREMENTS.md`（FR×6 / FUNC×26 / NFR×17 / CONSTR×12）、`docs/adr/ADR-001`~`005`（含 Tauri 冲突裁决）、`.agent/`（README + conventions + architecture + 9 份任务清单 + 4 份提示词模板）、`docs/prototype-contract.md`（A 契约 29 条 + B 免疫清单 17 项）、`docs/metrics.md`、`docs/dependencies.md`、`deny.toml`、`rust-toolchain.toml`（固定 1.99.0）。

**阶段 01 剩余 3 项**（不阻塞阶段 02 开工）：

1. 安装 `cargo-deny` / `cargo-bloat`（后台安装已取消，需手动执行）；
2. 首次派发任务时用 `implement-domain.md` 模板试跑 S2-03 验证模板可用；
3. 依赖 feature 清单的实测核对（pulldown-cmark / syntect 的 crates.io 元数据已核实并写入 ADR-002 与 `docs/dependencies.md`）。

## 2. 阶段总览与进度

| 序号 | 阶段         | 文档                                             | 对应需求文档章节                   | 对应里程碑 | 预估工期  | 状态                                           |
| -- | ---------- | ---------------------------------------------- | -------------------------- | ----- | ----- | -------------------------------------------- |
| 01 | 需求基线与技术决策  | [docs/01-需求基线与技术决策.md](./docs/01-需求基线与技术决策.md) | §1–§2、§5、§10               | 前置    | 0.5 周 | ✅ 已完成                                        |
| 02 | 工程骨架与分层边界  | [docs/02-工程骨架与分层边界.md](./docs/02-工程骨架与分层边界.md) | §5.3、§6.2、§7.1             | M1 前段 | 1 周   | ✅ 已完成                                        |
| 03 | 领域模型实现     | [docs/03-领域模型实现.md](./docs/03-领域模型实现.md)       | §6.1、§6.4、§7.1（domain/）    | M1    | 2 周   | ✅ 已完成                                        |
| 04 | 应用服务与用例编排  | [docs/04-应用服务与用例编排.md](./docs/04-应用服务与用例编排.md) | §6.2、§7.1（application/）    | M1–M2 | 1.5 周 | ✅ 已完成                                        |
| 05 | 基础设施实现     | [docs/05-基础设施实现.md](./docs/05-基础设施实现.md)       | §5.1、§7.1（infrastructure/） | M2    | 2 周   | ✅ 已完成（出口 6/6 通过）                             |
| 06 | 表现层与交互实现   | [docs/06-表现层与交互实现.md](./docs/06-表现层与交互实现.md)   | §3、§7.1（presentation/）     | M2–M3 | 2.5 周 | ✅ 已完成（含 S6-12 收尾修复）                          |
| 07 | 集成联调与原型对齐  | [docs/07-集成联调与原型对齐.md](./docs/07-集成联调与原型对齐.md) | §3.2、§4.1                  | M3    | 1 周   | ✅ 已完成（DEF-01/02 已修；macOS/Linux 真机回归待阶段 09）   |
| 08 | 性能优化与质量加固  | [docs/08-性能优化与质量加固.md](./docs/08-性能优化与质量加固.md) | §4.1、§8                    | M4    | 1.5 周 | ✅ 已完成（DEF-03/04 已修；出口 7 项对照全过）               |
| 09 | 测试体系、打包与发布 | [docs/09-测试体系打包与发布.md](./docs/09-测试体系打包与发布.md) | §4.3、§4.4、§11              | M4    | 1.5 周 | ✅ 已完成（覆盖率门禁生效；预发布演练离线部分 13/13，远端 / 真机部分待 CI） |


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
| ADR-001 | UI 框架  | **egui + eframe 0.33.3**（wgpu 后端），不采用 Tauri                                                   | 需求文档 §5.1 / §5.2；与原型 `DESIGN-OVERVIEW.md` §1「目标态 Tauri v2」冲突，以需求文档为准 |
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
