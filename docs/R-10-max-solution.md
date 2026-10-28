# R-10「最大解」评估：同时收口两半的形态对比

> 作者：顾图南（架构师）· 2026-10-28 · 依据：docs/architecture-remediation-plan-2026-10-27.md 问题 D
> 性质：纯架构分析，未改动 `crates/` 下任何源码。所有结论基于读代码实测。
> 配套文档：docs/R-10-evaluation.md（拆库 vs 端口升 crate 评估）。
>
> 状态：**项 A（M0）✅ `0f20bc8`；项 B（M3）✅ 本次落地** —— 见 §5 最小可行路径 2~3 步。

---

## 0. 现状装配面画像（读码实测）

- `Services` 聚合结构体在 `src/app/bootstrap.rs:154-195`，字段含 5 个端口 `Arc<dyn …>`（dialogs/opener/images 等）+ 5 个领域服务 + registry/render_cache/assets 等。
- 装配是**级联 `with_X` 链**（`bootstrap.rs:199-389`）：`assemble`（:210）→ `assemble_with_dialogs`（:222）→ `assemble_with_clock_and_dialogs`（:240）→ `assemble_with_opener`（:253）→ `assemble_with_image_store`（:273）。13 个具体 impl 散落内联点名：链入口 `RfdDialogAdapter`（:211）/ `SystemClock`（:226）/ `SystemExternalOpener`（:245）/ `FsImageAssetStore`（:264），deepest 函数里 `InProcessEventBus`（:281）/ `TomlPreferencesRepository`（:288）/ `FsDocumentRepository`（:289）/ `FsWorkspaceRepository`（:291）/ `RopeBufferFactory`（:294）/ `PulldownMarkdownParser`（:295）/ `SyntectHighlighter`（:324）/ `VendoredSpellChecker`（:332）/ `RoutingExporter`（:335）/ `AssetLoader`（:384）。
- 调用方：`src/app.rs:14` `use bootstrap::{ConfigPaths, Services};`——**组合根（root crate）持有 `Services` 并触发装配**。
- `bootstrap.rs:3-7` 自述「本文件是整个代码库**唯一**允许同时看见四个层的地方」——即「组合根直连 impl」是**被设计许可的本职**，不是越界。

**当前本地依赖边（实测，9 条无环）**
```
core  → {actions, shared}
infra → core
ui    → {core, render}
root  → {core, infra, icons, ui}
shared → (叶子：serde/thiserror/ahash，零 workspace 依赖)
```
红线：`core → infra` **不存在且被禁**（`tests/workspace_topology_test.rs:439`「多一条即层级倒置或成环」）。

---

## 1. 四种形态 + 依赖方向图（含 core→infra 红线验证）

### M0（对照·方案 2 半解）
只建 `rustmark-ports`（收 5 端口），bootstrap 不动。
```
ports → shared;  core/ui/infra/root → ports（经 core 桥接）
```
解决半 A（端口离 shared 工具箱）；半 B 完全未动。无环。

### M1 · 端口 crate + 装配模块下沉 core/application
**陷阱成立**：若 `core/src/application/composition.rs` 引用 infra 具体 impl（`FsDocumentRepository` 等），则新增 `core → infra`：
```
core → {actions, shared, infra}   ← 新增边
infra → core                      ← 既有边
⇒ 编译期成环，且撞 topology:439 红。FORBIDDEN。
```
**安全形态**（装配模块只依赖端口/仓储 trait + core 自身，impl 由外注入）：core 暴露 `compose(bundle) -> Services`，`bundle` 是已构造好的 `Arc<dyn …>` 集合，由 root 在外面造好传入。
```
core → {actions, shared, ports}   (无 infra)
infra → core
root → {core, infra, icons, ui, ports}   (root 造 impl 再调 core::compose)
```
无环。但 **root 仍点名 13 个 impl**——半 B 只是把函数签名挪进 core，点名行为没消失 = **伪收敛**。

### M2 · 端口 crate + 独立装配 crate
新建 `rustmark-assembly`（依赖 `core+infra+ports`），root 只 `rustmark_assembly::assemble(config)`。
```
ports      → shared
assembly   → {core, infra, ports}
infra      → core
root       → {core, infra, icons, ui, ports}
```
无环（assembly→core+infra，未新增 core↔infra 边）。**但**：root 在 bootstrap 之外仍大量用 `crate::infrastructure`（`frame.rs:163/165`、`frame/capture.rs:4/332/355/387/389`、`frame/loop.rs:250`、`frame/settings.rs:402`、`frame/state.rs:370`）→ **root 不能丢 infra 依赖**，M2 不缩减 root 依赖面。半 A 解决；半 B：13 个 impl 名从 bootstrap 平移到 assembly crate = **平移而非收敛**。

### M3 · 端口 crate + core 内 DI 边界（不新增装配 crate）
core 暴露 `AppServices` 聚合 + 类型化注入接口（**只依赖端口与 core 自身，绝不 `use rustmark_infra`**）；root 造 impl → 注入 → `resolve() -> Services`。
```
core → {actions, shared, ports}   (无 infra)
infra → core
root → {core, infra, icons, ui, ports}   (root 仍造 impl 并注入)
```
无环。半 A 解决；半 B：**类型化注入边界 + 可去级联脆化**（把 4 级 `with_X` 链换成「造 bundle→注入→resolve」单点），testability/replaceability 实质提升，但 root 仍点名 impl = **部分真收敛**。

> M1 安全形态 ⊆ M3 的 core 侧；M1 危险形态 = 成环。故「装配进 core」只有 M3 这一种安全写法。

---

## 2. 是否真正闭合两半（伪收敛判定）

| 形态 | 半 A 端口位置 | 半 B 组合根直连 | 本质判定 |
|---|---|---|---|
| M0 | ✅ 解决 | ❌ 未动 | 仅半解 |
| M1（危险） | ✅ | ❌ + **成环** | 不可行 |
| M1（安全） | ✅ | ❌（root 仍点名） | 伪收敛 |
| M2 | ✅ | ⚠️ 平移到 crate | 伪收敛（仅第二消费者可复用是真实收益） |
| **M3** | ✅ | 🟡 类型化边界/去级联（root 仍点名） | **最接近真收敛** |

**结论**：半 B 的「强闭合」（root 零 impl 名）在六边形架构下**不可达**——必然要求某个持有 infra 依赖的东西去点名 impl，而那东西按定义就是组合根。所以任何方案都只能让 root「薄 + 类型化」，不能「空」。M2 只是把点名平移（伪），M3 才动到装配逻辑的**结构**（单点注入、可测、可替换）。

---

## 3. 与现有守卫/门禁的冲突（改动点清单）

- `tools/arch_check.rs`：`layer_of` 把 `shared`→`Layer::Shared`（:397）；新增 `rustmark-ports` 需在枚举加 `Layer::Ports` 并补 `ROOT_FEDERATED_LAYERS`（:2554，当前 `["domain","shared","application"]`）与 `scan_project` 登记表键 `ports/<file>`；跑 `cargo run --bin check-arch`。
- `tests/workspace_topology_test.rs`：**M0/M1/M2/M3 都会触发** `infra` 期望依赖 `:436` 由 `["rustmark-core"]` 改为 `["rustmark-core","rustmark-ports"]`（:439 报错文案同步）；**M2 额外**需为 `rustmark-assembly` 新增断言（期望 `[core, infra, ports]`）；`default_members_cover_every_workspace_member`（:70）随 `members` 列表自动校验新 crate。
- `docs/architecture-constraints.md` §4.5 登记表路径 `shared/kernel/dialog` → `ports/dialog`。
- `core/src/lib.rs:40` `pub use rustmark_shared::shared;` — 若用桥接保 `rustmark_core::shared::<port>` 路径不变，需增 `pub use rustmark_ports::*;`。

---

## 4. 改动面与回归风险排序

| 形态 | 新增 crate | 核心改动 | 回归风险 |
|---|---|---|---|
| M0 | +1 (ports) | 挪 5 端口 + 桥接 + 2 处门禁 | **低** |
| M1 危险 | +1 | —（成环，不可落地） | 致命 |
| M1 安全 | +1 | core 增 compose 壳 + root 仍点名 | 中（收益≈0） |
| M2 | +2 (ports+assembly) | 搬 bootstrap 内容 + root 改 caller + 新拓扑断言 | 中（平移，无结构收益） |
| **M3** | +1 (ports) | core 增 DI 边界（禁 infra 导入！）+ 重写 assemble 链 + 保测试入口 | 中（动公共 API） |

**M3 相比 M0 的额外回归地雷**：
1. `core/src/application/composition.rs` 必须审计**永不 `use rustmark_infra`**（否则悄悄成 M1 危险态）；
2. `bootstrap.rs` 的 4 级 `with_X` 公共 API（`assemble_with_dialogs` / `assemble_with_clock_and_dialogs` / `assemble_with_opener` / `assemble_with_image_store`）被测试直接调用——M3 改写装配链时**必须保留或重定向这些测试入口**，否则测试大面红；
3. `Services` 结构体位置若随装配搬动，`frame/*.rs` 的 `self.services.*` 字段访问不受影响（按字段用），但 root `src/app.rs:14` 的 type import 要改指向。

---

## 5. 推荐 + 触发条件 + 最小可行落地路径

**「组合根直连」算不算真违规？——不算。** 它是组合根本职（`bootstrap.rs:3-7`）。半 B 真正该收敛的是三件事（非「让 root 别点名」）：
- (a) **级联脆化**：`bootstrap.rs:251-252` 自认「给中间层加参数会一次性改动所有测试调用点」——每加一个可注入端口就深一层、所有测试调用点跟着改；
- (b) **装配可测/可替换**：想换一套 infra（mock / 第二消费者）时，root-only 装配无法复用；
- (c) **类型化清晰度**：散落字段赋值不如单点注入边界清晰。

**推荐**：把 R-10 拆成两个独立、按触发条件各自启动的项——
- **项 A（M0，端口 crate）**：现在就可做，便宜低风险，解决半 A。
- **项 B（M3，core DI 边界）**：解决半 B 的实质部分（去级联 + 可测/可替换），**不新增装配 crate**，不碰 `core→infra` 红线。
- **M2（装配 crate）明确 YAGNI**：直到出现**第二消费者**（无头 CLI / wasm / 独立测试 harness）需要复用装配而不拉 root 时才做——届时 M2 的「平移」才变成真实收益（装配可独立调用）。

### 最小可行「最大解」落地（项 A + 项 B，不含 M2）
1. 建 `rustmark-ports`（收 5 端口，`ports → shared`），见 R-10-evaluation.md §5 步骤 1–3。
2. `core/src/application/composition.rs`：定义 `ImplBundle { dialogs: Arc<dyn DialogPort>, clock: …, opener: …, images: …, repositories: …, … }` + `compose(bundle, config) -> Services`；**只 `use` 端口/仓储 trait 与 core 自身**。
3. 重写 `bootstrap.rs`：保留 `assemble` / `assemble_with_dialogs` 等**测试入口签名**，但内部改为「root 造 impl → 装 bundle → `core::application::compose` → `Services`」，消除 4 级 `with_X` 链。
4. 门禁三连改：topology `:436` 加 `rustmark-ports`；arch_check 加 `Layer::Ports` + `ROOT_FEDERATED_LAYERS`；constraints §4.5 路径。
5. 回归：跑 `cargo test` 全套（重点 `assemble_with_*` 测试入口）+ `cargo run --bin check-arch` + topology test。

### 触发条件（R-10 当前延后 P2 非回归）
- 项 A：想拿整洁度即做（已推荐）。
- 项 B（M3）：当且仅当 (i) 级联再次脆化、(ii) 要单测装配本身、(iii) 需要类型化注入清晰度。
- M2：当且仅当**第二消费者出现**。

**一句话**：最大解 ≠ 「让 root 不点名 impl」（那不可能）；最大解 = **端口独立成 crate（半 A）+ core 类型化注入边界（半 B 的实质）**。M1 危险态会成环，M2 是伪收敛，M3 才是真收敛的正确形状。
