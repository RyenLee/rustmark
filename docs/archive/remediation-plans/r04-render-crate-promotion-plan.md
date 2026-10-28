# R-04 升 crate 实施方案：rich → `rustmark-render`

> 文档类型：ADR 级实施计划（供评审，不立即执行）  
> 关联：`docs/remediation/r01-r11/remediation-plan-2026-10-27.md` §3 R-04  
> 状态：草案（等待主理人拍板是否执行）

---

## 0. 决策摘要

- **触发阈值已满足**：`rich/` 实测 **6,442 行**（math 2,042 / mermaid 3,467 / render 401），超过方案设定的 "5.6k 行" 升 crate 触发线（总含测试已超；非测试 5,520 行，差约 80 行）。提交数触发线（10/年）未达（R-04 共 5 提交）。"或"条件之一成立。
- **端口化已完成**（R-04 步 0–4，`48bb819`→`75fa2ba`）：`rich` 内部已拆为 egui-free 几何 + 唯一 egui 后端。**本方案只做"物理搬出 ui 成独立 crate"，不含任何重写逻辑。**
- **设计目标**：新 crate `rustmark-render` **默认依赖面 egui-free**（唯一 egui 后端走 `feature = "egui"`）；`rustmark-ui` 经 re-export 拼回原路径，**全部消费方零代码改动**。

---

## 1. 现状核验（已实测，非沿用）

| 维度                           | 实测结论                                                                                                                                                                                                                                                                                                                                                     | 证据                                                                                                |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `rich/` 规模                   | 6,442 行（含测试）/ 5,520 行（非测试）                                                                                                                                                                                                                                                                                                                               | `find ... -name '*.rs' \| wc -l`                                                                  |
| egui 使用范围                    | **仅 `render/egui.rs` 有真实 `use egui` 代码**；其余 `rich/` 内 "egui" 出现处均为模块文档注释                                                                                                                                                                                                                                                                                 | `grep -rln "egui" rich/` → 仅 `render/egui.rs` 含 `use egui`/`egui::`；`render/geom.rs` 等仅在 doc 注释提及 |
| `rich/` 对 `rustmark-core` 依赖 | **零**（内部 `use` 仅为 `super::`/`self::` 或 `crate::presentation::rich::render::*` 自引用）                                                                                                                                                                                                                                                                       | `grep "use rustmark_core"` 在 `rich/` 无命中                                                          |
| `rich/` 对 `rustmark-ui` 依赖   | 无外部 ui 项；仅自引用 `crate::presentation::rich::render::*`                                                                                                                                                                                                                                                                                                     | `[A]` grep                                                                                        |
| 消费方（全部在 root crate `src/`）   | `src/presentation/views/preview_view/embedded.rs:5,83,84,119,120,197-212`、`inline/math.rs:11,272,273,278`（`render::{self,HAlign}`、`render::apply_egui`、`render::rgb8_from_color32`、`math::emit`）；`src/presentation/views.rs:27`（`pub use crate::presentation::rich::{math,mermaid}`）；`src/presentation.rs:20`（`pub use rustmark_ui::presentation::rich`） | `[B]` / `[C]` grep                                                                                |
| egui 载体定位                    | `render.rs:18` `mod egui;` + `render.rs:24` `pub use self::egui::{EguiBackend, apply_egui, rgb8_from_color32};` —— **整模块唯一 egui 载体，两行即可 `#[cfg]` 门控**                                                                                                                                                                                                    | `Read render.rs:18-25`                                                                            |
| 是否死代码                        | **否**：被实时预览（数学/图表块）消费                                                                                                                                                                                                                                                                                                                                    | 见上消费方                                                                                             |

> 关键结论：`rich` 是自包含的 egui-free 几何层 + 单一 egui 后端，且不依赖 core。搬出后 `rustmark-render` 的依赖面 = 仅 `optional egui`，无环、无 core 反向依赖。

---

## 2. 目标结构

```
crates/rustmark-render/            ← 新 crate（物理承载原 rich/）
├── Cargo.toml                     ← egui = { workspace = true, optional = true }; 无 default-features 含 egui
│                                  ← [features] egui = ["dep:egui"]
└── src/
    ├── lib.rs                     ← pub mod math; pub mod mermaid; pub mod render;
    ├── math/ ...                  ← 原 rich/math/*（doc 链接改 crate::presentation::rich:: → crate::）
    ├── mermaid/ ...               ← 原 rich/mermaid/*
    └── render/ ...                ← 原 rich/render/*（egui.rs 在 feature 下编译）

crates/rustmark-ui/
└── src/presentation.rs            ← 删 `pub mod rich;`，改：
                                     pub mod rich { pub use rustmark_render::*; }
                                     （保留 rustmark_ui::presentation::rich::* 路径 → 消费方零改）
    [dependencies] += rustmark-render = { path = "../rustmark-render", features = ["egui"] }

root 工作区 Cargo.toml             ← members + default-members 各加 "crates/rustmark-render"
```

**消费方路径不变性证明**：`src/presentation.rs:20` `pub use rustmark_ui::presentation::rich;` → ui 的 `presentation::rich` 现为 `pub mod rich { pub use rustmark_render::*; }` → 解析到 `rustmark_render` 的公共项。`embedded.rs`/`math.rs` 的 `crate::presentation::rich::render::{self,HAlign}` 与 `views.rs:27` 的 `crate::presentation::rich::{math,mermaid}` 全部仍解析成功，**无需改任何消费方源码**。

---

## 3. 具体迁移清单（机械为主）

### 3.1 新建 crate 骨架

- `crates/rustmark-render/Cargo.toml`：`name = "rustmark-render"`、`version` 同其它本地 crate、`edition = "2024"`、`rust-version = "1.99"`、`publish = false`、`[dependencies] egui = { workspace = true, optional = true }`、`[features] egui = ["dep:egui"]`、`[lints] workspace = true`。
- `crates/rustmark-render/src/lib.rs`：`pub mod math; pub mod mermaid; pub mod render;`（保持 `rich` 内部三子模块的公开面）。

### 3.2 物理搬迁

- `git mv crates/rustmark-ui/src/presentation/rich/* crates/rustmark-render/src/`（保留 `math/ mermaid/ render/` 子目录与 `tests.rs`）。
- 删 `crates/rustmark-ui/src/presentation/rich.rs`（原 `pub mod rich;` 那行所在文件），改在 `presentation.rs` 写 re-export（见 3.5）。

### 3.3 路径重写（在 `rustmark-render/src` 内，机械）

- **真实 `use` 语句**（7 处）：`use crate::presentation::rich::render::{...}`（`mermaid/class.rs`、`gantt.rs`、`graph_layout.rs`、`model.rs`、`primitives/emit.rs`、`primitives/paint.rs`、`sequence.rs`、`math/paint.rs`）→ 改 `use crate::render::{...}`（新 crate 内 `render` 为顶层模块）。
- **doc 注释中的 intra-crate 链接**：`crate::presentation::rich::render::egui`（如 `render.rs:16`、`math.rs`、`mermaid/*.rs`）→ `crate::render::egui`。仅影响 rustdoc 锚点，不破坏编译，但建议一并改净（避免 `cargo doc` 警告）。

### 3.4 feature-gate（唯一关键改动，`render.rs:18-24`）

```rust
// 改前
mod egui;
pub mod geom;
pub use self::egui::{EguiBackend, apply_egui, rgb8_from_color32};
// 改后
#[cfg(feature = "egui")]
mod egui;
pub mod geom;
#[cfg(feature = "egui")]
pub use self::egui::{EguiBackend, apply_egui, rgb8_from_color32};
```

- `pub mod geom;` 与 `pub use geom::Rect;` 保持（egui-free，常编）。
- `render.rs` 内定义的 `Pt`/`Rgb8`/`HAlign`/`TextAnchor`/`RenderCommand`/`RenderBackend` trait 全部 egui-free，常编。
- 消费方（`embedded.rs`/`math.rs`）调用 `render::apply_egui`/`rgb8_from_color32` 仅在其启用 `features=["egui"]` 时可用——root 经 ui 传递开启，成立。

### 3.5 ui 侧 re-export

- `crates/rustmark-ui/src/presentation.rs`：删除 `pub mod rich;`（第 12 行），改为 `pub mod rich { pub use rustmark_render::*; }`。
- 新增 `[dependencies] rustmark-render = { path = "../rustmark-render", features = ["egui"] }`。

### 3.6 工作区注册

- root `Cargo.toml` 的 `members`（第 18-25 行）与 `default-members`（第 28-36 行）**各加** `"crates/rustmark-render"`（两者必须同步，否则 `tests/workspace_topology_test.rs::default_members_cover_every_workspace_member` 报红）。

---

## 4. 守卫改造（3 处）

1. **`check_rich_egui_isolation`（`tools/arch_check.rs:1438`）**：
   - 常量 `RICH_DIR = "presentation/rich"`（1406）→ `"render"`（指向新 crate 的 `src` 根下 `render/`）；`RICH_EGUI_BACKEND = "presentation/rich/render/egui.rs"`（1412）→ `"render/egui.rs"`。
   - `Layer::PresentationRich => "presentation/rich"`（345）重映射：因其不再属 ui 子层，改为直接扫描 `rustmark-render` 的 `src/render/`（或删 `PresentationRich` 层、改守卫接收 crate 名参数）。实现时读取 `arch_check` 既有目录遍历逻辑后定点改。
   - 接受准则不变：**除 `render/egui.rs` 外不得引用 egui**。
2. **依赖面拓扑断言**（`tests/workspace_topology_test.rs`，对应 ui Cargo.toml 注释的 `ui_crate_dependency_face_is_egui_plus_core`）：
   - ui 期望依赖集由 `{egui, rustmark-core}` 扩为 `{egui, rustmark-core, rustmark-render}`。
   - 新增 `rustmark-render` 默认面断言：`cargo tree -p rustmark-render --no-default-features` 不含 `egui`（或解析其 `Cargo.toml` 确认 `egui` 为 `optional` 且不在 `default`）。CI 已有 `docs/spec/workspace-topology.expected.txt` 拓扑 diff 兜底。
3. **（可选）新增 `check_render_egui_free_by_default`**：在 `arch_check` 解析 `rustmark-render/Cargo.toml`，断言 `egui` 为 `optional = true` 且未列于 `default` features——把上述第 2 点的本地门禁固化。配 fixture（把 egui 改非 optional → 红）。

---

## 5. 路径 / 文档同步

- **`docs/spec/architecture-constraints.md` §10 crate 地图**：新增 `rustmark-render` 行（依赖面：默认无 / 仅 `egui` feature）；更新 `rustmark-ui` 依赖面（egui + core + render）；注明 render 主面 egui-free。
- **`docs/spec/thresholds.baseline.yaml`**：被搬迁文件的 `source:` 路径由 `crates/rustmark-ui/src/presentation/rich/...` → `crates/rustmark-render/src/...`（**`--fix` 只回行号，路径必须手改**）；涉及 `via` 路径同步；跨 crate 搬迁的 3 件套（field_visibility 白名单 / source / §10）照 R-09 纪律执行。
- **整改计划 `docs/remediation/r01-r11/remediation-plan-2026-10-27.md` §3 R-04**：状态补「升 crate ✅ 落地（commit X）」；触发线说明（行数已越 5.6k）。
- **`docs/plans/PROGRESS.md`**（若存在）回填实测数字。
- root `Cargo.toml` 第 166-195 行注释（"rich / shortcuts / widgets 在 V11-62 抽出的 rustmark-ui"）补一句：rich 已于 R-04 升 `rustmark-render`。

---

## 6. 七门禁与体积预算

- `cargo fmt --check` / `cargo clippy --workspace --all-targets --all-features -- -D warnings` / `cargo run --bin check-arch` / `python tools/check_thresholds.py`（121 条） / `cargo test --workspace --all-targets --no-fail-fast` / `cargo test --workspace --doc` / `bash scripts/size_gate.sh`。
- **体积预算**：拆 crate 预期 **+2~3 KB**（LTO 边界，见整改计划风险表）；ui 侧仅 `pub use` re-export 不进 release 产物。**实测记录实际增量**，若超 3 KB 需解释（不应超，因纯搬运）。

---

## 7. 风险与缓解

| 风险                                                | 影响                                          | 缓解                                                                |
| ------------------------------------------------- | ------------------------------------------- | ----------------------------------------------------------------- |
| `render.rs:24` 的 `pub use self::egui::*` 漏 cfg 门控 | egui 变硬依赖，`rustmark-render` 默认面含 egui，违反验收点 | 3.4 两行同步加 `#[cfg(feature="egui")]`；拓扑门禁（§4.2）捕获无 egui 构建          |
| 某 non-egui 文件误 `use render::egui::*`              | 无 egui feature 时编译红                         | 门禁 `--no-default-features` 构建（或 CI cargo tree）捕获                  |
| `rich` 实际依赖 core（假设错误）                            | 搬出后 `render → core`，与 `core → ?` 可能成环       | 已实测 `rich` 零 core 依赖（§1）；迁后 `cargo tree` 复核无环                     |
| 消费方路径漂移                                           | 预览渲染失效                                      | 消费方经 ui re-export，路径不变（§2 证明）；迁移后跑 preview 集成测试 + `startup_smoke` |
| `autotests=false` 下新 crate 测试入口未登记                | 测试静默缺席全绿                                    | 新 crate 若有 `tests.rs` 随 `git mv` 带入；`test_target_guard` 守卫拦截遗漏登记  |



---

## 8. 提交粒度（增量，SOP：message 写文件 + `git commit -F`，门禁数字进 message，不 push）

- **commit A（搬迁 + 路径/feature 改写）**：新建 `rustmark-render` crate、搬 `rich/`、`§3.3/3.4` 改写、新 crate 独立门禁绿（`cargo test -p rustmark-render`、feature on/off 双构建）。
- **commit B（接线 + 守卫）**：ui re-export（§3.5）、workspace 注册（§3.6）、拓扑/egui-isolation 守卫改造（§4）、`cargo tree` 复核无环、七门禁绿。
- **commit C（文档回填）**：constraints §10 / thresholds source 路径 / 整改计划 R-04 状态 / PROGRESS（§5）、七门禁绿。

---

## 9. 决策点与回滚

- 触发线虽已越 5.6k 行，但升 crate 仍属 **P2 / 可选 / 非阻断**。执行与否最终由主理人拍板。
- 若执行中出现非预期：依赖环、体积增量超 3 KB、或预览回归无法快速定位 → 可回退到"维持延后"状态（git revert 三个 commit 即可），不损害现有架构。
- 回滚后 `rich` 仍在 ui 内（端口化已完成，egui-free 几何层价值保留），仅不独立成 crate。
