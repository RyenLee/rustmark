# rustmark 发布卫生审计（2026-10-31）

> **性质**：只读审计。本文档是本轮唯一新增文件，未修改任何源码、测试、脚本或既有文档，也未提交 commit。
> **范围**：`d:\Dev\workspace\2026\10\rust\rustmark` 全仓 tracked 文件（783 个）。
> **结论口径**：每条发现均标注「事实 / 判断」。事实来自命令输出与实际文件内容；判断是审计意见，可被反驳。
> **优先级**：P0 阻断发布 · P1 发布前应处理 · P2 建议处理 · P3 可择机处理。
> **工作量**：S（<30 min）· M（0.5–2 h）· L（>2 h）。

---

## 1. 执行摘要

**总体判断：仓库已处于可发布状态，没有 P0 级阻断项。** 发布门禁脚本在目标版本上全绿，依赖与许可证治理闭环，源码内无调试残留硬伤。需要处理的是**发布面文档漂移**与**少量误提交/一次性产物**，全部可在 1–2 个低风险批次内清完。

关键事实（均可复现，见附录 B）：

| 检查项 | 事实结果 | 判断 |
|---|---|---|
| `python tools/release_dryrun.py --tag v0.2.2` | **16/16 PASS** | 目标版本发布演练全绿 |
| `python tools/release_dryrun.py --tag v0.2.1` | 14/16，2 FAIL（版本守卫、SBOM 根组件） | 预期性失败：当前版本已是 0.2.2，非缺陷 |
| `python tools/check_thresholds.py` | exit 0「OK」，但打印 3 处 `[drift]` | 行号漂移，CI 默认不失败；建议 `--fix` 回写 |
| `cargo metadata --no-deps` | 10 个包全部 `version 0.2.2`、`MIT OR Apache-2.0`、`publish=[]` | 版本与许可元数据一致 |
| `dbg!` / `todo!` / `unimplemented!` / `FIXME` / `TODO` | 源码内 **0 命中** | 无调试宏与占位实现遗留 |
| 注释掉的整段代码（启发式） | **0 命中** | 无大面积注释代码 |
| `#[ignore]` | 11 处，**全部带理由字符串** | 符合仓库「ignored 用例须说明」约定 |
| `log::*!` | 33 处 / 14 文件，无刷屏证据 | 属 release observability，**保留** |

需要处理的高优先项（P1，共 2 条）：

1. **根目录 `lcov.info` 被跟踪**（1,256,956 B 覆盖率生成物，自初始提交 `338d333` 起入库），且 `.gitignore` 未忽略 `lcov.info`。CI 每次 `cargo llvm-cov` 会重写该文件，易造成无意义 diff 与仓库膨胀。
2. **`.claude/artifacts/plans/large-file-refactor.md`**（54,952 B）是重构期的一次性 agent 计划：`Author: ryenlii`、`Last updated: 2026-10-11`、`Baseline commit: 674ce65`、AC-4 基线「1572 passed」已过期，且含约 100 条指向 `file:///d:/data/WorkBuddyCache/...` 的**本机绝对路径死链**。`.claude/` 未被 `.gitignore` 忽略，是唯一被跟踪的 `.claude` 文件。

P2 主要是**文档口径漂移**：11 MiB 红线已上调，但 `Cargo.toml:198` 与 `docs/guides/dependencies.md:53` 仍写「10 MB」；`docs/README.md` 漏收 `reviews/`、`diagrams/` 且总数 139 vs 实测 143；`docs/reports/release-compliance-gap-report.md` 版本/组件数停留在 0.2.0/186/388；多份文档引用被 gitignore 的 `.agent/` 目录。

**结论：先做批次 0（文档口径统一，S），再做批次 1（误提交产物与一次性文档，M），P3 择机。**

### 1.1 批次执行记录（2026-10-31）

> 本节记录落地状态；未提交 commit，改动留在工作区。版本保持 `0.2.2`，未打 tag。

**已完成（批次 1 + 批次 2 + 批次 0（含剩余项），均 2026-10-31）：**

| 项 | 落地动作 | 状态 |
|---|---|---|
| #1 `lcov.info` | `git rm --cached`（保留本地文件）；`.gitignore` 增 `lcov.info` / `*.lcov` | ✅ 已完成 |
| #2 `.claude/artifacts/plans/large-file-refactor.md` | 提炼结论到 §1.2；`git rm` 移除（工作区与索引） | ✅ 已完成 |
| #3 `Cargo.toml:198` | 「10 MB」→「11 MiB（11,534,336 B，2026-10-25 上调）」 | ✅ 已完成 |
| #4 `docs/guides/dependencies.md:53` | 同上口径 | ✅ 已完成 |
| #6 `docs/README.md` | 补 `reviews/`、`diagrams/` 索引；合计 139→143；`remediation` 4→5；目录数 12→14；修正 `.agent/conventions.md` 死引用 | ✅ 已完成 |
| 同类过期口径 | `docs/guides/release.md:49`、`docs/guides/debug-guide.md:75/85-88/438`、`crates/rustmark-shared/src/shared/support/pattern.rs:15` 一并改为 11 MiB | ✅ 已完成 |
| 批次2 #14 `preview_view_test.rs:1737` | 删除无条件 `[dump]` eprintln（断言已含 `overflow2` 诊断） | ✅ 已完成 |
| 批次2 #15 `search_alignment_test.rs:143` | 删除失败分支 `DEBUG` eprintln（断言消息已含 texts） | ✅ 已完成 |
| 批次2 #16 `CHANGELOG.md` | 补 `[0.2.1]`/`[0.2.2]` 链接定义；`[未发布]` compare 基线改 `v0.2.2`；`docs/metrics.md`→`docs/spec/metrics.md`（全文 5 处） | ✅ 已完成 |
| 批次2 #17 `PRIVACY.md` | 适用版本 `v0.2.0`→`v0.2.2` | ✅ 已完成 |
| 批次2 #18 `debug-guide.md:106` | **误报**：该行是 `.vscode/launch.json` 的 schema `"version": "0.2.0"`，非项目版本，**不改** | ✅ 已核实 |
| 批次2 #19 `README.md` / `THIRD-PARTY-LICENSES.md` | 注明 186（许可条目口径）vs 395（SBOM 组件口径）差异 | ✅ 已完成 |
| #5 `docs/spec/thresholds.baseline.yaml` | `python tools/check_thresholds.py --fix` 回写 3 处行号（`window_min_width` 197→210、`window_min_height` 197→210、`env_skip_arch_check` 60→82）；复跑无 `[drift]` | ✅ 已完成 |
| #7 `docs/reports/release-compliance-gap-report.md` | 顶部加「数字口径（2026-10-22 快照）」段：§0–§5 的 `0.2.0`/186 crate/388 组件/exe 字节数为当日实测并按「历史不改写」保留；现行口径指向 `Cargo.toml`(0.2.2)、CHANGELOG、SBOM 395、`spec/metrics.md` | ✅ 已完成 |
| #8 `README.md:44/181` | `状态核对基准` 的 `.agent/tasks/` → `docs/plans/PROGRESS.md` + `docs/archive/`；`编码规范：.agent/conventions.md` → `CONTRIBUTING.md §4` + `docs/spec/architecture-constraints.md` | ✅ 已完成 |
| #9 `CONTRIBUTING.md:4/73/116` | `.agent/README.md` → `docs/plans/PROGRESS.md`；`.agent/tasks/` 任务编号 → `docs/plans/PROGRESS.md` / `docs/stages/`（外部贡献者可用 Issue 编号）；`.agent/conventions.md` → `docs/spec/architecture-constraints.md` + `REQUIREMENTS.md` 的 `CONSTR-*` | ✅ 已完成 |
| #10 `docs/guides/testing.md` | 4 条「当前状态」改为 1 条「最近基线（2026-10-23 实测）」+ 3 条「历史记录」；并注明本文件不随每次提交更新、当前计数以 CI 为准 | ✅ 已完成 |
| #12 `highlighting` | **决策：保留实现**。复核发现已接入生产（`start_session` → `highlight_worker`），原「未接入」为误报；重写 `highlighting.rs` 与 `incremental.rs` 的现状注释；删除 `reused_lines()`/`parsed_ratio()`/`last_spans()` 三个死方法与 `mod diff` 的无用 `#[allow(dead_code)]` | ✅ 已完成 |
| #13 Logo 中间产 | **决策：移出发布仓**。`git rm` 删除 `rustmark-logo.miora` + `rustmark-logo_assets/`（12 PNG，8,867,575 B）；`assets/logo/build_logo.py` 头部注明来源与可追溯性；定稿资产（SVG/PNG/ico/icns）保留 | ✅ 已完成 |
| #11 `docs/pic/PixPin_2026-10-31_09-50-29.png` | 未跟踪截图：**移入 `.agent/artifacts/`**（`.agent/` 已 gitignore），不删除用户素材；`docs/pic/` 下不再有未跟踪文件 | ✅ 已完成 |
| #20 测试临时目录 helper | 新增 `tests/support/tempdir.rs`（`new/path/join/write/mkdir` + `Drop`），替换 5 个基础设施测试文件中各自复制的 `TempDir`；`preferences` 的 `backups()` 降为文件内自由函数；`asset_loader`/`exporter` 的 `dir.0` 改用 `dir.path()` | ✅ 已完成 |

### 1.2 已移除一次性文档的可保留结论（`large-file-refactor.md` 提炼）

原文件（2026-10-11，基线 `674ce65`）的核心结论保留如下；原文件含约 100 条本机绝对路径死链且行号已随重构失效，故不再入库。

- **性质**：DDD 大文件专项拆分，纯机械搬迁、零业务逻辑变更。
- **验收口径（AC-1~AC-9）**：① >500 行的 31 个文件拆分后 facade 与子文件均 ≤500 行；② 每个 commit diff 只含「整体移动 / `mod`·`use` 声明 / 可见性关键字 / `include_bytes!` 相对路径」四类；③ 既有模块路径零破坏，`cargo check --all-targets` 通过；④ 测试结果逐字节一致（当时基线 1572 passed / 1 failed / 1 ignored）；⑤ `check-arch` + `clippy -D warnings` + `fmt --check` 全绿；⑥ golden 快照零改动；⑦ 覆盖率不降（整体 ≥70%、domain ≥90%）；⑧ release 构建通过且不超体积红线；⑨ 每个原始文件一个独立 commit，可单独 `git revert`。
- **执行结构**：波次 0 基线与护栏 → 波次 1 领域层（4 commit）→ 波次 2 应用服务层（3）→ 波次 3 基础设施层（6）→ 波次 4 表现层 UI State（5）→ 波次 5 视图/主题/快捷键（12）→ 波次 6 组合根 `frame.rs`（1）→ 波次 7 收口。
- **阈值定义**：红线 >500 行；黄线 300–500 行（登记 follow-up）；统计口径按 `\n` 计数。

> 结论：上述为「当时的执行方案」，属历史证据；现行架构以 `docs/spec/architecture-constraints.md` 与 `docs/adr/` 为准。

### 1.3 本批验证结果（2026-10-31）

| 命令 | 结果 |
|---|---|
| `cargo fmt --check` | exit 0（无输出） |
| `cargo check --workspace --all-targets` | exit 0（Finished dev profile） |
| `python tools/release_dryrun.py --tag v0.2.2` | **16/16 PASS** |
| `python tools/check_thresholds.py` | exit 0「OK」；3 处既有行号漂移（`window_min_width`、`window_min_height`、`env_skip_arch_check`）**未变化**，非本批引入 |
| `git ls-files lcov.info` / `git ls-files .claude` | 均为空（已取消跟踪）；本地 `lcov.info` 保留 1,256,956 B |
| `docs/README.md` 索引链接 | `docs/reviews/`、`docs/diagrams/project-relationship-map.md` 均存在，无新增死链 |

**残留 `10 MB` 命中说明**：本批改完后仍有 `10 MB`/`10 MiB` 字样的文件均为**历史证据或决策引用**，按仓库「历史不改写」纪律保留——`CHANGELOG.md`（历史条目）、`docs/stages/*`（阶段快照）、`docs/spec/metrics.md`（历史实测节）、`docs/spec/architecture-constraints.md:607`（记录上调动作）、`scripts/size_gate.sh:16-19`（历史注释）、`.github/workflows/ci.yml:56`（记录「2026-10-25 由 10 MiB 上调」）、`crates/rustmark-infra/src/infrastructure/{spell/dictionary.rs:5,export/html_sanitize.rs:23,export/system_pdf_exporter.rs:6}`（引用 ADR-001 / §8.1 的**历史**预算）。

### 1.4 批次 2 验证结果（2026-10-31）

| 命令 | 结果 |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo check --workspace --all-targets` | exit 0（Finished dev profile） |
| `python tools/release_dryrun.py --tag v0.2.2` | **16/16 PASS** |
| `python tools/check_thresholds.py` | exit 0「OK」；3 处既有行号漂移不变（非本批引入） |
| CHANGELOG 链接 | `[0.2.1]`、`[0.2.2]` 定义已补；`[未发布]` 基线改 `v0.2.2`；`docs/metrics.md` 已全部改为 `docs/spec/metrics.md` |
| 测试文件改动 | 仅删 2 处调试打印，未改任何断言逻辑；`cargo check --all-targets` 通过 |

### 1.5 批次 0 剩余项验证结果（2026-10-31）

| 命令 | 结果 |
|---|---|
| `python tools/check_thresholds.py --fix` | 回写 3 处行号（`[fix] 回写 3 处行号`） |
| `python tools/check_thresholds.py` | exit 0「OK: 基线与源码一致」，**无 `[drift]` 输出** |
| `cargo fmt --check` | exit 0 |
| `cargo check --workspace --all-targets` | exit 0（Finished dev profile） |
| `python tools/release_dryrun.py --tag v0.2.2` | **16/16 PASS** |
| `rg '\.agent/'`（根 Markdown） | `README.md` / `CONTRIBUTING.md` 已无命中；仅 `CHANGELOG.md:4/111` 保留（历史条目与历史 commit 约定说明，按「历史不改写」保留） |
| 文档链接 | `docs/plans/PROGRESS.md`、`docs/spec/architecture-constraints.md`、`docs/stages/`、`CONTRIBUTING.md` 均为 tracked 文件，无新增死链 |

> 残留说明：`CHANGELOG.md:4` 仍写「`[S{阶段}-{编号}]` 对应 `.agent/tasks/` 的任务编号」。该行为 CHANGELOG 头部对历史 commit 约定
> 的说明，本批未纳入 #8/#9 范围，按「历史不改写」保留；如需一并改为 `docs/plans/PROGRESS.md`，请在后续批次确认。

### 1.6 批次 3（#12 / #13）验证结果（2026-10-31）

| 命令 | 结果 |
|---|---|
| `cargo check -p rustmark-infra --all-targets` | exit 0，**无 dead_code 告警**（移除 `#[allow(dead_code)]` 后 `diff` 全部被引用） |
| `cargo test -p rustmark-infra --lib incremental` | **11 passed / 0 failed / 2 ignored**（收益基准按设计 `#[ignore]`） |
| `cargo fmt --check` | exit 0 |
| `cargo check --workspace --all-targets` | exit 0（Finished dev profile） |
| `python tools/release_dryrun.py --tag v0.2.2` | **16/16 PASS** |
| `git ls-files rustmark-logo_assets rustmark-logo.miora` | 空（已取消跟踪；本地文件随 `git rm` 移出） |
| `rg 'rustmark-logo_assets\|rustmark-logo.miora'` | 仅 `assets/logo/build_logo.py` 与历史文档（`CHANGELOG`、`docs/logs`、`docs/plans`）命中，无活引用 |

> 说明：#12 只删死代码与改注释，未触及增量算法与 `start_session` 接线；#13 只删设计中间产，未触及 `assets/logo/` 定稿资产与打包脚本。

### 1.7 批次 3 尾项（#11 / #20）验证结果（2026-10-31）

| 命令 | 结果 |
|---|---|
| `cargo check -p rustmark --test infrastructure` | exit 0，**0 warning**（原 3 个未使用导入与 9 处 `dir.0` 已修） |
| `cargo test -p rustmark --test infrastructure` | **309 passed / 0 failed / 1 ignored**（与抽取前一致） |
| `cargo check -p rustmark --test presentation` | exit 0，0 warning（`support::tempdir` 已加模块级 `dead_code` 豁免） |
| `cargo fmt --check` | exit 0 |
| `cargo check --workspace --all-targets` | exit 0 |
| `python tools/release_dryrun.py --tag v0.2.2` | **16/16 PASS** |
| `git status --short`（未跟踪项） | 无（截图已移入 gitignore 区 `.agent/artifacts/`） |

> 说明：#20 仅搬家 `TempDir` 与 3 个通用方法，测试断言未改；`preferences` 的 `backups()` 保持原过滤语义（`.bak-`）。

---

## 2. 扫描范围与方法

### 2.1 范围

| 类别 | 实测 tracked 文件数 | 说明 |
|---|---|---|
| `src/` | 92 | root 包源码（app / application / domain / infrastructure / presentation / shared / bin） |
| `crates/*/src/` | 291 | 9 个成员 crate（actions / assembly / core / icons / infra / ports / render / shared / ui） |
| `tests/` | 154 | `autotests = false`，全部测试目标在 Cargo.toml 显式声明 |
| `docs/` | 143 | 15 个子目录 + 1 个 README |
| `assets/` | 50 | 字体、Logo、图标、词表、编码表 |
| `.github/` | 6 | `ci.yml` / `release.yml` 等 |
| `scripts/` | 5 | `size_gate.sh` / `package-{windows.ps1,macos.sh,linux.sh}` 等 |
| `tools/` | 8 | 架构校验、阈值、覆盖率、SBOM、发布演练 |
| `rustmark-logo_assets/` | 12 | Logo 设计中间产（PNG） |
| 根级与其他 | 22 | 含 `Cargo.toml` / `Cargo.lock` / `build.rs` / `deny.toml` / 根 Markdown / 许可证，及 `.claude/` 下 1 个计划文件 |
| **合计** | **783** | `git ls-files` 全量 |

**不在范围**：`target/`、`dist/`（构建产物，非源码）。

### 2.2 方法与命令

全部为只读操作：

```
git ls-files                                  # 枚举 783 个 tracked 文件，按目录计数
git status                                    # 工作区状态（13 修改 + 1 未跟踪）
rg 'dbg!|todo!|unimplemented!|FIXME|TODO'     # 调试宏与占位
rg 'println!|eprintln!'                       # 输出分诊（区分正式输出/诊断/调试）
rg '#\[allow\(dead_code\)\]'                  # 死代码豁免
rg '#\[ignore'                                # ignored 用例
rg 'log::(debug|trace|info|warn|error)!'      # 日志
rg '^//\s*(let|fn|if|match|for|while|use|...)' # 注释掉代码启发式
cargo metadata --no-deps                      # 成员、版本、许可、publish
cargo tree                                    # 依赖拓扑（与 workspace-topology.expected.txt 比对）
python tools/release_dryrun.py --tag v0.2.1   # 发布演练（预期性失败，见 §1）
python tools/release_dryrun.py --tag v0.2.2   # 发布演练（目标版本，全绿）
python tools/check_thresholds.py              # 阈值基线核对（3 处行号漂移）
```

### 2.3 未执行项与理由

- **全量 `cargo test` 未本地复跑**：耗时高，且 CI `check` job（三平台矩阵）与 `perf` job（Linux release）已覆盖。本文档引用现有 CI 口径，不新造测试结论。
- **`cargo check --workspace --all-targets` 未复跑**：CI `check` job 以 `-D warnings` 编译全部目标，死代码/未用导入会直接红；本地复跑收益低。
- **未使用依赖（unused dependency）未逐条核验**：`cargo-machete`/`cargo-udeps` 未安装，本文档将其列为「待确认」，不写成结论。

### 2.4 已排除的误报（避免后续重复劳动）

| 模式 | 位置 | 排除理由 |
|---|---|---|
| `eprintln!` | `crates/rustmark-core/src/application/document/service/stats/equivalence.rs:286-289` | `#[cfg(test)]` 性能对照用例内的**有意指标输出**；release 口径做硬断言（`:290-295` 有注释与 `#[cfg(not(debug_assertions))] assert!`） |
| `eprintln!` | `src/app/startup_timing.rs:88` | 受 `enabled()` 门控的 `[startup-timing]` 冷启动里程碑，**诊断型正式输出** |
| `println!` | `tests/integration/main.rs` 大量 `[metrics]` | CI perf job 抓取指标用，非调试打印 |
| `println!` | `tests/common/golden_fixture.rs:78` | golden 生成器提示，`#[ignore]` 手动运行时才输出 |
| `println!` | `tests/integration/main.rs:645/865` 等 | 是**测试语料字符串内容**，不是打印语句 |
| `evil.exe` / `//host/share` / `javascript:` | `docs/reviews/c1-h1-fix-plan-2026-10-30.md` 等 | 安全测试 payload，非死链 |
| `missing.png` / `images/photo.png` | `tests/data/golden/gfm-full.md` | golden 语料占位，非死链 |
| `#[allow(dead_code)]` | `src/app/frame/actions.rs:195-197` | 有「机制预留、实例为零」的显式理由注释 |

### 2.5 环境噪声说明（事实）

`CARGO_TERM_COLOR=always` 会让 `cargo tree` 输出带 `\e[2m` 等 ANSI 转义。手工比对 `docs/spec/workspace-topology.expected.txt` 时若未先 `--color=never` 或剥离转义，会误报「DIFF」。**实测：两文件均 79 行，版本与依赖项逐条一致**，属比较方法噪声，非真实漂移。CI 已用 `--color=never` + `sed` 规避。

---

## 3. 按类别的发现表

### 3.1 维度一：调试遗留

| # | 文件路径 | 发现 | 证据 | 风险 | 建议动作 | 优先级 | 工作量 |
|---|---|---|---|---|---|---|---|
| D-01 | `lcov.info`（根） | 覆盖率生成物被跟踪，未进 `.gitignore` | `git ls-files` 含 `lcov.info`；`fw=1256956`；`git log --diff-filter=A` 显示自 `338d333` 入库；`.gitignore` 无 `lcov.info` 规则 | 每次 `cargo llvm-cov` 重写导致无意义 diff；仓库膨胀；误当源码审阅 | 删除 + 补 `.gitignore`（`lcov.info`、`*.lcov`） | **P1** | S |
| D-02 | `.claude/artifacts/plans/large-file-refactor.md` | 一次性 agent 重构计划，含本机路径与过期基线 | `Author: ryenlii`、`Last updated: 2026-10-11`、`Baseline commit: 674ce65`；AC-4「1572 passed / 1 failed / 1 ignored」；约 100 条 `file:///d:/data/WorkBuddyCache/workspace/rustmark/...` 死链 | 暴露本机用户名/缓存路径；读者按过期基线误判；死链拉低文档质量 | 删除（历史价值由 `docs/archive/architecture-reviews/` 承接），或移入 `docs/archive/` 并标注日期、剥离本机路径 | **P1** | S |
| D-03 | `tests/presentation/preview_view_test.rs:1737` | 无条件 `eprintln!` 调试 dump | `eprintln!("[dump] texts right>1150: {overflow:?}")`，随后才 `assert!(overflow2.is_empty())` | 每次运行刷屏；release 测试日志噪声 | 改写：删除打印，或移入断言失败信息（`assert!(..., "{overflow:?}")`） | P3 | S |
| D-04 | `tests/presentation/search_alignment_test.rs:143` | 失败分支 `eprintln!("DEBUG ...")` | 仅在 `headers.is_empty()` 时打印 | 命名随意（`DEBUG`）；诊断信息被打印而非附到断言 | 改写：并入 `assert!` 的 message 参数 | P3 | S |
| D-05 | `crates/rustmark-infra/src/infrastructure/highlighting.rs:5-12` | **误报**：增量高亮早已接入生产路径，仅注释与 `#[allow(dead_code)]` 过期 | `syntect_highlighter.rs:296` 的 `start_session()` 返回 `SyntectSession`；`highlight_worker.rs:303/341-354` 按 `DocumentId` 持有会话；`diff` 各函数均被 `incremental.rs` 引用 | 注释误导读者以为「未接入」；`reused_lines()`/`parsed_ratio()`/`last_spans()` 为真死代码 | **已处理**（2026-10-31）：重写模块注释为「已接入」；移除 `mod diff` 的无用 `#[allow(dead_code)]` 与 3 个死方法 | P2 | S |
| D-06 | `docs/pic/PixPin_2026-10-31_09-50-29.png` | 未跟踪调试截图 | `git status` 显示 `??` | 若误 `git add .` 会被提交 | **已处理**（2026-10-31）：移至 `.agent/artifacts/`（已 gitignore），不删除用户素材 | P2 | S |
| D-07 | `docs/archive/crash-reports/crash-20261014135941-29300.txt` | 崩溃报告快照（版本 0.1.0、重构前旧路径） | `版本：0.1.0`；`位置：src\application\workspace\service.rs:468:27`；**无用户路径、无文档内容** | 低：属历史证据；旧路径已失效 | **保留**（历史快照，已自带日期与「不含文档内容」声明）；建议在 `docs/archive/README` 标注「历史快照」 | P3 | S |
| D-08 | `rustmark-logo.miora` + `rustmark-logo_assets/`（12 PNG，8,867,575 B） | 根目录 Logo 设计中间产 | `git ls-files rustmark-logo_assets` = 12；合计 8.66 MB | 仓库体积；非运行必需 | **已处理**（2026-10-31）：`git rm` 移出发布仓（历史可追溯）；`assets/logo/` 保留定稿 SVG/PNG/ico，`build_logo.py` 为几何唯一真源 | P2 | S |
| D-09 | 源码全仓 | `dbg!` / `todo!` / `unimplemented!` / `FIXME` / `TODO` | `rg` 全仓 **0 命中** | 无 | **保留现状**（无问题） | — | — |
| D-10 | 源码全仓 | 注释掉的整段代码 | 启发式 `rg '^//\s*(let|fn|if|match|...)'` **0 命中** | 无 | **保留现状**（无问题） | — | — |

> **判断**：D-03/D-04 属「测试诊断输出」，不是生产调试遗留；改写即可，不必删除用例。

### 3.2 维度二：日志

| # | 文件路径 | 发现 | 证据 | 风险 | 建议动作 | 优先级 | 工作量 |
|---|---|---|---|---|---|---|---|
| L-01 | `src/**`、`crates/*/src/**`（14 文件） | `log::*!` 共 33 处（`app.rs` 2、`scheduler` 6、`registry` 5 等） | `rg 'log::(debug\|trace\|info\|warn\|error)!'` 计数 33 | 低：属 release observability | **保留**；未发现明显无意义或高频刷屏日志 | P3 | — |
| L-02 | `src/app/startup_timing.rs:88` | 受门控的 `eprintln!` | `enabled()` 判定后输出 `[startup-timing]` | 低：门控关闭时静默 | **保留**（诊断型正式输出，非调试打印） | P3 | — |

> **判断**：本维度**无需要清理项**。按用户约束，`debug!`/`trace!` 未默认判删。

### 3.3 维度三：注释与文档

| # | 文件路径 | 发现 | 证据 | 风险 | 建议动作 | 优先级 | 工作量 |
|---|---|---|---|---|---|---|---|
| DOC-01 | `Cargo.toml:198` | 注释仍称「10 MB 未压缩红线（NFR-PERF-01）」，与现行 11 MiB 矛盾 | `rg '10 MB'` → `198:# 以 include_bytes! 编进 exe——这是 10 MB 未压缩红线（NFR-PERF-01）下` | 读者按旧红线误判余量；与 `size_gate.sh` 默认值 11534336 冲突 | 改写为「11 MiB（11,534,336 B）」并注明 2026-10-25 上调 | P2 | S |
| DOC-02 | `docs/guides/dependencies.md:53` | 同上「10 MB 未压缩红线」 | `53:> 编进 exe，是 10 MB 未压缩红线（NFR-PERF-01）下最大的一块可省体积` | 同上 | 改写口径与 `Cargo.toml` 同步 | P2 | S |
| DOC-03 | `docs/README.md` | 文档地图漏收 `reviews/`（2 篇）、`diagrams/`（1 篇）；总数 139 vs 实测 143；`remediation` 表写 4 实际 5（含 README） | 头部声明「顶层只剩本页 + 12 个分类目录」「合计 139 个文件」；实测 docs = 143（archive 72 / pic 15 / stages 11 / adr 13 / guides 9 / spec 5 / remediation 5 / reports 4 / **reviews 2** / plans 2 / design 1 / **diagrams 1** / logs 1 / samples 1 / README 1）；`docs/README.md:32` 引用 `.agent/conventions.md`（未跟踪） | 文档导航失真；新读者找不到审计/图示 | 改写：补 `reviews/`、`diagrams/` 两行；更正总数与 remediation 计数；`.agent/` 引用改为 `docs/spec/architecture-constraints.md` 或删 | P2 | S |
| DOC-04 | `docs/reports/release-compliance-gap-report.md` | 版本/组件数漂移，且被标为「仍是活文档」 | 文中 `0.2.0`、`186 crate`、`388 组件`；实际 `0.2.2` / `395 组件`（`sbom.py` 输出）；`docs/README.md` 标注其为活文档 | 活文档给出过期结论，误导发布决策 | 改写：加「2026-10-22 快照，数据截至当日；当前见 CHANGELOG/SBOM」日期戳，或迁移为 archive | P2 | M |
| DOC-05 | `README.md:44`、`README.md:181`；`CONTRIBUTING.md:4/73/116`；`docs/README.md:32` | 引用 `.agent/` 目录，而 `.agent/` 被 `.gitignore` 忽略、不随仓分发 | `rg '\.agent/'` 在根 Markdown 命中上述行；`docs/spec/architecture-constraints.md:5-7` 已声明「`.agent/` 整目录在 `.gitignore` 里，真源入库会导致拿到代码的人拿不到规则」 | 新克隆者按引用找不到文件；规范可达性缺口 | 改写：指向已入库的 `docs/spec/architecture-constraints.md` / `docs/plans/v1.1-backlog.md`；历史文档（`docs/stages`、`docs/archive`）**保留不改** | P2 | M |
| DOC-06 | `docs/guides/testing.md` | 同一文件并列三条「当前状态」计数冲突 | 2026-10-23 = 2125 passed / 4 ignored；2026-10-22 = 2101 passed；更早快照；均以「当前」表述 | 读者不知哪个是现状 | 改写：只保留最新一条为「当前」，其余改写为带日期的历史记录 | P2 | S |
| DOC-07 | `CHANGELOG.md` | 链接引用缺 `[0.2.1]`、`[0.2.2]` 定义；第 393 行引用旧路径 `docs/metrics.md` | 正文有 `## [0.2.2]`（9）、`## [0.2.1]`（19），但文末只定义 `491:[未发布]`、`492:[0.2.0]`；`393` 行 `docs/metrics.md §2.4`（现为 `docs/spec/metrics.md`） | 渲染时 `[0.2.1]`/`[0.2.2]` 成断链；路径漂移 | 改写：补两条链接定义；第 393 行旧路径修正（属**历史条目**，仅改路径不改叙述） | P3 | S |
| DOC-08 | `PRIVACY.md` | 适用版本仍写 v0.2.0 | `rg 'v0\.2\.0'` 命中适用版本段 | 轻微：读者不确定隐私口径是否覆盖 0.2.2 | 改写为 v0.2.2（若口径未变）；若有变更需补说明 | P3 | S |
| DOC-09 | `docs/guides/debug-guide.md:106` | **误报**：该行是 `.vscode/launch.json` 的 schema `"version": "0.2.0"`，非项目版本 | `rg '0\.2\.0' debug-guide.md` 仅此一处，上下文为 launch.json | 无 | **保留**（不得改为项目版本） | — | — |
| DOC-10 | `docs/logs/问题记录1.txt:1454` | 旧路径 `docs/logs/示例文档.md` | 单行引用 | 极低：历史日志 | **保留**（历史执行记录，不改写） | P3 | — |
| DOC-11 | `docs/archive/**`（72 文件） | 历史快照，多数日期不全在标题 | `git ls-files docs/archive` = 72 | 中：易被误当现状 | **保留**；建议在 `docs/archive/README` 统一标注「历史快照，不代表当前状态」 | P3 | M |
| DOC-12 | `docs/spec/thresholds.baseline.yaml` | 3 处行号漂移 | `check_thresholds.py` 打印 `[drift]`：`window_min_width: 197 → 210`、`window_min_height: 197 → 210`、`env_skip_arch_check: 60 → 82` | 中：行号引用失效；机读基线失真 | 改写：`python tools/check_thresholds.py --fix`（只回写行号，不改值） | P2 | S |

> **事实 vs 判断**：DOC-01/02/12 的「数值/行号过期」是**事实**；「应改写为 11 MiB」是**判断**（以 `scripts/size_gate.sh:26` 默认值 11534336 与 2026-10-25 上调记录为依据）。

### 3.4 维度四：测试

| # | 文件路径 | 发现 | 证据 | 风险 | 建议动作 | 优先级 | 工作量 |
|---|---|---|---|---|---|---|---|
| T-01 | 全仓测试 | `#[ignore]` 共 11 处，**全部带理由字符串** | `rg '#\[ignore'` → 11 命中：golden 生成器 1、性能基准 4+、需外网 1、PoC 基准 2 | 低：与「4 个 ignored」文档口径一致 | **保留**；无「裸 `#[ignore]`」 | — | — |
| T-02 | `tests/common/golden_fixture.rs:75` | `#[ignore = "生成型用例：需要时用 --ignored 手动跑"]` | 同上 | 低 | **保留**（生成器，非门禁用例） | — | — |
| T-03 | `tests/integration/main.rs:848` | `#[ignore = "收益基准：需 release 下显式单独运行"]` | 同上 | 低 | **保留** | — | — |
| T-04 | `tests/presentation/highlight_layout_bench.rs:65` | `#[ignore = "性能基准：需 release 下显式运行"]` | 同上 | 低 | **保留** | — | — |
| T-05 | `tests/infrastructure/asset_loader_test.rs:291` | `#[ignore = "需外网"]`，`295/300` 行 println 为网络降级提示 | 同上 | 低 | **保留** | — | — |
| T-06 | `tests/integration/large_file_test.rs:206` | 注释说明用例「已从 `#[ignore]` 恢复为门禁」 | 注释原文 | 低：已受控 | **保留**（性能门禁，勿删） | — | — |
| T-07 | `tests/infrastructure/{exporter,asset_loader,fs_document_repository,preferences_repository,workspace_repository}_test.rs` 等 6 文件 | 重复的临时目录 helper：`std::env::temp_dir().join(unique.replace(['(', ')', ' '], ""))` | 分别在第 31/21/27/24/26 行附近 | 中：同一逻辑复制 6 份，改一处漏五处 | 待确认：抽到 `tests/common/` 单点 helper（属重构，非发布阻断） | P3 | M |
| T-08 | `tests/{infrastructure,integration,common}` | `sleep` / 硬编码路径 | 已见 `temp_dir()` 用法；未发现跨机器绝对路径 | 低 | **保留**；若后续新增 `sleep` 需注明原因 | P3 | — |
| T-09 | `tests/presentation/preview_view_test.rs:1737`、`search_alignment_test.rs:143` | 调试打印（见 D-03/D-04） | 同 D-03/D-04 | 低 | 同 D-03/D-04 | P3 | S |
| T-10 | `tests/data/golden/**`、`tests/fixtures/**` | golden / fixture 语料 | `git ls-files tests/data/golden` 非空 | 中：误删将失去回归基线 | **保留**（用户明确：不删 golden/fixture/回归用例） | — | — |

> **判断**：测试体系健康。唯一可做优化的是 T-07 的 helper 重复，属可维护性，不阻断发布。

### 3.5 维度五：发布合规

| # | 文件路径 | 发现 | 证据 | 风险 | 建议动作 | 优先级 | 工作量 |
|---|---|---|---|---|---|---|---|
| R-01 | `Cargo.toml` / 成员 manifest | 版本一致：10 包均 0.2.2 | `cargo metadata --no-deps` 全 0.2.2；成员 manifest 无重复 `version`/`license`/`edition`（继承 `[workspace.package]`） | 无 | **保留** | — | — |
| R-02 | `Cargo.toml:137` / `rust-toolchain.toml:11` | MSRV 1.99 与工具链 channel 1.99 同步 | `rust-version = "1.99"`；`channel = "1.99"` | 无 | **保留**（只钉 minor 是既定策略） | — | — |
| R-03 | `CHANGELOG.md` | 有 `[未发布]`、0.2.2、0.2.1、0.2.0 分节；但缺两条链接定义（见 DOC-07） | 见 DOC-07 | 低 | 见 DOC-07 | P3 | S |
| R-04 | `SECURITY.md` | 支持版本表 `v0.2.2（当前）` | 头部表格 | 无 | **保留** | — | — |
| R-05 | `LICENSE-MIT` / `LICENSE-APACHE` / `NOTICE` / `THIRD-PARTY-LICENSES.md` | 双许可文件齐备；第三方清单存在 | `git ls-files` 根级含四文件；README 声明 `MIT OR Apache-2.0` | 无 | **保留**；`THIRD-PARTY-LICENSES.md` 的「186 依赖」表述与 SBOM 395 组件口径不同，见 R-11 | — | — |
| R-06 | `deny.toml` | 许可白名单 + `RUSTSEC-2025-0141`（bincode 经 syntect）登记 + `self_cell` clarify | `[advisories].ignore` 有登记；`[[licenses.clarify]]` 有 `self_cell` | 无 | **保留** | — | — |
| R-07 | `.github/workflows/release.yml` | tag `v*` → 三平台 build → 签名/公证复核 → `SHA256SUMS` → SBOM → `gh release create` | 文件内容 | 无 | **保留** | — | — |
| R-08 | `.github/workflows/ci.yml` | `SIZE_LIMIT_BYTES=11534336`、`SIZE_WARN_BYTES=11010048`；check/perf/coverage/release-dryrun/deny 五 job | 文件内容 | 无 | **保留** | — | — |
| R-09 | `scripts/{size_gate.sh,package-windows.ps1,package-macos.sh,package-linux.sh}` | 体积红线统一 11534336；macOS 字体布局与签名/公证 fail-closed | 各脚本 `packageLimitBytes`/`LIMIT` 均为 11534336；macOS `Contents/Resources/fonts` 要求 | 无 | **保留** | — | — |
| R-10 | `tools/sbom.py` | CycloneDX 1.5，395 组件，确定性（无时间戳） | `release_dryrun` 两次生成逐字节一致 | 无 | **保留** | — | — |
| R-11 | `README.md:9` / `THIRD-PARTY-LICENSES.md` | 「186 个依赖」与 SBOM 395 组件口径不一致 | README 第 9 节「含 186 个依赖的许可分布」；SBOM 395 | 中：读者/合规方困惑 | 待确认：明确「186 = 直接+传递 crate 的许可条目口径，395 = SBOM 组件口径」，在文档注明差异 | P3 | S |
| R-12 | `docs/spec/thresholds.baseline.yaml` | `runtime_consumed: false`，`scan_base_commit: 574b946+c08fd13` | 头部字段 | 无 | **保留**（只读参考，不参与运行时） | — | — |
| R-13 | 根目录 | 无 SBOM 文件入仓（由 CI 构建期生成） | `git ls-files` 无 `sbom*.json` | 无 | **保留**（按设计由 release 生成） | — | — |

> **判断**：发布合规闭环完整。R-11 是**口径说明**问题，不是缺陷。

### 3.6 维度六：冗余与重复

| # | 文件路径 | 发现 | 证据 | 风险 | 建议动作 | 优先级 | 工作量 |
|---|---|---|---|---|---|---|---|
| X-01 | `docs/**` | 图片跨目录无哈希重复 | 对 `docs/pic`、`docs/archive/pic`、`assets/logo/png`、`assets/logo/reference`、`rustmark-logo_assets` 做哈希比对：**无重复** | 无 | **保留** | — | — |
| X-02 | `docs/archive/pic/`（24 文件） | 未被文件名引用的归档图 | 文件名引用扫描：24 个无引用 | 低 | **保留**（`docs/README` 已声明「无引用已归档」策略） | P3 | — |
| X-03 | `docs/pic/`（15 文件） | 多数由 `docs/logs/问题记录1.txt` 按编号引用 | 引用扫描 | 低 | **保留** | P3 | — |
| X-04 | `assets/logo/reference/*`、`rustmark-compact.ico` | 设计参考/未直接引用的图标资源 | 引用扫描 | 低 | 待确认：确认是否被打包脚本按约定使用 | P3 | S |
| X-05 | 测试临时目录 helper | 6 处重复（见 T-07） | 见 T-07 | 中 | 见 T-07 | P3 | M |
| X-06 | `tools/onboard_cost.py` | 长期分析工具，不在 CI 调用链 | 被 `docs/spec/metrics.md` 引用；CI 未调用 | 低：非一次性脚本 | **保留**（接入成本度量基线） | P3 | — |
| X-07 | 依赖 | 未见明显无用依赖 | `cargo metadata`/`cargo tree` 未显示孤立项；精确未用依赖核验未做（见 §2.3） | 中 | 待确认：`cargo-machete` 核验 | P3 | M |
| X-08 | `docs/reports/{threshold-audit,threshold-review}-2026-10-25.md` | 两份同日报告（审计 + 复核） | 文件存在且 `thresholds.baseline.yaml` 同时引用 | 低 | **保留**（审计/复核双签，属治理证据） | — | — |

---

## 4. 逐文件清理清单

> 仅列**需要动作**的文件；无问题文件按目录在附录 A 合并说明。「动作」四选一：保留 / 删除 / 改写 / 待确认。

| 序号 | 文件 | 动作 | 具体改动 | 优先级 | 工作量 |
|---|---|---|---|---|---|
| 1 | `lcov.info` | **删除** | 移除跟踪；补 `.gitignore` 规则 `lcov.info`、`*.lcov` | P1 | S |
| 2 | `.claude/artifacts/plans/large-file-refactor.md` | **删除**或移 `docs/archive/` | 若保留：剥离 `file:///d:/data/...` 死链、标注「2026-10-11 一次性计划」 | P1 | S |
| 3 | `Cargo.toml:198` | **改写** | 「10 MB → 11 MiB（11,534,336 B，2026-10-25 上调）」 | P2 | S |
| 4 | `docs/guides/dependencies.md:53` | **改写** | 同 #3 | P2 | S |
| 5 | `docs/spec/thresholds.baseline.yaml` | **改写** | `python tools/check_thresholds.py --fix` 回写 3 处行号 | P2 | S |
| 6 | `docs/README.md` | **改写** | 补 `reviews/`、`diagrams/` 行；139 → 143；remediation 4 → 5；`.agent/` 引用改为已入库文档 | P2 | S |
| 7 | `docs/reports/release-compliance-gap-report.md` | **改写** | 加 2026-10-22 快照日期戳；更正版本/组件数或标注「截至当日」 | P2 | M |
| 8 | `README.md:44`、`:181` | **改写** | `.agent/tasks/`、`.agent/conventions.md` → `docs/spec/` / `docs/plans/` 已入库路径 | P2 | S |
| 9 | `CONTRIBUTING.md:4`、`:73`、`:116` | **改写** | 同 #8 | P2 | S |
| 10 | `docs/guides/testing.md` | **改写** | 只保留最新「当前状态」，其余加日期 | P2 | S |
| 11 | `docs/pic/PixPin_2026-10-31_09-50-29.png` | **已完成**（决策：移入 gitignore 区） | 移至 `.agent/artifacts/`，保留用户素材；仓库不再出现未跟踪文件 | P2 | S |
| 12 | `crates/rustmark-infra/src/infrastructure/highlighting.rs`（+ `highlighting/diff.rs`、`incremental.rs`） | **已完成**（决策：保留并纠正注释） | 原判「未接入」为误报；已接入 `start_session` → `highlight_worker`。本次移除 3 个死方法与 `mod diff` 的无用 `#[allow(dead_code)]` | P2 | S |
| 13 | `rustmark-logo.miora` / `rustmark-logo_assets/` | **已完成**（决策：移出发布仓） | `git rm` 删除 13 个文件（8.66 MB）；定稿资产仍在 `assets/logo/`；`build_logo.py` 头部注明可追溯 | P2 | S |
| 14 | `tests/presentation/preview_view_test.rs:1737` | **改写** | 删 `[dump]`，或并入断言 message | P3 | S |
| 15 | `tests/presentation/search_alignment_test.rs:143` | **改写** | 删 `DEBUG` 打印，并入断言 message | P3 | S |
| 16 | `CHANGELOG.md`（文末 491-492 附近） | **改写** | 补 `[0.2.1]`、`[0.2.2]` 链接定义；第 393 行旧路径 `docs/metrics.md` → `docs/spec/metrics.md` | P3 | S |
| 17 | `PRIVACY.md` | **改写** | 适用版本 v0.2.0 → v0.2.2 | P3 | S |
| 18 | `docs/guides/debug-guide.md:106` | **保留**（误报） | 该行是 launch.json 的 schema `"version": "0.2.0"`，非项目版本 | — | — |
| 19 | `README.md:9` / `THIRD-PARTY-LICENSES.md` | **改写**（说明性） | 注明 186（许可条目）vs 395（SBOM 组件）口径差异 | P3 | S |
| 20 | 5 个基础设施测试文件临时目录 helper | **已完成** | 抽到 `tests/support/tempdir.rs` 单点 helper（原判 6 个，实测为 5 个重复 `struct TempDir`；另有 2 个文件内联使用 tempdir，非重复结构） | P3 | S |
| 21 | `docs/archive/README`（若存在） | **改写** | 统一标注「历史快照」 | P3 | M |

---

## 5. 建议的清理批次

### 批次 0 — 文档口径统一（低风险，S）
**目标**：消除自相矛盾的红线与版本描述，不改代码。
- #3 `Cargo.toml:198`（仅注释）
- #4 `docs/guides/dependencies.md:53`
- #5 `thresholds.baseline.yaml`（`--fix`）
- #6 `docs/README.md`
- #7 `release-compliance-gap-report.md`
- #8 / #9 `.agent/` 引用（根 Markdown）
- #10 `testing.md`「当前状态」
**验收**：`check_thresholds.py` 无 `[drift]`；`rg '10 MB'` 仅历史文档命中（不新增）。

### 批次 1 — 误提交与一次性产物（中低风险，S–M）
**目标**：缩小仓库、去掉本机路径。
- #1 `lcov.info` + `.gitignore`
- #2 `.claude/.../large-file-refactor.md`
- ~~#11 未跟踪调试截图~~ ✅ 已完成（2026-10-31）：移入 `.agent/artifacts/`
**验收**：`git ls-files` 不再含 `lcov.info`；新克隆可干净构建；`release_dryrun --tag v0.2.2` 仍 16/16。

### 批次 2 — 测试诊断与 CHANGELOG 收尾（低风险，S）
- #14 / #15 测试调试打印
- #16 CHANGELOG 链接与旧路径
- #17 / #18 / #19 版本与口径微调
**验收**：`cargo test --workspace --all-targets` 不因日志变化失败；CHANGELOG 渲染无断链。

### 批次 3 — 需产品/维护者决策（中风险，M–L）
- ~~#12 `highlighting` PoC（接入 or 移除）~~ ✅ 已完成（2026-10-31）：实为「已接入、注释过期」，保留实现并清理死代码
- ~~#13 Logo 设计中间产归属~~ ✅ 已完成（2026-10-31）：移出发布仓
- ~~#20 测试 helper 抽取~~ ✅ 已完成（2026-10-31）：`tests/support/tempdir.rs`，5 处重复 `TempDir` 收口
- `docs/archive` 统一「历史快照」标注
**验收**：决策记录写入本文档「待确认问题」或后续 ADR；移除项不得影响 `cargo build --release` 与 `size_gate`。

> **顺序理由**：先改「说法」（批次 0，零运行时风险）→ 再删「不该在仓里的东西」（批次 1，需 git 操作）→ 再做小修（批次 2）→ 最后做结构决策（批次 3）。

---

## 6. 「必须保留」清单

以下文件/目录**不得在清理中删除或削弱**：

| 类别 | 具体 | 理由 |
|---|---|---|
| 发布脚本 | `scripts/size_gate.sh`、`scripts/package-windows.ps1`、`scripts/package-macos.sh`、`scripts/package-linux.sh`、`scripts/*`（5 个） | 体积门禁、三平台打包、签名/公证 fail-closed |
| 发布工具 | `tools/release_dryrun.py`、`tools/sbom.py`、`tools/gen_release_notes.py`、`tools/check_thresholds.py`、`tools/coverage_gate.py`、`tools/arch_check.rs`、`tools/gen_gb18030_ranges.py`、`tools/onboard_cost.py` | 离线演练、SBOM、Notes、阈值、覆盖率、架构校验、编码表生成、接入成本度量 |
| 构建校验 | `build.rs`、`tools/arch_check.rs`、`SKIP_ARCH_CHECK` 逃生口 | 构建期分层校验 |
| Fixture / golden | `tests/data/golden/**`、tests 内 fixture、`tests/common/golden_fixture.rs` | 回归基线；用户明确不删 |
| 性能基线 | `docs/spec/thresholds.baseline.yaml`（含 `docs/reports/threshold-{audit,review}-2026-10-25.md`）、tests 内 `[metrics]` 输出、`#[ignore]` 性能基准 | 性能门禁与可复现口径 |
| 历史 CHANGELOG | `CHANGELOG.md` 全部历史条目（0.2.0/0.2.1/0.2.2 及更早） | 发布历史不可改写；仅改死链 |
| 历史快照 | `docs/archive/**`（72 文件，含 crash-reports、stage-reports、architecture-reviews、tooling） | 审计/阶段证据；标注而非删除 |
| 许可证 | `LICENSE-MIT`、`LICENSE-APACHE`、`NOTICE`、`THIRD-PARTY-LICENSES.md` | 双许可分发要求 |
| 随包字体许可 | `assets/fonts/Fonts-LICENSE.txt`、`OFL-1.1.txt`、`UFL.txt`、`OFL.txt`、`Hack-Regular.txt`、`emoji-icon-font-mit-license.txt`、`DejaVuMathTeXGyre-LICENSE.txt` | macOS/Linux 打包复制到 `Contents/Resources/` / `usr/share/doc/` |
| 合规配置 | `deny.toml`、`rust-toolchain.toml`、`.rustfmt.toml`、`.gitignore` | 许可/工具链/格式化门禁 |
| CI | `.github/workflows/ci.yml`、`.github/workflows/release.yml` | 门禁与发布流水线 |
| 编码表 | `gb18030_ranges.bin`、`gbk_table.bin`、`words.txt`（按实际路径） | 运行时数据，随包分发 |
| 架构真源 | `docs/spec/architecture-constraints.md`、`docs/adr/**`（13 篇）、`docs/spec/workspace-topology.expected.txt` | 已入库规则真源与拓扑快照 |
| 需求基线 | `REQUIREMENTS.md` | 需求追溯 |
| 版本真源 | `Cargo.toml` 的 `[workspace.package]`、`Cargo.lock` | 单一真源 |

> **不发明新要求**：本项目未强制源码文件头许可证声明，审计**不提出**新增许可证头建议。

---

## 7. 验证计划

按批次执行后逐项验证（只读命令，不改仓库）：

| 阶段 | 命令 | 通过标准 |
|---|---|---|
| 批次 0 后 | `python tools/check_thresholds.py` | 无 `[drift]` 输出，exit 0 |
| 批次 0 后 | `rg -n '10 MB\|10MB' src crates docs/guides Cargo.toml` | 仅历史文档（`docs/archive`、`CHANGELOG` 历史条目）命中 |
| 批次 1 后 | `git ls-files \| rg 'lcov\|large-file-refactor'` | 0 命中 |
| 批次 1 后 | `python tools/release_dryrun.py --tag v0.2.2` | 16/16 PASS |
| 批次 1 后 | 干净克隆 `cargo build --release`（或 CI check job） | 通过；`size_gate.sh` 不越线 |
| 批次 2 后 | `cargo test --workspace --all-targets`（或引用 CI check job） | 与当前基线一致，无新增失败 |
| 批次 2 后 | `cargo fmt --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings` | 通过 |
| 批次 2 后 | CHANGELOG 链接检查（`rg '^\[0\.2\.[12]\]'`） | 两条定义存在 |
| 批次 3 后 | `cargo build --release --bin rustmark` + `./scripts/size_gate.sh` | 产物 ≤ 11,534,336 B |
| 全批次后 | CI 三平台 `check` + `perf` + `coverage` + `release-dryrun` + `deny` | 全绿 |

> 全量测试不在本地强制复跑；以 CI `check` job（三平台）与 `perf` job（Linux release）结果为准。本节所有标准均可由现有脚本自动判定，不需要新写校验器。

---

## 8. 待确认问题

1. ~~**`highlighting` PoC 去留**~~ **已裁决（2026-10-31）**：经复核，`SyntectSession` 已通过 `start_session`（`syntect_highlighter.rs:296`）与 `highlight_worker` 的会话表接入生产，原「未接入」判断为误报。决策为**保留实现**，仅清理过期注释与 `reused_lines()`/`parsed_ratio()`/`last_spans()` 三个死方法。
2. ~~**`rustmark-logo.miora` 与 `rustmark-logo_assets/`（12 PNG，8.66 MB）**~~ **已裁决（2026-10-31）**：属 AI 概念稿中间产、非构建输入（`build_logo.py` 的几何为唯一真源），决策为**移出发布仓**（`git rm`，历史可追溯）；不放入 `assets/logo/`。
3. **`lcov.info` 处理方式**：仅 `.gitignore` + `git rm --cached`，还是连历史一并清理？历史清理涉及重写，需单独决策。
4. **`.claude/` 是否整体忽略**：目前仅 1 个文件被跟踪。是否把 `.claude/` 加入 `.gitignore`（与 `.agent/`、`.workbuddy/` 同策略）？
5. ~~**`186` vs `395` 口径**~~ **已处理（批次 2）**：`THIRD-PARTY-LICENSES.md` §2 与 README 已注明两种口径及其差异。
6. ~~**`docs/reports/release-compliance-gap-report.md` 定位**~~ **已处理（批次 0）**：继续作为活文档维护 §6，§0–§5 加「2026-10-22 快照」日期戳并指向现行口径。
7. **`.agent/` 引用策略**：根文档改指向已入库真源后，是否需要在 README 保留一段「`.agent/` 为本地执行记录、不入库」的说明？
8. **测试临时目录 helper 抽取**：6 文件重复，是否接受一次性重构（涉及多个测试目标，需逐一回归）？
9. **`docs/archive/pic/` 无引用图 24 个**：确认「无引用已归档」策略仍成立，无需补引用或删除。
10. **`rustmark-compact.ico` / `assets/logo/reference/*`**：确认打包脚本或 `build_logo.py` 是否仍按约定使用。

---

## 附录 A：tracked 文件目录覆盖表

> 实测 `git ls-files` 共 783 个文件。下表覆盖所有 tracked 目录；「审计结论」为按目录合并判断。

| 目录 | 文件数 | 审计结论 |
|---|---|---|
| `src/` | 92 | **健康**：无 `dbg!`/`todo!`/注释代码；`eprintln!` 仅 `startup_timing.rs:88`（门控诊断，保留）；无临时路径 |
| `crates/rustmark-actions/src/` | — | 健康（合并计入 crates 291） |
| `crates/rustmark-assembly/src/` | — | 健康 |
| `crates/rustmark-core/src/` | — | 健康；`equivalence.rs:286-289` 测试指标输出属有意保留 |
| `crates/rustmark-icons/src/` | — | 健康 |
| `crates/rustmark-infra/src/` | — | **1 项待确认**：`highlighting` PoC（`diff.rs`/`incremental.rs`） |
| `crates/rustmark-ports/src/` | — | 健康 |
| `crates/rustmark-render/src/` | — | 健康 |
| `crates/rustmark-shared/src/` | — | 健康 |
| `crates/rustmark-ui/src/` | — | 健康 |
| `crates/*/src/` 合计 | 291 | 仅 infra 的 PoC 模块待确认 |
| `tests/` | 154 | **基本健康**：`#[ignore]` 全带理由；2 处测试调试打印（P3）；6 文件 helper 重复（P3）；golden/fixture 保留 |
| `docs/archive/` | 72 | **保留**（历史快照）；建议统一标注 |
| `docs/pic/` | 15 | 保留；含 1 未跟踪调试截图（P2） |
| `docs/adr/` | 13 | 保留（架构决策） |
| `docs/stages/` | 11 | 保留（阶段历史）；`.agent/` 引用不改写 |
| `docs/guides/` | 9 | **2 处改写**：`dependencies.md:53`（10 MB）、`testing.md`（当前状态冲突）；`debug-guide.md:106` 版本号（P3） |
| `docs/spec/` | 5 | **1 处改写**：`thresholds.baseline.yaml` 行号漂移；其余（`architecture-constraints.md` 等）为真源，保留 |
| `docs/remediation/` | 5 | 保留；`docs/README.md` 计数需更正 |
| `docs/reports/` | 4 | **1 处改写**：`release-compliance-gap-report.md` 漂移；`threshold-{audit,review}` 保留 |
| `docs/reviews/` | 2 | 保留；**文档地图漏收**（DOC-03） |
| `docs/plans/` | 2 | 保留 |
| `docs/design/` | 1 | 保留 |
| `docs/diagrams/` | 1 | 保留；**文档地图漏收**（DOC-03） |
| `docs/logs/` | 1 | 保留（历史日志，不改写） |
| `docs/samples/` | 1 | 保留 |
| `docs/README.md` | 1 | **改写**（DOC-03） |
| `docs/` 合计 | 143 | 见上 |
| `assets/fonts/` | — | **必须保留**（字体 + 许可证全文） |
| `assets/logo/` | — | 保留；`reference/` 待确认引用 |
| `assets/` 合计 | 50 | 健康；无运行必需资源缺失 |
| `.github/workflows/ci.yml`、`release.yml` | 6 | **必须保留** |
| `scripts/` | 5 | **必须保留**（体积/打包/签名） |
| `tools/` | 8 | **必须保留**；`onboard_cost.py` 非 CI 但保留 |
| `rustmark-logo_assets/` | 12 | **待确认**（设计中间产，8.66 MB） |
| 根级 Markdown | — | `README.md`（2 处 `.agent/` 改写 + 1 处口径）、`CHANGELOG.md`（链接/路径 P3）、`SECURITY.md`/`PRIVACY.md`（版本微调）、`CONTRIBUTING.md`（3 处 `.agent/`）、`CODE_OF_CONDUCT.md`/`REQUIREMENTS.md` 健康 |
| 根级构建/发布 | — | `Cargo.toml`（1 处注释改写）、`Cargo.lock`/`build.rs`/`deny.toml`/`rust-toolchain.toml`/`.rustfmt.toml`/`.gitignore` 健康（`.gitignore` 需补 `lcov.info`） |
| 根级许可证 | — | `LICENSE-MIT`/`LICENSE-APACHE`/`NOTICE`/`THIRD-PARTY-LICENSES.md` **必须保留** |
| 根级误提交 | 1 | `lcov.info`（P1 删除） |
| 根级设计中间产 | 1 | `rustmark-logo.miora`（待确认） |
| `.claude/` | 1 | `artifacts/plans/large-file-refactor.md`（P1 删除/归档） |
| 根级与其他合计 | 22 | 见上 |

---

## 附录 B：证据命令与输出摘要（事实）

```text
# 文件总量与目录分布
$ git ls-files | wc -l
783

# 调试宏与占位实现（0 命中）
$ rg 'dbg!|todo!|unimplemented!|FIXME|TODO' src crates tests build.rs
(no output)

# 注释掉代码启发式（0 命中）
$ rg '^//\s*(let|fn|if|match|for|while|use|return|self\.|struct|impl|pub |#\[)' src crates
(no output)

# #[ignore]（11 处，全部带理由）
$ rg -n '#\[ignore' tests crates
tests/common/golden_fixture.rs:75  #[ignore = "生成型用例：需要时用 --ignored 手动跑"]
...

# log 宏（33 处 / 14 文件）
$ rg -c 'log::(debug|trace|info|warn|error)!' src crates
app.rs:2
scheduler:6
registry:5
...

# 版本与许可元数据
$ cargo metadata --no-deps --format-version 1
10 packages, all version=0.2.2, license="MIT OR Apache-2.0", publish=[]

# 发布演练（目标版本）
$ python tools/release_dryrun.py --tag v0.2.2
16/16 PASS   (含 SBOM 395 组件两次生成逐字节一致、无私钥入仓、回滚三步、macOS 字体布局)

# 发布演练（旧版本，预期性失败）
$ python tools/release_dryrun.py --tag v0.2.1
14/16 PASS, 2 FAIL  (版本守卫 0.2.1 != 0.2.2；SBOM 根组件 0.2.2 != 0.2.1)

# 阈值基线
$ python tools/check_thresholds.py
OK: 基线与源码一致
[drift] window_min_width: 197 -> 210
[drift] window_min_height: 197 -> 210
[drift] env_skip_arch_check: 60 -> 82
121 thresholds / 9 modules; 110 numeric verified, 11 non-numeric skipped

# 工作区状态（审计时点）
$ git status
13 modified (CHANGELOG/Cargo.lock/Cargo.toml/README/SECURITY/...)
1 untracked  docs/pic/PixPin_2026-10-31_09-50-29.png
last commit c0784d2 fix(release): macOS 打包脚本全角标点致 bash 3.2 unbound variable
```

---

## 附录 C：版本与事实快照

| 项 | 值 | 来源 |
|---|---|---|
| 当前版本 | `0.2.2` | `Cargo.toml [workspace.package]`；10 包一致 |
| 工具链 | `1.99`（只钉 minor） | `rust-toolchain.toml:11` |
| MSRV | `1.99` | `Cargo.toml:137` |
| 许可 | `MIT OR Apache-2.0`，`publish = false` | `[workspace.package]` |
| 体积红线 | 11,534,336 B（11 MiB） | `scripts/size_gate.sh:26`；`ci.yml:62` |
| 体积预警 | 11,010,048 B（10.5 MiB） | `scripts/size_gate.sh:27`；`ci.yml:63` |
| SBOM 组件数 | 395 | `tools/sbom.py` 输出 |
| `Cargo.lock` `name =` 条目 | 396（含 workspace 成员） | `Cargo.lock` |
| 阈值条目 | 121（9 模块） | `check_thresholds.py` |
| `docs` 文件数 | 143 | `git ls-files docs` |
| tracked 文件总数 | 783 | `git ls-files` |
| 发布演练（v0.2.2） | 16/16 PASS | `release_dryrun.py` |

> 本文档为只读审计结论。执行批次 0–3 时请逐条回到 §4 清单，并在每批完成后按 §7 验证。若决策项（§8）改变结论，请更新本文档对应条目而非另立文档。
