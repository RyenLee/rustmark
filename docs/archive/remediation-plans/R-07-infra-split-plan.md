# R-07 · `rustmark-infra` 大文件拆分方案（只读设计）

> 状态：只读设计，未改动 `crates/` 下任何源码，未提交。
> 基线：`git rev-parse HEAD` = `3991701`，工作树干净（仅 `docs/*.html` 未跟踪）。
> 测量口径：所有行数由 `wc -l` / 源码实测，非凭记忆。

---

## 0. 测量基线（事实先行）

`rustmark-infra` 源码总行数 **11,011** 行。≥400 行的大文件共 9 个，合计 **4,750** 行。
逐文件拆出「生产代码 / 内联测试」比例（测试边界以 `#[cfg(test)] mod tests` 实际行号为据）：

| 文件 | 总行 | 生产 | 测试 | 测试起点 | 备注 |
|---|---:|---:|---:|---:|---|
| `highlighting/incremental.rs` | 853 | 554 | 299 | L555 | `mod tests` @L556；消费 `syntect_highlighter::classify` |
| `fonts.rs` | 659 | 233 | 426 | L234 | `mod tests` @L234；已有 `assembly`/`discovery`/`port` 子模块 |
| `parsing/pulldown_parser/image_size.rs` | 584 | 273 | 311 | L274 | `mod tests` @L275；纯格式探测逻辑 |
| `parsing/pulldown_parser/stack.rs` | 548 | 548 | 0 | — | `OpenBlock` + ~14 个 `pub(super)` helper |
| `persistence/asset_loader.rs` | 441 | 441 | 0 | — | `mod coding; mod policy;`；含平台证书根 |
| `persistence/fs_document_repository.rs` | 437 | 437 | 0 | — | `impl DocumentRepository` 端口实现 |
| `highlighting/syntect_highlighter.rs` | 417 | 417 | 0 | — | `classify` 为 `pub(super)`，被 `incremental.rs` 引用 |
| `parsing/pulldown_parser/end.rs` | 409 | 409 | 0 | — | **单个** `handle_end` 函数，~16 个 `TagEnd::` 分支 |
| `export/viewport_capture.rs` | 402 | 140 | 262 | L141 | 已有 `mod image;`（crop/stitch/write_png）；`mod tests` @L141 |

合计：生产 **3,452** 行 / 内联测试 **1,298** 行（占 27%）。

**纠正一处旧记录**：`fonts.rs` 的测试模块实际从 **L234** 起（非旧记的 L76），故其为 ~233 生产 / ~426 测试，并非「几乎全是测试」。9 文件里 73% 是生产代码，所以「把测试搬到 `xxx/tests.rs`」只是其中一个（且偏小的）杠杆，不能当成主收益。

---

## 1. 设计原则（infra 是实现层，不是应用层）

R-06 抽的是 **core 的纯领域逻辑**（高价值、低耦合）。R-07 拆的是 **infrastructure 的实现代码**——它的本职是适配外部库（syntect / image / 系统字体 API / HTTP / 文件系统）和平台代码。照搬 R-06 的「纯函数提取」打法会踩坑：

1. **保端口边界**：infra 对外只通过 `rustmark_core` 的端口 trait（如 `DocumentRepository`）暴露能力；拆分内部时 `impl XxxPort` 必须保持整体内聚，不要把端口方法打散到子模块。
2. **零新双源**：拆出来的子模块共享状态（如解析器 `OpenBlock`、高亮会话）时，必须「移动而非复制」，单一真源不得分裂。
3. **行为保全优先于行数**：IO / 平台代码（证书根、BOM、draft-id 解析）改了不能靠肉眼验，必须过既有集成测试。
4. **无硬门禁**：本仓库 **没有单文件行数门禁**——`scripts/size_gate.sh` 管的是 release 二进制字节数（红线 11 MiB），`tools/check_thresholds.py` 管的是常量数值基线（121 条）。因此 R-07 纯粹是**维护性/可读性驱动**，没有任何合规压力逼着拆。结论应据此把风险门槛抬高。

---

## 2. Q1 · 边界划分（逐文件拆分维度 + 目标行数 + 拆后模块）

### 2.1 `incremental.rs`（853）
- **维度**：抽两个自由函数 `changed_range`（L506）与 `push_shifted`（L540）——均为纯函数、无状态、不依赖 syntect——到 `highlighting/diff.rs`。
- **不拆**：`SyntectSession` 状态机（L103–472：`new`/`highlight`/`highlight_with_stats`/`last_spans`/`can_reuse`/`run_full`/`append_reused_tail`/`run`/`reset`）是**单一内聚体**，`can_reuse→run_full→append_reused_tail` 的增量复用不变量必须整体保留；`HighlightSession` impl（L473）与 `classify` 调用（L420）留在原位。
- **目标**：`incremental.rs` ≈ 600（会话 + 测试），`diff.rs` ≈ 50（自由函数）。
- **耦合保留**：`incremental.rs:48` 的 `use super::syntect_highlighter::{classify, default_syntax_set, push_span, resolve_language}` 路径不变。

### 2.2 `fonts.rs`（659，已子模块化）
- **维度**：生产门面（`FontSetup`/`font_definitions`/`editor_font_choices`/`install_egui_base_fonts`）仅 ~233 行且已委托给 `assembly`/`discovery`/`port`。**不拆生产**，仅把 L234–659 的 `mod tests` 迁到 `fonts/tests.rs`（`mod tests;` 引入；遵守 CONSTR-03，无 `mod.rs`）。
- **目标**：`fonts.rs` ≈ 233，新增 `fonts/tests.rs` ≈ 426。

### 2.3 `image_size.rs`（584，纯逻辑）
- **维度（两选一，推荐先只做低风险项）**：
  - 低风险：把 `mod tests`（L275–584）迁到 `image_size/tests.rs`。
  - 中低风险（按需）：把各格式解析器抽到 `pulldown_parser/image_size/{png,jpeg,gif,bmp,ico,webp}.rs`，**但格式嗅探 dispatch 必须留在 `image_size.rs` 门面（单一调度源，不得复制）**。
- **目标**：仅迁测试 → `image_size.rs` ≈ 273 + `tests.rs` ≈ 311；若格式拆分 → 门面 ≈ 80 + 各格式模块 20–40。

### 2.4 `stack.rs`（548，解析器内部状态）
- **维度**：~14 个 `pub(super)` helper 都操作同一 `OpenBlock` 栈。可按职责分组：`inline_style.rs`（`current_inline_style`/`current_link_dest`/`push_plain_run`/`push_keycap_frame`/`pop_keycap_frame`/`push_markup_aware`/`flush_inline_text`/`math_body_if_standalone`）、`images.rs`（`shifted_images`/`has_text_outside_images`/`nearest_producer`）、`content.rs`（`push_content`/`split_paired`）。**所有子模块必须显式 `use super::*` 共享栈类型，不得复制栈状态逻辑**。
- **目标**（若拆）：每组合 ~80–150 行。
- **风险**：高（见 §3 / §5）。**默认建议不拆**，除非有具体可读性痛点且配解析器集成测试。

### 2.5 `asset_loader.rs`（441，IO/平台）
- **维度**：抽 `agent`（`http_agent` L368 / `build_agent` L379 / `DEFAULT_AGENT` L349）到 `asset_loader/agent.rs`；抽 `platform_certs`（`PLATFORM_ROOTS` L399 / `platform_root_certs` L417）到 `asset_loader/platform_certs.rs`。`AssetLoader` impl（L64）与既有 `coding`/`policy` 保持不变。
- **目标**：`asset_loader.rs` ≈ 250（loader impl），`agent.rs` ≈ 70，`platform_certs.rs` ≈ 40。
- **保全点**：平台证书根数量/内容必须逐字节不变（有测试保驾）。

### 2.6 `fs_document_repository.rs`（437，端口实现）
- **维度**：抽 `parse_draft_id`（L434，纯）到 `fs_document_repository/draft_id.rs`；抽 `LineEnding` 归一化（L57）到 `fs_document_repository/line_ending.rs`。`impl DocumentRepository`（L260）保持整体。
- **清理**：`const UTF8_BOM`（L53）是 `super::encoding::UTF8_BOM` 的**派生别名**（非独立副本、非双源），改为 `use super::encoding::UTF8_BOM;` 直接引用即可，顺手消除一层间接。
- **目标**：`fs_document_repository.rs` ≈ 300（impl），`draft_id.rs` ≈ 40，`line_ending.rs` ≈ 60。

### 2.7 `syntect_highlighter.rs`（417，数据 + 共享 helper）
- **维度**：抽数据表 `LANGUAGE_ALIASES`（L65）/`STYLE_SELECTORS`（L125）到 `syntect_highlighter/data.rs`；抽 `classify`（L367）/`push_span`（L394）到 `syntect_highlighter/spans.rs`。
- **关键耦合**：`classify` 被 `incremental.rs:48` 以 `use super::syntect_highlighter::classify` 引用。**必须在 `syntect_highlighter.rs` 保留 `pub use spans::classify;`（以及 `push_span`）再导出**，使 `incremental.rs` 的 import 路径零改动；绝不复制 `classify`。
- **目标**：`syntect_highlighter.rs` ≈ 150（高亮器主体 + 再导出），`data.rs` ≈ 120，`spans.rs` ≈ 60。

### 2.8 `end.rs`（409，**不拆**）
- 整文件就是**单个** `handle_end` 函数，~16 个 `TagEnd::` 分支都在变更同一份解析器栈状态。按分支类别拆子模块会在一个有状态处理器上人为划界，极易造成栈变更逻辑分裂/双源。
- **结论：保持整体。**

### 2.9 `viewport_capture.rs`（402，已子模块化）
- 已有 `mod image;`（crop/stitch/write_png）。门面（`PortFrame`/`CaptureError`/`MAX_*`/`capture_region_to_png`）仅 ~140 行。**不拆生产**，仅把 `mod tests`（L141–402）迁到 `viewport_capture/tests.rs`。
- **目标**：`viewport_capture.rs` ≈ 140，新增 `tests.rs` ≈ 262。

---

## 3. Q2 · 风险排序与分批（每批独立可验）

按「风险↑ / 收益↓」排序（实现层风险集中在「共享状态」与「IO/平台」两类）：

| 风险 | 文件 | 主因 |
|---|---|---|
| 高 | `stack.rs` | 解析器共享 `OpenBlock` 状态，~14 helper 牵一发动全身 |
| 高（不拆） | `end.rs` | 单函数、有状态、~16 分支 |
| 中高 | `incremental.rs` | `SyntectSession` 增量复用不变量（但仅抽自由函数则低风险） |
| 中 | `syntect_highlighter.rs` | 已知 `classify` 耦合，需保再导出 |
| 中 | `fs_document_repository.rs` | 端口实现，BOM/draft-id 语义必须保全 |
| 中 | `asset_loader.rs` | 平台证书根 / HTTP agent 行为保全 |
| 中低 | `image_size.rs` | 纯逻辑，但格式探测正确性关键 |
| 低 | `fonts.rs` / `viewport_capture.rs` | 已是薄门面 + 子模块，仅迁测试 |

### 分批顺序（每批独立编译 + `cargo test -p rustmark-infra` 全绿即可验收）

- **Batch 0 — 测试迁移（零生产改动，纯低风险）**
  - `fonts.rs` → `fonts/tests.rs`；`viewport_capture.rs` → `viewport_capture/tests.rs`。
  - 独立可验：测试数量与通过率不变。GREEN checkpoint。
- **Batch 1 — 纯函数 / 数据提取（安全）**
  - `incremental.rs` 抽 `diff.rs`；`syntect_highlighter.rs` 抽 `data.rs`+`spans.rs`（保 `pub use classify`）；`image_size.rs` 抽 `tests.rs`（格式拆分留按需）。
  - 独立可验：每文件编译通过 + crate 测试全绿；`classify` 仍可被 `incremental` 引用。
- **Batch 2 — IO/平台与端口实现（行为保全）**
  - `asset_loader.rs` 抽 `agent.rs`/`platform_certs.rs`；`fs_document_repository.rs` 抽 `draft_id.rs`/`line_ending.rs` + 清 `UTF8_BOM` 别名。
  - 独立可验：FS 往返（写含 BOM 文件→读→字节一致）、draft-id 解析、agent 构建、平台证书根数量不变。
- **Batch 3 — 解析器内部（最侵入，谨慎）**
  - `stack.rs`：仅当有具体痛点且配解析器集成测试时按 §2.4 分组拆；否则保持整体。`end.rs` **不拆**。`image_size` 格式解析拆分（若 Batch 1 仅迁测试）。
  - 独立可验：pulldown 解析集成测试 + 渲染产物冒烟；仓库**无 snapshot/golden 测试**覆盖 `parsing/`，Batch 3 前需先补或靠手动冒烟保驾。

---

## 4. Q3 · 双源 / 耦合风险点排查（含「否证」）

> 纪律：只消灭**真**双源，不为 KPI 硬凑。

1. **`incremental.rs` ↔ `syntect_highlighter.rs` 的 `classify` 耦合（真耦合，非双源）**
   - `incremental.rs:48` 引用 `syntect_highlighter::classify`；`syntect_highlighter.rs:367` 是唯一定义。
   - 拆 `syntect_highlighter` 时若把 `classify` 移到 `spans.rs`，**必须 `pub use spans::classify;` 再导出**，使调用方路径零改动。移动而非复制 → 单一真源保全。
2. **`fs_document_repository.rs:53` 的 `UTF8_BOM`（否证为双源）**
   - 实测为 `const UTF8_BOM: [u8;3] = super::encoding::UTF8_BOM;`——**派生别名**，数值由 `super::encoding::UTF8_BOM` 在编译期推导，源改变量随之变。不是独立副本，**不构成双源**；仅建议改为直接 `use` 引用以消除一层间接。
3. **`stack.rs` 的 `OpenBlock` 状态（潜在新双源）**
   - 拆分时若两个子模块各自维护一份「最近的图片生产者 / 内联样式」推导逻辑，会造出新双源。预防：helper 全部 `use super::*` 共享同一 `OpenBlock`，且 `nearest_producer`/`shifted_images` 只在一处定义。
4. **`viewport_capture` 的 `image` 子模块（已存在，防回归）**
   - `crop`/`stitch`/`write_png` 已在 `viewport_capture/image.rs`。进一步拆分绝不可在别处重造这些像素操作。
5. **`fonts.rs` 门面（防膨胀）**
   - `assembly`/`discovery`/`port` 已子模块化；新字体逻辑应落子模块，门面保持薄。

---

## 5. Q4 · 验收口径（每批可验证）

统一门禁：拆分后 `cargo build`（全 workspace）通过；`cargo test -p rustmark-infra` 全绿；无 `check-arch` 违规（infra 不得反向依赖上层——本方案不新增任何 crate 边界，故不触发）。因**无单文件行数硬门禁**，验收以「可读性改善 + 测试通过 + 行为保全」为准，而非行数达标。

- **Batch 0（测试迁移）**：`cargo test -p rustmark-infra` 用例数与通过率与拆前一致；`git diff --stat` 显示仅测试位置移动。
- **Batch 1（纯提取）**：每个源文件独立 `cargo build` 通过；`incremental` 仍引用 `syntect_highlighter::classify` 且编译通过；`image_size` 行为由既有测试覆盖。
- **Batch 2（IO/平台）**：
  - `fs_document_repository`：写「含 UTF-8 BOM 文件 → 读回」字节一致；`parse_draft_id` 对合法/非法 draft-id 的既有测试通过。
  - `asset_loader`：HTTP agent 构建测试通过；**平台证书根数量与内容逐字节不变**（加断言或核对既有测试）。
- **Batch 3（解析器）**：pulldown 解析集成测试全绿 + 渲染产物手动冒烟；因无 snapshot 测试，Batch 3 启动前建议先补 1–2 个解析 golden 用例兜底。

---

## 6. Q5 · 是否该做（坦率 verdict）

**结论：R-07 部分值得做，且明确有几处不该动。**

- **明确不拆**：`end.rs`（单函数、有状态、整体内聚，拆了只增风险零收益）；`incremental.rs` 的 `SyntectSession` 状态机（增量复用不变量必须整体）；`stack.rs` 的 `OpenBlock` 共享状态（除非有具体痛点且配集成测试，否则保持整体）；`fonts.rs`/`viewport_capture.rs` 的**生产代码**（已子模块化，仅迁测试）。
- **确定性收益、近零风险（推荐优先做）**：Batch 0（测试迁移）+ Batch 1（安全提取）。纯改善可读性，不碰共享状态与 IO 语义。
- **中等收益、中等风险（按需做）**：Batch 2（IO/端口），只要行为保全即可，价值在于让端口实现与平台适配各自清晰。
- **低收益、高风险（建议不拆）**：Batch 3 的解析器拆分，尤其 `end.rs`/`stack.rs`。这不是高价值的架构整改——不像 R-06（抽纯领域逻辑）或 R-03（解耦视图），它只是实现层的文件瘦身，且仓库无行数硬门禁逼着做。

**测序建议**：R-07 不应阻塞主线。R-11（task-marker 识别双方言正本清源，`r11-dedup` 在飞）修复的是**线上可见缺陷**，优先级高于 R-07；R-06 收尾也先于 R-07。R-07 可机遇性地推进 Batch 0+1（低风险、随时可停），Batch 2 看排期，Batch 3 的 parser 部分**建议搁置**。不要为 KPI 硬拆——「建议不拆」的结论比凑数拆更有价值。

---

## 7. 给实施者的硬纪律（沿用本项目已立规矩）

- 移动而非复制：任何 helper/常量/数据表只允许一个定义点，子模块用 `pub use` 再导出承接旧路径。
- 取证姿势：改完跑 `cargo test -p rustmark-infra`；涉及 IO/端口的改 `cargo test` 全 workspace。
- 不新增 crate 边界：本方案全在 `rustmark-infra` 内部，不触发 `check-arch`；若将来有人想把某子模块升 crate，须单独立项（参照 ADR-007 的触发条件）。
- 不提交：本文件为只读设计产物，落地由实施者开 PR。
