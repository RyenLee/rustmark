# RustMark 调试与运行命令手册

> 适用对象：需要在本机运行、调试 `rustmark`（egui + wgpu 原生 Markdown 编辑器）的开发者。
> 本文档与 `docs/troubleshooting.md` 互补：本文偏「命令怎么敲」，后者偏「已知故障怎么修」。

---

## 0. 你遇到的问题（以及为什么现在已修好）

报错原文：

```
error: `cargo run` could not determine which binary to run.
Use the `--bin` option to specify a binary, or the `default-run` manifest key.
available binaries: check-arch, rustmark
```

**根因**：`Cargo.toml` 声明了两个 `[[bin]]` 目标——`rustmark`（编辑器本体）和 `check-arch`（分层架构校验器）。当仓库里有 **多于一个** 二进制目标、且 `[package]` 没有 `default-run` 时，裸 `cargo run` 无法判断你要跑哪一个，于是报错。

**已修复**：已在 `Cargo.toml` 的 `[package]` 段加入 `default-run = "rustmark"`。此后：

- `cargo run` ≡ `cargo run --bin rustmark`（直接启动编辑器）。
- 想跑架构校验器仍需显式写 `cargo run --bin check-arch`。

如果你不想改 `Cargo.toml`（例如 fork 后保留双默认），也可以**永远显式带 `--bin`**，见下一节。

---

## 1. 最常用的运行命令

| 目的 | 命令 | 说明 |
| --- | --- | --- |
| 启动编辑器（调试构建） | `cargo run` | 已设 default-run，等价于 `--bin rustmark` |
| 显式指定二进制 | `cargo run --bin rustmark` | 不改 Cargo.toml 时的稳妥写法 |
| 发布构建运行 | `cargo run --release --bin rustmark` | `opt-level="z"` + LTO，体积最小但编译慢 |
| 只编译不运行 | `cargo build --bin rustmark` | 产物在 `target/debug/rustmark.exe` |
| 跑架构校验器 | `cargo run --bin check-arch` | 验收门禁之一，验证分层无越界 |
| 发布构建（产物） | `cargo build --release --bin rustmark` | 产物在 `target/release/rustmark.exe` |

> Windows 提示：`cargo run` 会把 GUI 窗口直接拉起来；若你想用外部工具（调试器 / 进程监控）观察，建议先 `cargo build --bin rustmark`，再直接双击或命令行启动 `target/debug/rustmark.exe`，这样进程与 `cargo` 脱离，便于 attach。

---

## 2. 调试 GUI 应用的三条路径

`rustmark` 是带 `wgpu/dx12` 后端的 GUI 程序，**不能用 `println!` 在终端里看交互过程**（窗口抢走焦点，且 stdout 未必可见）。推荐以下方式：

### 2.1 VS Code 调试（Windows 首选）

在项目根目录新建 `.vscode/launch.json`：

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Debug rustmark",
      "type": "cppvsdbg",
      "request": "launch",
      "program": "${workspaceFolder}/target/debug/rustmark.exe",
      "cwd": "${workspaceFolder}",
      "args": [],
      "console": "integratedTerminal",
      "stopAtEntry": false
    }
  ]
}
```

- Windows 用 `cppvsdbg`（需装「C/C++」扩展）。
- macOS / Linux 改用 `"type": "lldb"`，并把 `program` 改成 `target/debug/rustmark`。
- 先 `cargo build --bin rustmark`（不要 `cargo run`，否则 VS Code 会重复拉起），再按 F5。

### 2.2 命令行调试器（类 Unix）

```bash
# 先编译
cargo build --bin rustmark
# GDB
rust-gdb target/debug/rustmark
# 或 LLDB
rust-lldb target/debug/rustmark
```

Windows 上 `rustup` 自带的 `rust-gdb` / `rust-lldb` 通常未安装；建议走 2.1 的 VS Code 或系统级 WinDbg。

### 2.3 先启动再 attach

```bash
# 终端 A：拉起应用
cargo run --bin rustmark
# 终端 B / 调试器：附加到进程 rustmark.exe
```

适合「只在某个交互动作后才崩溃」的场景——先复现到崩溃前，再 attach 打断点。

### 2.4 日志输出

代码已引入 `log` 门面（`Cargo.toml` 的 `dependencies` 中有 `log = "0.4"`），主要用于基础设施层越界告警。是否真正打印取决于 `app` 层是否装载了 logger（如 `env_logger`）。验证方式：

```bash
RUST_LOG=debug cargo run --bin rustmark
```

- **有日志输出**：说明已挂载 sink，可继续用 `RUST_LOG=trace` 看更细。
- **无任何输出**：说明当前未初始化 logger，需要先在 `app` 启动路径里 `env_logger::init()`（或等价物）再使用该环境变量。

---

## 3. 调试测试（而非界面）

绝大多数逻辑缺陷用 `cargo test` 复现比手动点界面快得多。本项目测试按层拆成独立 target（见 `Cargo.toml` 的 `[[test]]`）。

| 目的 | 命令 |
| --- | --- |
| 全部单测 + 集成 | `cargo test --all-targets` |
| 仅库内单测 | `cargo test --lib` |
| 仅表现层 | `cargo test --test presentation` |
| 跑单条用例 | `cargo test --test presentation multi_document -- --nocapture` |
| 精确匹配用例名 | `cargo test -- --exact <完整用例名>` |
| 按子串过滤 | `cargo test theme`（匹配所有含 theme 的用例） |
| 单线程跑（看打印顺序） | `cargo test -- --test-threads=1` |
| 显示用例内 `println!` | 在命令末尾加 `-- --nocapture` |

**重点回归用例（与最近四处修复对应）**：

```bash
# 修复1：编辑区在深色/浅色下都白板
cargo test --test presentation editor_view
# 修复4：多文档切 tab 预览不联动
cargo test --test presentation multi_document
# 修复2+3：顶部菜单分层与下拉文案/快捷键重叠
cargo test --test presentation top_menu
```

---

## 4. 格式化与静态检查（提交前门禁）

README §6 规定的验收命令，建议每次改动后都跑一遍：

```bash
cargo fmt --check          # 格式检查（想自动修：cargo fmt）
cargo clippy --all-targets -- -D warnings   # 严格 lint，警告即错误
cargo test --all-targets    # 全量测试
cargo run --bin check-arch  # 分层架构校验
```

---

## 5. 常见故障排查

### 5.1 `LINK : fatal error LNK1104: 无法打开文件 "...rustmark.exe"`

**现象**：编译到链接阶段失败，提示打不开 `rustmark.exe`。
**原因**：上一次 `cargo run` 拉起的 `rustmark.exe` 仍在运行（GUI 进程没关），占住了二进制文件。
**处理**：
1. 关闭已打开的编辑器窗口，或在任务管理器结束 `rustmark.exe`；
2. 或直接在 `target/x86_64-pc-windows-msvc/debug/` 下删除 `rustmark.exe` 与 `deps/rustmark.exe` 后重试。

### 5.2 `warning: error deleting lock file ... 拒绝访问`

**现象**：编译/链接时刷一堆「删除锁文件被拒绝」的警告。
**原因**：杀毒软件或 IDE 实时扫描 `target/` 目录，短暂锁定了 incremental 的 lock 文件。与代码无关，**可忽略**；若想消除，把项目目录加入杀软/IDE 的实时扫描排除列表。

### 5.3 在无显示器环境跑 `cargo run` 没反应 / 崩

`rustmark` 需要真实显示设备（wgpu/dx12 后端）。CI 或远程无头环境请用 `cargo test` 代替 `cargo run`，不要尝试直接拉起 GUI。

### 5.4 启动即 panic：`No wgpu backend feature ...`

属于 DEF-03 类配置缺陷，`Cargo.toml` 已按平台补了 wgpu 后端 feature（Windows→dx12）。若仍报，先 `cargo clean` 再重新构建，确保 feature 重新解析。

---

## 6. 一键速查

```bash
# 启动编辑器（日常调试）
cargo run

# 跑全部门禁（提交前）
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --all-targets && cargo run --bin check-arch

# 单点复现最近修复
cargo test --test presentation editor_view
cargo test --test presentation multi_document
cargo test --test presentation top_menu
```

---

_文档生成日期：2026-10-10。命令基于仓库当前 `Cargo.toml`（`default-run = "rustmark"` 已生效）。_
