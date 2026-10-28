# ADR-010 · `presentation` 内部依赖方向与集成测试的库内部视角禁令

- **状态**：已接受（2026-10-20）
- **决策者**：项目架构负责人
- **关联任务**：V11-54a R-02 / R-13
- **关联文档**：`docs/spec/architecture-constraints.md` §4.0 / §9、
  `tools/arch_check.rs`、`tests/arch_violation_fixture.rs`、`ADR-007`、`ADR-008`

## 背景

架构评估（V11-54）把「表现层内部没有依赖规则」列为 P1：`presentation` 有 123 个
文件、7 个子层（`views` / `state` / `rich` / `theme` / `widgets` / `shortcuts` /
`icons`），彼此之间**没有任何机械约束**。同时 `tests/` 完全不在扫描范围外——
它是全部架构规则的盲区。

两件事分开做，但共用同一个机制（R-01 重写后的词法级判定）：

- **R-13**：`presentation` 内部方向单向化并机械化；
- **R-02**：`tests/` 纳入扫描，禁「库内部视角」。

## 决策

### 1. `presentation` 内部：四条方向，全部单向

```
views ──► state ──► (domain DTO，经 application)
  │
  └──► theme / widgets / icons / rich
rich / widgets / icons ──► （不得回指 views / state / theme / widgets）
```

| 子层 | 额外禁止 |
|---|---|
| `presentation/state/` | `crate::presentation::views` |
| `presentation/rich/` | `crate::presentation::views` / `state` / `theme` / `widgets` |
| `presentation/widgets/` | `crate::presentation::views` |
| `presentation/icons.rs` | `crate::presentation::views` |

`views/` 可读 `state/`（那是它存在的理由），故不在禁止清单里。四个子层全部
**叠加**父层禁令，不是替换。

### 2. 层判定细粒度化，根文件与目录同规则

`layer_of` 取路径第二段判子层，覆盖两种形态：
`presentation/state/foo.rs` 与 `presentation/state.rs`（层根文件）的第二段
都是 `"state"`。这正是 R-01 修掉的同一类缺陷的镜像——`src/domain.rs` 曾因
「按首段判、首段是 `domain.rs`」整份逃过检查。

### 3. 窄豁免跟着子层细化走

`views/` 独立成层后，`crate::app::frame` 窄豁免（4.2）必须同步扩展到它，
否则 **35 个视图文件同时报错**（实测：全是 `use crate::app::frame::EditorApp`
与 `frame::PendingDialog`）。这不是「更严的架构」，而是规则没跟上自己的细化。
**窄前缀本身不变**——`bootstrap` / `crash_log` 等仍拦。

### 4. `tests/` 判 `tests` 层，只禁库层名

测试是消费者不是层：`tests/domain/`、`tests/presentation/` 是**测试目标的
划分**，与 `src/` 的层毫无关系；按目录名判层会让 `tests/presentation/main.rs`
误套表现层禁令。

禁令只列库层名（`crate::app` / `domain` / `application` / `infrastructure` /
`shared` / `presentation`），天然放行 `crate::support` / `crate::golden_fixture`
这类**测试自身**的共享模块（全仓 243 处）。

## 后果

**收益**：

- `state/` 回指 `views/` 会当场报错。这类环不产生编译错误，且以「状态被视图的
  渲染副作用污染」的形式发作——立即模式框架下极难定位。
- `rich/` 不得读 `state/` / `theme/`：「排版结果随 UI 状态变化」被挡在门外
  （同一段文档在侧栏开着与关着时排版不同，这种差异不报错也看不出原因）。
- `tests/` 不再是规则盲区：从 `src/` 复制粘贴过来的 `crate::domain::…` 会被拦。
- 帧预约唯一出口（ADR-009）首次罩住测试侧——测试里直接写
  `request_repaint_after` 一样会造出「看起来在测、其实绕过 scheduler」的假绿。

**代价**：

- `layer_of` 从「取首段」变成「首段 + 第二段匹配」，判定逻辑复杂度上升。
  用 8 个注入用例 + 8 轮变异验证覆盖（含层根文件形态）。
- 报告里 `tests/` 侧文件带 `tests/` 前缀，与 `src/` 侧路径形式不同。
- **存量实测为 0 处真实违规**（`state/`→`views` 命中 2 处全在文档注释，
  `rich/`→`views` 1 处也在注释里）——即本次改造**不改变任何编译结果**。
  它的价值与 `PRESENTATION_STATE_FORBIDDEN` 相同：防未来松动。

**已知不覆盖**：

- `theme/` 与 `shortcuts/` 未细粒度化（当前无反向依赖问题，且 `theme` 被
  `rich/` 禁引——方向已经由 rich 那条规则保证）。若将来出现
  「`theme/` 引用 `views/`」的需求，需先改本文档与校验器。

## 备选方案

| 方案 | 否决理由 |
|---|---|
| 用 Rust 模块可见性（`pub(super)`）表达方向 | 编译期保障最强，但 `views` 与 `state` 同属 `presentation`，`pub(crate)` 已是最细粒度；要真生效得把 `state/` 拆成独立 crate，代价与收益不成比例 |
| 只做文档约定，不机械化 | 本项目已有两次「约定被无声违反」的历史（`check-arch` 4 类绕过形态、`persisted_key` 漏加不报错），约定不入校验器等于没有 |
| `tests/` 按目录名判层 | `tests/presentation/` 会误套表现层禁令——误报 35+ 处，规则直接失去可信度 |
