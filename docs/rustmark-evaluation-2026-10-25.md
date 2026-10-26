# rustmark 全方位评估报告

> 评估日期：2026-10-25
> 协作方式：阿客实验室研发部多智能体并行评估（架构设计 / 代码质量 / 文档一致性三维度分头负责，主理人汇总）
> 评估口径：只读静态分析（Grep / Glob / Read + 关键文件精读），未修改任何文件
> 项目基线：rustmark 0.2.0，Rust 2024 + egui 0.37 + eframe(wgpu)；workspace 5 crate（root / rustmark-core / rustmark-actions / rustmark-icons / rustmark-ui）；core 170 文件/27,892 行、ui 73/17,651、root 85/21,188；门禁 ≈2164 测试；release exe ≈10.52 MB（红线 11 MiB）

---

## 0. 执行摘要

### 0.1 总体结论
rustmark 是一套**工程纪律极强、文档与代码同源性很高**的成熟项目：分层、依赖矩阵、`shared` 准入三问、`mod.rs` 禁令、帧预约唯一出口、豁免/登记双表、`shared` 内部无环、workspace 拓扑断言构成了一套几乎无死角的机械门禁，且评估中逐项取证确认**当前代码零架构违例、零 `unsafe`、生产路径 `unwrap/expect/panic` 已基本清零、安全/隐私文档与代码高度吻合**。

因此本次评估的价值**不在复述已落地的守卫**，而在三处：
1. **确凿的正确性 bug**（代码质量维度）—— 正则字符类区间在大小写不敏感时漏匹配大写字母，影响查找/替换与跨文件搜索两处核心功能。
2. **机械守卫的语义盲区**（架构维度）—— `check-arch` 作为词法级守卫，对 `super::` 相对路径与 glob 导入两类跨层写法完全失明。
3. **重构后的文档滞后**（文档维度）—— 表现层与配置文档仍指向已迁入 `rustmark-ui` / `rustmark-core` 的旧 `src/` 路径，会直接误导接手者。

### 0.2 优先级总览（34 项，去重合并）

| 优先级 | 数量 | 维度分布 |
|---|---|---|
| 🔴 高 | 3 | 代码 1、文档 2 |
| 🟡 中 | 16 | 架构 5、代码 5、文档 6 |
| 🟢 低 | 15 | 架构 5、代码 5、文档 5 |

### 0.3 最该优先处理的 Top 项
- **E-01（高·代码）**：正则字符类区间大小写不敏感漏匹配大写字母 —— 唯一确凿正确性 bug，修复成本极低（折叠 `ch` 一行 + 补测试），应排第一。
- **E-02 / E-03（高·文档）**：表现层技术文档与 config-reference 仍指向已迁移路径 —— 直接误导开发者定位源码，仅改文档前缀即可闭环。
- **E-04 / E-05（中·架构）**：`super::` 与 glob 导入绕过层禁令 —— 词法守卫的确定性未来漏检缺口，建议推进到机械校验。
- **E-09（中·代码）**：迷你正则引擎无步骤上限（ReDoS）—— UI 线程同步执行，潜在可用性炸弹。

---

## 1. 评估范围与方法

| 维度 | 负责角色 | 范围 | 方法 |
|---|---|---|---|
| 架构设计 | 顾图南（software-architect） | 分层落地、模块边界、依赖耦合、机械守卫盲区、crate 切分权衡、测试金字塔 | 通读 `docs/architecture-constraints.md` 与 `tools/arch_check.rs`，对照 `Cargo.toml`、拼接点、事件总线、拓扑断言取证 |
| 代码质量 | 霍承钧（backend-architect） | 核心逻辑正确性、可维护性、错误处理、性能瓶颈、并发健壮性 | Grep 统计 `unwrap/expect/panic` 分布 + 关键模块精读（undo/坐标/正则引擎/asset_loader/导出消毒/聚合 apply） |
| 文档一致性 | 关千锤（release-gatekeeper） | 文档↔代码一致性、缺失说明、过时内容（重点 V11 重构后滞后） | Grep 文档路径串→对照文件系统；复算 crate 规模；核对枚举计数与代码 |

**方法约束**：全部为只读分析；避免触发 `cargo build` 等大耗时写盘；文档声明一律到代码取证，不轻信。

---

## 2. 三维度结论概览

- **架构设计（成熟度：高）**：分层与依赖矩阵实际落地且与约束文档一致，crate 间无环、方向正确，事件总线耦合受控。最大风险在**守卫本身的语义边界**（词法守卫对 `super::`/glob 失明）与**拆 crate 的耦合税**（169/548 条 `pub` 字段白名单 + 22 setter）。
- **代码质量（成熟度：高）**：分层边界清晰、`check-arch` 机械校验、clippy 把 `unwrap/expect/panic` 设 warn、零 `unsafe`、常量集中、撤销/光标/UTF-8 边界严谨。除 1 个确凿 bug 与 1 个 ReDoS 隐患外，多为中低优先健壮性/性能打磨。
- **文档一致性（成熟度：中高）**：架构真源（constraints §7/§10.1/§10.2）已更新为 V11 后布局，最可信；主要滞后集中在阶段技术文档（06/10/11）与配置文档（config-reference）的文件系统路径。安全/隐私文档与代码吻合，已废弃假设无残迹，**未发现"文档比代码新"的系统性反向问题**。

---

## 3. 统一问题清单（按优先级排序）

> 编号规则：`E-` 统一编号；标注 `［维度｜优先级］`。每项含【问题描述】【影响分析】【改进建议】【位置】。

### 🔴 高优先级（3 项）

#### E-01 · 正则字符类区间在大小写不敏感时漏匹配大写字母 ［代码质量｜高］
- **问题描述**：`crates/rustmark-core/src/shared/pattern.rs:295-302` `class_matches` 的 `ClassItem::Range` 分支，case-insensitive 时只把 `low/high` 折叠为小写，**未折叠待匹配字符 `ch`**，直接做 `low <= ch && ch <= high`。后果：`[A-Z]`/`[a-z]` 在忽略大小写时只能命中小写，匹配不到大写（如 `"ABC"` 用 `ci:[A-Z]` 搜不到 `A`）。该引擎同时服务查找替换面板（`engine.rs`）与侧栏跨文件搜索（`search.rs:454`）。单字符类走 `One` 分支正确，故极易被遗漏。无既有测试覆盖此情形。
- **影响分析**：查找/替换与跨文件搜索在「忽略大小写 + 字符类区间」组合下**静默漏掉大写命中**，结果错误，且用户无感知。
- **改进建议**：在 case-insensitive 分支同时 `fold_lower(ch)` 后比较（或统一改为与 `chars_eq` 一致的双向折叠，保持端点同侧保证区间有序）。补回归测试：`Regex::compile("[A-Z]", false)` 在 `"ABC"` 应命中 `A`；`"[a-z]" ci` 在 `"ABC"` 应命中 `A/B/C`。
- **位置**：`crates/rustmark-core/src/shared/pattern.rs:295-302`

#### E-02 · 表现层技术文档（06/10/11）路径指向已迁移模块 ［文档一致性｜高］
- **问题描述**：`docs/06-表现层与交互实现.md`、`docs/10-编辑区技术实现.md`、`docs/11-预览区技术实现.md` 仍把 `src/presentation/{state,theme,shortcuts,rich}/...` 及 `src/presentation/views/{math,mermaid}/` 当作当前文件路径。实际这些非视图子模块已于 V11-62 全迁 `crates/rustmark-ui/src/presentation/`，root 仅保留 `views`，且 root `src/presentation.rs:11` 用 `pub use rustmark_ui::presentation::{rich,shortcuts,state,theme,widgets}` 桥接。`math/mermaid` 在 `rich/` 下、`views` 下无 math/mermaid。
- **影响分析**：开发者按文档定位源码会扑空，误判「哪些模块在 root、哪些在 core/ui」，最易误导新接手者。
- **改进建议**：在 06/10/11 顶部加「模块现住 crate」说明；把文件前缀改为 `crates/rustmark-ui/src/presentation/...` 与 `crates/rustmark-core/src/...`，并注明「经 root `pub use` 桥接，`crate::presentation::*` 逻辑路径不变」。
- **位置**：`docs/06:40,59,75,92,221,228,335`；`docs/10:3,90,93-96,381-384`；`docs/11:3,90,369-374` → 代码 `src/presentation.rs:11`、`crates/rustmark-ui/src/presentation/`

#### E-03 · config-reference.md 用户面配置文档路径失效 ［文档一致性｜高］
- **问题描述**：`config-reference.md`（发布合规 DOC-03）引用 `src/domain/preferences/aggregate.rs`、`src/infrastructure/persistence/toml_preferences_repository.rs`、`src/infrastructure/fonts.rs` 等 root 路径，实际在 `crates/rustmark-core/src/`。验证：`src/` 无 `domain/`、`infrastructure/` 目录。
- **影响分析**：用户/维护者溯源配置实现扑空；合规文档应准确。
- **改进建议**：前缀改为 `crates/rustmark-core/src/`，补一句「core 经 root `pub use` 桥接，逻辑路径 `rustmark::domain::…` 不变」。
- **位置**：`config-reference.md:6-7,44,55,68,105,135` → `crates/rustmark-core/src/domain/preferences/aggregate.rs`、`crates/rustmark-core/src/infrastructure/persistence/toml_preferences_repository.rs`（实测存在）

### 🟡 中优先级（16 项）

#### E-04 · `super::` 相对路径绕过层禁令，守卫完全失明 ［架构设计｜中］
- **问题描述**：`tools/arch_check.rs:1817-1888` 的 `collect_candidates` 只把以 `crate::`/`rustmark_core::` 起头或裸 crate 名的路径视为候选；`use super::application::…`、`use super::super::infrastructure::…` 等相对跨层写法不会被任何 forbidden 表命中。现有 `super::` 用法经穷查均停留同层（如 `application/document/service/drafts.rs:9`），当前零违例。（注：`shared` 内部无环守卫已强制绝对路径以堵同类盲点，层禁令未跟进。）
- **影响分析**：一旦有人为图省事写 `use super::super::application::…`（Rust 合法），层禁令静默失效，无编译错误、常规单测不覆盖——与历史上「帧预约漏写」同类隐患。
- **改进建议**：在 `collect_candidates` 中，对 `super::`/`self::` 起点也按「解析后所属层」判定；或要求非测试 src 一律用绝对 `crate::` 路径，并在 `check-arch` 里把相对跨层路径报违规（借鉴 `shared` 无环守卫做法）。
- **位置**：`tools/arch_check.rs:1817-1888`；佐证 `crates/rustmark-core/src/application/document/service/drafts.rs:9`

#### E-05 · glob 导入（`use crate::X::*`）绕过层禁令与 R-13 子层方向 ［架构设计｜中］
- **问题描述**：`parse_use_tree` 对 `use a::*` 展开为 `a::*`，`matches_ban`（`tools/arch_check.rs:1121-1126`）不匹配任何「层段前缀」禁令。故 `use crate::infrastructure::*;`（domain/application 文件）或 `use crate::presentation::*;`（state 子层）会静默拉入跨层符号而不报违规。现存 glob 均为同层内（如 `application/preferences/service.rs:30-31`），无滥用。
- **影响分析**：与 E-04 同源——词法守卫对「展开后」的符号不可见，未来跨层 glob 漏检。
- **改进建议**：在 `check_file` 中对 `use crate::<层>::*;` 形式、若目标层对当前层是 forbidden，直接报违规（要求显式具名导入）；或同样改为要求绝对具名导入。
- **位置**：`tools/arch_check.rs:1020-1027`（Star 分支）、`1121-1126`

#### E-06 · infrastructure `egui` 棘轮为文件级「衰减-only」边界，且 headless 复用前提受制于 core 自身依赖 egui ［架构设计｜中］
- **问题描述**：`V11_63_KNOWN_EGUI_LEAKS`（`tools/arch_check.rs:121-127`）仅放行 5 个文件（fonts 系 3 + viewport_capture 系 2），**清单内文件可随意新增 `egui`**；清单外新增才被拦。且 `crates/rustmark-core/Cargo.toml:26` 使 core 本身依赖 `egui`（绘制原语）。结果：「core 可 headless 复用」前提短期不成立，`rustmark-infra` 拆分触发条件（`cargo tree -p rustmark-core | grep egui == 0`）当前无法满足。已确证非清单文件中的 `egui` 字样全在注释中，无真实新增泄漏。
- **影响分析**：5 个文件是 unbounded 的耦合汇；只要不清空，core 永远拖着 egui，任何 headless/CLI/嵌入消费者都被迫引入整条 egui 依赖。
- **改进建议**：将 V11-63 端口化落地（fonts/viewport_capture 改为产出字节/端口类型、执行端外移），随后删除 5 条放行；同时明确「core 依赖 egui 仅限 epaint 文本布局原语（无窗口/后端）」，把「headless 复用」重新定义为「无窗口复用」，消除文档与现实张力。
- **位置**：`tools/arch_check.rs:98-127`；`crates/rustmark-core/Cargo.toml:26`；`docs/architecture-constraints.md:106-117`
- **✅ 已收口（2026-10-26，V11-63 / E-06 两阶段）**：本条的**根因**（core 依赖 egui）已消除——`crates/rustmark-core/Cargo.toml` 的 `egui` 依赖已删，`cargo tree -p rustmark-core | grep -c egui` 实测 **0**；`V11_63_KNOWN_EGUI_LEAKS` 常量与放行分支双删，`INFRASTRUCTURE_FORBIDDEN` 的 egui 项升为无条件生效（验收用例 `former_v11_63_leak_paths_are_now_caught` 把 5 个历史路径钉成"必报"）。建议中"把 headless 复用重新定义为无窗口复用"这一折中**不再需要**：端口化已让 core 真正 egui 无关。上述"位置"与行号是 2026-10-25 快照，现状见 `docs/architecture-constraints.md` §4 与 tasklist V11-63 节后收官记录。

#### E-07 · `views 留 root / state 留 core` 的跨 crate 字段保护摩擦，是持续增长的耦合税 ［架构设计｜中］
- **问题描述**：为保留 R-11/R-12 的跨 crate 字段保护不被降级，引入 169/548 条 `pub` 字段白名单 + 22 个 `pub` setter + 6 行桥接（`views` 经桥接访问 core state）。白名单由 `tests/field_visibility_guard_test.rs` 集合相等双向断言守门，安全但随功能增长。
- **影响分析**：白名单越大，越反向说明 crate 边界切在了一处紧耦合上；每次新增跨 crate 可见字段都要同时改清单，演化摩擦显著。若未来真出现非 UI 消费者触发 `rustmark-infra` 拆分，此摩擦还会加剧。
- **改进建议**：定期度量白名单增速；若增速持续，重评「state 是否应与 views 同留 root」或「由 `EditorApp` 直接持有 UI 状态、state 退为纯逻辑容器」。短期维持现状（守卫已到位），但把「白名单条目数」作为架构健康度指标纳入 metrics。
- **位置**：`tests/field_visibility_guard_test.rs`（548 条目集合相等）；`docs/architecture-constraints.md:559`、`§10.3`

#### E-08 · R-13 的「views 方向」禁令在 ui crate 内失效，真实保证来自 crate 拓扑，文档未澄清 ［架构设计｜中］
- **问题描述**：`views` 在 root、`state/widgets/rich/theme/shortcuts` 在 `rustmark-ui`。ui 内写 `crate::presentation::views` 无法解析，词法禁令（`PRESENTATION_RICH_FORBIDDEN` 等含 `crate::presentation::views`）实际是 no-op；「叶子不得依赖视图编排者」这一 R-13 核心意图，在 views 方向上改由 **Cargo 禁止 ui→root** 兜底。兄弟子层方向（rich→state 等）仍由词法守卫生效。文档 §4.0/§7 仍把 R-13 表述为词法守卫生效。
- **影响分析**：后人可能误以为词法守卫仍在管 views 方向，在回迁子层时产生误判。
- **改进建议**：在 `docs/architecture-constraints.md §4.0` 明确分工——「views 方向由 crate 拓扑（ui 不得反向依赖 root）保证；兄弟子层方向由词法守卫保证；若某子层搬回 core，views 方向词法禁令自动重生」（icons 同款先例已写明，建议统一措辞）。
- **位置**：`tools/arch_check.rs:171-176`；`docs/architecture-constraints.md:119-142`

#### E-09 · 迷你正则引擎无步骤上限 → 灾难性回溯（ReDoS） ［代码质量｜中］
- **问题描述**：`crates/rustmark-core/src/shared/pattern.rs:174` `match_seq` 与 `pattern.rs:235` `match_repeat` 为纯递归回溯，无步数/递归深度预算，无记忆化。形如 `(a+)+$`、`(a*)*b` 对长串 `a`+非匹配字符会在 UI 线程上指数级搜索。该扫描在查找面板键入（`engine.rs:50`）与侧栏搜索（`search.rs:454`）同步执行。
- **影响分析**：用户手输或文档内嵌此类正则时界面冻结。属严重可用性隐患，需对抗性输入才触发。
- **改进建议**：在 `match_repeat`/`match_seq` 加全局步数预算（如 `MAX_STEPS`，超限返回 `None` 并提示「正则过于复杂」）；或在编译期限制嵌套重复深度；UI 侧对正则搜索加超时/取消。补测试：对抗性模式在固定步数内必须返回。
- **位置**：`crates/rustmark-core/src/shared/pattern.rs:174, 235-281`

#### E-10 · 工作区树/移动错误被静默吞掉，FS 与内存树可能不一致 ［代码质量｜中］
- **问题描述**：`crates/rustmark-core/src/application/workspace/service/tree.rs:56` 先 `check_on_clone(...)?` 校验，但真正落库的 `tree.rs:67` `let _ = ws.add_entry(entry)`、`tree.rs:79` `let _ = ws.move_entry(...)` 结果被丢弃。`recent.rs:69` 的 `forget_recent_quietly` 有注释说明是清理动作，可接受。
- **影响分析**：校验路径返回 `?` 而实际变更忽略错误，二者不一致。若真实 `add_entry`/`move_entry` 因边界失败（条目已存在、并发竞态），文件系统已变更但内存树未更新 → 两侧视图不一致，且用户无感知。
- **改进建议**：将 `tree.rs:67/79` 的 `let _ =` 改为 `?` 传播或至少 `log::warn!` 记录；或确认 `check_on_clone` 是否已在克隆上完成真实变更、冗余的二次调用可移除。
- **位置**：`crates/rustmark-core/src/application/workspace/service/tree.rs:56-67, 75-79`

#### E-11 · `load_remote` 每次调用新建 `ureq::Agent`，无连接复用 ［代码质量｜中］
- **问题描述**：`crates/rustmark-core/src/infrastructure/persistence/asset_loader.rs:262` 每次远程下载重建 `ureq::Agent`（根证书经 `OnceLock` 缓存，但 Agent 结构/连接池未复用）。
- **影响分析**：多张远程图 = N 次独立 TLS 握手，无 keep-alive 复用。因在后台线程（`image_cache.rs:191`），不阻塞 UI，但首屏远程图并发下载效率偏低。
- **改进建议**：将 `ureq::Agent` 提为进程级单例（`OnceLock`/lazy），下载时 `clone` 复用。
- **位置**：`crates/rustmark-core/src/infrastructure/persistence/asset_loader.rs:262-274`

#### E-12 · 远程图加载线程无并发上限、通道无界 ［代码质量｜中］
- **问题描述**：`crates/rustmark-ui/src/presentation/views/preview_view/image_cache.rs:191` 对每个不同远程 URL `std::thread::spawn` 一个脱离线程；通道 `remote_channel()` 为 `std::sync::mpsc::channel()`（无界）。
- **影响分析**：含大量不同远程 URL 的文档会同时拉起大量线程、回执无限堆积。
- **改进建议**：引入有界通道 + 受控 worker 池（如信号量限并发数）。
- **位置**：`crates/rustmark-ui/src/presentation/views/preview_view/image_cache.rs:160-198, 206-212`

#### E-13 · 本地/内联图片在主线程同步解码，大图可能引发帧尖刺 ［代码质量｜中］
- **问题描述**：`crates/rustmark-ui/src/presentation/views/preview_view/image_cache.rs:305` 对本地路径与 data URI 在 UI 线程同步读盘+解码；注释称「几十毫秒量级」。
- **影响分析**：接近 `max_local_bytes=8MiB` 上限的大本地图单次解码可能超帧预算，造成可感知卡顿。
- **改进建议**：对超阈值（如 `LARGE_FILE_BYTES`）的本地图也走后台解码，或按尺寸分档离屏解码。
- **位置**：`crates/rustmark-ui/src/presentation/views/preview_view/image_cache.rs:262-327`

#### E-14 · MIME 白名单两处同源仅靠注释约束 ［代码质量｜中］
- **问题描述**：`asset_loader.rs:165` 用 `ALLOWED_MIME` 判定图片，`shared/system.rs:47` `is_safe_url` 另有 URL 安全取向；`asset_loader.rs:15` 注释要求「两处必须同源」，当前靠注释约束，无编译期保证。
- **影响分析**：未来任一处增删 MIME 易漂移，可能放过危险类型或误拦合法类型。
- **改进建议**：把允许的 MIME 集合提为 `shared` 单一常量，两端引用同一来源（含 `svg+xml` 永久排除）。
- **位置**：`crates/rustmark-core/src/infrastructure/persistence/asset_loader.rs:165`、`crates/rustmark-core/src/shared/system.rs:47`

#### E-15 · 阶段文档 03/04/05/08 以 `src/<layer>/` 描述四层 ［文档一致性｜中］
- **问题描述**：四层的「涉及文件/输入/输出」路径全写为 `src/domain`、`src/application`、`src/infrastructure`，实际在 `crates/rustmark-core/src/`。root `src/lib.rs:44` 用 `pub use rustmark_core::{application,domain,infrastructure,shared}` 桥接，故 `crate::` 路径有效但文件不在 `src/`。
- **影响分析**：历史阶段文档的溯源路径失效；docs/README 已声明阶段文档为历史快照，但仍有误导。
- **改进建议**：在 docs/README 或各文档补注「路径以 workspace 拆分后为准（`crates/rustmark-core/src/`）」；技术实现类（10/11）按 E-01/E-02 修订。
- **位置**：`docs/03:3,20,31`；`docs/04:3,19,20,31-99`；`docs/05:3,21,22,33-105`；`docs/08:40,54,68` → `src/lib.rs:44`

#### E-16 · metrics.md 内部体积红线口径矛盾（10 MB vs 11 MB） ［文档一致性｜中］
- **问题描述**：`metrics.md:37` §2 汇总表写「文件总大小（判定口径）| 5–7 MB（红线 10 MB）」，而同文件 NFR-PERF-01 状态行（`:12`）与 `c08fd13` 阈值整改后基线（10,520,576 B）采用「≤ 11 MB（预警 10.5 MB）」，`scripts/size_gate.sh` 默认红线也是 11 MiB。
- **影响分析**：发布工程师按 §2 汇总表会误判当前门禁，可能误报体积超限/未超限。
- **改进建议**：将 §2 汇总行红线改为 11 MB（预警 10.5 MB），并注明「红线于 2026-10-24 由 10 MiB 上调至 11 MiB」。
- **位置**：`metrics.md:12`、`:37`、`core-refactor-tasklist:450`（size_gate.sh 默认 11 MiB / 10.5 MiB 预警）

#### E-17 · docs/README.md 缺失 10 篇近期评审/评估索引 ［文档一致性｜中］
- **问题描述**：`core-structure-review-2026-10-24`、`core-refactor-tasklist-2026-10-24`、`workspace-split-assessment-2026-10-24`、`state-field-audit`、`release-compliance-gap-report`、`threshold-audit/review-2026-10-25`、`shared-internal-cycle-review`、`python-cleanup-report`、`cache-design` 均未在文档地图登记（Grep 均不命中 `docs/README.md`）。
- **影响分析**：读者无法从唯一入口找到关键文档（尤其发布合规差距报告、拆分决策依据）。
- **改进建议**：在 docs/README 增加「近期评审与评估（2026-10-24/25）」小节并列出。
- **位置**：`docs/README.md`

#### E-18 · architecture-refactor-plan.md 旧 `src/presentation/...` 路径未归位 ［文档一致性｜中］
- **问题描述**：改造计划文档大量引用已迁 ui crate 的 `src/presentation/state|theme|shortcuts|rich` 路径（如 `:1139,1280,1285,1422,1453`）。V11-62 已收官，未清理易与现状混淆。
- **影响分析**：作为「改造方案」虽可理解为历史，但未加注易误导。
- **改进建议**：文首加注「本文写于 V11-62 前，所引 `src/presentation/<子层>` 路径现已迁 `crates/rustmark-ui/src/presentation/`」，或批量归位。
- **位置**：`docs/architecture-refactor-plan.md:1139,1280,1285,1422,1453` → `crates/rustmark-ui/src/presentation/`

#### E-19 · 三个新 crate 缺贡献者实操指南 ［文档一致性｜中］
- **问题描述**：ADR-011/012/013 解释「为什么拆」，但「如何新增可持久化动作（须在 `UiAction` 与领域 `ActionId` 同步加、且 `persisted_key` 显式声明，无 `_` 兜底）」「如何新增图标」「何时把模块放 `rustmark-ui`」缺少 `CONTRIBUTING.md` 式串联。
- **影响分析**：贡献者扩展 actions/icons/ui 时易违反契约（如漏同步 `ActionId`、漏显式 `persisted_key`）。
- **改进建议**：在 CONTRIBUTING.md 或 docs/README 增「扩展三个新 crate」checklist，链接 ADR-011/012/013。
- **位置**：ADR-011/012/013 存在且被 docs/README 索引；`rustmark-actions/src/action/impls.rs:106` 要求两侧 `ActionId::all()` 与本表逐项比对

### 🟢 低优先级（15 项）

#### E-20 · `unreachable!` 在表现层查找引擎 ［代码质量｜低］
- **问题描述**：`crates/rustmark-ui/src/presentation/state/find_replace_ui_state/engine.rs:43` `unreachable!("已确认是 Literal 分支")`，`panic="abort"` 下一旦假设被打破即整体退出。
- **影响分析**：理论不可达，但属进程级终止点，违反「非测试代码尽量不 panic」约定。
- **改进建议**：改为 `debug_assert!(matches!(&pattern, Pattern::Literal(_)))`，或返回 `Err`/记日志降级。
- **位置**：`crates/rustmark-ui/src/presentation/state/find_replace_ui_state/engine.rs:43`

#### E-21 · `find_in` 的 `end <= to` 是恒真死代码 ［代码质量｜低］
- **问题描述**：`crates/rustmark-core/src/shared/pattern.rs:140` 条件恒成立（`end = from + start + len` 且 `len` 落在 `slice` 内），无实际作用。
- **改进建议**：删除，或明确改成真正需要的上界语义。
- **位置**：`crates/rustmark-core/src/shared/pattern.rs:140-142`

#### E-22 · `load_local` 先 `fs::metadata` 再 `fs::read` 的 TOCTOU ［代码质量｜低］
- **问题描述**：`crates/rustmark-core/src/infrastructure/persistence/asset_loader.rs:223-238` 两次 IO 之间存在竞态窗口。
- **改进建议**：乐观预检可保留，但注明是乐观判断；read 后已按 `bytes.len()` 统一 `asset.len() > limit` 兜底，风险低，可不处理。
- **位置**：`crates/rustmark-core/src/infrastructure/persistence/asset_loader.rs:223-238`
- **✅ 已收口（2026-10-26，仅注释、零行为变更）**：`load_local` 的 `metadata` 预检处补注「乐观判断」语义——TOCTOU 窗口客观存在，刻意不做「读完再二次 stat」的收紧（多一次 IO 且窗口只是变窄）；真正的上界由 `load()` 拿到字节后的 `asset.len() > limit` 再判一次兜底（内联 data URI 路径同理），竞态最坏只让一次超限分配漏到读后校验，不会放过超限内容。

#### E-23 · 远程图片无跨会话磁盘缓存 ［代码质量｜低］
- **问题描述**：`image_cache.rs` 仅缓存当前 UI 线程 `TextureHandle`（thread_local），每次启动/上下文重建都重新下载远程图。
- **影响分析**：重复打开含远程图的文档每次联网下载，浪费带宽、离线不可用。
- **改进建议**：可选加一层按 URL 的进程级/磁盘级字节缓存（缓存键含 URL 与最终重定向 URL，TTL 或手动清除）。
- **位置**：`crates/rustmark-ui/src/presentation/views/preview_view/image_cache.rs:262-330`
- **⛔ 不做（2026-10-26 决策，理由归档）**：磁盘缓存会把**第三方远程内容长期留在本机**，与本项目「远程图只发一次 GET、不落文档内容」的隐私姿态相反；要做则必须同时给出缓存目录、TTL、容量上限、一键清除与默认关闭开关，属**产品决策**而非缺陷修复（报告本身也标「可选」）。收益也有限：命中只发生在「重复打开同一份含远程图的文档」，而该类文档本就要求联网。
  若将来实现，前置条件是 `PRIV-01`（禁止加载远程图片开关）先落地——两者共用一个「远程资源策略」设置，避免出现「一边禁止加载、一边偷偷缓存」的自相矛盾。现状见 `PRIVACY.md` §2。

#### E-24 · D-4 视图拆分阈值（>600 行须拆）尚未机械校验 ［架构设计｜低］
- **问题描述**：`docs/architecture-constraints.md:419-436` 明示为「评审纪律，当前未机械校验」。现存超阈值 `preview_view/inline.rs`（642 行）留待评审。
- **影响分析**：纪律规则依赖人记，新增超阈值视图 PR 可能漏拆。
- **改进建议**：仿 `CONSTR-03` 的 `mod.rs` 扫描（`tools/arch_check.rs:1498-1520`），在 `scan_project` 内对 `presentation/views/**/*.rs` 做行数断言（排除 `tests.rs`），超 600 行即构建失败并提示拆法。
- **位置**：`docs/architecture-constraints.md:419-436`；`tools/arch_check.rs:1498-1520`

#### E-25 · 组合根 `app/frame.rs` 巨型 god-facade ［架构设计｜低］
- **问题描述**：`src/app/frame.rs` 单文件承载 `EditorApp` 结构体（`:154-395`，~45 字段）、构造函数（`:438-538`）、eframe `ui` 主循环（`:577-668`）、字体装配、IME 处理；虽已拆 25 个子模块，facade 仍偏大。
- **影响分析**：功能增长会持续向 `frame.rs` 与 `EditorApp` 累加，长期降低可导航性；属 egui 即时模式惯用法，非结构性错误。
- **改进建议**：将 `frame.rs` 拆为 `frame/state.rs`（结构体+构造函数）与 `frame/loop.rs`（eframe impl+编排），并保持 facade 可阅读。
- **位置**：`src/app/frame.rs:154-395, 438-538, 577-668`
- **✅ 已收口（2026-10-26）**：拆为 `src/app/frame/state.rs`（`EditorApp` 字段表 + 构造函数 + `StatsPace`/`STATS_DEBOUNCE_MS`，434 行）与 `src/app/frame/loop.rs`（`eframe::App` 实现 + `run` 启动入口 + 字体装配 + IME 窗口常量，258 行）；`frame.rs` 839 → **193 行**，只留导入前奏、子模块声明/再导出与唯一字体适配器。路径零变化：`pub use state::EditorApp;` / `pub use r#loop::run;` 使 `crate::app::frame::EditorApp` 与 `rustmark::app::frame::run` 两条外部路径保持原样（模块名 `loop` 是关键字，用 raw identifier `r#loop`）。
  两个必要连带改动：① **22 个原「模块私有」字段转 `pub(super)`**——同一批兄弟子模块（`editor.rs`/`tabs.rs`/`capture.rs` 等）仍在读写它们，而 `state` 与它们已是兄弟关系；`pub(super)` = `app::frame` 内可见，对外可见面零变化（root `src/` 不在 field-visibility 白名单口径内）。② 修正**三处被打乱的源码文档**：`LineIndexCache` 与 `StatsPace` 的文档里混入了 `EditorApp` 的「为什么 UI 状态与服务平铺」段落，已各自归位（该缺陷早于本次拆分，属顺带修复）。
  门禁：fmt/check-arch/clippy 干净，`--all-targets` **2178 passed / 0 failed / 4 ignored** 与拆分前逐项守恒（纯重构）。

#### E-26 · 测试金字塔倒置——presentation 测试最多，domain 最少 ［架构设计｜低］
- **问题描述**：测试分布：presentation ≈43 文件、app ≈21、infrastructure 14、shared 7、integration 5、e2e ≈15，而核心纯逻辑层 domain 仅 8、application 仅 7。
- **影响分析**：对 egui UI 而言 CPU 布局不变量值得重测，且 domain 逻辑经 app/integration/e2e 间接覆盖，非硬伤；但最该厚的基础逻辑层专属单测偏薄。
- **改进建议**：优先为 domain 增加纯函数单测（markdown 解析边界、undo/redo 合并规则、geometry 字符边界），抬高金字塔基底。
- **位置**：`tests/domain/`（8 文件）、`tests/presentation/`（~43 文件）
- **✅ 已收口（2026-10-26）**：先复核口径——本条按**文件数**判「金字塔倒置」，但按**用例数**测 domain 并不薄：
  `tests/domain` 10 文件 / **194 例**（19.4 例/文件）vs `tests/presentation` 44 文件 / 626 例（14.2 例/文件），
  另有 `domain/undo.rs` 2 例、`domain/markdown/lint.rs` 15 例内联。文件少是因为领域层模块少，不是覆盖稀。
  真实缺口在**纯策略对象**：`UndoPolicy` / `UndoStack` 的合并窗口与容量下限此前只被文档级用例间接覆盖。
  已补 `tests/domain/undo_policy_test.rs` **6 例**：滚动窗口（连续 5 次击键仍只 1 个撤销点——朴素「按首次编辑计时」
  实现会把它切碎）、超窗新开点、窗口端点闭区间、零值容量按 1、字节预算至少保留一份快照、空栈重做是 no-op。
  两处关键契约做了**变异验证**：删掉 `last_at = Some(at)` 的滚动更新 → 滚动用例红；预算保留条件 `> 1` 改 `> 0`
  → 「至少一份」用例红。补测后 `tests/domain` 为 11 文件 / **200 例**（18.2 例/文件）；测试基线 2178 → **2184**（+6）；同时把「按用例数而非文件数」的口径写进
  `docs/metrics.md` §1.1，避免后续再按文件数误判覆盖。

#### E-27 · `shared` 准入三问 + 内部边清单治理负担偏重，且本地开发期存在登记时差 ［架构设计｜低］
- **问题描述**：19 文件三问登记表（`docs/architecture-constraints.md:261-283`）+ 11 条内部边清单（§4.6）均为人工散文；新增 shared 文件须先答三问并登记一行，否则构建失败（`tools/arch_check.rs:1450-1460`）。
- **影响分析**：构建期拦截到位，但若本地仅跑 `cargo test`（未带 `--workspace`）或跳过 `check-arch`，缺口可拖到 CI 才暴露。
- **改进建议**：把「shared 文件数 / 内部边条数」纳入 `docs/metrics.md` 健康度指标；并在 `check-arch` 零违规报告中附「shared 登记完整性 OK」。
- **位置**：`docs/architecture-constraints.md:242-312`；`tools/arch_check.rs:1450-1460`

#### E-28 · constraints §8 早期体积预算「5–7 MB」与实测不符 ［文档一致性｜低］
- **问题描述**：`docs/architecture-constraints.md:448` 预算表「总计 5–7 MB（红线 11 MB）」的估算区间过时（实测 ≈10.5 MB），红线值本身已正确。
- **改进建议**：表下加注「5–7 MB 为早期需求预算，实测见 metrics.md §2.8（当前 ≈10.5 MB）」。
- **位置**：`docs/architecture-constraints.md:448` vs `metrics.md §2.8`

#### E-29 · 根 README.md 无 ADR 索引 ［文档一致性｜低］
- **问题描述**：根 README 仅特性注释提 ADR-004，未给 13 篇 ADR 入口。
- **影响分析**：外部用户/贡献者缺少决策记录入口。
- **改进建议**：根 README 加一行指向 `docs/README.md` 的 ADR 表或简述 ADR-001~013。
- **位置**：`README.md:27,106`（仅 ADR-004）；`docs/README.md:69-81`（全覆盖）

#### E-30 · config-reference 未说明 24 个「仅 UI 不持久化」动作 ［文档一致性｜低］
- **问题描述**：`config-reference.md:156-169` §6.2 列 38 个持久化动作（与代码 `ActionId` 38 变体一致 ✓），未提 `UiAction` 全集 62 个中另有 24 个不进入快捷键持久化（OpenFolder/Revert/InsertImage/ToggleList/GotoTab1-9 等）。
- **影响分析**：读者可能以为 38 即全部动作。
- **改进建议**：补一句「另有 24 个仅 UI/菜单动作不持久化（见 `UiAction` 枚举）」。
- **位置**：`config-reference.md:156-169` vs `rustmark-actions/src/action.rs`（共 62 变体）

#### E-31 · 本地 SVG 防护为「隐式关闭 feature」而非显式白名单 ［文档一致性｜低］
- **问题描述**：PRIVACY/SECURITY 将「不放行 SVG」写为全局硬规则，但本地 `.svg` 不被渲染的真正原因是 `image` crate 的 svg feature 被关闭（`policy.rs:147` 仍映射 svg MIME），`ALLOWED_MIME` 仅用于 data-URI/远程分支。
- **影响分析**：未来若启用 svg feature，会误以为仍有防护，存在脚本执行风险敞口。
- **改进建议**：在 SECURITY/PRIVACY 或 asset_loader 注释说明「本地 svg 因 image crate 未启用 svg 解码而拒，属隐式防护；若开启 svg feature 须辅以显式拒绝」。
- **位置**：`policy.rs:40-47`（ALLOWED_MIME 无 svg）、`:147`（mime_from_extension 含 svg）、root `Cargo.toml` image features 仅 png/jpeg/gif/webp/bmp

#### E-32 · 隐私差距 PRIV-01 未闭环登记 ［文档一致性｜低］
- **问题描述**：PRIVACY.md:40 自承「禁止加载远程图片」开关（PRIV-01）本期未实现。
- **影响分析**：已知隐私控制缺口若不在 `release-compliance-gap-report` 登记跟踪，易随版本遗漏。
- **改进建议**：确认 `release-compliance-gap-report` 已登记 PRIV-01；未登记则补登并标注状态。
- **位置**：`PRIVACY.md:40`

#### E-33 · architecture-review-2026-10-23「10 MB 红线」已过时 ［文档一致性｜低］
- **问题描述**：该复审（2026-10-23）仍写「exe ≤ 10 MB 红线」，早于 10-24/25 红线上调至 11 MiB。
- **影响分析**：当作现状解读会误判体积门禁。
- **改进建议**：在该复审文档加注「体积红线于 2026-10-24 上调至 11 MiB（见 metrics.md §2.8）」，或视为历史快照。
- **位置**：`architecture-review-2026-10-23.md:63` vs `metrics.md §2.8`

#### E-34 · core 规模行数 8 行级差异 ［文档一致性｜低］
- **问题描述**：`docs/architecture-constraints.md:503` 记 core 27,892 行，实测 27,900 行（差 8 行）。
- **影响分析**：可忽略，疑似末行换行/计数口径差异。
- **改进建议**：复跑 `find crates/rustmark-core -name '*.rs' | xargs wc -l` 确认，或接受为统计噪声。
- **位置**：`docs/architecture-constraints.md:503` vs 实测 27,900

---

## 4. 改进路线图建议

### P0 — 立即修（正确性 + 直接误导，成本极低）
| 项 | 动作 | 工作量 |
|---|---|---|
| E-01 | 修复 `pattern.rs:295-302` 大小写区间折叠 `ch` + 补回归测试 | 0.5 天 |
| E-02 | 06/10/11 顶部加 crate 归属说明，路径前缀改为 `crates/rustmark-ui/...` 与 `crates/rustmark-core/...` | 0.5 天 |
| E-03 | config-reference.md 路径前缀改为 `crates/rustmark-core/...` | 0.5 天 |

### P1 — 近期迭代（守卫盲区 + 健壮性/性能隐患）
- **架构**：E-04/E-05 把 `super::` 与 glob 跨层写法纳入 `check-arch` 机械校验；E-06 推进 V11-63 端口化并清空放行清单；E-08 澄清 §4.0 文档分工。
- **代码**：E-09 给正则引擎加 `MAX_STEPS` 护栏；E-10 修复 `tree.rs` 错误吞没；E-11/E-12/E-13 远程图 Agent 复用 + 并发上限 + 大本地图离屏解码；E-14 MIME 单源常量。
- **文档**：E-15/E-16/E-17/E-18/E-19 阶段文档补注、metrics 红线统一、docs/README 补索引、refactor-plan 加注、新 crate 贡献指南。

### P2 — 持续治理（健康度指标 + 纪律机械化）
- E-07 把「字段白名单条目数」纳入 metrics 健康度；E-24 把 D-4 拆分阈值机械校验；E-25 拆分 `frame.rs`；E-26 补 domain 纯函数单测；E-27 把 shared 文件/边计数纳入健康度；E-20~E-23、E-28~E-34 各低优先项随相关改动顺手清理。

---

## 5. 值得肯定的正面项（非问题，供维护者信心参考）
- **分层与机械门禁成熟**：分层、依赖矩阵、`shared` 准入三问、`mod.rs` 禁令、帧预约唯一出口、豁免/登记双表、`shared` 内部无环、workspace 拓扑断言全部机械落地，评估中取证确认**当前零违例**。
- **代码健壮性基线好**：全仓 **0 处 `unsafe`**；生产路径 `unwrap/expect/panic` 基本清零（`anyhow` 严格限组合根）；常量集中（`shared/constants.rs`）；撤销栈每文档私有 + 字节预算兜底；导出 HTML 经 `sanitize_html` 消毒；远程图下载已在后台线程、失败当帧降级。
- **安全/隐私文档与代码吻合**：远程图下载（ureq+rustls、10s 超时、8 MiB 上限、MIME 白名单、不放行 SVG）均有代码支撑；已废弃假设（Tauri/SQLite/IPC）全程显式否决、无残迹；archive 区明确标注历史归档。
- **事件总线耦合受控**：事件只携带 ID+标量，发布者全在 application、订阅者集中注册且无重入死循环。

---

## 附录 A · 三维度完整发现索引
- 架构设计 9 项（F1–F9）：见 §3 E-04/E-05/E-06/E-07/E-08/E-24/E-25/E-26/E-27。
- 代码质量 11 项（含 1 高 bug）：见 §3 E-01/E-09/E-10/E-11/E-12/E-13/E-14/E-20/E-21/E-22/E-23。
- 文档一致性 14 项（Q1–Q14）：见 §3 E-02/E-03/E-15/E-16/E-17/E-18/E-19/E-28/E-29/E-30/E-31/E-32/E-33/E-34。

> 本报告所有结论均附 `文件:行号` 证据，可直接作为后续改进的任务拆分依据。

---

## 6. 整改状态索引（2026-10-26 收口）

> 本报告保持「2026-10-25 评估快照」原貌（问题描述与证据行号均为当时口径），整改结论统一登记在此表。
> 提交号均为本地 `main`；文档批次 `585b3d2` 一次性覆盖 E-02/03/08/15~19/28~34/07/27。

| 编号 | 结论 | 落地 |
|---|---|---|
| E-01 | 已修：`ClassItem::Range` 的 ci 分支折叠 `ch` | `54acd58` |
| E-02 / E-03 | 已修：06/10/11 与 config-reference 路径归位 | `585b3d2` |
| E-04 / E-05 | 已修：`check-arch` 覆盖 `super::` 相对路径与 glob 跨层导入 | `cf4c1fd` |
| E-06 | 已修：端口化两簇，core 零 egui 依赖，棘轮拆除 | `ef7d5c2` + `ffcc0ee` |
| E-07 | 已登记：白名单条目数进 metrics §1.1 健康度观测项 | `585b3d2` |
| E-08 | 已澄清：§4.0 注明 views 方向由 crate 拓扑兜底 | `585b3d2` |
| E-09 | 已修：迷你正则加 `MAX_STEPS` 步数预算 | `54acd58` |
| E-10 | 已修：`tree.rs` 静默丢弃 → `log::warn!` 暴露偏差 | `a1b7ad9` |
| E-11 | 已修：`ureq::Agent` 进程级单例 | `422e3ff` |
| E-12 | 已修：后台线程并发上限 + 有界回执 | `a448939` |
| E-13 | 已修：大本地图后台解码 | `a448939` |
| E-14 | 已修：MIME 白名单提为 `shared` 单源 | `422e3ff` |
| E-15 ~ E-19 | 已修：阶段文档补注、metrics 红线统一、docs/README 索引、refactor-plan 加注、贡献清单 | `585b3d2` |
| E-20 | 已修：`unreachable!` → `match` 合并判定，零 panic 路径 | `a1b7ad9` |
| E-21 | 已修：删除恒真死代码 `end <= to` | `54acd58` |
| E-22 | 已修：预检补注「乐观判断」语义（注释） | `8fa1dab` |
| E-23 | **不做**：磁盘缓存与隐私姿态冲突，属产品决策（见本节上文理由） | — |
| E-24 | 已修：先拆 `inline.rs`，再加 D-4 行数机械校验 | `6b9da65` |
| E-25 | 已修：`frame.rs` 拆 `state.rs` + `loop.rs` | `8fa1dab` |
| E-26 | 已修：补撤销策略 6 例并纠正「金字塔倒置」口径 | `100135d` |
| E-27 | 已登记：shared 文件数/内部边进 metrics §1.1 | `585b3d2` |
| E-28 ~ E-34 | 已修/已核：预算注记、ADR 索引、动作全集口径、SVG 隐式防护说明、PRIV-01 已登记、红线历史注记、行数复核 | `585b3d2`（E-31 的代码注释在 `422e3ff`） |

**34 项处置完毕**：33 项落地、1 项明确不做（E-23）。
