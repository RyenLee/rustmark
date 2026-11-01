# 故障排查（Troubleshooting）

> 面向用户。按「现象 → 处置」组织；诊断信息只涉及本机文件，无任何遥测。

---

## 1. 配置目录在哪里？

| 平台 | 路径 |
|---|---|
| Windows | `%APPDATA%\rustmark\`（即 `C:\Users\<用户名>\AppData\Roaming\rustmark\`） |
| macOS | `~/Library/Application Support/rustmark/` |
| Linux | `$XDG_CONFIG_HOME/rustmark/`，未设则 `~/.config/rustmark/` |

目录内容：

| 文件 / 目录 | 用途 |
|---|---|
| `preferences.toml` | 用户设置（主题、快捷键、拼写检查开关等） |
| `preferences.toml.bak` | 设置损坏回退默认时自动留下的备份 |
| `recent.json` | 最近打开的文件 |
| `.rustmark-draft/` | 自动保存草稿（异常退出后可恢复） |
| `crash/` | 崩溃报告（仅真实崩溃过才会出现） |

## 2. 如何重置设置？

1. 关闭 rustmark。
2. 删除（或改名）`preferences.toml`。原文件若解析失败，应用本就会把它备份为
   `preferences.toml.bak` 再回退默认——所以「删掉主文件、留着 .bak」是最稳妥的重置。
3. 重新启动，设置即为默认值。

快捷键冲突、主题异常、字号诡异等问题，重置设置通常一步到位。

## 3. 如何找回未保存的内容（自动保存 / 草稿恢复）？

- rustmark 默认在**修改停顿满 30 秒**（可在设置中调整或关闭）后，
  把该文档的未保存内容写入 `.rustmark-draft/`；
  应用异常退出（崩溃、断电）后，下次启动会弹出恢复提示。
- 若启动恢复提示未出现但草稿确实存在（例如正常关闭前的改动），
  可直接查看 `.rustmark-draft/` 目录：草稿文件是**纯文本 Markdown**，
  扩展名 `.md`，内容就是编辑器缓冲的快照，用任意编辑器都能打开。
- 找回后记得手动另存到目标位置；草稿在文档正常保存/关闭后会被清理。

## 4. 应用崩溃了怎么办？（崩溃报告的提交方式）

rustmark **不做任何遥测**，崩溃时只在本地写一份诊断报告，
由你决定是否提交：

1. 打开配置目录下的 `crash/`，找到崩溃时间点对应的
   `crash-<时间>-<进程ID>.txt`（最多保留最近 10 份，更早的自动清理）。
2. 报告内容**只有**：应用版本、平台（OS/架构）、崩溃时间、线程、
   源码位置、栈回溯——**不含你编辑的任何文档内容**，可直接作为附件。
3. 连同崩溃前做了什么（打开什么文件、点了什么菜单）一起提交到项目 Issue。

> 报告打不开或 `crash/` 目录不存在：说明崩溃发生在 hook 安装前
> （极早期启动失败），此时请直接提交「启动命令行输出」。

## 5. 启动失败：提示「无法读取应用配置」

启动装配唯一可失败的点是**配置文件读不到**（权限异常、路径是目录等）。
按报错信息里的默认位置检查：

- 目录是否存在且可读；
- `preferences.toml` 是否被其他程序占用；
- Windows 上可尝试以当前用户身份重新创建 `%APPDATA%\rustmark\`。

配置**内容**损坏不会导致启动失败（会自动备份 + 回退默认），
报这个错基本是文件系统权限问题。

## 6. 启动失败：提示「找不到可用的图形适配器」（Linux 无 Vulkan 驱动）

**现象**：从终端启动时立刻退出，stderr 打印

```text
rustmark 启动失败：找不到可用的图形适配器（GPU）
rustmark 在 Linux 上只使用 **Vulkan** 后端：为了控制发行体积，OpenGL / GLES
回退没有编入……
原始错误（提交 Issue 时请附上）：
WGPU error: No suitable graphics adapter found; … vulkan drivers/libraries could not be loaded …
```

Windows 上同一故障表现为**双击没反应**（release 构建无控制台），此时看
`%APPDATA%\rustmark\crash\` 下最新的一份 `crash-*.txt`。

**原因**：Linux 版只编入了 wgpu 的 Vulkan 后端，没有 OpenGL / GLES 回退
（体积取舍，见 [P1 #3 处置记录](../reviews/p1-03-linux-vulkan-disposition.md)）。
本机没有可加载的 Vulkan loader / 驱动时，窗口无法创建。

**处置**：

1. 安装 Vulkan loader 与驱动：

   | 发行版 | 命令 |
   |---|---|
   | Debian / Ubuntu | `sudo apt install mesa-vulkan-drivers libvulkan1 vulkan-tools` |
   | Fedora / RHEL | `sudo dnf install mesa-vulkan-drivers vulkan-loader vulkan-tools` |
   | Arch | `sudo pacman -S vulkan-icd-loader vulkan-tools`，驱动另装 `vulkan-intel` / `vulkan-radeon` / `vulkan-nvidia` |

2. 自检：`vulkaninfo --summary` 输出里出现 `GPU0` 一段即通过。
   若报 `Cannot create Vulkan instance`，说明 loader 或驱动仍未就位。
3. 虚拟机 / 云桌面 / WSL2：在宿主侧开启 3D 加速并安装 guest Vulkan 驱动；
   RDP 与部分 VNC 协议不提供 Vulkan 设备，请在本机桌面会话下运行。
4. 确认「后端 feature 有没有编进去」：`cargo test --test startup_smoke`
   （只证明编译配置正确，**不**证明本机驱动可用）。

**可复现的验证步骤**（回归这条诊断时用）：

```bash
# 1) 准备一台没有 Vulkan 驱动的 Linux（容器/VM 里卸掉即可，不影响宿主）
docker run --rm -it ubuntu:24.04 bash
apt update && apt install -y curl
#    注意：故意**不**装 mesa-vulkan-drivers / libvulkan1

# 2) 放入 release 产物（宿主机先 cargo build --release 并可访问）
#    这里用宿主目录挂载（示例）：
#    docker run --rm -it -v "$PWD/target/release:/app" ubuntu:24.04 /app/rustmark
#    若用 X11 转发，需额外装 xauth 并挂载 /tmp/.X11-unix、设 DISPLAY。

# 3) 预期输出（关键三条，缺一即回归）
#    a. 首行 "rustmark 启动失败：找不到可用的图形适配器（GPU）"
#    b. 建议里出现 "mesa-vulkan-drivers" 与 "vulkaninfo"
#    c. 落盘出现在 $XDG_CONFIG_HOME/rustmark/crash/crash-<时间>-<PID>.txt

# 4) 装上驱动后应恢复（宿主有 X11 时）
apt install -y mesa-vulkan-drivers libvulkan1 vulkan-tools
vulkaninfo --summary   # 应列出 GPU0
```

> 卸掉驱动只影响容器/VM 的图形栈，不影响宿主机；这也是为什么该验证
> 不需要专用机器。**注意**：本仓库开发机为 Windows 且 WSL/Docker 不可用，
> 上述第 2~4 步尚未在 Linux 真机上执行过，属于待补验证项。

## 7. 已知问题速查

| 现象 | 说明 |
|---|---|
| Linux 启动即退出，提示「找不到可用的图形适配器」 | 缺 Vulkan 驱动；装 `mesa-vulkan-drivers` + `libvulkan1`，见 §6 |
| Windows 双击 exe 没反应 | 多为 GPU 初始化失败；看 `%APPDATA%\rustmark\crash\` 最新报告，见 §6 |
| Windows 首次运行提示 SmartScreen | 安装包**未签名**（证书成本原因），点「仍要运行」即可；校验 SHA256 与发布页一致 |
| macOS 首次运行被 Gatekeeper 拦截 | 同上未签名未公证，右键 → 打开，或 `xattr -d com.apple.quarantine <App路径>` |
| PDF 导出提示未实现 | PDF 导出暂未交付（需系统级 PDF 引擎），请改用 HTML / Markdown / 图片导出 |
| 拼写检查对中文无效 | 设计如此：拼写检查仅覆盖英文词表（演示级） |
| 预览区公式 / Mermaid 不出图 | 编辑器内为结构化卡片；像素级渲染在导出 HTML 后由浏览器完成 |
| 图片导出范围只有预览区 | 视口截图即当前所见；勾选「整篇长图」可导出全文档 |
| 超长文档导出长图被拒 | 超过 256 屏或 16384px 高度上限，请改用 HTML / PDF 导出 |
| GBK 文件打开出现少量问号 | 文件含 GBK 之外的非法字节；合法 GBK/GB18030 内容可无损打开与保存 |
