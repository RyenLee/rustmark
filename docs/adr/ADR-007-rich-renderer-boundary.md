# ADR-007 · 富内容渲染引擎的边界：不拆 crate，以窄 facade 为契约

- **状态**：已接受（2026-10-19）
- **决策者**：项目架构负责人
- **关联任务**：T9（优化任务清单 C4-3）、T6
- **关联文档**：`docs/02-工程骨架与分层边界.md`、`src/presentation/rich.rs` 模块文档

## 背景

`math`（LaTeX 子集排版，约 2500 行）与 `mermaid`（图表渲染，约 3300 行）是两个**手写渲染引擎**，此前放在 `src/presentation/views/` 下，与普通视图并列。

T9 的原始诉求是「独立成子 crate 或 feature gate，为将来完整渲染（KaTeX / Mermaid.js）替换留边界」。

动手前先核对了两个事实：

1. **依赖面已经干净**：两个引擎共 5619 行，`crate::` 引用**为零**（不碰 `theme` / `icons` / `state` / `domain`），只用 `egui` 的 `Pos2` / `Rect` / `Vec2` / `Align` / `text::LayoutJob` 五个类型；
2. **公共面已经收窄**：所有实现模块（`layout` / `model` / `parser` / `symbols` / `graph_parse` / `graph_layout` / `primitives` / `class` / `gantt` / `sequence`）都是私有 `mod`，facade 只导出 6 项。

也就是说，T9 想要的「边界」在**逻辑层面已经存在**，缺的是两件事：
① 目录上把「引擎」与「视图」分开；② 把替换契约写成可追溯的决策。

## 决策

1. **下沉目录**：`views/math` → `presentation/rich/math`，`views/mermaid` → `presentation/rich/mermaid`。`views::math` / `views::mermaid` 通过 re-export 保留，既有路径与 import 一行不用改。
2. **不拆 crate**，也不做 feature gate。
3. **边界契约 = facade 的公共项**（见下表）。替换引擎时保持这些签名，`views::preview_view` 不动。

| 引擎 | 排版（纯函数，不碰 egui） | 绘制 | 尺寸 | 错误 |
|---|---|---|---|---|
| `math` | `layout(&str, size: f32, display: bool, Measure) -> Result<MathLayout, MathError>` | `paint(&Painter, Pos2, &MathLayout, f32, Color32)` | `fitted_height` | `MathError` |
| `mermaid` | `layout(&str, size: f32, Measure) -> Result<MermaidLayout, MermaidError>` | `paint(&Painter, Rect, &MermaidLayout, &MermaidStyle)` | `fitted_height` | `MermaidError` |

两个 `layout` 的度量都通过 `Measure = &dyn Fn(&str, f32) -> f32` 注入，
所以「分数比分母高」「矩阵列宽取最宽项」这类排版规则能在无头环境断言——
这也是替换可以「先换排版、保留绘制」的原因。

## 备选方案与代价

| 方案 | 编译收益 | 边界强度 | 代价 | 结论 |
|---|---|---|---|---|
| **A. 窄 facade + 目录下沉**（选中） | 0 | 中（模块私有） | 20 个文件移动、一处 re-export | ✅ |
| B. 拆 2 个子 crate（workspace） | **负** | 强（crate 边界） | workspace 改造 + 门禁改 `--workspace` + `egui` 依赖扩散 | ❌ |
| C. feature gate（`#[cfg(feature = "math")]`） | 0 | 无 | 默认开启则零收益；关闭则功能缺失 | ❌ |

### 为什么「拆 crate 能加速编译」是伪收益

Rust 以 **crate** 为编译单元，且依赖方不做 API 兼容判定：只要依赖的 crate 产出了新元数据，依赖方就整体重编。

拆出去后，改 `math/paint.rs` 一行：

- 现在：重编 `rustmark`（约 40k 行）；
- 拆之后：重编 `rustmark-math`（5.8k 行）**+ 重编 `rustmark`（40k 行）**。

总工作量变大。只有当「存在多个互不依赖的消费者」时，隔离才有意义——本项目没有第二个消费者。

### 为什么不做 feature gate

两个引擎是**核心功能**（预览里的公式与图表）。默认开启 → 零收益；默认关闭 → 用户看不到公式。而体积收益（release 下约百 KB 级）与功能损失不对等，且当前体积问题的根因已定位在 egui/wgpu 升级（`metrics.md` §2.6），不在这里。

## 后果

**正面**

- 「引擎」与「视图」在目录上分开，替换时删一个目录、改一处 `rich.rs` 即可；
- 替换契约进入版本控制的决策记录，不再依赖口头约定；
- 零行为变化：1843 条测试全绿，其中 `presentation::rich::math::tests` 等 12 条引擎测试路径自动跟随。

**负面 / 未解决**

- 边界强度只有「模块私有」，不是编译器强制的 crate 边界。若将来有人在 `rich/math.rs` 里加 `pub mod internal`，守卫不会报警——**这一点靠 review，不靠工具**；
- `egui` 仍是引擎的直接依赖（`paint` 需要 `Painter`）。若将来换成 WebView 渲染，`paint` 会被整体替换，`layout` 的纯函数部分可保留。

## 触发重新评估的条件

满足任一条再考虑物理拆分：

1. 出现第二个消费方（CLI 批量导出、插件系统）；
2. `rich` 需要独立发布或独立版本节奏；
3. 主 crate 编译时间成为日常瓶颈，且**实测**证明拆出去能缩短（而不是变长）。
