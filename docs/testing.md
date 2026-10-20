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

顶层另有 `tests/startup_smoke.rs`（配置型启动缺陷守卫）与
`tests/arch_violation_fixture.rs`（架构校验器自验）。

### 运行约定

- 全量：`cargo test --all-targets`
  （V11-13 已裁定：`large_file_test.rs` 里的**有界性能断言**只在 release 生效，
  debug 下这些用例仅打印 `[metrics]` 指标、不做门禁，因此常规回归无需
  `--skip`，也不会因 debug 吞吐偏低而误判。release 门禁由 CI 的 `perf` job
  以 `cargo test --release --test integration large` 常态化执行）。
- 单层：`cargo test --test <层名>`，如 `cargo test --test domain`。
- 全量**测试执行**耗时约 37s（2026-10-14 实测，热构建；release 构建另计）；连续 3 次运行结果一致
  是 S9-01 的稳定性验收，见任务清单状态段。

> **当前状态（2026-10-20 实测）**：`cargo test --all-targets` 为 **1909 passed / 0 failed / 4 ignored**，全量回归约 44s（热构建）。
> 4 个 `ignored` 分布于 4 个测试目标，均为**有意忽略**：2 个性能基准（`#[ignore = "…需 release 下显式运行"]`）、1 个生成型 golden（`"生成型用例：需要时用 --ignored 手动跑"`）、1 个需外网（`"需外网"`）。
> 2026-10-10 深夜曾实测 1519 passed / 7 failed，7 项分三类：① `presentation` 的 4 条 `top_menu_test`——工具栏「格式」组已**平铺为按钮**，测试仍按「分组下拉标题」定位，属**期望滞后于代码**；② `integration` 的 2 条语料快照——解析器新增 `header_spans`/`rows_spans`/`depth`/`ordinal`/`spans` 字段后 `{:?}` 快照文本已变，`tests/data/golden/*.expected` 未重生成，属**期望滞后**；③ `infrastructure` 的 `real_export_writes_a_real_pdf_to_the_chosen_path`——无头 Edge 在沙箱环境不产出 PDF，属**环境限制**。
> **三类均已关闭**（V11-14 / V11-15，详见 `docs/v1.1-backlog.md`）：①②随 2026-10-11~14 批次修复（测试改按平铺按钮定位、golden 重生成复核），③经 2026-10-14 复测确认在当前环境通过。
> 本节其余数字（覆盖率、25s 耗时等）为 2026-10-09 快照，未复测。

---

## 2. 覆盖率门禁

工具：**cargo-llvm-cov**（LLVM source-based，Rust 官方推荐路径；
tarpaulin 仅 Linux 且对 wgpu 依赖树支持差，故不选）。

```bash
# 本地生成报告
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
cargo llvm-cov --all-targets --lcov --output-path lcov.info --

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
