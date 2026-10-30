# 依赖清单与版本锁定

> 关联需求：§5.1、§5.2、§8.1；关联任务：S1-08、S2-01、S5-01~03、S8-02
> 核实方式：crates.io API 查询，日期 2026-10-06；**2026-10-30 按 7 份 manifest + `Cargo.lock` 逐条复核（V11-67 依赖统一收口），修正了 `toml` / `pulldown-cmark` / `egui`·`eframe` 三处陈旧记录，并补入 `image` / `wgpu` 两行**。**新增依赖须登记体积贡献并经 ADR 追加评审。**

---

## 1. 直接依赖（锁定版本）

| 类别 | crate | 锁定版本 | 仓库最新 | 用途 | 体积预算 | 备注 |
|---|---|---|---|---|---|---|
| UI | `egui` | **0.37.0** | 0.37.0 | 立即模式 GUI | 3–4 MB（含 eframe） | 2026-10-16 由 0.33.3 升级（评估报告见 `archive/stage-reports/egui-0.37-upgrade.md`）；REL-03 起 **`default_fonts` 与 `monochrome_emoji_fonts` 均关闭**：5 个内嵌字体（1,417,504 B）改为随发行包分发，运行期由 `infrastructure::fonts` 装配回比例 / 等宽基础族（见 metrics §2.7） |
| UI | `eframe` | **0.37.0** | 0.37.0 | 应用框架与窗口 | 同上 | 随 egui 联动；渲染后端特性 `wgpu` → **`wgpu_no_default_features`**（见下方注） |
| 解析 | `pulldown-cmark` | **0.13**（锁 `0.13.5`） | 0.13.5 | Markdown 解析 | ≤ 0.5 MB | `default-features = false` + `["html"]`（仅 HTML 导出需要 `push_html`）；**不带 `simd`**（此前本行误记，manifest 从未声明）、不带 `getopts`；**GFM 为运行期 Options，非 feature** |
| 高亮 | `syntect` | **5.3.0** | 5.3.0 | 代码语法高亮 | ≤ 1.5 MB | `default-features = false` + `["parsing","default-syntaxes","dump-load","regex-fancy"]`；`regex-fancy` 走 fancy-regex（纯 Rust，不链 onig），`dump-load` 连带 `dump-create`/`flate2`/`bincode`（实测 `cargo tree -e features -i syntect`）；语法集方案见 ADR-002 §2.1 |
| 文本缓冲 | `ropey` | **1.6.1** | 1.6.1（2.0.0-beta 存在） | Rope 文本结构 | ≤ 0.2 MB | 不用 beta |
| 对话框 | `rfd` | **0.17.2** | 0.17.2 | 原生文件/目录对话框 | 计入 0.5 MB | 三平台原生实现 |
| 序列化 | `serde` | **1** | 1.x | DTO / 配置序列化 | 计入 0.5 MB | 仅启用 `derive` |
| 配置 | `toml` | **1.1.8**（锁 `1.1.8+spec-1.1.0`） | 1.1.8 | 配置文件读写 | 计入 0.5 MB | manifest 实际声明 `1.1.8`（此前本行写的 0.9.6 已过期）；开默认特性 → 实启用 `default`（`display`/`parse`/`serde`/`std`），`to_string_pretty` 依赖 `display`，见 §6.2 |
| 错误 | `thiserror` | **2** | 2.0.21 | 领域/应用层错误枚举 | 极小 | 领域层专用 |
| 错误 | `anyhow` | **1** | 1.x | 仅 `app.rs` / `main.rs` | 极小 | **禁止进入领域层**（`CONSTR-10`） |
| 网络 | `ureq` | **3.5.0**（`default-features=false` + `["rustls-no-provider","_ring"]`） | 3.5.0 | 远程图片下载（http/https，缺陷 025） | 见 metrics §2.7 | rustls 纯 Rust TLS、无系统依赖；取代 S5-11 的手写 `TcpStream` 下载（该方案 https 全部失败）。REL-03 起不启 `rustls-webpki-roots`（省 ~400 KB 裸 DER），根证书改由下一行的 `rustls-native-certs` 运行期读取 |
| 网络 | `rustls-native-certs` | **0.8** | 0.8.5 | 读平台信任库的根证书（REL-03 配套） | ≈ 10 KB 代码 | 许可证 Apache-2.0 OR ISC OR MIT（均在 `deny.toml` 白名单）。纯 Rust：Windows 读注册表证书库、macOS 经 Security.framework、Linux 读 `/etc/ssl/certs`，**不链 libssl**（NFR-COMPAT-03 不受影响） |
| 日志 | `log` | **0.4** | 0.4.x | 基础设施层越界告警 | 极小 | 未列入需求 §5.1，为 S5-01「不 panic、用 `log::warn!`」所必需，在此登记；后端见下一行 |
| 日志 | `env_logger` | **0.11**（`default-features=false` + `["humantime"]`） | 0.11.x | `log` 的 stderr 后端（F-2 前无后端，12 个告警调用点全部静默丢弃） | 极小 | **不启 `regex`**：依赖树此前无 `regex`（仅 fancy-regex → regex-automata），模块级过滤不依赖正则；不启 `termcolor`：着色交给终端。默认级别 warn，`RUST_LOG` 可调 |
| 缓存 | `ahash` | **0.8**（`default-features=false` + `["std","no-rng"]`） | 0.8.12 | 预览 / 高亮 / 拼写缓存 Map 的哈希器 | 极小 | 缓存键均为内部 `DocumentId`(u64)，关 `runtime-rng` 以免引入 `getrandom`；与 eframe / egui 对 ahash 的用法一致（§6） |
| 图片 | `image` | **0.25**（锁 `0.25.10`） | 0.25.10 | 预览区图片解码（root 视图 + infra 落盘/缩放） | 已计入 eframe 预算 | `default-features = false` + `["png","jpeg","gif","webp","bmp"]`，与 `asset_loader::policy::ALLOWED_MIME` 白名单逐一对应；**不启 `svg`**（SVG 可携带 `<script>`，见 asset_loader 安全说明） |
| 渲染 | `wgpu` | **30.0.1** | 30.0.1 | 仅补开平台后端 feature（应用代码不直接调用） | 计入 eframe 预算 | `default-features = false` + `["std"]`；后端按平台各开一个（Windows `dx12` / macOS `metal` / 其它 `vulkan`+`gles`），见 §1 注与 §7 |
| 测试 | `tempfile` | **3** | 3.27 | 仅 dev-dependency（原子保存、仓储测试） | 不进 release | 阶段 05 起使用 |

> **eframe 渲染后端（实测）**：`eframe` 的 `default` 后端是 **`glow`（OpenGL）而非 wgpu**。
> 需求 §5.1 要求 wgpu，故 `Cargo.toml` 中关闭 `default-features` 并显式启用后端特性，
> 同时避免把两个渲染后端都编译进产物。该组合已通过 `cargo check` 验证。
>
> **2026-10-16（egui 0.33.3 → 0.37.0）后端特性改名**：0.37 起 `eframe` 的 `"wgpu"` 特性会
> **连带**打开 `egui-wgpu/default` → `wgpu/default`（等于各平台后端全开，体积回到 DEF-03 之前的
> 水平）。故改启用 **`wgpu_no_default_features`**：只引入 `dep:wgpu` + `dep:egui-wgpu` 而不带默认
> 特性，后端仍由各平台下直接声明的 `wgpu` 按平台各开一个（`dx12` / `metal` / vulkan）。
> 当前 `wgpu` 锁 **30.0.1**（由 27.0.1 升级），`winit` 未变（0.30.13）。
>
> **文本栈换代（0.34 起）**：`epaint` 0.33.3 依赖 **`ab_glyph`**；0.37.0 改为
> **`harfrust` 0.12 + `skrifa` 0.44 + `vello_cpu` 0.2**（整形 / 字形解析 / CPU 光栅化）。
> 字形整形更准（支持 hinting 与字距连字）但**更贵**：实测「27 万字节超长单行」排版由 13.5 ms
> 退到 69.1 ms（约 5×），而常规文档与滚动帧持平或更优——详见评估报告 §3.3。
>
> **2026-10-14 复查移除 `accesskit`**：accesskit 仅用于向屏幕阅读器暴露语义树，
> 本应用代码零引用，键盘导航 / IME / 焦点链路全部走 egui 自身实现，禁用不影响任何交互；
> 却可卸掉整条跨平台 accessibility 依赖（Windows 编译期 4 个 crate，跨平台锁文件 60 个包，
> 详见 §6）。**2026-10-22（REL-03，废止此前口径）**：`default_fonts` 与
> `monochrome_emoji_fonts` **均改为关闭**——这两个特性此前把 Ubuntu-Light / Hack /
> NotoEmoji / emoji-icon-font / egui-icons 五个字体（1,417,504 B）以 `include_bytes!`
> 编进 exe，是 10 MB 未压缩红线（NFR-PERF-01）下最大的一块可省体积。现改为随发行包
> 以 `fonts/` 目录分发（许可证见 `assets/fonts/Fonts-LICENSE.txt`），运行期由
> `infrastructure::fonts::font_definitions` 装回比例 / 等宽基础族，族顺序与
> tweak 缩放逐项对齐原先的内嵌实现，**界面观感不变**；`egui-icons.ttf`（3.4 KB）
> 不随包——本项目对 `egui::special_emojis` 零引用。

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
| `cargo-deny` | **已安装 0.20.2**（本机可用，CI `deny` job 常态执行；2026-10-22 复核） | 许可证与漏洞门禁 | `cargo install cargo-deny --version 0.20.2 --locked` |
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
| egui / eframe 锁 0.37.0 | 需求 §5.1 最初指定 0.33.3，**2026-10-16 已升级到 0.37.0**（评估报告 `archive/stage-reports/egui-0.37-upgrade.md`）；本行此前仍写 0.33.3，2026-10-30 按 manifest 修正。再升级须单独立项、回归阶段 06 全部视图 |
| toml 锁 1.1.8 | manifest 实际声明 `1.1.8`（锁文件 `1.1.8+spec-1.1.0`）；本行此前记 0.9.6 已过期。再升级需评估配置格式兼容性（`Cargo.toml` 之外的落盘格式见 §6.2） |
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
| `ureq` 的 `rustls-no-provider` + `_ring` | **保留** | 纯 Rust TLS（ring 提供者），三平台行为一致；换 `native-tls` 会在 Linux 引入 OpenSSL（即 metrics §2.4 方案 3，未采纳：22.04 构建会链 libssl3，Ubuntu 20.04 无法运行） |
| `rfd` 默认特性、`eframe` 的 `x11` / `wayland` | **保留** | 均为 **Linux 目标专属**依赖（target-gated），对 Windows / macOS 产物零影响，去掉只会伤 Linux |
| `ureq` 的 `rustls-webpki-roots` | **2026-10-22 改为不启（REL-03）** | 该特性内嵌 Mozilla 根证书包（`webpki-roots` ≈ 400 KB 裸 DER），是字体之后最大的一块可省数据；根证书改由 `rustls-native-certs` 运行期读取平台信任库（顺带覆盖企业自建 CA / TLS 代理） |
| `eframe` / `egui` 的 `default_fonts`、`monochrome_emoji_fonts` | **2026-10-22 改为不启（REL-03）** | 此前口径「项目无比例西文字体，去掉会缺字形」仍成立，但解法改为**随包分发**：4 个字体文件放 `assets/fonts`，运行期装回基础族，界面观感不变（见 §1 注） |

> 备注：`getrandom 0.3.4` 在 `Cargo.lock` 中可能残留一条**陈旧条目**（cargo 未即时回收），
> 但 `cargo tree` 已确认它不在构建图中、不进产物，下次 cargo 自然重写锁文件时会被回收。
> 本次未跑 `cargo update`（全量重解析会冒改版本的风险），仅动 `Cargo.toml` 的 feature 范围。

## 7. workspace 依赖单点真源（V11-65 / V11-67）

> **准入判据**：同一第三方依赖有 **≥2 个声明点**才进根 `[workspace.dependencies]`——
> 只有重复声明才有分叉风险。成员侧写 `{ workspace = true }` 继承版本与 feature，
> 需要时在其上追加 crate 专属 feature 或 `optional = true`。
> 单一声明点的依赖留在使用处：那个声明点本身就是真源，迁入只多一层间接。
>
> 判据的机械兜底：`tests/workspace_topology_test.rs` 钉住各 crate 的依赖面，
> CI 的 `cargo tree` 拓扑 diff 钉住依赖边与锁文件版本（`docs/spec/workspace-topology.expected.txt`）。

### 7.1 已收口（根真源，成员继承）

| 依赖 | 真源规格（根 `[workspace.dependencies]`） | 继承方 |
|---|---|---|
| `egui` | `0.37.0`，`default-features = false` | root / icons / ui（render 另加 `optional = true`） |
| `image` | `0.25`，`default-features = false` + `png`/`jpeg`/`gif`/`webp`/`bmp` | root / infra |
| `syntect` | `5.3.0`，`default-features = false` + `parsing`/`default-syntaxes`/`dump-load`/`regex-fancy` | infra / root(dev) |
| `log` | `0.4` | root / core / infra |
| `tempfile` | `3` | root(dev) / infra(dev) |
| `serde` | `1` + `derive` | core / infra / ports / shared |
| `thiserror` | `2` | core / infra / ports / shared |
| `ahash` | `0.8`，`default-features = false` + `std`/`no-rng` | core / ports / shared |
| `wgpu` | `30.0.1`，`default-features = false` + `std` | root 的三个 `[target.*]` 段各追加后端 feature |

### 7.2 例外（刻意不入真源）

| 依赖 | 声明点 | 理由 |
|---|---|---|
| `pulldown-cmark`、`ropey`、`rfd`、`toml`、`ureq`、`rustls-native-certs` | 仅 infra | 单一声明点，无分叉风险 |
| `eframe`、`anyhow`、`env_logger` | 仅 root | 同上；且 eframe 的 `wgpu_no_default_features` / `x11` / `wayland` 是组合根专属规格 |
| `egui` 的 `optional = true`（render）、`wgpu` 的平台后端 feature | render / root 目标段 | **有意的规格差异**：render 默认 egui-free；wgpu 按平台各开一个后端（体积最小）。差异保留，不在真源里抹平 |

### 7.3 复核结论（2026-10-30）

- **无版本或 feature 分叉**：收口前 `serde`（4 处）、`thiserror`（4 处）、`ahash`（3 处）逐字相同，属"同规格重复"而非漂移；`wgpu` 三个目标段版本也一致。收口后的 `cargo tree --workspace --depth 1 --no-dedupe` 与拓扑基线**逐行一致**，`Cargo.lock` 无变化。
- **`cargo tree -d` 的重复版本全部来自上游**：`bit-set` 0.8/0.10、`bit-vec` 0.8/0.9、`getrandom` 0.2/0.4、`hashbrown` 0.16/0.17、`miniz_oxide` 0.8/0.9、`rustc-hash` 1.1/2.1、`syn` 2/3、`windows-sys` 0.52/0.61——分别由 `wgpu 30` / `syntect 5.3` / `egui 0.37` 的传递依赖约束决定，本项目无直接声明可调，故不动。
- **锁文件陈旧条目**：`thiserror 1.0.69` 仍在 `Cargo.lock` 里，但 `cargo tree -i thiserror@1.0.69` 输出 `nothing to print`——不在构建图中，不进产物，待下次 cargo 重写锁文件时回收（与上方 `getrandom 0.3.4` 备注同性质）。本次未跑 `cargo update`。
