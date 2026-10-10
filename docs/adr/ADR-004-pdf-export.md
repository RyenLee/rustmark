# ADR-004 · PDF 导出：外置系统打印（不内嵌引擎）

- **状态**：已接受（2026-10-06）
- **关联需求**：§3.1（`FUNC-P1-04` 导出 PDF，P1）、§8.1（体积预算）
- **关联任务**：S1-04、S5-10、S6-11

## 背景

PDF 导出为 P1 需求。可选路径有三类：内嵌 Rust PDF 引擎（`printpdf` / `pdf-writer` / `typst`）、调用外部命令行工具（`wkhtmltopdf` / `pandoc`）、调用系统打印设施。体积预算总盘 5–7 MB（上限 10 MB），任何新增渲染引擎都可能击穿预算。

## 决策

**采用「生成打印就绪 HTML → 调用系统打印/导出设施」的外置方案，不内嵌 PDF 引擎。**

- Windows：生成临时 HTML → 调用系统关联程序打印（或 Shell 打印动词）；
- macOS：生成临时 HTML → `NSPrintOperation` / 系统打印对话框（PDF 可由用户在打印对话框内「存储为 PDF」）；
- Linux：探测 `lp` / `wkhtmltopdf` / 桌面门户打印服务，按可用性依次尝试。

不可用时返回 `Err(Unsupported { format: Pdf })`，提示文案明确引导用户先导出 HTML 再用系统工具转换。

## 代价与边界

| 维度 | 影响 |
|---|---|
| 体积 | 零新增依赖（`cargo tree \| grep -i pdf` 必须为空） |
| 体验 | 无法静默导出到指定路径，需一次系统对话框交互 |
| 一致性 | 三平台输出样式受各自打印引擎影响，不保证像素一致 |
| 分页控制 | 依赖 CSS `break-inside: avoid` 等打印样式，需提供默认打印样式表 |

## 复盘触发条件

用户反馈「必须静默导出」成为高频诉求，且体积预算有 ≥2 MB 余量时，重新评估内嵌轻量引擎。

## 修订（2026-10-10）：Windows 改为无头 Chromium 优先

原实现的 Windows 路径是 `powershell -Command "Start-Process -FilePath $args[0] -Verb PrintTo" <html>`。
上线后用户报告导出直接报错：

```
Start-Process : Cannot validate argument on parameter 'FilePath'. The argument is null or empty.
```

**两个独立缺陷叠在一起**：

1. **尾随参数根本没进 `$args`。** `powershell -Command <字符串> <参数…>` 的尾随参数
   不会绑进 `$args`——实测 `powershell -NoProfile -Command 'Write-Output "[$args]"' hello world`
   输出 `[]`，且 `hello` / `world` 被当成**脚本正文**另起语句执行。
   于是 `-FilePath $args[0]` 恒为 `null`。修法：路径改走**环境变量**
   （`$env:RUSTMARK_PRINT_SOURCE`），彻底绕开命令行引号与参数绑定。
2. **`printto` 动词本身也不合适。** 注册表里
   `HKEY_CLASSES_ROOT\htmlfile\shell\printto\command` 指向
   `rundll32.exe mshtml.dll,PrintHTML "%1" "%2"`——**已废弃**的 MSHTML 打印引擎，
   且必须经打印对话框（用户还得自己在对话框里「另存为 PDF」）。

**决策调整为：Windows 优先无头 Chromium 内核（Edge → Chrome）的 `--print-to-pdf`，
PowerShell `printto` 降为兜底。** 这不是「新的外部依赖」：Edge 在 Windows 10/11 上
随系统预装，零新增 crate，与本节「不内嵌引擎」的立场一致。

实测（本机 Edge 154.0.4258.53）：

| 观察项 | 结论 |
|---|---|
| 产物 | 真 PDF（`%PDF-` 头），静默、无窗口、无对话框 |
| 与已运行的 Edge 实例 | **不冲突**，18 个 Edge 进程在跑时仍能正常产出 → 不需要 `--user-data-dir` |
| 退出码 | 成功为 0；但**写盘失败也可能是 0**，故判据取「产物是否存在」 |
| 页眉页脚 | `--no-pdf-header-footer` 有效；`--print-to-pdf-no-header` **已失效**（带与不带字节数完全一致） |
| `@page` 纸张 | 被尊重：A4 → 595×842pt、A5 → 420×595pt |

顺带修掉的两处：

- **纸张选项从未生效**：打印样式表里此前一条 `@page` 都没有，而界面一直承诺
  「纸张会写入导出件的 `@page` 规则」。现在按 `ExportOptions::page_size` 注入
  `@page{size:A4|letter;margin:18mm}`。
- **大小不再一律报 0**：能直接落盘时回报真实字节数，界面据此区分
  「已导出 xxx（N 字节）」与「已调起打印对话框，请另存」。

调用 `printto` 兜底时仍**不能**静默落盘，`PdfBackend::writes_pdf` 把这条差别
暴露给调用方，避免界面在两种情形下说同样的话。
