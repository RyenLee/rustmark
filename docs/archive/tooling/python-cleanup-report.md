# Python 工具清理报告

> 执行日期：2026-10-22　范围：仓库根目录下全部 `*.py`（排除 `target/`）
> 结果：**26 → 5**，删除 21 个一次性脚本；备份目录 `D:\Dev\Cache\Temp\rustmark-py-backup-20261022\tools\`
> 用途：记录删除依据、保留理由与验证结论，供日后复核或恢复。

---

## 0. 结论

| 项 | 数 | 说明 |
|---|---|---|
| 清理前 | 26 | `tools/` 25 个 + `assets/logo/` 1 个 |
| 删除 | 21 | 20 个 `mut_*.py` 变异验证脚本 + 1 个性能基准脚本 |
| 保留 | 5 | 被 CI / 发布流程调用，或为随包数据与资产的唯一再生成路径 |

判定原则：**只有「无任何调用方 + 结论已固化进文档」两者同时成立才删**。
仅「当前没人跑」但属于再生成路径的脚本（如数据表生成器）一律保留——
删掉它等于让仓库里的二进制数据变成不可再生的黑盒。

---

## 1. 判定方法

1. 全量列出 `*.py`（26 个，排除 `target/`），逐个读文件头的用途声明与关键调用点；
2. 对每个文件名在 `*.md / *.yml / *.yaml / *.ps1 / *.sh / *.toml / *.rs / *.txt / *.json`
   中做字面检索（排除 `target/`），确认上游是否仍有引用；
3. 交叉核对 `.github/workflows/`、`scripts/`、`README.md`、`Cargo.toml` 中的实际调用；
4. 分类：
   - **A 流程在用**：CI / 发布 / 构建直接调用 → 保留；
   - **B 再生成路径**：随包数据（`gb18030_ranges.bin`）、品牌资产（logo SVG/PNG/ICO/ICNS）
     的唯一生成器 → 保留；
   - **C 一次性验证**：无任何代码或流程调用，只作为**当时的**验证记录被文档引用 → 删除。
5. 删除前先整目录复制到临时备份，再逐个删除（不用通配符批量删）。

---

## 2. 删除清单（21 个）

全部位于 `tools/`。共同特征：针对某个改造项（R-xx / S-x / V11-xx）**一次性**注入变异、
跑测试、`try/finally` 还原源码；锚点写死了当时的源码片段，重复执行只会得到「锚点已漂移」的失败，
没有任何长期复用价值。其结论已逐条写进 `docs/architecture-refactor-plan.md` /
`docs/plans/v1.1-backlog.md` / `docs/reports/shared-internal-cycle-review.md`。

| 文件 | 原用途 | 删除理由 |
|---|---|---|
| `bench_arch_r02.py` | R-02 验收：`cargo build` 构建期耗时前后对比（3 次取中位数，`touch src/lib.rs` 强制重跑 build script） | 一次性基准，结论已固化进改造计划；无任何调用方 |
| `mut_arch_check.py` | 逐个破坏校验器修复点，确认对应用例变红 | 同上；且校验器已由其自身的注入用例（`tests/arch_violation_fixture.rs`）常驻守护 |
| `mut_arch_r02.py` | R-02：把未知层守卫改坏，确认新增用例变红 | 一次性变异验证 |
| `mut_arch_r03.py` | R-03：把实现搬回 shared / 削弱禁令 | 一次性变异验证 |
| `mut_arch_r13.py` | R-13：清空 state 禁止清单，验证方向规则用例 | 一次性变异验证 |
| `mut_opener_r03.py` | R-03 用户可见层：改坏 opener 调用，确认 5 条用例变红 | 一次性变异验证（与 `mut_arch_r03.py` 互补） |
| `mut_r04.py` | R-04：改坏 Revert / InsertImage 接线与编排 | 一次性变异验证 |
| `mut_r05.py` | R-05：改坏「0 / 空串 → 未设」归一分支 | 一次性变异验证 |
| `mut_r06.py` | R-06：证明 `tab_cursor` 三层判据对「切标签丢光标」有牙齿 | 一次性变异验证 |
| `mut_r11_1.py` | R-11 子批 1：改坏 `frame/export.rs` 新增写入口 | 一次性变异验证 |
| `mut_r11_2.py` | R-11 子批 2：改坏 `frame/diff.rs` 新增写入口 | 一次性变异验证 |
| `mut_r11_3_editor.py` | R-11 子批 3（editor 组）：改坏 `frame/editor.rs` 写入口 | 一次性变异验证 |
| `mut_r11_3_preview.py` | R-11 子批 3（preview 组）：改坏 `frame/preview.rs` 写入口 | 一次性变异验证 |
| `mut_r11_3_remaining.py` | R-11 子批 3 收尾（settings_ui / find / status_bar / shortcut_overview） | 一次性变异验证 |
| `mut_r11_3_sidebar.py` | R-11 子批 3（sidebar 组）：改坏 `frame/sidebar.rs` 写入口 | 一次性变异验证 |
| `mut_r11_3_ui.py` | R-11 子批 3（ui 组）：改坏 `frame/ui.rs` 写入口 | 一次性变异验证 |
| `mut_r12.py` | R-12：破坏「表现层不得直写 app 状态字段」规则 | 一次性变异验证 |
| `mut_r17.py` | R-17：破坏 widgets 收口的两处配方 | 一次性变异验证 |
| `mut_r20.py` | R-20：破坏字体装配的两条「顺序」约束 | 一次性变异验证 |
| `mut_r21.py` | R-21（item 8 判据加强）：破坏分隔线解析 | 一次性变异验证 |
| `mut_v11_55.py` | V11-55：破坏「对话框过滤器派生自白名单」的两条约束 | 一次性变异验证 |

> 备份与恢复：上述 21 个文件已原样复制到
> `D:\Dev\Cache\Temp\rustmark-py-backup-20261022\tools\`（文件名保持不变）。
> 需要复跑某个变异时，把它复制回 `tools/` 即可；注意锚点可能已随源码演进而漂移。

---

## 3. 保留清单（5 个）

| 文件 | 功能 | 调用方 / 再生成路径 |
|---|---|---|
| `tools/coverage_gate.py` | 覆盖率门禁：解析 `lcov.info`，判定整体 ≥70%、领域层 ≥90%，2% 缓冲区内只 WARN | `.github/workflows/ci.yml` 的 `coverage` job；`README.md` §6「可选本地门禁」 |
| `tools/release_dryrun.py` | 离线发布演练 13 项：版本守卫（绿/红路径）、校验和汇总与篡改检测、Release Notes 归组与渲染、无私钥入仓、回滚步骤一致性 | `.github/workflows/ci.yml` 的发布演练 job；`README.md` §6 |
| `tools/gen_release_notes.py` | 按阶段标签把 commit subject 归组，渲染 `RELEASE_NOTES.md` | `.github/workflows/release.yml` 生成步骤；被 `release_dryrun.py` 以模块方式导入复用 |
| `tools/gen_gb18030_ranges.py` | 从 WHATWG `index-gb18030-ranges` 生成 `src/infrastructure/persistence/gb18030_ranges.bin`（随包二进制数据表） | 唯一再生成路径，见 `src/infrastructure/persistence/encoding.rs` 模块文档 |
| `assets/logo/build_logo.py` | Logo 几何唯一真源：同时产出 SVG / PNG / ICO / ICNS（含 16px 紧凑模式变体） | 品牌资产再生成路径，见 `CHANGELOG.md` 057 条目与 `assets/logo/` 产物 |

---

## 4. 文档引用同步

删除后原本指向这些脚本的引用全部改写为「`mut_xxx.py`（已清理）」，保留当时的验证结论：

| 文件 | 改写处数 | 说明 |
|---|---|---|
| `docs/architecture-refactor-plan.md` | 16 | R-02 / R-03 / R-04 / R-05 / R-11 / R-12 / R-13 / R-17 / R-20 / R-21 各段的变异验证记录 |
| `docs/plans/v1.1-backlog.md` | 7 | 同上的回填记录 |
| `docs/reports/shared-internal-cycle-review.md` | 1 | V11-55 变异验证 |
| `src/infrastructure/fonts.rs` | 1 | R-20 单测注释改为指向改造计划的 R-20 段 |
| `tests/app/writing_goal_test.rs` | 1 | R-05 用例注释改为指向改造计划的 R-05 段 |

`.workbuddy/` 与 `.agent/` 下的历史工作日志未改动：它们是当时的执行记录，
不是项目文档，改掉反而失真。

---

## 5. 清理后验证

| 检查 | 命令 | 结果 |
|---|---|---|
| 语法编译 | `python -m py_compile`（5 个保留脚本） | 通过 |
| 发布流程核心链路 | `python tools/release_dryrun.py` | **13/13 通过**（版本守卫、校验和、Notes 归组、无私钥、回滚步骤） |
| 架构门禁 | `cargo run --bin check-arch` | OK（未发现分层依赖违规） |
| 全量回归 | `cargo test --all-targets` | **2102 passed · 0 failed · 4 ignored** |

`tools/coverage_gate.py` 需要 CI 产出的 `lcov.info` 才能实跑，本次仅做语法编译检查；
其调用方未变，CI 行为不受本次清理影响。

---

## 6. 恢复方式

```powershell
# 单个恢复（示例）
Copy-Item "D:\Dev\Cache\Temp\rustmark-py-backup-20261022\tools\mut_r05.py" `
          "d:\Dev\workspace\2026\10\rust\rustmark\tools\mut_r05.py"
```

备份目录为临时目录，确认无需恢复后可整目录删除。
