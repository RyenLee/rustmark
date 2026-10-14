# 指标口径与基线（metrics）

> 关联需求：`NFR-PERF-01`~`07`；关联任务：S1-07（口径）、S8-01~08（填充实测值）、S7-04（大文件）
> 原则：**先统一口径，后填数字**。任何"达标"结论必须能被本文件的命令复现。

---

## 1. 七项硬指标口径

| 编号 | 指标 | 阈值 | 测量命令 / 工具 | 采样与取值 | 状态 |
|---|---|---|---|---|---|
| `NFR-PERF-01` | 安装包体积 | ≤ 10 MB（预警 8 MB） | Windows：`dir` 或 `stat` 产物；macOS：`du -h *.dmg`；Linux：`stat -c%s *.AppImage` | 三平台各 1 次，取最大值 | **Windows 实测 10.93 MiB，仍越线 0.93 MiB**——初始 11.04 MiB 见 §2.4 归因，依赖复查后的复测见 §2.5 |
| `NFR-PERF-02` | 冷启动 | ≤ 2s（p50 目标 300–500ms） | 计时脚本：exec 到首帧提交（`app.rs` 内埋点输出） | 每平台 5 次，取中位数；**排除首次磁盘冷缓存** | 待阶段 08 |
| `NFR-PERF-03` | 空闲内存 | **按平台各自阈值**（ADR-006 修订）：Windows **≤ 150 MB**（判定口径 **Private Working Set**）；macOS / Linux 待 Stage 09 真机补测后填入 | Windows：PowerShell `Get-Process` 取 **PrivateMemorySize64 之外的 Private Working Set**（WorkingSet64 仅作参考，噪声 ±20 MB）；macOS：`footprint <pid>`；Linux：`/proc/<pid>/status` VmRSS | 打开空白文档静置 10s 后取值；三口径**不可横向比较**，各记各的；定阈值前**必须先做地板探针**分清图形栈基线与自有代码 | ✅ 已裁定（Windows 125.3 MB / 阈值 150 MB） |
| `NFR-PERF-04` | 编辑响应 | ≤ 16ms（一帧），p95 | 帧内埋点：输入事件到重绘完成 | 连续输入 100 次，取 p95 | 待阶段 08 |
| `NFR-PERF-05` | 语法高亮 | ≤ 10ms（10KB 全文） | `HighlightQuery` 计时（调试面板 + 基准） | 20 次取 p50/p95 | 待阶段 05/08 |
| `NFR-PERF-06` | Markdown 解析 | ≤ 5ms（10KB） | `RenderPreviewQuery` 计时 | 20 次取 p50/p95 | 待阶段 05/08 |
| `NFR-PERF-07` | 大文件 1MB | 可编辑不卡顿 | 手动验证 + 帧时间 p95 ≤ 16ms | 打开/滚动/查找/保存四项 | 待阶段 07/08 |

## 2. 体积预算分解（预估 vs 实测）

> **口径警告（必读）**：「实测」列是 `cargo bloat` 给出的 **`.text` 段**（代码）口径，
> 与 §2.2 的「**文件总大小**」口径**不同**，二者不可相加、不可相互换算。
> 字体数据、`.rodata`、资源与 PE 头等都在 `.text` 之外——这也是为什么下面
> 逐组件 `.text` 合计只有 3.8 MiB、而文件总量是 6.85 MiB。
> 判断「是否超 10 MB 红线」一律以**文件总大小**为准；逐组件归因只用 `.text` 定位热点。

| 组件 | 预估（需求 §8.1） | 实测（`.text`，S8-01） | 偏差 | 处置 |
|---|---|---|---|---|
| egui + eframe（+ epaint） | 3–4 MB | **328.6 KiB**（225.4 + 38.9 + 64.3） | **−90%**（低于预估） | 不处置，转监控。偏差主因是口径：预估含未裁剪/调试符号，实际 `opt-level="z"` + LTO + `strip` 后大幅收缩；且 egui 字体在 `.rodata` 不计入 `.text` |
| pulldown-cmark | 0.44 MB（预算 ≤0.5 MB） | **60.4 KiB** | −87% | ✅ 不查 feature |
| syntect | 0.8–1.5 MB（预算 ≤1.5 MB） | **未进前 25**（≤37.8 KiB；S5-03 实测 17.1 KiB） | **−97%** | ✅ **ADR-002 分级裁剪预案（20→10→5）不触发**；原因见 §2.2 |
| ropey | 0.15 MB（预算 ≤0.2 MB） | **未进前 25**（S5-03 实测 11.5 KiB） | – | ✅ |
| rfd + serde 等 | 0.5 MB | **toml 49.7 KiB**（rfd 未进前 25） | – | ✅ |
| 应用代码（rustmark） | 0.5–1 MB | **229.7 KiB** | −55% | ✅ Stage 06/07 大量表现层代码后仍在预算内 |
| **`.text` 合计** | – | **3.8 MiB** | – | 供归因用 |
| **文件总大小（判定口径）** | **5–7 MB（红线 10 MB）** | **6.85 MiB**（见 §2.3） | **在预估区间内** | ✅ 距 8 MB 预警线余量 1.15 MiB |

### 实际体积大头（S8-01 新识别，预估表未单列）

预估表把注意力放在 syntect 上，实测显示**真正的体积大头是另外三组**：

| 组 | 实测 `.text` | 来源 | 处置 |
|---|---|---|---|
| **wgpu 系**（naga + wgpu_core + wgpu） | **838.7 KiB**（490.2 + 291.9 + 56.6） | ADR-001 选 wgpu 后端 | 最大第三方组；ADR-001 已接受该代价，红线内不处置 |
| **正则系**（regex_automata + aho_corasick + regex_syntax） | **439.7 KiB**（265.1 + 101.1 + 73.5） | syntect 的 `regex-fancy` + `toml` 共同引入 | 第二大头；S8-02 结论表明不值得为它动 syntect |
| **图片解码链**（zune_jpeg + image_webp + image + png） | **294.5 KiB**（104.7 + 78.1 + 73.9 + 37.8） | S6-05 预览区显示图片 | 已在 `image` 上只开白名单格式（无 svg）；如需再压，唯一选项是砍图片预览（P1 特性，不推荐） |
| std | 853.0 KiB | 标准库 | 单项最大，不可控 |

测量命令：`cargo bloat --release --bin rustmark --crates 25`（每完成一个子模块跑一次并登记）。

> ⚠️ **S6-01 之前**该命令曾必须改用 `--bin size-probe`：那时 `app::run()` 是空壳，
> LTO 会把未被引用的 egui / wgpu 全部裁掉，量到的是「假达标」。S6-01 组合根接线后
> 主二进制已引用全部依赖，**测量对象已切回 `--bin rustmark`**，`size-probe` 随之删除。

## 2.1 阶段 05 体积实测（S5-03 出口）

> 环境：Windows / `x86_64-pc-windows-msvc` / rustc 1.99.0 / `--release`（`opt-level="z"` + LTO）。
> 复现命令见上方。**该表测于 S6-01 接线之前，用的是 `size-probe`**（原因见 §3.1）；
> 接线后的整体数字见 §2.2。逐 crate 贡献量的量级仍然有效。

| crate | 实测 `.text` | 预算 | 结论 |
|---|---|---|---|
| **syntect** | **17.1 KiB** | ≤ 1.5 MB | ✅ 余量约 90 倍 |
| **pulldown-cmark** | **60.4 KiB** | ≤ 0.5 MB | ✅ |
| **ropey** | **11.5 KiB** | ≤ 0.2 MB | ✅ |
| regex_automata（含 aho-corasick / regex-syntax） | 440 KiB | – | 由 syntect 的 `regex-fancy` 与 `toml` 共同引入 |
| fancy-regex | 32.7 KiB | – | syntect 纯 Rust 正则后端 |
| toml（含 toml_parser） | 73 KiB | – | 配置持久化 |
| rustmark（自有代码） | 50.0 KiB | – | 阶段 05 全部适配器 |
| **`.text` 合计** | **1.1 MiB** | 5–7 MB | ✅ |
| **文件总大小** | **2.1 MiB** | ≤ 10 MB | ✅（**尚未链接 egui + wgpu**） |

## 2.2 接线后体积实测（S6-01 出口，**首个有意义的整体数字**）

> 环境同上。复现：`cargo build --release --bin rustmark`。
> 测量对象已从 `size-probe` 切回主二进制（`size-probe` 已删除，理由见本文顶部）。

| 项 | 实测 | 预算 | 结论 |
|---|---|---|---|
| **release 文件总大小** | **6.19 MiB（6,491,648 bytes）** | ≤ 10 MB（预警 8 MB） | ✅ 余量 38%，距预警线还有 1.8 MiB |
| debug 文件总大小 | 22.4 MiB | – | 仅供开发，不作基线 |

**依赖确实链入的验证**（不能只看文件大小——LTO 下一个「没接线」的空壳也能是小文件）：
扫描二进制字符串表，`wgpu` / `naga` / `winit` / `egui` / `eframe` / `accesskit` /
`syntect` / `pulldown-cmark` / `rfd` / `ropey` 全部有命中。
`wgpu-hal` 无命中属**正常**：Windows 上它走 d3d12 / vulkan 原生动态加载，
符号名不留在字符串表里。

> 注（2026-10-14）：上表是**阶段 05 当时**的字符串表快照。此后 V11-26 依赖复查移除了
> `accesskit` 特性，**当前二进制已不再含 `accesskit`**（其余仍命中）——见 §2.5。

**结论：ADR-001 选 egui + wgpu 的体积代价是可控的**，6.19 MiB 在 10 MB 红线内。
S8-01 用 `cargo bloat` 做逐 crate 归因，重点看 wgpu 与 naga 各自的占比，
以及是否还有进一步裁剪空间。

**结论：ADR-002 里「syntect 体积击穿预算」这条高风险项已解除**，
不需要「语言集裁剪 20 → 10 → 5」的分级预案，也不需要自研 regex 高亮 fallback。

关键原因：**没有使用 syntect 的 Theme**。领域层 `SpanStyle` 是自有枚举、
配色归表现层，因此只需 scope 语义（`ParseState` + `ScopeStack` + `ScopeSelectors`），
`default-themes` feature 整个关闭，主题 packdump 未被链接。

## 2.3 阶段 08 体积实测（S8-01 出口，`cargo bloat` 逐 crate 归因）

> 环境：Windows / `x86_64-pc-windows-msvc` / rustc 1.99.0 / `--release`（`opt-level="z"` + LTO + `codegen-units=1` + `strip`）。
> 复现：`cargo build --release --bin rustmark` 后 `cargo bloat --release --bin rustmark --crates 25`。
> 测量对象：主二进制 `--bin rustmark`（组合根已接线，LTO 不会误裁依赖）。

| 项 | 实测 | 阈值 | 结论 |
|---|---|---|---|
| **release 文件总大小** | **7,183,360 bytes = 6.851 MiB** | ≤ 10 MB（预警 8 MB） | ✅ 距红线余量 3.15 MiB，距预警线余量 **1.15 MiB** |
| 相对 S6-01 出口（6.19 MiB / 6,491,648 B） | **+0.66 MiB** | – | Stage 06/07 新增表现层（视图 / 快捷键 / diff / 状态栏）的代价，属预期增长 |
| `.text` 段合计 | 3.8 MiB | – | 占文件总量 55.4%，其余为字体 / `.rodata` / 资源 / PE 头 |

**逐 crate 前 20（`cargo bloat --crates 25`）**

| crate | `.text` | 占 `.text` |
|---|---|---|
| std | 853.0 KiB | 22.0% |
| naga | 490.2 KiB | 12.6% |
| wgpu_core | 291.9 KiB | 7.5% |
| regex_automata | 265.1 KiB | 6.8% |
| **rustmark（自有代码）** | **229.7 KiB** | 5.9% |
| egui | 225.4 KiB | 5.8% |
| zune_jpeg | 104.7 KiB | 2.7% |
| hashbrown | 101.9 KiB | 2.6% |
| aho_corasick | 101.1 KiB | 2.6% |
| image_webp | 78.1 KiB | 2.0% |
| image | 73.9 KiB | 1.9% |
| regex_syntax | 73.5 KiB | 1.9% |
| ttf_parser | 69.8 KiB | 1.8% |
| epaint | 64.3 KiB | 1.7% |
| winit | 63.6 KiB | 1.6% |
| pulldown_cmark | 60.4 KiB | 1.6% |
| wgpu | 56.6 KiB | 1.5% |
| toml | 49.7 KiB | 1.3% |
| eframe | 38.9 KiB | 1.0% |
| png | 37.8 KiB | 1.0% |
| 其余 108 个 crate 合计 | 526.3 KiB | 13.5% |

> `cargo bloat` 自述其数字是「guesswork」（按符号前缀归因），量级可信、精确值不可作绝对值使用。
> **syntect / ropey / rfd 均未进前 20**，即各自 `.text` < 37.8 KiB。

**⚠️ 平台覆盖限制（诚实声明）**：以上为 **Windows 单平台**实测。
macOS（`.dmg`）与 Linux（`.AppImage`）产物体积**本机无法构建**（无对应工具链与目标机），
`NFR-PERF-01` 的三平台数字转由 Stage 09 打包后在真机补测；本阶段 CI 门禁（S8-07）按
「各平台各自阈值」判定，不假设 Windows 数字可外推。

## 2.4 v1.1 增强后的体积实测（2026-10-14，**⚠️ 已越过红线，待决策**）

> 环境：Windows / `x86_64-pc-windows-msvc` / rustc 1.99.0 / `--release`（口径与 §2.3 相同）。
> 复现：`cargo build --release --bin rustmark`；产物在 `target/x86_64-pc-windows-msvc/release/rustmark.exe`
> （本机全局 cargo 配置了 `build.target`，CI 布局见 §4）。

| 项 | 实测 | 阈值 | 结论 |
|---|---|---|---|
| **release 文件总大小** | **11,576,320 bytes = 11.04 MiB** | ≤ 10 MB（预警 8 MB） | ❌ **越过红线 1.04 MiB**（见下方待决策） |
| 相对 S8-01 出口（6.85 MiB） | **+4.19 MiB** | – | 两笔功能性增量，逐项列于下表 |

**增量归因（2026-10-13 ~ 10-14 两笔，均有问题记录可查）**

| 增量 | 估算体积 | 内容 | 记录 |
|---|---|---|---|
| 内置字体资产（`include_bytes!`） | **≈ +2.4 MiB**（资产原始字节） | JetBrains Mono / Fira Code / IBM Plex Mono 各 Regular + Noto Sans SC GB2312 子集（1.57MB）+ DejaVuMathTeXGyre 数学兜底（578KB）；TTF 在 `.rodata` 压缩率有限 | 问题记录 022 / 023（V11-22 / V11-21） |
| `ureq` + `rustls` + `ring` 依赖链 | **≈ +1.8 MiB**（差值倒推） | https 图片下载（缺陷 026）：纯 Rust TLS 的代码 + webpki 根证书 | 问题记录 026（V11-25）、`docs/dependencies.md` §1 |
| 其余（甘特图重写、检查/搜索截断修复等代码） | ≈ +0.0~0.1 MiB | 均为小量代码增长 | 问题记录 024 / 025 |

**⚠️ 待决策（不自行处置，理由如下）**：`NFR-PERF-01` 的 10 MB 红线被越过，
但两笔增量都是**用户明确要求的功能**（「内置字体，下拉不再取决于本机装了什么」
「https 图片要能显示」），回退任何一笔都等于撤功能。可选缓解方案（供评审）：

1. **字体改为「首次使用时解包到用户数据目录」或安装器外置资源**：主程序立减
   ≈2.4 MiB 回到 8.6 MiB（预警线之上、红线之下）；代价是「开箱即用」变两步。
2. **CJK 子集从 GB2312 缩到常用 3500 字**：资产 1.57MB → 约 0.8MB；生僻字在
   无系统中文字体的环境下变方框（与内置初衷相悖，不推荐单做）。
3. **TLS 换 `native-tls`（Windows=schannel / macOS=Security.framework）**：
   预计省 ≈1 MiB（ring + webpki 不进二进制）；代价是 Linux 链 OpenSSL（交叉
   编译困难）且三平台行为不再一致。
4. **维持现状**：11.04 MiB 对桌面编辑器仍是轻量级（vs 典型 Electron 100 MiB+），
   红线数字本身来自 v1.0 需求基线，可在 v1.1 评审中修订。

在决策落地前，`NFR-PERF-01` 视为**已知超限、带豁免候选**，不做静默处置。

## 2.5 依赖复查后的复测（2026-10-14，S8-02 依赖复查）

> 口径同 §2.4。本轮变更：移除 `eframe` 的 `accesskit` 特性、收窄 `ahash` 特性
> （详见 `docs/dependencies.md` §6 与问题记录 027）。两次均用同一 `cargo build --release --bin rustmark` 口径。

| 项 | 实测 | 相对 §2.4 | 结论 |
|---|---|---|---|
| **release 文件总大小** | **11,463,680 bytes = 10.93 MiB** | **−112,640 bytes = −110 KiB（−0.97%）** | 仍越红线 0.93 MiB，但已消掉一笔**零功能代价**的浪费 |

- **归因**：`accesskit` 特性此前在 Windows 上链入 accesskit / accesskit_winit / accesskit_windows /
  accesskit_consumer 四个 crate（另含重复版本 `hashbrown 0.15` / `foldhash 0.1`）；
  `ahash` 的 `runtime-rng` 是 `getrandom 0.3.4` 的唯一引入者。二者**均无任何功能使用**。
- **跨平台意义更大**：`Cargo.lock` 同步移除 **60 个包**，其中 Linux 的 `zbus` / `atspi` / `async-*`
  与 macOS 的 `accesskit_macos` 只在对应平台编译，**Linux / macOS 产物的实际缩减会大于本机 Windows 的 110 KiB**
  （待 Stage 09 真机补测）。
- **`NFR-PERF-01` 状态**：**10.93 MiB，仍超 10 MB 红线 0.93 MiB**。§2.4 的四条缓解方案依旧有效，
  本次先把其中「零功能代价」的部分落地，剩余超限仍需在 v1.1 评审中决策。

## 3. 启动耗时分解表

> 复现：`RUSTMARK_STARTUP_TIMING=1 ./rustmark.exe`（release），stderr 输出里程碑。
> 埋点实现见 `src/app/startup_timing.rs`：默认零开销，只有设了该环境变量才记录，
> 以免测量本身污染被测对象。样本：5 次（排除首次磁盘冷缓存后取中位数）。

| 阶段 | 目标 | 实测（S8-03，release） | 结论 |
|---|---|---|---|
| 进程启动 → `main` | – | （未单独埋点，包含在下一行） | – |
| `Services::bootstrap`（配置读盘 + 语法集加载 + 最近文件恢复） | – | **1.8 ms** | ✅ |
| **eframe 建窗口 + wgpu/dx12 初始化** | – | **682 ms** | ⚠️ **占冷启动 99.3%，全部在第三方** |
| `EditorApp::new`（主题解析 + 快捷键 + 草稿扫描） | – | **0.5 ms** | ✅ |
| 首帧提交（骨架可见） | – | **0.7 ms** | ✅ |
| 内容就绪（本场景无文档，等同首帧） | – | 0.7 ms | – |
| **合计（冷启动，p50）** | **≤ 2s**（目标 300–500ms） | **704 ms** | ✅ 达标；⚠️ 未进目标区间 |
| 语法集 dump 加载（`SyntaxSet::load_defaults_newlines`） | ≤ 30ms | **1.97 ms**（release 复测） | ✅ 余量 15 倍 |
| **首个代码块首次高亮**（`LazyContexts` 反序列化） | – | **30.4 ms** ⚠️ | 见 §3.0 已知问题 |

**关键结论：冷启动的大头不是本项目的代码**——自有部分（装配 + `EditorApp::new` + 首帧）
合计约 **3.0 ms**，其余 682 ms 全在 eframe/wgpu 的窗口与渲染器初始化里。
在不更换渲染后端（ADR-001 锁定 wgpu）的前提下，**这一段基本不可优化**。

> ⚠️ 平台限制：以上为 **Windows 单平台**。macOS / Linux 冷启动本机无法实测，转 Stage 09 真机补测。

### 3.0.1 空闲内存实测（S8-04 基线）

> 口径：**Windows Private Working Set**（`NFR-PERF-03` 经 ADR-006 修订后的判定口径），
> release 二进制，打开**空白文档**静置 12s 后取值。
> `WorkingSet64` 因含共享页且噪声可达 ±20 MB，降为**参考值**，不再用于达标判定。

| 项 | 实测 | 阈值（ADR-006） | 结论 |
|---|---|---|---|
| **Private Working Set（判定口径）** | **131,399,680 B = 125.3 MB** | **≤ 150 MB** | ✅ **达标**（余量 19.7 MB） |
| WorkingSet64（参考） | 198,430,720 B = 189.2 MB | – | 仅记录，不作判定（噪声 ±20 MB） |
| PrivateMemorySize64（参考） | 509,558,784 B = 486 MB | – | 含预留提交，非判定口径 |
| PeakWorkingSet64（参考） | 198,430,720 B = 189.2 MB | – | 与静置值相同，无峰值尖刺 |

### 3.0.2 内存归因：**本项目自身的内存贡献 ≈ 0.9 MB**

用「地板探针」把图形栈基线与自有代码分开（测完即删，沿用 §3.1 先例）：
一个**空的 eframe + wgpu 窗口**（不引用 rustmark 任何模块，故 syntect / ropey /
pulldown-cmark 均不参与链接）与完整应用对比：

| 被测对象 | Private Working Set | WorkingSet64 |
|---|---|---|
| 完整应用 `rustmark` | 125.3 MB | 189.4 MB |
| 空 eframe+wgpu 窗口（地板） | 124.4 MB | 186.8 MB |
| **差值（= 本项目全部自有代码 + 全部依赖数据）** | **≈ 0.9 MB** | ≈ 2.6 MB |

**结论**：

1. 空闲内存 ~125 MB（private）几乎**全部是 egui + wgpu + dx12 的固有基线**，
   本项目自身（含 syntect 语法集常驻、ropey、解析器、各类缓存）只贡献 **约 0.9 MB**。
2. 因此 **S8-04 的 §8.3 五项（Rope 唯一缓冲 / LayoutJob LRU 64 / Galley 缓存 /
   纹理缓存 / 撤销栈上限）在本口径下没有可观测的优化空间**——即便把它们全做到极致，
   能省的也是这 0.9 MB 里的一部分。
3. **原 ≤100 MB 阈值与实测基线在数学上冲突**：在 wgpu 后端（ADR-001 锁定）下，
   光是空窗口就要 ~125 MB。**这不是实现缺陷，是需求假设与实测的冲突**——
   已上报需求方，裁定结果为 **ADR-006：阈值改为按平台各自口径，Windows ≤ 150 MB
   （判定口径改为 Private Working Set）**，实测 125.3 MB ✅ 达标
   （见 `docs/adr/ADR-006-memory-threshold-revision.md` 与
   `docs/archive/stage-reports/optimization-log.md` OPT-007）。
4. 判定口径改为 Private Working Set 的实测依据：完整应用的 WorkingSet 甚至
   **略低于**空窗口探针（189.2 vs 210.1 MB，另一次 189.4 vs 186.8 MB）——
   说明 WorkingSet 受 OS 换页与共享页影响，噪声可达 ±20 MB，
   **不适合用来判断 1 MB 量级的优化收益**。归因与判定一律以 **Private Working Set** 为准。

> 三口径不可横向比较（Windows Private WS / macOS footprint / Linux VmRSS），
> 门禁按「各平台各自阈值」判定；macOS 与 Linux 本机无法实测，转 Stage 09 真机补测，
> 且**补测时同样先做地板探针**再定阈值（ADR-006 待办）。

### 3.0 阶段 05 性能实测（`cargo test --release --test integration -- --nocapture measure`）

| 指标 | 目标 | 实测 | 结论 |
|---|---|---|---|
| 10KB 文档解析 | ≤ 5 ms（`NFR-PERF-06`） | **p50 0.11 ms / p95 0.17 ms** | ✅ 余量约 30 倍 |
| 1MB 文档解析 | 可编辑不卡顿 | **p50 17.7 ms**（一次性全量） | ✅ |
| 语法集冷加载 | ≤ 30 ms | **2.0 ms** | ✅ |
| 20 语言高亮（**热**，上下文已缓存） | – | **58 µs/语言** | ✅ 远低于 16 ms 帧预算 |
| 20 语言高亮（**冷首调**合计） | – | **493 ms**（≈24 ms/语言） | ⚠️ 见下 |

**⚠️ 已知问题：首次使用某种语言时要现场反序列化它的语法上下文（约 25–30 ms/语言）。**
这是 syntect 的 `LazyContexts` 机制，不是本项目的实现缺陷。
若用户在一篇文档里用到 20 种语言，理论上会累计约 0.5 s 的首次卡顿。

处置（阶段 08，S8-03）：启动后利用空闲帧**预热**常用语言
（§8.2「非关键路径异步初始化」），把这段延迟移出用户可感知的路径。
本阶段已按 ADR-003 把语法集加载做成 `OnceLock` 全局单例，
避免多处构造高亮器时重复解压 packdump（二次构造实测 100 ns）。

### 3.2 阶段 08 编辑响应实测（S8-05，`cargo test --release --test presentation/integration -- --nocapture measure`）

| 指标 | 目标 | 实测 | 结论 |
|---|---|---|---|
| 10KB 全文高亮 · **span 生成**（syntect） | ≤ 10 ms（`NFR-PERF-05`） | **p50 56.5 ms / p95 59.1 ms** | ❌ **超 5.6 倍**（见下） |
| 10KB 全文高亮 · **排版**（增量，改 1 行） | – | **冷 0.75 ms ｜ 增量帧 p50 0.16 ms / p95 0.51 ms** | ✅ 重建行数 == 1 |
| 编辑区高亮 · **空闲帧**（文档未变） | ≤ 16 ms | **p50 0.01 ms / p95 0.02 ms** | ✅ 见 OPT-010 |
| 编辑区高亮 · **冷**（首次，含语法上下文反序列化） | – | **209.7 ms** | ⚠️ 一次性，与 §3.0「冷首调」同源 |
| 编辑区高亮 · **编辑帧·打字期**（版本刚变，防抖窗口内） | ≤ 16 ms（`NFR-PERF-04`，仅打字帧） | **≈ 0.00 ms**（返回空片段，不进 syntect） | ✅ OPT-011 ① 防抖 |
| 编辑区高亮 · **编辑帧·停手后**（每 150 ms 至多一次全量） | – | **~59 ms**（一次性上色） | ⚠️ FR-03 观感降级可接受 |
| 1MB 文档 · 降级冷首帧 | – | **47.9 ms** | ✅ 一次性 |
| 1MB 文档 · **稳态滚动帧** | ≤ 16 ms | **p50 0.26 ms / p95 1.19 ms** | ✅ |
| 超长单行（27 万字符 / 无换行）· 冷排版 | 不卡死 | **13.5 ms** | ✅ |
| 超长单行 · 改一个字符（整行重排） | 不卡死 | **p50 13.5 ms / p95 14.5 ms** | ✅ 无需行级截断 |

**增量高亮生效（验收「重建行数 == 1」）**：10KB 文档改一行 → `reshaped_lines == 1`、
`cached_lines == 572`，由 `measure_10kb_full_highlight_is_bounded` 与
`highlight_layout_test::editing_one_line_reshapes_only_that_line` 双重断言。

**✅ 已裁定并落地：OPT-011 选 ① 高亮防抖。** 空闲帧已由 OPT-010 降到 0.01 ms；
内容一变时，`MarkdownAppService::highlight` 在 150 ms「静止窗口」内返回空片段
（打字期 ≈ 0.00 ms，不上色），`clock` 推进过窗口后再做一次 syntect 全文高亮
（~59 ms，每 150 ms 至多一次）。代价是连续打字时语法色暂时消失（FR-03 观感降级），
停手即恢复。② 增量高亮（扩领域接口）与 ③ 接受现状均未采用。详见
`docs/archive/stage-reports/optimization-log.md` OPT-011。

#### 3.2.1 编辑帧成本归因（`measure_markdown_highlight_parse_vs_classify`）

把一次 10KB Markdown 全量高亮拆成两段，判断成本到底在哪：

| 段 | 实测（release, p50） | 占比 |
|---|---|---|
| **syntect 逐行解析**（`ParseState::parse_line` + 区间迭代） | **58.38 ms** | **97.4%** |
| 其余（scope 栈 → `SpanStyle` 判定 + 片段合并 + `Vec` 增长） | ≈ **1.56 ms** | 2.6% |
| 合计（全量 `highlight`） | **59.94 ms** | 100% |

**结论**：成本几乎**全部**在 syntect 的解析本身，不在本项目的判定与组装逻辑。
这直接否决了一条原本看来最省事的候选（「给 scope → 样式加一张备忘表」）：
即便把判定做成零成本，也只省 2.6%，编辑帧仍会在 58 ms 量级。
OPT-011 已选 **① 防抖降级**（打字期返回空片段、停手后一次性全量高亮）；
② 增量高亮（扩领域 `SyntaxHighlighter` 接口）与 ③ 接受并下调指标均未采用——
前者属架构调整、后者放弃帧预算，均不在本阶段可自决范围。

### 3.3 防抖与窗口值校准（S8-06）

| 指标 | 目标 | 实测 | 结论 |
|---|---|---|---|
| 连续输入 20 字符（60 ms/字符，共 1.2 s）预览重解析次数 | **≤ 2** | 修复前 **7 次** → 修复后 **期间 1 次 / 累计 2 次** | ✅（见 OPT-012） |
| 10KB 文档解析 | ≤ 5 ms（`NFR-PERF-06`） | p50 **0.11 ms / p95 0.17 ms**（§3.0） | ✅ 余量 30 倍 |
| 1MB 文档解析 | 可编辑不卡顿 | p50 **17.7 ms**（一次性全量，§3.0） | ✅ |

**校准结论（两个窗口值均维持原值，理由如下）：**

- **预览防抖 150 ms —— 维持。** 它是「停手多久后重解析」：调大到 300 ms 会让预览在停手后再等
  300 ms 才更新，属可感知延迟；调小到 80 ms，则快速打字（80–120 ms/字）会频繁触发重解析，
  失去防抖意义。150 ms 处于「打字时不打扰、停手后立刻跟上」的区间。
- **撤销合并窗口 400 ms —— 维持。** 语义是「与**上一条记录**间隔 > 400 ms 才算新的撤销单元」，
  即**停顿**超过 400 ms 才断开，而非「每 400 ms 切一刀」。日常打字 200–300 ms/字会被合并成
  一整段，一次 Ctrl+Z 撤掉一整句——符合直觉。该语义已由
  `tests/domain/undo_test.rs` 固化（窗口内合并 / 窗口外断开 / 撤销后重置窗口）。

## 3.1 早期体积信号（阶段 02 实测，**不可外推**）

| 项 | 实测 | 说明 |
|---|---|---|
| release 骨架二进制 | **115,200 bytes（0.11 MB）** | 阶段 02 骨架：仅 `shared` 四模块 + 空模块树 |
| 为何如此之小 | eframe / egui / wgpu / syntect 虽已声明为依赖，但**尚未被任何代码引用**，链接器把未使用代码全部丢弃 | 该数字**不代表** egui + wgpu 的真实体积 |
| 何时必须重测 | ~~**S6-01 组合根真正构造 `eframe::App` 之后**~~ → **已完成，见 §2.2**；以及 S8-01 用 `cargo bloat` 逐组件归因 | 首次出现 wgpu 后的数字才是有效基线 |
| **阶段 05 的处置** | 新增 `src/bin/size-probe.rs`：显式构造并调用每个适配器，使链接器无法裁掉它们 | 测得 `.text` 1.1 MiB / 文件 2.1 MiB（见 §2.1） |
| **S6-01 的处置** | 组合根接线完成后主二进制已引用全部依赖，`size-probe` 失去存在价值 → **已删除**，测量对象切回 `--bin rustmark` | 6.19 MiB（见 §2.2） |

> **教训（对后续阶段同样适用）**：任何开启 LTO 的项目，若主二进制存在
> 「尚未接线」的空壳入口，`cargo bloat` 量到的体积**没有参考价值**——
> 它会把「还没用到的依赖」全部裁掉，从而给出「体积达标」的假象。
> 本项目用「临时探针 + 接线后切回」化解：**探针的存在期限应绑定在
> 「主二进制是否已引用该依赖」上，而不是永久保留**。
> 一个长期存在的体积探针会让人误以为主程序还没接线，是个会误导后来者的陷阱。

## 4. 环境基线（已实测）

| 项 | 实测值 | 备注 |
|---|---|---|
| Rust 工具链 | cargo / rustc **1.99.0** | 满足 `CONSTR-01`；`rust-toolchain.toml` 固定 `1.99.0` |
| `edition = "2024"` 可用性 | ✅ 通过 | 探针项目 `cargo check` 0.61s，含 `rust-version = "1.99"` |
| 平台与工具链 | Windows / `x86_64-pc-windows-msvc` | 全局 `~/.cargo/config.toml` 设了 `build.target`，故本机产物在 `target/<triple>/{debug,release}/`；**CI 无该配置，产物在 `target/release/`** —— CI 体积统计已兼容两种布局 |
| 链接器 | `rust-lld` | 来自全局 cargo 配置 |
| 依赖拉取 | 成功，耗时 5m27s | 已下载 wgpu 27 / naga / wgpu-hal 等（体积风险点，见 §3.1） |
| `cargo-deny` / `cargo-bloat` | **已安装**（`cargo-bloat` 0.12.1、`cargo-deny` 0.20.2） | 阶段 05 已用于体积归因 |
| `cargo-nextest` | 未安装 | 阶段 09 按需 |

## 5. 已知环境陷阱（已实测）

| 现象 | 根因 | 处置 |
|---|---|---|
| `rustc` ICE：`rustc_metadata/rmeta/encoder.rs` panic | 并发 cargo 进程导致增量缓存损坏（伴随 `拒绝访问 (os error 5)` 写入 incremental 失败） | `cargo clean` 后重建即恢复，**非源码问题**；避免同时运行多个 cargo 命令 |
| `error: failed to write ... invoked.timestamp: 拒绝访问` | 文件锁竞争（同上） | 重试或 `cargo clean`；release 构建曾失败一次、直接重试即成功 |

## 6. 填写规则

1. 数字只写进本文件，禁止散落在代码注释或 PR 描述；
2. 每次填写须同时记录：命令、采样次数、环境（OS / 工具链版本 / commit）；
3. 阶段 08 的每条优化在 `docs/archive/stage-reports/optimization-log.md` 记录「手段 / 前值 / 后值 / 代价 / 结论」，含放弃项；
4. 三平台内存口径不可横向比较，门禁按「各平台各自阈值」判定。
