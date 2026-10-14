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
| 网络 | `ureq` | **3.4.2**（`default-features=false` + `["rustls"]`） | 3.4.2 | 远程图片下载（http/https，缺陷 025） | 见 metrics §2.4 实测 | rustls 纯 Rust TLS、无系统依赖；取代 S5-11 的手写 `TcpStream` 下载（该方案 https 全部失败） |
| 日志 | `log` | **0.4** | 0.4.x | 基础设施层越界告警 | 极小 | 未列入需求 §5.1，为 S5-01「不 panic、用 `log::warn!`」所必需，在此登记 |
| 缓存 | `ahash` | **0.8**（`default-features=false` + `["std","no-rng"]`） | 0.8.12 | 预览 / 高亮 / 拼写缓存 Map 的哈希器 | 极小 | 缓存键均为内部 `DocumentId`(u64)，关 `runtime-rng` 以免引入 `getrandom`；与 eframe / egui 对 ahash 的用法一致（§6） |
| 测试 | `tempfile` | **3** | 3.27 | 仅 dev-dependency（原子保存、仓储测试） | 不进 release | 阶段 05 起使用 |

> **eframe 渲染后端（实测）**：`eframe 0.33.3` 的 `default` 后端是 **`glow`（OpenGL）而非 wgpu**。
> 需求 §5.1 要求 wgpu，故 `Cargo.toml` 中关闭 `default-features` 并显式启用
> `["default_fonts", "wgpu", "x11", "wayland"]`，同时避免把两个渲染后端都编译进产物。
> 该组合已通过 `cargo check` 验证（`egui-wgpu 0.33.3` 正常参与编译）。
>
> **2026-10-14 复查移除 `accesskit`**：accesskit 仅用于向屏幕阅读器暴露语义树，
> 本应用代码零引用，键盘导航 / IME / 焦点链路全部走 egui 自身实现，禁用不影响任何交互；
> 却可卸掉整条跨平台 accessibility 依赖（Windows 编译期 4 个 crate，跨平台锁文件 60 个包，
> 详见 §6）。`default_fonts` 保留：UI 需要 egui 内嵌的比例字体（Ubuntu-Light）——本项目
> 只自带等宽 / CJK / 数学字体，去掉默认字体会让按钮与标签失去可用字形。

## 2. 明确不引入

| crate | 理由 | 依据 |
|---|---|---|
| `comrak` | 二进制 2,767 KiB，为 pulldown 的 6 倍 | §5.2 |
| `onig` / syntect 的 `default-onig` | C 依赖，编译与维护成本高 | §5.2、ADR-002 |
| `uuid` | ID 用 `u64` 单调计数器即可，省体积 | S2-04 |
| 任何 PDF 引擎 | 击穿体积预算 | ADR-004 |
| 任何遥测 / 自动更新库 | 依赖与隐私开销 | S9-08 |
| `iced` / Tauri / Electron | 架构已定 egui | ADR-001 |
| `accesskit`（eframe 特性） | 屏幕阅读器语义树，本应用零引用；启用它会链入跨平台 accessibility 全家（含 Linux `zbus`/`atspi`、macOS `accesskit_macos`） | 2026-10-14 依赖复查（§6） |

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

## 6. 依赖复查（2026-10-14，S8-02 之后首次全量复核）

> **目的**：扫出「未被实际使用」或「可用更小范围替代」的直接依赖，减少产物体积。
> **方法**：逐 crate 反查 `src/` + `tests/` 引用点 → `cargo tree -e features -i <crate>`
> 反查 feature 来源 → 同一 release 口径（`cargo build --release --bin rustmark`）前后对比。

### 6.1 已处置（本轮实际变更）

| 项 | 变更 | 理由（证据） | 效果 |
|---|---|---|---|
| `eframe` 特性 | 移除 `"accesskit"` | 全仓 `grep accesskit` **零命中**；该特性只向辅助技术暴露语义树，不参与键盘 / IME / 焦点 | Windows 产物 **−110 KiB**（11.04 → 10.93 MiB）；跨平台锁文件 **−60 个包**：accesskit 全家（windows/unix/macos/consumer/winit）+ Linux `zbus`/`atspi`/`async-*` 栈 + 重复版本 `hashbrown 0.15` / `foldhash 0.1` |
| `ahash` 特性 | `"0.8"` → `default-features=false` + `["std","no-rng"]` | 三处缓存 Map（highlight / render / spell）键**全是 `DocumentId`(u64)**，非攻击者可控，无需 HashDoS 防护；`runtime-rng` 是依赖树中 `getrandom 0.3.4` 的**唯一**引入者 | `getrandom 0.3.4` 退出构建图（`cargo tree -i getrandom@0.3.4` → nothing to print）；种子改由 ASLR + 实例计数器派生（ahash 文档所述「最弱但可用」档），对缓存语义无影响 |

### 6.2 复核后确认「保持现状」（记录决策，避免反复讨论）

| crate / 特性 | 结论 | 理由 |
|---|---|---|
| `pulldown-cmark` 的 `html` | **必需** | `infrastructure/export/pulldown_html_exporter.rs` 使用 `pulldown_cmark::html::push_html`（S5-09 HTML 导出），非冗余特性 |
| `image` 的 5 个格式特性 | **必需** | `asset_loader::policy::ALLOWED_MIME` 白名单恰为 png/jpeg/gif/webp/bmp，逐一对应用户可插入的图片类型，无格式可再砍 |
| `toml` 的 `display` | **必需** | `recent_files_store` / `toml_preferences_repository` 用 `toml::to_string_pretty` 写盘 |
| `syntect` 的 `default-syntaxes` | **保留** | 高亮依赖内嵌语法集；替换为 curated 子集属 ADR-002 方案 B，非本次范围 |
| `log` | **保留** | 真实使用（`application/document/registry.rs` 的 `log::warn!`），且已被 eframe / ureq 引入，无额外体积 |
| `anyhow` | **保留** | 仅 `app.rs` / `main.rs`，符合 `CONSTR-10`；体积极小 |
| `ureq` 的 `rustls` | **保留** | `ureq` 的 `rustls` = ring 提供者 + webpki 根，已是该库最小的合规 TLS 组合；换 `native-tls` 会在 Linux 引入 OpenSSL（即 metrics §2.4 方案 3，未采纳） |
| `rfd` 默认特性、`eframe` 的 `x11` / `wayland` | **保留** | 均为 **Linux 目标专属**依赖（target-gated），对 Windows / macOS 产物零影响，去掉只会伤 Linux |
| `eframe` / `egui` 的 `default_fonts` | **保留** | 项目自带字体只有等宽 + CJK + 数学，**无比例西文字体**；去掉默认字体后按钮 / 标签会缺字形 |

> 备注：`getrandom 0.3.4` 在 `Cargo.lock` 中可能残留一条**陈旧条目**（cargo 未即时回收），
> 但 `cargo tree` 已确认它不在构建图中、不进产物，下次 cargo 自然重写锁文件时会被回收。
> 本次未跑 `cargo update`（全量重解析会冒改版本的风险），仅动 `Cargo.toml` 的 feature 范围。
