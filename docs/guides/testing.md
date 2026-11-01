# 测试体系与覆盖率门禁（S9-01 / S9-02）

> 面向贡献者。测试怎么组织、怎么跑、覆盖率怎么算、什么会被排除。

---

## 1. 测试金字塔与目录规范

测试按层分目录，每层一个 `main.rs` 入口，`Cargo.toml` 里**显式声明**
`[[test]]` 目标（删入口文件 = 有意识地移除该层测试）：

| 目录 | 内容 | 依赖形态 |
|---|---|---|
| `tests/shared/` | shared 层单测（几何、错误、ID） | 纯内存 |
| `tests/domain/` | 纯领域单测（聚合、值对象、撤销栈、解析模型） | 纯内存 |
| `tests/application/` | 用例测试（fake 仓储注入） | fake |
| `tests/infrastructure/` | 适配器与 golden 测试（文件系统 tempdir、Rope、编码、导出器、属性测试） | tempdir / 真实实现 |
| `tests/integration/` | 跨层 + 真实文件系统（tempdir） | 真实 IO |
| `tests/e2e/` | 可自动化端到端（headless 驱动真实 `update` 帧循环） | headless |
| `tests/presentation/` | 表现层（egui CPU 布局） | headless |
| `tests/app/` | 组合根与调度（bootstrap / 崩溃日志 / 对话框） | fake 对话框 |

顶层另有 `tests/startup_smoke.rs`（配置型启动缺陷守卫）、
`tests/arch_violation_fixture.rs`（架构校验器自验）与
`tests/field_visibility_guard_test.rs`（core 字段级 `pub` 白名单守卫，
D-3 阶段 4b-5：集合相等双向断言钉死 core 的 pub 字段面）。

### 运行约定

- 全量：`cargo test --workspace --all-targets`
  （`--workspace` **必带**：workspace 化（D-3 阶段 4a，2026-10-23）后，不带时
  `crates/rustmark-core` 的测试目标会静默缺席）
  （V11-13 已裁定：`large_file_test.rs` 里的**有界性能断言**只在 release 生效，
  debug 下这些用例仅打印 `[metrics]` 指标、不做门禁，因此常规回归无需
  `--skip`，也不会因 debug 吞吐偏低而误判。release 门禁由 CI 的 `perf` job
  以 `cargo test --release --test integration large` 常态化执行）。
- 单层：`cargo test --test <层名>`，如 `cargo test --test domain`。
- 文档示例测试：`cargo test --workspace --doc`（CI `check` job 同款，2026-10-24 起
  常态化）。**`--all-targets` 不含 doctest**——它不是编译目标，而是从文档示例
  另起的测试目标；漏跑会让示例代码静默腐化（教训：html_sanitize 4a 迁移后的
  E0433 在此盲区数日，本地与 CI 全绿）。
- 全量**测试执行**耗时约 37s（2026-10-14 实测，热构建；release 构建另计）；连续 3 次运行结果一致
  是 S9-01 的稳定性验收，见任务清单状态段。

> **最近基线（2026-10-23 实测，D-3 阶段 4b-5 字段级收紧后）**：`cargo test --workspace --all-targets` 为 **2125 passed / 0 failed / 4 ignored**。新增测试目标 `field_visibility_guard`（`tests/field_visibility_guard_test.rs`，core 字段级 `pub` 白名单 + 3 条匹配逻辑注入用例）计 4 条 passed。**本文件不随每次提交更新**：当前计数以 CI `check` job 输出为准，后续新增用例见 `CHANGELOG.md`。
>
> **历史记录（2026-10-23 实测，D-3 阶段 4a workspace 化后）**：`cargo test --workspace --all-targets` 为 **2125 passed / 0 failed**。拆分后 `crates/rustmark-core` 是独立测试目标（core 单测 357 passed），**不带 `--workspace` 时会静默缺席**——这是下方 2026-10-22 及更早快照的口径差异来源（当时为单 crate，命令无 `--workspace`）。
>
> **历史记录（2026-10-22 实测）**：`cargo test --all-targets` 为 **2101 passed / 0 failed / 4 ignored**，全量回归约 75s（热构建）。相对 2026-10-21 基线 2099，净增 **+2**（`tests/infrastructure/workspace_repository_test.rs` 的路径形态两条：前缀不变量 + 「canonical 形态不同的根」回归，后者在能构造差异的平台上才有牙齿，构造不出则跳过）。
>
> **历史记录（2026-10-21 实测）**：`cargo test --all-targets` 为 **2099 passed / 0 failed / 4 ignored**，全量回归约 44s（热构建）。相对 2026-10-21 上午基线 2090，净增 **+9**，其中 R-06「切标签保留光标」+3（`tests/app/tab_cursor_test.rs` 三层判据：应用层 per_doc_cursor 表记/取、pending_cursor 排入并被渲染帧消费、真帧读 egui `TextEditState` 脱字符落在恢复位置）、R-17「widgets 收口」+4（`tests/presentation/search_alignment_test.rs` 搜索分组头对齐集成 1 + `widgets/row.rs` 的 `paint_row_background` 单测 3）、R-20「fonts.rs 拆分」+1（`src/infrastructure/fonts/mod.rs` 的 `discovery_prefers_project_fonts_dir_over_system_candidates` 注入式单测）、V11-55「shared 内部环破除」+1（`src/shared/dialog.rs` 的 `default_filters_derives_from_the_whitelist` 对拍用例）。**R-11 整体完成 + R-12 机械规则上线 + R-06 切标签保留光标 + R-17 widgets 收口落地 + R-20 fonts 可测拆分落地 + V11-55 shared 环破除**。
> 4 个 `ignored` 分布于 4 个测试目标，均为**有意忽略**：2 个性能基准（`#[ignore = "…需 release 下显式运行"]`）、1 个生成型 golden（`"生成型用例：需要时用 --ignored 手动跑"`）、1 个需外网（`"需外网"`）。
> 2026-10-10 深夜曾实测 1519 passed / 7 failed，7 项分三类：① `presentation` 的 4 条 `top_menu_test`——工具栏「格式」组已**平铺为按钮**，测试仍按「分组下拉标题」定位，属**期望滞后于代码**；② `integration` 的 2 条语料快照——解析器新增 `header_spans`/`rows_spans`/`depth`/`ordinal`/`spans` 字段后 `{:?}` 快照文本已变，`tests/data/golden/*.expected` 未重生成，属**期望滞后**；③ `infrastructure` 的 `real_export_writes_a_real_pdf_to_the_chosen_path`——无头 Edge 在沙箱环境不产出 PDF，属**环境限制**。
> **三类均已关闭**（V11-14 / V11-15，详见 `docs/plans/v1.1-backlog.md`）：①②随 2026-10-11~14 批次修复（测试改按平铺按钮定位、golden 重生成复核），③经 2026-10-14 复测确认在当前环境通过。
> 本节其余数字（覆盖率、25s 耗时等）为 2026-10-09 快照，未复测。

---

## 2. 覆盖率门禁

工具：**cargo-llvm-cov**（LLVM source-based，Rust 官方推荐路径；
tarpaulin 仅 Linux 且对 wgpu 依赖树支持差，故不选）。

```bash
# 本地生成报告
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
cargo llvm-cov --workspace --all-targets --lcov --output-path lcov.info --

# 门禁判定（CI 同款）
python tools/coverage_gate.py lcov.info            # 整体 ≥70%，领域 ≥90%
```

### 门限与缓冲

| 指标 | 门限 | 缓冲 |
|---|---|---|
| 整体行覆盖 | ≥ 70% | 落入门限 2% 内只 WARN 不红 |
| `src/domain/**` 行覆盖 | ≥ 90% | 同上 |

缓冲的依据（阶段 9 风险表）：合法重构可能让覆盖率小幅回落；
缓冲区 WARN 是「该补测试了」的信号，硬红留给**真正的**倒退。
重大重构期间可临时下调并必须在 PR 说明、事后恢复。

### 为什么领域层 90%

领域层是正确性核心（编码无损、撤销语义、文档状态机都在这），
缺陷代价最高且**完全可以**纯内存测——90% 是可达到的诚实门槛，
不是凑数。表现层不设高门槛：UI 逻辑靠阶段 07 的手工清单
（`docs/07-*.md`）与 headless 行为测试，像素级效果属真机目测。

### 排除清单（及理由）

| 排除项 | 理由 |
|---|---|
| `src/bin/**` | 架构校验工具自身，不随应用分发；其正确性由 `tests/arch_violation_fixture.rs` 直接验证 |
| `#[cfg(test)]` 代码 | 生产代码的覆盖率统计天然不含它（lcov 报告只含生产构建） |
| derive 生成代码 | `thiserror`/`serde` 的宏展开无法手测，排除后统计的是**我们写的**逻辑 |

上述排除由 `tools/coverage_gate.py` 的 `is_excluded` 实现（排除 `src/bin/`；
后两类 llvm-cov / lcov 本就不计入）。排除后领域层有效覆盖率仍 ≥ 90%
（S9-02 验收项，见任务状态段实测数字）。

### CI 集成

`.github/workflows/ci.yml` 的 `coverage` job：装 llvm-cov → 跑报告 →
`coverage_gate.py` 判定 → 摘要写 `$GITHUB_STEP_SUMMARY`。
人为删若干领域测试可验证门禁**确实会红**（S9-02 验收，见状态段演练记录）。

---

## 3. 平台专属用例（unix-only）

`#[cfg(unix)]` 的用例在 Windows 上**根本不进编译**——不是被跳过，是那几行代码
压根不存在，`--include-ignored` 也选不中它们。本仓库开发机是 Windows，所以
「这 5 条到底能不能过」在本地拿不到任何证据，证据只能来自 CI 的 macOS / Linux
runner。本节把这根证据链固定下来：**哪 5 条、怎么挂进 lib、CI 哪一行跑、本机怎么按需复跑。**

### 3.1 清单

| 文件:行 | 用例 | 为何 `#[cfg(unix)]` |
|---|---|---|
| `crates/rustmark-infra/src/infrastructure/persistence/fs_document_repository/tests.rs:206` | `sync_parent_dir_survives_missing_directory` | 被测的 `sync_parent_dir` 只在 unix 有实体（`persistence/atomic_file.rs:113`），非 unix 是空实现（`:134`）——「目录打不开 → 走 `log::warn!`」这条分支在 Windows 上不存在 |
| 同上 `:220` | `sync_parent_dir_on_real_directory_is_quiet` | 同上。目录 fsync 是 POSIX 语义（`File::open(dir)` + `sync_all`）；Windows 的持久性是 `MoveFileEx`，没有可对应的调用 |
| `.../asset_loader/tests.rs:163` | `symlink_escaping_base_is_rejected` | 需要 `std::os::unix::fs::symlink`。Windows 建符号链接要管理员或开发者模式，在 CI 上不可靠 |
| 同上 `:184` | `symlinked_directory_escaping_base_is_rejected` | 同上（目录符号链接越界） |
| 同上 `:206` | `symlink_staying_inside_base_is_allowed` | 同上，反向用例：指向 base **内部**的链接不得误伤 |

> 五条都**没有** `#[ignore]`：它们是「这条平台上不适用」，不是「暂时不跑」。
> 别把两者混为一谈——`#[ignore]` 的用例在任何平台都不会给信号，`#[cfg(unix)]`
> 的用例在 unix 上是**必须绿**的正常回归项。

### 3.2 它们怎么进 lib 单测（以及一个容易误判的点）

两个 `tests.rs` 都以 `#[cfg(test)] mod tests;` 挂在父模块尾部
（`fs_document_repository.rs:358`、`asset_loader.rs:539`），父模块 `persistence.rs`
的 `pub mod asset_loader;` / `pub mod fs_document_repository;` 没有 feature 门控。
因此它们就是 `rustmark-infra` 的**lib 单测**，不是某个被关掉的路径下的孤儿。

**与 `remote-images` feature 无关**——这点值得写下来，因为 `asset_loader` 确实有
一大批 feature 门控用例（文件后半段「#7 远程路径的行为级用例」那批）。
3 条 symlink 用例调用的 `AssetLoader::ensure_within_base_real`
（`asset_loader.rs:376`）上**没有** `#[cfg(feature = …)]`：它只做本地路径
canonicalize 比对，不发网络请求。所以无论 feature 开还是关，这 3 条都已编译、
都会执行。

**另一点容易搞反**：`sync_parent_dir` 是「unix 实体 + 非 unix 空实现」的双定义，
所以那 2 条**在 Windows 上编译得过**——加 `#[cfg(unix)]` 的理由是**语义**
（空实现里没有 warn 分支可测），不是编译错误。这意味着「忘了门控」的后果是
「静默测了个空函数」，比编译失败更隐蔽。

### 3.3 覆盖它们的 CI

`check` job（`.github/workflows/ci.yml:73`）跑三平台矩阵（`:83`）：

```yaml
os: [windows-latest, macos-latest, ubuntu-24.04]
```

- **`macos-latest` 与 `ubuntu-24.04` 两行是 unix**，这 5 条在此编译并执行；
- `windows-latest` 那行 `cfg(unix)` 为假，5 条不进编译——该行的测试总数天然少于
  另两行，这是**预期差异**，不是漏跑。

执行步骤是 `cargo test (all targets)`（`:197`），命令原文（`:201`）：

```bash
cargo test --workspace --all-targets --no-fail-fast
```

三个 flag 各自不可省，理由与本文件 §1「运行约定」同源：

- `--workspace`：显式覆盖全部成员。§1「运行约定」记的那次事故（不带它时
  `rustmark-core` 的测试静默缺席，D-3 阶段 4a，2026-10-23）现已由 `default-members`
  列全 + `tests/workspace_topology_test.rs` 的
  `default_members_cover_every_workspace_member` 断言关闭；CI 仍显式写
  `--workspace`，是不让「测试会不会跑」继续依赖另一处配置的正确性。
- `--all-targets`：含 `--lib`，cargo 会为 `rustmark-infra` 的 lib 目标另起一个
  测试 harness（输出行形如 `Running unittests src/lib.rs`）——这 5 条就在那个
  harness 里。只跑 `--test <层名>` 不会碰到它们。
- `--no-fail-fast`：一个目标红也跑完其余，一次拿到全部失败面。

**核验结论（2026-11-01 只读核对 `a3994d4`）：覆盖成立，无缺口。** 三条判据：
① 5 条均无 `#[ignore]`；② 所在模块与其依赖函数均无 feature 门控（见 §3.2）；
③ `--all-targets` 的 `--lib` 覆盖 infra 的 lib 单测目标（本地 Windows 实测该
目标 `98 passed`，输出里能看到 `Running unittests src/lib.rs (…rustmark_infra…)`）。
顺手记一个事实：infra 里另有 `#[cfg(target_os = "windows")]` 的导出用例，
所以 unix 与 Windows 两侧的用例**数目不能拿来做对拍。**

### 3.4 本地怎么按需复跑

Windows 开发机上**做不到**——`#[cfg(unix)]` 不进编译，本机 WSL 被安全策略禁用、
无 Docker（同 `troubleshooting.md` §6 的说明）。能做的只有：

```bash
# 在 Linux / macOS 上按名字取这 5 条
cargo test -p rustmark-infra sync_parent_dir
cargo test -p rustmark-infra symlink_

# 或整个 lib 目标（含上述 5 条与其余 infra 单测）
cargo test -p rustmark-infra --lib
```

**改了这 5 条附近的生产代码时**：本地红灯**不会**提醒你。`sync_parent_dir` 在
Windows 走空实现，符号链接形态也不会被构造。此时判断依据只有两条：CI 的
macos / ubuntu 两行，或在一台 unix 机器上跑上面两条命令。反过来说，如果你在
本地看到这 5 条中的某条红了，那说明你在 unix 上——这不是环境异常。
