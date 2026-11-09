# ADR-014 · 数据目录改为应用目录（便携优先）

- **状态**：已接受（2026-11-09）
- **取代**：ADR-003 §2 中「路径遵循各平台配置目录规范」这一句**仅限路径**；
  原子写、schema 版本、草稿语义、崩溃恢复策略全部保持不变。ADR-003 原文
  按「决策记录不可改写」规则不修改，本条是其增量修订。
- **关联**：`PRIVACY.md` §4、`README.md` §2、`docs/guides/config-reference.md` §1、
  `docs/guides/troubleshooting.md` §1、`scripts/package-*`
- **实现**：`crates/rustmark-assembly/src/bootstrap.rs::ConfigPaths`

## 背景

用户反馈「应用打开后在 `%APPDATA%`（Roaming）下生成文件」，希望数据留在
应用目录内，解压目录整体删除即完全清理。

复核现状后的事实：

1. 0.2.x 的三种产物都是**解压即用**形态（Windows zip、Linux tar.gz、macOS
   `.app`/dmg），没有安装器、不写注册表、没有 per-user 安装概念；
2. 应用数据只有四类：`preferences.toml`、`recent.json`、
   `.rustmark-draft/`（草稿 + 旁车）、`crash/`（崩溃报告），全部**懒创建**；
3. 框架侧的 eframe `persistence` feature 未启用，不会再写第二处目录；
   若将来启用，其默认路径 `%APPDATA%\<app_id>\data\` 与本决策不是一回事，
   必须显式设置 `NativeOptions::persistence_path`，否则会出现「数据分家」。

ADR-003 的复盘触发条件之一写的是「用户反馈配置目录位置不符合**平台习惯**」。
本次反馈方向相反：用户要的是**便携性**而非平台惯例。两者都是合理诉求，
但分发形态已经决定了取舍——没有安装器的应用按安装式布局存数据，
卸载时必然在用户目录留下孤儿目录。

## 决策

### 1. 默认根目录

`current_exe()` 的父目录下的 `data/`。文件与子目录布局不变：

```text
<应用目录>/
├── rustmark.exe
├── fonts/
└── data/                    ← 设置、草稿、崩溃报告
    ├── preferences.toml
    ├── recent.json
    ├── .rustmark-draft/
    ├── crash/
    └── .rustmark-migrated   ← 迁移完成标记
```

启动时通过「建目录 + 写一个随即删除的探针文件」判定可写性。
这与 ADR-003 时代「目录不在启动期创建」不同：写权限必须在启动期判定，
否则第一次改设置或自动保存才失败，而错误信息会把文件系统问题
伪装成「配置读取失败」。探针不留常驻文件。

### 2. 回退（不是错误）

以下情况沿用平台配置目录，并在启动日志给出**具体原因**：

| 情形 | 原因 |
|---|---|
| 应用目录不可写（装在 `Program Files`、受管目录、只读介质） | 写进去必然失败，且失败点会滞后到第一次保存 |
| macOS `.app` bundle（`…/*.app/Contents/…`） | 往签名 bundle 内写数据会破坏代码签名，Gatekeeper 拒启 |
| Linux AppImage（`/tmp/.mount_*/…`） | squashfs 只读挂载，且该目录重启即消失 |
| 拿不到 `current_exe()` | 无参照点，只能用平台目录 |

回退目标仍是 Windows `%APPDATA%\rustmark\`、macOS
`~/Library/Application Support/rustmark/`、Linux `$XDG_CONFIG_HOME/rustmark/`。
**回退不是降级**：功能完全一致，只是数据不跟应用走。

### 3. 旧数据迁移

首次启动把旧平台目录的内容**复制**到新 `data/`：

- **只复制、不移动、不删除**。旧目录是用户既有数据，草稿尤其不能丢；
- 目标已存在的文件一律跳过（用户改过的新配置优先）；
- 符号链接不跟随（宁漏勿错）；任何 I/O 失败只跳过该条目，不阻断启动；
- 成功后写 `.rustmark-migrated` 标记：既避免每次启动扫描旧目录，
  也避免用户删掉新配置想「重置」时，旧配置又被复制回来。

旧目录因此会**保留**（含残留 tmp 与旧崩溃报告），由用户自行决定是否删除；
清理口径见 `PRIVACY.md` §4 与 `troubleshooting.md` §1。

### 4. 打包产物不包含 `data/`

zip / tar.gz / dmg 内**不预置** `data/`，避免覆盖用户配置；
首次运行自行创建。`.gitignore` 忽略 `/data/`（源码树旁的开发构建）。

## 代价与风险（已接受）

1. **写权限**：靠启动期探测 + 回退兜住；回退原因进日志，不弹窗打断启动。
2. **多用户 / 多实例**：`data/` 是共享目录，同一份解压目录被多个用户或
   两个进程使用时，设置与最近文件会互相覆盖；原子写只防同文件并发截断。
   这是「便携」定位的固有代价，与 README §4 第 11 条（同文件最后写入者胜）
   同级披露。
3. **升级覆盖**：新版解压覆盖旧目录时，`data/` 与 exe 同级，若用户
   「先删整个目录再解压」会丢数据。发布说明必须提示**保留 `data/`**。
4. **平台差异**：macOS `.app` 与 Linux AppImage 实际仍走平台目录；
   文档不得承诺「三平台一律在应用目录」。
5. **隐私契约变更**：`PRIVACY.md` §4 是已发布的用户契约，必须同步改写；
   数据内容本身不变，也没有新增联网行为。

## 与 ADR-003 的口径校正

顺带记录三处历史文档与代码的命名漂移（不改变行为）：

| 文档写法 | 实际 |
|---|---|
| ADR-003 `settings.toml` | `preferences.toml` |
| PRIVACY.md `drafts/` | `.rustmark-draft/` |
| `recent.json` | 文件名是 `.json`，**内容格式是 TOML** |

## 影响范围

- `ConfigPaths::detect` / `detect_with_note` / `root_dir` / `migrate_legacy`；
- `src/app.rs`：一次解算路径，同时供装配、崩溃 hook、残留清理、迁移使用；
- `src/app/startup_diagnosis.rs`：诊断文案改为打印实际路径 + 回退说明；
- 所有测试仍用 `ConfigPaths::in_dir(临时目录)` 注入，与默认目录解耦；
- 文档与三个打包脚本的 README 文案、`.gitignore`、Issue 模板。

## 复盘触发条件

- 出现真实用户因「装在只读目录却没收到任何提示」而丢数据；
- 便携目录下多实例并发导致设置互相覆盖的反馈 ≥ 2 例；
- 启用 eframe `persistence` 时未同步 `NativeOptions::persistence_path`。
