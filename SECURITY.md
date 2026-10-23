# 安全政策（Security Policy）

## 1. 支持版本

| 版本 | 是否接收安全修复 |
|---|---|
| v0.2.0（当前，未正式发布） | 是 |
| `main` 分支 HEAD | 是（开发版，仅作验证用途） |

本项目为单机桌面应用，无服务端、无远程更新通道；修复以**新版本发布**形式提供，
用户需自行下载替换（见 `README.md` §2）。

## 2. 报告漏洞

**请勿在公开 Issue 中披露未修复的安全问题。** 请通过 GitHub 私有安全公告提交：

- <https://github.com/RyenLee/rustmark/security/advisories>（Security Advisories → Report a vulnerability）

若该入口不可用，可先在 Issue 中**只留标题**（不含复现细节）并请求私下联系渠道。

请在报告中提供：

1. 影响的版本 / commit；
2. 复现步骤（若为文档触发型缺陷，请附上**最小复现 Markdown 片段**，不要附真实文档）；
3. 影响面（崩溃 / 数据损坏 / 任意文件读写 / 远程内容执行 / 其它）；
4. 你有依据的修复建议（可选）。

### 响应预期

| 阶段 | 目标 |
|---|---|
| 确认收悉 | 7 日内 |
| 初步定级与影响评估 | 14 日内 |
| 修复发布或明确不修复的理由 | 视严重等级，尽力在 30 日内 |

## 3. 已实施的安全措施（当前状态）

| 类别 | 措施 |
|---|---|
| 网络 | 仅 `ureq` + `rustls`（纯 Rust TLS）；远程图片下载有**全局超时 + 体积硬上限 + MIME 白名单** |
| 内容安全 | 远程/本地资源加载**不放行 SVG**（SVG 可携带脚本）；支持的位图格式为显式白名单 |
| 导出安全 | HTML 导出有 URL 协议白名单（`http:`/`https:`/`mailto:`/`tel:`），其余协议在导出前处理 |
| 进程隔离 | PDF 导出调用本机浏览器的无头打印能力，不经网络；外部链接交由系统默认程序打开 |
| 依赖安全 | `cargo deny check licenses advisories bans sources`（CI 每次推送执行）；源仅允许 crates.io |
| 构建安全 | `panic = "abort"`、发布流程含「无私钥/证书入仓」检查（`tools/release_dryrun.py`） |
| 隐私 | 无遥测、无自动更新、无崩溃自动上传；崩溃报告只写本地且不含文档内容（见 `PRIVACY.md`） |

## 4. 已知的非目标（Out of Scope）

- **本机恶意文档的任意代码执行不在威胁模型内**：本应用会按用户意图打开本地文件、
  调用系统默认程序打开链接、调用本机浏览器导出 PDF——这些是编辑器的既定功能。
- **依赖链上游漏洞**：由 `cargo deny check advisories` 跟踪并随版本升级修复；
  单个上游 crate 的独立 PoC 请直接向上游报告。
- **未签名产物的供应链提示**：当前发布产物**未做代码签名**（Windows SmartScreen /
  macOS Gatekeeper 会拦截）。发布页提供 SHA256 校验和，请先核对再运行。
  签名证书属可选投入，见 `docs/release.md` §6。

## 5. 产物完整性校验

每次发布附带 `SHA256SUMS`。请务必核对：

```bash
sha256sum -c SHA256SUMS        # Linux / macOS
Get-FileHash <file> -Algorithm SHA256   # Windows（与发布页核对）
```
