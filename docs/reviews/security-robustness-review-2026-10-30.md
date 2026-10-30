# Rustmark 安全与健壮性审计合并报告

| 项目 | 内容 |
| --- | --- |
| 状态 | 评审稿（Review Draft） |
| 日期 | 2026-10-30 |
| 范围 | 全仓：root crate + 9 个成员 crate（`rustmark-actions / assembly / core / icons / infra / ports / render / shared / ui`） |
| 输入 | 源审计 A（健壮性，14 项发现）、源审计 B（安全，11 项发现） |
| 方法 | 两份只读静态审计的合并、去重、重排优先级；对 P0 关键位置做一次当前代码抽查 |
| 本轮改动 | **无**。本文档任务不修改任何代码、不提交 |
| 架构入口 | `docs/diagrams/project-relationship-map.md`（仅作导航，结论以本报告证据为准） |

---

## 1. 审计范围与方法

### 1.1 覆盖范围

- **持久化与文档生命周期**：文件读写、原子保存、编码探测与回写、草稿、最近文件、偏好落盘。
- **工作区**：目录遍历、跨文件搜索、文件/目录增删改、符号链接与 Windows junction。
- **预览与资源加载**：本地图片、远程图片、data URI、图片解码与缓存。
- **导出与打印**：HTML 导出（白名单清洗）、PDF 打印管线（无头 Chromium / CUPS）、导出件外部资源。
- **系统集成与交互**：外链打开器、原生对话框调度、崩溃日志、自动保存、UI 帧循环。
- **供应链与工程门禁**：`unsafe` 使用、`cargo deny` / `cargo-audit`、HTML 清洗单测。

### 1.2 方法

1. 两份源审计由不同视角独立完成（A 偏健壮性/资源边界，B 偏安全边界）。
2. 本报告逐条比对两份发现，按“同一根因 / 同一代码位置 / 同一修复面”合并，保留所有不同发现。
3. 严重级别只升不降：两份判断不一致时取更保守结论，并在第 6 章写明分歧。
4. 对 P0 涉及的 5 处代码位置做**定向抽查**（不是重新全量审计），行号以当前工作区代码为准；发现漂移即更新（见第 12 章）。
5. 按“安全 / 数据完整性 / 可用性 / 可恢复性”四类归档，并给出 P0/P1/P2 排期建议。

---

## 2. 限制声明（必读）

**本报告的所有结论都来自只读静态审计，未做运行期最小复现。**

- 源审计 A、B 均未启动应用、未做 GUI 实机点击、未做动态渗透测试。
- 外链 RCE（SEC-01）的代码路径已逐行确认（`cmd /C start` 的二次解析确实存在），但**未实机点击验证**。
- PDF 脚本执行（SEC-02）、symlink 跟随写入（DAT-03 的一半）、协议相对 URL 绕过（SEC-05）属“代码已证实、需运行期触发”的发现。
- 部分结论依赖平台行为（Windows junction、Unix 权限、ureq 重定向默认），属静态推断，已在“证据置信度”列标注。
- 静态审计**不能**证明“某处没有问题”，只能证明“已审计范围内未发现问题”。第 10 章列出未覆盖范围。
- 本报告**不含**任何“已修复”表述：本轮没有改代码，所有条目都处于“待修复”。
- 关闭一条发现的最低条件见第 9.3 节：代码修复 + 自动化测试 + 必要的人工/运行期复现记录。

---

## 3. 执行摘要

1. **最严重的问题是外链打开器的命令注入/任意程序执行（SEC-01，critical）**。Windows 上外链经 `cmd /C start "" <target>` 打开，而 `is_safe_to_open` 只拦小写 `javascript:`/`vbscript:`/`data:`。含 `&` 的 URL、本地可执行路径、UNC 路径都能穿透，`cmd` 会对参数二次解析。点击一篇恶意 Markdown 里的链接即可能执行本地任意程序或触发 SMB 凭据外泄。代码级已确认，未实机点击。
2. **PDF 打印链路绕过了 HTML 白名单清洗（SEC-02，high）**。`build_print_html` 只调用 `render_fragment`，不清洗；产物写入 `.print.html` 后交给无头 Chromium 以 `file:///` 打印。正文里的原始 `<script>` 会在本地文件上下文中执行，导出 HTML 侧已有的清洗防线在 PDF 侧完全缺席。
3. **保存路径存在静默数据损坏（DAT-01，critical）**。保存直接用内存全文覆盖，不比对 mtime / size / 内容；外部已修改或已删除的文件会被静默覆盖或重建；`fs::read().ok()` 在读失败时默认按 UTF-8 回写，GBK 文件可能被改坏。
4. **BOM + 非法 UTF-8 会被 lossy 回写成不可逆内容（DAT-02，critical）**。带 UTF-8 BOM 但含非法字节的文件走 `from_utf8_lossy`，保存时把 U+FFFD 写回磁盘，原文不可恢复。
5. **原子写的“原子性”与“临时文件安全性”都不成立（DAT-03，high）**。临时名固定为 `.rustmark-tmp-{file_name}`，注释声称的 PID/计数器并不存在；并发保存会互相覆盖。写入用 `create(true).truncate(true)` 跟随符号链接，预置同名 symlink 可改写任意文件（最近文件存储同一策略）。
6. **资源边界缺失会同时造成 OOM 与 UI 冻结（AVL-02 / AVL-04，high/medium）**。单文件读取、跨文件搜索候选、全文搜索字符表都没有大小上限（约 12 B/字符）；图片先全量 `image::load_from_memory` 再 `thumbnail(1600)`，8 个后台槽位叠加解压炸弹可致 OOM。
7. **崩溃可恢复性与持久化细节仍有缺口（REC-01 / REC-02 / REC-03，low/medium）**。panic hook 实际已安装在 `app.rs:31`（修正源审计 A 的待确认项），但启动极早期窗口与 hook 测试仍缺；`revert` 按锁外读到的旧长度做替换存在竞态（release 下 `panic = "abort"` 会终止进程）；`clear_draft` 失败被 `let _ =` 丢弃，陈旧草稿可能被误恢复。

---

## 4. 风险总览

### 4.1 按严重级别

| 严重级别 | 数量 | 条目 |
| --- | --- | --- |
| critical | 3 | SEC-01、DAT-01、DAT-02 |
| high | 6 | SEC-02、SEC-03、DAT-03、AVL-01、AVL-02、AVL-03 |
| medium | 7 | SEC-04、SEC-05、SEC-06、AVL-04、AVL-05、REC-02、REC-03 |
| low | 7 | SEC-07、SEC-08、REC-01、REC-04、REC-05、REC-06、REC-07 |
| **合计** | **23** | — |

### 4.2 按类别 × 严重级别

| 类别 | critical | high | medium | low | 合计 |
| --- | --- | --- | --- | --- | --- |
| 安全（SEC） | 1 | 2 | 3 | 2 | 8 |
| 数据完整性（DAT） | 2 | 1 | 0 | 0 | 3 |
| 可用性（AVL） | 0 | 3 | 2 | 0 | 5 |
| 可恢复性（REC） | 0 | 0 | 2 | 5 | 7 |
| **合计** | **3** | **6** | **7** | **7** | **23** |

### 4.3 一句话判断

- **安全**：三条外部输入边界（外链打开、PDF 打印、图片加载）没有被同一套策略约束；HTML 导出的清洗质量反而是全仓最好的部分之一。
- **数据完整性**：风险全部集中在“保存”这一个动作上——不检测冲突、不做编码保护、原子写不唯一也不防 symlink。
- **可用性**：架构的并发韧性设计较好，真正的短板是“没有内容/资源上限”，以及 reload/refresh 仍在 UI 帧内同步扫描。
- **可恢复性**：panic hook 已在位（与源审计 A 的怀疑相反），但草稿、还原、锁中毒等恢复路径的边角仍会静默失败。

---

## 5. 统一发现表

> 位置列格式为 `文件:行`（当前工作区实测）。同一发现来自两份审计时，`原审计ID` 用 `+` 连接。
> 证据置信度：`代码级`＝逐行确认；`代码级·已复核`＝本轮抽查复核；`静态推断`＝依赖平台行为或理论分支；`已复核·修正`＝原审计前提被当前代码推翻。

| 统一ID | 原审计ID | 类别 | 严重级别 | 位置（file:line） | 触发条件 | 影响 | 证据置信度 | 修复建议 | 优先级 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| SEC-01 | B C-1（并入 B L-1 的协议判定问题） | 安全 | critical | `crates/rustmark-infra/src/infrastructure/system.rs:40-44`；`crates/rustmark-ports/src/system.rs:51-79`；`src/presentation/views/preview_view/inline/text_render.rs:165-190` | 用户点击恶意 Markdown 中的链接，目标含 `&`、竖线、`^`、`%` 等 cmd 元字符，或本地可执行路径、UNC | `cmd /C start` 二次解析 → 任意本地程序执行 / 命令注入；`\\host\share` 触发 SMB 凭据外泄 | 代码级·已复核（未实机点击） | 改用 `ShellExecuteW` 或 opener crate，不经 shell；URL 策略拒绝 UNC、cmd 元字符、可执行扩展名；本地路径仅允许文档类或“在文件管理器中显示” | P0 |
| SEC-02 | B H-1 | 安全 | high | `crates/rustmark-infra/src/infrastructure/export/system_pdf_exporter.rs:159-180`；`.../export/system_pdf_exporter/dispatch.rs:163-190`；`.../export/pulldown_html_exporter.rs:247-249` | Markdown 正文含原始 `<script>` / `onerror=` 等 HTML，用户导出 PDF | 打印用 HTML 未清洗，无头 Chromium 以 `file:///` 打开并执行脚本 → 本地文件读取、外联、进一步攻击 | 代码级·已复核（需运行期触发） | `build_print_html` 复用 `sanitize_html`；无头打印追加 `--blink-settings=scriptEnabled=false`；补“产物不得含 `<script`”单测 | P0 |
| SEC-03 | B H-2 | 安全 | high | `crates/rustmark-infra/src/infrastructure/persistence/asset_loader.rs:200-263` | Markdown 引用 `![](../secret.png)` 或绝对路径（`C:\...`、`/etc/...`） | 仅靠扩展名/MIME 白名单，可读取并显示工作区外任意 PNG/JPEG/GIF/WebP/BMP → 信息泄露 | 代码级 | 拒绝绝对路径与 `..`；`canonicalize` 后断言仍在文档所在目录/工作区前缀内；越界返回明确错误 | P1 |
| SEC-04 | B M-1 | 安全 | medium | `crates/rustmark-infra/src/infrastructure/persistence/asset_loader.rs:273-331`；`.../asset_loader/agent.rs:45-58` | Markdown 引用内网/环回/元数据地址的远程图片；默认 `load_remote_images=true` | SSRF：内网探测、访问 `169.254.169.254` 类端点；重定向可跳出初始校验；未显式 `https_only` | 代码级（ureq 默认跟随重定向，静态确认） | DNS 解析后拒绝 loopback / RFC1918 / link-local / 元数据地址；限制重定向跳数并逐跳复检；默认 `https_only` | P1 |
| SEC-05 | B M-4 + B L-1 | 安全 | medium | `.../export/html_sanitize/policy.rs:92-118`（`/` 前缀放行在 108-114）；`crates/rustmark-ports/src/system.rs:63-79` | 导出件/Markdown 中出现 `//evil.com/x.png`；或外链写成 `HTTPS://...` | 协议相对 URL 绕过协议白名单，导出件加载外部资源；外链判定大小写敏感导致合法链接被误拒、`file://` 与所有无 `://` 字符串全放行 | 代码级·已复核 | 收敛为单一 `UrlPolicy`：拒绝协议相对与 UNC、统一小写归一、私网判定、scheme 白名单；导出清洗与外链打开共用 | P1 |
| SEC-06 | B M-3 | 安全 | medium | `.../export/pulldown_html_exporter.rs:166-199` | 含公式/图表的文档导出 HTML，读者联网打开 | KaTeX 0.16.47 / Mermaid 10.9.8 的 CDN `<script defer>` 无 `integrity` / `crossorigin`；CDN 投毒即可执行任意脚本，且导出件默认联网 | 代码级 | 补 SRI + `crossorigin`，或导出时内联渲染资源；至少在发行说明中声明联网与供应链风险 | P2 |
| SEC-07 | A M6 | 安全 | low | `crates/rustmark-infra/src/infrastructure/persistence/fs_workspace_repository.rs:176-184` | 工作区内存在 Windows junction 目录 | 只判 `file_type().is_symlink()`，junction 不被跳过 → 重复遍历，靠 depth/entries 上限兜底，仍可能放大扫描成本 | 静态推断（Windows 行为） | 读取 `FILE_ATTRIBUTE_REPARSE_POINT` 一并跳过；与 symlink 同一条日志路径 | P2 |
| SEC-08 | B L-4 | 安全 | low | `.../persistence/fs_workspace_repository.rs:315-338`（canonicalize 在 82-98） | 工作区内的文件/目录被替换为 symlink/junction 后执行删除或保存 | `remove` / `save` 经 canonicalize 作用到链接目标，可能删改工作区外文件 | 静态推断 | canonicalize 后断言目标在工作区前缀内；越界拒绝并提示 | P2 |
| DAT-01 | A C1 | 数据完整性 | critical | `.../persistence/fs_document_repository.rs:246-266`；`crates/rustmark-core/src/application/document/service/lifecycle.rs:86-122` | 文件被外部编辑器修改、被删除，或原文件为非 UTF-8 编码，用户随后保存 | 不做 mtime/size/内容比对 → 静默覆盖外部修改、按错误编码重建已删除文件；读失败 `fs::read().ok()` 后按 UTF-8 回写，GBK 文件被改坏 | 代码级·已复核 | 写前 stat + mtime/size（必要时哈希），不一致进冲突对话框；文件消失但注册表认为存在时提示“已移走”，不重建；保存前记录原编码，读失败即拒绝覆盖 | P0 |
| DAT-02 | A C2 | 数据完整性 | critical | `.../persistence/encoding.rs:105-141`（BOM 分支 107-112） | 文件带 UTF-8 BOM 但含非法字节，用户打开后保存 | BOM 分支直接 `from_utf8_lossy`，保存把 U+FFFD 写回，原文不可逆丢失 | 代码级·已复核 | BOM 且非法时报错或强制只读；记录“文本来自 lossy 解码”的来源标记，禁止覆盖保存 | P0 |
| DAT-03 | A H1 + B M-2 | 数据完整性 | high | `.../persistence/fs_document_repository.rs:178-227`（临时名 188-194、打开 197-202）；`.../persistence/recent_files_store.rs:139-173`（临时名 151-155） | 两个实例/两线程并发保存同一文件；或目标目录被预置同名 symlink | 临时名固定 → 并发写互相覆盖、rename 可能落半截文件；`create(true).truncate(true)` / `File::create` 跟随 symlink → 改写任意文件 | 代码级·已复核 | 临时名带 PID + 单调计数器/随机后缀；`create_new(true)`；失败清理；必要时 `O_NOFOLLOW`；两处 atomic_write 收敛为一份加固实现 | P0 |
| AVL-01 | A H5 | 可用性 | high | `crates/rustmark-core/src/application/event_subscriptions.rs:32-99` | 任一线程在持有 `AutoSaveTracker` 内 Mutex 时 panic | `lock()` 中毒后 `record` 不记录、`due` 返回空、`contains`/`next_due_in` 静默失败，自动保存永久停摆且无告警 | 代码级 | `lock().unwrap_or_else(PoisonError::into_inner)` + 一次性 warn；补“中毒后仍能记录与到期”单测 | P1 |
| AVL-02 | A H3 | 可用性 | high | `.../persistence/fs_workspace_repository.rs:274-280`；`crates/rustmark-core/src/application/workspace/service/search.rs:262-267`；`.../workspace/service/search/scan.rs:161-223` | 打开或搜索 100 MB+ 单文件；工作区含大量大文件 | 整文件读入 + 解码 + 全文 `char_indices` 字符表（约 12 B/字符），无大小/字符数上限 → 内存爆增、UI 冻结 | 代码级 | 引入统一 `ContentLimits`：读前 `metadata().len()` 阈值（建议 8 MiB）跳过并计数；候选总字节预算；`search_text` 改流式/逐行匹配 | P1 |
| AVL-03 | A H4 | 可用性 | high | `src/app/frame/loop.rs:87-94`（注释自述 reload/refresh 仍同步） | 在大目录执行 reload / refresh | 扫描在 UI 帧内同步执行，冻结数百毫秒到数秒；重复触发无 loading 门禁 | 代码级·已复核 | 复用 PickFolder 的后台线程 + 结果轮询；扫描中显示 loading 并禁用重复触发 | P1 |
| AVL-04 | A H6 | 可用性 | medium | `src/presentation/views/preview_view/image_cache.rs:458-472`（`load_from_memory` 459、`thumbnail` 462） | 解压炸弹或超大像素图片，8 个后台解码槽位并发 | 先全量解码再缩到 1600px，峰值内存可达数百 MB，可致 OOM | 代码级 | 用 `image::io::Reader` + `limits()` 在读头阶段拒绝超大图；超限占位提示（需核对当前 image 版本 API） | P1 |
| AVL-05 | A M2 | 可用性 | medium | `src/app/frame/outline.rs:119-142`；`src/presentation/views/sidebar_view/search.rs:146-180` | 1000 条命中时滚动或任意重绘 | 每帧重建分组并持有一份结果（缓存命中仍复制），持续分配、帧时间抬升 | 代码级 | 搜索结果放进视图状态，按 revision/查询指纹失效；帧内只读引用 | P2 |
| REC-01 | A H2 | 可恢复性 | low | `src/main.rs:14-26`；`src/app.rs:26-31` | `app::run` 之前的极早期 panic（`startup_timing` / `env_logger` 初始化） | panic hook 实际已在 `app.rs:31` 安装；残余窗口极小，但缺“hook 已装”的自动化断言 | 已复核·修正 | 把 hook 安装前移到 `main()` 首句（或保留现状并备注），补一条启动即安装的测试；Cargo 仍保留 `panic="abort"` | P1 |
| REC-02 | A M3 | 可恢复性 | medium | `crates/rustmark-core/src/application/document/service/lifecycle.rs:217-256`（长度在 219-229 读取，替换在 237-238） | 用户在 `revert` 的“读盘期间”继续编辑，文档长度变化 | 按旧 `len` 执行 `replace(0..len)` → 越界 panic（release `abort` 直接终止）或范围错位 | 代码级 | 替换前在写锁内复核当前长度；变化则重新计算范围或返回“文档已变化” | P1 |
| REC-03 | A M1 | 可恢复性 | medium | `src/app/frame/files.rs:132-135`、`175-178`、`243-246` | `clear_draft` 失败（权限、占用、磁盘异常） | 结果被 `let _ =` 丢弃，陈旧 `.draft` 残留 → 下次启动误报“待恢复”，可能恢复已丢弃内容 | 代码级 | 记录日志并纳入观测；恢复提示按 mtime 判定新鲜度，过期草稿默认不弹 | P2 |
| REC-04 | A M4 | 可恢复性 | low | `crates/rustmark-core/src/application/workspace/service/search.rs:147-159` | 单文件命中数恰好等于 `MAX_HITS_PER_FILE` | `raw.len() >= MAX` 误标“已截断”，用户误以为还有更多 | 代码级 | 请求 `max_per_file + 1`，仅 `> MAX` 标记截断 | P2 |
| REC-05 | A M5 | 可恢复性 | low | `.../persistence/encoding/writer.rs:76-79` | GBK 目标下遇到双字节表与 GB18030 都覆盖不了的字符（注释称理论不可达） | 把 `\u{FFFD}` 的 UTF-8 字节写进 GBK 字节流 → 编码语义错误、下游解码异常 | 静态推断（防御分支） | 跳过并计数告警，或保存前统一校验可编码性；保留“绝不 panic”的约定 | P2 |
| REC-06 | B L-2 | 可恢复性 | low | `src/app/crash_log.rs:83-109`、`149-158` | 发生 panic 且报告落盘 | 报告含 panic 消息中的绝对路径（本地目录结构）→ 用户提交 Issue 时泄露路径信息 | 代码级 | 路径相对化或截断；在报告渲染处统一脱敏 | P2 |
| REC-07 | B L-3 | 可恢复性 | low | `crates/rustmark-assembly/src/bootstrap.rs:60-129`；`.../persistence/toml_preferences_repository.rs`（无权限收紧） | Unix 多用户主机，其他账户读取配置/草稿目录 | 未设 0600，同机其他账户可读偏好与草稿内容 | 代码级 | 创建文件/目录后设 0600 / 0700（Unix），Windows 依赖用户目录 ACL | P2 |

---

## 6. 去重、合并与分歧裁定

### 6.1 合并项

| 合并后 | 来源 | 合并理由 | 严重级别处理 |
| --- | --- | --- | --- |
| DAT-03 | A H1 + B M-2 | 同一原语：`atomic_write` 的临时文件策略。A 关注并发互覆盖，B 关注 symlink 跟随；两处实现（文档仓储、最近文件）都要改，收敛为一份加固实现 | 取较高：high |
| SEC-01 | B C-1 + B L-1（协议判定部分） | 外链风险同时包含“打开器不经 shell”与“URL 策略太弱”；L-1 的大小写敏感、`file://` 全放行与 C-1 是同一策略的两个症状 | 取较高：critical |
| SEC-05 | B M-4 + B L-1（导出侧 URL 判定） | `is_safe_url` 与 `is_safe_to_open` 是两份漂移的策略；协议相对 URL、大小写、UNC 都应在同一个 `UrlPolicy` 里裁定 | 取较高：medium |
| REC-01 | A H2 | 源审计 A 的结论以“需确认 `app::run` 是否安装 hook”为前提；抽查确认已在 `app.rs:31` 安装，风险显著下降 | **降级**：high → low（见 6.2） |

### 6.2 分歧与裁定

1. **panic hook 是否存在（A H2）**
   - A 判断：`main()` 未见安装，叠加 `panic="abort"` 可能无崩溃日志，列为 high，但明确写了“需确认 `app::run` 是否调用”。
   - 当前代码：`app.rs:26-31` 在一切装配逻辑之前调用 `crash_log::install_hook`，注释也明确“必须是一切逻辑之前”。`main()` 之前只剩 `startup_timing` 与 `env_logger` 两行。
   - 裁定：**原高风险前提不成立**，降为 low（REC-01），保留两项残余工作：hook 前移到 `main()` 首句（可选）、补“启动即安装”的自动化断言。降级有据，不属于“降低严重级别掩盖问题”。

2. **atomic_write 的严重级别（A high vs B medium）**
   - A 从“并发保存互相覆盖、rename 可能落半截文件”出发给 high；B 从“symlink 跟随”出发给 medium。
   - 裁定：两个机制叠加在同一个小函数里，且直接影响用户文件完整性 → 取 **high（DAT-03）**。两个机制都必须修，不能只修一个。

3. **图片边界的两次发现不是同一件事**
   - A H6 是“解码阶段无像素/解压上限”（可用性）；B H-2 是“本地路径接受绝对路径与 `..`”（安全）。二者位置相近但根因不同，分别保留为 AVL-04 与 SEC-03，不合并、不降级。

4. **保存冲突与 BOM 回写**
   - A 将其列为两条 critical（C1、C2）。二者都发生在 `save` 路径，但一个是“不检测外部变化”，一个是“编码降级后写回”，修复手段不同（冲突协议 vs lossy 来源标记），故保留 DAT-01、DAT-02 两条，不合并为一条。

5. **两份审计对“URL 策略”的判断**
   - B 在 C-1（打开器）与 M-4（导出清洗）分别给 critical / medium。本报告不把 M-4 升为 critical：导出件加载外部图片不构成直接 RCE；但 **SEC-01 的 URL 策略修复必须与 SEC-05 共用同一实现**，否则会再次分叉。

---

## 7. P0 / P1 / P2 路线图

> 工作量口径：S ≈ 0.5–1 人日；M ≈ 2–3 人日；L ≈ 4–5 人日（含测试）。依赖关系按“必须先完成”排序。

### 7.1 P0（立即排期，阻断发布）

| 编号 | 条目 | 理由 | 依赖 | 验收标准 | 工作量 |
| --- | --- | --- | --- | --- | --- |
| P0-1 | SEC-01 外链打开器去 shell + URL 策略 | 唯一可被“点击一次”触发的 RCE 面；影响所有打开恶意 Markdown 的用户 | 无（先用最小策略，P1-4 再统一） | 打开器不经 `cmd`；`https://a.com&calc.exe`、`C:\x\evil.exe`、`//host/share`、`\\host\share` 全部拒绝；合法 http(s)/mailto/tel/文档路径仍可打开；有单测 | S |
| P0-2 | SEC-02 PDF 打印前清洗 + 禁脚本 | 导出是高频动作；未清洗的 `file:///` 打印等于本地脚本执行 | 无（可复用现有 `sanitize_html`） | `build_print_html` 产物不含 `<script`、` onerror=`、`javascript:`；无头打印命令带 `scriptEnabled=false`；恶意样例导出后 PDF 不含执行痕迹（人工核验一次） | S |
| P0-3 | DAT-03 原子写唯一名 + `create_new` | 是所有保存/导出/草稿/最近文件的公共原语；并发与 symlink 都直接改用户文件 | 无 | 临时名唯一（PID + 计数器/随机）；预置 symlink 时写盘失败且目标文件不变；并发保存不互覆盖；文档仓储与最近文件共用同一实现 | S |
| P0-4 | DAT-01 保存冲突检测 + 消失文件不重建 | 静默覆盖外部修改是编辑器最不可接受的故障；GBK 回写会造成不可逆乱码 | P0-3（先有可信写原语） | 保存前 stat + mtime/size 比对；冲突弹“保留我的 / 用磁盘的 / 另存副本”；文件已被移走时提示而非重建；原文件读失败时拒绝覆盖并保留脏标记；有集成测试 | M |
| P0-5 | DAT-02 BOM/非法 UTF-8 保护 | 一次保存即造成不可逆内容丢失；触发文件常见于历史编码迁移场景 | 可与 P0-4 同批 | BOM 且非法时报错或只读；lossy 来源标记阻止覆盖；往返测试证明原文不被写回 U+FFFD | S |

**P0 出口标准**：上述 5 项全部有自动化测试；`cargo clippy -D warnings`、`cargo test --workspace`、`cargo deny check` 通过；SEC-01 与 SEC-02 各留一份人工复现记录。

### 7.2 P1（紧随 P0，一个迭代内）

| 编号 | 条目 | 理由 | 依赖 | 验收标准 | 工作量 |
| --- | --- | --- | --- | --- | --- |
| P1-1 | REC-01 panic hook 前移 + 断言 | hook 已存在，但“启动最早处”仍是注释约定而非测试保证 | 无 | hook 在 `main()` 首句（或明确记录现状）；启动期 panic 能在临时目录产出报告；有测试 | S |
| P1-2 | AVL-01 自动保存锁中毒恢复 | 一次 panic 会让自动保存永久停摆，且用户毫无感知 | 无 | 中毒后 `record`/`due` 仍工作；一次性 warn；有“故意中毒”单测 | S |
| P1-3 | REC-02 revert 长度复核 | 并发编辑即可触发 panic；release 下 `abort` 直接退出 | 无 | 写锁内复核长度；变化时返回明确错误或重算范围；并发测试无 panic | S |
| P1-4 | SEC-05 统一 `UrlPolicy` | 阻止外链/导出/图片三处策略继续分叉；也是 SEC-01 的长期收敛点 | P0-1（先落地最小策略） | 单一模块实现 scheme 白名单、拒绝 `//` 与 UNC、小写归一、私网判定；导出与外链共用；表驱动单测 | M |
| P1-5 | SEC-03 本地图片限定工作区 | 恶意 md 可把工作区外图片读进预览，存在信息泄露 | 无 | 拒绝绝对路径与 `..`；canonicalize 后前缀断言；`../secret.png` 与绝对路径用例被拒 | M |
| P1-6 | SEC-04 远程图片禁私网 + 重定向复检 | 默认开启远程图片，SSRF 面长期暴露 | P1-4（复用私网判定） | 私网/环回/链路本地/元数据地址拒绝；重定向逐跳复检；默认 `https_only`；用假 transport 测试（替换现有 localhost 成功用例） | M |
| P1-7 | AVL-02/AVL-03/AVL-04 资源上限 + 异步扫描 | 三项同属“无边界”问题，一次引入 `ContentLimits` 与统一后台扫描可同时收敛 | P1-5/6 可并行 | 单文件 8 MiB 阈值（可配置）；候选总字节预算；搜索逐行匹配；reload/refresh 后台化并有 loading；图片读头阶段拒绝超大图；观测计数 | L |

### 7.3 P2（排期内的加固与清理）

| 编号 | 条目 | 理由 | 验收标准 | 工作量 |
| --- | --- | --- | --- | --- |
| P2-1 | SEC-06 CDN SRI 或内联 | 导出件供应链风险；需产品决定“内联 vs CDN” | 导出件 CDN 标签带 `integrity`/`crossorigin`，或改为内联；离网打开行为有说明 | S/M |
| P2-2 | SEC-07/SEC-08 文件系统边界 | junction 与“被替换为 symlink 的路径”属低频但影响面大 | junction 跳过；删除/保存前工作区前缀断言；有平台条件测试 | M |
| P2-3 | AVL-05 搜索结果去每帧克隆 | 大结果集下持续分配，拖慢帧 | 结果进视图状态，按 revision 失效；1000 条命中滚动无重复分配 | M |
| P2-4 | REC-03/04/05 细节正确性 | 草稿误恢复、截断误报、防御分支字节错误 | 三项各有单测；草稿按 mtime 判新鲜度 | S |
| P2-5 | REC-06/07 敏感信息落盘 | 崩溃报告路径与 Unix 权限 | 报告路径脱敏；Unix 0600/0700 | S |
| P2-6 | 工程门禁与观测 | 防止回归；把“跳过/拒绝/中毒/冲突”变成可观测指标 | CI 阻断 `cargo deny check`；明确 cargo-audit 取舍；五项指标有计数与日志 | M |

---

## 8. 结构性修复设计

> 下面的设计不是“再加一层补丁”，而是把四类边界收敛成可复用、可测试的单一实现。P0/P1 中的单项修复应尽量落在这些结构里。

### 8.1 内容信任模型

把全仓输入分两级，并在类型上表达：

| 级别 | 来源 | 允许的操作 |
| --- | --- | --- |
| 不可信 | Markdown 原文、正文内嵌 HTML、本地/远程图片、导出件的读者环境 | 必须先过清洗/策略/上限，再进入渲染或落盘 |
| 可信 | 导出骨架（`<meta charset>`、`<style>`、`<title>`）、本进程生成的常量 | 可直接拼装，不参与清洗 |

当前问题正是信任边界不统一：HTML 导出把“正文 + TOC”送清洗、骨架在外（正确），而 PDF 打印把整段不可信正文直接塞进 `file:///` 打印（错误）。

### 8.2 `SanitizedHtml` newtype

- 定义：只有经过 `sanitize_html` 的字符串才能构造 `SanitizedHtml`；`build_print_html` / 写文件接口只接受 `SanitizedHtml`。
- 收益：让“忘记清洗”变成编译错误，而不是靠注释提醒（`build_printable_document` 的注释恰好在鼓励“不要清洗”）。
- 迁移：HTML 导出路径先改造，PDF 打印路径复用；骨架仍用普通 `String`。

### 8.3 单一 `UrlPolicy`

- 输入：URL/路径字符串 + 使用场景（打开外链 / 导出资源 / 加载图片）。
- 输出：`Allow` / `Reject(reason)`，并给出规范化后的目标。
- 规则：scheme 白名单（http/https/mailto/tel）、拒绝 `//` 与 UNC、拒绝 cmd 元字符、拒绝可执行扩展名、私网/环回判定、可选 `https_only`、大小写归一。
- 现状：`is_safe_to_open`（ports）与 `is_safe_url`（infra export）两份实现已开始漂移；SEC-01/SEC-05 应合并到同一份实现，导出侧只保留“允许的相对资源”差异。

### 8.4 `ContentLimits`

统一结构（建议放 `rustmark-ports` 或 `shared`，零 IO）：

- `max_file_bytes`：单文件读取上限（建议 8 MiB，可配置）。
- `max_candidates_bytes`：跨文件搜索候选总字节预算。
- `max_chars_in_memory`：全文搜索字符表预算。
- `max_image_pixels`：解码前像素上限（替代“解码后缩略”）。
- 所有超限路径统一返回“跳过并计数”，不允许静默吞掉。

### 8.5 加固版 `atomic_write`

收敛为一份实现，至少包含：

1. 临时名 = `.rustmark-tmp-{pid}-{单调计数器或随机}-{file_name}`；
2. `OpenOptions::new().create_new(true).write(true)`，Unix 可加 `O_NOFOLLOW`；
3. 写入 → `flush` → `sync_all` → `rename`；
4. 任一步失败清理临时文件，并保持原文件不变；
5. 保存前 `stat` + 冲突检测（DAT-01）；
6. 文档仓储、草稿、最近文件、偏好、导出全部复用。

### 8.6 CI 门禁

- `cargo deny check` 已是有效基线（advisories/bans/licenses/sources 全过），需**在 CI 中阻断**而非仅本地运行。
- `cargo-audit` 未安装：需明确“以 `cargo deny` advisories 为唯一来源”或补装；两者不要长期并存却都不阻断。
- 关键持久化路径的 `unwrap/expect/panic`：由 `warn` 提升为 `deny` 或增量门禁（至少覆盖 `fs_document_repository`、`lifecycle`、`search/scan`）。
- 架构校验（`tools/arch_check.rs`）与阈值基线（`thresholds.baseline.yaml`）继续生效；新增“视图不得直接持有宽上下文”的既有守卫。

### 8.7 敏感信息落盘

- 崩溃报告：路径相对化/截断；不写入文档内容（现状已如此，保持）。
- Unix：偏好、草稿、崩溃目录 0600/0700。
- 日志：`log` 已在 `main()` 初始化（默认 `warn`），把“跳过超大文件数 / 搜索耗时 / 图片解码拒绝数 / 锁中毒次数 / 保存冲突次数”做成结构化计数。

---

## 9. 验证与测试计划

### 9.1 P0 用例（建议先写失败测试，再修）

| 条目 | 用例 | 断言 | 命令 |
| --- | --- | --- | --- |
| SEC-01 | 对 `is_safe_to_open` / 新 `UrlPolicy` 的参数化表：`https://a.com&calc.exe`、`C:\x\evil.exe`、`//host/share`、`\\host\share`、`javascript:alert(1)` | 全部拒绝；`https://example.com`、`mailto:`、`tel:`、文档相对路径仍允许 | `cargo test -p rustmark-ports` |
| SEC-01 | 预览点击集成测试（假 opener） | 不安全目标不调用 `open`；安全目标恰好调用一次且参数未被改写 | `cargo test --test presentation`（或现有预览集成测试目标） |
| SEC-02 | `build_print_html` 输入含 `<script>alert(1)</script>`、`<img onerror=...>` | 产物不含 `<script`、` onerror=`、`javascript:` | `cargo test -p rustmark-infra build_print_html` |
| SEC-02 | 无头打印命令构造 | 参数含 `--blink-settings=scriptEnabled=false`（Windows 分支） | `cargo test -p rustmark-infra shell_print_command` |
| DAT-01 | 临时目录打开文件 → 外部改写 → 保存 | 返回冲突决策，磁盘内容未被覆盖；用户选择“用磁盘的”后内容一致 | `cargo test -p rustmark-infra save_conflict` + 应用层集成测试 |
| DAT-01 | 外部删除文件 → 保存 | 提示“文件已移走”，不重建同名文件 | 同上 |
| DAT-02 | BOM + 非法字节样例 → 打开 → 保存 | 拒绝覆盖或只读；文件字节不变 | `cargo test -p rustmark-infra encoding` |
| DAT-03 | 目标目录预置 `.rustmark-tmp-*` symlink → 保存 | 写入失败，symlink 指向的文件不变；目标文件不变 | `cargo test -p rustmark-infra atomic_write` |
| DAT-03 | 两线程并发保存同一路径 | 临时名不冲突；最终文件为某一完整版本，无半截内容 | 同上（可用 barrier 控制并发） |

### 9.2 P1 用例

| 条目 | 用例 | 断言 |
| --- | --- | --- |
| REC-01 | 在临时目录安装 hook 后故意 panic | 生成 `crash-*.txt`；不含文档正文；最多 10 份 |
| AVL-01 | 线程持锁 panic → 再 `record` / `due` | 仍能记录并到期；日志有一次 warn |
| REC-02 | `revert` 读盘期间并发 `replace` 改变长度 | 无 panic；返回明确错误或替换范围正确 |
| AVL-02 | 构造 100 MB 文本文件 | 读取被跳过并计数；UI 无冻结；搜索逐行处理 |
| AVL-03 | 大目录 reload / refresh | 后台线程执行，UI 显示 loading，重复点击被禁用 |
| AVL-04 | 伪造超大像素图片头 | 读头阶段拒绝并出占位，不进入全量解码 |
| SEC-03 | `../secret.png`、绝对路径、工作区外文件 | 全部拒绝；工作区内图片正常 |
| SEC-04 | 环回/RFC1918/`169.254.169.254`、重定向到私网 | 全部拒绝；正常公网图（假 transport）通过 |
| SEC-05 | `//evil.com/x.png`、`HTTPS://...`、`file://` | 前两者拒绝；后者按场景明确（打开本地文件允许，导出资源拒绝） |

### 9.3 关闭条件（适用于全部条目）

一条发现可被标记“关闭”，必须同时满足：

1. 代码修复已合入，且修复点有对应测试；
2. 静态审计里“需运行期触发”的条目（SEC-01、SEC-02、DAT-03 symlink、SEC-05）附**运行期复现/修复前后对比记录**；
3. GUI 相关（预览点击、打印、扫描 loading）附一次人工验证记录；
4. 安全类条目由安全评审确认，产品类决策（冲突文案、图片沙箱范围、SSRF 策略）由产品确认。

**只改代码、不补运行期证据，不得关闭。**

### 9.4 建议加入的工程化测试

- 对 `pulldown_parser` 与 `search/scan.rs` 做 fuzz / property：任意字节不 panic。
- 关键持久化路径的 `unwrap/expect/panic` 门禁（见 8.6）。
- 观测指标断言：跳过超大文件数、检索耗时、图像解码拒绝数、锁中毒次数、保存冲突次数。

---

## 10. 未覆盖范围与下一轮专项审计建议

> 以下为“未审计”，不等于“无问题”。

### 10.1 源审计 A 明确未覆盖

- `rustmark-render` 的 `math`、`mermaid` 渲染路径。
- `pulldown_parser` 的 `start.rs` / `end.rs` / `event_map.rs`。
- `incremental.rs`（增量解析）。
- `view_context.rs`（宽接口收敛后的运行时行为）。
- `highlight_layout.rs`（高亮布局）。
- `dialog_runner.rs`（原生对话框后台执行）。
- 窗口关闭 / 脏文档关闭流程。
- `favorites` / 偏好落盘。
- `theme` 自定义调色板加载。

### 10.2 源审计 B 未覆盖 / 限制

- C-1 无 GUI 实机点击；H-1 / M-2 / M-4 代码已证实但需运行期触发；未做动态渗透。
- 未覆盖导出件在真实浏览器中的行为、系统打印驱动差异、企业代理环境。

### 10.3 下一轮建议（三个专项）

1. **导出/打印信任模型与供应链专项**：`SanitizedHtml` 落地、PDF 全平台后端、CDN/SRI/内联决策、导出件离线行为。
2. **文件系统边界专项**：symlink / junction / UNC / 大小写 / 权限 / 原子写跨平台一致性（Windows 优先）。
3. **崩溃恢复与持久化一致性专项**：草稿新鲜度、窗口关闭序列、自动保存、锁中毒、外部变更检测的完整状态机。

每个专项都应包含**运行期复现**与最小 PoC，不再只做静态阅读。

---

## 11. 正向基线（不能抵消上述发现）

这些是真实的优点，应保留并作为回归基线：

- **全仓 `unsafe` 为 0**：没有手工内存管理面，消除了整类未定义行为风险。
- **HTML 导出有手写白名单且单测覆盖较好**：`sanitize_html` 的边界（正文 + TOC 在内、骨架在外）设计正确，TOC 转义有领域层标记 + `TocBuilder::verify` 兜底。
- **`cargo deny check`（0.20.2）advisories / bans / licenses / sources 全过**：依赖面基线干净；唯一缺口是 `cargo-audit` 未安装且有无效力定义。
- **架构韧性设计较好**：唯一帧预约出口、高亮后台 revision 丢弃、工作区遍历深度/条目/符号链接上限、图片并发槽位与缓存字节预算、导出调度端口化（`PrintDispatcher` 可注入）。
- **编码探测做了认真处理**：UTF-8 BOM → UTF-16 BOM → 裸 UTF-8 → GBK 兜底，且区分“损坏 UTF-8”与“真 GBK”；问题只在 BOM 分支的 lossy 兜底（DAT-02）。
- **无 CLI 参数面**：减少了参数注入类攻击面。

**但**：这些基线只说明“没有某类问题”，不能抵消 SEC-01 的 RCE、SEC-02 的脚本执行与 DAT-01/02 的数据损坏。

---

## 12. 对 P0 关键位置的当前代码抽查

抽查范围仅 P0 五项涉及的 8 个位置；未重新全量审计。结论：**全部发现成立**，个别行号漂移已在本报告表格中更新。

| 条目 | 源审计引用 | 当前代码 | 结论 |
| --- | --- | --- | --- |
| SEC-01 打开器 | `infra/system.rs` L42-44 | `system.rs:40-44`（Windows 分支 `cmd /C start "" target`），macOS/Linux 在 46-53 | 成立；行号微调 |
| SEC-01 策略 | `ports/system.rs` L51-78 | `ports/system.rs:51-79`（`is_safe_to_open`）；仅拦小写 `javascript:`/`vbscript:`/`data:`，`://` 前放行所有无协议字符串与 `file://` | 成立；函数尾部 +1 行 |
| SEC-01 调用点 | `text_render.rs` `handle_span_click` | `text_render.rs:165-190`，`link` 分支经 `is_safe_to_open` 后调用 opener | 成立 |
| SEC-02 打印 HTML | `system_pdf_exporter.rs` L159-163 | `system_pdf_exporter.rs:159-180`：`build_print_html` 内 `render_fragment`（L163）后直接 `build_printable_document`，无 `sanitize_html` | 成立；原引用只覆盖函数头 |
| SEC-02 打印命令 | `dispatch.rs` L164-174 | `dispatch.rs:163-190`（`headless_print`），参数无 `scriptEnabled=false` | 成立；参数列表已扩至 L190 |
| SEC-02 可打印文档 | `pulldown_html_exporter.rs` L247 | `pulldown_html_exporter.rs:247-249`（`build_printable_document` 不做清洗），注释明确“不额外扩大攻击面” | 成立；L247 未漂移 |
| DAT-01 保存 | `fs_document_repository.rs` L246-266；`lifecycle.rs` L86-122 | `fs_document_repository.rs:246-266`：`save` 直接 `fs::read(&fs_path).ok()`，不比对 mtime/size；`lifecycle.rs:86-122` 保存后 `mark_saved` | 成立；行号未漂移 |
| DAT-02 BOM | `persistence/encoding.rs` `decode` | `encoding.rs:105-141`，BOM 分支 L107-112 `from_utf8_lossy` 且返回 `Encoding::Utf8Bom` | 成立 |
| DAT-03 临时名 | `fs_document_repository.rs` L188-194；`recent_files_store.rs` L151-155 | `fs_document_repository.rs:188-194` 固定 `.rustmark-tmp-{file_name}`；`fs_document_repository.rs:197-202` `create(true).truncate(true)`；`recent_files_store.rs:151-155` 同一命名 + `File::create` | 成立；行号未漂移 |
| REC-01 hook | `main.rs` L14-26 + `crash_log.rs` | `main.rs:14-26` 未安装；`app.rs:26-31` **已安装** `install_hook` | **原前提不成立**，已降级为 REC-01（low） |

---

## 13. 评审者 checklist

先看什么、哪些需要拍板：

- [ ] **先读第 3 章执行摘要 + 第 5 章的 SEC-01 / SEC-02 / DAT-01 / DAT-02**：这四条决定本轮是否阻断发布。
- [ ] 确认 P0 五项（第 7.1 节）的负责人、排期与联调窗口；P0-3 必须先于 P0-4。
- [ ] **产品/安全拍板：SEC-01 修复后，“本地路径”是否仍允许直接打开？只允许“在文件管理器中显示”是否可接受？**
- [ ] **产品拍板：DAT-01 冲突对话框三出口文案（保留我的 / 用磁盘的 / 另存副本）与默认选项。**
- [ ] **产品/安全拍板：SEC-03 是否允许用户显式打开工作区外的绝对路径图片（默认应禁止）。**
- [ ] **产品/安全拍板：SEC-04 私网拦截是否会影响企业内网图床；是否默认 `https_only`。**
- [ ] **性能/产品确认：AVL-02 的单文件 8 MiB 阈值、候选总字节与字符预算。**
- [ ] **安全/CI 拍板：`cargo-audit` 是否引入，或明确以 `cargo deny` advisories 为唯一来源并阻断 CI。**
- [ ] 确认平台优先级：SEC-01 / SEC-02 / DAT-03 / SEC-07 均为 Windows 高发，macOS/Linux 的覆盖时点需明确。
- [ ] **同意关闭条件（第 9.3 节）：静态审计结论必须附运行期复现/修复前后记录才能关闭**；仅改代码不算关闭。

---

### 附：源审计条目到统一 ID 的映射

| 源审计 A | 统一 ID | 源审计 B | 统一 ID |
| --- | --- | --- | --- |
| C1 | DAT-01 | C-1 | SEC-01 |
| C2 | DAT-02 | H-1 | SEC-02 |
| H1 | DAT-03 | H-2 | SEC-03 |
| H2 | REC-01（降级） | M-1 | SEC-04 |
| H3 | AVL-02 | M-2 | DAT-03（合并） |
| H4 | AVL-03 | M-3 | SEC-06 |
| H5 | AVL-01 | M-4 | SEC-05 |
| H6 | AVL-04 | L-1 | SEC-01 / SEC-05（拆分合并） |
| M1 | REC-03 | L-2 | REC-06 |
| M2 | AVL-05 | L-3 | REC-07 |
| M3 | REC-02 | L-4 | SEC-08 |
| M4 | REC-04 | — | — |
| M5 | REC-05 | — | — |
| M6 | SEC-07 | — | — |

**报告结束。**
