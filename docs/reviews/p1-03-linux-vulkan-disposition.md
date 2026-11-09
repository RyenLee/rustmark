# P1 #3 处置记录：Linux 无 Vulkan 驱动时启动失败

> 来源：[全维度架构评审 2026-11-01](architecture-review-2026-11-01.md) 优先级表 #3、§2 / §8 的同名条目。
> 处置人：韩渡川（DevOps）· 基准 HEAD `601fb65` · 2026-11-01
> 性质：**代码加固 + 文档登记 + 体积决策量化**。未改 wgpu feature 集、未启用 gles、未新增依赖。

---

## 1. 结论先说

| 问题 | 结论 |
|---|---|
| 无 Vulkan 驱动时是 `Err` 还是 panic？ | **`Err`**。`eframe::run_native` 返回 `Err(eframe::Error::Wgpu(…))`，进程不 panic |
| panic hook（`crash_log`）能收到吗？ | **不能**。没有 panic，`panic = "abort"` 也永远等不到 hook |
| 错误文本是什么？ | `WGPU error: No suitable graphics adapter found; … vulkan drivers/libraries could not be loaded, …` |
| Linux release 用户看到什么？ | stderr 上这句技术文案 + eframe 自己的一条 `log::error`；**缺少「装什么、怎么查」** |
| Windows release 用户看到什么？ | **什么都没有**（`windows_subsystem = "windows"` 无控制台），表现为「双击没反应」 |
| 本次加固 | 把该错误翻成可操作诊断（按平台给动作），并**落盘**到 `crash/`（零新增依赖、零体积增长） |
| gles 回退要不要恢复？ | 见 §5：估算 Linux 产物回到 **≈13.79 MB（13.15 MiB）**，仍在现 13.5 MiB 红线内但余量只剩 0.35 MiB。**未实施**，等 Linux 真机验证（§6）后再决策 |

---

## 2. 失败路径：逐跳源码依据

以下行号对应本机 cargo registry 的实际版本：`eframe 0.37.0` / `egui-wgpu 0.37.0` /
`wgpu 30.0.1` / `wgpu-core 30.0.1` / `wgpu-types 30.0.1`。

**① `wgpu::Instance::new` 不 panic。**

只在「当前目标平台一个后端 feature 都没编进来」时 panic
（`wgpu-30.0.1/src/api/instance.rs:66`，即 DEF-03 的场景，已由 `Cargo.toml` 的按平台
feature 修掉，并由 `tests/startup_smoke.rs` 守住）。

驱动缺失走的是另一条路：`wgpu-core` 尝试初始化 Vulkan backend，失败时只记一条
`log::debug!("Instance::new: failed to create {:?} backend: {:?}")`
（`wgpu-core-30.0.1/src/instance.rs:140`），随后该后端**不进 `instance_per_backend`**，
于是也不出现在 `active_backends()`（同文件 `:650`）里。

**② `request_adapter` 返回 `Err(RequestAdapterError::NotFound { … })`。**

`egui-wgpu` 的 `request_adapter` 用 `?` 把它变成 `WgpuError::RequestAdapterError`
（`egui-wgpu-0.37.0/src/lib.rs:46`、`:132-196`）。`Display` 由
`wgpu-types-30.0.1/src/adapter.rs:251,276-330` 生成，逐个后端解释原因：

| 情形 | 措辞 |
|---|---|
| 编译进来了、也被请求了，但实例没建起来（**无 libvulkan / 无驱动**） | `vulkan drivers/libraries could not be loaded`（`:310`） |
| 编译进来了，但 enumeration 为空 | `vulkan found no adapters` |
| 当前平台没编这个后端 | `vulkan support not compiled in` |

首句固定是 `No suitable graphics adapter found; `（`:287`）。所以**本次实测预期文本**：

```text
WGPU error: No suitable graphics adapter found; noop support not compiled in,
vulkan drivers/libraries could not be loaded, metal not requested,
dx12 support not compiled in, gl not requested
```

（`metal` / `dx12` / `gl` 的措辞取决于 `InstanceDescriptor::backends`：
eframe 默认 `Backends::all()`，故被「请求过」但没编译 → `support not compiled in`。）

**③ eframe 把它从事件循环里带出来，不 panic。**

`WgpuWinitApp::resumed` 返回 `crate::Result<EventResult>`（`eframe-0.37.0/src/native/wgpu_integration.rs:459`），
`WinitAppWrapper::handle_event_result` 收到 `Err` 后写一条
`log::error!("Exiting because of error: {err}")`（`eframe-0.37.0/src/native/run.rs:178`）、
存进 `return_result`、退出事件循环，最终从 `run_native` 返回
（`eframe::Error::Wgpu`，`eframe-0.37.0/src/lib.rs:504` 枚举 / `:610` 的 `Display` 前缀 `WGPU error: `）。

**④ `app::run()` 原本怎么处理？**

`src/app.rs` 里是 `frame::run(services)?`——错误经 `anyhow` 直接冒到 `main`，
`main` 返回 `Err` 时 std 会打 `Error: <Debug>` 到 stderr 并以退出码 1 结束。

| 平台 | 用户实际感知 |
|---|---|
| Linux / macOS（终端启动） | stderr 上出现 eframe 的 `log::error` 那一行（`env_logger` 默认级别 `warn`，能收到 error）+ std 的 `Error: …`。**技术文案，不含任何处置动作** |
| Windows release（双击） | `windows_subsystem = "windows"`（`src/main.rs`）无 console，两条输出**全部丢失**，表现为「双击没反应」 |

---

## 3. 本次加固（零体积代价）

| 改动 | 文件 |
|---|---|
| 新增诊断模块：`classify`（纯函数）/ `diagnose`（纯函数）/ `HostPlatform` 参数化平台 | `src/app/startup_diagnosis.rs`（新） |
| 复用 `crash_log` 的报告结构与写盘：`CrashReport::from_startup_failure` + `write_startup_failure` | `src/app/crash_log.rs` |
| 在 `run()` 接住 `frame::run` 的 `Err` → 生成诊断 → 落盘 → 以可读文案返回 | `src/app.rs` |
| 单测：分类 3 例、平台文案 4 例、骨架/自检项断言 | `src/app/startup_diagnosis.rs`（`mod tests`） |
| 单测：非 panic 写盘路径（前缀、正文、版本） | `tests/app/crash_log_test.rs` |

**为什么不新增依赖、不动 wgpu feature**：诊断只需要字符串处理 + 已有的 `std::fs::write`。
`write_startup_failure` 复用 `write_report`，因此**自动继承** `crash-*.txt` 前缀与
「最多 10 份」的保留策略（`prune_reports`），不必写第二套清理逻辑。

**体积影响**：新增代码全部是字符串常量与几个分支（约 200 行源码 + 文案），
在 `opt-level="z"` + LTO + strip 下预计 < 8 KiB；未新增任何 crate，
`Cargo.lock` 零变化。**该结论需在下次 Linux 真机/CI 体积门禁上复核**（Windows 本机
体积不受本改动约束，红线仍是 11 MiB）。

**分类的降级设计**：`classify` 只在看到 eframe 的 `WGPU error:` 前缀时才认为与图形栈
相关；`No suitable graphics adapter found` 再细分到「装驱动」分支。若将来 wgpu 升级改了
错误文案，最多降级到「GPU 初始化失败，请更新驱动」——**不会**降级成「与图形无关」，
不会把用户引向错误方向。

---

## 4. 文档

| 位置 | 内容 |
|---|---|
| `README.md` §2 系统依赖（Linux） | 原文写「Vulkan 或 OpenGL」——**与代码不符**（无 GL 回退），改为「Vulkan 必需」+ 三发行版安装命令 + `vulkaninfo` 自检 |
| `README.md` §4 已知限制 第 10 条 | 登记「Linux 必须有可用 Vulkan 驱动」，含两种平台的表现差异与文档指路 |
| `docs/guides/troubleshooting.md` §6（新） | 现象 → 原因 → 处置（安装表 / 自检 / 虚拟机会话）→ **可复现验证步骤**（容器里卸驱动跑 release） |
| `docs/guides/troubleshooting.md` §7 速查表 | 加两行（Linux 启动即退出 / Windows 双击没反应） |

---

## 5. gles 决策量化（**估算，未实施**）

### 5.1 数据来源（都是实测，不是推算）

| 测量 | 值 | 出处 |
|---|---|---|
| Linux、`vulkan + gles` 双开 | **13,792,824 B = 13.154 MiB** | `8e70c3d` 提交信息（"Linux 段原双开 vulkan + gles，release 实测 13,792,824 B"） |
| Linux、`vulkan` 单开 | **13,371,056 B = 12.752 MiB** | `docs/spec/metrics.md` §2.9（2026-11-01 Linux 首次 CI 实测，ubuntu-24.04） |
| 现 Linux 红线 / 预警线 | 14,155,776 B（13.5 MiB）/ 13,631,488 B（13.0 MiB） | `scripts/size_gate.sh:39-48` |
| 现 Windows 红线 | 11,534,336 B（11 MiB），实测基线 10,589,696 B | 同上 / `metrics.md` §2.1 |

### 5.2 gles 的边际体积

```
13,792,824（vulkan+gles） − 13,371,056（vulkan） = 421,768 B ≈ 0.40 MiB
```

**这是下界**：两个测点之间还夹着 `d86e17f`（F-01~F-04 安全修复，`src`/`crates` 净增
215 行），它只会让**后**那个数偏大，从而让差值偏小。故真实 gles 增量略大于 421,768 B，
量级仍是 **0.4 MiB 级**，不是 `Cargo.toml` 注释里暗示的 2.7 MB。

> 附带发现（**文档口径问题，本次未改 `Cargo.toml`**）：`Cargo.toml:325` 的注释
> 「双开 vulkan + gles 会让 Linux release 比 Windows 基线大 2.7 MB」把两件事混成了
> 一件——2.7 MB 是 **Linux 总计 vs Windows 基线** 的差（含 x11 + wayland + vulkan 栈），
> 不是 gles 的边际成本。将来若有 Linux 数据复核，建议顺手把这句改准。

### 5.3 恢复 gles 后的产物与红线

| 项 | 估算值 | 备注 |
|---|---|---|
| 恢复后的 Linux 产物 | `13,371,056 + 421,768 ≈` **13,792,824 B = 13.154 MiB** | 与 `8e70c3d` 的历史实测值一致（交叉印证） |
| 距现红线 13.5 MiB | **362,952 B（0.346 MiB）** | **不破线，但余量只有 §2.9 设计窗口（约 0.75 MiB）的一半** |
| 现预警线 13.0 MiB | 13,631,488 < 13,792,824 | **常态进入预警**（`size_gate` 只告警不失败） |

**两条可选路线**：

- **路线 A（不动红线）**：红线保 13.5 MiB，接受 0.346 MiB 余量 + 常态预警。
  适合「先恢复可用性、接受门禁长期黄」。
- **路线 B（推荐：保 0.75 MiB 窗口）**：红线 13.5 → **14.0 MiB（14,680,064 B）**、
  预警 13.0 → **13.5 MiB（14,155,776 B）**。此时余量 887,240 B = 0.846 MiB，
  且不会常态预警。代价是 Linux 档比现在的 13.5 MiB 再宽 0.5 MiB。

> 两条路线都要同步改三处（改动方式见 `docs/spec/thresholds.baseline.yaml` 现有机制）：
> `scripts/size_gate.sh` 的 Linux 分支、`docs/spec/metrics.md` §2.9、threshold 基线。
> **本记录不实施**——先按 §6 拿到真机结论，再决定值不值得为「无 Vulkan 机器」付这 0.4 MiB。

### 5.4 恢复 gles 并不等于自动回退

即便把 gles 编进来，`wgpu` 的默认行为是「在已编译后端集合里挑一个可用适配器」，
顺序不保证是 vulkan 优先。要真正实现评审建议的「运行时探测 vulkan 不可用 → 回退 gles」，
还需要 `WgpuConfiguration::native_adapter_selector` 或 `WGPU_BACKEND` 环境变量的显式编排，
属于**第二个改动面**（`src/app/frame/loop.rs` 的 `NativeOptions` 装配）。
这条工作量本次未计入估算，恢复 gles 时请一并评估。

---

## 6. 仍需 Linux 真机验证的事项（本次未做，环境所限）

开发机为 Windows，**WSL 被安全策略禁用、无 Docker**，因此以下三条只能待 Linux 环境执行：

1. **确认失败路径的实机表现**：在无 `mesa-vulkan-drivers` 的 Linux 上跑 release，
   预期 stderr 首行为 `rustmark 启动失败：找不到可用的图形适配器（GPU）`，
   且建议里出现 `mesa-vulkan-drivers` 与 `vulkaninfo`。
   （失败路径本身已可从源码逐跳确认，见 §2；待确认的是**文案确实出现在屏幕上**、
   以及 `raw` 字符串与 `classify` 的判据逐字吻合。）
2. **确认诊断落盘**：`ls data/crash/`（ADR-014 后的默认位置；若应用目录不可写
   或跑的是 AppImage，则回退到 `~/.config/rustmark/crash/`）出现
   `crash-<时间>-<PID>.txt`，内容含「位置：<启动期：eframe 返回 Err，不是 panic>」。
3. **体积复核**：装好后跑一次 `bash scripts/size_gate.sh`，把 Linux 实测值记进
   `metrics.md` §2.9，与 §3 的「< 8 KiB」预估对照。

步骤的可复制版本写在 `docs/guides/troubleshooting.md` §6「可复现的验证步骤」。

---

## 7. 未做的事（明确边界）

- 未恢复 gles、未改 wgpu feature 集（P1 的**决策**部分等 §6 数据）。
- 未改 Linux 红线/预警线（同上）。
- 未引入消息框 / 原生对话框（Windows 上「双击没反应」的最优解是弹窗，
  但需要平台 API 依赖，违反「不新增依赖」）。
- 未触碰评审报告中的 P2 项（`asset_loader.rs`、`fs_document_repository.rs`、
  `tests/field_visibility_guard_test.rs`、`tools/arch_check.rs` 等）。
