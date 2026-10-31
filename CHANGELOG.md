# Changelog

本项目的版本遵循[语义化版本](https://semver.org/lang/zh-CN/)。
commit message 中的 `[S{阶段}-{编号}]` 对应 `.agent/tasks/` 的任务编号，
Release Notes 依此自动归组。

## [未发布]

## [0.2.1] · 2026-10-31

### 本次发布概要

- **macOS 发布修复**：内置字体改按标准 bundle 布局打进 `Contents/Resources/fonts`
  （不再放 `Contents/MacOS/fonts`），修复 codesign 把 `.ttf` 当代码容器后报
  `code object is not signed at all` 导致的发布失败；运行时字体查找优先
  `Contents/Resources/fonts`，并保留 exe 旁 `fonts/` 兜底以兼容旧包与开发环境。
- **签名与公证**：签名不再依赖已弃用的 `--deep`，先签嵌套代码再签外层 bundle，
  保留 hardened runtime / timestamp / ad-hoc 分支；release workflow 支持导入
  Developer ID 证书，并在配置公证凭据时对 `.app`（经临时 zip）与 `.dmg` 执行
  `notarytool` 公证 + `stapler` 装订，最后以 `codesign --verify --deep --strict`、
  `stapler validate`、`spctl --assess` 复核。
- **无用户可见功能变更**：本版为发布链路修复，编辑器行为与 `0.2.0` 一致。

## [0.2.0] · 2026-10-22

### 本次发布概要

- **定位**：原生轻量级 Markdown 编辑器（Rust 2024 + egui/wgpu），单文件产物、零遥测、
  GBK/GB18030 无损读写、分屏实时预览；目标平台 Windows / macOS / Linux。
- **产物形态**：Windows `rustmark-v0.2.0-windows-x64.zip` / macOS `.app`+`.dmg` /
  Linux `AppImage`+`tar.gz`，均随附 `LICENSE-MIT`、`LICENSE-APACHE`、`NOTICE`、
  `THIRD-PARTY-LICENSES.md` 与 `fonts/`（含字体许可）；产物校验用逐文件 `.sha256`
  （聚合 `SHA256SUMS` 待三平台产物齐备后生成）。
- **与上一版的边界**：`0.1.0` 为阶段 01–08 开发期基线；本版收纳 2026-10-10 ～ 10-22
  的全部工作——egui 0.37 升级、V11 架构改造收官、性能口径裁定、用户反馈批次修复、
  CI/Release 工作流重构与发布合规文档补齐。
- **合规与依赖**：许可 `MIT OR Apache-2.0`（双许可，条款要点见 README §9）；
  第三方清单见 `THIRD-PARTY-LICENSES.md`（186 crate，无强 copyleft 进入产物）；
  `cargo deny check licenses advisories bans sources` **全绿**（6 条 CVE 补丁升级 +
  1 条停维豁免登记，见 `deny.toml`）；隐私与威胁模型见 `PRIVACY.md` / `SECURITY.md`。
- **产物校验**：逐文件 `.sha256`；**未签名**，SmartScreen/Gatekeeper 绕过方式与
  校验命令见 README §2。
- **已知限制**：与 README §4 同源，保持一致。

### 2026-10-22 · 体积合规 REL-03（11.37 MiB → 9.98 MiB，回到红线内）

> 依「保持 10 MB 未压缩红线」的裁定执行 `docs/metrics.md` §2.4 方案 1 与等效方案 3，两步均零功能代价。

- **字体外置**：关闭 `egui` / `eframe` 的 `default_fonts` 与 `monochrome_emoji_fonts`。
  Ubuntu-Light / Hack / NotoEmoji / emoji-icon-font 四个字体（1,417,504 B）改为随发行包
  以 `fonts/` 分发，运行期由 `infrastructure::fonts::install_egui_base_fonts` 装回
  比例 / 等宽基础族——族顺序与 `FontTweak` 缩放逐项对齐，界面观感不变；
  `egui-icons.ttf` 不随包（本项目对 `egui::special_emojis` 零引用）。字体许可全文随包补齐。
- **根证书外置**：关闭 `ureq` 的 `rustls-webpki-roots`（约 400 KB 裸 DER），改用
  `rustls-native-certs` 运行期读取平台信任库（Windows 注册表 / macOS Keychain /
  Linux `/etc/ssl/certs`）；TLS 实现不变（rustls + ring），不链 libssl，Ubuntu 20.04 兼容性不受影响，
  并顺带支持企业自建 CA。读不到信任库时返回空根集并记录 `warn`，https 图片降级失败、进程不退出。
- **实测**：exe 11,924,480 B → **10,466,816 B（9.98 MiB）**，红线 10,485,760 B，余量 18,944 B；
  同配置重建字节数一致。Windows 便携包复打为 7.14 MB（含 4 个新字体与许可全文）。
- **测试适配**：无头测试上下文不再有内嵌字体，新增 `tests/support::ctx_with_app_fonts()`；
  `presentation::widgets::text` 的左缘阈值由 1pt 放宽到 2pt 并注明「合成豆腐块侧边距」。
- **文档口径同步**：`docs/metrics.md`（§1 / §2.4 / §2.6 + 新增 §2.7）、`docs/dependencies.md`、
  `THIRD-PARTY-LICENSES.md`（186 crate、43 项复核表）、`deny.toml`（移除已退出依赖图的三项许可）、
  `docs/manual-qa-*.md`、`.github/workflows/ci.yml`、`docs/release-compliance-gap-report.md`。

### 2026-10-22 · 模块布局回归 CONSTR-03（移除 `mod.rs`）

> `REQUIREMENTS.md` 的 `CONSTR-03` 与 `CONTRIBUTING.md` 均明令「禁止 `mod.rs`：一级模块用
> `xxx.rs`，子模块放 `xxx/`」，但实测仍有 2 处残留（`src/infrastructure/fonts/`、`tests/support/`）。

- **`src/infrastructure/fonts/mod.rs` → `src/infrastructure/fonts.rs`**：门面内容（模块文档、
  `pub mod assembly` / `pub mod discovery`、`pub use` 重导出）原样搬移，子模块仍留在 `fonts/`；
  模块路径与公开 API 零变化，调用点无需改动。
- **`tests/support/mod.rs` → `tests/support.rs`**：7 个测试入口的
  `#[path = "../support/mod.rs"]` 改为 `"../support.rs"`。注意 `#[path]` 加载的模块，
  其子模块按「该文件所在目录 + 子模块名」查找（会落成 `tests/clock.rs`），
  故 `support.rs` 内的 4 个子模块改用 `#[path = "support/clock.rs"]` 等显式指向 `tests/support/`。
- **`Cargo.toml` 增 `autotests = false`**：测试目标本就全部以 `[[test]]` 显式声明，
  否则 `tests/support.rs` 会被自动发现成一个零用例的 `support` 测试目标（重复编译）。
- **`tools/arch_check.rs` 增布局校验（本条的防回退）**：`scan` / `scan_with_tests`
  逐个文件判文件名，`src/` 与 `tests/` 任一侧出现 `mod.rs` 即报违规，且**不设
  `arch-allow` 豁免**（与帧预约规则同一口径）。`tests/arch_violation_fixture.rs`
  新增注入用例：`src/shared/mod.rs` 与 `tests/support/mod.rs` 各报一条，
  `shared.rs` + `shared/gateway.rs` 必须放行。
- 门禁：fmt ✓ / check-arch ✓ / clippy `-D warnings` ✓ / `cargo test --all-targets`
  **2102 passed · 0 failed · 4 ignored**（基线 2101 + 新增布局用例 1）；`src` 与 `tests` 下已无 `mod.rs`。

### 2026-10-22 · Python 工具清理（26 → 5）

> `tools/` 下堆积了 20 个针对单个改造项的一次性「变异验证」脚本，锚点写死当时的源码片段；
> 它们没有任何代码/CI 调用方，结论早已写进改造计划与 backlog。

- **删除 21 个**：`mut_arch_*.py`（3）、`mut_r04/r05/r06/r12/r17/r20/r21.py`、`mut_r11_1/2/3_*.py`（6）、
  `mut_opener_r03.py`、`mut_v11_55.py`，以及一次性性能基准 `bench_arch_r02.py`。
  删除前已整目录备份到临时目录，逐文件清单与理由见 `docs/python-cleanup-report.md`。
- **保留 5 个**：`coverage_gate.py`（CI 覆盖率门禁）、`release_dryrun.py`（CI 发布演练）、
  `gen_release_notes.py`（`release.yml` 生成 Notes）、`gen_gb18030_ranges.py`（随包数据表唯一再生成路径）、
  `assets/logo/build_logo.py`（Logo 几何唯一真源）。
- **引用改写**：`docs/architecture-refactor-plan.md`（16 处）、`docs/v1.1-backlog.md`（7 处）、
  `docs/shared-internal-cycle-review.md`（1 处）及 2 处源码/测试注释，改为
  「`mut_xxx.py`（已清理）」并保留当时的验证结论；`.workbuddy/` / `.agent/` 历史日志不动。
- 验证：`python -m py_compile` 5 个保留脚本通过、`python tools/release_dryrun.py` **13/13**、
  `cargo run --bin check-arch` OK、`cargo test --all-targets` **2102 passed · 0 failed · 4 ignored**。

### 2026-10-22 · 最终发布合规复查（安全公告闭环 + 版本裁定 0.2.0 + 发布文档补全）

> 前几轮合规审查的收尾复查，逐项验证 `docs/release-compliance-gap-report.md`。

- **DEP-02 闭环**：`cargo deny check advisories` 本轮跑通，命中 7 条 → 修复 6 条
  （`rustls` 0.23.45→0.23.46、`rustls-webpki` 0.103.16、`webpki-roots` 1.0.9→1.0.10，
  覆盖 RUSTSEC-2026-0346/0347/0348/0349/0381/0382）+ 登记 1 条
  （RUSTSEC-2025-0141 bincode 停维，带理由豁免写入 `deny.toml`）。四项检查全绿。
- **DOC-01 裁定**：版本升 **0.2.0**（`Cargo.toml` / README / PRIVACY / SECURITY 同步）；
  `settings_view.rs` 硬编码版本号改为 `env!("CARGO_PKG_VERSION")`，清偿两处版本漂移之债。
- **DEP-03**：`deny.toml` 增加 `self_cell` 的 clarify 条目与两级把关说明。
- **发布文档补全**：README §9 增「授权要点」（LEG-08）、`CONTRIBUTING.md` 增贡献者
  授权条款（LEG-02/DCO）、新增 `docs/config-reference.md` 配置与主题格式参考（DOC-03）、
  更正 `Fonts-LICENSE.txt` 分发方式表述（DOC-07）、更正 `docs/dependencies.md` §3
  工具状态（DEP-05）。
- 门禁复核：fmt ✓ / check-arch ✓ / clippy ✓ / `cargo deny --offline check
  licenses advisories bans sources` → **全绿**。

### 2026-10-22 · 修复「打开文件夹后文件树为空 + 删除确认无效」（CI macOS job 5 条红）

> 症状：`--test app` 报 `148 passed; 5 failed`，失败用例是
> `delete_confirm_test::{confirming_a_delete_removes_the_file, deleting_a_directory_warns_about_its_contents}`
> 与 `workspace_reveal_test::{opening_a_folder_from_welcome_reveals_workspace_view, opening_a_folder_respects_user_collapsed_decision, switching_workspace_clears_stale_expansions}`。
> 本机（Windows）153 全绿——**只在临时路径含符号链接的平台出现**（macOS 的 `/var` → `/private/var`）。

- **根因（单一，两处症状同源）**：`FsWorkspaceRepository::scan` 先把根 `canonicalize`，
  再用**解析后的形态**拼子目录路径；而调用方（`EditorApp::workspace_root`、
  `request_delete` 传进来的目标）持有的是**用户选定的原始形态**。两者字符串前缀对不上：
  ① 树投影 `is_under_root` 逐条失配 → 文件树画成空列表（实测 `[]`）；
  ② 领域聚合 `Workspace::remove_entry` 要求存在**路径相等**的条目，找不到就返回
  `NotFound` → 删除在**碰磁盘之前**被拒（点了「是」文件还在）；删目录的确认文案
  也因此拿不到「目录 + 内容条数」。
- **修复**：产出路径的前缀**一律用调用方给定的根**（新增 `join_given_root`，
  `collect` 改为「磁盘路径用于读、相对路径用于拼产出」）；canonicalize 只保留给
  **访问磁盘**用。用户看到的路径应当是他选的那个——这不是测试问题，
  macOS 上打开任何含符号链接的目录都会「树是空的」。
- **回归 2 条**（`tests/infrastructure/workspace_repository_test.rs`）：前缀不变量 +
  「canonical 形态不同的根」用例（符号链接优先，Windows 无开发者模式时退回大小写变体，
  都造不出则跳过）。**变异验证 1/1**：把前缀改回 canonicalize 形态 → 该用例当场红，
  其余 8 条不变（证明不变量用例在本平台是同义反复、真正有牙齿的是后者）。
- 门禁四绿实测：fmt✓ / check-arch✓ / clippy 0 lint / **2101 passed · 0 failed · 4 ignored**（2099 + 2）。

### 2026-10-22 · CI / Release 工作流重构（可定位、可复现、可缓存）

> 只改编排与诊断，**不改产物形态与门禁语义**：job id 与矩阵显示名保持原样
> （它们是分支保护的必需检查项），产物命名、tag 触发、校验和聚合、Release Notes 归组全部不动。

- **修掉的环境问题**：① `ubuntu-20.04` 已被 GitHub 下线 → 改 `ubuntu-22.04`；
  ② 全局 `RUSTFLAGS: -D warnings` 会作用于**所有**编译单元（含第三方依赖）且让缓存
  指纹失效 → 移除，本仓库代码的严格判定仍由 `cargo clippy -- -D warnings` 承担；
  ③ workflow 里硬编码 `toolchain: 1.99.0` 与 `rust-toolchain.toml` 重复 → 删掉，
  唯一真源是该文件；④ `cargo install cargo-deny --locked || true` 会**吞掉安装失败** →
  去掉 `|| true` 并固定版本 0.20.2（规则语义随版本变，装最新版会造成本机/CI 口径不一致）；
  ⑤ `scripts/*.sh` 在 git 里是 `100644` → 改 `100755`。
- **快速定位**：新增「环境快照」步骤（打印 `std::env::temp_dir()` 与它的真实形态——
  二者不一致即该平台含符号链接，正是本次缺陷的触发条件）；测试用
  `--no-fail-fast` 跑完全部目标，日志解析成「按目标统计」表格写进 Job Summary，
  并把 FAILED 行逐条提升为 `::error::` 注解。
- **结构**：体积门禁从内联 shell 抽到 `scripts/size_gate.sh`（CI 与本地同一份实现，
  已验证绿/红/预警三条路径）；cargo 命令行工具装到 `.cargo-tools/` 并缓存
  （**不**装进 `~/.cargo/bin`，那里是 rustup 的 cargo/rustc 代理）；
  `deny` job 把「许可（离线可跑）」与「安全公告（需拉 RustSec 库）」拆成两步，
  网络抖动不再连坐许可门禁；各 job 加 `timeout-minutes`、最小权限 `contents: read`。

### 2026-10-22 · 发布合规文档补齐（隐私 / 安全 / 第三方许可）

> 合规复查发现项，逐条要求见 `docs/release-compliance-gap-report.md`。

- **新增 `PRIVACY.md`**：明确「无账号 / 无遥测 / 无自动更新 / 无崩溃自动上传」，
  并首次把**仅有的三类联网行为**成文披露——① 文档含 `http(s)` 图片时预览会向该 URL
  发起 GET（有超时 + 体积上限 + MIME 白名单，不放行 SVG）；② 导出 HTML 勾选 KaTeX /
  Mermaid 注入后，打开导出件会向 `cdn.jsdelivr.net` 请求资源；③ 预览点击外链交给系统默认程序。
  另记录 PDF 导出（本机 Edge / Chrome 无头）的进程间数据流向与三平台配置目录清理方式。
- **新增 `SECURITY.md`**：漏洞走 GitHub 私有安全公告（不在公开 Issue 披露），
  给出响应时限预期、当前已实施的安全措施清单、以及**明确排除的威胁模型**
  （本机恶意文档触发的既定功能不在范围内）。
- **新增 `THIRD-PARTY-LICENSES.md`**：246 个 crate 的许可分布 + 41 个含非 MIT/Apache-2.0
  选项的依赖逐项列表 + 随包字体（OFL-1.1 / Bitstream-DejaVu）与 vendored 数据文件来源。
  **重点记录 `self_cell` 的 `Apache-2.0 OR GPL-2.0-only`：本项目明确选 Apache-2.0 选项**，
  故不触发 GPL 义务；若上游改 GPL-only 需先评审。
- **`NOTICE` 补字体条目**：此前列了依赖与数据文件，漏了随包分发的 5 款字体
  （OFL-1.1 要求随附版权声明与许可全文），现按逐字体版权补入，并指向完整清单。
- **`docs/release.md` §1 前置检查**补三项：依赖安全公告须通过 / **许可文件须随包**
  / 合规文档须随发布行为复核。README 新增「隐私与安全」章节并链接上述文档。

### 2026-10-21 · 架构改造 V11-54a 收官（M6：R-17 / R-19 / R-20 / R-21）

> 任务状态：`docs/v1.1-backlog.md` V11-54a；方案：`docs/architecture-refactor-plan.md`。
> 至此 **21 条改造项（R-01~R-21）全部完成**。

- **R-17 · widgets 收口**：搜索分组头限宽段的 `allocate_ui_with_layout(vec2(avail, 0.0))`
  零高反模式改走 `widgets::text::elided_segment`（高度参数取已排控件 `Rect`，
  类型层面杜绝「传 0 高」）；新增 `widgets::row::paint_row_background`，
  收口「选中→`accent_soft` / 悬停→`bg_tertiary` 圆角 4pt」在 quick_open /
  command_palette / 顶栏导航箭头 / close_all **4 处逐字重复**的配方
  （独立封装而非给 `clickable_row` 加参数，避免与父级底色叠加两次）。测试 2093 → **2097**。
- **R-19 · ADR 补齐**（纯文档）：补 `ADR-011`（动作路由与双意图：穷举 `action_route`
  + `Unbound` 显式声明、无 `_` 兜底；跳转双意图 `RevealIntent`）与 `ADR-012`
  （图标系统与资源布局：废止字形图标、规格即数据 16×16 栅格、草稿旁车
  `<doc_id>.draft`+`.path`、字体外置随包分发）。**编号偏差**：方案原定的
  「表现层状态所有权」主题已由 R-13 先行交付的 ADR-010 覆盖，故不重复造。
  `docs/README.md` 索引补 011/012，与 `docs/adr/` 文件数一致。
- **R-20 · `infrastructure/fonts.rs` 拆分**：1111 行拆为 `fonts/discovery.rs`（路径发现，
  接受注入根目录）/ `fonts/assembly.rs`（纯装配）/ `fonts/mod.rs`（门面重导出，
  `app/frame.rs` 调用点零改动）。**6 例依赖真实字体文件的用例改为缺失即 skip**
  （干净克隆不再红），新增注入式单测。测试 → **2098**；release exe 11.37 MiB（体积差 ≈ 0）。
- **R-21 · 低成本清理包**：两处 `highlight_cache.rs` 改名（`galley_cache.rs` /
  `span_cache.rs`，`git mv` 保留历史）；打开管线归位 `workspace.rs`；状态栏方法并入
  `frame/status_bar.rs`；`insert_at_cursor` 归位 `editor.rs`；`group_search_hits` 归位
  `find.rs`；`thematic_break_is_parsed` 判据加强为「块数 3 + `BlockKind::Rule` + 源码区间」。
  ★ 方案第 2/3 项（shared 归位与内部环）**经实测不成立**（相关模块确有表现层依赖），
  另立 V11-55 处置。

### 2026-10-21 · 表现层状态所有权收敛（R-11 / R-12）+ 切标签保留光标（R-06）

- **R-11 · 46 处直写收敛**：`src/presentation/views` 对 app 状态字段的**真实赋值**
  由 46 处降到 0（方案所称「59 处」含 13 处 `==` 比较误报）。按 state 结构体分五组推进、
  每组一个 commit，收口到 `EditorApp` 的方法入口。★ 已知边界（文档已标注）：
  `TextEdit::singleline(&mut app.x.y)` 与 `ui.checkbox(&mut app.x.y, …)` 因 egui 控件
  必须拿 `&mut`，保留原样——改成「返回 `&mut` 的访问器」只是换名字，所有权并未收敛。
- **R-12 · 上线机械规则**：`tools/arch_check.rs` 新增 token 级判定，表现层出现
  `app.<字段> =` 赋值即构建失败（`==` / `!=` 与 `&mut` 借用由 token 区分自然放行，
  **不设豁免**）。顺带收口 R-11 两层 grep 漏掉的**单层**直写
  （`app.editor_text =` → `EditorApp::set_editor_text`）。测试 2086 → **2090**。
- **R-06 · 切标签保留光标**：新增 `per_doc_cursor`，切走时 stash、切回时按新文本
  `byte_to_char` 换算后经 `pending_cursor` 恢复。★ 两个踩坑：关的不是当前页时不能 restore
  （快照陈旧会打回历史位置）；新打开的文件绝不 restore（恢复通道会把
  `focus_at_end` 落定的光标强设回 0）。测试 → **2093**。

### 2026-10-21 · 1MB 档性能口径裁定（V11-41，口径 A）与 shared 内部环破除（V11-55）

- **V11-41（口径 A 裁定）**：1MB 档定位为「尽力而为，不自绘」——p50 **14.35ms 已达标**，
  p95 贴 16ms 线（15.6~17.3ms）**未稳定达标**；继续下压需绕过 egui `TextEdit` 的 galley
  契约（自绘只读视图 / 视口虚拟化），成本远超收益，本期不做。
  ★ 其第②点「自绘 vs 修订口径」议题（T5）**已于 10-19 由排版层「原地补丁」先行闭环**
  （排版层 4.57 → 0.241ms，1MB p50 17.62 → 14.35ms）。同步 README 已知限制第 9 条、
  `docs/metrics.md` §1、`REQUIREMENTS.md` NFR-PERF-04/07。
- **V11-55（破环）**：`file_filter()` + `FILTER_NAME` 从 `text_format.rs` 迁入 `dialog.rs`，
  shared 内部依赖图由「唯一环 `text_format ↔ dialog`」变为**全单向无环**。
  对拍用例锚定源头（`BUILTIN_TEXT_EXTENSIONS`）而非与 `file_filter` 结果比对——
  后者是同义反复。测试 2098 → **2099**（`2099 passed · 0 failed · 4 ignored`）。

### 2026-10-19 · 大纲点击跳转精确定位到标题顶部（用户截图反馈）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 066；任务状态：`docs/v1.1-backlog.md` V11-53。承接 065（大纲行垂直对齐）。

- **问题**：点击侧栏目录条目后，编辑区没有滚动到该标题所在内容区域的顶部——真帧探针实测两个反模式：标题已可见但不在顶部时，点击后 `scroll_y` 纹丝不动（1152.00 → 1152.00，「点了没反应」）；标题在视口外时落在距顶 156.25pt（视口上部 1/4）处而非贴顶。另有一个渲染层系统偏差：所有跳转（含「预览点击 → 编辑区」同步）恒定偏高 4pt。
- **根因**：① `scroll_to_offset` 把查找「下一个」的防抖策略（已可见就不动 + 落视口 1/4 处）套给了大纲跳转这类**离散跳转**——「落在视口里」对查找足够，对跳转不够。② egui TextEdit atom frame 有 4pt 竖向内边距（`inner_margin = button_padding`，本项目 y 向 = `space_xs` = 4pt），文本 Galley 顶端比滚动内容原点低 4pt，而 `line_tops` 是 Galley 坐标系——纯函数测试与 wiring 测试都抓不到这个常量偏差。
- **修复**：① 新增 `RevealIntent` 双意图分流——`Top`（离散跳转：一律贴视口顶、留 `TARGET_MARGIN` 8pt、不因已可见而跳过）与 `IntoViewport`（查找防抖：已可见不动、否则落 1/4 处）；查找「下一个」走后者，大纲跳转走前者（搜索命中/拼写建议同为离散跳转，与大纲共用一条路径，保证手感一致）。② 4pt 偏差改为**每帧实测补偿**：`ScrollSync::editor_text_top` 由编辑区每帧实测 Galley 顶端相对滚动内容原点的偏移——不硬编码常量、不被主题间距绑死、换 egui 版本自动跟上；「预览点击 → 编辑区」同步同源修正。
- **回归**：新增 `tests/presentation/outline_jump_scroll_test.rs` 3 条（全真帧：状态层目标值 ±2pt + inset ≈ 4pt + 渲染层标题行落视口顶 +8pt + 已可见也贴顶 + 末章夹紧且整行完整可见）；修复前两条精准红（1383.75 vs 1532.00、1152.00 vs 1224.00）；2 次变异验证（意图退回防抖 / 去掉 4pt 补偿，各自精准红）。门禁四绿：1817 passed · 2 ignored。

### 2026-10-18 · 大纲/检查行内多段文本垂直对齐（用户截图反馈）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 065；任务状态：`docs/v1.1-backlog.md` V11-52。

- **问题**：大纲行里「# + 标题 + 行号」不在一条水平线上，标题比另两段低半个行高（用户截图：「文案部分低于其他部分」）。根因：标题用 0 高的 `allocate_ui_with_layout` 限宽，0 高子矩形被行的 Center 交叉对齐填满行高后落在**行垂直中点**，其中的文字再按 egui「横向布局不向上越界」规则被压到中点之下。
- **连带发现并修复**：检查标签的问题行用 `with_layout` 排内容——它不限定子 Ui 高度，整行被放到滚动区剩余可用高的垂直中点（浮在面板中部），每行游标暴涨约 512pt 导致只有第一行可见，且点击命中区与文字相距 248pt。改用 `ui.horizontal`（与大纲/文件/搜索同构）后恢复正常。
- **修复**：大纲行子 Ui 高度改为传行高（`#` 标签实测高度），子矩形与行重合；检查行改 `ui.horizontal` + 命中区挂行矩形，`LINT_ROW_HEIGHT` 退役。截断/宽度约束逻辑未动。
- **回归**：新增 `tests/presentation/outline_alignment_test.rs` 3 条（大纲三段同线 / 超长标题仍截断在侧栏内 / 检查行三段同线且三行都可见）；2 次变异验证（大纲高度退 0、检查退 `with_layout`，各自精准红）。门禁四绿：1814 passed · 2 ignored。

### 2026-10-18 · 欢迎页打开文件夹后切到工作区视图（用户反馈）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 064；任务状态：`docs/v1.1-backlog.md` V11-51。承接 062（文件树默认收起 + 开合持久化）。

- **问题**：从欢迎页「打开文件夹」后停在欢迎页原地——`should_show_welcome` 只判 `active_doc`，而打开文件夹不建文档；叠加 062 起侧栏默认收起，文件树扫完装好也没人看见，用户以为点了没反应。
- **修复**：① `should_show_welcome` 增加「且无工作区」条件，打开文件夹即进入工作区视图（编辑区由 `ensure_document` 每帧兜底建空白文档）；② 新增 `reveal_workspace_sidebar`（`PickFolder` 成功分支调用）：切「文件」面板、清掉上一个工作区的展开项（换根后旧路径全部失效）、按决定展开侧栏；「根 + 一层」由树投影 depth 0 恒可见天然保证，不需要额外展开动作。③ 「尊重手动折叠」用三态落实：`EditorSettings::sidebar_visible` 由 `bool` 改 `Option<bool>`——`None` = 没决定过（允许打开文件夹时自动展开），`Some(_)` = 明确决定（一律尊重）；旧配置缺字段缺省 `None` 而非 `Some(false)`（不该把没见过这个开关的用户锁死成「手动收起」）。落盘仍只在帧末一处，写 `Some(当前值)`。
- **回归**：新增 `tests/app/workspace_reveal_test.rs` 4 条（首次打开 / 切换文件夹清旧展开项 / 已决定收起时尊重 / 自动展开跨重启记住），全走真实入口 `apply_dialog_outcome`；加强域默认与旧配置两条既有用例；2 次变异验证（4 条全红 / 场景精准红）。门禁四绿：1811 passed · 2 ignored。

### 2026-10-18 · 侧栏切换按钮挪到面包屑行左端（用户反馈）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 063；任务状态：`docs/v1.1-backlog.md` V11-50。承接 061（按钮引入）/ 062（开合状态持久化）。

- **问题**：切换按钮在窗口右上角，侧栏展开时与它实际控制的侧栏隔了整个编辑区——用户反馈「与侧边栏间距过远，不人性化」。061 选右端是基于收起态的「空间对仗」，展开态下不成立：面包屑行从侧栏右侧起排，按钮与侧栏之间隔着整个内容区。
- **修复**：`draw_breadcrumb` 把按钮从行尾挪到行首（文档图标之前），矩形左界按 `clip_rect` 左缘 + 面板内边距推（与 059 右界同一纪律：环境可用宽不可信）；路径右界不再为按钮预留 24pt。不放侧栏自己的头部——收起后侧栏面板连头部一起消失，「收起」会变成单程票；行左端是两种状态下都紧贴侧栏的位置：展开时贴侧栏右缘、收起时贴窗口左上角，位置不随状态跳到窗口另一头。
- **回归**：`sidebar_toggle_test` 按钮定位从「行内最右描边圆角矩形」改为「行内最左」，新增两条位置断言——收起态按钮右缘 < 窗口宽 3%、展开态右缘 < 窗口宽 1/4（钉住「展开时不孤悬右上角」）；`breadcrumb_test` 不受影响（截断断言与按钮让位无关）。

### 2026-10-18 · 文件树默认收起 + 侧栏开合状态跨重启记住（用户四项反馈）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 062；任务状态：`docs/v1.1-backlog.md` V11-49。承接 061（侧栏默认收起 + 右上角收起/展开按钮已落地）。

- **「文件」标签下的文件树默认收起**：装载工作区后不再自动展开顶层目录——删掉 `SidebarUiState::needs_initial_expand` 与 `EditorApp::expand_top_level`（`parent_of` 随之退役）。当初自动展开的理由是「怕用户以为树是空的」，它不成立：`tree_rows` 里根的直接子项（depth 0）恒可见、被收起的目录自身也照画，只是不画它的子项。真正「按需展开」的是打开文件时 `expand_ancestors` 把该文件所有祖先目录展开到可见——故默认全收起不会让人找不到刚打开的文件（有回归成对钉住）。
- **记住上次的开合状态**：`EditorSettings` 新增 `sidebar_visible`（默认 `false`，`#[serde(default)]`——旧配置文件没有这个字段时按「收起」读出，而不是让整个配置加载失败），全链路接通：领域值对象 → DTO `EditorSettingsView` → `PreferencesAppService` 双向映射 → `EditorApp::new` 启动时读回（与 `split_ratio` 同一手法）。落盘点**只有一个**：`main_view::show` 的帧末（`EditorApp::persist_sidebar_visibility`，内部与会话级基准 `sidebar_visible_persisted` 比对，没变就直接返回）。不放各个切换入口——能改它的入口有四条以上，其中一个（侧栏活动栏点击）还不经 `execute`，逐个写就是漏一个静默失效。设置页「取消」与「恢复本组默认」对该字段的处理与 `split_ratio` 对齐。
- **右上角收起/展开按钮**：061 已落地（面包屑行右端常驻 `Icon::Sidebar`，动作走 `UiAction::ToggleSidebar`），本条未改动。
- **回归 6 条**：域默认收起与 `with_sidebar_visible` 单字段改写；旧配置缺字段按收起读出且不触发备份回退；应用级往返 `sidebar_visibility_round_trips_through_preferences`；真帧往返 `sidebar_visibility_is_remembered_across_restarts`（真帧点按钮 → 帧末落盘 → 换实例仍展开 / 反向仍收起）；文件树默认全收起 + 打开文件仍展开祖先。变异验证：注掉帧末落盘或注掉启动读回，两条往返用例当场变红。
- **边界**：文件树的展开/收起仍只活在会话里——展开状态是一串目录路径，工作区一换就全部失效，持久化它等于记住一堆不存在的目录。

### 2026-10-18 · 三连修复：页签重叠 + exe 终端 + 侧栏默认收起/展开（用户三项反馈）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 061。

- **标签条页签重叠**：真帧探针实锤两层根因——① 循环体内 `ui.put`（标题 / × 按钮）把父 Ui 光标推到 put 矩形右缘，下一槽位从被污染的光标出发，步进 166 ≠ 槽宽 170，底色重叠 2pt；② `ui.put` 子布局居中（`centered_and_justified`），**没被 elide 的标题从矩形水平中点起画**，长标题右移 ~65pt 压进关闭按钮区与条带右端（059 的面包屑「未命名文档」偏右即同一机制）。修：槽位 x 按序号显式推进 + `Label::halign(Align::LEFT)`（面包屑与欢迎页最近文件同步修）+ `TAB_GAP` 2→8。回归：`tabs_never_overlap_and_titles_stay_in_their_tab`（× 中心距 = 槽位步进 + 标题右缘不越本标签，修前双红）。
- **release exe 启动带终端**：`main.rs` 缺 `windows_subsystem` 声明，Windows 默认按 console 子系统链接。加 `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`——release 隐藏终端，debug 保留控制台（`RUST_LOG` 日志要经它）；macOS / Linux 的 GUI 程序不经 console 子系统启动，无此现象。
- **侧栏默认收起 + 右上角收起/展开按钮**：`AppUiState::new` 的 `sidebar_visible` 默认 `false`（会话级，不落盘）；新增 `Icon::Sidebar`（圆角外框 + 左栏竖线）与面包屑行右端**常驻**切换按钮——收起后整个侧栏面板含头部都不渲染，头部入口必成单程票；动作走 `UiAction::ToggleSidebar` 与 Ctrl+\ / 视图菜单同路径。回归：`sidebar_defaults_to_collapsed_and_topbar_button_toggles_it`（默认收起 / 按钮右上角定位 / 点击往返 / 展开有内容）。

### 2026-10-18 · 面包屑「文件路径」底部被遮挡修复（用户二反馈）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 060；任务状态：`docs/v1.1-backlog.md` V11-47。

- **问题**：059 修完右缘硬切后，用户二反馈路径文字**底部被面板底缘裁掉 1pt**。
- **根因不在截断逻辑，在顶栏纵向预算**：分段实测（`ui.cursor().top()` 埋点）——菜单栏实占 ~45pt（超 40pt 设计值 ~5pt）、`ui.separator()` 预留 ~10pt，二者把面包屑行挤到 `cursor.top=84`、面板底缘 `clip.bottom=100` → 面包屑带仅 **16pt**；路径文字 15pt 高、加字脚（font leading/descender）视觉高 ~17pt，`visual_bounding_rect` 实测 `86..101`、底部探出面板 1pt 被裁（egui `Label` 在 `put` 内 `rect.left_top()` 顶对齐、视觉外框含字脚 → 底缘溢出）。
- **修复两层**：① `ui.separator()` 改 `egui::Separator::default().spacing(1.0)`——只画 1px 发丝线、不再占 ~10pt 布局高度（该留白本就不在顶栏预算 `topbar+tabstrip+tabbar=100` 内），省下的空间还给标签条与面包屑，面包屑带回 ~25pt；② `draw_breadcrumb` 内 `ui.horizontal` 改 `ui.horizontal_top`（顶层横向排布必须顶对齐，项目 egui 布局纪律），避免子控件在窄带里被交叉轴居中。
- **回归**：`tests/presentation/breadcrumb_test.rs` 在原 2 条（右缘 + elide + 全文）基础上，各加一条「底缘不越面板」断言（`rect.bottom() <= panel_bottom + 0.5`，`panel_bottom = topbar+tabstrip+tabbar`），窄/宽窗口实测文字底缘均 ≤ 面板底缘。**边界交代**：菜单栏溢出其 40pt 设计值是既有的（注释已预警），本条只治面包屑被挤的症状、未动菜单栏自身高度逻辑；要彻底对齐预算应另立条目让菜单栏高度回落或把分隔线高度计入 `height`。

### 2026-10-18 · 面包屑「文件路径」截断修复（用户反馈）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 059；任务状态：`docs/v1.1-backlog.md` V11-46。关联：底部被遮挡见 060。

- **问题**：标签条下方的文件路径行，长路径延伸出面板右缘被硬切（截图实证 `logs` 只剩残形）。
- **根因两层**：`draw_breadcrumb` 裸 `ui.label` 不截断；且 egui 的 `Placer::expand_to_include_rect` 把 Ui 的 min_rect 与 max_rect 一起扩——同面板里更早的超宽内容（菜单栏/标签条横向溢出）把「环境可用宽」连带污染，`truncate()` 挂上后 put 的矩形右界照样漂出窗口（360pt 窗口实测漂到 440）。
- **修复**：`ui.put` 显式矩形 + `Label::truncate()` + `show_tooltip_when_elided`；矩形右界按 `clip_rect`（不随内容膨胀的唯一稳定面板界）对称推出，上下界钳到 clip（顺带修掉该行底部越出面板底缘 4pt 的隐性溢出——**该 4pt 实为常态、非本项主因**，真正的底部裁切由 060 的根因解释）。
- **回归**：`tests/presentation/breadcrumb_test.rs` 2 条真帧用例——窄窗口超长路径断言右缘不越界、`galley.elided` 置位、galley 全文保留完整路径（elide 只发生在字形层，语义不丢）；宽窗口短路径断言完整显示不 elide。

### 2026-10-18 · 欢迎页「最近文件」倒序 5 条（用户诉求）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 058；任务状态：`docs/v1.1-backlog.md` V11-45。

- **欢迎页「最近文件」展示数 8 → 5（用户诉求「倒序显示 5 条就行」）**：顺序侧无需改动——领域层 `RecentFiles` 按 `last_opened` 降序（最新在前），应用层只映射不改序，呈现层按序渲染，中文语境时间列表的「倒序」本就成立；仅 `MAX_RECENT` 由 8 收敛为 5，页脚「另有 N 个更早的文件」随之按新上限报数。新增呈现层回归（`tests/presentation/welcome_view_test.rs`，3 条真帧用例）：从领域到屏幕之间还有「DTO 映射 / 截断 / 渲染顺序」三段，任何一段被改坏都会域内全绿而用户看到另一个列表——注入 7 条最近文件（`TestClock` 逐条推进，避开同毫秒并列），钉住「恰好 5 行、`r6.md … r2.md` 最新在上、页脚报数」。边界：领域上限 20 与快速打开面板候选上限均未动。

### 2026-10-18 · 工具栏纯图形图标重构（用户诉求：「图片+文字」组合图标 → 纯图片形式）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 056；任务状态：`docs/v1.1-backlog.md` V11-43。

- **工具栏「字形 + 文字」组合图标 → 纯图形向量图标（用户诉求）**：原实现用字体字形当图标（`B` / `I` / `S` / `<>` / `🔗` 各配一行文字标签），两个方向不及格——字形落在已装载字体之外就是豆腐块方框（本项目已实测 `▾ ◂ ▦ ⌫ ⇥ ⤒ ↹ ✓` 一批符号无字形，且覆盖率随平台变化、只能逐台实测），而单字形只能是「一个字符」，谈不上配色、层次与风格。重构：① 新建 `presentation::icons`——**图标规格 = 数据**：每枚图标是一张 `IconOp` 图元表（直线 / 折线 / 二次弧 / 三次弧 / 圆 / 圆角矩形），坐标写在 16×16 设计栅格，每个图元自带 `Paint`（描边 / 实心）× `Tone`（主色 / 强调色）两维自由度，「融合多种视觉风格、配色与图形元素」即图元表的自由组合——新图标只往表里追加几行，渲染器一行不改；规格表经 `OnceLock` 缓存，每帧零分配。② 编辑区工具栏 5 个平铺按钮改为 30×30 图标按钮：手绘 `allocate_exact_size` + `Sense::click`，底色取 `widgets.style(&response).weak_bg_fill`、图形取 `fg_stroke.color`——与 egui `Button` 像素级一致（悬停 / 按下换档同源），可发现性由 `on_hover_text(action.label())` 补全。③ 图标语义与原动作一一对应：加粗 = 粗字身 B、斜体 = 右斜 I 带衬线、删除线 = S 拦腰横线、行内代码 = 双尖括号 + 强调色斜杠、插入链接 = 双色相扣链环。④ 面包屑的 `📄` emoji（同样依赖字体覆盖）换为同源 `Icon::Document` 向量图标。⑤ `UiAction::icon()` 那张 18 字形的死表（从未被渲染）与 `toolbar_label()` 一并删除。回归：4 条规格表纪律单测（非空 / 零分配 / 栅格边界 / 弧线采样）+ 工具栏视觉用例整文件重写为**几何定位**（每按钮 ≥2 个内嵌图元、图标中心偏离 <2.5pt、并集长边 9..18pt、平铺按钮等宽正方形）+ `toolbar_controls()` 清单与渲染层矩形对账；点击行为回归（真实指针事件点中按钮触发同一 `UiAction`）。**边界交代**：分组下拉触发器（「插入」「列表/结构」「行操作」）显示的是分组名而非动作图标，且原设计明确其可发现性价值，刻意保留文字；下拉菜单内的动作仍以文字菜单项呈现。问题记录 056。

### 2026-10-18 · Logo 投产重绘（AI 概念稿 → SVG 矢量 + 多尺寸位图）

> 原始诉求与逐项记录：`docs/logs/问题记录1.txt` 057；构建器 `assets/logo/build_logo.py`（几何唯一真源）。

- **定稿 Logo 按投产标准重绘为矢量并导出（用户诉求「按照投产标准重绘 SVG 矢量并导出」）**：定稿「收敛A」此前只以 AI 位图形式存档（`rustmark-logo.miora` + `rustmark-logo_assets/` 12 张参考图），不可投产。本轮从概念稿逐类阈值分割像素（墨黑 / 纸白 / Rust 橙三类）+ 逐行扫描连续段 + ASCII 拓扑图反推几何，收敛为一组栅格常量并写进**几何唯一真源** `assets/logo/build_logo.py`——SVG 与 PNG 从同一套坐标产出（矢量与位图同源），全产物可复现、可审查。设计规格：墨黑 `#1A1A1A` 2:1 圆角容器（200×100 栅格，圆角 r=10 = 容器高 10%）；纸白 `#F7F5F2` hash（56×61，两条通高斜杠 + 1/3、2/3 位斜横杠，整体右倾 11.3°）与右向圆角三角（圆角后墨迹盒 48×55，上/下顶点 r=7、尖点 r=5）；Rust 橙 `#CE422B` 分割线（50% 居中、宽 6、通高）；词标 "rustmark" 取 Bahnschrift 可变字体 600/87.5 实例（fonttools `instantiateVariableFont` 钉实例出 SVG 路径，PIL `set_variation_by_axes` 同参出位图，两路同源），墨高 58 = 58% 容器高，与容器间距 5%。**四条归一化决策**（AI 稿几何自相矛盾处的收口，均向原始概念与暗底定稿对齐，亮底定稿被判定为 AI 编辑漂移——其分割线位置 38.6% 与另两版矛盾、右缘还有渲染伪影）：① 分割线取 50% 居中；② hash / 三角各自居中于所属半区；③ 词标间距 2% → 5%；④ 词标字身取可得的最近似几何无衬线（概念稿原字体无系统对应）。产物：`rustmark-mark.svg`（标态）、`rustmark-logo.svg`（墨字横版）、`rustmark-logo-on-dark.svg`（纸白横版，深底用）；`rustmark.ico`（16~256 七级，PNG 数据条目，Vista+ 通用）、`rustmark.icns`（11 条目含 @2x，macOS）；`png/rustmark-icon-{16..1024}.png` 九级方形图标、`png/rustmark-mark-{512,1024}.png` 紧贴标态、`png/rustmark-logo-{1024}.png` 与深底变体横版锁定。
- **小尺寸短板修复：方形容器「紧凑模式」图标变体**：投产自检发现 16px（窗口标题栏 / 任务栏小图标的主场景）下 2:1 标态的 hash 墨迹仅 ~4×4px，双斜杠与横杠糊成团块、3% 宽分割线不足半像素（取色已沉成 (68,26,19)）。同尺寸对比 16/24/32/48/64px 两变体后（记录 `assets/logo/reference/square-vs-wide.png`）增产紧凑变体：容器改 1:1 方形（100×100，r=12）、分割线加粗到 5% 宽并通高、hash / 三角按 0.66 重排——缩放上界由「hash 墨迹右缘距分割线左缘 ≥5」解出（两半区各自居中得 23.75/76.25，净空实测左 5.17、右 7.91），不引入新造型，两套共用同一 hash/三角几何。16px 下紧凑变体的双斜杠、交叉、三角与通高锈橙线均可辨。产物：`rustmark-mark-compact.svg`、`png/rustmark-icon-compact-{16..1024}.png`、`rustmark-compact.ico`。**用途分工**：窗口 / 任务栏图标（16px 主场景）用紧凑模式，关于框 / 启动页 / 商店配图等大尺寸场景用 2:1 标态；应用尚未接线窗口图标，选型无阻塞。**边界交代**：紧凑变体是派生小尺寸模式，容器比例偏离已定稿的 2:1；四条归一化决策值（50% 分割线、5% 间距、10% 圆角、Bahnschrift 600/87.5）均非 AI 稿原值，是设计决策而非实测复刻。自检：SVG XML 合法、ICO 各级回读、ICNS 长度账平、hash/三角墨迹盒中心精确命中设计值、PNG 取色与设计值一致。问题记录 057。

### 2026-10-16 ~ 10-17 · egui 0.37 升级 + 文件格式白名单 + 编辑区每帧成本 + 编辑区几何 + 击键帧埋点 + 1MB 根因归因 + 应用层 O(全文) 开销 + diff 字节层化 + 排版层去重复分配 + 零散每帧成本收敛 + 状态栏字数统计防抖 + compute_stats 六遍遍历合并

> 原始问题记录：`docs/logs/问题记录1.txt` 044~055；逐项任务记录：`docs/v1.1-backlog.md` V11-36~42；
> 迁移全量清单见 `docs/archive/stage-reports/egui-0.37-upgrade.md`；性能数字见 `docs/metrics.md` §3.2.0 / §3.2.2 / §3.2.3 / §3.2.4 / §3.2.5 / §3.2.6 / §3.2.7 / §3.2.8 / §3.2.9 / §3.2.10；
> 1MB 击键帧归因报告见 `docs/archive/stage-reports/textedit-cost-attribution.md`（含三轮测量 + §14.3.1 建议二落地 + §14.3.2 建议四结论）。

- **egui / eframe 0.33.3 → 0.37.0（用户诉求：TextEdit 大文本卡顿）**：连带 `wgpu` 27.0.1 → 30.0.1，文本栈换代（`ab_glyph` → `harfrust` + `skrifa` + `vello_cpu`）。20 条 API 变更逐处迁移（`App` trait 拆 `ui` / `logic`、`Panel` 统一 + `exact_size`、`Context::run` → `run_ui`、`FullOutput` 的 `TexturesDelta` 需清空……）；两处 IME 行为变更最费时（0.37 的 `Commit("")` 不再等价于折叠选区、事件不再稳定排序到最前）；`LayoutJob::debug_sanity_check` 抓出既有的「预填 `text` 又 `append`」缺陷；emoji 字体拆到 `monochrome_emoji_fonts` feature 否则符号面板全豆腐块。回归：`tests/support` 新增 `run_ui` / `settle`，性能三项实测改善（1MB 冷首帧 47.9 → 19.23ms）。问题记录 044。
- **文件打开格式白名单（用户诉求：图片/二进制不得当文本打开）**：新建 `shared::text_format` 作唯一出处（内置 `.md` / `.markdown` / `.mdown` / `.mkd` / `.txt` / `.text` / `.csv`，大小写不敏感，前导点与无扩展名不算扩展名）。此前「哪些文件能当文本打开」散在三处各写一份且已漂移（拖放放行 `.csv`、原生对话框选不中）。`FilePath::extension`、原生对话框过滤器、四条打开入口（文件树置灰与双击拦截、拖放分类、快速打开拦截）全部同源；编码仍由基础设施层探测（白名单决定能不能开、编码探测决定按什么读）。问题记录 045。
- **编辑区每帧成本归因与修复（用户诉求，附 `docs/logs/T.txt` 审计）**：审计结论是该文的成因在本项目多不成立（编辑区自 S6-04 起就是整文档缓存 + 逐行差分，比其推荐的段落级缓存更细）；实测另找真因——**为两个整数付 O(全文)**：`highlight` / `spell_check` 每帧取全文只为拿版本号与字节长度，改走新增的 O(1) `revision_and_len`；高亮结果改 `Arc<Vec<SpanView>>` 共享（100KB 文档 3,080 个 span 不再每帧克隆）；行数 / 行号收敛到 `shared::text` 并按字节扫描（release 下 1.65× / 3.8×），光标行号只在行号栏开启时算。空闲帧 p50：10KB 0.14 → 0.11ms、100KB 0.56 → 0.33ms、1MB 4.44 → 3.16ms；剩余 1MB 约 3ms 全在 egui（0.37 的 `TextEdit` 无视口虚拟化 API）。问题记录 046。
- **编辑区框撑满面板（用户诉求，044 勘察的方向 A）**：那个圆角框是 `TextEdit` 自带的 frame，尺寸 = 控件尺寸，而控件高度默认只由 `desired_rows(行数)` 决定——短文档下框只包住文本，框底到状态栏之间留一大片裸底色（实测框 96pt vs 视口 625pt）。修复为滚动区 `auto_shrink([false, false])`（默认语义是「内容多高、滚动区就多高」）+ `TextEdit::min_size` 给满视口高（高度取 `ui.clip_rect()` 而非 `available_height()`，宽度一个点都不给），横滚动条随之落回面板底。新增 3 条几何回归（读 `read_response` 矩形与 `sync.geometry`，非截图比对）并做变异验证（撤销修复即红）。问题记录 047。
- **`NFR-PERF-04`「连续输入 100 次」整链路埋点（指标验收口径闭合）**：该指标此前只实测过**空闲帧**（§3.2.2），击键帧一直挂账「待阶段 09」。补齐：注入**真实的** `Event::Text`（走 egui 文本输入 → `TextEdit` 改写文本 → 应用侧 diff / 排版 → 重绘，而非直接改 `editor_text`）、每帧显式推进 8ms（约 125fps 连续人手节奏，间隔 < 150ms 防抖窗）、用**可控时钟**装配（真实 `SystemClock` 下用例会随时钟漂移改变被测行为——跑得快全在窗内、跑得慢中途越窗触发一次 59ms 级全量解析）。可控时钟从 IME 测试提升为共享设施 `tests/support/clock.rs`。三档实测（release p95）：10KB **0.58ms** ✅ 余量 27× ｜ 100KB **4.01ms** ✅ 余量 4× ｜ 1MB **42.00ms** ❌ **超 2.6×**——1MB 超限根因是 egui 文本一变即重建 58,514 行 `Galley`，而 0.37 的 `TextEdit` **无视口虚拟化 API**（与 §3.2.2 结论 6 同源），本项目不可规避，故阈值按档位分（≤ 500KB 守 16ms 指标本身，1MB 给 64ms 防数量级退化）**而不假装达标**，事实与归因写入 §3.2.3。回归以**计数**为主：100 次输入期间高亮缓存 `misses` 增量为 **0**（打字期一次全量高亮都没发生，三档一致），对照实验停手越窗后必须 +1，**变异验证**注掉防抖短路即抓到 **99 次**解析。顺带查清并记录「高亮查询跑在把缓冲写回文档之前、见到的版本总落后一帧」这一事实（现状无害，但回归必须照实写，否则误判成防抖失效），新增可观测入口 `HighlightPace::snapshot` / `MarkdownAppService::highlight_pace_snapshot`。问题记录 048。
- **1MB 击键帧根因被实测推翻（用户要求「跑验证测试并输出分析报告」，修正 048 的判定）**：048 把 1MB 超指标的根因判为「egui 文本一变即重建 58,514 行 `Galley`，0.37 无视口虚拟化 ⟹ 要达标只能自绘编辑器」。新增归因用例（`tests/presentation/textedit_cost_attribution_test.rs`）八组对照后该判定不成立：① **100 帧连续击键里每帧真正重排的行数 `max = 1`**（读 `HighlightStats::reshaped_lines`，三轮独立运行一致），整篇重排从未发生；② 裸宿主（`ScrollArea` + 撑满视口的 `TextEdit`，与真实编辑区同构）同文本、只换 layouter：本项目逐行 layouter p50 **17.24ms** 反而快于 egui 默认 layouter **19.04ms**——上游 0.37 的按段落缓存（`galley_cache::should_cache_each_paragraph_individually`）因本项目自行拆成单行 job（单行不含 `\n`）而**从未生效**，等价效果由自己的 `LINE_LAYOUT` 提供，故自研排版层不是负担、「换回上游」也不是出路；③ 40.92ms 的真实构成是「空闲帧 7.06 + 排版层 9.24 + `editor_ops::diff_replace` 6.93 + 余下 17.68」，其中 **`E1 − E7 = 23.67ms`（占 58%）全部来自应用层的文本变更链**——自绘编辑器替换的是编辑表面与排版，动不到这部分。顺带定位一处确定性缺陷：`diff_replace` 把 1MB 文本两侧各 `chars().collect::<Vec<char>>()`（约 2×1.4MB 分配 + 两遍 UTF-8 解码）再调两次 O(k) 的 `char_to_byte`，单项 6.93ms，改在字节层求前后缀即可回收（余下 17.68ms 的嫌疑清单见报告 §5）。连带修正文档：`metrics.md` §3.2.2 结论 6 / §3.2.3 结论 3 保留原文并加 ⚠ 修正标注、新增 §3.2.4，`textedit_frame_perf_test.rs` 两处注释。问题记录 049。
- **049 遗留的 17.68ms 拆到代码行：最大单块是状态栏的整篇字数统计（用户要求「继续评估，性能损耗地方」）**：049 把 1MB 击键帧拆到「排版层 9.24 + `diff_replace` 6.93 + 余下 17.68」后，最后一项只按代码列了嫌疑清单。本轮用三种手段量化：**消融**（状态栏不是单一函数而是「若干次 `O(全文)` 查询 + 绘制」的合成，合成探针只能逐项估、且 `compute_stats` 是私有函数测不到，故只切 `status_bar_visible`，其余代码路径完全相同）+ **单项合成探针** + **长度斜率**（同一探针跑 1MB 与 10KB 两档）。实测（release p50）：**状态栏开 41.45 ／ 关 30.43 → 每帧 11.02ms**，账目自洽——每帧恒定 0.86ms（`line_start_offsets` 0.82 + `line_count` 0.28）、每击键 10.16ms（= `get_stats` **miss 帧 9.49ms**）：状态栏每帧查两次统计，缓存键是 `(id, revision)`，**击键帧 `revision` 必变 → 每敲一个键就把 1MB 全文重新统计一遍**，而 `compute_stats` 在 1MB 上做**约六遍全文遍历**（按行 `split` + 逐字节扫链接 + 逐字符扫字数 + `chars().count()` + `chars().filter(!ws).count()` + `line_count`）——与 046「为两个整数付 O(全文)」、049 的 `diff_replace`「为前后缀付两遍字符化」是**同一类缺陷的第三例**。**049 的嫌疑清单被实测否决**：`prev.clone()` 0.22、`format!` 重建最终串 0.37、领域 `replace_range` 0.28、`transform_insert`（1MB 普通击键只走「单字符插入」短路分支、不扫全文）——钱花在**没人列进清单的状态栏**上。**关掉状态栏仍 30.43ms**（超标 1.9×），但剩余大头分别在应用层、应用层、`TextEdit` 内部，**没有任何一项需要自绘编辑器**（049 结论 4 继续成立）。优化候选：字数统计**纳入防抖**（~9.5ms，与 `HighlightPace` / 预览防抖同源先例）／`compute_stats` 六遍遍历合并为 1~2 遍（~4~6ms）／`diff_replace` 改字节层前后缀（~6.8ms）；①+③ 预计 41.45 → 约 25ms。回归新增 `profile_1mb_app_layer_whole_text_costs`（含「关掉状态栏必须真的更快（差额 > 2ms）」与每组注入字节自检）；文档：报告新增第二轮 §8~§12（§5/§7 加 ⚠ 标注指向第二轮，不改写第一轮原文），`metrics.md` 新增 §3.2.5 并补顶部 `NFR-PERF-04` 行。问题记录 050。
- **`diff_replace` 改字节层前后缀（050 优化候选③落地，1MB 单次 6.93 → 0.24ms）**：两轮归因后 `editor_ops::diff_replace` 是唯一的「纯浪费」——它挂在**每击键一次**的路径上（`editor_view::show` 的 `changed()` 分支），却先把两整篇文本 `chars().collect::<Vec<char>>()`（1MB = 2 × 35 万 char，约 2.8MB 分配 + 两遍 UTF-8 解码），再用两次 `char_to_byte`（内部 `char_indices().nth(k)`，O(k)）把字符索引换回字节，**做这一切只为求一个公共前后缀**。改为在**字节层**比较前后缀（不分配、不解码），并用 `str::is_char_boundary` 把字节边界**回退**／**收拢**到字符边界——可这么做的前提是 UTF-8 自同步：某位置是否为字符边界只取决于该位置自身是否为续字节，而公共前缀段两侧逐字节相同，故边界判定两侧一致（回退/收拢不可省，`café`↔`cafè` 的字节前缀会停在 2 字节字符中间、`é`↔`Ã©` 的字节后缀会落在字符中间）；字符索引此时只需在**前缀处**算一次，光标 =「前缀字符数 + 插入内容字符数」。实测（release p50）：单次调用 **6.93 → 0.24ms（×27）**，1MB 击键帧 **40.4~40.9 → 33.7~35.5ms**（三轮区间无重叠；p95 43.42 → 36.12~38.56），降幅与该单项绝对降幅一致，反证这笔钱确出自此处；**位置敏感度消失**（文末 0.24 / 中点 0.33，旧实现与位置无关恒 ~6.9）。**对外语义零变化**——把旧的字符层实现原样留作基准，对 16 组输入（中文 / 4 字节字符 / 上述两个边界）逐字段比对相等；另加三条边界单测，**变异验证**去掉 `is_char_boundary` 调整即全部 panic 在「非字符边界切片」上。1MB 仍超 16ms 2.3×，剩余大头是状态栏整篇统计（9.30ms）与排版层（9.09ms），即 050 的①②待办。文档：报告新增 §13（§11 的 #3 标「已落地」），`metrics.md` 新增 §3.2.6、候选③加 ⚠ 标注并更新顶部 `NFR-PERF-04` 行。问题记录 051。
- **排版层缓存 miss 路径去重复分配（049/050 四项优化建议之二落地；E2 9.70 → 4.62ms，−52%）**：050 的 A2 消融把 1MB 击键帧拆到代码行后，剩余大头里 E2（`mapper::highlight_layout_with_find`，文本变 1 字节）实测 9.24ms，其中约 **6.6ms 是本项目自己的缓存簿记**——也是唯一「与 egui 无关、自绘也躲不掉」的可修项（归因报告 §14.3 用隔离微基准逐项量出：整文档缓存读取 `borrow().clone()` 每帧复制整条条目含 1MB 全文 0.23ms、上一版 `lines.clone()` 58k 次小堆分配 2.47ms、回写时 58k 行 `String` 全量重建 2.29ms、`galleys.clone()` 0.44ms、拆行 `collect` 反复扩容搬移 1.17ms）。五项改动全落地，**既有逻辑（`reuse_map` 三段差分、span 裁剪、查找命中叠加、DEF-05 的 `sections` 补全、大文件降级）一行未动**：① 整文档缓存改**在借用内比对**、文本判据取 `galley.text()`——拼回来的整篇 `Galley` 本就携带全文（`job.text`，双击选词依赖它），故不必另存一份 `String`（1MB 常驻内存随之消失），比原建议的 `Arc<str>` 更省、比哈希键更安全（逐字节精确，无「碰撞 → 残留旧字形」之虞，本项目的确定性纪律偏保守）；② 上一版行布局用 `mem::take` **整条移出**、用完在函数末尾写回；③ 行内容改**移动复用**（命中映射表即 `Option::take` 取走上一版那条 `String`，只有真正重排的行才新建）——映射表是单射（前缀段 `< pre`、后缀段 `≥ n - suf`、中段夹在两者之间，三段互不相交），故每行至多被取走一次，万一不变式被破坏也只退回「重新分配一条」而绝不 panic；④ 行 Galley 表**移动**写回；⑤ 拆行按**上一版行数**预置容量（刻意不用原建议的「线程局部可复用缓冲」：项目零 `unsafe`，跨调用复用 `Vec<&str>` 需擦除生命周期，且再扫一遍全文数 `\n` 自身就要 0.3~0.5ms）。**同机 A/B 实测**（release、`--test-threads=1`，`git stash push` 回滚两个源文件后用同一命令再测——不同机器状态的两组数字不可相减）：E2 **9.70 → 4.62ms（−52%）**、E2b 整文档缓存命中 0.297 → **0.029ms**、1MB 击键帧 p50/p95 35.62/38.36 → **29.83/31.83ms**、`NFR-PERF-04` 口径 1MB p95 37.08 → **32.91ms**（超 16ms 2.3× → **2.06×**）、100KB p95 3.40 → 3.02、10KB p95 0.59 → 0.49、E7 自家排版（裸 `TextEdit` 宿主）18.65 → **13.45ms**，而 **E3 上游默认 layouter 对照组持平（20.08 / 20.25）**——证明机器状态未漂移，故这 −5.20ms 确实出在自家排版层；`reshaped_lines` 20 次调用恒 `[1]`（计数证据，非耗时推断）。**自家排版层由「比上游快 1.43ms」变成「快 6.80ms」**，「换回上游 layouter」这条路彻底关闭，也为「不建议自绘编辑器」再添一条硬证据。**实际 −5.08ms 低于预估 6.4ms**，差额全是「本来就要做」的固定项（1 行真实排版 ≈1.5ms、`concat` 逐行克隆 `Arc<Row>` ≈0.4ms、`reuse_map` ≈0.7ms、拆行扫描 ≈0.5ms）——**微基准量出的是成本上限，不是可回收量**，后续三项的收益预估同样应按此折扣理解。回归：新增 3 条守卫 + 3 次变异验证（复用索引错位 / 文本判据退化为只比长度 / 映射表中段无条件配对，均当场变红），并顺带补掉**既有守卫的缺口**——`assert_rows_equivalent` 原先只比字形**个数**（`'d'` 与 `'1'` 都是 1 个），已强化为比字形**字符**。文档：`metrics.md` 新增 §3.2.7 并刷新顶部 `NFR-PERF-04` 行，报告新增 §14.3.1 并把 §14.1/§14.2 标注为「落地前」口径。问题记录 052。
- **零散每帧成本收敛（049/050 四项优化建议之四落地；按用户指定优先级 ④→①→③，本轮只落 ④）**：050 把 1MB 击键帧拆到代码行后，§3.2.5 的隔离微基准又把「每帧前置工作」拆成 6 项，其中 S3（行起点表 0.82ms）+ S4（行数 0.28ms）合计 1.1ms/帧，且被状态栏（光标行列、选区行列、「N 行」）与编辑区（`desired_rows`、行号栏宽度）**各扫一遍**——同一份信息每帧被重复扫描 4~5 次。四项收尾：① 新增 `LineIndex`：`new(text)` **一遍扫描**同时填行起点表与行数（容量按平均行长 32 字节估，避免「先 `line_count` 定容再拆一遍」的两遍扫描），`EditorApp::line_index()` 按 `(active_doc, registry.revision_of, editor_text.len())` 缓存为 `RefCell<Option<(key, Rc<LineIndex>)>>`，帧内其余调用 O(1) 取 `Rc` 克隆；`cursor_line_col` / `selection_line_col` / `current_line_count` 全部改用 `_in` 变体共用这一次扫描，`status_starts` 按 `count` 截断摘掉文末哨兵、逐字节等于旧 `status_line_starts`。② `preferences::service` 新增 `has_writing_goal()`；`sync_goal` 先问它，**未设目标时跳过整篇字数统计**（与 `goal_progress(0)` 逐字段等价的 `GoalProgressView::default()`，省掉一次 1MB 上的六遍遍历）。③ `UndoStack` 把合并判据抽成单一实现 `merges_with_last`，`push` 仍走它；新增 `would_merge` / `merge`，`Document::apply` 改为「先 `would_merge` 再决定**是否构造整篇快照**」——合并命中只滚动时间窗（零分配），连打字时每个键省一次 ~1ms 的整篇 `String` 复制（旧实现 `push` 每次都先 `snapshot_at` 再丢弃）。④ `prev = editor_text.clone()` **据实保留**：同文件里有两条与 `InputState::events` 无关的写入路径（无障碍 `accesskit_set_text_request` 的 `SetValue`、拖放文本插入），按「无事件即不克隆」门控会**静默丢编辑**，补兜底要么不触发、要么触发时正是要省的那次复制——两边都不划算，结论记入 §14.3.2。**派生收益**（非整轮 A/B，依 §3.2.5 实测外推，免环境漂移争议）：① 把 4~5 遍/帧 降为 1 遍 + 复用，1MB 上省 ~0.8~1.1ms/帧；③ 连打字每键省一次 ~1ms 全文 `String` 复制；② 默认（未设目标）每帧省一次 `current_stats()` 查表。**对可观测行为零变化**。回归：① `LineIndex` 的 `starts` / `count` / `status_starts` 在 12 个边界输入上逐元素等于 `line_start_offsets` 与 `line_count`；② 撤销合并行为不变（`merge_within_window_is_single_undo_point` 经 `Document::apply` 已走新路径）；③ 未设目标时状态栏整项不画。门禁：`fmt` / `check-arch` / `clippy -D warnings` 全绿，`cargo test --all-targets` **1764 passed · 0 failed · 2 ignored**（较 052 后 +2 条 `LineIndex` 等价守卫）。问题记录 053。
- **状态栏字数统计防抖（049/050 四项优化建议之一落地；1MB 击键帧最大单块 ~9.5ms 移出热路径）**：050 的 A2 消融把 1MB 击键帧拆到代码行后，状态栏每击键一次的整篇字数统计被点名为**最大单块**（~9.5ms / 占 23%）——`status_bar_view` 每帧查两次 `get_stats`、键含 `revision`、击键帧必失效 → `compute_stats` 在 1MB 上做约六遍全文遍历（与 046「为两个整数付 O(全文)」、051 `diff_replace`「为前后缀付两遍字符化」是同一类缺陷第三例）。修：① 新增 `StatsPace` 节拍器（`EditorApp.stats_pace`）+ 窗口常量 `STATS_DEBOUNCE_MS = 150`（与 `HighlightPace` 一致），`status_bar_view::show` 每帧经 `current_stats_for_status_bar()` 驱动；② 仿 `HighlightPace::settled`，**首次见/切换文档立即算**（无闪烁）、`computed_revision == revision` 直返缓存、停手满窗口才重算、否则沿用上次静止值（打字期显示「上一次静止时算出的字数」、停手满 150ms 才刷新，用户指定「停手后刷新」口径）；③ `show` 每帧只算一次，结果以拥有值 `Option<StatsView>` 同时传给 `sync_goal`/`build_model`/`draw`，同帧共享一次结算（满足 V11-39「同帧两次调用须共享一次结算」，防抖期开销不翻倍）；④ on-demand 统计浮层（`stats_view.rs`）仍走 `current_stats()` 即时，不受防抖影响——那是点击时刻用户主动请求当前精确值。口径：打字期字数数字更新节奏改变，其余可观测行为零变化；门禁全绿（`fmt`/`check-arch`/`clippy -D warnings` / `cargo test --all-targets` **0 失败**），含 2 条新增防抖测试（`status_bar_stats_debounce_holds_while_typing` / `status_bar_stats_debounce_recomputes_on_document_switch`）+ 2 条既有测试口径同步修正（`word_count_matches_the_prototype` 改适配「拥有值」签名、`reaching_goal_announces_once_on_crossing` 用可控时钟推进窗口）+ `profile_1mb_app_layer_whole_text_costs` 断言翻转（由「关状态栏更快 > 2ms」改为「开/关差额 < 3ms（≈0）」，锁定 `compute_stats` 已移出每击键热路径）。文档：`metrics.md` 新增 §3.2.9 并刷新顶部 `NFR-PERF-04` 行与 §3.2.5 处注记，报告 §14.6 优先级更新。问题记录 054。
- **`compute_stats` 六遍遍历合并为 1~2 遍（049/050 四项优化建议之三落地；1MB 单次结算 9.49 → 5.55ms，−42%）**：050 A2 把 `compute_stats` 在 1MB 上点名为约六遍全文遍历（按行 `split` + 逐字节扫链接 + 逐字符扫字数 + `chars().count()` + `chars().filter(!ws).count()` + `line_count`），建议一（054）已把该成本从每击键热路径移出（防抖到停手帧），但停手帧仍付一次。修：重写 `compute_stats` 为**两遍**——第一遍用持久的 `char_indices()` 迭代器同时驱动字符 / 非空格字符 / 词（CJK 按字 + 西文按词状态机 `in_token`/`token_has_alnum`）/ 行数，行尾以**零拷贝切片** `&text[line_start..bi]` 结算标题 / 围栏 / 任务 / 段落；第二遍仅 `count_links_and_images` 做字节层 `link_tail_len` 配对（链接/图片极少，绝大多数文档此遍几乎不做事）；**零分配**（词状态机、行内容零拷贝 `&str` 切片，不再为每行 / 每 token 建 `String`）；旧六遍实现原样留作 `reference_compute_stats`，23 组混合输入逐项相等守住对外语义。同机 A/B（release）：1MB 单次结算 **9.49 → 5.55ms（−3.94ms，−42%）**，落 V11-39 ② 估的 −4~6ms 区间；对应 `get_stats` miss 帧（S6）由早期 11.30ms 收敛到 5.55ms。门禁：`fmt`/`check-arch`/`clippy -D warnings` 全绿，`cargo test --all-targets` **0 失败**（含 2 条新增守卫 `single_pass_matches_reference_on_mixed_inputs` / `single_pass_is_faster_than_reference_ab`，后者 release 同机 A/B 钉住「单遍更快」、debug 只打印比值）；既有 `document_service_test` / `alignment_test` 数字断言全保持。文档：`metrics.md` 新增 §3.2.10 并刷新顶部 `NFR-PERF-04` 行与 §3.2.5 / §3.2.9 处注记。问题记录 055。

### 2026-10-11 ~ 10-15 · 用户反馈批次收尾 + 渲染增强 + 依赖精化 + 侧栏重构

> 原始问题记录：`docs/logs/问题记录1.txt` 011~043；逐项任务记录：`docs/v1.1-backlog.md` V11-16~35。

- **侧边栏条目点击全部失效（10-14 用户报告）**：大纲行 / 文件树行 / 检查行 / 搜索命中行的行容器用 `ui.horizontal(...)` / `allocate_ui(...)`，egui 子 Ui 默认 sense 是 **hover**（`UiBuilder::sense` 默认值），`Response::clicked()` 恒为 false——页签（`selectable_label`）能切而条目全废。四处统一改 `scope_builder(UiBuilder::new().sense(Sense::click()), ...)`；附真实指针事件回归（点击大纲 → 光标跳行，修复前实测失败）。
- **预览区横向滚动条（10-14）**：开启「自动换行」时代码块与 Mermaid/公式降级源码卡仍写死 `wrap_width = INFINITY` 不折行。两类源码卡增加 `wrap` 分支：开 → 按面板可用宽折行；关 → 保持不折行由横向滚动条查看（既有设计）。
- **图片按 URL 尺寸参数渲染（10-14）**：新增 `parse_image_size_params`（阿里云 OSS `x-oss-process=image/resize,w_<W>[,h_<H>]` 与通用 `?w=&h=`）与 `fitted_size`（参数只收紧不放大、contain、面板上限兜底），随纹理缓存；图片块水平居中；加载失败的占位宽度也按参数区分并显示「请求尺寸」。URL 参数自 parser 至渲染层完整保留，附 parser 链回归。
- **图片按源码语法参数渲染（10-15 用户报告，修正上一条的方向）**：文档实际语法是 Typora / CSDN 私有扩展 `![alt](url#pic_center =30x30)`——「地址（本地 / http / https）+ #位置 + =大小」，上一条的 URL query 解析覆盖不到。此前脱糖步骤（`desugar_image_sizes`）把 `#pic_*` 对齐标记与 ` =宽x高` 尺寸后缀**直接剥掉丢弃**，参数从未到达渲染层。修复为四层贯通：领域 `BlockKind::Image` 增加 `display: ImageDisplay`；解析层新增与脱糖同源的 `parse_image_display` 与锚点提取 `image_display_at_source`（pulldown `End(Image)` 区间终点紧跟原文中本图 `)`，回推一字节即得完整目标地址，多图段落逐图提取互不串扰）；DTO / view_mapper 透传 `image_width` / `image_height` / `image_align`；渲染层 `effective_size_param`（语法参数优先、URL 查询参数兜底，不混用）与 `image_align_of`（居中 / 左 / 右）。附 21 条新回归（含「恒返回默认值即失败」的有效性验证）与 golden 快照更新。
- **图片位置摆放补充（10-15 用户补充）**：规则为「要居中就在图片范围内最后加 `#pic_center`，居左 / 居右加 `#pic_left` / `#pic_right`；参数规则可自定义」。效果截图（CSDN 编辑器 + 预览同屏）证实**无标记图片左对齐**——上一条的「未写标记默认居中」被证伪，`ImageAlign` 默认值改为 `Left`；两类参数改为**顺序无关**（`url#pic_center =30x30` 与 `url =30x30#pic_center` 解析一致：脱糖新增 `closes_destination` 放行「尺寸后缀后随完整对齐令牌」、`in_link_destination` 放行带尺寸后缀的地址前缀，解析扫描接受尾部对齐令牌）；参数识别**规则表化**（对齐令牌地址内任意位置 / 尺寸后缀仅地址结尾；新增规则只改 `image_size.rs` 一处，脱糖与解析自动同源，不变式由测试守住）。附反向顺序全链与示例文档四形态（无标记 / 仅尺寸 / 仅居中 / 居中+尺寸）全链回归；golden 快照两张普通图片 `align: Center → Left`。
- **图片前的文案被吞（10-15 用户报告）**：`图片: ![Alt](url)` 里图片能渲染，但「图片: 」不显示。根因是图片虽为**行内**元素，解析器却在块闭合时只要块内出现过 `Tag::Image` 就把**整块替换成图片块**，块内其余文字随之丢弃（同处代码还导致一段多图只留最后一张、列表项与引用里的图片整张消失）。修复为把「行内图片」建模出来：领域层新增 `InlineImage`（区间 + alt / url / display），挂在 `RenderedBlock` 与 `ListItem` 上；解析层把 image 单槽改为 `images` 列表 + `image_open`，块闭合时**只有整块就是图**才升级为图片块，混排则保持原块类型（文本回流 / 任务标记裁剪时同步平移图片区间）；DTO 增加 `BlockView.images` / `ListItemView.images`；渲染层新增 `draw_text_with_images`，按图片区间把文字切成「文字段 / 图片」序列（段内样式片段同步平移），段落 / 标题 / 引用 / 列表项统一走它，图片块与行内图片共用同一条绘制路径。附 9 条新回归与「回退旧行为即失败」的有效性验证；golden 快照仅 `ListItem` Debug 多出 `images: []`。
- **搜索标签黑框复发修复（10-15 用户报告）**：025 修过 lint 行溢出导致的同类黑框，本条是**另一处复发**且更深一层——`hit_row` 把片段拆成「前 / 命中 / 后」三个独立 `Label::truncate()`，前两段吃光可用宽后第三段只剩约 5pt，而 epaint 的截断保证「行不为空」（宽度装不下任何字形时仍排出一个字形），该段 galley 于是越界、本行 `min_rect` 冲出面板；侧栏 `ScrollArea` 横向按 `Expand to fit content` 把溢出宽度记进 content_size 并在同帧内正反馈放大，`SidePanel` 再按内容矩形最大 x 把编辑区整体推右，面板外无人绘制露出交换链清屏色。修复为三段拼成**一个** `LayoutJob`（命中词 section 带底纹）后只截断一次，并新增字符下标→字节区间的边界对齐换算。回归：真实磁盘 + 用户真实字体渲染三帧，断言侧栏内容右界 ≤ 面板宽、编辑区左界不被推右、命中片段必须是单个 galley（旧实现下必失败）。问题记录 033。
- **搜索崩溃修复（10-14 用户报告）**：跨文件搜索的摘要窗口按字节截取（前后各 24 字节），混合字节宽度上下文（中文 3 字节 / emoji 4 字节）下落点切进多字节字符中间导致 panic（`is not a char boundary`）；修复为切片前把窗口两端向下对齐到字符边界（新增 `floor_char_boundary`），摘要语义不变，附混合宽度回归用例。
- **搜索范围切换器（10-15 用户请求，布局重构第一步）**：侧栏按作用域分组后「搜索」归入文档级分组，但实现仍是全工作区检索，与旁边只看活动文档的大纲 / 检查形成语义断层。新增「当前文档 / 整个工作区」范围切换（默认当前文档，会话内有效）：`SearchQuery` 增 `only_document`，服务在候选收集后、读盘前按文档 ID 过滤（未落盘草稿同样有 ID，天然复用「草稿优先于磁盘内容」规则）；「当前文档 + 无活动文档」返回空结果而非退化成全工作区，视图给「没有打开的文档」空状态并指路切换范围；输入框占位提示随范围变化。回归：应用层限定过滤、表现层默认范围无文档必空 / 打开后只剩活动文档 / 点击切换器（落点取字形中心）后结果立即刷新，两处守卫均经「改坏必失败」验证。问题记录 034。
- **用户反馈五项修复（10-15 用户报告，截图 ×3）**：① 侧栏文件树此前用字形图标（`▾` 在本机字体链路无字形，渲染成豆腐块）且无树形结构、无双击打开语义——图标全部改为 painter 向量绘制（目录 = 三角箭头 + 文件夹，文件 = 折角页面）、新增逐级缩进树线，交互改为「目录单击展开/收起、文件单击选中、双击打开」（Markdown / TXT 兼容本就是打开过滤器既有能力，树形态修复后即可用）；② 快速打开搜索栏文字重叠是**双绘**——同一矩形先手绘一版 galley 又叠一个带 `hint_text` 的 `TextEdit`，删手绘只留 TextEdit，同列表长文件名一并改截断；③ 欢迎页「最近文件」长文档名直接排全名压住后方路径，改为先量目录宽度再按剩余空间截断（窄屏保名字舍目录，全路径看悬浮提示）；④ 搜索范围按钮选中态文字取强调色，与用户自定义主题的实色选中底色（恰为同色）撞车导致不可见——选中态文字改主文字色，同时范围语义按用户要求从「整个工作区」（读磁盘全部文件）改为「打开的文档」（标签条中所有打开的文档、含未保存草稿），读盘跳过做在候选收集阶段；⑤ 同一文档重复打开会出现重复标签——五条打开入口（文件树 / 搜索跳转 / 最近 / 快速打开 / 拖放）全部汇聚于 `open_workspace_file`，在此一处按路径去重（命中已打开则切换并选中）；标签条旧用可换行横向滚动区，标签多时换行撑爆顶栏高度预算、面板按内容伸展导致中央区顶边露出黑带——重写为单行横向滚动 + 高度钳制，标题截断、关闭按钮常显。回归：新增「打开的文档范围不搜未打开文件」用例、搜索结果分组顺序按打开顺序（注册表改插入序）、连带修好顶栏高度两条既有回归。问题记录 037。
- **用户反馈三项修复（10-15 用户报告，截图 ×1）**：① 「保存全部」没效果——`UiAction::SaveAll` 在动作表、「文件」菜单、快捷键（`Ctrl+Alt+S`）、命令面板四处都有，唯独汇聚点 `EditorApp::execute` 落进 `_ => {}` 兜底（与当年 `Ctrl+S`/`Ctrl+N` 静默失效同根因）；接线并新增 `save_all_documents`：逐个保存脏文档、未命名文档单独计数（提示「需先另存为」而非报失败）、单个写盘失败不中断其余、汇总报告，无脏文档时明确提示。② 标签条三项：新增打开文档数量上限（`EditorSettings.max_open_documents`，默认 12、范围 1..=30，**serde 入口把手改的 0 收敛为 1**——仓储对校验失败是「备份+整体回退默认」，为单字段 0 丢全部配置不可接受；限额检查在去重**之后**，已打开文档走切换不占名额，`open_workspace_file` 与 `new_document` 两处覆盖）；标签条右端常驻「全部关闭」按钮（量宽 + 固定矩形 + 分隔线 + 手绘 hover），有脏文档走原生确认框（复用 V11-06 的 pending + 后台确认模式，新增 `pending_close_all`，Confirm 出口按 pending 分流），关最后一个标签与按钮同险同防；竖向堆叠的根因是 `ScrollArea` 内层 Ui **继承父级 top_down 布局**——横向是内层 Ui 的属性（`ui.horizontal_top`）而非滚动区属性，且滚动条在视口内部占位会吃掉行高并引出无用竖条，故横向滚动区用 `AlwaysHidden`（只隐藏绘制，滚轮/拖拽仍可滚）。③ 设置页「恢复默认快捷键」从快捷键列表滚动区最底部移入对话框页脚（与「恢复本组默认」互斥显示），不再需要滑 60 个条目。回归：新增 save_all（5 例，含失败不中断与未命名计数）、open_limit（5 例，含「上限时仍可切回已打开文档」）、close_all_confirm（7 例）、tab_strip（4 例，含溢出时「全部关闭」仍可见与单行锚定）四个测试文件；仓储层补「手改 0 收敛为 1 且无备份」「缺字段按 12 读出」；设置页脚锚点用「取消」而非「保存」（列表里也有「保存」动作名）。问题记录 038。
- **用户反馈三项修复（10-16 用户报告，截图 ×2）**：① 侧栏「文件」标签下点击文件夹展开后，子项不显示在文件夹下方而是**拼接到文件树底部**（`archive` / `logs` 的子项全挤在所有根级文件之后）。根因不是折行算法，而是**遍历顺序违约**：`sidebar_ui_state::tree::tree_rows` 的输入契约是「深度优先前序（父目录紧跟其子项）」——它按列表顺序逐项渲染、刻意不重排（重排会破坏「重命名后位置不动」），而 `FsWorkspaceRepository::collect` 的写法是先推完本层全部条目、循环末尾再统一递归子目录，产出的是「本层全部条目 → 再逐目录递归」，所有子项被推到列表尾部。修复为前序 DFS：推入目录条目后**立即**递归（`max_depth > 0` 时）再继续兄弟，截断语义不变。连带纠正 `sidebar_view_test::tree_projection_builds_hierarchical_rows`——它把这份错误顺序（`["docs","readme.md","img","guide.md"]`）连同背书注释一起写成了预期，把缺陷锁进了回归，现已改为前序 DFS 序列 `["docs","img","guide.md","readme.md"]` / depths `[0,1,1,0]`。② 右键菜单左键点别处不消失：菜单是行内自绘的（egui 0.33 无 `ContextMenu`），关闭此前只有「点树底部空白」一条路径——点别的行走 select / toggle 分支不关菜单，点树外（编辑区）更无人收尾，而原型语义是 `window click → closeMenu`。补齐两条：`run_file_intent` 在本帧无 menu 意图但发生行点击时关闭（树内一半），新增 `dismiss_menu_on_outside_click` 在落点不在文件树 `clip_rect` 内时关闭（树外一半）；切活动栏标签也一并关闭。③ 标签条打开文档超过窗口宽度而无法查看后方文档：038 的 `ScrollArea::horizontal` + `AlwaysHidden`（滚动条在视口内占位会吃掉行高，隐藏理由成立）留下的问题是**纯鼠标没有任何横向滚动手段**——滚轮不动横向、滚动条不画、标签带 `Sense::click()` 吞拖拽。补一对 18pt 向量三角形导航箭头贴在滚动区与「全部关闭」之间（`◀/▶` 字形不在字体覆盖内，与文件树箭头同法手绘），点击按一个标签宽步进（改 `ScrollAreaOutput.state.offset` 后 `store` 写回），到头到底置灰；活动标签**变化时**自动滚入视野（egui temp memory 记上一帧 active id，只在变化时才 `scroll_to_rect`——逐帧强拉会把用户手动滚走的视图拽回来）。回归：仓储层新增前序 DFS 双层 / 三层序列断言；侧栏新增真实指针事件驱动的「右键开菜单 → 左键点别的行 / 点编辑区均关闭」（附 `right_click_at` 助手）；标签条新增「溢出时两个箭头都在『全部关闭』左侧、点 ▶ 后 doc0 滚出视口而 doc1 可见」（从真实帧里按 y 区间筛 3 顶点闭合路径定位箭头）。问题记录 040。
- **用户反馈：标签条按钮重叠 + 多文档卡顿（10-16 用户报告，截图 ×1）**：① **按钮重叠**——「全部关闭」图标按钮与分隔线是手写绝对坐标（041 引入），x 少加了面板内边距基准 `strip_left`（042 给导航箭头补过，此处漏网）：按钮整体左移压进 ▶ 的热区、分隔线正好画进 ▶ 的矩形。headless 测试测不出：既有断言比较「箭头 vs 图标」，图标在按钮内缩 14pt 恰好躲过 8pt 偏差——新增 `the_close_all_divider_never_crosses_a_nav_arrow`（收集条带内全部竖直线段断言不进箭头矩形），并做**变异验证**（撤销修复该测试立即红）。② **多文档卡顿**——全链路勘察确认**没有磁盘 IO**（切标签不读盘、无文件 watcher、自动保存事件驱动、侧栏读内存快照），真正的每帧热路径是：标签条为 N 个打开文档各做一次 `rope.to_string()` 全文快照 + 全文行数扫描（唯一随 N 线性增长的每帧开销）；高亮缓存容量 8 < 打开上限 12 且驱逐用 `HashMap` 任意迭代序，开满标签后来回切换必互相驱逐，miss 一次即一次 syntect 全文重算（10KB 文档 56.5ms，卡顿尖峰来源）；状态栏每帧两次 `get_stats`（全文克隆 + 全文统计）与两三次 `get_document`；预览每帧为一个 u64 版本号克隆整篇文本。修复：新增轻量投影 **`TabSummary`**（id/title/path/is_dirty/byte_len，构造 O(1) 不做全文快照）+ `list_open_tabs` / `get_tab_summary` 查询，`EditorApp::open_document_list` 改返摘要（全部调用方只用这五个字段，编译期即验证）、状态栏脏标记/类型/面包屑一并切换；**`get_stats` 按 `(id, revision)` 单槽缓存**（文本没变帧帧命中，配 `stats_cache_hits` 计数器，编辑后版本变即重算）；**高亮缓存 8→32 + 真 LRU**（`order: Vec` 维护访问序：命中触碰、驱逐最旧，行为确定）；**`revision_of`** 让预览只取版本号。回归：`list_open_tabs_matches_list_open_fields_and_order`（摘要与完整视图逐字段对账 + 打开序一致）、`get_stats_is_cached_until_the_document_changes`、`highlight_cache_evicts_the_least_recently_used_entry`、`highlight_cache_holds_at_least_the_max_open_documents`。问题记录 043。

- **用户反馈：导航箭头 ▶ 失效（041 修完 ◀ 之后坏到另一边）**：根因不是动画（041 的初判只对了一半），而是三处叠加。① **「上一帧 active id」存错了地方**（主因）：判定「活动标签是否刚变化」用的是 egui temp memory（`data.insert_temp` / `get_temp`），而 temp 项在每帧 `begin_frame` 就被清掉、下一帧读回恒为 `None` → `prev_active != now_active` **恒成立** → reveal **每帧**触发。因 reveal 语义是「仅在标签不可见时滚动」，小幅位移不会被撤销，但一旦滚出活动标签就被立刻拽回——表现即「▶ 点了像没生效」，连点 ◀ 也永远到不了最左。② **外部滚动增量泄漏**：egui 的 `pass_state.scroll_delta` 是**全局**的、不分滚动区，任何视图调 `ui.scroll_to_rect` 都会写进去并由**下一个 begin 的 ScrollArea** 消费；标签条在顶栏总是最先执行，于是把编辑区 / 预览区的滚动请求整个抢走，offset 随机漂移（诊断实测 924 → 754 → 584 → **481.5** → **595** → **499**，既不按 170 步长也不单调）。③ **真实槽宽与估算不符**：横向布局的 `item_spacing.x`（默认 8pt）掺进每个标签占位，真实槽宽 176 而估算按 `TAB_MAX_W + TAB_GAP` = 170；且尾部 `ui.add_space` 高度为 0、不计入内容 Ui 的 `min_size()`（实测 content_size = 2016 = 12×168），导致滚到「底」时最后一个标签仍被切一截，用户据此以为右边还有。修复：滚动偏移**提升为应用状态** `AppUiState::tabstrip_scroll_x`（会话级、不落盘），上一帧 active id 迁到 `AppUiState::tabstrip_last_active`，**不再**用 egui temp memory；每帧末尾**无条件**钳制并写回 `State.offset`，外部泄漏当帧即被纠正，offset 从此只有一个写者；`horizontal_top` 内层把 `item_spacing.x` 归零并**按槽位分配**（`allocate_exact_size(vec2(TAB_MAX_W + TAB_GAP, ...))`，标签本体只占前 `TAB_MAX_W`），槽宽精确 = 170 且 `estimated_w` 与真实布局同源；滚轮仍可用——仅当指针悬停在标签条内才采信 egui 算出的偏移，否则视为泄漏丢弃。回归：新增 `nav_arrows_step_in_both_directions_and_stop_at_the_ends`（连点 ◀ 一路滚到最左使 doc0 可见、连点 ▶ 一路滚回最右使 doc11 可见、到头到底后继续点击不再变化）。问题记录 042。

- **用户反馈两项修复（10-16 用户报告，截图 ×1）**：① 「全部关闭」文案换成图标按钮——原文案连内边距吃 ~70pt，标签一多全在挤占滚动区；改为固定 28pt 的向量手绘图标（圆角方框「一枚标签」+ 内部 ×，与单标签右上角的裸 × 区分；项目纪律图标不走字形），语义走 `on_hover_text("全部关闭")` 补全，测试锚点从文本查找改为「条带内唯一描边矩形」。② 040 刚补的导航箭头 **◀ 点击失效**——根因不是点击判定，而是 egui 滚动动画覆盖手动 offset：040 的「活动标签自动滚入」用 `ui.scroll_to_rect` 实现，它把动画目标写进 `ScrollArea State.offset_target`（**私有字段**，外部清不掉），此后每帧插值器把 offset 拉向旧目标、到 1pt 内才清除；箭头在 show 之后手动写回的 offset 下一帧就被抹掉——自动滚入的目标恰在右边，◀ 的位移被逐帧拉回（「点击失效」），▶ 与旧目标同向反而「看着正常」（滚轮同理）。修复：`ScrollArea::animated(false)` 关死动画（滚轮与 scroll_to 类请求都立即生效，`offset_target` 永远不会被写入，offset 的唯一写者就是本函数）；滚入重写为不走 `scroll_to_rect`——闭包里把活动标签的内容坐标 rect 存进 `Cell`，show 之后按「内容原点 = inner_rect.min − offset」换算出目标 offset 直接写回（与箭头共用同一写回点，即时生效，也正是 VS Code 标签条的形态）。回归：新增「◀ 两步 + ▶ 两步互逆且步进可累积」「滚入之后 ◀ 仍生效」（显式 `RawInput.time` 驱动动画时间轴——测试帧墙钟间隔是微秒级、动画永远走不完，不给时间轴测不出旧缺陷）；nav 测试按新语义重写（滚入即时生效，打开序最后一个 doc11 是活动标签，首帧即定位右端；旧测试「初始停在最左」的假设被微秒帧间隔掩盖，实为动画未及发作）。问题记录 041。
- **用户反馈三项修复（10-15 用户报告，截图 ×2）**：① 右下角主题切换不对称——原 `cycle_theme` 走三态轮换 `CYCLE = ["dark","light","system"]`，深色→浅色一步、浅色→深色却经「跟随系统」中转要两步，用户期望「点一下直接换深浅」。改为按当前生效明暗取反直接互换（目标 kind 取反、`set_theme` 持久化后立即刷新），system / custom 仍经设置页下拉可达；状态栏 `theme_label`、快捷键 `ToggleTheme` 文案同步改为二态（深↔浅）。② 导出浮窗顶部标签文案不可见，且「切主题后又正常了」——双根因：展示层 `format_row` 选中态文字取 `theme.accent`（与 037 同源，用户 `selection_bg` 可为实色 accent 蓝导致同色不可见），抽 `selected_format_label_color` 改返回 `theme.text_primary` 并加单测；数据层 `to_theme` 仅在 `kind == Custom` 时附调色板，custom 切到 light/dark 时配色被静默丢弃写盘（恰解释「切主题后又正常」：丢调色板后回退默认 22% alpha selection_bg 文字复现）——改为 match 双分支，非 custom 若 `view.custom.is_some()` 仍保留配色堵数据丢失，附「写入后回读配色仍在」回归。③ 浅色模式侧栏「大纲」标签下黑色线条——相邻 SidePanel 亚像素缝隙露出交换链黑清屏色，深色下缝隙同为黑浑然一体故只在浅色现身（与 025/033/037 同源家族，main_view 既有边界已修但 sidebar/editor 这条未修）；在 `main_view::show()` 起始铺最底层 `bg_secondary` 防御底色（整块可用区域），缝隙露出的不再是黑清屏色。回归：三态轮换测试删旧换新（dark↔light 互换）、to_theme 配色保留、status_bar 二态断言、sidebar_boundary（由 dbg_line_test 改名）无头渲染断言边界无贯穿黑缝。问题记录 039。

- **V11-16 编辑区回车换行**：连续 N 次回车只显示一个空行的根因是**渲染层**——自定义逐行 layouter 对空行产出零高度 Galley（epaint 空段落高度仅在非空 section 时置位），与 IME / 回车事件无关（运行时日志取证推翻前三轮的误判）；修复为空行补空 section，行号栏与正文恢复对齐。
- **V11-17 ~ V11-22 用户反馈六项**：超链接可点击（三平台零依赖打开器 + `javascript:`/`data:` 等 scheme 大小写不敏感拦截）；注脚跳转补契约回归；Typora `#pic_*` 对齐后缀剥离 + 远程图失败「浏览器打开」出口；大纲行号 1 基、点击不被同步开关吞、缓存未就绪时近似跳转；设置页字体选择（项目 `fonts/` + 系统等宽发现，选中置顶进等宽族）；内置三款西文等宽字体 + Noto Sans SC GB2312 子集 CJK 兜底 + DejaVuMathTeXGyre 数学兜底（「西文 → 数学 → 中文」链），67 个数学符号不再显示方框。
- **V11-23 甘特图重写**：对齐 mermaid.js——真实日期（零依赖 Hinnant 算法）转天序号、标题居中、任务名条内白字、四态配色、底部日期轴 + 自动刻度 + 竖向网格。
- **V11-24 检查标签黑框**：lint 行水平裸 label 不换行溢出，egui SidePanel 按内容 rect 保留布局把编辑区推右、溢出区露出清屏黑；修复为 truncate（搜索分组头 / 命中片段同类隐患一并修）。
- **V11-25 https 图片下载**：引入 `ureq` + `rustls` 取代手写 `TcpStream`，https 图床图片直接渲染（原方案 https 硬编码拒绝）。
- **V11-26 依赖复查**：eframe 移除未使用的 `accesskit` 特性、`ahash` 收窄去 `getrandom`——release 11.04 → 10.93 MiB（−110 KiB），跨平台锁文件 −60 包；仍超 `NFR-PERF-01` 红线 0.93 MiB（待评审，见 `docs/metrics.md` §2.4）。
- **V11-14 / V11-15 收尾**：2026-10-10 登记的 7 条红（4 工具栏期望滞后 + 2 golden + 1 PDF 环境）全部修复，复测 `cargo test --all-targets` **1623 passed / 0 failed**。

### 2026-10-10 · v1.1 P2 四项（V11-06 ~ V11-09）

- **V11-06 侧栏删除二次确认**：删除文件 / 目录前弹确认（`DialogRequest::Confirm`
  复用既有异步对话框通路，不阻塞帧循环）；确认对话框**不可用时提示且不删**。
  目录删除由「仅空目录」放宽为**递归删除**，其确认文案会报出目录内条目数。
- **V11-07 拼写检查增强**：内置词表扩充（补高频英文词与英式拼写变体）；
  领域 `SpellChecker` 新增 `suggest`（编辑距离 ≤2，按距离优先、字典序次之稳定排序）；
  编辑区在带波浪线的错词上**右键**可看到「你可能想输入」候选并一键替换。
- **V11-08 Markdown 格式检查（lint）**：新增领域规则集（标题层级跳跃 / 列表符号混用 /
  行尾多余空格 / 未闭合代码围栏，**跳过围栏代码块内部**）；规则可在
  「设置 → 语法检查」逐条开关（偏好新增 `LintSettings`，旧配置默认全开）；
  侧栏新增「检查」标签列出问题，**点击问题跳转**到对应行。
- **V11-09 自定义主题细粒度配色**：自定义主题在 8 项核心语义色之外，新增
  10 项语法高亮色（标题 / 加粗 / 斜体 / 代码 / 链接 / 引用 / 标记符 / 元信息 / 表格 / 其它）
  可**按项覆盖**（`None` = 跟随主题，旧主题文件自动向后兼容）；修复浅色自定义主题
  沿用深色语法高亮色的问题；配色签名纳入全部语法色，确保改色即时刷新预览。

> **测试状态**：上述四项新增用例全部通过；全量 `cargo test --all-targets` 为
> **1519 passed / 7 failed**，7 项失败均为既有红（V11-14 ×6 测试期望滞后 + V11-15 ×1
> 无头浏览器环境限制），本批**未引入新红、也未触碰既有红**。

### 2026-10-10 · 用户反馈修复批次（UI / 编辑器 / 预览 / 导出）

> 原始问题记录：`docs/logs/问题记录1.txt`（编辑区不可见 / 菜单层级 / 下拉重叠 / 多标签预览）。

- **编辑器内公式与图表渲染**：新增自研 LaTeX 子集排版引擎（`presentation/views/math.rs`）
  与 Mermaid `graph`/`flowchart` 分层布局引擎（`presentation/views/mermaid.rs`），
  预览区**直接渲染**；子集外写法明确回退源码卡，零新增依赖（不破体积预算）。
- **PDF 导出改走无头 Chromium**（ADR-004 修订）：Windows 由 `powershell printto`
  改为无头 Edge/Chrome `--print-to-pdf`，可静默落盘真实 PDF；`printto` 降为兜底，
  `PdfBackend::writes_pdf` 向界面暴露差别；修复「纸张选项从未注入 `@page`」与「大小恒报 0」。
- **预览区四批修复**：`053fbc6` 滚动 / 行内样式 / 公式图表 / 列表层级 / 脚注表格；
  `4838d22` 图表出图、链接可点击、双向滚动、自动换行、工具栏格式平铺；
  `46e0e2d` 目录 / 键帽 / 高亮 / 上下标、编辑区统一行高；`399d14f` 表格列宽自适应 +
  统一间距刻度 `space_*`。
- **交互与状态**：浮层不再每帧抢焦点（新增 `dialog_ui_state.rs`）；行号栏默认隐藏并接入
  设置开关；回车退出列表、去除每键整篇回读、程序性滚动不再驱动预览。
- 顶部菜单分层重构：下拉归应用级、格式动作归编辑区顶部工具栏。

> **测试状态**：上述批次改动了工具栏分层与解析器结构但未同步测试期望，
> `cargo test --all-targets` 当前为 **1195 passed / 7 failed**（5 条期望滞后 + 1 条 golden
> 快照 + 1 条无头浏览器环境限制），已立为 **V11-14 / V11-15**（见 `docs/v1.1-backlog.md`）。

### 阶段 09 · 测试体系、打包与发布 [S9-*]

- 测试金字塔目标显式化：`[[test]]` 声明全部分层测试目标（S9-01）。
- 覆盖率门禁：整体 ≥70%、领域 ≥90%，排除项书面化（S9-02）。
- 崩溃日志：panic hook 写盘 `crash/`，无遥测，保留最近 10 份（S9-08）。
- 三平台打包脚本（Windows zip / macOS .app+dmg / Linux AppImage+tar.gz）
  与 release workflow（tag 驱动、SHA256、Notes 归组）（S9-04/S9-06）。
- 未签名发布路径：证书成本列为可选，SmartScreen / Gatekeeper 绕过提示写入
  README 与 release 文档，私钥排除规则核对（S9-05）。
- 发布流程演练工具化：`tools/gen_release_notes.py`（Notes 改为**按阶段聚合**，
  组名标注编号区间）、`tools/release_dryrun.py`（离线演练 13/13，红路径 exit 1），
  并接入 CI `release-dryrun` job（S9-06）。
- 用户向 README、双许可（MIT OR Apache-2.0）、NOTICE、故障排查文档（S9-07）。
- 属性测试：Rope / 撤销栈 / 行列往返 3 组 × 256 例（固定种子，零新依赖）（S9-03）。

### v1.1 · P1 收官（本轮「先排除阶段 9 任务」口径）

- **V11-12 规范 15 机械门禁**：`lib.rs` 开启 `clippy::unwrap_used` / `expect_used` / `panic`
  （warn + CI `-D warnings`），测试豁免 `#[cfg_attr(test, allow(...))]`；逐处裁决 14 处非测试
  `unwrap`/`expect`，全部改写为非 panic 形态（锁损坏 `PoisonError::into_inner`、选项 `?`/`and_then`/
  `let…else`），零 `#[allow]` 残留，红路径演练通过。
- **V11-03 状态栏编码接线（DEF-05）**：共享内核纯展示型 `TextEncoding`（不污染领域层），
  基础设施探测的编码经 `DocumentSnapshot::encoding` 上报、应用层旁路表暴露、状态栏每帧同步；
  未落盘文档显示 `—` 而非谎称 UTF-8。
- **V11-04 PDF 导出开放（方案 B）**：开放 PDF 入口，明确提示「经系统打印对话框产出，请选择『另存为 PDF』」；
  打印副作用抽为可注入 `PrintDispatcher`，CI 无需真弹打印框（`bytes` 恒为 0 属预期）。
- **V11-05 写作目标进度条**：领域新增 `WritingGoal(u32)` 值对象（饱和上限 100 万、百分比截断 100、达成判定），
  `EditorSettings.writing_goal` 经 `#[serde(default)]` 兼容旧配置；状态栏显示百分比 + 细进度条，
  达成时提示（按「向上跨越」去重，切文档 / 改目标重置）。种子配置含 `writing_goal = 1000` 示例。

## [0.1.0] · 阶段 01–08（开发期）

### 功能（对齐需求 §3.1）

- 文档编辑：打开/保存/另存为、多标签、自动保存与草稿恢复、撤销/重做（合并窗口）。
- 编码：UTF-8 / UTF-8 BOM / UTF-16 LE·BE / GBK / GB18030 无损读写（含四字节增补汉字）。
- Markdown 预览：语法高亮、目录、图片（本地/远程/data URI）、公式与 Mermaid 结构化卡片。
- 查找替换：全文字数有界、正则、大小写、整词、替换预览。
- 导出：HTML（TOC/内联图片/KaTeX/Mermaid 资源注入）、Markdown 源码、图片（视口截图 / 整篇长图）。
  PDF **未开放**：基础设施（系统打印管线）已就绪，但产物需经系统打印对话框中转，界面未提供入口（v1.1 决策）。
- 工作区：文件树、大纲、全局搜索、最近文件。
- 拼写检查：内置 3779 词表 + 近邻门槛 + 波浪线标记（仅英文，演示级）。
- 设置：主题（含自定义 8 语义色）、快捷键自定义、编辑器参数、TOML 持久化（损坏自动备份回退）。
- 拖拽打开：文件与文件夹混拖，目录安全忽略。

### 质量

- 1281 自动化测试（分层金字塔），fmt/clippy/check-arch/测试四道机械门禁（覆盖率
  整体 85.27% / 领域 91.69%）。
- release 产物 ≤10MB 红线门禁（CI 强制）与性能冒烟基准。
- 架构分层（DDD 四层）由 `check-arch` 在 build.rs 构建期强制。

[未发布]: https://example.invalid/rustmark/compare/v0.2.0...HEAD
[0.2.0]: https://example.invalid/rustmark/releases/tag/v0.2.0
[0.1.0]: https://example.invalid/rustmark/releases/tag/v0.1.0
