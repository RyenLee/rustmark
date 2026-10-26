# rustmark 发布合规差距分析与待办清单

> 审计日期：2026-10-22（**发布前复检版**，含 REL-03 体积缓解落地与全量口径同步）
> 基线：`main` @ `83be9f1` 及其后本轮的合规修复　工具链：Rust 1.99.0 / cargo-deny 0.20.2
> 范围：法律合规 · 开源授权 · 第三方依赖许可 · 隐私与安全 · 发布文档完备性
> 本文是**发布阻断项的判定依据**与**剩余待办清单**；每项均给出「现状证据 + 补充要求 + 验收判据」。
> 本轮已闭环的项保留条目与证据，便于复核；未闭环项集中在 §6。
>
> **2026-10-26 补记**：两项 P1 已落地——`PRIV-01`（远程图片开关，关闭即零外联）与
> `PRIV-02`（导出 CDN 披露），提交 `ee8acfb`；`PRIVACY.md` 与 `docs/config-reference.md`
> 同步修正（旧文「不勾选则不注入」的说法本身有误：注入是自动的）。最后一个 P1 `SEC-03`
> （Release Notes 三段式）也**已于 2026-10-26 闭环**（`502ddf9`）——P1 清零。

---

## 0. 结论摘要

**判定：代码与文档侧已达到可发布状态；剩余的发布阻断项只有「三平台产物 + 真机回归」一类，
均受本机条件（无 macOS / Linux 机器）限制。**

| 维度 | 上一版结论 | 本轮结论 | 关键依据 |
|---|---|---|---|
| 自有授权 | 基本完备（有缺口） | ✅ 完备 | `LICENSE-MIT` / `LICENSE-APACHE`（201 行，含附录）/ `NOTICE` 齐全；`Cargo.toml` 声明 `MIT OR Apache-2.0`；README §9「授权要点」5 条已补 |
| 第三方依赖许可 | 通过 | ✅ 通过（依赖变动后复检） | `cargo deny check licenses bans sources` → `licenses ok, bans ok, sources ok`；186 crate 去重，无强 copyleft 进入产物 |
| 依赖安全公告 | **P0 未闭环** | ✅ 闭环 | `cargo deny --offline check advisories` → `advisories ok`（本机 RustSec 快照 **2026-10-21**；在线源 github.com 不可达，见 §3.2 说明） |
| 二进制再分发合规 | **P0 不合格** | ✅ 闭环（Windows 已验；macOS/Linux 待产物重打） | 三个打包脚本均随附 `LICENSE-MIT` / `LICENSE-APACHE` / `NOTICE` / `THIRD-PARTY-LICENSES.md`；Windows 产物解压核对通过 |
| 隐私 | 行为可控，披露已补齐 | ✅ 两项 P1 已闭环（2026-10-26） | 无遥测 / 无自动更新 / 崩溃报告本地（0 命中）；`PRIVACY.md` 披露三类联网行为与 PDF 进程间行为；**`PRIV-01` 远程图片开关**（关闭后零外联、状态栏常驻提示）与 **`PRIV-02` 导出 CDN 披露**（对话框常驻说明 + 导出件 `<head>` 注释）已落地，见 §4 |
| 发布文档 | **P0 不合格** | ✅ 达标（P2 只剩 rustdoc 发布方案与商标检索） | 版本 **0.2.0** 三处一致；CHANGELOG 有 `[0.2.0] · 2026-10-22`；`missing_docs` 实测 **0** 条 |
| 体积门禁 | **P0 不合格（11.36 MiB）** | ✅ 闭环 | release 产物 **10,466,816 B = 9.98 MiB**，红线 10,485,760 B，**余量 18,944 B**（§5） |

### 上一版 5 项 P0 的处置结果

| ID | 阻塞项 | 本轮结果 |
|---|---|---|
| `LEG-01` | 发行包未随附 LICENSE / NOTICE / 第三方许可清单 | ✅ 三个脚本已改，Windows 产物已复打并逐文件核对 |
| `DEP-02` / `SEC-01` | 依赖安全公告未跑通 | ✅ 离线快照判定 `advisories ok`；CI 在线 job 保留 |
| `DOC-01` | 版本号与 CHANGELOG 不一致 | ✅ 小版本升级至 **0.2.0**，`Cargo.toml` / README / CHANGELOG 一致 |
| `DOC-02` | 三平台产物与真机回归缺失 | ◐ Windows 产物已重打（含新字体与许可）；macOS / Linux 受本机条件限制未产出 |
| `REL-03` | 体积越红线且判定口径不一致 | ✅ 两步缓解落地：**11.37 MiB → 9.98 MiB**，口径统一为「未压缩 exe」（§5） |

---

## 1. 审计范围与方法

| 项 | 内容 |
|---|---|
| 代码与文档 | 全库扫描（`src/` 318 个 `.rs`、`tests/`、`docs/`、`scripts/`、`.github/`、根级文档） |
| 依赖许可 | `cargo deny check licenses bans sources` + `cargo deny list`（含各平台专属包） |
| 依赖安全 | `cargo deny check advisories`（在线失败 → 离线快照，见 §3.2） |
| 文档完备性 | `RUSTFLAGS="-W missing_docs" cargo check --lib` |
| 隐私行为 | 关键词扫描 `check_update\|auto_update\|analytics\|telemetry\|sentry\|reqwest`（0 命中）+ `ureq` / `Command::new` 调用点逐个核实 |
| 产物合规 | 三个 `scripts/package-*` 的产物组成逐行核对，并对 Windows 产物 `System.IO.Compression` 解包核对 |
| 体积 | `cargo build --release --bin rustmark` 后取 exe **文件字节数**（与 CI `size_gate` 同口径） |

复现命令：

```bash
cargo deny check licenses bans sources        # → bans ok, licenses ok, sources ok
cargo deny --offline check advisories         # → advisories ok（快照 2026-10-21）
RUSTFLAGS="-W missing_docs" cargo check --lib | grep -c "missing documentation"   # → 0
grep -rnE "check_update|auto_update|analytics|telemetry|sentry" --include=*.rs src | wc -l   # → 0
cargo test --all-targets                      # → 2101 passed · 0 failed · 4 ignored
cargo build --release --bin rustmark && stat -c%s target/release/rustmark   # → 10,466,816
```

---

## 2. 法律与开源授权（LEG）

### LEG-01 【✅ 已闭环】发行包随附许可文件

**证据**：

- `scripts/package-windows.ps1`：staging 内与 `rustmark.exe` 同级复制 `LICENSE-MIT` / `LICENSE-APACHE` / `NOTICE` / `THIRD-PARTY-LICENSES.md`；
- `scripts/package-macos.sh`：放入 `Rustmark.app/Contents/Resources/`（不进 `MacOS/`，避免影响签名主体）；
- `scripts/package-linux.sh`：tar.gz 的 `$stage/` 与 AppImage 的 `usr/share/doc/rustmark/` 各一份；
- Windows 产物 `dist/rustmark-v0.2.0-windows-x64.zip` 解包核对：4 个许可文件 + `fonts/` 内 6 份字体许可/声明均在。

**条款依据**：Apache-2.0 §4.1 / §4.2、MIT 第 1 条。字体部分由 `assets/fonts/Fonts-LICENSE.txt` 与随附许可全文覆盖。

**剩余**：macOS / Linux 产物须重打后复核同一份清单（并入 `DOC-02`）。

### LEG-02 【✅ 已闭环】贡献者许可授权（inbound license / DCO）

`CONTRIBUTING.md` §2.1 已写明「提交即以与本仓库相同的 `MIT OR Apache-2.0` 双许可授权」，
并要求每个 commit 带 `Signed-off-by`（`git commit -s`）。

### LEG-03 【✅ 已闭环（2026-10-26，`2f00a6e`）】`CODE_OF_CONDUCT.md`

已补 Contributor Covenant 2.1 中文版（注明英文原版为准），`CONTRIBUTING.md` §0 与 README §7 均已链接；
「执行」一节把投诉渠道指到 `SECURITY.md` 的私密公告，避免安全类投诉走公开 Issue。

### LEG-04 【◐ 部分完成】名称 / 商标 / 图标权利

- ✅ README §9「授权要点」第 4 条已声明：**许可不授予 `rustmark` 名称与 Logo 的商标使用权**（MIT / Apache-2.0 §7 保留商标权）；
- ⬜ 「rustmark」名称的商标与包名冲突检索（GitHub / crates.io / 主要应用商店）仍需人工完成并留档。

### LEG-05 【✅ 已闭环（2026-10-26，`2f00a6e`）】加密组件出口管制登记

`docs/release.md` 新增 §7：逐项列出 `rustls` + `ring` 的用途（TLS 客户端 / 远程图片下载，可在设置关闭）、
开源可获取性与开源豁免的一般适用，并明确「本登记不构成法律意见」与「组件或用途变化时本节必须同步」。
`REL-03` 后根证书改为读平台信任库，但**加密实现未变**，本项结论不变。

### LEG-06 【✅ 已闭环（2026-10-26，`2f00a6e`）】`words.txt` 来源与许可

`git log --follow` 实测：自初始提交 `338d333`（2026-10-10）即在仓内，无外部导入记录；内容 3,984 条 /
去重 3,635，仅单词、无释义与例句。`NOTICE` 与 `THIRD-PARTY-LICENSES.md` §4.3 已登记为**项目自编编译物**
（本项目双许可覆盖），并写明后续规则：替换为第三方词表前必须先登记来源与许可。
顺带修正两个 `.bin` 数据文件的路径（V11-64 后已迁到 `crates/rustmark-infra/src/infrastructure/…`）。

### LEG-07 【✅ 已完成】`NOTICE` 曾漏列随包字体

已补随包字体条目，并在本轮新增 4 个 egui 默认字体（Ubuntu-Light / Hack / NotoEmoji / emoji-icon-font）后再次核对。

### LEG-08 【✅ 已闭环】面向非开发者的「使用授权说明」

README §9 含 5 条授权要点（双许可选择、再分发条件、字体与第三方声明、商标保留、无担保），并链接 `LICENSE-*` 全文。

---

## 3. 第三方依赖许可（DEP）

### DEP-01 【✅ 通过】许可白名单检查（依赖变动后复检）

```text
cargo deny check licenses bans sources  →  bans ok, licenses ok, sources ok
```

- 依赖图 crate 数：**186**（`cargo deny list` 去重，含各平台专属包）；
- 许可分布（多许可 crate 按许可分别计数）：MIT 252 / Apache-2.0 209 / Zlib 17 / Unlicense 8 /
  ISC 6 / Apache-2.0 WITH LLVM-exception 4 / BSD-3-Clause 3 / BSD-2-Clause 2 / 0BSD 1 /
  GPL-2.0-only 1（仅 `self_cell` 的可选分支）/ Unicode-3.0 1；
- 逐项清单与人工复核表见 `THIRD-PARTY-LICENSES.md` §2.1（43 个含非 MIT/Apache-2.0 选项的 crate）。

**本轮依赖变动对许可的影响**

| 变动 | 许可影响 |
|---|---|
| 关闭 `default_fonts` / `monochrome_emoji_fonts` | `epaint_default_fonts` 退出依赖图，`OFL-1.1` / `Ubuntu-font-1.0` 不再需要出现在 `deny.toml` 白名单；字体许可改由随包文件承载 |
| 关闭 `rustls-webpki-roots` | `webpki-roots` 退出依赖图，`CDLA-Permissive-2.0` 白名单条目移除 |
| 新增 `rustls-native-certs` 0.8.5 | 许可 `Apache-2.0 OR ISC OR MIT`，均已在白名单内；平台适配依赖（`openssl-probe` / `security-framework` / `schannel`）同样为 MIT/Apache-2.0 |

### DEP-02 / SEC-01 【✅ 已闭环（附口径说明）】依赖安全公告

**结果**：`cargo deny --offline check advisories` → **`advisories ok`**（无命中项）。

**口径说明（必须留档）**：本机无法直连 `github.com`（`curl 28` 连接超时，代理 502），
在线 `cargo deny check advisories` 会失败在**数据库拉取**这一步。上述结论基于本机缓存的
RustSec 快照，提交时间 **2026-10-21 08:20 (+0200)**（`4b587f0`），距本次审计不足一天。
CI 的 `deny` job 在有网环境运行，会使用最新快照；**发布前仍须以 CI 的在线结果为准**。

### DEP-03 【✅ 已闭环】`self_cell` 的 GPL 选项固化

`deny.toml` 增加 `[[licenses.clarify]]`：`crate = "self_cell"`、`expression = "Apache-2.0 OR GPL-2.0-only"`，
并以包内 `LICENSE-APACHE` 为真源；两级把关（白名单不含 GPL-2.0-only + clarify 钉住表达式）已生效，
`licenses ok` 保持全绿。

### DEP-04 【✅ 已闭环】CI `deny` job 去伪绿

`.github/workflows/ci.yml` 的 `deny` job 已移除 `|| true` 并固定 cargo-deny 版本，安装失败即红。

### DEP-05 / DOC-06 【✅ 已闭环】`docs/dependencies.md` 状态漂移

- `egui` 行：由「必须并开 `monochrome_emoji_fonts`」改为「两个字体特性均关闭 + 随包分发」；
- `ureq` 行：`3.4.2 + ["rustls"]` → `3.5.0 + ["rustls-no-provider","_ring"]`，并新增 `rustls-native-certs` 行；
- §6「保留 / 不引入」表：补 `rustls-webpki-roots` 与 `default_fonts` 两条「2026-10-22 改为不启」的定格说明。

### DEP-06 【✅ 已闭环（2026-10-26，`2f00a6e`）】SBOM

未引入 `cargo cyclonedx`（要多装一个 cargo 子命令，版本漂移/装包失败都会让发布 job 变脆），改为
`tools/sbom.py`：零第三方依赖，从 `cargo metadata --locked` 生成 **CycloneDX 1.5** JSON。
范围取「发布二进制的实际依赖图」（从 `rustmark` 沿 resolve 可达），非整 workspace 的 dev 依赖；
含 purl / licenses（缺失写 `NOASSERTION`）/ 依赖边；**不写时间戳**，同一依赖树两次生成逐字节一致。
`release.yml` 已加生成步骤并把 `dist/sbom.cdx.json` 作为 Release 资产；`release_dryrun.py` 的 `check_sbom`
断言结构 + 确定性。实测 **388 组件 / 389 条依赖边**。

---

## 4. 隐私与安全（PRIV / SEC）

### PRIV-01 【✅ 已闭环（2026-10-26，`ee8acfb`）】远程图片自动加载，无开关

**原状**：`preview_view` 对文档内远程图片自动下载（异步、有超时与体积上限、MIME 白名单），
`PRIVACY.md` 已披露「打开含远程图片的文档会向图片主机发起请求」，但没有用户可见的开关。

**要求**：设置页增加「加载远程图片」（默认开），关闭后不发任何图片请求，占位文案与断网一致；
状态栏给出「远程图片已禁用」提示。**验收**：关闭后抓包零外联。

**落地**：设置 → 编辑器 → 「远程图片」复选框（`[editor] load_remote_images`，默认 `true`，
旧配置缺字段按 `true` 读出）。拦截点在 `image_entry` 的**缓存查找之前**：关闭后既不发起
请求、也不复用此前已缓存的纹理（「关了还看得见」会让用户无法确认自己真的断开了外联），
占位不进缓存，重新打开开关下一帧即恢复加载。占位文案复用断网版式，只换原因为
「远程图片加载已在设置中关闭（设置 → 编辑器）」；状态栏在关闭时常驻「远程图片已禁用」。
会话态与领域值由 `set_load_remote_images` 双写、启动回读、「取消」回滚三处钉住
（隐私承诺必须跨会话成立，不能只活在内存里）。

**验收证据**（抓包无法在无头环境执行，改用等价且更强的判据）：`tests/presentation/preview_view_test.rs`
两条对照用例——关闭时同一份 `https://example.invalid/*.png` 文档**不进入加载路径**
（缓存表里连占位都没有），开启时同一文档**确实进入**（占位或结果必有其一）；
`.invalid` 是 RFC 2606 保留域，测试不碰真实网络。变异验证：删掉拦截 → 关闭态用例红。

### PRIV-02 【✅ 已闭环（2026-10-26，`ee8acfb`）】导出 HTML 注入 jsdelivr CDN

**原状**：正文含公式 / Mermaid 时注入 `cdn.jsdelivr.net` 的 KaTeX / Mermaid 资源（带「资源缺失即跳过」守卫），
`PRIVACY.md` 已披露，但导出对话框内**无提示**。

**要求**：导出对话框在「注入 CDN 资源」选项旁明示「生成的 HTML 首次打开时会从 jsdelivr 加载资源」，
并在导出件 `<head>` 加一行 HTML 注释。**验收**：选项旁可见提示文案。

**落地（含一处口径纠正）**：本项目**没有「注入 CDN 资源」这个选项**——注入是自动的
（正文含 `class="math*"` 或 `language-mermaid` 才注入），`PRIVACY.md` 旧文里
「不勾选则不注入」的说法本身就是错的，已一并改正。因此披露放在两处而不是挂在开关旁：
① 导出对话框在 HTML 格式下常驻一行说明（选项下方，浅色次要文字）；② 导出件 `<head>` 在
注入块**首行**写一条 HTML 注释（排在资源引用之前，读取顺序即解释顺序）。

**验收证据**：`tests/presentation/export_view_test.rs::html_export_discloses_the_cdn_dependency`
用真帧收集绘制文字（`colored_label` 在窄窗会裁，字面量断言看不出来）：HTML 下必须出现
`cdn.jsdelivr.net` 文案，切到 Markdown 后必须消失（双向对照，防「恒显示」）。
`tests/infrastructure/exporter_test.rs::math_in_export_injects_katex_resources` 追加：
注释存在且位置早于 `katex.min.css`；`plain_export_has_no_external_resources` 继续保证
纯文本文档零外链（注释不会无差别注入）。

### PRIV-03 【◐ 已披露，P2】PDF 导出经由本机浏览器进程

Windows 调用本机 Edge / Chrome 无头模式，macOS 走系统打印链路；数据流（临时 HTML → 本机浏览器 → 删临时文件）
已在 `PRIVACY.md` §3 逐条披露。无需改动，仅需在下次隐私评审时复核浏览器行为差异。

### PRIV-04 / PRIV-05 【✅ 通过】崩溃报告本地化、无遥测、无自动更新

崩溃信息仅写本地日志（不含文档内容）；全库无遥测 / 自动更新 / 崩溃上报 SDK（关键词扫描 0 命中）。

### SEC-02 【✅ 已完成】漏洞披露政策

`SECURITY.md` 含私有披露渠道、响应时限、已实施安全措施、明确排除的威胁模型与校验和核对方式。

### SEC-03 【✅ 已闭环（2026-10-26，`502ddf9`）】产物未签名 + 发布页三段式

- ✅ README 写明「未签名 + 各平台绕过方式 + 校验和核对」；
- ✅ **发布页机械固化**：`tools/gen_release_notes.py` 的 `render` 开头固定
  「一、未签名 / 二、绕过方式（三平台）/ 三、校验和」三段，之后才是变更分组；
  `tools/release_dryrun.py` 断言三段齐全**且顺序正确**（变异：改掉任一段标题 → 演练当场红）。
- 产物仍**未签名**（证书成本见 §6 与 `docs/release.md` §6）；本项闭合的是「发布页必须说清」，
  不是「已经签名」。

### SEC-04 / DOC-05 【✅ 已闭环（2026-10-26，`2f00a6e`）】Issue / PR 模板

`.github/ISSUE_TEMPLATE/` 增 `bug_report.yml`（版本平台 / 复现步骤 / 期望 vs 实际 / 最小复现文档 / 日志位置，
含两条必勾确认：已搜过、**这不是安全问题**）、`feature_request.yml`（先问场景再问方案，附分层影响面下拉）、
`config.yml`（关空 Issue + 安全私密渠道 + 讨论区）；`.github/PULL_REQUEST_TEMPLATE.md` 把仓库既有纪律前置到
PR 表单：四道门禁实测数字、守卫改动的变异验证、新 `pub` 字段登记、新增测试入口补 `[[test]]`、阈值同步。

---

## 5. REL-03 体积红线（本轮重点，已闭环）

### 5.1 结论

| 项 | 数值 |
|---|---|
| 红线（`NFR-PERF-01`） | 10,485,760 B（10 MiB，**未压缩 exe**） |
| REL-03 起点实测 | 11,924,480 B（11.37 MiB，越线 1,438,720 B） |
| **本轮最终实测** | **10,466,816 B（9.98 MiB）** |
| 余量 | **18,944 B（0.18%）** |
| 同配置重建 | 两次重建字节数完全一致（可复现） |

### 5.2 两步缓解（均零功能代价）

| 步骤 | 变更 | 相对上一步 |
|---|---|---|
| ① 字体外置 | 关闭 `default_fonts` + `monochrome_emoji_fonts`；Ubuntu-Light / Hack / NotoEmoji / emoji-icon-font 随包分发，运行期由 `infrastructure::fonts::install_egui_base_fonts` 装回比例 / 等宽基础族 | **−1,410,560 B** |
| ② 根证书外置 | 关闭 `rustls-webpki-roots`，改 `rustls-native-certs` 运行期读平台信任库（`asset_loader::platform_root_certs`） | **−47,104 B** |
| 合计 | — | **−1,457,664 B（−1.39 MiB）** |

**证据与验证**：

- 视觉等价：族顺序、`FontTweak` 缩放（NotoEmoji 0.81 / emoji-icon 0.90）与 egui 内嵌实现逐项对齐，
  仅 `egui-icons.ttf`（3,484 B）不随包——本项目对 `egui::special_emojis` 零引用；
- 文件缺失不致 panic：字体键**条件注册**（文件不在就不进族，避免 egui 字体族的悬空键 panic）；
- TLS 实测：临时探针（已删）读平台根证书 **40 张**，`https://example.com/` → **200**；
- 打包实测：`dist/rustmark-v0.2.0-windows-x64.zip` = **7,486,958 B（7.14 MB）**，内含 4 个新字体与 4 份许可全文；
- 回归：2101 用例全绿（其中 3 个几何断言因「无头测试上下文现在没有内置字体」做了适配，
  见 §5.3），`clippy -D warnings`、`fmt`、`check-arch` 均绿。

### 5.3 本轮为体积改动而做的测试适配（透明留档）

| 位置 | 问题 | 处理 |
|---|---|---|
| `src/presentation/widgets/text.rs` | 无字体上下文里 `短` 落到「合成豆腐块」，其侧边距为 `0.1 × 字号`（14pt → 1pt），左缘断言 `< 1.0` 误报 | 阈值改为 `< 2.0` 并在注释里写明这 1pt 是字形内缩而非布局偏移（居中布局会偏 ~110pt，阈值仍有判别力） |
| `tests/support.rs` | 无头测试沿用 `egui::Context::default()`（现在族为空），字宽 / 基线类断言失真 | 新增 `ctx_with_app_fonts()`：装生产字体定义并空跑一帧落定 |
| `tests/presentation/outline_alignment_test.rs`、`preview_view_test.rs` | 3 个断言读真实字宽 / 基线 | 改用 `ctx_with_app_fonts()` |

### 5.4 残余风险

- **余量仅 18,944 B**：后续任何功能增量都会先吃掉它，CI 体积门禁随时可能转红。
  下一个杠杆（按性价比）：`image` 去 `webp`（≈78 KiB，代价是 WebP 不支持）→ `syntect` 语法集精选（ADR-002 方案 B）→
  `env_logger` 去 `humantime`（实测仅 −4 KiB，不建议，日志会失去时间戳）。
- **macOS / Linux 未实测量**：两步缓解对三者同样成立，但 Linux 侧另有 `x11` / `wayland` / `rfd` 的 GTK 依赖，
  实际字节须在 CI 或真机重测后才能对三平台下结论（并入 `DOC-02`）。
- **口径一致性（已修）**：`docs/metrics.md` §1 / §2.4 / §2.6 / 新增 §2.7、`ci.yml` 注释、打包脚本说明现在统一为
  「**未压缩 exe** ≤ 10 MiB」，不再出现「zip / 安装包」混用。

---

## 6. 剩余待办清单

### P0 · 发布阻断（1 项）

| # | ID | 待办 | 交付物 | 验收判据 |
|---|---|---|---|---|
| 1 | `DOC-02` | 产出 macOS / Linux 产物（脚本已就绪、许可文件已在脚本内）+ 聚合 `SHA256SUMS` + 三平台手工回归（V11-02） | `dist/`、`docs/manual-qa-*.md` | `sha256sum -c` 全 OK；三份清单有执行结论；三平台体积门禁绿 |

> 说明：Windows 产物已在本轮复打（`7,486,958 B`，SHA256 见同名 `.sha256`），三平台 `SHA256SUMS`
> 聚合文件尚未生成（当前为逐文件 `.sha256`）。**本项受本机条件限制**，需在 macOS / Linux 机器或 CI 上完成。

### P1 · 发布前应处置（**0 项**；3 项已于 2026-10-26 全部闭环）

| # | ID | 待办 | 验收判据 |
|---|---|---|---|
| ~~2~~ | ~~`PRIV-01`~~ | ~~「加载远程图片」开关（离线模式）~~ **已闭环（`ee8acfb`）**：设置页开关 + 拦截点前移到缓存查找之前 + 状态栏常驻提示 | 关闭后零外联请求（等价判据见 §4 PRIV-01） |
| ~~3~~ | ~~`PRIV-02`~~ | ~~导出对话框与导出件明示 CDN 引用~~ **已闭环（`ee8acfb`）**：对话框常驻说明 + 导出件 `<head>` 注释 | 对话框可见提示（真帧用例双向断言） |
| ~~4~~ | ~~`SEC-03`~~ | ~~Release Notes 固化「未签名 + 绕过方式 + 校验和」三段式~~ **已闭环（`502ddf9`）**：生成器固定三段 + 演练断言（含顺序） | 发布页可见三段说明（删段即演练红） |

### P2 · 下一版本（剩 2 项；5 项已于 2026-10-26 闭环）

| # | ID | 待办 |
|---|---|---|
| ~~5~~ | ~~`LEG-03`~~ | ~~新增 `CODE_OF_CONDUCT.md`~~ **已闭环（`2f00a6e`）**：Contributor Covenant 2.1 + CONTRIBUTING §0 / README §7 链接 |
| 6 | `LEG-04` | `rustmark` 名称 / 包名冲突检索并留档（**声明部分已做；检索需人工完成并留档**） |
| ~~7~~ | ~~`LEG-05`~~ | ~~`docs/release.md` 登记加密组件~~ **已闭环（`2f00a6e`）**：§7 组件表 + 开源豁免说明 + 变更纪律 |
| ~~8~~ | ~~`LEG-06`~~ | ~~追溯 `words.txt` 来源~~ **已闭环（`2f00a6e`）**：`git log --follow` 追溯 + NOTICE / 第三方清单登记 |
| ~~9~~ | ~~`DEP-06`~~ | ~~生成 SBOM~~ **已闭环（`2f00a6e`）**：`tools/sbom.py`（CycloneDX 1.5，388 组件）+ 随 Release 发布 + 演练断言 |
| ~~10~~ | ~~`SEC-04` / `DOC-05`~~ | ~~Issue / PR 模板~~ **已闭环（`2f00a6e`）**：两份 Issue 表单 + config + PR 模板（含安全披露警示） |
| 11 | `DOC-04` | rustdoc 产物发布方案（需先定托管位置，属产品决定） |

---

## 7. 本轮已闭环 / 已变更的文件

| 文件 | 动作 |
|---|---|
| `assets/fonts/` | 新增 4 个 egui 默认字体（Ubuntu-Light / Hack / NotoEmoji / emoji-icon-font）与 4 份许可全文；`Fonts-LICENSE.txt` 补条目 6–9 并重写「再分发须知」 |
| `Cargo.toml` | 关闭 `default_fonts` / `monochrome_emoji_fonts`；`ureq` 改 `["rustls-no-provider","_ring"]`；新增 `rustls-native-certs`；版本 `0.2.0`；注释同步体积决策 |
| `src/infrastructure/fonts.rs`（原 `fonts/mod.rs`） | 新增 `install_egui_base_fonts`（条件注册防悬空键），`font_definitions` 先装回基础族；7 处测试适配 |
| `src/infrastructure/persistence/asset_loader.rs` | 新增 `platform_root_certs`（`OnceLock` 缓存 + 空根集不 panic），Agent 显式注入 `RootCerts::Specific` |
| `tests/support.rs` 等 3 个测试文件 | 新增 `ctx_with_app_fonts()`；3 个几何断言改用真实字体上下文 |
| `tests/support.rs` + `tests/support/` | 模块布局回归 `CONSTR-03`：`support/mod.rs` 移到 `support.rs`，7 个测试入口改 `#[path]`；`Cargo.toml` 增 `autotests = false`（防 `support.rs` 被当成零用例测试目标） |
| `deny.toml` | 移除 `OFL-1.1` / `Ubuntu-font-1.0` / `CDLA-Permissive-2.0`（对应 crate 已退出依赖图），加注释说明去向 |
| `THIRD-PARTY-LICENSES.md` | 依 `cargo deny list` 重算分布与 §2.1 清单（43 项）；§4.1 补 4 个字体；§4.2 改写为「随包分发」 |
| `docs/dependencies.md` | `egui` / `ureq` 行与 §6 表按新口径重写 |
| `docs/metrics.md` | §1 `NFR-PERF-01` 状态、§2.4 闭环指针、§2.6 状态改为已闭环；**新增 §2.7**（两步缓解实测 + 残余风险） |
| `.github/workflows/ci.yml` | 体积门禁注释由「越线待决策」改为「已回到红线内，macOS/Linux 待复测」 |
| `docs/manual-qa-windows.md` / `-macos.md` | emoji 注由「egui 内置」改为「随包分发的 `fonts/`」 |
| `src/shared/system.rs` | 注释里的 `features = ["default_fonts"]` 过时写法修正 |
| `README.md` / `CHANGELOG.md` / `src/lib.rs` | 版本 0.2.0 同步；`#![warn(missing_docs)]` 已启用（实测 0 告警） |
| `NOTICE` | 补 4 个 egui 默认字体的版权 / 许可 / 用途，并注明「以 `fonts/` 文件形式分发、不编进可执行文件」 |
| `dist/rustmark-v0.2.0-windows-x64.zip` | 复打（7,486,958 B），含 4 个新字体、许可文件、更新后的 `NOTICE` 与新的 exe |

---

## 8. 发布门槛核对（Definition of Done）

| # | 条款 | 状态 |
|---|---|---|
| 1 | 三平台产物均含 4 个许可文件 | ◐ Windows 已核；macOS / Linux 待重打 |
| 2 | `cargo deny check licenses advisories bans sources` 全绿 | ✅ 本机离线判定全绿（advisories 快照 2026-10-21）；发布前以 CI 在线结果为准 |
| 3 | `Cargo.toml` version == tag == CHANGELOG 最高条目 | ✅ 三者均为 `0.2.0`（tag 待打） |
| 4 | 三平台产物 + `SHA256SUMS` 齐备，`sha256sum -c` 全 OK | ◐ Windows 有逐文件 `.sha256`；聚合文件待生成 |
| 5 | 三平台手工回归清单执行完毕并写回结论 | ⬜ 受本机条件限制 |
| 6 | 验证链四绿（fmt / check-arch / clippy -D warnings / test） | ✅ **2101 passed · 0 failed · 4 ignored** |
| 7 | 体积门禁绿且口径统一 | ✅ Windows 9.98 MiB（余量 18,944 B）；口径已统一为未压缩 exe |
| 8 | 发布页含「未签名 + 绕过方式 + 校验和」三段式 | ✅ `SEC-03` 已闭环：生成器固定三段 + 演练断言顺序（`502ddf9`） |
| 9 | `PRIVACY.md` / `SECURITY.md` / `NOTICE` / `THIRD-PARTY-LICENSES.md` 与实际行为、依赖一致 | ✅ 本轮已随依赖变动同步 |
| 10 | `docs/release.md` §1 前置检查清单逐项可勾 | ✅（`LEG-05` 登记项仍属 P2） |

---

## 附录 A · 关键实测记录（2026-10-22）

| 命令 / 检查 | 结果 |
|---|---|
| `cargo deny check licenses bans sources` | `bans ok, licenses ok, sources ok` |
| `cargo deny list`（去重） | 186 crate；MIT 252 / Apache-2.0 209 / Zlib 17 / Unlicense 8 / ISC 6 / LLVM-exc 4 / BSD-3 3 / BSD-2 2 / 0BSD 1 / GPL-2.0-only 1 / Unicode-3.0 1 |
| `cargo deny --offline check advisories` | `advisories ok`（RustSec 快照 `4b587f0`，2026-10-21 08:20 +0200） |
| `cargo deny check advisories`（在线） | `ERROR` — `git fetch` github.com:443 超时（`curl 28`），数据库更新失败 |
| `RUSTFLAGS="-W missing_docs" cargo check --lib` | **0** 条缺失告警 |
| `cargo fmt --check` | 通过 |
| `cargo run --bin check-arch` | 通过（含 `tests/` 扫描） |
| `cargo clippy --all-targets --all-features -- -D warnings` | 无告警 |
| `cargo test --all-targets` | **2101 passed · 0 failed · 4 ignored** |
| `cargo build --release --bin rustmark` → exe 字节数 | **10,466,816 B**（9.98 MiB；红线 10,485,760 B，余量 18,944 B） |
| 同配置重建 | 10,466,816 B（完全一致） |
| `scripts/package-windows.ps1` | `dist/rustmark-v0.2.0-windows-x64.zip` = 7,486,958 B（7.14 MB）+ `.sha256` |
| zip 解包核对 | `rustmark.exe` / `fonts/`（16 个文件：9 个字体 + 7 份许可与说明，含本轮新增 4 字体与 `UFL.txt` / `OFL.txt` / `Hack-Regular.txt` / `emoji-icon-font-mit-license.txt`）/ `LICENSE-MIT` / `LICENSE-APACHE` / `NOTICE` / `THIRD-PARTY-LICENSES.md` / `README.txt` |
| `grep -rnE "check_update\|auto_update\|analytics\|telemetry\|sentry\|reqwest" --include=*.rs src` | 0 命中 |
| `wc -l LICENSE-APACHE` | 201 行，含 `END OF TERMS AND CONDITIONS` 与 `APPENDIX` |
| 平台根证书探针（临时，已删除） | 读到 40 张根证书；`https://example.com/` → `200` |
| `.github/` 模板 | 无 `ISSUE_TEMPLATE/`、无 `PULL_REQUEST_TEMPLATE.md`（`SEC-04` / `DOC-05` 待办） |
| `CODE_OF_CONDUCT.md` | 不存在（`LEG-03` 待办） |

## 附录 B · 免责声明

本文档基于机器可识别的许可元数据、代码扫描与文件核对生成，**不构成法律意见**。
涉及对外发布的正式法律确认（尤其商标、出口管制、GDPR/PIPL 适用性与字体再分发），
建议由具备资质的法务复核。
