# 架构约束（强制）

> 来源：《需求文档 v2.0 整合版》§6、§7；冲突裁决见 `adr/`。
>
> **本文档是架构约束的唯一真源**（原在 `.agent/architecture.md`，2026-10-20 迁入
> `docs/`——原因见 `docs/archive/architecture-reviews/architecture-refactor-plan.md` §4.1 R-01「实施结果」：
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

**`shared` 定位声明（V11-55 附案，2026-10-25 落档）**：被跨层复用的**纯函数 /
纯数据结构**模块合法居 `shared`。这不是「不知道放哪就放 `shared`」的宽容，
而是矩阵的设计后果——同时被两个方向互禁的层需要的东西，共同落点只有
`shared`（`dialog` / `encoding` / `export_format` 同款理由，见 §4.5 登记表）。
现存三例，使用点均为 2026-10-25 实测（`src` 生产代码，测试消费者按 R-02 不计层）：

| 模块 | 使用点分布 | 下沉为何不可行 |
|---|---|---|
| `text_format` | app 2 / presentation 1 / shared 1（`dialog` 的过滤器清单派生） | 去 `app`：presentation 侧触发 §4 `crate::app` 禁令（4.2 窄豁免只罩 `frame`）；去 `application`：`shared/dialog.rs` 触发矩阵 shared 行全 ✗ |
| `lru` | application 3 / presentation 1 | 落任一层都让另一层违例；且它是四处缓存的唯一实现 |
| `text` | application 2 / presentation 1 | 同 `lru`：应用层与表现层共用的分词规则 |

三例**不搬移**——「为什么不归位」在此写成规则，避免后人重复提案
（V11-55 §2.2 的下沉方案即被该实测证伪；档案与复现命令见
`docs/reports/shared-internal-cycle-review.md`）。「至少 2 层需要」正是 §4.5 准入
第一问的**正面情形**，不是擦边球。新模块进 `shared` 先过 §4.5 三问并登记；
`shared` 内部依赖必须无环（§4.6）。

## 4. 各层禁止的具体 crate

| 层 | 禁止 |
|---|---|
| domain | `egui`、`eframe`、`rfd`、`syntect`、`pulldown_cmark`、`std::fs`、`std::io`、`anyhow`、`crate::app`、`crate::application`、`crate::infrastructure`、`crate::presentation`、直接打印（`print!` / `println!` / `eprint!` / `eprintln!` / `dbg!`） |
| application | `egui`、`eframe`、`rfd`、`syntect`、`pulldown_cmark`、`std::fs`、`std::io`、`crate::app`、`crate::infrastructure`、`crate::presentation` |
| infrastructure | `crate::app`、`crate::application`、`crate::presentation`、`egui`（V11-63 端口化收口后**无条件生效**，无文件级豁免） |
| presentation | `crate::domain`（含聚合与值对象）、`crate::infrastructure`、`crate::app`（**唯一例外 `crate::app::frame`**，见 4.2；**`views/` 子层不在此例外内**，R-03 B1 收官时撤回） |
| shared | `egui`、`eframe`、`rfd`、`syntect`、`pulldown_cmark`、`std::process`（R-03）、全部业务层（`crate::domain` / `crate::application` / `crate::infrastructure` / `crate::presentation` / `crate::app`） |

允许：domain 可用 `shared`、`ropey`、`regex`、`chrono`、`serde(derive)`、`thiserror`。

`crate::app` 是组合根。下层反向依赖组合根 = 依赖倒置被打破（领域层一旦能拿到
应用状态，聚合就不再是可独立测试的对象），故各层一律禁止。

`shared` 的 `std::process` 禁令见 R-03：端口 trait 与纯逻辑校验可留在 `shared`，
起进程的执行端必须在 `infrastructure`。`tests/` 的规则见第 9 节。

infrastructure 的 `egui` 禁令（V11-63，2026-10-26 收口）：依据是 §10.2
四问之④「egui 只应出现在表现适配器」——infrastructure 产字节与端口类型，
直接产 `egui::FontDefinitions` / `ColorImage` 意味着「core 可 headless 复用」
不成立。

落地分两步。**棘轮期**（2026-10-25）：存量泄漏 5 文件
（`infrastructure/fonts.rs`、`fonts/discovery.rs`、`fonts/assembly.rs`、
`export/viewport_capture.rs`、`viewport_capture/image.rs`，即 V11-63 任务清单
全集）以校验器内的文件级放行清单 `V11_63_KNOWN_EGUI_LEAKS` grandfather，
清单外文件（含同目录新文件）新增引用一律拦截——渗漏只能缩小不能扩大。
放行清单不放 §4.4 豁免登记表：那张表的语义是行内豁免注释的治理门槛，
文件级放行与规则同址置于校验器（`allowed_for` 的 `crate::app::frame`
窄豁免同款先例）。**端口化期**（2026-10-26）：阶段 A 以 `PortFrame`（RGBA8
字节）替掉 `ColorImage`，阶段 B 以 `PortFontDefinitions` / `PortFontFamily` /
`PortFontData` 替掉 `egui::FontDefinitions` 一族，egui 类型只在组合根
`src/app/frame.rs` 的适配器（`to_egui_font_definitions`）里出现。

**清单与放行分支已随之删除，禁令现为无条件生效**——校验器里已无
`V11_63_KNOWN_EGUI_LEAKS`，任何 infrastructure 文件引 egui 都报。验收用例
`tests/arch_violation_fixture.rs::former_v11_63_leak_paths_are_now_caught`
把那 5 个历史路径逐个钉成「必报」，防止清单以任何形式复活。

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
| `presentation/rich/` | **已升 crate（R-04，2026-10-28，`22e8a86`）**：`rich/{math,mermaid}` 物理搬出为独立 crate `rustmark-render`，`rustmark-ui` 经 `pub mod rich { pub use rustmark_render::*; }` shim 拼回原路径（消费方零改动）。子层禁令 `PRESENTATION_RICH_FORBIDDEN`（禁 `crate::presentation::views/state/widgets`，theme 词条防御性保留）现对真实 workspace **休眠**——已无 `presentation/rich` 目录，跨 crate 引用本就不可达，仅在 `arch_check` 合成夹具中保留防回归；egui 隔离另由 `check_rich_egui_isolation` 扫 `rustmark-render/src`（白名单仅 `render/egui.rs`）覆盖 |
| `presentation/widgets/` | `crate::presentation::views` |
| `presentation/icons.rs` | `crate::presentation::views`（**V11-58 起休眠**：icons 已抽去独立 crate `rustmark-icons`，core 内暂无文件落此子层。规则与 fixture（`leaf_layers_may_not_depend_on_views`）保留防回归——icons 若搬回 core 即自动生效） |

`views/` 可读 `state/`（那是它存在的理由），故不列在禁止清单里。
四个子层全部**叠加**父层 `presentation` 的禁令（`crate::domain` /
`crate::infrastructure` / `crate::app`），不是替换。

~~`views/` 与其余子层同样享有 4.2 的 `crate::app::frame` 窄豁免——豁免跟着
子层细化走，窄前缀本身不变。~~

> **R-03 B1 收官时撤回 `views/` 的这一条（2026-10-27）**：`views/` 现在**不**
> 享有 `crate::app::frame` 窄豁免，其余表现层子层照旧。理由是当初给它豁免的
> 条件已经兑现——`src/presentation/views/**` 现在零 `crate::app` 引用（trait
> 133 方法全部经 `EditorViewContext`）。留下的豁免只剩坏处：漏写一条 `use` 到
> `crate::app::frame` 不会有任何提示，规则从「不该这么做」退化成「随你」。
> 这是**兑现**，不是规则反悔，见 §10.1 硬约束 5。另四个子层
> （`state/theme/shortcuts/rich/widgets`）已迁入 `rustmark-ui` crate，
> 反向依赖在物理上写不出来，故其豁免保留不动（删了只是文档噪音）。

> **分工澄清（E-08，2026-10-26）**：`views` 在 root（`src/presentation/views/`）、
> `state/theme/shortcuts/rich/widgets` 在 `rustmark-ui` crate。因此 ui 内写
> `crate::presentation::views` **无法解析**，本表里的 `crate::presentation::views` 词条在 ui crate
> 内实为 **no-op**；「叶子不得依赖视图编排者」这一 R-13 核心意图，在 *views 方向* 上改由
> **Cargo 禁止 `rustmark-ui → root`** 兜底（crate 拓扑保证）。*兄弟子层方向*（如 `rich → state`）
> 仍由本表词法守卫生效。若某子层将来搬回 core，`views` 方向词法禁令会**自动重生**
> （`icons` 同款先例：V11-58 抽去 `rustmark-icons` 后 `icons.rs` 词条休眠，搬回即生效）。结论：
> 不要误以为词法守卫仍在管 views 方向——回迁子层前务必确认 crate 拓扑约束仍在。

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
   `crates/rustmark-shared/src/`（TD-04，`shared/` 前缀判 Shared）+
   `crates/rustmark-infra/src/`（V11-64，`infrastructure/` 前缀判 Infrastructure）+
   `tests/` **八个根**，core 侧只在**报告**时补 `crates/rustmark-core/src/` 前缀
   （层判定若吃这个前缀，首段 `crates` 会把整个 core 误判成「未知层」）。
6. **子层叠加**：`presentation/state` 同时受父层与子层两组禁令约束（不是替换）。
7. **`presentation` 内部细粒度**（R-13）：第二段为 `state` / `views` / `rich` /
   `widgets` / `icons` 时判对应子层；目录与层根文件（`presentation/state.rs`）
   两种形态由同一套规则覆盖。
8. **`tests/` 全部文件判 `tests` 层**（R-02）：测试是**消费者**不是层，
   `tests/domain/`、`tests/presentation/` 是测试目标的划分，与 `src/` 的层
   毫无关系——按目录名判层会误套禁令。
9. **相对路径先归一再判**（2026-10-25 补齐）：`super::…` / `self::…` 开头的
   `use` 与限定路径，先按「文件→模块路径」解析成绝对路径，再套同一张禁令表。
   `super::super::super::infrastructure::x` 与 `crate::infrastructure::x`
   是同一条边，只认后者等于给禁令留了一扇侧门。

   之所以能精确算出 `super` 跳几层：本工程**禁 `mod.rs`**（§7），于是
   「文件路径 → 模块路径」一一对应（`domain/document/aggregate.rs` 就是
   `crate::domain::document::aggregate`）；有 `mod.rs` 就得先把 `mod x;`
   声明表建起来，那是词法器看不见的东西。

   内联模块（`#[cfg(test)] mod tests { use super::…; }`）里的 `super` 相对的
   是**那个内联模块**，比文件本身深。词法器不知自己在第几层，故按文件深度与
   「多嵌 1~2 层」三种可能各算一次，任一候选越层即报（同一条例令只报一次）。
   报告保留源码拼写并附「解析为 …」，让读者能对着自己写的那一行看懂判据。

   glob 形态（`use crate::X::*`）无需专门处理：第 4 条的段前缀匹配对
   `crate::X::*` 同样成立。`tests/arch_violation_fixture.rs` 用三种 glob 形态
   （裸 glob / 组内 glob / `pub use` glob）把这件已查证的事实钉住——评估阶段
   曾误判 glob 可绕过禁令，实测三种形态均已被拦。

### 4.2 表现层的唯一例外：`crate::app::frame`

> **适用范围已收窄（R-03 B1 收官，2026-10-27）**：本节讲的是 `Layer::Presentation`
> 与 `state/theme/shortcuts/rich/widgets` 子层。`src/presentation/views/**` 已
> **撤回**这条豁免（理由见 §4.1 下的撤回说明与 §10.1 硬约束 5），视图一律收
> `&mut dyn EditorViewContext`。下面「54 个表现层文件」是撤回前的统计，保留作
> 历史依据；现行数字以 `check-arch` 实测为准。

表现层的非视图代码仍以 `&EditorApp` 接收应用状态
（`crate::app::frame::EditorApp`）——这是**被认可的下行通道**：组合根把状态句柄
交给表现层，表现层读状态、发动作。

若一刀切禁掉 `crate::app`，这些表现层文件会同时报错。那不是「更严的架构」，
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

**同一出口兼观测点（D-5，2026-10-26）**：`FrameScheduler` 是唯一每帧首尾都会被调到
的地方，故帧耗时也在 `flush` 结账（超 `FRAME_BUDGET` = 16ms 时 `log::warn!`，
带状态更新 / 绘制 / 未归因三段归因与到期任务；**只在 release 构建输出**，debug 的帧耗时
不可比，全超门槛的日志等于没有告警）。阶段边界由 `loop.rs` 的两次
`lap(FramePhase::…)` 给出——**不要在别处自己 `Instant::now()` 计时再打日志**：
那会和总耗时各算一套，出现「分项之和 > 总计」这种自相矛盾的输出。

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
> （infrastructure 禁 `egui` 的存量放行不走本表，见本节下文。）

### 4.5 `shared` 准入三问与登记表（D-6，机械守卫）

`shared` 是**共享内核**（crate `rustmark-shared`，TD-04 起独立；目录仍是
`crates/rustmark-shared/src/shared/`）：依赖矩阵里每一层都允许依赖它，它自己几乎不依赖任何层。
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
| `shared/kernel.rs` | 伞形重导出模块（R-02，2026-10-27）：仅 `pub mod …;` + `pub use kernel::*;`，把 `shared/kernel/` 子目录下的端口与值对象拼回 `shared::<名>`，自身无独立职责，不属三问对象 |
| `shared/support.rs` | 伞形重导出模块（R-02，2026-10-27）：仅 `pub mod …;` + `pub use support::*;`，把 `shared/support/` 子目录下的工具拼回 `shared::<名>`；其下各模块按职责分别登记于 `shared/kernel/`、`shared/support/` |
| `shared/support/constants.rs` | 跨层阈值常量（大文件 500KB / 防抖 150ms / 滚动容差 0.5pt）：core 的应用层、基础设施层、表现层与 `rustmark-ui`、root 五处消费，改一处漏一处即行为分叉；纯常量零 IO，无业务归属层 |
| `shared/support/encoding.rs` | 编码**标签**纯值：infrastructure 探测编码（IO），presentation 状态栏展示，两层方向互禁，抽值共享 |
| `shared/support/error.rs` | `AppError` 跨层统一错误：domain / application 具体错误的汇聚点，全部层与二进制入口共用 |
| `shared/kernel/export_format.rs` | 导出格式与页面尺寸唯一真源（R-07）：domain 校验、presentation 选择、infrastructure 路由三方需要 |
| `shared/kernel/fonts.rs` | 编辑区等宽字体契约（哨兵 + 选项）：设置页（presentation）与装配两侧需要；本模块不引入 egui、零 IO |
| `shared/support/geometry.rs` | 文本坐标值对象（字节偏移 / 区间 / 行列）：domain 与 presentation 共用的纯值，零 IO |
| `shared/support/ids.rs` | 强类型 ID（Shared Kernel）：四个上下文共用防 `u64` 误传；进程内计数器，零依赖零 IO |
| `shared/support/lru.rs` | 有界 LRU：高亮 / 拼写 / 渲染 / 图片四处缓存（应用层与表现层）的唯一实现，纯逻辑 |
| `shared/support/path.rs` | `FilePath` 路径值对象：document / workspace / export / preferences 四上下文共享内核；领域层禁 `PathBuf`，纯规则零 IO |
| `shared/support/pattern.rs` | 迷你正则引擎：表现层查找面板与应用层跨文件搜索共用（依赖方向单向，两层不能互引），自研控体积 |
| `shared/support/result.rs` | `AppResult` 结果别名：全应用统一错误结果，`error.rs` 的配套，被各层引用 |
| `shared/support/text_format.rs` | 可打开文本格式白名单唯一出处：拖放分类（app）、对话框过滤（`rustmark-ports` 的 `dialog` 端口）、状态栏标签（presentation）三处共用 |
| `shared/support/text.rs` | 英文分词规则唯一出处：应用层拼写建议与表现层右键菜单共用，与检查器同源 |
| `shared/kernel/theme.rs` | 主题种类唯一真源（R-09）：原散落六处的枚举 / 映射收敛，domain / application / presentation 共用 |
<!-- /shared:registry -->

> **R-10 项 A 搬出记录（2026-10-28）**：`dialog` / `event` / `image_asset` /
> `system` / `time` 五个端口已从 `shared/kernel/` 物理迁入独立 crate
> `rustmark-ports`（`crates/rustmark-ports/src/`），不再是 `shared/` 成员，
> 故不在上表登记；其「≥2 层需要 + 零 IO + 无业务归属层」理由随迁——端口
> crate 只单向依赖 `rustmark-shared` 的纯值类型（跨 crate 依赖面由
> `workspace_topology_test::ports_crate_declares_no_workspace_dependency` 兜底）。

> 新增文件的流程：**先答三问，再加一行**；答不出「≥2 层需要」就放回所属层。
> 表头行与分隔行不算登记项；文件列写 `shared/x.rs`、`src/shared/x.rs`、
> `crates/rustmark-shared/src/shared/x.rs`（现落点）或
> `crates/rustmark-core/src/shared/x.rs`（TD-04 前的历史写法，保留兼容）均可。
>
> **TD-04（2026-10-26）后落点变更**：`shared/` 目录整体迁到
> `crates/rustmark-shared/src/shared/`，本表键沿用 `shared/x.rs`——
> 校验器按「根内相对路径」匹配，新 crate 保留了 `shared/` 前缀，登记表
> 与 `build.rs` 的机械核对零改动。新增文件时若只在核心里写、忘了它其实住在
> 另一个 crate，构建会报「未登记」——那是提醒，不是误报。
>
> **R-02（2026-10-27）内部二分**：`shared/` 下的模块按职责落在两个子目录，
> 表键随之为 `shared/kernel/x.rs`（端口与值对象：`dialog` `event` `system`
> `image_asset` `time` `theme` `fonts` `export_format`）或
> `shared/support/x.rs`（与层无关的工具：常量 / 编码 / 错误 / 几何 / ID /
> LRU / 路径 / 正则 / 结果 / 分词 / 文本格式）。`shared.rs` 以
> `pub use kernel::*` / `pub use support::*` 把原 `shared::<名>` 路径拼回，
> 调用点零改动；`internal_cycle_guard_test` 已扩展为递归收集这两个已知子目录。

### 4.6 `shared` 内部依赖无环（V11-55 附案②，机械守卫）

`shared` 内部**不是**自由市场：模块图必须保持无环。Rust 允许 crate 内模块
互相引用——环编译得过、编译器沉默，但环让模块无法单独理解与单独测试，
且是日后拆 crate（如 V11-64）的硬墙。

机械守卫 `tests/shared/internal_cycle_guard_test.rs`，三条判据：

- **无环**：对真实工程跑 DFS，有环即报红并打出环路径；
- **边集钉死**：内部依赖边（当前 **11 条**）登记为显式清单，新增边须先落
  清单才能过——「shared 内部依赖面的任何变化都要落到清单上」；删边同样
  报红（清单出现过期项）；
- **形态唯一**：边只从 `crate::shared::<mod>` 绝对路径提取；
  `super::<mod>`、glob 导入后的裸 `<mod>::`、`self::<mod>` 等形态让边
  悄悄漏走（词法看不出 `super` 链的嵌套深度），一律报红要求改写。
  守卫口径经 `arch_check::lex_dump` 剔注释与字符串——文档注释里的
  intra-doc 链接不产生边。

历史：V11-55（2026-10-21）破除 `text_format` ↔ `dialog` 环后，2026-10-25
发现评审后又新生出 `path` ↔ `text_format` 二环（`FilePath::extension` 调
`crate::shared::text_format::extension_index`，而 `extension_of` 调
`crate::shared::path::file_name_of`）——环随即将 `extension_index` 迁入
`path.rs`（规则放在被依赖侧，方向保持 `text_format → path` 单向），
并立本守卫防复发。

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
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
cargo run --bin check-arch
```

`--workspace` 仍**显式书写**（自明意图、与 CI 同口径），但不再是正确性的依赖：
2026-10-26 起根 manifest 的 `[workspace] default-members` 已覆盖全部成员
（root `.` + 8 个 crate，共 9 成员），漏带 flag 时 cargo 也跑全量——原先「忘带 → core 单测
**静默缺席**、本地全绿 CI 才红」的失效模式（多 crate 评审债务 D-2）已由 cargo
自身兜底。两份清单的同步同样被机械钉死：
`tests/workspace_topology_test.rs::default_members_cover_every_workspace_member`
对 `members` + 隐式 root 做**集合相等**断言——新增 crate 只改一处即红。

> 边界：`default-members` 只影响「不带 `-p`/`--workspace` 的命令跑哪些包」。
> `cargo test --doc` 仍单独跑（`--all-targets` 不含 doc 测试），命令面见
> `docs/guides/testing.md`。

违规注入自检：在 `crates/rustmark-core/src/domain/**` 临时加入 `use egui;`
→ `check-arch` 必须退出码非 0 并打印带 `crates/rustmark-core/src/` 前缀的
文件行号（shell 侧注入同理，路径不带前缀）。

注：`cargo run --bin check-arch` 会先编译，而 `build.rs` 在编译期就做同一遍校验
并在违规时 `panic!`——违规时你看到的往往是 build.rs 的 panic。两处共用同一份
`format_report`，内容一致；想只看 bin 的输出可设 `SKIP_ARCH_CHECK=1` 跳过构建期校验。

校验器自身的用例在 `tests/arch_violation_fixture.rs`。它验的是**形状覆盖**：
每种逃逸形态（`pub use` / 嵌套花括号 / 跨行 / 别名 / 层根文件 / 组合根 /
注释与字符串里的假路径）各有一条注入用例，变异验证确认它们不是恒绿。

### 6.1 门禁定义（提交前七条 / 发布前追加一条）

上文代码块是**最小快速自查**；本小节是**唯一权威清单**，三套口径并存时以本节为准。

**提交前七条（全绿才可提交）**

```bash
cargo fmt --all --check
cargo run --bin check-arch
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --no-fail-fast
cargo test --workspace --doc
python3 tools/check_thresholds.py     # 121 条
bash scripts/size_gate.sh             # 先 build --release 再测量
```

**发布前追加一条**：`python3 tools/release_dryrun.py`（须 **15/15**）。

两条易漏口径：

1. `cargo test --doc` **必须单独跑**——`--all-targets` **不含** doc 测试
   （`--all-targets` = lib/bins/tests/benches/examples，doctest 是独立目标）；
2. clippy 必须带 `--all-features`——与 CI 同口径，否则 feature 门控下的代码
   （如 `rustmark-render` 的 egui 后端）本地不参与检查，本地绿而 CI 红。

`size_gate.sh` 自 2026-10-27 起**先 `cargo build --release` 再测量**（`6523200`），
终结「只 find 现成产物 → 量到陈旧二进制 → 静默假绿」的失效模式；
逃生口 `SIZE_GATE_SKIP_BUILD=1` 仅供离线复核使用。

**第八条 · 拓扑基线 diff（CI 步骤，本地不默认跑，改依赖时必跑）**

```bash
diff <(grep -v '^#' docs/spec/workspace-topology.expected.txt) \
     <(cargo tree --workspace --depth 1 --no-dedupe | sed -E 's| \([^)]*\)||g')
```

它不在上面七条里，是因为日常提交不涉及依赖变更；但**改 `Cargo.toml` / `Cargo.lock`
就必须跑**，否则 CI 红而本地全绿。

> 2026-10-28 实测教训：该文件注释头与机器主体之间曾混入一个**空行**——空行不以 `#`
> 开头，`grep -v '^#'` 会留下来，于是 diff 恒不等、本步骤长期红，而本地七门禁不含它，
> 无人发现。`scripts/gen_dep_graph.sh` 原先按「第一个非 `#` 起始行」切边界，会把这个
> 空行**原样保留**（幂等地维持红色状态）；已改为按「最后一个 `#` 起始行」切，重跑
> 产生的内容与 CI 比对口径一致。

## 7. 模块文件树（需求文档 §7.1，禁 `mod.rs`）

D-3 阶段 4a（2026-10-23）起为 workspace 多 crate：**core 只装六边形脊柱
（domain / application；表现层已于 V11-62 全部迁出，doc-only 空壳于 2026-10-30
删除），infrastructure 住
`rustmark-infra`（V11-64）、shared 住 `rustmark-shared`（TD-04），
shell（root）只留组合根、视图与 re-export 拼接点**；
`core → shell` 的反向依赖被 cargo 锁死，`rustmark_core::X` 与 `crate::X`
在校验器里按同一张禁令表判定（§4.1）。

本节是**文件级**模块树；workspace 级的 crate 地图（有哪些 crate、三阶段
演进到哪了）与「新模块该住哪个 crate」的判据见 §10（V11-56）。

```
src/                                    # shell（root crate `rustmark`）
├─ main.rs / lib.rs                     # lib.rs 公开 API shim：pub use rustmark_core::{application, domain, shared};
│                                       #   R-01（2026-10-27）：仅对外/历史 `rustmark::` 消费者维持路径稳定；
│                                       #   根源码（src/）禁走此联邦别名，须写显式 `rustmark_core::<层>::…`
│                                       #                  pub use rustmark_infra::infrastructure（V11-64）
├─ app.rs / app/{bootstrap.rs,frame.rs,crash_log.rs,…}
│                                       # 组合根；帧预约唯一出口 app/frame/scheduler.rs
├─ presentation.rs                      # 拼接点：pub mod views（唯一留 shell 的 presentation 子层）
│                                       #   + pub use rustmark_ui::presentation::{rich, shortcuts, state, theme, widgets}（V11-62 批次 1-5 收官）
│                                       #   + pub use rustmark_icons as icons（V11-58）
│                                       #   （core 的 presentation 已全迁 ui，空壳于 2026-10-30 删除）
├─ presentation/views/                  # 视图（唯一留在 shell 的 presentation 子层）
└─ bin/check-arch.rs                    # 工具二进制

crates/rustmark-core/src/               # core（crate `rustmark-core`）—— 六边形脊柱；装配 DI 边界在 application/composition.rs（ImplBundle/ComposeConfig/AppServices/compose，R-10 项 B，绝不引 infra）
├─ lib.rs / {domain,application}.rs
├─ domain/{document,markdown,workspace,preferences,export}/
│    └─ 各含 {aggregate.rs,value_objects.rs,events.rs,services.rs,repository.rs,errors.rs}
├─ application/{document,markdown,workspace,preferences,export}/ + dto/
└─ （shared 于 TD-04 抽去 `crates/rustmark-shared`；infrastructure 于 V11-64
     抽去 `crates/rustmark-infra`；两者的公开路径均经上面 lib.rs 的 re-export 不变）

crates/rustmark-infra/src/              # infra（crate `rustmark-infra`，V11-64）
├─ lib.rs
└─ infrastructure/{persistence,text,parsing,highlighting,export,events,dialogs,fonts,spell}/  # 保 `infrastructure/` 前缀 → 层判定与禁令表零改动

crates/rustmark-shared/src/             # shared（crate `rustmark-shared`，TD-04）
└─ lib.rs / shared.rs / shared/{kernel/（端口+值对象）,support/（工具）}  # R-02 二分；保 `shared/` 前缀，shared.rs 以 pub use 拼回 `shared::<名>`

crates/rustmark-actions/src/            # actions（crate `rustmark-actions`，V11-57）
└─ lib.rs / action.rs / action/{impls.rs,menu_group.rs}   # 零依赖词汇表，整根判 Free

crates/rustmark-icons/src/              # icons（crate `rustmark-icons`，V11-58）
└─ lib.rs                               # 16×16 图标规格 + Painter 渲染器；依赖 egui + rustmark-actions

crates/rustmark-ui/src/                 # ui（crate `rustmark-ui`，V11-62 批次 1-5 收官）
└─ lib.rs / presentation.rs / presentation/{widgets,state,shortcuts,theme,rich}/  # widgets（批次 1）、state（批次 2）、shortcuts（批次 3）、theme（批次 4）、rich（批次 5）；依赖 egui + rustmark-core
```

> **R-01 收敛（2026-10-27）**：root（src 根）对 `rustmark-core` 内部层的依赖已收敛为显式
> `rustmark_core::domain` / `rustmark_core::shared` / `rustmark_core::application`，不再经
> `lib.rs` 的联邦 re-export 走 `crate::<层>` 别名（消除未声明逻辑依赖，真实依赖在
> `Cargo.toml` 完全可见）。`lib.rs` 的两行 `pub use` 退化为**公开 API shim**，仅维持对外/
> 历史 `rustmark::<层>` 路径稳定；**根源码（src/）禁止再用该联邦别名**，由
> `tools/arch_check.rs` 的 `check_root_federated_layers` 机械化兜底（只拦 `crate::domain` /
> `crate::shared` / `crate::application` 字面三连，不影响 `crate::infrastructure` 装配路径，
> 也不误伤显式 `rustmark_core::<层>`）。`cargo tree -p rustmark` 仅含声明依赖。

白名单外的顶层位置（shell `src/foo.rs`、core `src/utils/` 等）会被校验器报
「未知层」：新目录要么归入既有层，要么先改本文档与校验器再放代码。
shell `src/bin/` 是工具二进制（`check-arch`），与 `main.rs` / `lib.rs` 一样
不受层级限制。

`crates/rustmark-actions/src/`（V11-57）整个根判 Free：零依赖词汇契约
crate 没有层内边界可言。「不得引用 core / 任何 crate」由 Cargo 循环依赖
（core 已依赖它，反向加普通依赖即解析期报错）与 workspace 拓扑断言
（`docs/spec/workspace-topology.expected.txt`，任何新依赖边都红）兜底——两者
都比词法规则强，且覆盖「获得任意依赖」而不只「引用 core」一种形状。
层禁令不适用于该根，但禁 `mod.rs`、帧预约唯一出口等**与层无关**的规则
照旧生效（fixture 第十六节钉住）。

`crates/rustmark-icons/src/`（V11-58）**不设**强制层：单文件 `lib.rs` 落在
crate 入口白名单里，`layer_of` 自然判 Free（依赖面 = egui，由
`tests/workspace_topology_test.rs` 断言；R-05 已移除 `rustmark-actions` 边）。禁
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

> 注：上表「5–7 MB」为**早期需求预算估算**，非实测。当前实测见
> `docs/spec/metrics.md` §2.8（release 产物 ≈10.5 MiB）；红线本身已于 2026-10-24
> 由 10 MiB 上调至 **11 MiB（预警 10.5 MiB）**，与 `scripts/size_gate.sh` 默认口径一致。

新增依赖必须登记体积贡献（`docs/guides/dependencies.md`）并经 ADR 追加评审。

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

### 9.1 测试归属口径与目标登记（D-6，机械守卫）

根包设了 `autotests = false`：`tests/` 下的文件**只有被 `[[test]]` 点名才会编译**。
好处是入口可命名、可按层分组；代价是漏登记时 cargo 不报错、测试静默不跑，
而全量测试依旧全绿——「检查缺席却显示绿」正是本项目最忌讳的失效形态
（同 `default-members`，见 §6：「跑了失败」是响的，「压根没跑」是哑的）。

**归属口径**（三条，按「有没有自己的 `main`、会不会被别的目标 `mod` 进来」判，
不按行数）：

| 形态 | 归属 | 例子 |
|---|---|---|
| `tests/<dir>/main.rs` | **一个独立测试目标**，按被测层 / 场景分组 | `tests/domain/main.rs`、`tests/presentation/main.rs` |
| `tests/<name>_test.rs`（顶层单文件） | **一个独立测试目标**，只装单主题守卫 | `tests/field_visibility_guard_test.rs`、`tests/workspace_topology_test.rs` |
| `tests/support.rs` 与 `tests/{support,common,data}/` | **共享模块 / 数据**，永不登记为目标 | 各目标以 `#[path = "../support.rs"] mod support;` 引入 |

**新增测试时**：先按上表定性；目录 / 单文件目标必须在根 `Cargo.toml` 补
`[[test]] name = … / path = …`；共享模块必须进
`tests/test_target_guard_test.rs` 的 `EXEMPT` 并写明理由。

**机械守卫**（`tests/test_target_guard_test.rs`，五条双向断言）：
① 每条 `[[test]]` 的 `path` 存在（防删文件忘删登记）；② 每个 `tests/**/main.rs` 已登记；
③ 每个顶层 `tests/*.rs` 已登记或在 `EXEMPT`；④ 每个 `tests/` 顶层目录要么是目标
要么在 `EXEMPT`；⑤ `name` / `path` 唯一。`EXEMPT` 自身也双向：条目必须真实存在，
且不得与已登记目标重叠——过时豁免会长期掩盖一次改名或删除。
变异验证：新增未登记目录 / 顶层文件 → ②③ 当场红。

**现状（2026-10-26）**：13 个测试目标（8 个目录型 + 5 个单文件，含本守卫自身），
共享模块 4 项（`support.rs` / `support/` / `common/` / `data/`）。

## 10. crate 地图与模块归属判据（V11-56，2026-10-24）

> 本章回答两个问题：**workspace 里应该有哪些 crate**（地图 + 三阶段演进，
> §10.1），以及**一个模块该住哪里**（四问判据 + 外移触发条件，§10.2/§10.3）。
> 判据来源为 `docs/archive/architecture-reviews/core-structure-review-2026-10-24.md`（评审三大病灶的
> 规则化收口）；决策记录见
> `docs/adr/ADR-013-core-adapter-residency-and-leaf-extraction.md`。
> §1–§9 的分层与禁令对**所有** crate 一视同仁——新 crate 不是法外之地：
> 层判定口径见 §4.1，自由层（Free）的来龙去脉见 §7。

### 10.1 crate 地图

**当前拓扑**（2026-10-29 实测，`cargo tree --workspace --depth 1`；本地依赖边共 16 条）：

```
rustmark（root：组合根 + views + bin）
├──► rustmark-core   ──► rustmark-actions
│                     ├► rustmark-shared
│                     └► rustmark-ports ──► rustmark-shared
├──► rustmark-infra  ──► rustmark-core
│                     └► rustmark-ports
├──► rustmark-icons
├──► rustmark-assembly ──► rustmark-core
│                        ├► rustmark-infra ──► rustmark-core / rustmark-ports
│                        └► rustmark-ports
└──► rustmark-ui     ──► rustmark-core
    └──► rustmark-render（默认 egui-free；egui 仅 `feature="egui"` 门控）
                        （16 条本地边，无环；actions 是唯一零依赖叶）
```

| crate | 角色 | 实测规模（2026-10-26，`src/` 口径） | 外部依赖面 |
|---|---|---|---|
| `rustmark`（root） | 组合根 + 视图层 + 工具 bin | 92 文件 / 22,113 行（2026-10-27 实测） | eframe / egui / image / rfd / anyhow / env_logger / log / wgpu(build) + 4 个本地 crate（core / infra / icons / ui） |
| `rustmark-core` | 六边形脊柱：domain / application（表现层非视图子模块已于 V11-62 全抽去 `rustmark-ui`，infrastructure 于 V11-64 全抽去 `rustmark-infra`；doc-only 的 `presentation` 空壳于 2026-10-30 删除，本 crate 不再有 presentation 路径）；**零 egui 依赖**（V11-63） | **105 文件 / 15,565 行**（2026-10-28 实测；R-06 三步后的口径：步 1 新增 `application/workspace/service/search/scan.rs` 483 行、同目录 `search.rs` 626 → 346；步 2 拆 `application/document/service/stats.rs` 为 facade 114 + `stats/{classify,links,accumulator,equivalence}.rs` 118+138+193+285，原 639；步 3 拆 `application/document/find.rs` 为 facade 359 + `find/matching.rs` 370，原 535） | **4 个**直接外部依赖（serde / thiserror / log / ahash）+ 3 个本地（actions / shared / ports） |
| `rustmark-infra` | 基础设施层：外部库适配与 IO（解析 / 高亮 / 文本缓冲 / 对话框 / 远程图片 / 图片解码 / 导出 / 字体装配）。**端口与领域类型仍住 core**（V11-64） | 65 文件 / 11,062 行（2026-10-28 实测） | **11 个**直接外部依赖（pulldown-cmark / syntect / image / ropey / rfd / serde / toml / thiserror / ureq / rustls-native-certs / log）+ 2 个本地（core / ports） |
| `rustmark-actions` | 动作词汇表契约（V11-57） | 4 文件 / 687 行 | **零依赖** |
| `rustmark-shared` | 共享内核：纯值 / 唯一真源（TD-04；端口已按 R-10 项 A 迁出为 `rustmark-ports`） | 18 文件 / 2,715 行（2026-10-28 实测） | ahash + serde + thiserror（**零本地**） |
| `rustmark-ports` | 架构端口层（R-10 项 A，2026-10-28）：`dialog` / `event` / `system` / `image_asset` / `time` 五个跨层契约，自 `rustmark-shared` 的 `kernel/` 物理搬出；自身零 IO、零业务归属层，单向依赖 shared 的纯值类型（error / path / text_format / geometry / ids / result）；core 经 `pub use rustmark_ports` 拼回 `rustmark_core::ports`，消费方零改动 | 6 文件 / 585 行（2026-10-28 实测；`src/` 口径不含 tests） | rustmark-shared + serde / thiserror / ahash（**零外部 IO**） |
| `rustmark-icons` | 16×16 图标规格 + Painter 渲染器（V11-58） | 1 文件 / 630 行 | egui |
| `rustmark-ui` | 表现层非视图部分：widgets（批次 1）/ state（批次 2）/ shortcuts（批次 3）/ theme（批次 4）/ view_context（R-03 B1：视图窄接口 `EditorViewContext`）；`presentation::rich` 为兼容 shim（R-04 升 crate 为 `rustmark-render`，经 `pub use rustmark_render::*` 拼回原路径，消费方零改动） | 48 文件 / 12,265 行（2026-10-28 实测，rich 升 crate 后；`src/` 口径不含 tests） | egui + rustmark-core + rustmark-render |
| `rustmark-render` | 富内容渲染引擎：数学排版 / Mermaid 自绘（`rustmark-ui::presentation::rich` 物理搬出，R-04 升 crate，2026-10-28）；默认 egui-free，唯一 egui 后端 `render/egui.rs` 走 `feature="egui"` 双门控（`#[cfg(feature = "egui")]`），依赖面仅 egui（optional）；对 `rustmark-core` 零依赖 | 27 文件 / 5,579 行（2026-10-28 实测；`src/` 口径不含 tests） | egui（optional，仅 `feature="egui"`） |
| `rustmark-assembly` | 组合根装配下沉（M2，R-10 项 B 补全，2026-10-29）：原 root `src/app/bootstrap.rs` 的 `production_bundle`（全仓唯一点名 13 个 infra impl 处）/ `Services`（14 字段）/ `ConfigPaths` / 5 个 `assemble_with_*` / 私有 `assemble_with_bundle` 整体平移至此；root 仅留 `pub use rustmark_assembly::bootstrap::*` 门面拼回 `rustmark::app::bootstrap::*` 原路径，消费方零改动；装配逻辑 egui-free（依赖面 core + infra + ports，三者对 egui / eframe / wgpu 命中均为 0） | 2 文件 / 351 行（2026-10-29 实测；`src/` 口径：lib.rs + bootstrap.rs） | rustmark-core + rustmark-infra + rustmark-ports（**零外部 IO**） |

规模口径：`find <源码根> -name '*.rs' -exec cat {} + | wc -l`（**不用 `xargs wc -l`**——超 ~100 文件会分批而 `tail -1` 只抓末批小计），只含 `src/`、
不含 `tests/`，命令随文可复跑。core 与评审快照（46,375 行 / 238 文件）的差额 =
V11-57/58/62 拆出的文件（actions 3 + icons 1 + ui 的 widgets 3 / state 24 /
shortcuts 5，另 2 个为 ui 的新建骨架 lib.rs / presentation.rs）、TD-04 抽出的
shared 19 文件 / 3,061 行、V11-64 抽出的 infra 54 文件 / 10,976 行，扣除期间
core 内的模块清单与文档增补；快照 238 与 git 树基线 `662536b` 的 244 文件
之间存在未逐文件核对的口径差（评审当时工作区含 D-6/D-7 在途改动），
**本节数字以今日实测为准**（2026-10-26 复测：root 91 / 21,953，core 99 / 14,576，
infra 55 / 11,011，shared 21 / 3,153，ui 71 / 17,301。其中 V11-61 把
`find_replace_ui_state/{engine,pattern}.rs`（353 + 21 = 374 行）上浮为 core 的
`application/document/find.rs`（521 行，含 8 条内联用例），故 **ui −2 文件 / −352 行、
core +1 文件**；表内数字此前未随 PRIV-01/02 与 D-5 刷新，本次一并复测。构成变化：TD-02 拆分
`preview_view.rs` +2 文件 / +20 行；`c08fd13` 阈值整改在 core / ui / root
分别 +171 / +3 / +101 行（撤销字节预算、纹理缓存字节预算等）；
`toolbar_view.rs` D-4 拆分 +1 文件 / +14 行；E-24 拆分 `inline.rs` 为
facade + 4 子模块 **+4 文件 / +208 行**；E-06 阶段 B 新增
`infrastructure/fonts/port.rs` **+1 文件**；E-25 拆 `src/app/frame.rs`
（839 行）为 facade 195 + `frame/state.rs` 434 + `frame/loop.rs` 257 = 886 行
（**+2 文件 / +47 行**）；rich 于批次 5 从 core 迁 ui，V11-62 收官；
TD-04 / V11-64 为整目录搬迁，两侧行数守恒——infra 的 55 文件 / 11,011 行 =
迁入 54 文件 / 10,976 行 + 新骨架 `lib.rs` 29 行 + PRIV-02 的导出件 `<head>` 注释 6 行）。

**三阶段演进**（照评审报告 §4；阶段 1 已由 V11-57 / V11-58 落地，阶段 2 已由
V11-62 全落地，阶段 3 的 `rustmark-infra` 行已由 V11-64 提前兑现——**触发依据是
维护者的消费者计划，不是消费者已存在**，这一点与评审原意（「真实非 UI 消费者
出现后」）的差异如实保留）：

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
`rustmark-core` 为 171 / 28,047（表现层非视图子模块全迁 ui，`presentation`
退化为 doc-only 空壳；`c08fd13` 另 +1 文件 / +171 行）。阶段 2 列 ui 的
18.1k 计划值（含 theme/rich）至此全落。

**2026-10-26 更新（TD-04 + V11-64）**：演进图阶段 3 的两行提前兑现——
`rustmark-shared` **21 文件 / 3,153 行**（TD-04，`2da434e`）、
`rustmark-infra` **55 文件 / 11,011 行**（V11-64，`7b9b6ba`，含新骨架
`lib.rs` 29 行；迁入量 54 文件 / 10,976 行与阶段 3 标注的 10.8k 吻合，
另 + PRIV-02 的 6 行 `<head>` 注释）。
`rustmark-core` 降至 **99 文件 / 14,576 行**（阶段 3 计划 16.7k 的进一步
收窄，差额来自 stage-3 估算未计入 shared 整体外移；V11-61 又把 find 引擎
上浮进来，+1 文件 / +585 行）；`rustmark-ui` **71 文件 / 17,301 行**（V11-61
移出 `engine.rs` + `pattern.rs`，−2 文件 / −352 行）。
阶段 3 尚未兑现的部分只剩「root 的 views 是否也外移」——评审未把它列入
计划，本表不预设。

方向规则不变的三条硬约束：

1. **root→core 方向规则保留**：组合根可依赖全部 crate，下层不得反向依赖
   root——由 Cargo 结构性锁死（core 已依赖 actions/icons，反向加普通依赖即
   解析期报错）；
2. **views 必须留 root**（ADR-013 决策 3）：presentation 外移采用「views 留
   root 的中间 crate」形态，否则 R-11/R-12 的「views ∈ root、state ∈ 另一
   crate」跨 crate 字段保护会被降级成同 crate 内约束；
3. **每拆一个 crate，三件套同步**：校验器扫描根 + `layer_of` 归一化 + 本地
   依赖边期望（`docs/spec/workspace-topology.expected.txt`）——照 actions/icons
   两例（§7）。漏任一件，新 crate 要么被误报「未知层」，要么依赖边静默增长。
4. **ui 子模块三处对账**（R-03 B2，2026-10-27）：`rustmark-ui` 的
   `presentation/` 子模块增删，必须同时改 root `src/presentation.rs` 的
   **逐项** `pub use rustmark_ui::presentation::<子模块>;`（**禁**整组
   `::{…}`——新增子模块会被静默带入 root，清单与本节失配却不报）与本节
   `rustmark-ui` 行；`check-arch` 的 `check_ui_submodule_sync` 拿三处真源
   （ui 侧 `pub mod` ↔ root 逐项 re-export ↔ 本节 §10.1）对账，缺一即报。
   「views 留 root」的已知约束见上一条硬约束 2（ADR-013 决策 3），不靠人记。
5. **视图窄接口（R-03 B1，2026-10-27 收官；2026-10-30 按视图窄化）**：
   `src/presentation/views/**` 下的视图**不得**直取组合根 `crate::app`——一律收
   **该视图自己的窄 trait**（`XxxViewAccess`，17 个，按面板裁剪能力面；
   trait 定义在 `rustmark-ui::presentation::view_context`，能力面仍分
   `ThemeAccess` / 各 `*UiAccess` / `DocumentAccess` / `CommandPort` /
   `MarkdownQueries` 等窄查询；实现在 root 的 `src/app/frame/view_context.rs`）。
   **宽上下文只留给路由者**：`main_view` / `overlays` 收 `&mut dyn EditorViewContext`
   （17 个窄 trait 拼成的 bundle，靠 trait object upcasting 零成本转交给各视图）。
   **三道守卫同时生效，缺一不可**：
   - `check_view_decoupling`（**路径**判据）：`views/**` 出现 `crate::app` 即报。
     路径枚举**只经 `parse_use_tree` 展开**——`use crate::{app::frame::EditorApp};`
     这种花括号形态能绕过「`crate` `::` `app` 相邻三连」的字面匹配，实测让
     `build.rs` 与 `check-arch` **两层全绿**（P0，2026-10-27 修复）。禁止再写
     第二套扫描器。
   - `check_view_no_composition_root_marker`（**记号**判据）：`views/**` 出现
     `EditorApp` **标识符**即报。补的是路径判据的结构性盲区——子模块经
     `use super::*` 拿到父模块的私有 `use` 别名后，本文件一行 `crate::app` 都没有
     却能声明 `app: &mut EditorApp`（`preview_view/{blocks,code,list,media,table,toc}.rs`
     六个文件正是如此，它们从未进过白名单、也从未被报过一次）。两条判据都走
     词法器，故视图文档里讨论「为什么不再用 `EditorApp`」的 20 处注释不触发。
   - `check_view_narrow_context`（**宽上下文**判据）：`views/**` 出现
     `EditorViewContext` **标识符**即报，白名单只有路由者 `main_view.rs` /
     `overlays.rs`。补的是窄化自己的回退口子——宽窄两种签名都能编译，某个视图
     写回宽上下文，越权访问就从「编译期不可达」悄悄退回「靠评审发现」，
     `cargo check` / `clippy` / 另两条判据全绿，只有这条拦得住。
   **白名单已退役**：`PENDING_DECOUPLED_VIEWS`（最初 32 条、「只减不增」的迁移
   进度表）在批 2 收官时清零，常量整个删除，`check_view_decoupling` 直接传
   `&[]`。不保留空表——空表读起来像「暂时没人管」，传 `&[]` 表达的是
   「一名都不许有」。
   **`crate::app::frame` 的按层放行已从 `views/` 撤回**：`allowed_for` 不再给
   `Layer::PresentationViews` 豁免（其余表现层子层照旧，见 §4.2）。当初并存是
   过渡态，撤回条件是「全部视图迁完」，该条件已兑现——这是**兑现**，不是规则反悔。
   **维护说明（变异验证的姿势）**：要证明守卫真能抓，必须
   `SKIP_ARCH_CHECK=1 cargo build --bin check-arch` 再独立运行。直接
   `cargo build --bin check-arch` 会让 `build.rs` 先 panic 退出，**bin 根本不会
   重建**，跑的是旧 exe ——结果是假绿。

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
  `docs/archive/architecture-reviews/state-field-audit.md`）。

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
「当前状态」列随每次度量刷新，数字以 `docs/spec/metrics.md` §7（接入成本基线）
为家，本表只放判定结论。

| 动作 | 触发条件（任一满足即启动评估） | 度量方式 | 当前状态（2026-10-26 复核） |
|---|---|---|---|
| presentation 非视图子模块 → `rustmark-ui`（V11-62） | ① UI 并行开发人数 ≥2；② core 增量编译 >10s；③ presentation 年变更 ≥80 提交**且**「views↔core state 跨 crate 摩擦」在 ≥3 个功能中复现 | `tools/onboard_cost.py` + `git log` 去重 | ② 未触发（增量 5.92s）。③ **提交数已去重实测**——表现层整体 **88**、含 `tests/presentation` **93**、仅待搬迁的非视图部分 **51**（阈值 80：整体口径满足、待搬迁部分不满足，口径差异如实保留）；**摩擦已取证：8 个功能复现**（state 字段白名单 169/548 条 + 22 个 `pub` setter + 桥接 6 行 / 21 个 view 文件经桥接访问）。**③ 判定：满足**（证据见 tasklist V11-62 节）。① 仍待维护者提供。**已收官（2026-10-25）**：五批次全部落地（widgets `9ab57ae` → state `c40235c` → shortcuts `6778f20` → theme `14f7d87` → rich `d8126a4`），`rustmark-ui` 收官态 73 文件 / 17,653 行（2026-10-26 实测）、本地依赖边 6 条、release 10,487,296 B；摩擦只搬迁不消除（views 留 root，白名单 169 条 + 22 个 `pub` setter 换 crate，保 R-11/R-12） |
| infrastructure → `rustmark-infra`（V11-63/64） | 出现真实非 UI 消费者（CLI / headless / 嵌入）**且**去 egui 化完成 | 需求事实 + `cargo tree -p rustmark-core \| grep egui` 为 0 | **两分支均已满足，已收官（2026-10-26，`7b9b6ba`）**：egui 侧 V11-63 达标（`cargo tree -p rustmark-core \| grep -c egui` 实测 0，依赖已删非仅无引用）；消费者侧由维护者对 Q-01 答「有计划（CLI / headless / 嵌入）」。落地形态：54 文件 / 10,976 行迁入 `crates/rustmark-infra`（保 `infrastructure/` 前缀），core 依赖面收窄为 `rustmark-{actions,shared}` + serde/thiserror/log/ahash，本地依赖边 7→9（root→infra 与 infra→core 各一条）。**触发门保留**：若日后 Q-01 改答「无计划」，本行应记为「已落地但不再扩建」，不再重评 |
| math / mermaid → `rustmark-render` | mermaid/math 年变更 ≥10 提交或合计 >8k 行（现 5.6k） | git log + wc -l | **已触发并落地（R-04，2026-10-28，`22e8a86`）**：`rich/{math,mermaid}` 物理搬出为 `rustmark-render`（27 文件 / 5,579 行，`src/` 不含 tests），默认 egui-free、egui 仅 `feature="egui"` 门控、对 `rustmark-core` 零依赖；`rustmark-ui` 经 shim 拼回原路径、消费方零改动。触发条件虽未达（5.6k < 8k），维护者拍板提前升 crate 以兑现 O4「富内容渲染可独立复用」 |
| 新 crate 合并回退（防过度拆分） | 新建 crate <1k 行且 12mo 变更 <5 提交 | git log | 不适用：actions（687 行）/ icons（613 行）均 <1k 行，但属零依赖契约 / 纯资源 crate，回退等于把契约塞回 core——判据本意防「为拆而拆的业务模块」（ADR-013 决策 5） |
| shared 通用件再拆分 | pattern+lru+text 合计 >1,500 行或出现第 4 个非领域通用模块 | wc -l | **已触发并落地（2026-10-26，`2da434e`）**：模块计数分支满足——非领域通用件 **6 个**（pattern 531 / text_format 310 / lru 291 / path 278 / geometry 248 / export_format 233 = **1,891 行**，6 ≥ 阈值 4）；行数分支按本行初版具名清单（pattern+lru+text）算 **992 行**、未达 1,500（两分支口径不一致已如实保留：1,500 行公式固化了初版三模块清单，模块计数分支未绑定清单）。维护者拍板**抽 crate**，TD-04 落地为 `rustmark-shared`（19 文件 / 3,061 行 + 骨架，依赖面 = ahash/serde/thiserror，零 workspace 成员）；**是否把整份 shared 抽走 vs 只抽 6 个通用件**选择了前者（前缀保留后两者代价相同，整抽避免「shared 一半在内一半在外」的判层歧义） |
| state 瘦身叫停 | 审计发现纯帧内字段 <10%（收益不抵改动风险） | 字段清单 | **已触发并闭环**（V11-59 审计纯帧内字段远低于阈值 → V11-60 关闭，结论见 `docs/archive/architecture-reviews/state-field-audit.md`） |
