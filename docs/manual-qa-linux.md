# S7-05 三平台手工回归 · Linux

> 阶段 07 · 集成联调与原型对齐 · 任务 S7-05
> 平台：**Linux（X11 / Wayland，需真机，本机无法自检）**
> 对应自动化覆盖见 §5；必须看屏幕 / 真实窗口 / 原生行为的部分进本清单。
> **最近同步：2026-10-09**，按中文字体实测结论重写 §2.4，并新增 §2.5a 文件编码、§2.5b 视口截图。

## 0. 适用范围

- 本清单覆盖 Linux 特有行为：X11 vs Wayland、fontconfig CJK 回退、GTK 原生对话框、CUPS 打印、桌面主题集成。
- 每条标注：**优先级**（P0/P1）、**是否需截图**、**判定标准**。
- 自动化已覆盖的项见 §5，手工不再重复。

## 1. 环境准备

1. 在**真实 Linux 桌面**上测试：至少覆盖一种 X11 会话（如 Xorg + GNOME/KDE）与一种 Wayland 会话（如 GNOME-on-Wayland）。
2. 准备中文目录：`/home/<你>/文档/Markdown 笔记/你好世界/`。
3. 确认字体：`fc-list | grep -i "noto.*cjk\|wenquanyi\|source han"` 至少有一个 CJK 字体；否则中文会是豆腐块。**注意：应用不依赖 fontconfig 回退，而是直接读字体文件**（见 §2.4），故须确认候选路径实际存在：
   ```bash
   ls /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
      /usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf \
      /usr/share/fonts/truetype/wqy/wqy-zenhei.ttc \
      /usr/share/fonts/truetype/wqy/wqy-microhei.ttc \
      /usr/share/fonts/truetype/arphic/uming.ttc 2>/dev/null
   ```
4. 确认 CUPS：`which lp lpstat`。无 `lp` 时验证 PDF 导出的 HTML 回退提示。
5. 准备一个 **GBK 编码**的中文 `.md`（§2.5a 需要）。

## 2. 回归项

### 2.1 DPI / 缩放（P0，需截图）

- **MQ-L-01** X11 下 100%：文字清晰。
- **MQ-L-02** Wayland 下 Fractional Scaling（如 125% / 150%）：窗口不模糊、布局正确（`pixels_per_point` 由 eframe 读原生）。
- **MQ-L-03** X11 高 DPI（`xrandr --dpi 192` 或修改缩放）：重启应用后字号正确。X11 的整数缩放与 Wayland 分数缩放行为不同，两者都验。
- **MQ-L-04** 应用运行中切换显示缩放：跟随，无黑边/重叠。

### 2.2 中文路径 / 非 ASCII 文件名（P0，需截图）

- **MQ-L-05** 在 `…/你好世界/` 下保存 `你好.md`：成功，重开路径显示正常。
- **MQ-L-06** 路径含空格：打开 / 另存为不报错。
- **MQ-L-07** 中英混合文件名：最近文件 / 标签页正常。
- **MQ-L-08** 含 BOM 中文文件：首行无不可见字符。BOM 由 `infrastructure/persistence/encoding.rs` 的 `decode` 统一识别并剥离（同时处理 UTF-16 LE/BE BOM，见 §2.5a）。

### 2.3 修饰键 / 快捷键（P0，需截图）

- **MQ-L-09** `Ctrl+N/O/S/Z/Shift+Z/F/H` 全部触发（Linux 同 Windows，物理 Ctrl）。
- **MQ-L-10** 快捷键一览 / 命令面板 / 菜单三处键位一致显示 `Ctrl+…`。
- **MQ-L-11** `Ctrl+Shift+8 / 7`（列表）生效（S6-07 补齐项）。
- **MQ-L-12** 单键 `F8` / `Esc` 无修饰键可触发。
- **MQ-L-13** 改键后重启生效，旧键不残留。

### 2.4 字体 / CJK 渲染（P0，需截图）

> **实测结论（务必按此判断，勿凭直觉）**：egui 0.33 内置字体族**不含任何 CJK 字形**，
> 所以「Linux 上 `system-ui` 会经 fontconfig 自动回退到 Noto / 文泉驿」这一旧假设**不成立**
> ——egui 的字体回退是**在已装载的字体内找字形**，fontconfig 参与与否不影响 egui 自己的字体族。
> 不显式加载中文字体时，界面中文**必然**是占位方框（`□`）。
> 现由 `infrastructure/fonts.rs` 在首帧前依次探测 §1.3 列出的 5 个路径，
> 读到的第一个文件**追加到字体族末尾**。**这些路径未在开发机验证过**，本节即首次真机确认。

- **MQ-L-14** 界面所有中文（菜单、工具栏、侧栏、状态栏、弹窗）显示清晰，无豆腐块（`□`）。**逐屏翻一遍**，不要只看编辑区。
- **MQ-L-14b** 状态栏、标签页标题、导出面板等**次要位置**中文同样正常。这两处字体族不同（比例族/等宽族），只验一处可能漏掉「等宽族漏挂 CJK」。
- **MQ-L-14c** 代码块内中文注释：等宽对齐不被破坏。若缩进错位，说明 CJK 字体被插到族**开头**（接管西文字形、破坏等宽），而非追加末尾。
- **MQ-L-14d** 工具栏图标（`√` / `×`）与符号面板全部条目有字形、不成方块。历史问题码位 `✓(U+2713)`、`✕(U+2715)`、`✗(U+2717)` 无字形，已换为 `√(U+221A)` / `×(U+00D7)`；符号面板另有 `▤→≡`、`▦→▣`、`ⓘ→ℹ`、`⏎→↩`、`⇒→↔`、`❝→"`、`🤔→💭`。
- **MQ-L-14e** **缺 CJK 字体时的退化（本平台重点）**：把 §1.3 全部候选路径临时改名后启动——
  应用**必须正常启动**、西文与 emoji 正常、**不崩溃**；stderr 打印一条明确警告（含安装建议）。
  此时中文显示为方框属**已知退化**，不算缺陷；但**静默无提示**算缺陷。
- **MQ-L-15** 字号 20px：编辑器 / 预览同步放大，行高保持。
- **MQ-L-16** 中文输入（IBus / Fcitx5）：候选框不遮挡，composition 不破坏文档。

### 2.5a 文件编码（GBK / BOM）（P0，需截图）

> 编码探测与解码是**平台无关的纯逻辑**，三平台共用 `infrastructure/persistence/encoding.rs`
> 与同一组测试。自动化已覆盖（见 §5），本节仅作真机抽样确认。

- **MQ-L-26** 打开 GBK 编码的中文文件：中文完全正确，无 `�`、无 `锟斤拷`。
- **MQ-L-26b** GBK 与 ASCII 混排段落不丢字符（GBK 尾部跟 ASCII 字母被吞即为回归）。
- **MQ-L-26c** 保存后重开：内容一致，文件转为 UTF-8（已知限制，见 §4）。状态栏编码项显示 `UTF-8` 且不可点击。
- **MQ-L-26d** 导出 HTML 在浏览器打开：`meta charset="utf-8"` 在 `<head>` 首部，中文正常。

### 2.5b 视口截图导出（P1，需截图）

- **MQ-L-27** 导出面板选 PNG 得到可正常打开的图片，非空白非全黑；内容与当前预览区所见一致（含中文与代码块高亮）。
- **MQ-L-27b** X11 与 Wayland **两种会话各测一次**：截图均正常。Wayland 下注意是否存在合成器抓屏限制导致的失败或黑图。
- **MQ-L-27c** 预览区被窗口裁切时截图：与所见一致，不崩溃、不出半张图。
- **MQ-L-27d** 导出面板中「范围＝预览区在窗口中的可见部分」的说明文字存在且准确（PNG 不是整篇长图）。

### 2.5 打印 / PDF 导出（P1，需截图）

- **MQ-L-17** 导出 PDF：探测 `lp`（优先）/ `lpstat`。有 CUPS 时走系统管线；**仅装 `lpstat` 无 `lp`** 时返回「请用浏览器打印该 HTML」的提示（`system_pdf_exporter.rs` 的 `backend_hint_is_stat_only` 分支）。
- **MQ-L-18** 导出 HTML：含 UTF-8 声明与内联样式，浏览器打开中文不乱码。

### 2.6 文件对话框（原生 GTK，rfd）（P1，需截图）

- **MQ-L-19** 打开 / 另存为弹 **GTK** 原生对话框，过滤器只列 `.md`。
- **MQ-L-20** 取消：返回 `Cancelled`，不触发「无名文件保存」。

### 2.7 大文件降级观感（FPS）（P1，需截图）

- **MQ-L-21** 1MB 文件：预览降级、滚动流畅、编辑不卡。
- **MQ-L-22** 大文件全文查找（默认大小写不敏感）：留意卡顿（**DEF-01**），记录延迟；修复后重测。

### 2.8 桌面主题集成（P1，需截图）

- **MQ-L-23** 跟随系统主题（深色 GTK 主题）：应用配色协调，对比度合格。
- **MQ-L-24** 深 / 浅手动切换：即时生效。

### 2.9 崩溃恢复（P0，关联 S7-06 / DEF-02）

- **MQ-L-25** 强制结束进程后重启：验证是否提示恢复未保存草稿。**当前运行时未接线草稿恢复（DEF-02）**，用于确认影响面与修复后回归。

## 3. 平台差异处置表

| 维度 | Linux | 与 Windows 差异 | 与 macOS 差异 | 处置 |
|---|---|---|---|---|
| 主修饰键 | 物理 `Ctrl` | 同 | macOS 为 ⌘ | 字符串统一 `Ctrl+…` |
| 会话 | X11 / Wayland 并存 | Win 单一 | macOS 单一 | 两种会话各验缩放与输入法 |
| 中文字体来源 | `/usr/share/fonts/…`：Noto CJK → 文泉驿 → 微米黑 → 正黑（**直接读文件，不经 fontconfig**） | `%WINDIR%\Fonts`：微软雅黑 → 等线 → 黑体 → 宋体 | `/System/Library/Fonts/PingFang.ttc` 等 | **egui 内置字体无 CJK，三平台均须显式加载并追加到族末尾**；缺字体不阻塞启动，仅打印明确警告。**Linux 路径未在开发机验证** |
| 文件编码 | 读取 UTF-8/BOM/GBK，保存 UTF-8 | 同 | 同 | 平台无关纯逻辑，三平台共用同一实现与测试 |
| 打印 | `lp` / `lpstat`（CUPS） | Win `powershell` | `lp` | 探测文案不同；`lpstat-only` 有专项回退 |
| 对话框 | GTK 原生（rfd） | Win32 原生 | Cocoa 原生 | rfd 抽象，行为一致 |
| 桌面主题 | 跟随 GTK 主题 | 无 | 跟随 NSAppearance | 协调即可，不强约束 |

## 4. 已知风险 / 遗留

- **DEF-01**：大小写不敏感查找 O(N×M)，大文件卡顿。
- **DEF-02**：自动保存 / 草稿恢复运行时未接线（见 `docs/archive/stage-reports/crash-recovery-report.md`）。
- **DEF-03**：保存一律写 UTF-8，无法写回原 GBK 编码。
- **DEF-04**：GB18030 / Big5 / Shift_JIS 未支持，遇这些编码可能出现替换字符。
- **DEF-05**：PNG 仅覆盖可见视口，整篇长图未实现。
- **Linux 字体候选路径未在开发机验证**——本清单 §2.4 是首次真机确认，请如实记录加载到哪个文件。
- X11/Wayland 双会话、缺 CJK 字体退化（MQ-L-14e）、IBus/Fcitx 输入法是 Linux 手工回归的核心价值。

## 5. 自动化覆盖对照（无需手工重复）

| 能力 | 自动化归属 |
|---|---|
| 原子保存不破坏原文件 | `fs_document_repository_test.rs` |
| 保存 / 重开逐字节一致 | `e2e_save_to_path_reopen_identical` |
| 快捷键四入口同源 | `shortcut_registry_test.rs` |
| >500KB 降级触发 | `large_file_test.rs` |
| 8 类边界不崩溃 | `large_file_test.rs` |
| GBK 解码、中英混排、损坏字节降级 | `fs_document_repository_test.rs` + `encoding.rs` 内联 12 条 |
| CJK 字体加载后中文/中文标点/等宽中文均有字形 | `fonts.rs` 内联 5 条（含「CJK 追加在末尾」顺序断言） |
| 符号面板每条目均有字形 | `s6_11f_test.rs` `every_symbol_glyph_is_renderable_under_the_real_font_setup` |
| 视口裁剪 / 越界拒绝 / 尺寸上限 / PNG 落盘往返 | `viewport_capture_test.rs` |

## 6. 签署

- 执行人：__________
- 执行环境（发行版 / 会话 X11·Wayland / 实际加载到的字体文件 / 打印机）：__________
- 通过项 / 阻塞项：__________
- 关联缺陷：DEF-01、DEF-02、DEF-03、DEF-04、DEF-05
