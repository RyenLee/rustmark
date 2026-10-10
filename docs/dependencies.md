# 依赖清单与版本锁定

> 关联需求：§5.1、§5.2、§8.1；关联任务：S1-08、S2-01、S5-01~03、S8-02
> 核实方式：crates.io API 查询，日期 2026-10-06。**新增依赖须登记体积贡献并经 ADR 追加评审。**

---

## 1. 直接依赖（锁定版本）

| 类别 | crate | 锁定版本 | 仓库最新 | 用途 | 体积预算 | 备注 |
|---|---|---|---|---|---|---|
| UI | `egui` | **0.33.3** | 0.36.2 | 立即模式 GUI | 3–4 MB（含 eframe） | 需求 §5.1 锁定；§10 以其 API 不稳定为由锁定，升级需单独评估 |
| UI | `eframe` | **0.33.3** | 0.36.2 | 应用框架与窗口 | 同上 | 随 egui 联动 |
| 解析 | `pulldown-cmark` | **0.13**（`0.13.4`） | 0.13.4 | Markdown 解析 | ≤ 0.5 MB | `default-features = false` + `["html","simd"]`；**GFM 为运行期 Options，非 feature** |
| 高亮 | `syntect` | **5.3.0** | 5.3.0 | 代码语法高亮 | ≤ 1.5 MB | `default-fancy`（纯 Rust，无 onig）；语法集方案见 ADR-002 §2.1 |
| 文本缓冲 | `ropey` | **1.6.1** | 1.6.1（2.0.0-beta 存在） | Rope 文本结构 | ≤ 0.2 MB | 不用 beta |
| 对话框 | `rfd` | **0.17.2** | 0.17.2 | 原生文件/目录对话框 | 计入 0.5 MB | 三平台原生实现 |
| 序列化 | `serde` | **1** | 1.x | DTO / 配置序列化 | 计入 0.5 MB | 仅启用 `derive` |
| 配置 | `toml` | **0.9.6** | 1.1.6 | 配置文件读写 | 计入 0.5 MB | 需求 §5.1 指定 0.9.6；若 0.9 线维护问题再评估升级 |
| 错误 | `thiserror` | **2** | 2.0.21 | 领域/应用层错误枚举 | 极小 | 领域层专用 |
| 错误 | `anyhow` | **1** | 1.x | 仅 `app.rs` / `main.rs` | 极小 | **禁止进入领域层**（`CONSTR-10`） |
| 日志 | `log` | **0.4** | 0.4.x | 基础设施层越界告警 | 极小 | 未列入需求 §5.1，为 S5-01「不 panic、用 `log::warn!`」所必需，在此登记 |
| 测试 | `tempfile` | **3** | 3.27 | 仅 dev-dependency（原子保存、仓储测试） | 不进 release | 阶段 05 起使用 |

> **eframe 渲染后端（实测）**：`eframe 0.33.3` 的 `default` 后端是 **`glow`（OpenGL）而非 wgpu**。
> 需求 §5.1 要求 wgpu，故 `Cargo.toml` 中关闭 `default-features` 并显式启用
> `["accesskit", "default_fonts", "wgpu", "x11", "wayland"]`，同时避免把两个渲染后端都编译进产物。
> 该组合已通过 `cargo check` 验证（`egui-wgpu 0.33.3` 正常参与编译）。

## 2. 明确不引入

| crate | 理由 | 依据 |
|---|---|---|
| `comrak` | 二进制 2,767 KiB，为 pulldown 的 6 倍 | §5.2 |
| `onig` / syntect 的 `default-onig` | C 依赖，编译与维护成本高 | §5.2、ADR-002 |
| `uuid` | ID 用 `u64` 单调计数器即可，省体积 | S2-04 |
| 任何 PDF 引擎 | 击穿体积预算 | ADR-004 |
| 任何遥测 / 自动更新库 | 依赖与隐私开销 | S9-08 |
| `iced` / Tauri / Electron | 架构已定 egui | ADR-001 |

## 3. 开发工具（dev-dependencies / 全局工具）

| 工具 | 状态 | 用途 | 安装 |
|---|---|---|---|
| `cargo-deny` | **未安装**（安装被取消） | 许可证与漏洞门禁 | `cargo install cargo-deny --locked` |
| `cargo-bloat` | **未安装**（安装被取消） | 体积分析（S8-01 起必需） | `cargo install cargo-bloat --locked` |
| `cargo-nextest` | 未安装 | 测试提速（阶段 09 按需） | `cargo install cargo-nextest --locked` |
| 覆盖率工具 | 未定 | `cargo-llvm-cov` 或 `cargo-tarpaulin` | 阶段 09（S9-02） |
| `proptest` | 未引入 | 属性测试（S9-03，可选） | 仅 dev-dependencies，不得进 release |

> `cargo-bloat` 是阶段 08 三项硬指标中"体积"的唯一测量手段，`cargo-deny` 是阶段 02 CI 的门禁步骤，二者需在阶段 02 开工前补齐。

## 4. 许可证合规

- 本项目以 **MIT / Apache-2.0 双许可**发布（§2.3）；
- `deny.toml` 已配置允许列表（MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode）与 `unknown-registry = deny`；
- 若采用 ADR-002 §2.1 的方案 B（自建 curated `.sublime-syntax`），需额外核查语法源文件的许可证并补入 `NOTICE` 与 `deny.toml`；
- 每次依赖变更须重跑：`cargo deny check licenses advisories bans sources`。

## 5. 版本锁定说明

| 项 | 说明 |
|---|---|
| egui / eframe 锁 0.33.3 | 需求 §5.1 指定；最新 0.36.2 存在跨版本 API 变更。升级须单独立项、回归阶段 06 全部视图 |
| toml 锁 0.9.6 | 需求 §5.1 指定；最新为 1.1.6，升级需评估配置格式兼容性 |
| 其余 | 按需求文档 §5.1 锁定；补丁级更新允许，次版本升级需登记 |
