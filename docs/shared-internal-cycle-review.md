# shared 内部环 · 立案审查（V11-55）

> **案件性质**：架构债正式立案。本档案登记 R-21 第 2/3 项搁置问题（2026-10-21 `de0d3d8` 实施记录中「解耦需另寻方案，登记待后续评估」项），把「shared 内部循环依赖」从**搁置备注**升格为**可排期的正式案件**。
>
> **审查基准日**：2026-10-21（所有行号 / 计数 / 依赖边均为当日实测，命令见 §6 附录）。

---

## 1. 案件信息

| 字段 | 内容 |
|---|---|
| 案件编号 | **V11-55**（本档案落地后登记入 `docs/v1.1-backlog.md`） |
| 来源 | ① V11-54 评估报告 P1「`shared` 内进程调用」的**残留半案**（进程调用部分已由 R-03 闭环，内部环部分遗留）；② R-21 第 2/3 项搁置（方案前提被实测证伪） |
| 优先级建议 | **P2**（无用户可见影响；纯结构债；有低成本解） |
| 建议工期 | **≤0.5 人日**（方案 A 约 20 行改动 + 1 条对拍测试；守卫项另计） |
| 关联档案 | `docs/architecture-review.md`（V11-54 评估）、`docs/architecture-refactor-plan.md` R-03 / R-21、`docs/architecture-constraints.md` §3 |

---

## 2. 事实审查（实测，非转述）

### 2.1 环的唯一定位

`src/shared/` 现有 **17 个模块**（`src/shared.rs` 的 `pub mod` 声明实测）：dialog / encoding / error / event / export_format / fonts / geometry / ids / image_asset / lru / path / result / system / text / text_format / theme / time。

shared 内部跨模块 import 共 **10 条边**（已剔除测试模块内的 `use super::*`）：

| 模块 | 依赖的 shared 模块 |
|---|---|
| `dialog.rs` | error、path、**text_format** |
| `event.rs` | error、geometry、ids |
| `image_asset.rs` | path、result |
| `result.rs` | error |
| **`text_format.rs`** | **dialog** |

其余 12 个模块（encoding / export_format / fonts / geometry / ids / lru / path / system / text / theme / time / error）在 shared 内部**零依赖**。

**全库唯一的 shared 内部环**：

```
text_format.rs:36  use crate::shared::dialog::FileFilter;
        ↕
dialog.rs:26       use crate::shared::text_format::TextFormats;
```

- 环的 A 边（text_format → dialog）：`text_format.rs:226-227` 的 `TextFormats::file_filter()` 把白名单翻译成对话框过滤器（消费 `FileFilter`）；
- 环的 B 边（dialog → text_format）：`dialog.rs:97` 的 `default_filters()` 派生自 `TextFormats::builtin().file_filter()`（问题 045「唯一出处」的既有设计——原生对话框过滤器清单必须与白名单同源，防「拖放放行 `.csv` 而对话框选不中」的漂移复发）。

两条边都是**正当语义**（不是偶然 import），因此本环**不是错误**，而是两个正当需求互相拉扯的结构后果：白名单要翻译成过滤器（A），过滤器清单要从白名单派生（B）。

### 2.2 方案前提证伪记录（R-21 第 2 项）

方案原文：「`shared/text_format.rs`（5 个使用点**全在 app 层**）下沉到 `src/app/text_format.rs` 或 `src/infrastructure/text_format.rs`」。**实测为假**——三模块使用点全图（`use` 语句级，2026-10-21）：

| 模块 | 使用点（文件:行 → 符号） | 层分布 |
|---|---|---|
| `text_format` | `app/frame.rs:50`（`TextFormats`）、`app/frame/drag_drop.rs:3`（`TextFormats`）、**`presentation/state/sidebar_ui_state.rs:23`（`default_extension`）**、shared 内部 `dialog.rs:26` | app 2 / **presentation 1** / shared 1 |
| `lru` | `application/markdown/service/render_cache.rs:5`、`span_cache.rs:9`、`spell_cache.rs:4`（均 `LruMap`）、**`presentation/views/preview_view.rs:93`（`LruMap`）** | application 3 / **presentation 1** |
| `text` | `application/document/service/stats.rs:22`（`line_count`）、`application/markdown/service/spell.rs:3`（`word_bounds_at`）、**`presentation/views/editor_view/spell.rs:10`（`word_bounds_at`）** | application 2 / **presentation 1** |

**结论**：三模块合计 11 处使用点，presentation 占 3 处。若按方案下沉到 app/application：

- 下沉到 `app` → presentation 的 3 处触发「表现层 → `crate::app`」check-arch 违例（`crate::app::frame` 的窄豁免不覆盖这些符号）；
- 下沉到 `application` → shared 内部的 `dialog.rs:26` 触发「shared → application」违例（依赖矩阵 shared 行全 ✗）；且 presentation 用 application 虽为矩阵明许，会把「纯数据结构」塞进业务层。

> **复核注（2026-10-25，附案① 落档 `constraints.md` §3 时复测）**：上表 11 处
> 使用点计数不变，三处路径漂移不影响结论——`sidebar_ui_state.rs` 随 V11-62
> 批次 2 迁入 `crates/rustmark-ui/`（层名不变），`preview_view.rs` 的 `LruMap`
> 消费点随 TD-02 拆入 `preview_view/image_cache.rs`，`src/shared/` 随 workspace
> 化迁至 `crates/rustmark-core/src/shared/`。§2.1 的「17 个模块 / 10 条边」为
> 基准日快照，现为 **19 个模块 / 11 条边**（现清单以 `constraints.md` §4.5
> 登记表与 §4.6 边集清单为准）。§2.3「shared 内部 import 不在任何机械规则
> 管辖内」亦已作废——2026-10-25 机械守卫
> `tests/shared/internal_cycle_guard_test.rs` 落地（三条判据见
> `constraints.md` §4.6）。

### 2.3 规则覆盖现状（环为何能存在）

- `docs/architecture-constraints.md` §3 依赖矩阵与 `tools/arch_check.rs:150` 的 `SHARED_FORBIDDEN`（已含 `std::process`，R-03 后加入）**只管跨层依赖**；
- **shared 内部模块互相 import 不在任何机械规则管辖内**——check-arch 无环检测，这是环能长期存在的直接原因；
- R-21 验收判据「`ls src/shared/` → 模块数 ≤ 5」已随 R-07（export_format）/ R-09（theme）等落地**自然作废**（现 17 个），本档案不沿用该判据。

### 2.4 关联案件已闭环部分（R-03，排除在本案外）

V11-54 P0「`shared/system.rs` 在共享内核做进程调用」**已由 R-03（M3，`4174a28`）闭环**，实测现状：

- `src/shared/system.rs` 仅剩 `ExternalOpener` trait（端口，零 IO）+ `is_safe_to_open`（纯校验，122 行）；
- 三平台 `Command::new(...).spawn()` 执行端在 `src/infrastructure/system.rs`；
- `SHARED_FORBIDDEN` 已含 `std::process`，机械守卫在岗。

本案**不重复立案**该部分；仅在记录中标注闭环，防止后人误以为 P0 未处理。

---

## 3. 问题定性

| # | 问题 | 定性 | 影响 |
|---|---|---|---|
| 主案 | `dialog ↔ text_format` 双向 import | **真环，低风险**：编译期可见、语义正当、无 IO、不跨层；坏处是 shared 内两个模块无法单独复用/测试隔离，且是「唯一违规样本」——既有环在，后人再长环无心理门槛 | 结构债，无用户可见影响 |
| 附案 | `text_format` / `lru` / `text` 三模块层归属争议 | **非问题**：三模块均为纯函数 / 纯数据结构，与 shared「值对象 + trait 契约 + 纯函数」定位一致；presentation 依赖 shared 为依赖矩阵明许。争议源于方案对使用点分布的误判 | 无 |
| 非案 | `shared/system.rs` 进程调用 | **已闭环**（R-03），见 §2.4 | 无 |

---

## 4. 方案评估

### 方案 A（推荐）：`file_filter()` 移入 dialog.rs

`file_filter()` 的全部调用点实测仅 **2 处**：`dialog.rs:97`（`default_filters`，生产调用）与 `text_format.rs:349`（自身测试）。

- **做法**：Rust 允许同 crate 跨模块写 `impl TextFormats`——把 `text_format.rs:226` 的方法体（约 10 行）移入 `dialog.rs`，`text_format.rs` 删除该方法与 `use crate::shared::dialog::FileFilter`（第 36 行），该方法的单测随移至 `dialog.rs` 测试模块。
- **效果**：A 边（text_format → dialog）消失，B 边（dialog → text_format）保留——`default_filters` 派生自白名单是问题 045 的**有意设计**，单向即可表达。**环破，零新增边、零行为变化**。
- **代价**：约 20 行；1 条对拍测试（迁移后 `default_filters` 内容不变：`grep`/快照级断言）；check-arch 天然放行（shared 内部 import 不在扫描范围，正因如此更需对拍测试兜底语义）。
- **风险**：极低。唯一要注意的是 `TextFormats::file_filter` 的公有 API 路径不变（仍是 `shared::text_format::TextFormats::file_filter`），外部调用方零改动。

### 方案 B：另立第三方模块（星型化）

新建 `shared/filters.rs` 存放 `FileFilter` + 白名单→过滤器转换，text_format 与 dialog 均依赖它。

- **不推荐**：① 新增模块与「shared 瘦身」方向相反；② `FileFilter` 现居 dialog 且被 `image_filters` 等同族函数使用，搬迁要动更多 import；③ 破一个环引入一个新模块，收益/复杂度比差于方案 A。

### 方案 C：机械守卫（独立小项，不作为本环解法）

在 `tools/arch_check.rs` 增加「shared 内部环检测」：扫描 shared 内模块互相 import 并建图，成环即报。

- **价值**：防止未来 shared 内再长环（本环之所以存在，部分原因是「无规则」）；
- **成本**：中（token 扫描需认 shared 内部 import + 建图找环 + 按项目纪律配变异验证）；
- **建议**：作为独立小项排队（可并入未来的守卫加固批），**不与方案 A 混做**——守卫不破环，环不因守卫消失，两件事分开验收。

---

## 5. 审查结论与建议

1. **立案成立**，编号 V11-55，优先级 P2，无阻塞项；
2. **推荐处置 = 方案 A**（≤0.5 人日）：`file_filter()` 移入 `dialog.rs`，破环 + 1 条对拍测试 + 四道门禁；实施时按项目纪律单 commit（`refactor(shared): 破除 dialog ↔ text_format 内部环（依据 V11-55）`）；
3. **附案以「定位声明」收口，不做搬移**：建议在 `docs/architecture-constraints.md` §3 的 shared 行下补一句定位声明——「被跨层复用的纯函数 / 纯数据结构模块合法居 `shared`（现存 `text_format` / `lru` / `text`）；下沉的唯一障碍是 `dialog` 对 `text_format` 的既有依赖，破环后此依赖单向化」——把「为什么不归位」写成规则，避免后人重复提案；
4. **方案 C（守卫）另立小项**，本档案仅登记意向，不并入 V11-55 验收；
5. 本档案替代 R-21 实施记录中的搁置备注，成为该问题的唯一事实源；R-21 记录保持历史原样不改写。

## 5.1 实施结果（2026-10-21，`93127c2`）

**方案 A 已落地，环已破除**：

- `file_filter()` 与 `FILTER_NAME` 从 `text_format.rs` 迁入 `dialog.rs`（跨模块 `impl TextFormats`，同 crate 合法）。字段 `extensions` 私有，实现取扩展名改走既有的 `extensions()` 访问器——**零行为差异**（`default_filters` 内容迁移前后逐字节一致）。
- 迁移后 shared 内部依赖图全单向：`dialog → {error, path, text_format}` / `event → {error, geometry, ids}` / `image_asset → {path, result}` / `result → {error}`，**无环**。
- 测试随实现迁入 dialog.rs 新建的 `tests` 模块：派生判据 `file_filter_is_derived_from_the_whitelist`（原 text_format.rs 测试原文迁入）+ 新增对拍 `default_filters_derives_from_the_whitelist`（直接锚定 `BUILTIN_TEXT_EXTENSIONS` 与 `FILTER_NAME`，防 dialog 侧另立清单；若与 `file_filter()` 结果比对则是同义反复，锚定源头才有牙齿）。
- 变异验证 `mut_v11_55.py`（已清理） **2/2 全捕获**：① `default_filters` 绕过派生改硬编码清单 → 对拍用例红；② `file_filter` 切断派生改硬编码 `&["md"]` → 派生判据红。
- 四道门禁实测：fmt ✓ / check-arch ✓（0 违规）/ clippy 0 lint（`-D warnings`）/ `cargo test --all-targets` **2099 passed / 0 failed / 4 ignored**（2098 + 1 对拍用例）。
- 附案（第 3 条定位声明）与方案 C（守卫小项）维持原建议，未并入本次验收。

---

## 6. 附录：实测命令（可复现）

```bash
# shared 内部 import 边（剔测试 use super::*）
grep -n "use crate::shared::" src/shared/*.rs

# 三模块使用点
grep -rn "shared::text_format\|shared::lru\|shared::text::" src/ --include="*.rs" | grep "use "

# file_filter 调用点
grep -rn "file_filter()" src/ tests/ | grep -v "fn file_filter"

# SHARED_FORBIDDEN 现状
grep -n -A 8 "SHARED_FORBIDDEN" tools/arch_check.rs

# 模块清单
grep "^pub mod" src/shared.rs
```

> 本档案所有行号 / 计数以 2026-10-21 为准；若代码演进后数字变化，以复跑上述命令的结果更新本档案（档案与代码不一致时，**以代码为准并修订档案**）。
