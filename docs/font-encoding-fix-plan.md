# 中文文本与图标显示异常 · 定位与修复方案

> 现象：应用界面中的中文文本与图标显示为乱码。
> 本文分三部分：**问题定位思路 → 具体修改点 → 验证方式**。
> 诊断结论均由实测得出（含字形探测数据），非推断。

---

## 〇、先纠正一个前提：本项目没有 Web 环节

排查清单里的若干项来自 Web 应用的典型故障谱系，但 **rustmark 是 Rust + egui + wgpu 的原生桌面应用**，
不存在以下环节。为避免"按 Web 思路查半天发现根本没有这一层"，先逐项对齐：

| 用户清单中的检查项 | 本项目是否存在 | 说明 |
|---|---|---|
| 文件读写的字符编码声明 | ✅ **存在** | `fs_document_repository::decode`；**这是本项目唯一真实的编码问题点** |
| HTML `meta charset` | ✅ 存在（仅导出物） | `pulldown_html_exporter`，**已正确声明 UTF-8** |
| HTTP `Content-Type` 头 | ❌ 不存在 | 无 HTTP 文本服务。`asset_loader` 只下载**图片二进制**，其中 `from_utf8_lossy` 仅用于解析 ASCII 的响应头 MIME，不涉及文本解码 |
| 数据库连接编码 | ❌ 不存在 | 全项目无数据库 |
| 构建工具默认编码 | ❌ 不存在（**结构性无风险**） | Rust 编译器强制源码为 UTF-8，`include_str!` 亦按 UTF-8 处理；不存在"构建工具默认 GBK"这种可配置项 |
| 图标引入路径与依赖 | ✅ 存在 | **但方案不是字体图标/Webfont/SVG，而是 Unicode 码位**（详见问题二） |

**结论**：真正需要修的是 **① 字体（界面中文 + 符号）、② 文件解码（GBK）** 两处。

---

## 一、问题定位思路

### 1.1 关键一步：先区分「缺字形」与「编码错乱」

这两类现象外观相似，成因与修法**完全不同**，混在一起查必然绕远路。

| 维度 | 缺字形（tofu / 豆腐块） | 编码错乱（mojibake） |
|---|---|---|
| 表现 | 空心方框 `□`、空白、问号 `?` | `锟斤拷`、`æ–‡ç«`、`�`（U+FFFD） |
| 成因 | 字体**不含**该码位的字形 | 字节按**错误编码**解码 |
| 发生层 | 渲染层（字体查找失败） | 解码层（字节 → 文本） |
| 修法 | 加载字体 / 换码位 | 修正解码器 / 探测编码 |

### 1.2 判定：界面中文属「缺字形」，与编码**无关**

推理链（可作为后续同类问题的判断模板）：

1. Rust 的 `String` / `&str` **保证**是合法 UTF-8，编译期即成立；
2. 源码字面量与 `include_str!` 的内容，编译器按 UTF-8 解释，无编码可配；
3. egui 渲染时按 **Unicode 码位**向字体要字形，取不到就画占位方框；
4. 上述链路里**不存在"用 A 编码读、按 B 编码解"的分歧点** → 不可能产生 mojibake。

**所以：界面中文显示异常 ⇒ 必然是字体缺字形，改编码声明是无效方向。**

### 1.3 实测证据（字形探测）

用 `egui::Context::fonts_mut(|f| f.has_glyph(&font_id, ch))` 逐码位探测，
得到的事实（`P`=Proportional 比例字体，`M`=Monospace 等宽字体）：

```
码位     字符   P       M
U+4E2D  中     false   false     ← 中文：全部无字形
U+6587  文     false   false
U+5668  器     false   false
U+FF0C  ，     false   false     ← 中文标点同样无字形
U+3002  。     false   false
U+300C  「     false   false
U+2014  —     true    true      ← 破折号有（属西文区）
U+2713  ✓     false   false     ← 问题图标：无字形
U+2715  ✕     false   false     ← 问题图标：无字形
U+2717  ✗     false   false     ← 问题图标：无字形
U+26A0  ⚠     true    true
U+00D7  ×     true    true      ← 可作 ✕ 的替代
U+221A  √     true    true      ← 可作 ✓ 的替代
U+2192  →     false   true      ← 比例无、等宽有（混用会不一致）
U+25CF  ●     false   true
U+2705  ✅     true    true      ← emoji：均有字形
U+274C  ❌     true    true
U+1F4C4 📄     true    true
（⭐💡🔥📌📝📚🔖🎯🚀💬⚙🔍 亦均为 true）
```

### 1.4 由此确定三个问题（与验收现象一一对应）

| 编号 | 问题 | 根因 | 严重度 |
|---|---|---|---|
| **P0-1** | **界面所有中文显示为豆腐块** | egui 默认字体族仅含西文 + emoji，**无任何 CJK 字形**；项目无字体资源、无 `set_fonts` 调用 | 阻塞级 |
| **P1-2** | **部分图标缺失**（`✓` `✕` `✗`） | 这三个码位不在 egui 内置字体的覆盖范围 | 中 |
| **P1-3** | **打开 GBK 编码文件后中文变成 `�`** | `decode()` 用 `String::from_utf8_lossy` 兜底，GBK 双字节序列被判非法 → 逐字节替换为 U+FFFD | 中 |

**P0-1 证据补充**：egui 0.33 的默认字体族定义（`epaint/src/text/fonts.rs`）：

```
Proportional = [Ubuntu-Light, NotoEmoji-Regular, emoji-icon-font]
Monospace    = [Hack, Ubuntu-Light, NotoEmoji-Regular, emoji-icon-font]
```

四个字体全部不含 CJK 字形。同时全仓 `grep -rn "FontDefinitions\|set_fonts\|FontData"` 仅命中 1 处无关的
`FontFamily::Monospace` 赋值——**说明从未注入过任何字体**。项目内也**不存在任何 `.ttf/.otf/.ttc` 文件**。

> 换言之：中文在界面里**从来没能显示过**，这不是偶发，而是必然。

---

## 二、具体修改点

### 修改点 1 · 注入 CJK 字体（修 P0-1）

**新增** `src/infrastructure/fonts.rs` —— 负责"发现并读取系统字体"（读文件属基础设施职责）：

```rust
//! 系统字体发现与读取。
//!
//! 只负责"把字体字节读出来"，不做 egui 相关组装——那属表现层职责。
//! 找不到字体时返回 `None`，由调用方决定降级策略（而非 panic）。

/// CJK 字体候选路径，按"字形覆盖优先"排序。
const CJK_CANDIDATES: &[&str] = &[
    // Windows
    "C:/Windows/Fonts/msyh.ttc",     // 微软雅黑，覆盖最全
    "C:/Windows/Fonts/Deng.ttf",     // 等线
    "C:/Windows/Fonts/simhei.ttf",   // 黑体
    "C:/Windows/Fonts/simsun.ttc",   // 宋体
    // macOS
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    "/usr/share/fonts/truetype/arphic/uming.ttc",
];

/// 读取第一个可用的 CJK 字体。返回 `(路径, 字节)`。
pub fn load_cjk_font() -> Option<(String, Vec<u8>)> {
    for path in CJK_CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(((*path).to_string(), bytes));
        }
    }
    None
}
```

**新增** `src/presentation/theme/text_fonts.rs` —— 负责组装 egui 字体定义：

```rust
use std::sync::Arc;
use egui::{FontData, FontDefinitions, FontFamily};

/// 在默认字体定义基础上追加 CJK 字体。
///
/// **必须追加到族末尾，不能插到开头**：
/// - 插开头会让西文也改用中文字体的西文部分（雅黑的拉丁字形不如 Ubuntu-Light 精细）；
/// - 等宽族（代码块、编辑区）会因此**失去等宽特性**，代码对齐全乱。
/// 追加到末尾则保持"西文优先、缺失才回落"的正确语义。
pub fn extend_with_cjk(defs: &mut FontDefinitions, bytes: Vec<u8>) {
    defs.font_data
        .insert("cjk".to_owned(), Arc::new(FontData::from_owned(bytes)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        defs.families.entry(family).or_default().push("cjk".to_owned());
    }
}
```

> **`.ttc` 说明**：微软雅黑等是字体集合（TrueType Collection），`FontData::index` 默认 `0`
> 即第一个 face（雅黑 Regular），符合需要，无需额外处理。

**接线** `src/app/frame.rs`（组合根，`run()` 内）：

```rust
eframe::run_native(
    "rustmark",
    options,
    Box::new(move |cc| {                       // ← 当前是 `|_cc|`，需改为 `|cc|`
        crate::app::startup_timing::mark("窗口就绪");

        // 字体必须在建界面**之前**装好，否则首帧会用旧字体画一次、再重排，出现闪烁。
        let mut defs = egui::FontDefinitions::default();
        match crate::infrastructure::fonts::load_cjk_font() {
            Some((path, bytes)) => {
                egui::log::debug!("已加载 CJK 字体：{path}");
                text_fonts::extend_with_cjk(&mut defs, bytes);
            }
            None => {
                // 找不到字体时**不 panic**：界面仍可用，只是中文显示为方框。
                // 静默失败会让"中文是方框"无从归因，故留一条明确警告。
                egui::log::warn!("未找到可用的 CJK 字体，中文将显示为方框");
            }
        }
        cc.egui_ctx.set_fonts(defs);

        let app = EditorApp::new(services, system_dark);
        crate::app::startup_timing::mark("应用构造完成");
        Ok(Box::new(app))
    }),
)
```

**同步注册模块**：
- `src/infrastructure.rs` 增 `pub mod fonts;`
- `src/presentation/theme.rs` 增 `pub mod text_fonts;`

---

### 修改点 2 · 修正图标码位（修 P1-2）

**方案 A（推荐，零依赖零体积）**：把无字形的三个码位换成**已验证有字形**的等价字符。

| 文件 | 位置 | 现状 | 改为 | 理由 |
|---|---|---|---|---|
| `src/presentation/views/sidebar_view.rs` | 321 | `"✓"` | `"√"` | U+2713 无字形；U+221A 有 |
| `src/presentation/views/sidebar_view.rs` | 322 | `"✕"` | `"×"` | U+2715 无字形；U+00D7 有 |
| `src/presentation/views/sidebar_view.rs` | 329 | `"回车提交 · Esc 或 ✕ 取消"` | `"… 或 × 取消"` | 文案同步 |
| `src/presentation/views/find_replace_view.rs` | 259 | `"✕ 关闭"` | `"× 关闭"` | 同上 |
| `src/presentation/views/status_bar_view.rs` | 185 | `"✓"` | `"√"` | 同上（`⚠` 保留，已有字形） |
| `src/presentation/state/symbol_ui_state.rs` | 331/336 | `"✓"` / `"✗"` | `"√"` / `"×"` | 「插入符号」面板的条目本身 |

**方案 B（补字体，可选增强）**：额外加载系统 emoji 字体（Windows `C:/Windows/Fonts/seguiemj.ttf`），
在保持原码位不变的前提下扩大覆盖。

> ⚠️ **方案 B 有技术风险**：`seguiemj.ttf` 是**彩色字体**（COLR/CPAL），而 egui 底层的
> `ab_glyph` 只取**轮廓**。彩色 emoji 字体能否被正确解析为单色轮廓**需实测**——
> 若不支持，`set_fonts` 可能报错或该字体被静默忽略。故**不作为首选**。
>
> 实测数据表明 emoji（`📄📌🚀🎯` 等）**当前均有字形**，`⚠` 也有，
> **真正缺的只有 `✓ ✕ ✗` 三个**——而它们换成 `√ ×` 即可，无需引入任何字体。方案 A 完胜。

**顺带发现的一致性隐患**：`→`/`←`/`●`/`□` 在**等宽字体有字形、比例字体没有**。
若这些字符用在比例文本中（如提示语），会显示为方框。当前它们只出现在**注释**里，不影响界面；
但后续若要在 UI 文本中使用，须先探测或改用 `→` 的替代（如 `->`）。

---

### 修改点 3 · 非 UTF-8 文件解码（修 P1-3）

**位置**：`src/infrastructure/persistence/fs_document_repository.rs` 的 `decode()`（当前 126-136 行）

现状：

```rust
fn decode(bytes: &[u8]) -> String {
    let body = bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes);
    // `from_utf8` 失败时用 `from_utf8_lossy` 兜底：编辑器不该因为一个
    // 非法字节就打不开文件（替换字符 U+FFFD 是业界通行做法）。
    let text = String::from_utf8_lossy(body);
    ...
}
```

**问题**：对 GBK 文件，每个汉字（2 字节）几乎都是非法 UTF-8 序列 → **每个字节**替换成一个 `�`。
用户看到"打开旧文件全是乱码"，且**内容已不可逆丢失**（保存后 `�` 被写回磁盘）。

**修复改为"按 BOM 与合法性分级探测"**：

```rust
fn decode(bytes: &[u8]) -> String {
    // 1) UTF-8 BOM：明确声明，直接按 UTF-8 解。
    if let Some(body) = bytes.strip_prefix(&UTF8_BOM) {
        return normalize(String::from_utf8_lossy(body).into_owned());
    }
    // 2) UTF-16 BOM：Windows 记事本"另存为 Unicode"的产物。
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return normalize(decode_utf16(rest, /* little_endian = */ true));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return normalize(decode_utf16(rest, /* little_endian = */ false));
    }
    // 3) 无 BOM：先按 UTF-8 试。合法则直接用——绝大多数现代文件走这条。
    if let Ok(text) = std::str::from_utf8(bytes) {
        return normalize(text.to_owned());
    }
    // 4) 非法 UTF-8 且无 BOM：疑似 GBK。按 GBK 解码（见下方选型）。
    decode_non_utf8(bytes)
}
```

第 4 步的 `decode_non_utf8` 需要"GBK → Unicode 映射"，有三种做法，**需裁定**：

| 选项 | 做法 | 优点 | 缺点 |
|---|---|---|---|
| **① 内嵌 GBK 映射表** | 生成 `gbk_table.bin`（约 2.2 万码位 → Unicode），`include_bytes!` 载入 | 零依赖；跨平台一致；可无头测试 | 体积 +约 120–180 KB（压缩后更小） |
| **② 引入 `encoding_rs`** | 用成熟 crate 的 `GBK` 解码器 | 最正确、支持更多编码（GB18030/Big5/Shift_JIS） | **新增依赖**（突破"零新增依赖"约束），产物体积 +约 300 KB–1 MB |
| **③ 仅检测并明确提示** | 不解码，改为报"该文件不是 UTF-8 编码"，由用户转码 | 零依赖、零体积、绝不产生错乱内容 | **不解决问题**，用户仍打不开 GBK 文件 |

> 与拼写检查词表同源的处理思路：**选项①可按项目既有做法（`include_str!` 词表）实现**，
> 不引入依赖。若选择②，需先更新 ADR 记录该决策。

**同步修正**：`tests/infrastructure/fs_document_repository_test.rs:211`
现有测试 `invalid_utf8_does_not_panic_and_uses_replacement_chars`
**断言"会产生 U+FFFD"**——这是把旧行为固化了。修复后：

- 若采用①/②：该测试应改为「非 UTF-8 字节按 GBK 正确解码」；
- 若采用③：该测试应改为「明确报错，且**不产生** U+FFFD」；
- 无论哪种，**兼容性底线不变**：绝不 panic（保留原测试的核心意图）。

---

### 修改点 4 · 导出 HTML 的字体族（预防性）

**位置**：`src/infrastructure/export/pulldown_html_exporter.rs` 的 `PAGE_STYLE`（约 87 行）

`meta charset="utf-8"` **已正确**，无需改动。但 CSS 字体族只列了西文：

```css
body{font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;...}
```

浏览器虽有自动回落，但显式声明可避免在缺少默认字体的环境下把中文交给不含 CJK 的字体。补中文字体：

```css
body{font-family:-apple-system,"Segoe UI","Microsoft YaHei","PingFang SC",\
"Hiragino Sans GB","Noto Sans CJK SC",Roboto,Helvetica,Arial,sans-serif;...}
```

同时 `code` / `pre` 的等宽字体也建议补 CJK 等宽（否则代码块里的中文注释会用比例字体，与代码不齐）：

```css
code,pre{font-family:ui-monospace,SFMono-Regular,"Cascadia Mono",Consolas,\
"Microsoft YaHei Mono","Noto Sans Mono CJK SC",monospace}
```

---

## 三、验证方式

### 3.1 修改点 1（字体）

**新增自动化测试** `tests/infrastructure/fonts_test.rs`：
- `load_cjk_font_finds_a_font_on_this_platform` —— 断言本机至少命中一个候选（Windows 必有 `msyh.ttc`）；找不到则 `#[ignore]` 并说明。
- `extend_with_cjk_appends_to_both_families_at_the_end` —— **断言位置**：CJK 必须追加在**末尾**。
  这是关键不变量——插到开头会让代码块失去等宽，属"看起来只是字体变了"的隐蔽回归。
- `extend_with_cjk_keeps_western_fonts_first` —— 断言 `families[Proportional][0]` 仍是 `Ubuntu-Light`。

**字形回归测试**（把本次诊断固化为断言，防止再次退化）：

```rust
#[test]
fn every_ui_glyph_has_a_font() {
    // 界面上实际用到的字符必须都能渲染；清单从源码 grep 得来。
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let ui_chars = "中文编辑器预览文档√×⚠📄📌📚🚀🎯✕✓✗";
    for ch in ui_chars.chars() {
        let ok = ctx.fonts_mut(|f| f.has_glyph(&FontId::proportional(14.0), ch));
        assert!(ok, "U+{:04X} {ch} 无字形，界面上会显示为方框", ch as u32);
    }
}
```

> **重要**：这条测试**不注入字体**时必然失败（这正是当前缺陷），注入后通过。
> 因此它同时是"缺陷复现"与"修复确认"——是本次最有价值的回归网。
> 字符串 `✕✓✗` 在采用方案 A 后应从清单中移除（改为 `√×`）。

**真机目测（无法自动化）**：
- 启动应用，确认菜单/状态栏/侧边栏/预览区中文正常；
- 打开含中文的 `.md`，确认预览区与编辑区中文均正常；
- 确认代码块中文注释在等宽字体下**与代码对齐**（验证未被插入到族开头）。

### 3.2 修改点 2（图标）

- 逐点目测 6 处（侧边栏重命名按钮、查找替换关闭按钮、状态栏成功标记、符号面板条目）；
- 补一条测试：`insert_symbol_items_use_glyphs_available_in_default_fonts`
  ——遍历 `symbol_ui_state` 的条目，断言每个 `glyph` 字段都有字形（防后续新增条目又踩同一个坑）。

### 3.3 修改点 3（编码）

**新增测试** `tests/infrastructure/` 内：
- `gbk_file_is_decoded_without_replacement_chars` —— 写一段 GBK 编码的"中文测试"字节，断言解出的是 `中文测试` 且**不含** `\u{fffd}`。这是 P1-3 的直接复现。
- `utf16_le_bom_file_is_decoded` —— 带 `FF FE` BOM 的 UTF-16 文件正确解出。
- `utf8_without_bom_still_uses_fast_path` —— 确认主流路径未被影响（性能与正确性双保险）。
- `binary_file_never_panics` —— 保留原测试的核心意图：任意字节输入都不 panic。
- 更新 `invalid_utf8_does_not_panic_and_uses_replacement_chars`（见修改点 3 说明）。

### 3.4 修改点 4（导出）

- 复用既有导出测试：断言导出 HTML **同时包含** `charset="utf-8"` 与中文字体族（如 `Microsoft YaHei`）。
- 手工验证：导出含中文的文档，浏览器打开（Chrome/Edge），确认无乱码；用"查看页面源代码"确认 meta 在 `<head>` 首部（charset 须在首 1024 字节内才有效——当前已满足）。

### 3.5 项目门禁（每项修改后必过）

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run --bin check-arch
cargo test --all-targets -- --skip large_full_text_find_is_bounded
```

> 环境注意：需 `CARGO_INCREMENTAL=0`（增量编译会触发 rustc ICE），且构建需写入 `target/`
> （沙箱内会被拦，须在非沙箱模式运行）。

---

## 四、修改涉及文件汇总

| 文件 | 类型 | 修改点 |
|---|---|---|
| `src/infrastructure/fonts.rs` | **新增** | 1 |
| `src/infrastructure.rs` | 改（注册模块） | 1 |
| `src/presentation/theme/text_fonts.rs` | **新增** | 1 |
| `src/presentation/theme.rs` | 改（注册模块） | 1 |
| `src/app/frame.rs` | 改（`run()` 接线字体；`|_cc|` → `|cc|`） | 1 |
| `src/presentation/views/sidebar_view.rs` | 改（2 处按钮 + 1 处文案） | 2 |
| `src/presentation/views/find_replace_view.rs` | 改（1 处按钮） | 2 |
| `src/presentation/views/status_bar_view.rs` | 改（1 处标记） | 2 |
| `src/presentation/state/symbol_ui_state.rs` | 改（2 处条目） | 2 |
| `src/infrastructure/persistence/fs_document_repository.rs` | 改（`decode` 分级探测 + 新增 GBK 解码） | 3 |
| `src/infrastructure/persistence/gbk_table.bin` | **新增**（若选①） | 3 |
| `src/infrastructure/export/pulldown_html_exporter.rs` | 改（`PAGE_STYLE` 字体族） | 4 |
| `tests/infrastructure/fonts_test.rs` | **新增** | 1 |
| `tests/infrastructure/fs_document_repository_test.rs` | 改（更新旧断言 + 新增用例） | 3 |
| `docs/adr/` | 若选②需新增 ADR | 3 |

---

## 五、优先级与依赖关系

1. **修改点 1（字体）** —— P0，独立，**必须最先做**。做完中文即可用，其余是体验完善。
2. **修改点 2（图标）** —— P1，独立，零风险（纯字符替换）。
3. **修改点 3（编码）** —— P1，**需先裁定选项①/②/③**。
4. **修改点 4（导出 CSS）** —— P2，预防性，独立。

前三项互相独立，可分别提交与验证。

---

## 六、实施记录（2026-10-08 已完成）

### 裁定结果

| 裁定点 | 选择 | 影响 |
|---|---|---|
| GBK 解码方案 | **① 内嵌映射表** | 零新增依赖；产物体积 +47880 字节（约 47 KB） |
| 实施范围 | **四项全做** | 字体 + 图标 + 编码 + 导出 CSS |

### 实际改动文件

| 文件 | 类型 | 内容 |
|---|---|---|
| `src/infrastructure/fonts.rs` | **新增** | 系统 CJK 字体探测/读取 + `FontDefinitions` 组装（5 例单测） |
| `src/infrastructure.rs` | 改 | 注册 `fonts` 模块 |
| `src/app/frame.rs` | 改 | `run()` 接线 `install_fonts`（`|_cc|` → `|cc|`） |
| `src/infrastructure/persistence/encoding.rs` | **新增** | 编码探测与解码（12 例单测） |
| `src/infrastructure/persistence/gbk_table.bin` | **新增** | GBK→Unicode 映射表，23940 格 / 47880 字节 |
| `src/infrastructure/persistence.rs` | 改 | 注册 `encoding` 模块 |
| `src/infrastructure/persistence/fs_document_repository.rs` | 改 | `decode` 委托给编码模块，不再用 `from_utf8_lossy` |
| `src/presentation/state/symbol_ui_state.rs` | 改 | 修正 9 个无字形的符号条目 |
| `src/presentation/views/{sidebar,find_replace,status_bar}_view.rs` | 改 | 6 处 `✓`/`✕` 替换为 `√`/`×` |
| `src/infrastructure/export/pulldown_html_exporter.rs` | 改 | `PAGE_STYLE` 补中文字体族（正文 + 代码） |
| `tests/presentation/s6_11f_test.rs` | 改 | 新增"所有符号条目都有字形"回归测试 |
| `tests/infrastructure/fs_document_repository_test.rs` | 改 | 新增 GBK 端到端用例，更新旧断言注释 |
| `tests/integration/large_file_test.rs` | 改 | 更新注释说明新行为 |
| `tests/infrastructure/exporter_test.rs` | 改 | 断言导出 HTML 含中文字体族 |

### 实施中新发现并修正的两个缺陷（原方案未预见）

**1. 把"大部分合法 + 零星坏字节"的 UTF-8 误判为 GBK。**

首版按"合法 UTF-8 覆盖率"判分（阈值 0.75），**实测不可行**：
GBK 中英混排文本的覆盖率是 0.75，而损坏的 UTF-8 是 0.83——两簇贴得太近。
更糟的是，一旦误判，原本正确的中文（三字节 UTF-8）被当成 GBK 双字节**二次解码**，
得到满篇看似正常却全错的汉字——**比原来的替换字符更危险**（用户看不出问题，一保存就毁掉原文）。

改为按**序列长度分档**判定：
- 真中文 UTF-8 的汉字是 `E4–E9` 开头的**三字节**序列；
- GBK 字节偶然凑出的合法 UTF-8 序列**几乎只落在双字节区**
  （首字节需落在 `C2–DF`、尾字节落在 `80–BF`，粗算约四分之一的概率）。

判据：`三字节序列数 > 双字节序列数` 即视为 UTF-8。实测各场景稳定。

**2. GBK 解码会吞掉 `0xFF` 之后的一个 ASCII 字符。**

原分支顺序是"先取尾字节再查表"，没先校验 `lead` 是否为合法首字节
（`0xFF` 永远非法）。结果 `[0x61, 0xFF, 0x62]` 解出 `"a�"`——**'b' 被静默吃掉**。
修正为：先判 `(0x81..=0xFE).contains(&lead)`，不合法则只作废该字节。

这条是被 `fs_document_repository_test` 的一条断言
（"坏字节两旁的 ASCII 不得丢失"）抓出来的——**写测试时加的那句"多余"断言，恰是唯一发现它的地方**。

### 验证结果

- `cargo test --all-targets`：**1237 passed / 0 failed**（较改动前 1217 增 20）
- `cargo fmt --check` 零差异
- `cargo clippy --workspace --all-targets -- -D warnings` **0 告警**
- `cargo run --bin check-arch`：OK
- **决定性验证**：`real_cjk_font_provides_glyphs_for_chinese` 实测加载字体后
  中文、中文标点、等宽族中文**全部有字形**（修复前实测均为 false）
- **回归网**：`every_symbol_glyph_is_renderable_under_the_real_font_setup`
  一次抓出 9 个此前无人知晓的缺失符号

### 仍待真机确认

- 界面观感（中文字重、字号与西文的搭配）需目测；
- macOS / Linux 的字体路径未在本机验证（本机为 Windows，实测命中 `msyh.ttc`）；
- GB18030 四字节扩展、Big5、Shift_JIS 等其他东亚编码**不支持**；
- 无 BOM 且非合法 UTF-8 的文件"猜编码"存在固有歧义（如纯西欧字符的 UTF-8 文件
  `café` 会被判为 GBK），与浏览器行为同理，想消除歧义只能靠 BOM 或用户手动指定。
