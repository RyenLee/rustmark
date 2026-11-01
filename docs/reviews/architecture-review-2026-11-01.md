# rustmark 全维度架构评审（2026-11-01）

> 评审人：顾图南（架构师）· 基准 HEAD `a1cd390`（2026-11-01）· 工作树未跟踪 `.trae/`、`deliverables/` 与本评审无关。
> 性质：**只读评审，未改动任何生产代码**。所有结论以读实际代码 + 实测命令为准；文档与代码不一致处以代码为准并在正文指出。
> 输入：`docs/diagrams/project-relationship-map.md`（2026-10-30 快照）、`Cargo.toml` / `crates/*/Cargo.toml`、`tools/arch_check.rs`、`docs/spec/architecture-constraints.md`、`composition.rs`、`rustmark-assembly`、`rustmark-ports`、`rustmark-infra/src/infrastructure/`、`src/app/frame/`、`src/presentation/views/`、CI/门禁脚本。
> 规模：10 crate / 16 条本地依赖边（无环）/ 511 个 `.rs`。

---

## 0. 评审基准与实测口径

- **crate 拓扑**：root 依赖 {core, infra, icons, ui, assembly}；assembly 依赖 {core, infra, ports}；core 依赖 {actions, shared, ports}；infra 依赖 {core, ports}；ui 依赖 {core, render}；ports 依赖 {shared}。合计 **16 条本地边，无环**。`rustmark-actions` 唯一零依赖叶；root 是唯一叶子消费者，无 crate 反向依赖 root。**结构属实**。
- **M2 已落地**：`crates/rustmark-assembly/` 存在，`crates/rustmark-assembly/src/bootstrap.rs`（343 行）承载 `production_bundle` / `Services` / `ConfigPaths` / 5 个 `assemble_with_*`；root `src/app/bootstrap.rs` 为 `pub use rustmark_assembly::bootstrap::*` 薄门面。rock 依赖面**未缩减**（仍 5 个本地依赖），与 `docs/remediation/r10/m2-design.md` 的 YAGNI 判定一致。
- **实测口径声明（文档漂移，正文逐条展开）**：
  1. `docs/diagrams/project-relationship-map.md` §7 称"17 个 `XxxViewAccess`"——实测 17 个 access trait 中 **16 个**以 `ViewAccess` 结尾，第 17 个是 `ScrollSyncAccess`（`view_context.rs:1070`），命名口径略宽。
  2. `docs/spec/metrics.md` §1.1 称"字段守卫现扫 **5 个** src 根（core / icons / shared / infra / ui）"——实测 `tests/field_visibility_guard_test.rs:869` 为 **7 根**（另含 render / ports）。metrics 该行已陈旧。
  3. 关联图 §3 称"`app/frame/*（24 文件）` / 25 个 frame 子文件"——实测 `src/app/frame/` 下 **26 个 `.rs`**（25 顶层 + `editor/` 子目录）。数量级无碍，数字待刷新。

---

## 1. 可扩展性

### ✅ 合理之处
- **"新增即编译失败"防线齐全**（这是本项目最突出的优点）：
  - `ImplBundle`（13 字段）/ `AppServices`（13 字段）+ `Services`（14 字段）三处用**穷尽解构**互锁——`assembly/bootstrap.rs:310-324` 对 `AppServices` 逐字段解构，core 新增服务字段时 assembly 立即编译失败，强迫作者决定是否在 `Services` 暴露。这是"漏接"最易腐化方向的编译期阻断，比测试更早。
  - 新增可注入依赖只需"`ImplBundle` 加字段 + `production_bundle` 填一行"，5 个 `assemble_with_*` 互不级联（各自覆盖自己关心的字段），测试调用点零改动（`assembly/bootstrap.rs:284-298`）。
- **落点清晰**：关联图 §9「改动该往哪落」把新增 UI 命令 / 视图 / UI 状态 / 用例 / 适配 / 帧调度逐条映射到具体目录；实测该映射与 `arch_check.rs` 的分层守卫、`frame/` 按域拆分（26 文件）一致。
- **命令三处同源**：菜单 / 快捷键 / 命令面板统一走 `UiAction` → `EditorApp::execute`（关联图 §6 不变式 1），新增一条 UI 命令只改 `crates/rustmark-actions/src/action/` + `frame/actions.rs` 两处，不散落。
- **视图越权编译期不可达**：视图只收 `&mut dyn XxxViewAccess`（17 个 access trait，`rustmark-ui::presentation::view_context.rs`），实现集中在 `frame/view_context.rs`（796 行）；`EditorViewContext` 宽上下文只允许两个路由者（`main_view.rs` / `overlays.rs`）。新增视图的签名形状被三道守卫钉死（见 §5）。

### ⚠️ 风险与不足
- **`Services` 的 14 个 `pub` 字段对 UI 是"宽门"**：视图虽收窄 trait，但 `frame/` 侧仍可直接读 `services.<field>`（任意字段）。真正的能力裁剪在 view_context 层完成，`Services` 本身无收窄——新增字段默认对所有 frame 子模块可见。这是设计取舍（组合根持有全部能力），但意味着"字段可见性"只靠 `field_visibility_guard` 的跨 crate 白名单守，而该白名单**不含 assembly**（见 §5/§6 缺口）。
- **`field_visibility_guard` 的扫描根是硬编码 `[(&str, PathBuf); 7]`**（`tests/field_visibility_guard_test.rs:869`）：新增一个含 `pub` 字段的 crate 不会自动纳入射程，需手改数组。M2 新增 assembly 含 16 个 `pub` 字段，即**静默落在射程外**——"新增即失败"机制在"新增 crate"这一维度上有缺口。

### 🔧 改进建议
- **P2**｜把 assembly 纳入字段守卫：`tests/field_visibility_guard_test.rs:869` 的 `roots` 数组加 `("assembly/", base.join("crates").join("rustmark-assembly").join("src"))`，并在 `APPROVED_PUB_FIELDS` 登记 `assembly/bootstrap.rs::Services::*`（14）与 `ConfigPaths::*`（2），`// total:` 594 → 610。**收益**：组合根 crate 的公开字段面与 core 同级受集合相等断言保护。**改动面**：1 个测试文件。
- **P2**｜把 `roots` 数组从"硬编码长度 7"改为按 `crates/*/src` 目录扫描（或至少加一条断言"crates 下的成员目录 ⊆ roots"），让"新增 crate 忘登记"变成红而非静默漏守。**改动面**：`field_visibility_guard_test.rs` 局部。

---

## 2. 性能

### ✅ 合理之处
- **帧调度统一收口**：`frame/scheduler.rs` 是**唯一**真实调用 `request_repaint_after` 的地方（实测 `src/` 内 4 个文件出现该字符串，仅 `scheduler.rs:288` 是代码调用，其余 `files.rs` / `loop.rs` / `state.rs` 全是注释——守卫用词法器跳过注释，判定成立）。`begin`/`flush` 成对契约有 debug 断言（`scheduler.rs:264-267`），过期到期点钳到 1ms 防忙循环，`arm` 同类取更早者。这解决了"每新增定时功能就漏预约"的整类问题。
- **三档优化已落地并有实证**：P1 停手补算后台化（2026-10-29，体积 10,589,696 B）／P2 增量高亮（`infra/../highlighting/incremental.rs` 988 行）／P3 高亮预热（`app.rs` 的 `prewarm_highlighting`）。`NFR-PERF-05` 重定义为"交互帧"后 p95 ≈ 0.00–0.01ms。
- **体积预算可控且有手段**：wgpu `.text` 838.7 KiB 是最大第三方组（ADR-001 已接受）；图片解码链 294.5 KiB；syntect 分级裁剪（20→10→5）**未触发**（实测 17.1 KiB）。TLS 链 feature 化可整条剔除 −1.02 MiB（`5d0512d`）；Linux 只留 vulkan 后端收体积（`8e70c3d`）。
- **远程图片有连接复用与流式上限**：`agent.rs` 进程级共享 `ureq::Agent`（`OnceLock`，连接池 + TLS 会话复用），`asset_loader.rs:376-399` 边读边判 8 MiB 上限，避免恶意/超大响应先全量进内存。
- **帧耗时可观测**：`scheduler.rs` 的 `FRAME_BUDGET=16ms` 超预算分阶段归因 + 限流（120 帧/条）+ 常态指标（600 帧/条），仅 release 输出（debug 帧耗时不可比）——排障从"用户说卡了一下"变成可归因日志。

### ⚠️ 风险与不足
- **1MB 档 p95 贴线未稳定达标**（16.5ms > 16ms 一帧）。已按 V11-41 口径 A 定为"尽力而为、不硬达标"（守线范围限 ≤100KB，已达标余量 ≥9×）。这是**已知取舍**而非回归；1MB 排版层每帧 `O(全文+行数)` 拼装是量级瓶颈（原地补丁已把 4.57→0.241ms），进一步需绕过 `TextEdit` galley 契约，属 T5 决策。
- **Linux vulkan-only 的启动可用性风险（待验证）**：`Cargo.toml:324-328` 删除 gles fallback、只留 vulkan，理由是双开导致 Linux release 超 11 MiB。代价是**无 vulkan 驱动的机器（老 Intel 核显、部分虚拟机/云桌面）会 `wgpu::Instance::new` panic 启动失败**。是否为问题取决于目标用户 GPU 基线——**待验证**：用一台无 `mesa-vulkan-drivers` 的 Linux 或旧核显机器实跑 release；若覆盖到此类环境，建议保留 gles 作为运行时回退（而非编译期双开）。
- **远程图片并发下载的上限（待验证）**：预览区含 N 张远程图时是否限并发、是否阻塞 UI 线程，需看 `preview_image_fetcher` / 预览取图的调度；本次未展开确认。**验证方法**：一篇含 20+ 张远程图的文档，观察下载是否串行阻塞绘制帧、有无并发上限与超时聚合。

### 🔧 改进建议
- **P1（待验证）**｜Linux 启动回退：先按上法验证老 GPU 覆盖情况；若需覆盖，评估"运行时探测 vulkan 不可用 → 回退 gles"（需把 gles 重新编入，体积回到分档红线内需重新核算）。**改动面**：`Cargo.toml` 目标段 + 启动路径探测。
- **P2**｜把"远程图片并发/超时聚合"策略显式化并记录（当前散落在取图路径），避免大量远程图时整帧阻塞。**改动面**：`frame/` 预览取图路径 + 文档。
- **P2**｜1MB 档维持"尽力而为"记录在 README 已知限制（已在 metrics §1 声明），无需动作；若要攻，单独立项，不混入日常迭代。

---

## 3. 可靠性

### ✅ 合理之处
- **错误分层清晰**：`anyhow` 只在组合根入口（root `Cargo.toml:284` + `app.rs`/`main.rs`），领域/应用/适配器一律 `thiserror`（`deny.toml` 与 lint 均体现）。装配阶段唯一可失败点是偏好读盘（`composition.rs:176`），且**配置损坏不阻断启动**（ADR-003 备份 + 回退默认，`composition.rs:172-176` 注释）。
- **落盘原子性**：`fs_document_repository.rs:178-227` 同目录临时文件 → `write_all` → `flush` → `sync_all` → `rename`，任一步失败清理临时文件且**原文件不被触碰**（符合 FR-04"不破坏原文件"）；`toml_preferences_repository` / `recent_files_store` 同用 `atomic_write`。
- **草稿与恢复**：草稿不覆盖原文件、单独 `.rustmark-draft/`；`workspace.restore_recent()` 失败**不阻断启动**（`composition.rs:233-235`）。
- **崩溃恢复/诊断**：`crash/` 目录集中崩溃报告（用户手动提交），`ConfigPaths::crash_dir()`。
- **后台 worker 失败路径**：帧耗时观测、图片下载失败均降级为占位/日志，不 panic（`panic="abort"` 下尤为关键）。
- **flaky 治理有实证**：`392b447` 把击键帧门禁改双判据（p50 守线、p95 守 2 倍数量级）；`scheduler.rs` 单测用"下界 ≥1ms、上界 <5s"避免机器快慢抖动；增量不真等 16ms 而注入耗时。
- **调度契约**：`begin`/`flush` 成对 debug 断言 + 消费复位（`scheduler.rs:264-271`），漏调 `begin` 的静默失效被当场炸出。

### ⚠️ 风险与不足
- **`atomic_write` 未 fsync 父目录（durability 窄窗口）**：`fs_document_repository.rs:219-225` 只 `sync_all` 了文件内容，rename 后**未 `File::open(parent).sync_all()`**。POSIX 下 rename 的目录项持久性不保证：极端断电/内核崩溃后可能"内容已同步但目录项回退"，表现为**上一次保存丢失（回退到旧内容）**，但不会产生半截/损坏内容（内容已 sync）。单机 Markdown 编辑器下这是小概率，且与"不破坏原文件"契约不冲突（退回的是旧文件，不是坏文件）。
- **临时文件名与注释不一致（并发写同文件冲突）**：`fs_document_repository.rs:188-194` 注释称"临时文件名带**进程 ID 与计数器**语义：避免与并发实例互相覆盖"，但实际 `format!(".rustmark-tmp-{file_name}")` **既无 PID 也无计数器**。两个实例同时保存同一文件会共用同一 tmp 路径 → 互相 `truncate`/`rename`，可能一方的改动被另一方覆盖或 rename 拿到半写内容。概率低（单用户本地编辑器，双开同一文件罕见），但**注释声明的防护并不存在**，属"承诺 vs 实现"漂移。
- **跨进程/多实例编辑同一文件无协调（设计边界）**：纯 FS、无文件锁、无 watch。两实例并发编辑同一文档 = 最后写入者胜、静默丢改动。这是"无数据库/无协作"的固有边界，非缺陷，但**未在 README/已知限制显式声明**，用户可能踩。

### 🔧 改进建议
- **P2**｜补父目录 fsync：`atomic_write` 在 `fs::rename` 成功后，Unix 下 `File::open(parent)?.sync_all()`（Windows 的 rename 持久性语义不同，可 gate 掉）。**收益**：rename 目录项持久性；**改动面**：`fs_document_repository.rs` 一处 + 单测（模拟失败不易，可只验证调用不 panic）。
- **P2**｜修临时文件名与注释对齐：改为 `.rustmark-tmp-{pid}-{counter}-{file_name}`（PID + 原子计数器），或删掉注释里的"进程 ID 与计数器"表述使之与实现一致。**建议取前者**（真正消除并发实例覆盖风险）。**改动面**：`fs_document_repository.rs:188-194`。
- **P2**｜在 README 已知限制登记"不支持多实例并发编辑同一文件；同时编辑会最后写入者胜"，把边界显式化。

---

## 4. 安全性

### ✅ 合理之处
- **F-01~F-04（`d86e17f`）逐项到位**：
  - **F-01（隐私）**：PDF 导出临时 `*.print.html` 含正文 HTML，经 RAII 守卫 `TempPrintSourceGuard` 在 `export()` 返回（含 `?` 提前返回）时统一删除，修复长期违反 `PRIVACY.md §3` 的残留。
  - **F-02（本地越界）**：`asset_loader.rs:218-327` 有文档上下文时**收束进文档目录**、拒绝对路径与 `../` 越界；无上下文（受信选图）保留绝对路径。`resolve_within_base` 按文本分量归一，拦住路径穿越。
  - **F-03（SSRF）**：`agent.rs:57-61` `max_redirects(0)`，禁止盲跟随 3xx 打内网/回环/链路本地。
  - **F-04（DoS）**：数学解析单一收敛点 `parse_row` 加递归预算（≤256 层），超限回退源码卡而非栈溢出触发 `panic=abort`。
- **MIME / data URI 白名单精确到子类型**：`asset_loader.rs:176` 只放行 `png/jpeg/jpg/gif/webp/bmp`，**排除 `svg+xml`**（`<svg><script>` 向量）；`data:text/html` 被拒。真源在 `infrastructure::image_policy`，与导出消毒 `html_sanitize::is_safe_url` **共用同一张表**（`asset_loader.rs:15-17`），避免"预览放行 / 导出被剥"不一致。
- **导出 HTML 消毒**：`html_sanitize.rs` 手写词法器 + 栈式主循环，三类处理（DROP 连子树丢 / ALLOW 属性过滤 / 未知标签拆壳后**子节点重新清洗**），`<svg><script>` 丢弃、`<foo><img src=x onerror=…>` 拆壳后 `onerror` 仍被剥（有 doctest 钉住）。`on*` 事件属性无处逃逸。
- **体积/资源硬约束**：data URI 512 KiB、本地/远程 8 MiB，三条来源统一在 `load()` 末尾判（避免某分支忘检）；远程边读边判上限；坏图解码不 panic（`panic=abort` 下关键）。
- **供应链**：`deny.toml` 覆盖 advisories（唯一条 bincode 1.3.3 有理由豁免）/ licenses（白名单 + self_cell Apache 分支 clarify，不触发 GPL）/ bans（`wildcards=deny` + `allow-wildcard-paths` 兼容内部 path 依赖）/ sources（仅 crates.io）。配置成熟。
- **TLS 用平台信任库**：`agent.rs` 显式注入 `platform_root_certs()`，不内置 Mozilla 根包（避免根证书陈旧 + 体积）。ureq 纯 Rust TLS，三平台行为一致、不链系统 OpenSSL。

### ⚠️ 风险与不足
- **符号链接越界（已知未覆盖）**：`resolve_within_base` 注释明确"仅做文本分量归一（不解析符号链接）"，`doc_dir` 内的符号链接指向目录外仍可被读。对"下载的不可信 `.md`"这一威胁模型，攻击者若能控制文档目录内容（如解压一个含符号链接的压缩包到文档目录），可借符号链接读任意文件。**严重度取决于文档目录是否可能被不可信内容填充**——待验证：若"打开他人给的 md 时其同目录 `images/` 可能含符号链接"，则该面成立。
- **远程图片无 MIME magic 校验、仅信响应头/扩展名**：`asset_loader.rs:361-374` 优先响应头 MIME，缺失按最终 URL 扩展名；均命中白名单才放行。但**不校验字节头 magic**——响应头可伪造 `image/png` 而正文是别的。危害有限（下游 `image` 解码失败会降级占位，不执行内容），但严格可加 magic 校验。
- **PDF/打印源内容仍是明文临时文件**：F-01 修的是"用后即删"，但若进程在导出中途被强杀（非 `?` 返回路径），RAII 来得及吗？RAII 在 normal return / panic-unwind 生效，`panic=abort` 下 panic 不 unwind——强杀/abort 时守卫不执行，临时文件可能残留。**待验证**：`panic=abort` 与 RAII 的交互（正常 `?` 返回删得掉；硬 abort 删不掉，属固有限制）。

### 🔧 改进建议
- **P2**｜符号链接防护：在 `resolve_within_base` 后用 `std::fs::canonicalize` 校验最终真实路径仍在 `canonicalize(doc_dir)` 子树内（注意 canonicalize 对不存在路径报错的取舍）；或至少在文档里把该面登记为"文档目录需可信"。**改动面**：`asset_loader.rs` 一处 + 测试。
- **P2（低成本）**｜远程图片加 magic 头校验（前 8 字节对白名单位图签名），与 MIME 白名单双重把关。**改动面**：`asset_loader/policy.rs` + 测试。
- **P2**｜把"硬 abort 时 PDF 临时文件可能残留"记入 `PRIVACY.md`（属 OS 级强杀的固有边界，无需代码改）。

---

## 5. 可维护性

### ✅ 合理之处
- **spec 即代码契约，且有机械化对账**：`tools/arch_check.rs`（词法级）在 `build.rs` / `check-arch` bin / fixture 三处 `include!`，同一份源码；`arch_violation_fixture` 120 例（零违规 + 注入必报）双向钉住校验器自身。文档与代码的漂移会被门禁或 CI 拓扑 diff 抓。
- **三道视图守卫是词法级的，不是字面 `contains`**：`check_view_decoupling`（禁 `crate::app` 路径）、`check_view_no_composition_root_marker`（禁 `EditorApp` 标识符，堵 `use super::*` 带进来的隐式子模块）、`check_view_narrow_context`（非路由视图禁 `EditorViewContext`）。三者都走 `Lexer`，注释里讨论"为什么不用 EditorApp"（实测 20 处）不会误报——这是"规则可写文档"的关键。
- **门禁可维护性有自觉**：`scan_project` 的"新增 crate 时只改 `src_roots` 一处"设计（`arch_check.rs:2114-2118`），`check_rich_egui_isolation_with` 与 `scan_project` 共用同一份根清单，避免"新根进了一处漏了另一处"。
- **规模控制有节**：`frame/` 按域拆 26 文件；views 55 文件（含各视图 `tests.rs`）；超 400 行的生产文件多担得起（见下）。
- **可复现**：`scripts/gen_dep_graph.sh` 保留注释头只重写机器主体，`cargo tree` 与 `expected.txt` 幂等 diff；`check_thresholds.py` 纯标准库 + 锚点 + `via` 别名解析。

### ⚠️ 风险与不足
- **`arch_check.rs` 2976 行巨型校验器**：单一文件承载层禁令 + 视图三守卫 + 帧预约 + rich-egui 隔离 + UI 子模块对账 + 视图文件规模 + 视图解耦。作为工具可接受，但"改分层规则要先在 3000 行里定位"是长期成本；且它同时被 `include!` 进 build.rs，任何编译期错误会影响整个构建。
- **超 400 行文件（生产源码）**：`ui/view_context.rs` 1227（17 个 trait + 大量 supertrait 声明，**合理**）、`infra/highlighting/incremental.rs` 988、`frame/view_context.rs` 796（17 trait 的 impl，**职责内聚**）、`ui/theme/mapper/highlight_layout.rs` 773、`frame/editor.rs` 727、`frame/scheduler.rs` 757（约半为内联测试）。其中 `incremental.rs`（988）与 `highlight_layout.rs`（773）**待验证是否单一职责**——增量高亮与布局映射都是算法密集模块，规模可能正当，也可能可拆。
- **文档漂移多处**（本报告 §0 已列 3 处）：metrics 说 5 个扫描根实为 7、关联图 17 个 `XxxViewAccess` 实为 16+1、frame 文件数 24/25 实为 26。漂移不致命，但"spec 是契约"的项目里，活文档数字应随版本更新（CI 未对这类散文数字做机械校验）。

### 🔧 改进建议
- **P2**｜把 `arch_check.rs` 按守卫职责拆 `mod`（层禁令 / 视图守卫 / 帧预约 / 规模阈值 / UI 对账），facade 只 `pub use`——`include!` 的仍是同一份 `main`；**收益**：改某类规则不必在 3000 行里找。**改动面**：工具文件内部重构（不影响生产代码，但需 fixture 全绿兜底）。
- **P2（观察）**｜`incremental.rs` / `highlight_layout.rs` 若后续继续膨胀，按既有 `stats/` `find/` 的拆法（facade + 子模块）处理；当前不急。
- **P2**｜活文档数字（crate 数 / 边数 / 扫描根数 / trait 数 / frame 文件数）尽量改为"从命令生成"或加一条 CI 轻校验（如比较 `crates/*/src` 目录数 vs metrics 宣称），减少手维护漂移。

---

## 6. 模块耦合度

### ✅ 合理之处
- **16 条边逐条可解释、无环**，且方向被 cargo 锁死。红线 `core → infra` 不存在且被 `workspace_topology_test` 钉死（core 依赖面恰为 {actions, shared, ports}）。
- **DI 边界有效**：`composition.rs` 只 `use crate::{ports,domain,application,shared}`，**绝不 `use rustmark_infra`**（模块文档 + `core_crate_dependency_face` 双向断言）；具体 impl 由 assembly 注入 `Arc<dyn …>`。这是六边形"契约归 core、实现归 infra"的干净落法。
- **egui 边界清晰**：core 零 egui（V11-63）；infra 零 egui；render 默认 egui-free，唯一后端 `render/egui.rs` 走 `feature="egui"` 双门控；icons/ui 是合法 egui 消费者。egui 只经 root 与表现层进入。
- **`frames`/`views` 的同源不变式**：命令三处同源、帧预约唯一出口、视图越权编译期不可达——三条都落在结构/守卫上，不靠人记。
- **re-export 桥接是"公开路径平移"而非"隐性耦合"**：`rustmark::app::bootstrap` / `crate::infrastructure` / `crate::presentation::icons|widgets` 都是单向 `pub use` 拼回原路径，消费方零改动；桥接本身不引入反向依赖。

### ⚠️ 风险与不足
- **M2 的 `assembly → ports` 是"声明边、无代码使用"**：`crates/rustmark-assembly/src/bootstrap.rs` 全程用 `rustmark_core::ports::time::{Clock, SystemClock}` 取端口（**没有一处 `use rustmark_ports::`**，实测 grep 无命中），但 `crates/rustmark-assembly/Cargo.toml:19` 仍直连 `rustmark-ports`，且 `workspace_topology_test` 把 {core, infra, ports} 钉为期望。即：**这条边为"端口契约在 ports、impl 在 infra/ports"的语义而声明，但当前代码并不需要**——它在清单里是冗余的，而拓扑断言把冗余固化成"必须存在"。有两种诚实处置：删边（体现"经 core re-export 即可"）或保留并注明"预留直连以便未来不依赖 core 的 re-export"。当前是"保留但没写清为什么代码不用它"。
- **M2 未收敛 root 依赖面（YAGNI 如实保留）**：M2 把 13 impl 从 root 平移到 assembly，但 root 仍依赖 {core, infra, icons, ui}（frame 侧 14 处 `crate::infrastructure::` + `lib.rs` re-export 需要 infra），并**新增** root→assembly 一条边。故本地边 12→16，root 依赖面**不缩反增**。这与 `max-solution.md §5` / `m2-design.md` 的判定一致：**M2 是"平移"而非"收敛"**，其真实收益是"装配逻辑集中 + root 门面变薄 + 未来第二消费者可复用"，而非"减少 root 依赖"。当前**不存在**需要复用装配的第二消费者（包内 integration test 结构性链接 root lib + eframe/egui，无法脱离 root），故这部分收益仍是**预付**。
- **`Services`（14 字段）的 `pub` 面宽**：见 §1。它是组合根对 frame 的"全能力"暴露，收窄在 view_context 层；若将来 frame 子模块增多，"哪个子模块能读哪些字段"仍靠评审而非编译。
- **`assembly` 的 `Services` 与 core 的 `AppServices` 是两个几乎同构的类型**（14 vs 13，差 `assets`），靠穷尽解构同步。新增 AppServices 字段时 assembly 编译失败（好），但**新增 Services 专属字段（如 assets）不在 core 的守卫射程**，靠人。这是"影子类型"的固有维护成本，尚可接受。

### 🔧 改进建议
- **P2**｜对 `assembly → ports` 直连边做明确裁决：要么**删**（`Cargo.toml` 去掉 ports，`workspace_topology_test` 期望改为 {core, infra}，代码本就只用 core re-export），要么**保留并加注释**说明"预留不经 core re-export 的直连能力 / 语义完整性"。当前"声明了但代码不用、断言还钉死"是最含糊的第三种。**改动面**：`assembly/Cargo.toml` + 拓扑断言 + `expected.txt`（若删）。
- **P2**｜M2 结论在文档里如实收敛（`m2-design.md` 已写，但 `metrics.md §1.1` / 关联图仍可补一句"root 依赖面未缩减"），避免后来者误以为 M2 是"减依赖"。
- **P2（观察）**｜若 frame 子模块继续增长，考虑把 `Services` 按能力拆成几个窄句柄（如 `DocumentServices` / `PortServices`），让"谁能读什么"上移到编译期。

---

## 7. 数据一致性

### ✅ 合理之处
- **落盘一致性**：文档/偏好/最近文件/草稿全部走"同目录 tmp → sync → rename"（`fs_document_repository.rs` / `recent_files_store` / `toml_preferences_repository`），原文件不被触碰；`fs_workspace_repository::rename` 先查目标存在再 rename（`L299-306`），避免 POSIX 静默覆盖。
- **缓存失效按文档 ID 精确**：`markdown/service.rs:175-203` 支持 `invalidate(doc_id)`（单文档）与 `invalidate_all()`（主题切换）两条路径，且**只影响目标文档**；三者（render cache / 高亮 / 拼写）同进同退。事件订阅（`EventSubscribers::register`）在 `bus` 被 `workspace` 消费前取好克隆，`provider()` 后置提取打破"文档服务→其余服务"的环（`composition.rs:178-232`）。
- **DTO 投影避免跨层实体泄漏**：`application/dto/` 是跨 crate 唯一投影契约，视图与 ui 状态只认 DTO，不认领域实体（关联图 §4）。
- **编码往返**：`fs_document_repository` 按探测到的编码解码/编码（`L170-174`），BOM/UTF-8/UTF-16/GBK 分级。

### ⚠️ 风险与不足
- **多实例并发编辑同一文件无协调**（同 §3）：最后写入者胜、静默丢改动。纯 FS 设计边界，未显式声明。
- **并发写的临时文件冲突**（同 §3）：tmp 名只用 file_name，双实例保存同一文件会撞同一 tmp 路径。
- **rename 目录项持久性**（同 §3）：未 fsync 父目录，极端断电可能回退到上一次保存。
- **RenderCache 容量的正确性**：`RenderCache::new(RENDER_CACHE_CAPACITY)` 有界 LRU，`shared/support/lru.rs` 是唯一实现；淘汰是丢缓存重算（正确性无损，仅性能）。未见一致性风险。

### 🔧 改进建议
- 同 §3 三条（父目录 fsync / tmp 名加 PID+计数器 / README 声明多实例边界）——这四条在"可靠性"和"数据一致性"两个维度同源，**按最高优先级统一处理**。

---

## 8. 技术选型合理性

### ✅ 合理之处
- **egui/eframe/wgpu**：ADR-001 原生 egui + wgpu，符合需求 §5.1；按平台各开**一个** wgpu 后端（Windows dx12 / macOS metal / Linux vulkan）收体积；`wgpu_no_default_features` 避免连带 `wgpu/default` 全开。虽然 wgpu 是最大 `.text` 组（838.7 KiB），但立即模式 GUI 的合理代价，且已接受。
- **syntect**：只留解析 + 语法集 + `regex-fancy`（纯 Rust，避开 onig），禁 default-themes/yaml-load/plist-load；实测 17.1 KiB，分级裁剪预案（20→10→5）**未触发**——选型与裁剪都克制。
- **pulldown-cmark / ropey**：体积小（60.4 / 11.5 KiB），职责清晰。
- **ureq + rustls（纯 Rust TLS）+ 平台根证书**：三平台行为一致、不链 OpenSSL、不内置 Mozilla 根包；**feature 化可整条剔除 −1.02 MiB**（`remote-images`），单一开关点在 root（避免 infra 多消费者各自把 TLS 拖回）。这是"按需付费"的好设计。
- **rfd**（原生对话框）、**ahash `no-rng`**（键非攻击者可控，关 `runtime-random` 免 getrandom）、**serde derive**、**thiserror/anyhow 分层**——均与场景匹配。
- **Rust 2024 / resolver 3 / MSRV 1.99**：`[workspace.package]` 单点真源版本/edition/license/publish，10 个 manifest 统一继承，避免逐字重复分叉。

### ⚠️ 风险与不足
- **Linux vulkan-only**（同 §2）：可用性 vs 体积的取舍，需真机验证目标 GPU 覆盖率。
- **MSRV 1.99 极新**：自建二进制分发无碍；若将来走发行版打包（Debian/Ubuntu 仓库工具链通常滞后），需评估是否降 MSRV。
- **`image` 解码链 294.5 KiB**：已只开白名单格式、无 svg；进一步再压只能砍图片预览（P1 特性），不建议。
- **体积红线按平台分档（Linux 13.5/13.0 MiB）**：Windows/macOS 仍 11/10.5——分档本身合理（Linux 图形栈更重），但**跨平台"达标"口径不一**，汇报时需强调"二进制口径、分平台"。

### 🔧 改进建议
- **P1（待验证）**｜Linux GPU 基线验证（同 §2），据结果决定是否恢复 gles 运行时回退。
- **P2**｜MSRV 1.99 在 `docs/guides/` 记录"若走发行版打包需评估降级"，把决策依据留档。
- **P2**｜无（其余选型不需要改）。

---

## 总体结论

1. **这是一套"纪律密度很高"的架构**：真正稀有的是机制——词法级分层守卫 + 三道视图守卫 + 帧预约唯一出口 + 穷尽解构 + 拓扑/字段/阈值三类集合断言，把"架构规则"从散文变成了 `cargo build` 会失败的契约。可扩展性、可维护性的地基非常扎实。
2. **安全面处于良好状态**：F-01~F-04 四项已修，MIME/data URI 白名单精确到子类型且与导出消毒同源，SSRF 与路径穿越已堵，HTML 消毒有 doctest 钉住，供应链 `deny.toml` 成熟。剩余是"符号链接越界""远程 MIME 无 magic 校验""硬 abort 残留 PDF 临时文件"这类**边角待验证项**，无 P0/P1 安全缺陷。
3. **M2 的判断必须如实保留**：`rustmark-assembly` 让装配逻辑集中、root 门面变薄（结构收益真实），但 **root 依赖面不缩反增（12→16 边），且当前无第二消费者复用装配**——它是"平移"而非"收敛"，收益部分预付。`assembly → ports` 直连边当前**无代码使用**，属可裁决的声明冗余。
4. **最值得动的几处**（都不改变架构骨架）：① 原子写的两个窄缺口（父目录 fsync、tmp 名加 PID+计数器）；② 字段守卫纳入 assembly 并让"新增 crate 忘登记"变红；③ Linux vulkan-only 的可用性验证与必要时回退；④ 活文档数字漂移收口。其余为观察项。
5. **无 P0 级问题**。整体判定：**架构健康，可继续演进**；上述改进均属"加固/收口"，不涉及重构。

---

## 优先级改进汇总表

| # | 问题 | 影响 | 建议动作 | 优先级 | 预估改动面 |
|---|---|---|---|---|---|
| 1 | `atomic_write` 未 fsync 父目录（`fs_document_repository.rs:219-225`） | 极端断电可回退到上一次保存（内容不坏、目录项可能回退） | rename 成功后 Unix 下 `File::open(parent)?.sync_all()` | P2 | 1 文件 + 单测 |
| 2 | tmp 名注释称"带 PID+计数器"实为只用 file_name（`fs_document_repository.rs:188-194`） | 双实例保存同一文件撞同一 tmp 路径，可能互相覆盖 | 改 `.rustmark-tmp-{pid}-{counter}-{file_name}` | P2 | 1 文件 |
| 3 | Linux 只留 vulkan，无 vulkan 驱动机器启动失败（`Cargo.toml:324-328`） | 老 GPU/VM 上 `wgpu` panic 启动失败（待验证目标用户覆盖） | 真机验证；必要时恢复 gles 运行时回退并重核算体积 | P1（待验证） | Cargo.toml + 启动探测 |
| 4 | `field_visibility_guard` 不含 assembly，扫描根硬编码 7（`tests/field_visibility_guard_test.rs:869`） | 组合根 16 个 pub 字段未被集合相等断言保护；新增 crate 静默漏守 | 加入 `assembly/` 根 + 登记 16 条；或按 `crates/*/src` 扫描 | P2 | 1 测试文件 |
| 5 | `assembly → ports` 声明边无代码使用（`assembly/Cargo.toml:19`，代码全用 `core::ports`） | 清单冗余且被拓扑断言固化，语义含糊 | 删边（期望改 {core,infra}）或加注释说明"预留直连" | P2 | Cargo.toml + 拓扑断言 + expected.txt |
| 6 | 符号链接可越出文档目录读任意文件（`asset_loader.rs:305-327` 仅文本归一） | 不可信文档目录含符号链接时可读任意文件（威胁模型待定） | 加 `canonicalize` 子树校验，或文档登记"文档目录需可信" | P2 | 1 文件 + 测试 |
| 7 | 远程图片无 magic 头校验、仅信响应头/扩展名（`asset_loader.rs:361-374`） | 伪造 MIME 的响应可绕过白名单（下游解码失败会降级，危害有限） | 前 8 字节对白名单位图签名校验 | P2 | policy.rs + 测试 |
| 8 | 多实例并发编辑同一文件无协调（纯 FS 无锁） | 最后写入者胜、静默丢改动；未在 README 声明 | README 已知限制登记该边界 | P2 | 文档 |
| 9 | `arch_check.rs` 2976 行巨型校验器 | 改分层规则需在 3000 行定位，是长期成本 | 按职责拆 `mod`，facade 只 re-export | P2 | 工具文件（fixture 兜底） |
| 10 | 活文档数字漂移（metrics 5 根实为 7、关联图 17 ViewAccess 实 16+1、frame 24/25 实 26） | "spec 即契约"的项目里散文数字陈旧，误导后来者 | 更新 metrics §1.1 / 关联图 §0/§3/§7；或加轻量 CI 校验 | P2 | 多处活文档 |
| 11 | 1MB 档 p95 贴线未达标（16.5ms） | 已按 V11-41 口径 A 定"尽力而为"，非回归 | 维持现状；若要攻单独立项（T5 决策），不混日常迭代 | P2（观察） | — |
| 12 | `incremental.rs`(988) / `highlight_layout.rs`(773) 规模偏大（待验证是否单职责） | 若继续膨胀会降低可读性 | 观察；超阈值时按 `stats/` `find/` 拆法处理 | P2（观察） | — |

> 说明：表中 P0 为空——本评审未发现"必须尽快修"的缺陷。P1 仅 #3（Linux 可用性）为"待验证 + 决策"，验证方法已写在 §2/§8。
