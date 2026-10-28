# R-10 评估：拆库 vs 端口升 crate

> 作者：顾图南（架构师）· 2026-10-28 · 依据：docs/remediation/r01-r11/remediation-plan-2026-10-27.md 问题 D
> 性质：纯架构分析，未改动 `crates/` 下任何源码。所有结论基于读代码实测。

---

## 1. 现状违规的具体画像（文件:行号级证据）

### 1.1 端口现在住在哪里

- `rustmark-shared/src/shared/kernel.rs:11-18` 列出 kernel 的 8 个模块：端口 `dialog` / `event` / `system` / `image_asset` / `time` + 值对象 `theme` / `fonts` / `export_format`。kernel 与 support（11 个工具模块）同住一个 crate。
- `rustmark-shared/src/lib.rs:1-23` 自述该 crate 是「共享内核：与层无关的纯值、端口与唯一真源（零 IO）」——即**端口与值对象、工具混在一个「工具箱」里**，端口没有独立身份。
- 五个端口 trait 定义（均为 `Send + Sync`）：
  - `DialogPort` — `shared/kernel/dialog.rs:65`
  - `ExternalOpener` — `shared/kernel/system.rs:40`
  - `ImageAssetStore` — `shared/kernel/image_asset.rs:39`
  - `EventBus` — `shared/kernel/event.rs:75`
  - `Clock` — `shared/kernel/time.rs:58`

### 1.2 组合根如何「直连」它们

- `src/app/bootstrap.rs:54-60` 直接从 `rustmark_core::shared::{dialog,event,image_asset,system,time}` 引入五个端口类型（`DialogPort` / `EventBus` / `ImageAssetStore` / `ExternalOpener` / `Clock` / `SystemClock`）。
- `bootstrap.rs:48-53` 直连 core 的**领域仓储 trait**（`DocumentRepository` / `BufferFactory` / `Exporter` / `PreferencesRepository` / `Workspace` / `WorkspaceRepository`）；`:26-38` 直连 **13 个 infra 具体实现**（`RfdDialogAdapter` / `InProcessEventBus` / `FsDocumentRepository` / `RoutingExporter` / `RopeBufferFactory` / `SyntectHighlighter` / `PulldownMarkdownParser` / `FsImageAssetStore` / `TomlPreferencesRepository` / `VendoredSpellChecker` / `SystemExternalOpener` / `FsWorkspaceRepository` / `AssetLoader`）。
- 装配函数 `assemble_with_image_store`（`bootstrap.rs:273-389`）把 infra 实现直接绑到端口/领域 trait：`Arc::new(InProcessEventBus::new())` 绑 `Arc<dyn EventBus>`（:281）、`FsDocumentRepository` 绑 `Arc<dyn DocumentRepository>`（:289）、`FsWorkspaceRepository` 绑 `Arc<dyn WorkspaceRepository>`（:291）、`RoutingExporter` 绑 `Arc<dyn Exporter>`（:335）。

### 1.3 为什么违反「契约归领域、实现归 infra」

- 端口（架构契约）本应贴着 domain/application 边界、由 infra 实现；现在它们被放进「谁都能依赖的共享工具箱」（`rustmark-shared`），与值对象/工具平级，没有表达「契约是分层边界」这一身份。
- 组合根把「端口 ↔ infra 实现」的配对硬编码在 `bootstrap.rs`（:281/:289/:291/:335），装配逻辑未收敛成边界——这是 R-10 的**两半**：①端口位置错（在工具箱）；②组合根直连（装配散落）。

---

## 2. 「拆库」的精确语义（把含糊说法落为可执行形态）

「拆库」是模糊词，落成三个可执行形态：

### 2.1 1a · kernel 整体升 crate
把 `shared/kernel`（5 端口 + 3 值对象，8 模块）整体抬成 `rustmark-kernel`；`rustmark-shared` 只剩 `support`（11 工具）。

- 依赖方向：`rustmark-kernel` 因端口引用 `shared/support` 的类型（`FilePath` `AppError` `AppResult` `TextRange` `ids`，见 `dialog.rs:24-26` / `image_asset.rs:18-19` / `event.rs:10-12`），故 `kernel → shared`；core / ui / infra / root → kernel。shared 是叶子（仅 serde/thiserror/ahash），**无环**。
- 是否满足原则：**弱**。端口与值对象仍混在一个 crate，只是换名，未把「契约」单独拎出。

### 2.2 1b · kernel 拆成 ports + values 两个 crate
`rustmark-ports`（5 端口）+ `rustmark-values`（3 值对象）；`rustmark-shared` 只剩 `support`。

- 依赖：`ports → shared`、`values → shared`；其余 → ports/values。无环。
- 是否满足原则：比 1a 好——端口独立成 crate。

### 2.3 1c · 端口下沉回 core/domain
把 5 端口移入 `rustmark-core`（如 `core/src/contracts/` 或 `core/src/domain/ports/`）。

- 依赖方向：**不会产生 Cargo 环**。`shared` 是叶子（无回边），端口引用 shared support 类型只让 `core → shared`（本已存在，见 `core/Cargo.toml:30`）；infra 实现端口 `infra → core`（本已存在）；ui / root 经 `core` 用端口（ui→core 已存在）。
- 但有两个真实阻碍：(i) 与 `rustmark-core` 自身定位冲突——`core/Cargo.toml:11` 自述 core「只剩**纯六边形脊柱**」，把端口塞进去违背该身份；(ii) 切断既有路径 `rustmark_core::shared::dialog::DialogPort`（`bootstrap.rs:54` 在用），需在 core 内重桥接，且让 core 既当领域又当契约宿主、职责变糊。
- 附带张力：端口留在 shared 的说辞是「presentation 不得依赖 domain」（`dialog.rs:6`），但 `ui → core` 已成立（`rustmark-ui/Cargo.toml:24`），即 ui 已能看见 core 全量——该约束与现状图存在潜在不一致，值得单列一条记入账册，但不影响本次结论。

---

## 3. 「把端口升 crate」的精确形态

- 新建 `rustmark-ports`（或 `rustmark-contracts`），只放 5 个端口模块 `dialog/event/system/image_asset/time` 及其紧邻值类型（`FileFilter` `ImportedImage` `Timestamp` `DomainEvent` `IMAGE_EXTENSIONS` `is_safe_to_open` 等都在这些文件内，无需动 shared）。
- **公开面**：5 个端口 trait + 上述伴随值类型。
- **谁依赖它**：core（领域/应用服务用 `Clock` / `EventBus`）、infra（实现全部 5 个端口）、ui（预览区用 `ExternalOpener` 等，经 core 或直接）、root（bootstrap 装配）。
- **依赖方向 / 无环**：`rustmark-ports → rustmark-shared`（端口引用 shared support 类型）；shared 叶子 → 全无环。core / ui / infra / root → ports 都是单向。
- **端口是否真的离开 shared 工具箱**：是。shared 退化为 `support`（工具）+ `kernel` 仅剩 3 个值对象（`export_format` / `fonts` / `theme`，已验证 kernel 内值对象**不引用任何端口模块**——grep `use crate::shared::{dialog,event,system,image_asset,time}` 在 kernel 下**零命中**，故抬走端口后值对象干净留在 shared）。
- **组合根直连那一半是否被解决**：**否**。bootstrap 仍 `use rustmark_ports::dialog::DialogPort`，仍内联 `Arc::new(InProcessEventBus::new())` 绑 `Arc<dyn EventBus>`（:281）。位置换了，装配写法没变。

---

## 4. 两者区别对比表

| 维度 | 1a kernel 整体升 crate | 1b kernel 拆 ports+values | 1c 端口下沉 core | **2 端口升 crate** |
|---|---|---|---|---|
| 依赖方向/环风险 | ports+values→shared，无环 | ports→shared、values→shared，无环 | core→shared（本有），无环 | ports→shared，无环 |
| 对「契约归领域、实现归 infra」满足度 | 弱（端口仍混值对象） | 中（端口独立 crate） | 字面满足但 core 职责变糊 | **强（契约成独立边界）** |
| 迁移改动面 | 中（整体挪 + 桥接） | 大（拆两份 + 桥接） | 大（破 `rustmark_core::shared::<port>` 路径 + 重桥接） | **小（只挪 5 端口 + 桥接）** |
| 与 R-02（shared 已二分）衔接 | 推翻 kernel/support 二分、改回整体 | 把 R-02 的 kernel 进一步二分 | 不动 shared 二分，但 core 内部新增 ports 子层 | **顺接 R-02**：shared 仅留 support+值对象，kernel 端口已迁出正合二分原意 |
| 是否顺带解决「组合根直连」半 | 否 | 否 | 否 | **否** |
| 回归风险 | 中（路径大挪） | 高（双 crate 双桥接） | 高（core 身份冲突 + 路径断裂） | **低（改动局限 5 文件 + 两处门禁）** |

---

## 5. 推荐

**R-10 的本意是「契约归领域、实现归 infra」——两半中，端口位置错是主因，组合根直连是次因（且本就是组合根的天职之一）。**

推荐 **方案 2（端口升 crate）**，落地形态 `rustmark-ports`，理由：
- 它把「契约」作为一等公民独立出来，最贴合 R-10 精神，且不污染 core 的「纯六边形脊柱」身份；
- 改动面最小、回归风险最低，且与 R-02 的 kernel/support 二分自然衔接（端口本就该在 kernel 之外）；
- 此前 Problem D 设计里建议的「未来 `rustmark-contracts` crate，置于 core/shared 之下」正对应此形态——本次复核进一步确认方向正确（且纠正了旧记里「下沉 core 会成环」的措辞：实测**无 Cargo 环**，真正阻碍是 core 身份与路径断裂，故独立 crate 更优）。

### 最小可行步骤（方案 2）
1. 新建 `crates/rustmark-ports/`，`Cargo.toml` 依赖 `rustmark-shared` + serde/thiserror/ahash（同 shared）。
2. 迁移 5 端口模块 `shared/kernel/{dialog,event,system,image_asset,time}.rs` → `rustmark-ports/src/`，内部 `crate::shared::*` 改 `rustmark_shared::shared::*`。
3. `shared/kernel.rs` 删这 5 个 `pub mod`，留 `export_format/fonts/theme`。可选 `pub use rustmark_ports::*`，使 `rustmark_core::shared::dialog::DialogPort` 路径不变（零调用点改动）。
4. `rustmark-core` 加 `rustmark-ports` 依赖（`Cargo.toml:30` 附近）；若用桥接，`core/src/lib.rs:40` 增 `pub use rustmark_ports::*;`。`rustmark-infra` 加依赖（实现端口）。
5. **回归地雷①**：`tests/workspace_topology_test.rs:436` infra 期望依赖恰为 `["rustmark-core"]`，加了 `rustmark-ports` 必红 → 改 `["rustmark-core","rustmark-ports"]`（:439 报错文案同步改）。
6. **回归地雷②**：`tools/arch_check.rs` `layer_of` 把 `shared`→`Layer::Shared`（:397），新 crate 不在枚举 → 需加 `Layer::Ports` 并补 `ROOT_FEDERATED_LAYERS`（:2554）/`scan_project` 登记表键 `ports/<file>`；跑 `cargo run --bin check-arch`。
7. 更新 `docs/architecture-constraints.md` §4.5 登记表路径（`shared/kernel/dialog` → `ports/dialog`）。

---

## 6. 顺带指出：只升端口 crate 是否算真正解决 R-10？

**不算完整解决，只解决了一半。**
- 「端口升 crate」消除了 R-10 的 **「端口位置」半**（端口不再在 shared 工具箱），但 **「组合根直连」半完全未动**——`bootstrap.rs` 仍把每个端口/领域 trait 与具体 infra 实现硬编码配对（:281/:289/:291/:335）。这是六边形架构里装配逻辑未收敛的表现。
- 要真正闭环，需**另立专项**把装配从组合根收敛：引入应用层 `Assembler` / 服务定位器 / DI 边界，让 bootstrap 不再逐一点名 13 个具体 impl。但那是比「挪端口」大得多的重构，且 R-10 已明确**延后（P2 非回归、无当前阻断）**。
- 因此结论：**方案 2 是 R-10 的「最小正确半解」，值得立项**；组合根收敛应作为**独立后续项**，按触发条件（如装配逻辑再次膨胀、或第二消费者出现）再启动，本次不应一并做。
