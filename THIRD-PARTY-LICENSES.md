# 第三方许可清单（Third-Party Licenses）

> rustmark 以 **MIT OR Apache-2.0** 双许可发布（见 `LICENSE-MIT` / `LICENSE-APACHE`）。
> 本文件是随发行包分发的**第三方组件许可清单**，与 `NOTICE`（摘要）互补：
> `NOTICE` 面向人读，本文件面向审计（含全部非 MIT/Apache-2.0 双许可的依赖）。
>
> 生成口径与复现命令见 §5。本清单与 `Cargo.lock` 绑定，**每次依赖变更后必须重跑**。

---

## 1. 自有许可

| 项目 | 许可 |
|---|---|
| rustmark 本体（`src/`、`tests/`、`scripts/`） | MIT OR Apache-2.0（二选一） |
| 商标 / 名称 | 见 `README.md` §8；图标与 Logo 素材见 `assets/logo/` |

## 2. Rust 依赖（Cargo 依赖图）

审计基线（2026-10-22 实测，`cargo-deny 0.20.2`，口径：`deny.toml` 的 4 个目标三元组 × `all-features`）：

- 依赖图 crate 数：**186**（`cargo deny list` 去重计数；含各平台专属包）
- 口径说明：此处 **186** 是「许可条目口径」的 crate 数；发布时 `tools/sbom.py` 生成的 CycloneDX SBOM 按组件口径统计为 **395**（同一 crate 的多目标/feature 实例会分别计入）。两数不同属正常，引用时勿混用。
- `cargo deny check licenses`：**通过（licenses ok）**
- 许可分布（一个 crate 多许可时按许可分别计数）：

| 许可 | 出现次数 | 备注 |
|---|---|---|
| MIT | 252 | 宽松许可 |
| Apache-2.0 | 209 | 宽松许可（含本项目自有许可之一） |
| Zlib | 17 | 宽松许可 |
| Unlicense | 8 | 公有领域奉献 |
| ISC | 6 | 宽松许可 |
| Apache-2.0 WITH LLVM-exception | 4 | 宽松许可 + LLVM 例外 |
| BSD-3-Clause | 3 | 宽松许可 |
| BSD-2-Clause | 2 | 宽松许可 |
| 0BSD | 1 | 宽松许可 |
| **GPL-2.0-only** | 1 | **仅 `self_cell` 的一个选项，本项目不采用，见 §3** |
| Unicode-3.0 | 1 | Unicode 数据表（`unicode-ident`） |

> **2026-10-22（REL-03）变化**：`epaint_default_fonts`（OFL-1.1 / Ubuntu-font-1.0）
> 与 `webpki-roots`（CDLA-Permissive-2.0）已不在依赖图中——前者随 `default_fonts` /
> `monochrome_emoji_fonts` 特性一起关闭，后者被平台信任库（`rustls-native-certs`）取代。
> 对应字体文件仍**随包分发**（见 §4.1），但已不属于「Rust 依赖」，故本表的
> OFL-1.1 / Ubuntu-font-1.0 / CDLA-Permissive-2.0 三行相应移除。

所有 crate 都能解析到 `deny.toml` 白名单内的许可（`A OR B` 表达式由 cargo-deny 取白名单分支），无 copyleft 强传染许可进入产物。

### 2.1 需要留意的依赖（43 个含非 MIT/Apache-2.0 选项的 crate）

`cargo deny` 对 `A OR B` 表达式自动取白名单内的一项；下表列出所有**除 MIT/Apache-2.0 之外还有其他许可选项**的 crate（含逐平台重复版本），供人工复核：

| crate | 版本 | 许可表达式 |
|---|---|---|
| `adler2` | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| `aho-corasick` | 1.1.5 | Unlicense OR MIT |
| `bytemuck` | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| `bytemuck_derive` | 1.12.1 | Zlib OR Apache-2.0 OR MIT |
| `byteorder-lite` | 0.1.0 | Unlicense OR MIT |
| `cursor-icon` | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| `dispatch2` | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| `foldhash` | 0.2.0 | Zlib |
| `glow` | 0.17.0 | MIT OR Apache-2.0 OR Zlib |
| `jiff` | 0.2.38 | Unlicense OR MIT |
| `jiff-core` | 0.1.1 | Unlicense OR MIT |
| `libloading` | 0.8.9 | ISC |
| `linux-raw-sys` | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `linux-raw-sys` | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `memchr` | 2.8.3 | Unlicense OR MIT |
| `miniz_oxide` | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| `miniz_oxide` | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| `moxcms` | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| `objc2-app-kit` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-foundation` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-graphics` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| `objc2-metal` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| `objc2-quartz-core` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| `pxfm` | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| `raw-window-handle` | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| `ring` | 0.17.14 | Apache-2.0 AND ISC |
| `rustix` | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `rustix` | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `rustls` | 0.23.46 | Apache-2.0 OR ISC OR MIT |
| `rustls-native-certs` | 0.8.5 | Apache-2.0 OR ISC OR MIT |
| `rustls-webpki` | 0.103.16 | ISC |
| `same-file` | 1.0.6 | Unlicense/MIT |
| `self_cell` | 1.3.0 | **Apache-2.0 OR GPL-2.0-only**（见 §3） |
| `subtle` | 2.6.1 | BSD-3-Clause |
| `unicode-ident` | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `untrusted` | 0.9.0 | ISC |
| `walkdir` | 2.5.0 | Unlicense/MIT |
| `winapi-util` | 0.1.11 | Unlicense OR MIT |
| `xkeysym` | 0.2.1 | MIT OR Apache-2.0 OR Zlib |
| `zerocopy` | 0.8.60 | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zerocopy-derive` | 0.8.60 | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zune-core` | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| `zune-jpeg` | 0.5.15 | MIT OR Apache-2.0 OR Zlib |

## 3. `self_cell` 的 GPL 选项处置（重要）

`self_cell` 1.3.0 的许可表达式是 **`Apache-2.0 OR GPL-2.0-only`**（双许可，二选一）。
本项目**明确选择 Apache-2.0 选项**使用与再分发该 crate，因此**不触发** GPL-2.0 的
copyleft 义务。

依据：Rust 生态通行规则——当 crate 提供多个许可选项时，下游可选择其中任意一项；
`deny.toml` 的 `[licenses].allow` 含 `Apache-2.0`，`cargo deny check licenses` 据此判定通过。

**再分发要求**：无论选哪一项，GPL-2.0 选项的存在本身**不产生**额外义务；但为可审计，
本清单固定记录该选择。若日后 `self_cell` 改为 GPL-only（不再是 OR），必须在升级前
发起评审——那会直接违反本项目的许可策略。

## 4. 随包分发的非代码资产

### 4.1 字体（`assets/fonts/`，随发行包以独立 `fonts/` 目录分发）

| 文件 | 版权 | 许可 |
|---|---|---|
| `JetBrainsMono-Regular.ttf` | Copyright 2020 The JetBrains Mono Project Authors | OFL-1.1 |
| `FiraCode-Regular.ttf` | Copyright 2012–2015 The Fira Code Project Authors | OFL-1.1 |
| `IBMPlexMono-Regular.ttf` | Copyright 2017 IBM Corp. | OFL-1.1 |
| `NotoSansSC-Subset.ttf`（子集化，未改字形） | Copyright 2014–2020 Adobe | OFL-1.1 |
| `DejaVuMathTeXGyre.ttf` | Bitstream Vera / DejaVu | Bitstream Vera + DejaVu 许可（允许再分发） |
| `Ubuntu-Light.ttf`（egui 默认比例字体） | Copyright 2011 Canonical Ltd. | Ubuntu-font-1.0（全文 `UFL.txt`） |
| `Hack-Regular.ttf`（egui 默认等宽字体） | Copyright (c) 2018 Source Foundry Authors | MIT（全文 `Hack-Regular.txt`） |
| `NotoEmoji-Regular.ttf`（emoji 兜底） | Copyright 2013 Google Inc. | OFL-1.1（全文 `OFL.txt`） |
| `emoji-icon-font.ttf`（emoji 次级兜底） | Copyright (c) 2014 John Slegers | MIT（全文 `emoji-icon-font-mit-license.txt`） |

OFL-1.1 全文见 `assets/fonts/OFL-1.1.txt`；逐字体版权与来源见
`assets/fonts/Fonts-LICENSE.txt`；DejaVu 许可全文见
`assets/fonts/DejaVuMathTeXGyre-LICENSE.txt`。上述许可文件**随发行包一并分发**。

OFL-1.1 要点：可随软件捆绑与再分发（含商业用途）；不得单独出售字体文件本身；
修改后分发须继续以 OFL-1.1 公开，且不得使用保留字体名（如 "JetBrains Mono"）。

### 4.2 egui 默认字体：由「内嵌」改为「随包分发」（2026-10-22，REL-03）

2026-10-22 之前，egui 的 `default_fonts` / `monochrome_emoji_fonts` 会把
`epaint_default_fonts` 的 5 个字体（Ubuntu-Light / Hack / NotoEmoji /
emoji-icon-font / egui-icons，合计 1,417,504 B）以 `include_bytes!` 编进可执行文件。

REL-03 起两个特性**均关闭**，前四个字体改为 §4.1 列出的随包文件（字节、版本与
内嵌时代完全一致，许可全文随附）；`egui-icons.ttf`（3,484 B）不随包——它只支撑
`egui::special_emojis` 的少数图标，本项目对该接口零引用，egui 自身绘制也不使用。

**结论：可执行文件内不再含有任何第三方字体数据**，全部字体以文件形式随发行包分发，
版权与许可条款见 `assets/fonts/Fonts-LICENSE.txt` 及各许可全文。

### 4.3 数据文件（仓库内 vendored）

| 文件 | 来源 | 许可 / 依据 |
|---|---|---|
| `crates/rustmark-infra/src/infrastructure/persistence/gbk_table.bin` | 由公开 Unicode / WHATWG 编码索引生成 | 生成数据，遵循上游（Unicode 许可 / 规范数据） |
| `crates/rustmark-infra/src/infrastructure/persistence/gb18030_ranges.bin` | WHATWG Encoding Standard `index-gb18030-ranges` | 规范数据（公有领域性质） |
| `crates/rustmark-infra/src/infrastructure/spell/words.txt` | **项目自编**英文常用词表（自初始提交 `338d333` 即在仓内，`git log --follow` 可追溯；3,984 条 / 去重 3,635，仅单词，无释义与例句） | 自有编译物（本项目双许可覆盖）；不含第三方受版权保护的文本。**后续规则**：若替换为第三方词表，落地前必须在 `NOTICE` 与本表登记来源与许可 |

## 5. 复现与审计命令

```bash
cargo install cargo-deny --locked      # 本机 0.20.2
cargo deny check licenses              # 许可白名单（本文档 §2 的来源）
cargo deny check advisories bans sources
```

- 配置：`deny.toml`（`allow` 白名单、置信度阈值 0.93、`wildcards = "deny"`、仅允许 crates.io 源）。
- CI：`.github/workflows/ci.yml` 的 `deny` job 每次推送执行四条检查。
- **本清单由 `cargo deny list` 导出**，与 `Cargo.lock` 强绑定；依赖变更后必须重跑并同步本文件。

## 6. 免责声明

本清单基于机器可识别的许可元数据（`Cargo.toml` 的 `license` 字段与 SPDX 表达式）生成，
不构成法律意见。如需对外发布前的正式法律确认，请由具备资质的法务复核。
