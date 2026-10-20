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
| shared | `egui`、`eframe`、`rfd`、`syntect`、`pulldown_cmark`、全部业务层（`crate::domain` / `crate::application` / `crate::infrastructure` / `crate::presentation` / `crate::app`） |

允许：domain 可用 `shared`、`ropey`、`regex`、`chrono`、`serde(derive)`、`thiserror`。

`crate::app` 是组合根。下层反向依赖组合根 = 依赖倒置被打破（领域层一旦能拿到
应用状态，聚合就不再是可独立测试的对象），故各层一律禁止。

### 4.1 判定口径（校验器怎么判）

校验器是**词法级**的（不解析 AST，不引入额外依赖）。判定口径即规则的一部分：

1. **词法**：源码 → token 流。行注释、块注释（含嵌套）、字符串字面量、raw string、
   字符字面量**全部剔除**——注释与字符串里出现的路径**不是依赖**。
2. **`use` 展开**：`use` 声明按 use-tree 语法展开成**完整路径**，覆盖
   `pub use` / `pub(crate) use` / `use a::{b, c}` / `use a::{self, b}` / 嵌套组 /
   `use a::b as c`（别名不改变依赖对象）/ `use a::*` / 跨行 / 前导 `::`。
3. **限定路径**：非 `use` 位置的 `a::b::C` 同样参与判定——
   `fn f(d: crate::domain::x::Y)` 也是跨层依赖。
4. **匹配**：与禁止清单做**路径段**前缀匹配——`crate::domain` 命中
   `crate::domain::x::Y`，但**不**命中 `crate::domainfoo`。
5. **层判定**：先剥掉末段 `.rs` 再取首段——`src/domain.rs` 属于 domain 层，
   `src/presentation/state.rs` 属于 `presentation/state` 子层。
6. **子层叠加**：`presentation/state` 同时受父层与子层两组禁令约束（不是替换）。

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

### 4.4 豁免（`arch-allow`）

行尾注释 `// arch-allow: <对象>` **只豁免该对象**（不是整行放行），
且必须登记在下方豁免表里。三种情况都会报错：

| 情形 | 报错 |
|---|---|
| 豁免未登记 | `arch-allow 豁免未登记` |
| 该行没有依赖它的违规（豁免写错或已过时） | `arch-allow 豁免无效` |
| 豁免对象与实际违规对象不符 | 照常报违规（豁免只认写明的那个对象） |

<!-- arch-allow:registry -->
| 文件 | 对象 | 理由 |
|---|---|---|
<!-- /arch-allow:registry -->

> 表头行与分隔行不算登记项。文件列写 `src/domain/x.rs` 或 `domain/x.rs` 均可。
> 当前无任何豁免登记——这正是期望状态：豁免是例外，例外应当可见且可回收。


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
cargo clippy --all-targets -- -D warnings
cargo check
cargo test
cargo run --bin check-arch
```

违规注入自检：在 `src/domain/**` 临时加入 `use egui;` → `check-arch` 必须退出码非 0 并打印文件行号。

注：`cargo run --bin check-arch` 会先编译，而 `build.rs` 在编译期就做同一遍校验
并在违规时 `panic!`——违规时你看到的往往是 build.rs 的 panic。两处共用同一份
`format_report`，内容一致；想只看 bin 的输出可设 `SKIP_ARCH_CHECK=1` 跳过构建期校验。

校验器自身的用例在 `tests/arch_violation_fixture.rs`。它验的是**形状覆盖**：
每种逃逸形态（`pub use` / 嵌套花括号 / 跨行 / 别名 / 层根文件 / 组合根 /
注释与字符串里的假路径）各有一条注入用例，变异验证确认它们不是恒绿。

## 7. 模块文件树（需求文档 §7.1，禁 `mod.rs`）

```
src/
├─ main.rs / app.rs / app/{bootstrap.rs,frame.rs}
├─ shared/{error.rs,result.rs,event.rs,ids.rs,geometry.rs}
├─ domain/{document,markdown,workspace,preferences,export}/
│    └─ 各含 {aggregate.rs,value_objects.rs,events.rs,services.rs,repository.rs,errors.rs}
├─ application/{document,markdown,workspace,preferences,export}/ + dto.rs
├─ infrastructure/{persistence,text,parsing,highlighting,export,events,dialogs}/
└─ presentation/{state,views,widgets,theme,shortcuts}/
```

## 8. 体积与性能预算（改动前自查）

| 组件 | 预算 |
|---|---|
| egui + eframe | 3–4 MB |
| pulldown-cmark | ≤ 0.5 MB |
| syntect | ≤ 1.5 MB |
| ropey | ≤ 0.2 MB |
| rfd + serde 等 | 0.5 MB |
| 应用代码 | 0.5–1 MB |
| **总计** | **5–7 MB（红线 10 MB）** |

新增依赖必须登记体积贡献（`docs/dependencies.md`）并经 ADR 追加评审。
