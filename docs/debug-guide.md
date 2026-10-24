# RustMark 调试与运行命令手册

> 适用对象：需要在本机运行、调试 `rustmark`（egui + wgpu 原生 Markdown 编辑器）的开发者。
> 本文档与 `docs/troubleshooting.md` 互补：本文偏「命令怎么敲 + 缺陷怎么定位」，后者偏「终端用户的已知故障怎么修」。
> **§6「GUI 缺陷定位方法论」**沉淀自问题记录 001–027 的真实排障（运行时取证 / 插桩量 rect /
> 逐字形实测 / panic=abort 纪律 / 依赖审查法），排障前建议先读。

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
| 启动计时埋点 | `RUSTMARK_STARTUP_TIMING=1 cargo run` | stderr 打印五个里程碑耗时（见 §1.1） |
| 发布构建运行 | `cargo run --release --bin rustmark` | `opt-level="z"` + LTO，体积最小但编译慢；性能门禁只认 release 口径（§6.6） |
| 只编译不运行 | `cargo build --bin rustmark` | 产物路径见下方「产物在哪」注 |
| 跑架构校验器 | `cargo run --bin check-arch` | 验收门禁之一，验证分层无越界 |
| 发布构建（产物） | `cargo build --release --bin rustmark` | 同上，release 口径 |

> **产物在哪（本机特有，踩过坑）**：本机全局 cargo 配置（`CARGO_HOME` = `D:\Dev\Rust\.cargo\config.toml`）设了
> `build.target = "x86_64-pc-windows-msvc"`，因此 exe **不在** `target/debug/`，而在
> `target/x86_64-pc-windows-msvc/debug/rustmark.exe`（release 同理换 `release/`）。
> CI 或未配 `build.target` 的机器上才是 `target/{debug,release}/`。找不到 exe 时最可靠的定位方式：
> `cargo build --bin rustmark -v 2>&1 | grep -o '\-\-out-dir [^ ]*'`，或直接 `find target -name rustmark.exe`。

> **打开文件只能走 UI**：应用不解析命令行参数（`env::args` 零引用），没有 `cargo run -- some.md` 这种用法。
> 想快速验证渲染，把 `docs/logs/示例文档.md`（含 Mermaid / 公式 / 表格语料）拖进窗口即可。

### 1.1 启动计时埋点（`RUSTMARK_STARTUP_TIMING=1`）

`NFR-PERF-02`（冷启动 ≤ 2s）的测量入口。默认完全不工作（避免测量污染被测对象），
设环境变量后才记录，经 `eprintln!` 打到 **stderr**——所以必须从终端启动才看得见：

```bash
RUSTMARK_STARTUP_TIMING=1 cargo run --bin rustmark
```

五个里程碑（口径对齐 `docs/metrics.md` §3）：`进程启动` → `装配完成`（配置读盘 + 语法集加载 +
最近文件恢复）→ `应用构造完成`（主题解析 + 快捷键注册 + 草稿扫描）→ `首帧提交`（窗口骨架可见）→
`内容就绪`（文档解析 + 高亮完成，有文档打开时才有意义）。

### 1.2 构建命令（debug / release / check / 打包）

| 目的 | 命令 | 说明 |
| --- | --- | --- |
| 调试构建（默认） | `cargo build` 或 `cargo build --bin rustmark` | debug profile：`opt-level=0`、`debug=1`（仅行号）、`codegen-units=16`（本机全局配置值，见 §1「产物在哪」）——编译最快、带调试信息 |
| 发布构建 | `cargo build --release --bin rustmark` | release profile 由 `Cargo.toml` 统一控制：`opt-level="z"` + `lto=true` + `codegen-units=1` + `panic="abort"` + `strip=true`——**体积与性能数字只认这个口径**（§6.6） |
| 快速类型检查 | `cargo check --all-targets` | 不链接、不产 exe，改完代码最快验证「能不能编译过」；CI 门禁之一 |
| 只编译测试不跑 | `cargo test --no-run` | 改测试代码时确认编译通过，省掉执行时间 |
| 单独构建校验器 | `cargo build --bin check-arch` | 也可以 `cargo run --bin check-arch` 直接跑（§4 门禁） |
| 逐 crate 体积归因 | `cargo bloat --release --bin rustmark --crates 15` | CI 构建摘要同款；需先 `cargo install cargo-bloat --locked` |
| Windows 便携打包 | `scripts/package-windows.ps1 [-Version 1.0.0]` | 内部先 `cargo build --release`，产物 `dist/rustmark-v<版本>-windows-x64.zip` + 同名 `.sha256`，末尾执行 10MB 红线门禁（见下方注意 3） |

**构建前必须知道的三件事**：

1. **`build.rs` 就是第一道架构门禁**：每次构建都无条件全量扫描 `src/` 的 `use` 语句，分层违规直接
   `panic!` 让构建失败，并打印 `文件:行号 —— 原因`。「莫名其妙编译不过」先看这段输出；
   人工复核用 `cargo run --bin check-arch`。逃生口 `SKIP_ARCH_CHECK=1 cargo build`
   **仅供本地临时排查，CI 不得设置**。
2. **本 crate 没有任何 `[features]` 开关**：构建不需要传 `--features`；wgpu 后端按 `target_os`
   自动三选一（Windows→dx12 / macOS→metal / Linux→vulkan+gles），交叉编译时自动切换，无需手工指定。
3. **当前 release 已超打包红线**：`target/x86_64-pc-windows-msvc/release/rustmark.exe` 实测
   11,463,680 字节（10.93 MiB）> `package-windows.ps1` 内置的 10 MB 门禁——**现在直接跑打包脚本
   会在体积检查处 throw**。缓解方案待 v1.1 评审（见 `docs/metrics.md` §2.4）；方案落地前只是想
   验证打包流程，可本地临时调大脚本里的 `$limitBytes`（**勿提交该改动**）。

> 工具链由 `rust-toolchain.toml` 固定为 **1.99.0**（含 rustfmt / clippy 组件，`profile = "minimal"`），
> 首次构建时 rustup 自动安装，无需手工 `rustup install`。
> 切换工具链或改过 feature 后遇到「怪问题」（如 §5.4 的 wgpu panic），先 `cargo clean` 再重建。

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
      "program": "${workspaceFolder}/target/x86_64-pc-windows-msvc/debug/rustmark.exe",
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
- **`program` 路径以 §1「产物在哪」为准**：本机带 `x86_64-pc-windows-msvc` 三重目录（上面已按本机写）；
  若你的机器没配 `build.target`，改回 `${workspaceFolder}/target/debug/rustmark.exe`。

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
（本机因全局 `build.target` 配置，路径同样是 `target/x86_64-pc-windows-msvc/debug/rustmark`，见 §1 注。）

### 2.3 先启动再 attach

```bash
# 终端 A：拉起应用
cargo run --bin rustmark
# 终端 B / 调试器：附加到进程 rustmark.exe
```

适合「只在某个交互动作后才崩溃」的场景——先复现到崩溃前，再 attach 打断点。

### 2.4 日志输出（env_logger 已注册，`RUST_LOG` 调级别）

> **2026-10-24 修订（D-2 纠错）**：本节旧版声称「本项目没有 logger 后端、`RUST_LOG` 无效」——
> 那是 F-2 之前的状态，**已过期**。按旧版操作会得出「日志坏了」的错误结论。

现状（真源：`src/main.rs` 的 `env_logger::Builder` 调用）：

- **logger 后端已注册**：F-2 起 `main` 在启动时注册 `env_logger`，输出到 stderr；
- **默认级别 warn**：不设 `RUST_LOG` 时，所有 `log::warn!` / `log::error!` 直接可见
  （如 `RegistryProvider` RwLock poisoned、未知语言降级、scope 栈失衡）——**默认就能看到告警**；
- **排障开全量**：`RUST_LOG=debug cargo run`；
- **只看一个模块**：`RUST_LOG=rustmark::app=debug cargo run`（模块 + 级别指令，
  不依赖正则——env_logger 未开 `regex` feature，正则模式的 `RUST_LOG` 表达式不支持）；
- 启动埋点走另一条通路：`RUSTMARK_STARTUP_TIMING` 的 `eprintln!`（§1.1），与 logger 无关；
- panic 栈照旧：debug profile 是 unwind，且全局已设 `RUST_BACKTRACE=1`。

> release profile 是 `panic = "abort"`：panic 时**没有栈展开、没有 backtrace**，进程直接终止。
> 调试 panic 一律用 debug 构建（`cargo run`），不要用 `--release`。

---

## 3. 调试测试（而非界面）

绝大多数逻辑缺陷用 `cargo test` 复现比手动点界面快得多。本项目测试按层拆成独立 target（`Cargo.toml` 的 `[[test]]` 显式声明，「删掉一个入口文件 = 从构建里移除该层」是有意识的决定）：

| 测试目标（`cargo test --test <名>`） | 覆盖 |
| --- | --- |
| `--lib` | 库内单测（`src` 里的 `#[cfg(test)]`） |
| `domain` | 领域层纯单测（零依赖、编译期强制） |
| `application` | 应用层用例（fake 依赖） |
| `infrastructure` | 适配器 / golden / 资源加载 |
| `presentation` | 表现层（egui CPU 布局，无需 GPU） |
| `shared` | shared 层单测 |
| `integration` | 跨层 + 真实 tempdir + golden 导出 |
| `app` | 组合根与调度 |
| `e2e` | 可自动化端到端 |
| `startup_smoke` | 启动冒烟（守「能编译能测试但启动即崩」的配置型缺陷，DEF-03 教训） |
| `arch_violation_fixture` | 架构校验器自身验证（零违规 + 注入违规必被检出） |

常用命令：

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
| 跑被忽略的用例 | `cargo test --test infrastructure -- --ignored`（如 `real_https_download` 需真实外网，默认 `#[ignore]`） |

**重点回归用例（与近几批修复对应，详见问题记录与 §6 方法论）**：

```bash
# 修复1：编辑区在深色/浅色下都白板
cargo test --test presentation editor_view
# 修复4：多文档切 tab 预览不联动
cargo test --test presentation multi_document
# 修复2+3：顶部菜单分层与下拉文案/快捷键重叠
cargo test --test presentation top_menu
# 014：连续回车产生可见空行（渲染层根因，非 IME）
cargo test --test presentation highlight_layout empty_line
# 025：切「检查」标签不把编辑区推右（布局回归）
cargo test --test presentation lint_tab_does_not_push_the_editor_right
# 024：甘特图真实日期 / 状态配色 / 日期轴
cargo test --test presentation mermaid::tests gantt
# 026：https 图片下载失败降级不 panic（真实外网用例为 #[ignore]，需手动跑）
cargo test --test infrastructure asset_loader
```

---

## 4. 格式化与静态检查（提交前门禁）

README §6 规定的验收命令，建议每次改动后都跑一遍：

```bash
cargo fmt --check                              # 格式检查（想自动修：cargo fmt）
cargo clippy --all-targets --all-features -- -D warnings   # 严格 lint，警告即错误
cargo test --all-targets                       # 全量测试
cargo run --bin check-arch                     # 分层架构校验
```

> `-D warnings` 必须放在 `--` 之后（`--` 之后的部分原样传给 clippy 驱动），
> 写成 `cargo clippy -D warnings --all-targets` 会报无法识别的参数。

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

### 5.5 编译成功但找不到 `rustmark.exe`

`cargo build` 零报错，却被告知「文件不存在」。九成本机全局 cargo 配置指定了
`build.target = "x86_64-pc-windows-msvc"`，产物在 `target/x86_64-pc-windows-msvc/debug/` 下，
而脚本 / 文档 / VS Code 配置写的是 `target/debug/`。定位与修复见 §1「产物在哪」注：
把消费方路径改成三重目录，或 `cargo build -v` 看真实 `--out-dir`。

---

## 6. GUI 缺陷定位方法论（实战沉淀）

> 本章内容全部来自本项目的真实排障记录（`docs/logs/问题记录1.txt` 001–027、`docs/metrics.md`、
> `docs/archive/stage-reports/` 各报告），不是通用教条。每条注明案例编号以便追溯。

### 6.1 第一原则：渲染层缺陷必须「运行时取证」，单测注入事件会集体打偏

**案例**：011/012/013 连续三轮修复「按回车不换行」，十几次改动全部打在「IME / 回车事件」方向，
全部无效。014 用运行时日志才定位到真正根因在**渲染层**（见 6.5 与下）。

**教训**：单测与探针直接注入 `egui::Event` 会**跳过真实翻译链**（IME、合成、`remove_ime_incompatible_events`
都在控件内部按当帧 state 决策），「测试里复现不了」不等于「缺陷不存在」，「测试里修好了」也不等于
「用户机器上修好了」。GUI 缺陷的第一动作是拿**运行时证据**：

1. 临时插桩日志（帧循环 / 事件流 / 领域 version 比对），headless 帧循环驱动真实 `update`
   （eframe `Frame::_new_kittest()`，无需窗口与 GPU）；
2. 关键不变量写成「注入事件也必须满足」的暴力扫描测试（如 012 的 `enter_sweep_test.rs`：
   每次非 repeat 回车必须让领域 / 绑定串 / 可见 galley 立即各多一个 `\n`）；
3. 定位完成、插桩**全部移除**并复跑门禁（014/025 均如实记录「插桩已移除，源码恢复干净」）。

### 6.2 布局类缺陷：逐帧插桩量 rect，然后读框架源码确认保留规则

**案例**：025 的「检查标签黑框」——侧栏与编辑区之间一条竖向纯黑矩形，编辑区被推右。
初判「画了黑 shape」抓不到凶手；插桩逐帧量 `available_rect` 才发现编辑区左缘从 x=240 被推到 423.9。

**根因链**（两个 egui 事实叠加）：

- 水平布局（`Layout::left_to_right`）里的裸 `ui.label` **不换行**，文字超宽时 `min_rect`
  直接溢出分配宽（lint 行 232pt 实测溢到 407.9pt）；
- egui `SidePanel` 给后续布局保留的是**内容 `response.rect` 的最大 x**（`panel.rs` 的
  `allocate_left_panel`），不是 `exact_width`——侧栏实占 [0, 415.9]；
- 而 [240, 415.9] **没有任何面板绘制**：侧栏底色被 `set_clip_rect` 裁在面板矩形内，溢出区
  谁都不画，真实渲染露出交换链清屏色（黑）。

**方法**：布局异常先量 rect（插桩 `available_rect` / `response.rect` 逐帧打印），定位到
「谁被推走 / 谁没画」，再读框架源码确认保留规则，最后用**真实帧渲染 + 几何断言**的回归测试
钉死（`lint_tab_does_not_push_the_editor_right`：断言编辑区背景左缘紧贴侧栏右缘）。
**凡是侧栏行文本（lint / 大纲 / 文件树 / 搜索），一律 truncate**——这是本仓库的既有约定，
新增侧栏视图漏截断就会以完全相同的方式撑出黑框。

### 6.3 字形 / 字体类故障：逐字形实测覆盖，不靠肉眼

**案例**：022 数学符号方框——符号表把 `\forall` 映射到 `∀` 之类 Unicode 字形，但**映射过去不等于
渲染得出来**：字形还要落在已装载字体的覆盖范围内。按真实字体配置逐符号实测，67 个常用符号在
西文与中文字体里全部落空（U+2200–22FF 与 U+2A00–2AFF 两边都不覆盖）。

**教训**：这类故障**不报错、不崩溃**，肉眼在几十个符号里几乎无法归因（工具栏图标踩过同款坑），
只能靠自动化断言守住。凡是「字符 → 字形」的映射，都要按**真实字体配置**（`font_definitions` +
完整兜底链）逐字符断言字形存在；随仓库分发的字体资产本身也要有测试守护——本项目
`panic = "abort"`，畸形字体文件会**直接终止进程**，没有兜底机会。

### 6.4 「拼写只是触发器，语法家族才是根因」

**案例**：015 流程图渲染失败。示例文档用了流传甚广的笔误 `flowchat`（少个 r），报错却显示
`Unsupported("flowchat")`——看上去像引擎连 `flowchart` 都不认，极具误导性。但把笔误改对也无济于事：
该节用的是 **flowchart.js 语法**（`id=>type: 文本` 定义），与 Mermaid 是**两个语法家族**。

**方法**：解析器报「不支持的 X」时，先分清 X 是**拼写问题**还是**语法家族问题**；识别签名按
「两套语法互不误伤」设计（flowchart.js 签名 = 行首 `id=>type` 定义行，Mermaid 合法行不可能如此）。

### 6.5 egui / epaint 特有陷阱（均为实测结论）

| # | 陷阱 | 后果 | 对策 |
|---|---|---|---|
| 1 | 未绑定的 `FontFamily::Name` 是**直接 panic**（epaint `fonts.rs:808`） | `panic="abort"` 下窗口直接消失；021 的「自建字体族」方案因此被否决（6 条既有单测以 panic 暴露） | 字体置顶进**内置**族（`FontId::monospace` 永远有绑定）；无头测试上下文也要装字体 |
| 2 | 水平布局裸 label 不换行（见 6.2） | 撑爆 SidePanel 布局、露出清屏黑 | 侧栏行文本一律 `truncate()` |
| 3 | `set_fonts` 会重建字体图集 | 一次**可见卡顿** | `ensure_editor_fonts` 每帧比对持久化值与已装值，**没变就不动** |
| 4 | 纯空 `LayoutJob` 的 Galley **零高度**（空段落高度仅在非空 section 时置位） | 空行不可见、行号栏错位（014 的真实根因） | 空行补一个空 section（`job.append("", 0.0, base)`） |
| 5 | 缓存签名漏项 | 「换了字体 / 字号却没了生效」——Galley 被整批复用 | `editor_metrics_signature` 必须覆盖一切排版输入（字体族 / 字号 / 主题） |

### 6.6 性能数字的口径纪律

1. **debug 与 release 差约 40%**（`docs/07` 已知问题）——性能门禁一律放 release；
   debug 下的耗时断言只打印 `[metrics]` 不做门禁（V11-13 裁定），否则会把达标项误判为不达标。
2. **LTO 项目里「未接线的空壳入口」会让 `cargo bloat` 量到假达标**——链接器把未引用代码全裁掉，
   小体积是假象。体积探针的寿命绑定在「主二进制是否已引用该依赖」上，**用完即删**（`docs/metrics.md` §3.1 教训）。
3. **归因先做地板探针**：本项目空闲内存 125.3 MB 里自有代码只贡献约 0.9 MB，其余全是
   egui + wgpu + dx12 基线——不探明地板就会为「不存在的优化空间」做无用功（`docs/metrics.md` §3.0.2）。

### 6.7 依赖审查方法（可复现）

用户请求「找出未使用或可更小范围替代的依赖」时的标准动作（V11-26 实例）：

1. 逐 crate 反查引用：`grep -rn "<crate>" src tests`——零命中且特性无副作用才考虑移除；
2. feature 来源反查：`cargo tree -e features -i <crate>`——确认某个 feature 拖进来了什么
   （实例：`ahash` 的 `runtime-rng` 是 `getrandom 0.3.4` 的唯一引入者）；
3. 重复版本扫描：`cargo tree -d`；
4. 体积：**同一 release 口径**（`cargo build --release --bin rustmark`）前后对比；
5. 「复核后保持现状」的结论也要**记录理由**（`docs/dependencies.md` §6.2），避免反复讨论。

### 6.8 诊断插桩的纪律

插桩（`eprintln` / 探针截图 / 临时测试）是定位手段，不是交付物：

- 定位完成即全部移除，复跑四道门禁确认源码干净（014/025 均如实记录）；
- 有价值的定位方法（而不是数据本身）沉淀进本章与回归测试；
- 探针项目 / 临时二进制的存在期限必须绑定在某个具体问题上，问题关闭即删。

---

## 7. 一键速查

```bash
# 启动编辑器（日常调试；把 docs/logs/示例文档.md 拖进窗口可快速验证渲染）
cargo run

# 启动计时（stderr 输出五个里程碑耗时）
RUSTMARK_STARTUP_TIMING=1 cargo run --bin rustmark

# 构建：日常 debug / 门禁口径 release / 最快编译检查
cargo build --bin rustmark
cargo build --release --bin rustmark
cargo check --all-targets

# 跑全部门禁（提交前；注意 -D warnings 在 -- 之后）
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings \
  && cargo test --all-targets && cargo run --bin check-arch

# 单点复现最近修复
cargo test --test presentation editor_view
cargo test --test presentation multi_document
cargo test --test presentation top_menu
```

---

_文档生成日期：2026-10-10；§6 方法论章节增补于 2026-10-14（源自问题记录 001–027 与 metrics 各节实测）。
本地启动与构建信息于 2026-10-14 二次校准，对齐工程实际：产物路径（全局 `build.target` 三重目录）、
日志机制（env_logger 已注册、默认 warn、`RUST_LOG` 调级别，2026-10-24 修订，见 §2.4）、启动埋点 `RUSTMARK_STARTUP_TIMING`（§1.1）、
构建命令总表（§1.2：debug/release/check/打包/cargo bloat 与 build.rs 门禁、10MB 红线现状）、
门禁命令 `--all-features` 与 `-- -D warnings` 位置（§4）、测试目标总览（§3）。
命令基于仓库当前 `Cargo.toml`（`default-run = "rustmark"` 已生效）。_
