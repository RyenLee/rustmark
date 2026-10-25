# 架构约束（强制）

> 来源：《需求文档 v2.0 整合版》§6、§7；冲突裁决见 `adr/`。
>
> **本文档是架构约束的唯一真源**（原在 `.agent/architecture.md`，2026-10-20 迁入
> `docs/`——原因见 `docs/architecture-refactor-plan.md` §4.1 R-01「实施结果」：
> `.agent/` 整目录在 `.gitignore` 里，真源不入库会导致「拿到代码的人拿不到规则」，
> 且校验器的豁免登记表在 CI 上读不到）。

---

## 0. 已废弃假设（务必先读）

以下假设来自既有 Web 原型，**v1.0 不生效**，实施时不得沿用：

| 废弃假设 | 出处 | 处置 |
|---|---|---|
| 目标态为 Tauri v2（Rust core + WebView） | 原型 `DESIGN-OVERVIEW.md` §1 | ADR-001 否决，改用 egui 原生 |
| SQLite 持久化（`files` / `drafts` / `settings` 表） | 原型 `DESIGN-BACKEND-CONTRACT.md` §3 | 改为本地文件 + TOML（ADR-003） |
| HTTP / IPC 接口契约与前后端映射表 | 原型 `DESIGN-BACKEND-CONTRACT.md` §4、§8 | v1.0 不存在后端进程 |
| store 内部将 `localStorage` 替换为 IPC 调用的对接设计 | 原型 `DESIGN-OVERVIEW.md` §2 | 不适用；原型仅作行为契约（ADR-005） |

---

## 1. 分层

```
Presentation (表现层)   egui 视图、Widget、UI State
Application  (应用层)   Command / Query / AppService / DTO
Domain       (领域层)   聚合、值对象、领域事件、领域服务 trait、仓储 trait
Infrastructure(基础设施) 仓储实现、外部库适配（ropey / pulldown / syntect / fs / rfd）
Shared       (共享内核) error / result / event / ids / geometry
app.rs       (组合根)   唯一允许依赖全部层的地方
```

## 2. 依赖方向

```
presentation ──► application ──► domain ◄── infrastructure
                                    ▲
                                    └── trait 实现反向注入
```

### 2.1 core 的可见性口径（D-3 阶段 4b，2026-10-23 收紧完成）

依赖矩阵管「能不能 import」，可见性管「能不能碰」——两道防线缺一不可。
4b 后 core（`crates/rustmark-core`）的规则：

- **默认 `pub(crate)`**：core 内部协作项不对外暴露；模块级 `pub` 只留给
  shell / root `tests` 有真实读点的接口（消费者视角判定，R-02）；
- **字段级 `pub` 走白名单**：`tests/field_visibility_guard_test.rs` 登记
  548 条已批准 `pub` 字段，**集合相等双向断言**——新增或把 `pub(crate)`
  放宽回 `pub` 必先改清单（增长留痕）；字段收紧/删除后清单同步移除；
- **收紧方向由编译器守门**：shell 读 core 私有项报 E0616 / E0451，无白名单
  也过不了；**放宽方向由白名单守门**：`pub(crate)` → `pub` 编译器沉默，
  靠清单报红。两个方向都有机械执行，不靠 review 把关；
- 新增 core 接口先问：shell / root `tests` 谁读它？无人读则 `pub(crate)`。

## 3. 依赖矩阵（✓ 允许 / ✗ 禁止）

| 从＼到 | shared | domain | application | infrastructure | presentation |
|---|:---:|:---:|:---:|:---:|:---:|
| shared | – | ✗ | ✗ | ✗ | ✗ |
| domain | ✓ | – | ✗ | ✗ | ✗ |
| application | ✓ | ✓ | – | ✗ | ✗ |
| infrastructure | ✓ | ✓ | ✗ | – | ✗ |
| presentation | ✓ | ✗ | ✓ | ✗ | – |
| app.rs | ✓ | ✓ | ✓ | ✓ | ✓ |

## 4. 各层禁止的具体 crate

| 层 | 禁止 |
|---|---|
| domain | `egui`、`eframe`、`rfd`、`syntect`、`pulldown_cmark`、`std::fs`、`std::io`、`anyhow`、`crate::app`、`crate::application`、`crate::infrastructure`、`crate::presentation`、直接打印（`print!` / `println!` / `eprint!` / `eprintln!` / `dbg!`） |
| application | `egui`、`eframe`、`rfd`、`syntect`、`pulldown_cmark`、`std::fs`、`std::io`、`crate::app`、`crate::infrastructure`、`crate::presentation` |
| infrastructure | `crate::app`、`crate::application`、`crate::presentation` |
| presentation | `crate::domain`（含聚合与值对象）、`crate::infrastructure`、`crate::app`（**唯一例外 `crate::app::frame`**，见 4.2） |
| shared | `egui`、`eframe`、`rfd`、`syntect`、`pulldown_cmark`、`std::process`（R-03）、全部业务层（`crate::domain` / `crate::application` / `crate::infrastructure` / `crate::presentation` / `crate::app`） |

允许：domain 可用 `shared`、`ropey`、`regex`、`chrono`、`serde(derive)`、`thiserror`。

`crate::app` 是组合根。下层反向依赖组合根 = 依赖倒置被打破（领域层一旦能拿到
应用状态，聚合就不再是可独立测试的对象），故各层一律禁止。

`shared` 的 `std::process` 禁令见 R-03：端口 trait 与纯逻辑校验可留在 `shared`，
起进程的执行端必须在 `infrastructure`。`tests/` 的规则见第 9 节。

### 4.0 `presentation` 内部依赖方向（R-13）

`presentation` 不是一个均匀的层，内部方向**单向**：

```
views ──► state ──► (domain DTO，经 application)
  │
  └──► theme / widgets / rich
rich / widgets ──► （不得回指 views / state / theme / widgets）
```

| 子层 | 额外禁止 |
|---|---|
| `presentation/state/` | `crate::presentation::views` |
| `presentation/rich/` | `crate::presentation::views` / `state` / `widgets`（theme 已于 V11-62 批次 4 抽去 `rustmark-ui`；rich 本身于批次 5 收官迁入 ui，与 widgets/state/shortcuts/theme 同 crate——禁令改为**词法守门**：rich 内出现 `crate::presentation::views` / `state` / `widgets` 字面即触发，不因同 crate 而放松；`crate::presentation::theme` 词条在 `tools/arch_check.rs` 的 `PRESENTATION_RICH_FORBIDDEN` 保留作防御性守门） |
| `presentation/widgets/` | `crate::presentation::views` |
| `presentation/icons.rs` | `crate::presentation::views`（**V11-58 起休眠**：icons 已抽去独立 crate `rustmark-icons`，core 内暂无文件落此子层。规则与 fixture（`leaf_layers_may_not_depend_on_views`）保留防回归——icons 若搬回 core 即自动生效） |

`views/` 可读 `state/`（那是它存在的理由），故不列在禁止清单里。
四个子层全部**叠加**父层 `presentation` 的禁令（`crate::domain` /
`crate::infrastructure` / `crate::app`），不是替换。

`views/` 与其余子层同样享有 4.2 的 `crate::app::frame` 窄豁免——豁免跟着
子层细化走，窄前缀本身不变。

### 4.1 判定口径（校验器怎么判）

校验器是**词法级**的（不解析 AST，不引入额外依赖）。判定口径即规则的一部分：

1. **词法**：源码 → token 流。行注释、块注释（含嵌套）、字符串字面量、raw string、
   字符字面量**全部剔除**——注释与字符串里出现的路径**不是依赖**。
2. **`use` 展开**：`use` 声明按 use-tree 语法展开成**完整路径**，覆盖
   `pub use` / `pub(crate) use` / `use a::{b, c}` / `use a::{self, b}` / 嵌套组 /
   `use a::b as c`（别名不改变依赖对象）/ `use a::*` / 跨行 / 前导 `::`。
   `extern crate a;` / `extern crate a as e;`（2015 版式，`#[macro_use]` 场景）
   同等对待；`extern "C" { … }` / `extern "C" fn` 不是依赖声明。
3. **限定路径**：非 `use` 位置的 `a::b::C` 同样参与判定——
   `fn f(d: crate::domain::x::Y)` 也是跨层依赖。
4. **匹配**：与禁止清单做**路径段**前缀匹配——`crate::domain` 命中
   `crate::domain::x::Y`，但**不**命中 `crate::domainfoo`。
   `rustmark_core::X` 先**归一为** `crate::X` 再匹配同一张禁令表
   （D-3 阶段 4a：二者是同一条边的两种拼写，root 靠 re-export 拼接；
   不归一的话把违规改写成 `rustmark_core::` 前缀即可绕过全部禁令）。
   报告文案保留源码原拼写，`banned` 仍是表词条。
5. **层判定**：先剥掉末段 `.rs` 再取首段——`src/domain.rs` 属于 domain 层，
   `src/presentation/state.rs` 属于 `presentation/state` 子层。自由位置是
   **白名单**（`src/main.rs` / `src/lib.rs` / `src/bin/**`）；其它顶层名字
   报「未知层」——默认放行等于新目录静默逃过全部层禁令。
   层判定按**扫描根内的相对路径**进行：workspace 化（阶段 4a）后
   `scan_project` 扫 shell `src/` + `crates/rustmark-core/src/` +
   `crates/rustmark-actions/src/`（V11-57，整根 Free）+
   `crates/rustmark-icons/src/`（V11-58，`lib.rs` 自然判 Free）+
   `crates/rustmark-ui/src/`（V11-62 批次 1，`presentation/` 前缀判子层）+
   `tests/` 六个根，core 侧只在**报告**时补 `crates/rustmark-core/src/` 前缀
   （层判定若吃这个前缀，首段 `crates` 会把整个 core 误判成「未知层」）。
6. **子层叠加**：`presentation/state` 同时受父层与子层两组禁令约束（不是替换）。
7. **`presentation` 内部细粒度**（R-13）：第二段为 `state` / `views` / `rich` /
   `widgets` / `icons` 时判对应子层；目录与层根文件（`presentation/state.rs`）
   两种形态由同一套规则覆盖。
8. **`tests/` 全部文件判 `tests` 层**（R-02）：测试是**消费者**不是层，
   `tests/domain/`、`tests/presentation/` 是测试目标的划分，与 `src/` 的层
   毫无关系——按目录名判层会误套禁令。

### 4.2 表现层的唯一例外：`crate::app::frame`

表现层的视图函数普遍以 `&EditorApp` 接收应用状态
（`crate::app::frame::EditorApp`）——这是**被认可的下行通道**：组合根把状态句柄
交给视图，视图读状态、发动作。

若一刀切禁掉 `crate::app`，54 个表现层文件会同时报错。那不是「更严的架构」，
而是**一条没人能遵守的规则**——规则一旦无法遵守，例外就会蔓延成惯例，
最后连真正要拦的反向依赖也一起淹掉。

但**不能**因此放开整个 `crate::app`：表现层只许碰 `app::frame`（状态句柄与
对话框枚举），不许碰 `bootstrap` / `crash_log` / `dialog_runner` /
`startup_timing` 这些**启动与基础设施编排**的东西——那些是组合根的私事，
视图伸手去拿就等于绕过了组合根。

### 4.3 帧预约的唯一出口（T11 / ADR-009，机械守卫）

`request_repaint_after` 在 `src/` 里**只允许**出现在 `app/frame/scheduler.rs`。
定时任务一律「`arm(Deadline, due_ms)` 登记到期点」，由帧末的
`FrameScheduler::flush` 统一换算成一次预约。

理由：egui 是事件驱动重绘，无输入即无帧。调用点自己预约的写法历史上漏过三次
（统计防抖、高亮补算、消息 TTL），每次的表现都是「定时功能完全不生效」，
且**不产生编译错误、常规单测也覆盖不到**。故用文本扫描拦在构建期。

这条规则对**所有**文件生效，**含组合根 `app/`**。组合根豁免的是「层依赖」
（可以依赖全部层），不是「所有规则」——调用点自己预约帧，正是上面那三次的根因。
本条**不设 `arch-allow` 豁免**：给它写豁免会被指出「这条规则没有豁免」。

### 4.4 豁免（`arch-allow`）

行尾注释 `// arch-allow: <对象>` **只豁免该对象**（不是整行放行），
且必须登记在下方豁免表里。三种情况都会报错：

| 情形 | 报错 |
|---|---|
| 豁免未登记 | `arch-allow 豁免未登记` |
| 该行没有依赖它的违规（豁免写错或已过时） | `arch-allow 豁免无效` |
| 豁免对象与实际违规对象不符 | 照常报违规（豁免只认写明的那个对象） |

**窗口形状**（两种 deliberately 不同）：

- **行尾豁免**（`use egui; // arch-allow: egui`）只作用于**本条语句**——
  包括它坐在里面的多行 `use` 组，但**不越过**这条语句；
- **独立成行**的豁免额外作用于**紧邻的下一行**（写在 `use egui;` 上面的那种），
  且只罩紧邻的那一条。

少了这个区分，第 1 行的行尾豁免会把第 2 行一条**独立的**违规一并压掉——
豁免看着写在第 1 行，实际吞了第 2 行。组合根 / 自由文件（无层禁令的位置）里的
`arch-allow` 同样要体检：那里的豁免必然无对象，报「无效」而不是被静默忽略。

<!-- arch-allow:registry -->
| 文件 | 对象 | 理由 |
|---|---|---|
<!-- /arch-allow:registry -->

> 表头行与分隔行不算登记项。文件列写 `src/domain/x.rs` 或 `domain/x.rs` 均可。
> 当前无任何豁免登记——这正是期望状态：豁免是例外，例外应当可见且可回收。

### 4.5 `shared` 准入三问与登记表（D-6，机械守卫）

`shared` 是**共享内核**：依赖矩阵里每一层都允许依赖它，它自己几乎不依赖任何层。
这既是它存在的理由，也是它最危险的地方——「不知道放哪就放 `shared`」会让它
变成绕过依赖矩阵的杂物间。历史先例：`std::process` 曾以「不在禁止清单」为由
落位 `shared/system.rs`（R-03 已把执行端移到 `infrastructure/system.rs`）。

**新文件进 `shared/` 必须同时回答三问**：

| # | 问题 | 判据 |
|---|---|---|
| 1 | 是否至少被 **2 个层**需要？ | 只被一层需要的东西放进那一层；「2 层需要」须能指名是哪两层，且它们按依赖方向确实不能互相引用 |
| 2 | 是否**零 IO**？ | 不含 `std::fs` / `std::io` / `std::process` 等（§4 禁令已机械拦截）；需要 IO 的留**端口**（trait），实现出去 |
| 3 | 是否**无可归属的层**？ | 先尝试归入 domain / application / infrastructure / presentation；都归不进去的（三方共用的值对象、端口契约、唯一真源）才属于这里 |

三问的答案不是口头论证，而是**登记**——下表与 §4.4 豁免表同级，且同样由
`check-arch` 机械核对：`shared/` 下每个 `.rs` 文件必须在表中出现，
**未登记即违规**（`build.rs` 构建期与 CI 同步拦截，报文件级错误）。

<!-- shared:registry -->
| 文件 | 三问理由 |
|---|---|
| `shared/constants.rs` | 跨层阈值常量（大文件 500KB / 防抖 150ms / 滚动容差 0.5pt）：core 的应用层、基础设施层、表现层与 `rustmark-ui`、root 五处消费，改一处漏一处即行为分叉；纯常量零 IO，无业务归属层 |
| `shared/dialog.rs` | 对话框端口：presentation（菜单触发）、application（用例取路径）、infrastructure（rfd 实现）三方共用——矩阵禁 presentation→domain、infrastructure→application，共同落点只有 shared |
| `shared/encoding.rs` | 编码**标签**纯值：infrastructure 探测编码（IO），presentation 状态栏展示，两层方向互禁，抽值共享 |
| `shared/error.rs` | `AppError` 跨层统一错误：domain / application 具体错误的汇聚点，全部层与二进制入口共用 |
| `shared/event.rs` | 事件总线（Shared Kernel）：事件只携带 ID 与标量，订阅者横跨各层，总线契约须被全部层引用 |
| `shared/export_format.rs` | 导出格式与页面尺寸唯一真源（R-07）：domain 校验、presentation 选择、infrastructure 路由三方需要 |
| `shared/fonts.rs` | 编辑区等宽字体契约（哨兵 + 选项）：设置页（presentation）与装配两侧需要；本模块不引入 egui、零 IO |
| `shared/geometry.rs` | 文本坐标值对象（字节偏移 / 区间 / 行列）：domain 与 presentation 共用的纯值，零 IO |
| `shared/ids.rs` | 强类型 ID（Shared Kernel）：四个上下文共用防 `u64` 误传；进程内计数器，零依赖零 IO |
| `shared/image_asset.rs` | 图片导入端口（R-04b）：app 编排与 infrastructure 实现共享同一契约；写盘在实现侧，端口零 IO |
| `shared/lru.rs` | 有界 LRU：高亮 / 拼写 / 渲染 / 图片四处缓存（应用层与表现层）的唯一实现，纯逻辑 |
| `shared/path.rs` | `FilePath` 路径值对象：document / workspace / export / preferences 四上下文共享内核；领域层禁 `PathBuf`，纯规则零 IO |
| `shared/pattern.rs` | 迷你正则引擎：表现层查找面板与应用层跨文件搜索共用（依赖方向单向，两层不能互引），自研控体积 |
| `shared/result.rs` | `AppResult` 结果别名：全应用统一错误结果，`error.rs` 的配套，被各层引用 |
| `shared/system.rs` | 外部打开器**端口**与链接安全校验：app 与 infrastructure 共用契约；执行端已按 R-03 移出，本体零 IO |
| `shared/text_format.rs` | 可打开文本格式白名单唯一出处：拖放分类（app）、对话框过滤（shared）、状态栏标签（presentation）三处共用 |
| `shared/text.rs` | 英文分词规则唯一出处：应用层拼写建议与表现层右键菜单共用，与检查器同源 |
| `shared/theme.rs` | 主题种类唯一真源（R-09）：原散落六处的枚举 / 映射收敛，domain / application / presentation 共用 |
| `shared/time.rs` | `Clock` 可注入时钟与毫秒标量：领域逻辑禁读系统时间（可单测性），各层共用，零依赖 |
<!-- /shared:registry -->

> 新增文件的流程：**先答三问，再加一行**；答不出「≥2 层需要」就放回所属层。
> 表头行与分隔行不算登记项；文件列写 `shared/x.rs`、`src/shared/x.rs` 或
> `crates/rustmark-core/src/shared/x.rs` 均可。

## 5. 限界上下文与聚合根

| 上下文 | 类型 | 聚合根 | 职责 |
|---|---|---|---|
| Document | 核心域 | `Document` | 文本编辑、光标、撤销、查找 |
| Markdown | 核心域 | `RenderedDocument`（值对象聚合） | 解析、高亮、预览 |
| Workspace | 支撑域 | `Workspace` | 文件树、大纲、最近文件 |
| Preferences | 通用域 | `Preferences` | 主题、快捷键、编辑器设置 |
| Export | 支撑域 | `ExportJob` | HTML / PDF 导出 |

上下文映射：Document → Markdown（Customer-Supplier）；Document ↔ Workspace（Shared Kernel，`DocumentId`）；Workspace → Document（ACL）；Preferences → 全部（Published Language，事件）。

## 6. 每单元验证链（提交前必跑）

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
cargo run --bin check-arch
```

`--workspace` **必带**（D-3 阶段 4a workspace 化后）：不带时 cargo 只跑默认成员
（root 包），`crates/rustmark-core` 的检查与单测会**静默缺席**——CI 同口径
（`ci.yml` 的 clippy / check / test / llvm-cov 四处均已加）。

违规注入自检：在 `crates/rustmark-core/src/domain/**` 临时加入 `use egui;`
→ `check-arch` 必须退出码非 0 并打印带 `crates/rustmark-core/src/` 前缀的
文件行号（shell 侧注入同理，路径不带前缀）。

注：`cargo run --bin check-arch` 会先编译，而 `build.rs` 在编译期就做同一遍校验
并在违规时 `panic!`——违规时你看到的往往是 build.rs 的 panic。两处共用同一份
`format_report`，内容一致；想只看 bin 的输出可设 `SKIP_ARCH_CHECK=1` 跳过构建期校验。

校验器自身的用例在 `tests/arch_violation_fixture.rs`。它验的是**形状覆盖**：
每种逃逸形态（`pub use` / 嵌套花括号 / 跨行 / 别名 / 层根文件 / 组合根 /
注释与字符串里的假路径）各有一条注入用例，变异验证确认它们不是恒绿。

## 7. 模块文件树（需求文档 §7.1，禁 `mod.rs`）

D-3 阶段 4a（2026-10-23）起为 workspace 双 crate：**core 装四层与非视图
子模块，shell（root）只留组合根、视图与两个 re-export 拼接点**；
`core → shell` 的反向依赖被 cargo 锁死，`rustmark_core::X` 与 `crate::X`
在校验器里按同一张禁令表判定（§4.1）。

本节是**文件级**模块树；workspace 级的 crate 地图（有哪些 crate、三阶段
演进到哪了）与「新模块该住哪个 crate」的判据见 §10（V11-56）。

```
src/                                    # shell（root crate `rustmark`）
├─ main.rs / lib.rs                     # lib.rs 拼接点：pub use rustmark_core::{application, domain, infrastructure, shared}
├─ app.rs / app/{bootstrap.rs,frame.rs,crash_log.rs,…}
│                                       # 组合根；帧预约唯一出口 app/frame/scheduler.rs
├─ presentation.rs                      # 拼接点：pub mod views（唯一留 shell 的 presentation 子层）
│                                       #   + pub use rustmark_ui::presentation::{rich, shortcuts, state, theme, widgets}（V11-62 批次 1-5 收官）
│                                       #   + pub use rustmark_icons as icons（V11-58）
│                                       #   （core 的 presentation 已全迁 ui，退化为 doc-only 空壳）
├─ presentation/views/                  # 视图（唯一留在 shell 的 presentation 子层）
└─ bin/check-arch.rs                    # 工具二进制

crates/rustmark-core/src/               # core（crate `rustmark-core`）
├─ lib.rs / {shared,domain,application,infrastructure,presentation}.rs
├─ shared/{error.rs,result.rs,event.rs,ids.rs,geometry.rs}
├─ domain/{document,markdown,workspace,preferences,export}/
│    └─ 各含 {aggregate.rs,value_objects.rs,events.rs,services.rs,repository.rs,errors.rs}
├─ application/{document,markdown,workspace,preferences,export}/ + dto/
├─ infrastructure/{persistence,text,parsing,highlighting,export,events,dialogs,…}/
└─ presentation/                          # 无 views；rich（V11-62 批次 5 抽去 ui）、theme（批次 4）、state（批次 2）、shortcuts（批次 3）、widgets（批次 1）与 icons（V11-58）已抽出；core 的 presentation 退化为 doc-only 空壳

crates/rustmark-actions/src/            # actions（crate `rustmark-actions`，V11-57）
└─ lib.rs / action.rs / action/{impls.rs,menu_group.rs}   # 零依赖词汇表，整根判 Free

crates/rustmark-icons/src/              # icons（crate `rustmark-icons`，V11-58）
└─ lib.rs                               # 16×16 图标规格 + Painter 渲染器；依赖 egui + rustmark-actions

crates/rustmark-ui/src/                 # ui（crate `rustmark-ui`，V11-62 批次 1-5 收官）
└─ lib.rs / presentation.rs / presentation/{widgets,state,shortcuts,theme,rich}/  # widgets（批次 1）、state（批次 2）、shortcuts（批次 3）、theme（批次 4）、rich（批次 5）；依赖 egui + rustmark-core
```

白名单外的顶层位置（shell `src/foo.rs`、core `src/utils/` 等）会被校验器报
「未知层」：新目录要么归入既有层，要么先改本文档与校验器再放代码。
shell `src/bin/` 是工具二进制（`check-arch`），与 `main.rs` / `lib.rs` 一样
不受层级限制。

`crates/rustmark-actions/src/`（V11-57）整个根判 Free：零依赖词汇契约
crate 没有层内边界可言。「不得引用 core / 任何 crate」由 Cargo 循环依赖
（core 已依赖它，反向加普通依赖即解析期报错）与 workspace 拓扑断言
（`docs/workspace-topology.expected.txt`，任何新依赖边都红）兜底——两者
都比词法规则强，且覆盖「获得任意依赖」而不只「引用 core」一种形状。
层禁令不适用于该根，但禁 `mod.rs`、帧预约唯一出口等**与层无关**的规则
照旧生效（fixture 第十六节钉住）。

`crates/rustmark-icons/src/`（V11-58）**不设**强制层：单文件 `lib.rs` 落在
crate 入口白名单里，`layer_of` 自然判 Free（依赖面 = egui +
rustmark-actions，由 `tests/workspace_topology_test.rs` 断言）。禁
`mod.rs`、帧预约唯一出口等与层无关的规则照旧生效；将来若长出第二个文件，
「未知层」警报会逼着给它定层——缺口要响，不静。

**禁 `mod.rs` 已机械校验**（`CONSTR-03`，2026-10-22 起）：`scan_project`
在 shell `src/`、`crates/rustmark-core/src/`、`crates/rustmark-actions/src/`、
`crates/rustmark-icons/src/`（V11-58）与 `tests/` 五侧逐个文件判文件名，出现 `mod.rs` 即报违规，且**不设
`arch-allow` 豁免**。正确形状是「`foo.rs` 与 `foo/bar.rs` 共存」——两者对编译器语义等价，但对人是
「模块树 = 文件树」与「模块树藏在 `mod.rs` 里」的区别，
而这恰恰是评审最容易漏、编译器又从不报错的一类偏差。

**视图文件拆分阈值（D-4 规则化，2026-10-24）**：`presentation/views/` 下
单文件 **> 600 行须拆子目录**——保留 `foo.rs` 为入口，子模块平铺进 `foo/bar.rs`
（与既有先例 `sidebar_view.rs` + `sidebar_view/`、`settings_view.rs` +
`settings_view/` 同形，遵守禁 `mod.rs` 规则）。依据：同目录已按此标准拆过的
模块评审定位成本显著更低。**TD-02 已闭环（2026-10-25，`999e63c`）**：`preview_view.rs`
（原 1,421 行）按本标准拆为入口 `preview_view.rs` 458 行 +
`preview_view/image_cache.rs`（图片纹理缓存簇 319 行）+
`preview_view/tests.rs`（外置单测 664 行，`app/frame/tests.rs` 同款先例），
公开路径 `preview_view::{show, image_cache_state, is_remote_url, …}` 经
`pub use` 重导出零变化。**D-4 第二例已闭环（2026-10-25，`22d4550`）**：原 695 行 → 入口
`toolbar_view.rs` **211 行**（`show` / 面包屑 / 侧栏切换）+ `toolbar_view/tab_strip.rs`
**498 行**（`draw_tab_strip` 及其硬规则 doc、15 个标签条常量、`paint_nav_arrow`；`draw_tab_strip`
转 `pub(super)` 供 facade 调用——子模块私有项对父不可见，E0603 实证）。簇内零 `pub` 项，公开路径
零变化（外部仅 `tab_strip_test.rs` 注释按值钉常量）。现存超阈值文件：`preview_view/inline.rs`
（642 行，已拆目录内的子模块，行内排版为内聚单元，是否再拆留待评审）与 `preview_view/tests.rs`
（664 行，`#[cfg(test)]` 外置单测——不进 release、评审定位成本低，不列为拆分候选）。
本条为**评审纪律**（当前未机械校验，见 architecture-review D-4），
评审超阈值视图 PR 时对照执行。

## 8. 体积与性能预算（改动前自查）

| 组件 | 预算 |
|---|---|
| egui + eframe | 3–4 MB |
| pulldown-cmark | ≤ 0.5 MB |
| syntect | ≤ 1.5 MB |
| ropey | ≤ 0.2 MB |
| rfd + serde 等 | 0.5 MB |
| 应用代码 | 0.5–1 MB |
| **总计** | **5–7 MB（红线 11 MB）** |

新增依赖必须登记体积贡献（`docs/dependencies.md`）并经 ADR 追加评审。

## 9. 集成测试的库内部视角禁令（R-02）

`tests/` 下的全部 `.rs` 文件纳入扫描（`scan_project` 的 tests 根），判 `tests` 层。

测试是**消费者**，不是层：它可以引用任何层的公开 API
（`rustmark::domain::…`、`rustmark::presentation::…`），`tests/app/` 下的
跨层用例天然要同时看见四层。

但**不许**以 `crate::<层名>::…` 的库内部视角书写：

```
✗ use crate::domain::document::aggregate::Document;   // 库内部视角
✓ use rustmark::domain::document::aggregate::Document; // 公开 API
```

`crate::` 在集成测试里指向**测试 crate 自身**，写 `crate::domain` 说明作者的
心智模型是「我在 `src/` 里」——那正是「绕过公开 API 偷看内部」的起点。
这类写法编译就过不了，所以它不是「编译能过的偷看」，而是**从 `src/` 复制
粘贴过来时**原样带进来的坏味道。

放行：`crate::support` / `crate::golden_fixture` 等**测试自身**的共享模块
（集成测试的正常组织方式）。禁令只列库层名，天然放行它们。

报告里 `tests/` 侧文件带 `tests/` 前缀，与 `src/presentation/…` 等同名路径
可区分。与层无关的通用规则（帧预约唯一出口，见 4.3）照样罩住测试侧。

## 10. crate 地图与模块归属判据（V11-56，2026-10-24）

> 本章回答两个问题：**workspace 里应该有哪些 crate**（地图 + 三阶段演进，
> §10.1），以及**一个模块该住哪里**（四问判据 + 外移触发条件，§10.2/§10.3）。
> 判据来源为 `docs/core-structure-review-2026-10-24.md`（评审三大病灶的
> 规则化收口）；决策记录见
> `docs/adr/ADR-013-core-adapter-residency-and-leaf-extraction.md`。
> §1–§9 的分层与禁令对**所有** crate 一视同仁——新 crate 不是法外之地：
> 层判定口径见 §4.1，自由层（Free）的来龙去脉见 §7。

### 10.1 crate 地图

**当前拓扑**（2026-10-24 实测，`cargo tree --workspace --depth 1`；本地依赖边共 6 条）：

```
rustmark（root：组合根 + views + bin）
├──► rustmark-core  ──► rustmark-actions
├──► rustmark-icons ──► rustmark-actions
└──► rustmark-ui    ──► rustmark-core ──► rustmark-actions
                        （三条本地链在 rustmark-actions 汇合；actions 零依赖）
```

| crate | 角色 | 实测规模（2026-10-24，`src/` 口径） | 外部依赖面 |
|---|---|---|---|
| `rustmark`（root） | 组合根 + 视图层 + 工具 bin | 85 文件 / 21,188 行 | eframe / egui / image / rfd / anyhow / env_logger / log / wgpu(build) + 3 个本地 crate |
| `rustmark-core` | 四层脊柱；表现层非视图子模块已全部于 V11-62 批次 1-5 抽去 `rustmark-ui`（core 的 `presentation` 退化为 doc-only 空壳） | 170 文件 / 27,892 行 | 13 个直接外部依赖（`cargo tree -p rustmark-core`）+ 1 个本地（actions） |
| `rustmark-actions` | 动作词汇表契约（V11-57） | 4 文件 / 687 行 | **零依赖** |
| `rustmark-icons` | 16×16 图标规格 + Painter 渲染器（V11-58） | 1 文件 / 613 行 | egui + rustmark-actions |
| `rustmark-ui` | 表现层非视图部分：widgets（批次 1）/ state（批次 2）/ shortcuts（批次 3）/ theme（批次 4）/ rich（批次 5，V11-62 收官） | 73 文件 / 17,651 行 | egui + rustmark-core |

规模口径：`find <源码根> -name '*.rs' | xargs wc -l`，只含 `src/`、不含
`tests/`，命令随文可复跑。core 与评审快照（46,375 行 / 238 文件）的差额 =
V11-57/58/62 拆出的文件（actions 3 + icons 1 + ui 的 widgets 3 / state 24 /
shortcuts 5，另 2 个为 ui 的新建骨架 lib.rs / presentation.rs）扣除期间
core 内的模块清单与文档增补；快照 238 与 git 树基线 `662536b` 的 244 文件
之间存在未逐文件核对的口径差（评审当时工作区含 D-6/D-7 在途改动），
**本节数字以今日实测为准**（2026-10-25：core 170 / 27,892，ui 73 / 17,651，
root 85 / 21,188——TD-02 拆分 `preview_view.rs` 后 +2 文件 / +20 行；
`c08fd13` 阈值整改在 core / ui / root 分别 +171 / +3 / +101 行（撤销字节
预算、纹理缓存字节预算等）；`toolbar_view.rs` D-4 拆分再 +1 文件 / +14 行；
rich 于批次 5 从 core 迁 ui，V11-62 收官）。

**三阶段演进**（照评审报告 §4；阶段 1 已由 V11-57 / V11-58 落地，阶段 2/3 为计划值）：

```
阶段 0（评审时）            阶段 1（P1 首拆，已落地）    阶段 2（触发后）           阶段 3（真实非 UI 消费者出现后）
 rustmark                   rustmark                     rustmark                   rustmark（组合根 + views + bin）
 ├─ app/            7.0k    ├─ app/            7.0k      ├─ app/          7.0k      ├─ app/                    7.0k
 ├─ presentation/  13.8k    ├─ presentation/  13.8k      ├─ presentation/views 13.8k ├─ presentation/views      13.8k
 └─ rustmark-core  46.4k    ├─ rustmark-icons  0.6k ─┐   ├─ rustmark-actions 0.7k   ├─ rustmark-actions         0.7k
    ├─ shared       3.0k    ├─ rustmark-actions 0.7k ─┤  ├─ rustmark-ui     18.1k   ├─ rustmark-ui             18.1k
    ├─ domain       7.2k    ├─ rustmark-ui      0.6k ─┤  │  └─ state/theme/rich/    └─ rustmark-core          16.7k（去 egui 化）
    ├─ application  6.6k    └─ rustmark-core   44.7k     │     shortcuts/widgets    ├─ rustmark-infra          10.8k
    ├─ infrastructure 10.8k    ├─ shared     3.0k        └─ rustmark-core   27.6k   └─（core 只留 shared/domain/
    └─ presentation 18.8k      ├─ domain     7.2k           ├─ shared / domain /       application 脊柱）
       ├─ state    6.7k        ├─ application 6.6k          │  application
       ├─ rich     5.9k        ├─ infrastructure 10.8k      └─ infrastructure 10.8k
       ├─ theme    3.5k        └─ presentation 17.4k           （fonts/viewport_capture 端口化后
       ├─ shortcuts 1.5k          ├─ state / theme / rich          可再拆 rustmark-infra）
       ├─ widgets  0.5k           │  / shortcuts
       └─ icons    0.6k           └─（icons / widgets 已拆出）
                          方向：icons→actions；ui→core；root→{ui,core,icons,actions}
```

（阶段 0/2/3 行数为评审时实测的 k 口径；阶段 1 的 core 45.3k 与上表
45,280 行同源。阶段 2/3 落地时以实测刷新本图。）

**2026-10-25 更新（V11-62 批次 1–5 收官）**：`rustmark-ui` 实测
**73 文件 / 17,651 行**（widgets 0.6k + state 6.7k + shortcuts 0.9k + theme 3.5k + rich 5.9k，
另含 `c08fd13` 阈值整改 +3 行），属演进图阶段 2「触发后」列的部分落地；
`rustmark-core` 为 170 / 27,892（表现层非视图子模块全迁 ui，`presentation`
退化为 doc-only 空壳；`c08fd13` 另 +1 文件 / +171 行）。阶段 2 列 ui 的
18.1k 计划值（含 theme/rich）至此全落。

方向规则不变的三条硬约束：

1. **root→core 方向规则保留**：组合根可依赖全部 crate，下层不得反向依赖
   root——由 Cargo 结构性锁死（core 已依赖 actions/icons，反向加普通依赖即
   解析期报错）；
2. **views 必须留 root**（ADR-013 决策 3）：presentation 外移采用「views 留
   root 的中间 crate」形态，否则 R-11/R-12 的「views ∈ root、state ∈ 另一
   crate」跨 crate 字段保护会被降级成同 crate 内约束；
3. **每拆一个 crate，三件套同步**：校验器扫描根 + `layer_of` 归一化 + 本地
   依赖边期望（`docs/workspace-topology.expected.txt`）——照 actions/icons
   两例（§7）。漏任一件，新 crate 要么被误报「未知层」，要么依赖边静默增长。

### 10.2 模块归属判据（四问 + 两条强信号）

新模块放哪、旧模块动不动，一律先过四问（照评审报告 §3.1）：

| # | 维度 | 口径（怎么量） | 判定规则 |
|---|---|---|---|
| 1 | 依赖方向 | core 内消费者数 + root 引用数 | 被下层依赖 = 核心；只被 root 消费 = 可外移 |
| 2 | 变更频率 | 12mo 提交数（新旧路径合并计数，`tools/onboard_cost.py`） | 高频 + 单向消费 = 外移优先（改它不该牵连领域） |
| 3 | 复用范围 | 消费者是否跨 crate / 外部复用计划 | 纯资源（数据 / 绘制 / 词汇）= 独立 crate 候选 |
| 4 | IO / 框架耦合 | 外部 crate 引用文件数 | egui 只应出现在表现适配器；FS / 网络 / TLS 只应在 infrastructure；domain 必须为零 |

两条本项目实测强信号（单独任一命中也值得启动评估）：

- **零内部消费者（叶层信号）**：在 core 内除自身子模块外零消费者、只被
  root 消费或装配——presentation 与 infrastructure 均满足（评审实测）；
- **state 零 serde**：`presentation/state` 24 文件无一处 serde——UI 状态不
  持久化（持久化走 `domain/preferences`），「state 必须在 core 才能被持久化」
  的理由不成立（V11-59 审计 K=201 / V=0 / A=0 复核确认，
  `docs/state-field-audit.md`）。

**抽查示例**（本章验收方式：任取一模块按上表能复现评审结论）：以 `icons`
（V11-58 已落地）走一遍——① core 内消费者 0、root 引用 3 个 view 文件 →
可外移；② 12mo 提交 ≈2 → 不阻挠；③ 纯资源（16×16 规格数据 + Painter
绘制）→ 独立 crate 候选；④ 只引 egui + actions、无 FS / 网络 → 与「独立
资源 crate」相容。四问一致指向「拆出」，与落地事实一致。

反例同样可复现：`shortcuts` 子模块内部，动作词汇表三文件（action /
impls / menu_group，651 行）只被 root 侧消费 → 拆出（V11-57）；而
combo / keys / palette（402 行）被 core 内的 command_palette 消费 → 留
core。同一子模块内两种结论，判据的粒度是**模块**，不是目录。

### 10.3 外移触发条件（量化）

触发式任务不做无据评估——任一条件满足即启动评估，度量方式随文；
「当前状态」列随每次度量刷新，数字以 `docs/metrics.md` §7（接入成本基线）
为家，本表只放判定结论。

| 动作 | 触发条件（任一满足即启动评估） | 度量方式 | 当前状态（2026-10-24） |
|---|---|---|---|
| presentation 非视图子模块 → `rustmark-ui`（V11-62） | ① UI 并行开发人数 ≥2；② core 增量编译 >10s；③ presentation 年变更 ≥80 提交**且**「views↔core state 跨 crate 摩擦」在 ≥3 个功能中复现 | `tools/onboard_cost.py` + `git log` 去重 | ② 未触发（增量 5.92s）。③ **提交数已去重实测**——表现层整体 **88**、含 `tests/presentation` **93**、仅待搬迁的非视图部分 **51**（阈值 80：整体口径满足、待搬迁部分不满足，口径差异如实保留）；**摩擦已取证：8 个功能复现**（state 字段白名单 169/548 条 + 22 个 `pub` setter + 桥接 6 行 / 21 个 view 文件经桥接访问）。**③ 判定：满足**（证据见 tasklist V11-62 节）。① 仍待维护者提供。**已开工**：动作 2 批次 1 已落地（widgets 538 行 → `rustmark-ui`，本地依赖边 4→6，门禁 2144 / release exe 10,485,248 B 与基线逐字节相同）；批次 2–5 待续（theme → shortcuts → state → rich） |
| infrastructure → `rustmark-infra`（V11-63/64） | 出现真实非 UI 消费者（CLI / headless / 嵌入）**且**去 egui 化完成 | 需求事实 + `cargo tree -p rustmark-core \| grep egui` 为 0 | 未触发（无 headless 消费者；core 现引 egui。2026-10-25 复核仍成立：`grep -c egui` = 1；「egui 归零」是 V11-63 的验收而非触发门，触发门 = 首条需求事实，评估记录见 tasklist V11-64 节后） |
| math / mermaid → `rustmark-render` | mermaid/math 年变更 ≥10 提交或合计 >8k 行（现 5.6k） | git log + wc -l | 未触发 |
| 新 crate 合并回退（防过度拆分） | 新建 crate <1k 行且 12mo 变更 <5 提交 | git log | 不适用：actions（687 行）/ icons（613 行）均 <1k 行，但属零依赖契约 / 纯资源 crate，回退等于把契约塞回 core——判据本意防「为拆而拆的业务模块」（ADR-013 决策 5） |
| shared 通用件再拆分 | pattern+lru+text 合计 >1,500 行或出现第 4 个非领域通用模块 | wc -l | 未触发 |
| state 瘦身叫停 | 审计发现纯帧内字段 <10%（收益不抵改动风险） | 字段清单 | **已触发并闭环**（V11-59 审计纯帧内字段远低于阈值 → V11-60 关闭，结论见 `docs/state-field-audit.md`） |
