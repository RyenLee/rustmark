# 贡献指南

> 适用：`rustmark` 全部代码、测试与文档。
> 执行模型：任务单元制（见 `.agent/README.md`）——一次一个单元，每单元可编译、可测试、可提交。

---

## 1. 提交前验证链（每单元必跑）

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check
cargo test
cargo run --bin check-arch
```

任一命令失败即视为单元未完成。`cargo run --bin check-arch` 会在违规时打印 `文件:行号 —— 原因`。

另有两项**工具化门禁**（纯标准库 Python，无额外依赖），CI 与本地同款：

```bash
# 覆盖率门禁：整体 ≥70%、领域 ≥90%（2% 缓冲区内 WARN）
cargo llvm-cov --all-targets --lcov --output-path lcov.info -- --skip large_full_text_find_is_bounded
python tools/coverage_gate.py lcov.info

# 离线发布演练：版本守卫 / 校验和 / Notes 归组 / 无私钥入仓 / 回滚步骤
python tools/release_dryrun.py              # 红路径验证：--tag v9.9.9 须 exit 1
```

> **已知缺口（v1.1 待办）**：§4「生产代码禁用 `unwrap()` / `expect()` / `panic!`」
> 目前**没有机械检查**——`clippy` 默认不开启 `unwrap_used` / `expect_used`。
> 现存 14 处均位于不变式守卫之后（如 `let Some(_id) = ... else { return; }`）
> 或在 `panic = "abort"` 下不可达（panic 直接终止进程，Mutex 不会被 poison），
> 但规范文字与实现之间存在灰区，需 lint 门禁 + 逐处裁决（登记为 V11-12）。

## 2. Commit 规范

```
feat({模块}): {一句话描述} [S{阶段}-{编号}]
```

示例：

```
feat(shared): 实现 TextRange 与 ByteOffset 值对象 [S2-05]
fix(editor): 撤销栈按文档隔离，修复跨文件污染 [S3-08]
```

规则：

- 每任务单元**一个 commit**，便于回滚；
- 类型前缀：`feat` / `fix` / `refactor` / `test` / `docs` / `chore` / `perf`；
- 末尾方括号内必须是 `.agent/tasks/` 中存在的任务编号。

## 3. 分层规则速查

| 层 | 禁止 |
|---|---|
| domain | `egui` / `eframe` / `rfd` / `syntect` / `pulldown_cmark` / `std::fs` / `std::io` / `anyhow` / `println!` |
| application | 上述 UI 与解析器 crate、`std::fs`，且签名不得出现聚合根 |
| infrastructure | `crate::application` / `crate::presentation` |
| presentation | `crate::domain` / `crate::infrastructure` |

完整矩阵见 `.agent/architecture.md`。确需豁免时，在 `use` 行尾加 `// arch-allow: <crate>` 并在该文档登记。

## 4. 编码约定要点

- Rust 2024 Edition，**禁止 `mod.rs`**；路径统一 `crate::` 开头；
- 生产代码禁用 `unwrap()` / `expect()` / `panic!`（`panic = "abort"` 下即进程终止）；
- 领域层错误用 `thiserror`，禁止 `anyhow`；`anyhow` 仅用于 `app.rs` / `main.rs`；
- 所有公开 API 必须有 `///` 文档注释，含边界语义（命中边界返回什么）；
- 涉及时间的行为（防抖、撤销合并窗口）注入 `Policy` / `Clock`，不得直接读系统时间。

详见 `.agent/conventions.md`。

## 5. 依赖变更

新增依赖必须：

1. 在 `docs/dependencies.md` 登记版本、用途与体积贡献；
2. 通过 `cargo deny check licenses advisories`；
3. 属于 P0 必需或经 ADR 追加评审——**任务单元内禁止顺手加依赖**。

## 6. Golden 测试数据

`tests/data/golden/` 下的语料为解析、高亮、导出的比对基准。变更语料必须在 PR 说明中写明变更原因与影响面，禁止为让测试变绿而修改语料。

### 6.1 快照更新流程

快照文件是 `*.expected`，由 `tests/integration/main.rs` 比对。

**快照变了 ≠ 代码错了。** 正确流程：

1. 先跑 `cargo test --test integration`，读失败信息里打印的**差异行**；
2. **人工审阅**：这个变化是否符合预期？特别要警惕
   「为了让某个测试通过而顺手改了快照」；
3. 确认无误后，用环境变量显式更新：

   ```bash
   UPDATE_GOLDEN=1 cargo test --test integration
   ```

4. 在 PR 描述里写明：**哪条快照变了、为什么变、是否属于有意行为变更**。

**绝不允许**：不看内容就批量 `--accept`；为了让测试变绿而修改 `*.md` 语料；
把 `UPDATE_GOLDEN=1` 写进 CI 或默认脚本。

### 6.2 哪些文件可以改

| 文件 | 能否手工修改 | 说明 |
|---|---|---|
| `tests/data/golden/*.md` | ❌ 仅在**新增覆盖场景**时 | 修改语料等于修改测试的「正确答案」 |
| `tests/data/golden/*.expected` | ❌ 只能由 `UPDATE_GOLDEN=1` 生成 | 手工改等于绕过审阅 |
| `target/golden/large-1mb.md` | ✅ 自动生成 | 1MB 语料不入库，按需生成 |

## 7. 测体积的正确姿势

**用 `cargo bloat --release --bin size-probe --crates`，不要用 `--bin rustmark`。**

原因见 `docs/metrics.md` §3.1：主二进制在S6-01 接线前是空壳入口，
LTO 会把所有未被引用的依赖整体裁掉，量出「体积达标」的假象。
`size-probe` 会显式构造每个适配器，使链接器无法裁剪。
S6-01 接线完成后可改回主二进制，但需同步更新本节与 `docs/metrics.md`。
