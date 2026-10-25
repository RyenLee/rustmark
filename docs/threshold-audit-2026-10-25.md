# 阈值清单与基线（2026-10-25）

- **扫描基线**：`574b946` + 2026-10-25 阈值整改工作树（未提交）
- **本版性质**：整改后复核。新增 6 项（撤销字节预算、图片分层上限、纹理字节预算、解码边长、2 条 NFR 断言），并更新搜索/文件树/字号档位等取值；共 **121 项**。
- **扫描范围**：`src/`（84 个 .rs）、`crates/*/src/`（247 个 .rs）、`build.rs`、`Cargo.toml`、
  `rust-toolchain.toml`、`.rustfmt.toml`、`deny.toml`、`.github/workflows/{ci,release}.yml`、
  `scripts/*`、`tools/coverage_gate.py`，以及 `tests/` 中构成 CI 门禁的性能断言。
- **收录判据**：约束「输入规模 / 资源用量 / 时间窗口 / 数量上限 / 比例区间」的边界值。
- **排除项**：纯几何排版常量（窗口与对话框宽高、padding、圆角、图标尺寸、绘制比例）；
  `tests/` 内部的数据构造尺寸与 fixture 参数（仅收录性能/覆盖率门禁断言）；
  `Cargo.lock` 的依赖版本；文档与注释中的说明性数字。
- **不存在的类别（已确认，非遗漏）**：仓库无 `.env` / `.env.example`，无 Dockerfile 或部署清单；
  运行时无重试、退避、并发度、限流、分页参数（本地单线程 GUI，网络仅图片下载一条同步路径，
  超时即失败、不重试）。偏好配置为 `settings.toml`（键名 `theme.*` / `editor.*`），
  **不存在**「一个阈值一个环境变量」的通用覆盖机制。
- **配套文件**：`docs/thresholds.baseline.yaml`（与本文同源生成，供后续校验脚本消费）；
  合理性判定与调整建议见 `docs/threshold-review-2026-10-25.md`。

> 说明：表中「可配置性」只描述**当前代码事实**；标注为硬编码的项，修改需改代码并过四门禁。

## 一、阈值清单

### 1. 构建工具链与 CI 门禁 — 21 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `toolchain_channel` | 1.99.0 工具链版本 | `rust-toolchain.toml:4` | 构建配置 | 全仓 Rust 工具链唯一真源，CI 不再另写版本号。文件缺失或版本低于此值即编译失败；升级需改这一处。 | 配置文件 rust-toolchain.toml |
| `rust_msrv` | 1.99 Rust 版本 | `Cargo.toml:78` | 构建配置 | 包级最低 Rust 版本声明，cargo 解析依赖时据此拒绝过低版本。与 toolchain_channel 须同步。 | 配置文件 Cargo.toml |
| `rustfmt_max_width` | 100 列 | `.rustfmt.toml:2` | 构建配置 | 格式化行宽硬上限，cargo fmt --check 的第一步门禁。超宽即 fmt 失败、CI 红。 | 配置文件 .rustfmt.toml |
| `deny_confidence_threshold` | 0.93 比例 | `deny.toml:27` | 构建配置 | 许可证文本识别的置信度下限。低于阈值的依赖无法自动判定许可，需手写 [[licenses.clarify]]，否则 licenses 检查失败。 | 配置文件 deny.toml |
| `artifact_size_limit_ci` | 11534336 字节 | `.github/workflows/ci.yml:60` | CI workflow | CI 体积红线（11 MiB），由 size_gate.sh 读取；越线 exit 1 使 check job 失败。按基线 10,487,296 B 尚有约 1.0 MiB 余量。 | 环境变量 SIZE_LIMIT_BYTES（CI env） |
| `artifact_size_warn_ci` | 11010048 字节 | `.github/workflows/ci.yml:61` | CI workflow | CI 体积预警线（10.5 MiB），越线只输出 ::warning 不失败；保留约 512 KiB 可行动窗口，当前产物不触发。 | 环境变量 SIZE_WARN_BYTES（CI env） |
| `artifact_size_limit_local` | 11534336 字节 | `scripts/size_gate.sh:25` | 发布脚本 | 本地与 CI 共用的体积红线默认值（11 MiB）；CI 经环境变量覆盖，故实际生效值取决于 CI。越线 exit 1。 | 环境变量 SIZE_LIMIT_BYTES（默认值） |
| `artifact_size_warn_local` | 11010048 字节 | `scripts/size_gate.sh:26` | 发布脚本 | 体积预警线默认值（10.5 MiB）；越线仅告警并打印距红线余量。 | 环境变量 SIZE_WARN_BYTES（默认值） |
| `package_size_limit_linux` | 11534336 字节 | `scripts/package-linux.sh:114` | 发布脚本 | Linux 打包后 tar.gz / AppImage 单产物红线（安装包口径，11 MiB）；越线 status=1 使打包失败。 | 硬编码字面量（脚本内） |
| `package_size_limit_macos` | 11534336 字节 | `scripts/package-macos.sh:109` | 发布脚本 | macOS .app.zip / .dmg 单产物红线（安装包口径，11 MiB）；越线 exit 1。 | 硬编码字面量（脚本内） |
| `package_size_limit_windows` | 11534336 字节 | `scripts/package-windows.ps1:73` | 发布脚本 | Windows zip 产物红线（安装包口径，11 MiB）；越线 throw 终止打包。 | 硬编码字面量（脚本内） |
| `ci_timeout_check` | 45 分钟 | `.github/workflows/ci.yml:76` | CI workflow | check job（三平台编译 + 全量测试）超时上限；超时即取消并判定失败。 | 配置文件 ci.yml |
| `ci_timeout_perf` | 30 分钟 | `.github/workflows/ci.yml:223` | CI workflow | perf job（release 性能冒烟）超时上限；超时即失败。 | 配置文件 ci.yml |
| `ci_timeout_coverage` | 30 分钟 | `.github/workflows/ci.yml:259` | CI workflow | coverage job（llvm-cov 全量插桩）超时上限；超时即无 lcov.info，门禁失败。 | 配置文件 ci.yml |
| `ci_timeout_release_dryrun` | 10 分钟 | `.github/workflows/ci.yml:313` | CI workflow | release-dryrun job 超时上限；离线演练为纯 Python，超时即流程异常。 | 配置文件 ci.yml |
| `ci_timeout_deny` | 20 分钟 | `.github/workflows/ci.yml:333` | CI workflow | deny job（依赖许可 / 安全公告）超时上限；超时即门禁未判定。 | 配置文件 ci.yml |
| `ci_timeout_release_build` | 45 分钟 | `.github/workflows/release.yml:48` | CI workflow | release 打包矩阵单平台超时上限；超时即该平台产物缺失。 | 配置文件 release.yml |
| `ci_timeout_release_publish` | 15 分钟 | `.github/workflows/release.yml:112` | CI workflow | release 汇总与 gh release create 的超时上限；超时即 tag 已推但 Release 未生成。 | 配置文件 release.yml |
| `coverage_overall_min` | 70 百分比 | `tools/coverage_gate.py:88` | 工具链门禁 | 整体行覆盖门限；低于「门限-2 个百分点」即 ::error 并 exit 1。 | 命令行参数 --overall（默认值） |
| `coverage_domain_min` | 90 百分比 | `tools/coverage_gate.py:89` | 工具链门禁 | 领域层（src/domain/**）行覆盖门限；低于 门限-2 即失败。 | 命令行参数 --domain（默认值） |
| `coverage_buffer` | 2 百分比 | `tools/coverage_gate.py:105` | 工具链门禁 | 覆盖率门限缓冲区：落在 [门限-2, 门限) 只告警，低于门限-2 才失败。 | 硬编码字面量（脚本内） |

### 2. 性能门禁（release 测试断言） — 21 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `perf_1mb_parse_p50` | 2000 毫秒 | `tests/integration/large_file_test.rs:159` | 测试门禁::integration | 1MB 文档解析 p50 上限（仅 release 生效）；超线测试失败，perf job 红。 | 硬编码字面量（测试断言） |
| `perf_1mb_first_screen_p50` | 500 毫秒 | `tests/integration/large_file_test.rs:177` | 测试门禁::integration | 1MB 文档首屏渲染 p50 上限（降级路径，仅 release）；超线判退化。 | 硬编码字面量（测试断言） |
| `perf_1mb_scroll_map_p50` | 200 毫秒 | `tests/integration/large_file_test.rs:196` | 测试门禁::integration | 1MB 文档滚动映射 p50 上限（仅 release）；超线说明映射退化为 O(全文)。 | 硬编码字面量（测试断言） |
| `perf_1mb_find_p50` | 1500 毫秒 | `tests/integration/large_file_test.rs:234` | 测试门禁::integration | 1MB 全文查找（忽略大小写）p50 上限（仅 release）；超线即命中扫描退化。 | 硬编码字面量（测试断言） |
| `perf_1mb_find_case_sensitive_p50` | 1500 毫秒 | `tests/integration/large_file_test.rs:260` | 测试门禁::integration | 1MB 全文查找（大小写敏感）p50 上限（仅 release）。 | 硬编码字面量（测试断言） |
| `perf_100k_lines_parse` | 5000 毫秒 | `tests/integration/large_file_test.rs:413` | 测试门禁::integration | 10 万行文本解析上限；该断言无 release 限定，debug 下同样生效。 | 硬编码字面量（测试断言） |
| `perf_long_url_parse` | 3000 毫秒 | `tests/integration/large_file_test.rs:458` | 测试门禁::integration | 10 万字符超长 URL 解析上限（防灾难性回溯）；debug 下同样生效。 | 硬编码字面量（测试断言） |
| `perf_10kb_parse_p95` | 50 毫秒 | `tests/integration/main.rs:492` | 测试门禁::integration | 10KB 文档解析 p95 上限（目标 5ms 的 10 倍宽限）；超线判数量级退化。 | 硬编码字面量（测试断言） |
| `perf_nfr_10kb_parse_p95` | 10 毫秒 | `tests/integration/main.rs:501` | 测试门禁::integration | NFR 合规断言（仅 release）：10KB 解析 p95 ≤10ms（指标 5ms + 2× 容差），与 50ms 的 smoke 门禁并存。 | 硬编码字面量（测试断言） |
| `perf_syntax_set_load` | 30 毫秒 | `tests/integration/main.rs:546` | 测试门禁::integration | syntect 语法集加载耗时上限；超线影响首屏高亮可用时间。 | 硬编码字面量（测试断言） |
| `perf_highlight_per_lang_warm` | 1000 微秒 | `tests/integration/main.rs:607` | 测试门禁::integration | 20 种语言热路径单语言高亮耗时上限（约 1ms）；超线逼近一帧预算。 | 硬编码字面量（测试断言） |
| `perf_10kb_incremental_frame_p95` | 40 毫秒 | `tests/presentation/highlight_perf_test.rs:212` | 测试门禁::presentation | 10KB 文档改一行后的增量帧 p95 上限（目标 10ms 的 4 倍宽限）。 | 硬编码字面量（测试断言） |
| `perf_nfr_10kb_highlight_settle` | 10 毫秒 | `tests/integration/main.rs:761` | 测试门禁::integration | NFR 合规断言（仅 release）：10KB 停手后一次性全文高亮 ≤10ms，正对 NFR-PERF-05 的口径。 | 硬编码字面量（测试断言） |
| `perf_1mb_degraded_frame_p95` | 16 毫秒 | `tests/presentation/highlight_perf_test.rs:258` | 测试门禁::presentation | 1MB 降级路径稳态滚动帧 p95 上限（60fps 单帧预算）；超线即掉帧。 | 硬编码字面量（测试断言） |
| `perf_long_line_p95_release` | 500 毫秒 | `tests/presentation/highlight_perf_test.rs:332` | 测试门禁::presentation | 超长单行（27 万字节）改一字符的 p95 上限（release）；超线判定需行级截断保护。 | 硬编码字面量（测试断言） |
| `perf_long_line_p95_debug` | 1500 毫秒 | `tests/presentation/highlight_perf_test.rs:330` | 测试门禁::presentation | 同一用例的 debug 阈值（release 的 3 倍）；只用于钉住“不卡死”，达标结论以 release 为准。 | 硬编码字面量（测试断言） |
| `perf_idle_frame_limit` | 8 / 24 毫秒 | `tests/presentation/textedit_frame_perf_test.rs:247` | 测试门禁::presentation | 编辑区空闲帧 p50 上限：文档 <1MiB 取 8ms，≥1MiB 取 24ms（release）；超线说明 O(全文) 工作回到每帧路径。 | 硬编码字面量（测试断言） |
| `perf_typing_frame_limit` | 16 / 64 毫秒 | `tests/presentation/textedit_frame_perf_test.rs:204` | 测试门禁::presentation | 编辑区击键帧 p95 上限：<1MiB 取 16ms，≥1MiB 取 64ms（release）；超线说明高亮防抖失效。 | 硬编码字面量（测试断言） |
| `perf_debug_multiplier` | 8 比例 | `tests/presentation/textedit_frame_perf_test.rs:249` | 测试门禁::presentation | debug 构建下空闲帧/击键帧阈值的统一放大倍数（debug 排版约慢一个数量级）。 | 硬编码字面量（测试断言） |
| `perf_search_tab_switch` | 100 毫秒 | `tests/presentation/search_perf_test.rs:225` | 测试门禁::presentation | 切到搜索标签首帧上限（1MB 文档 + 50 命中，仅 release）；超线即 UI 卡顿。 | 硬编码字面量（测试断言） |
| `perf_current_doc_search` | 10 毫秒 | `tests/presentation/search_perf_test.rs:327` | 测试门禁::presentation | 「当前文档」范围单次搜索上限（1000 文件工作区）；超线说明又按工作区全量读盘。该断言 debug 也生效。 | 硬编码字面量（测试断言） |

### 3. 文档与工作区 — 17 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `preferences_schema_version` | 1 版本 | `crates/rustmark-core/src/domain/preferences/aggregate.rs:12` | core::domain::preferences::aggregate | 偏好配置 schema 版本；版本不匹配时配置按迁移/回退处理，避免旧字段静默生效。 | 硬编码 const |
| `autosave_interval_default` | 30 秒 | `crates/rustmark-core/src/domain/preferences/value_objects/editor.rs:258` | core::domain::preferences::value_objects::editor | 自动保存默认间隔；None 关闭自动保存，0 被 is_valid 拒绝。间隔过短会频繁落盘，过长丢失窗口变大。 | 配置文件 settings.toml（editor.auto_save_interval）+ 硬编码默认值 |
| `undo_merge_window_ms` | 400 毫秒 | `crates/rustmark-core/src/domain/document/undo.rs:44` | core::domain::document::undo | 撤销合并窗口：窗口内连续同类编辑合并为一个撤销点。过短则撤销粒度碎，过长则一次撤销吃掉整段输入。 | 配置文件 settings.toml（editor.undo_merge_window_ms）+ 硬编码默认值 |
| `undo_max_depth` | 200 份快照 | `crates/rustmark-core/src/domain/document/undo.rs:45` | core::domain::document::undo | 每文档撤销栈最大深度；超出丢弃最旧快照。过大徒增内存，过小历史不够用。 | 硬编码 const（UndoPolicy::default_policy） |
| `undo_max_bytes` | 67108864 字节 | `crates/rustmark-core/src/domain/document/undo.rs:46` | core::domain::document::undo | undo + redo 两份栈的合计字节预算（64 MiB）；快照是全文副本，只按条数限制会让 1MB 文档最坏占到 200 MiB。超限先丢 redo 远端，再丢 undo 最旧项。 | 硬编码 const（UndoPolicy::default_policy）＋ with_max_bytes 可覆盖 |
| `max_open_documents_default` | 12 个 | `crates/rustmark-core/src/domain/preferences/value_objects/editor.rs:151` | core::domain::preferences::value_objects::editor | 同时打开文档数默认上限；达到上限后打开新文件需先关闭。低于 12 会碍事，高于 30 标签条已无法辨认。 | 配置文件 settings.toml（editor.max_open_documents）+ 硬编码默认值 |
| `max_open_documents_max` | 30 个 | `crates/rustmark-core/src/domain/preferences/value_objects/editor.rs:157` | core::domain::preferences::value_objects::editor | 打开文档数可选上限（设置页滑块量程上端）；超过 30 无实际收益。 | 硬编码 const |
| `max_open_documents_min` | 1 个 | `src/presentation/views/settings_view.rs:83` | presentation::views::settings_view | 设置页滑块下限；1 表示「一次只看一份」也是合法用法。 | 硬编码字面量（表现层量程） |
| `recent_files_limit` | 20 条 | `crates/rustmark-core/src/domain/workspace/value_objects.rs:73` | core::domain::workspace::value_objects | 最近文件列表持久化条数上限；超出按最近打开时间淘汰。 | 硬编码 const |
| `welcome_recent_shown` | 5 条 | `src/presentation/views/welcome_view.rs:58` | presentation::views::welcome_view | 欢迎页展示的最近文件条数；只影响呈现层，不裁剪持久化列表。 | 硬编码 const |
| `tree_max_depth` | 8 层 | `crates/rustmark-core/src/application/workspace/service.rs:48` | core::application::workspace::service | 文件树扫描深度上限；超深目录被截断，避免误指 node_modules 之类拖死 UI。 | 硬编码 const |
| `tree_max_entries` | 10000 条 | `crates/rustmark-core/src/infrastructure/persistence/fs_workspace_repository.rs:60` | core::infrastructure::persistence::fs_workspace_repository | 单次文件树遍历条目数硬上限；超出即停止扫描，表现为树不完整而非卡死。 | 硬编码字面量（TraversalRules 默认值） |
| `search_hits_per_file` | 100 处 | `crates/rustmark-core/src/application/workspace/service.rs:39` | core::application::workspace::service | 跨文件搜索单文件命中上限；超出该文件的后续命中被丢弃，并置 truncated。 | 硬编码 const |
| `search_hits_total` | 1000 处 | `crates/rustmark-core/src/application/workspace/service.rs:41` | core::application::workspace::service | 搜索总命中上限（1000）；达到后置 truncated 标志并停止扫描，UI 明确显示已截断。 | 硬编码 const |
| `search_snippet_context` | 40 字节 | `crates/rustmark-core/src/application/workspace/service/search.rs:508` | core::application::workspace::service::search | 命中摘要前后各取的字节数（随后对齐 UTF-8 边界）；24 字节在混合排版下上下文过短，放宽到 40。 | 硬编码字面量 |
| `draft_preview_chars` | 80 字符 | `src/app/frame/recovery.rs:19` | app::frame::recovery | 崩溃恢复草稿的标题预览长度；超出截断，仅用于列表呈现。 | 硬编码字面量 |
| `doc_view_preview_chars` | 120 字符 | `crates/rustmark-core/src/application/document/service/mapping.rs:57` | core::application::document::service::mapping | 文档详情视图的正文预览长度；该函数会读全文，故上限同时限制快照成本。 | 硬编码字面量 |

### 4. 渲染、防抖与缓存 — 13 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `truncate_threshold_bytes` | 512000 字节 | `crates/rustmark-core/src/application/markdown/service.rs:61` | core::application::markdown::service | 源文档超过 500 KiB 即标记截断预览（FR-06），不调用解析器；用户看到的是降级纯文本。 | 硬编码 const |
| `render_cache_capacity` | 32 条 | `crates/rustmark-core/src/application/markdown/service.rs:69` | core::application::markdown::service | 渲染缓存 LRU 容量（按文档）；超出驱逐旧条目，再次切换标签会重解析。 | 硬编码 const（装配处引用） |
| `highlight_cache_capacity` | 32 条 | `crates/rustmark-core/src/application/markdown/service.rs:78` | core::application::markdown::service | 高亮缓存 + 防抖计时表 + 拼写缓存共用的容量；小于同时打开文档数会反复触发全文重算。 | 硬编码 const |
| `highlight_checkpoint_lines` | 25 行 | `crates/rustmark-core/src/infrastructure/highlighting/incremental.rs:55` | core::infrastructure::highlighting::incremental | 增量高亮检查点间隔；越小恢复点越近但检查点内存越大，越大则增量收益下降。 | 硬编码 const |
| `max_incremental_bytes` | 512000 字节 | `crates/rustmark-core/src/infrastructure/highlighting/incremental.rs:58` | core::infrastructure::highlighting::incremental | 超过该字节数不做增量高亮（降级纯文本）；与截断阈值、排版阈值共用同一常量。 | 硬编码 const |
| `large_file_bytes_ui` | 512000 字节 | `crates/rustmark-ui/src/presentation/theme/mapper/galley_cache.rs:83` | ui::presentation::theme::mapper::galley_cache | UI 排版侧的大文件阈值：超过后停止 span 着色，只做纯文本排版（FR-06）。 | 硬编码 const |
| `preview_debounce_ms` | 150 毫秒 | `src/app/bootstrap.rs:65` | app::bootstrap | 预览防抖窗口（FR-01），装配 DebouncePolicy 时传入；打字期不重排，停手 150ms 后渲染。 | 硬编码 const |
| `preview_debounce_ms_ui` | 150 毫秒 | `crates/rustmark-ui/src/presentation/state/preview_ui_state.rs:22` | ui::presentation::state::preview_ui_state | 表现层另一份预览防抖常量，用于判断预览是否到期；与装配层同名同值但独立定义。 | 硬编码 const |
| `stats_debounce_ms` | 150 毫秒 | `src/app/frame.rs:148` | app::frame | 状态栏字数统计防抖窗口；1MB 文档 compute_stats 约 9.5ms，故等停手后算一次。 | 硬编码 const |
| `image_texture_cache` | 64 条 | `src/presentation/views/preview_view/image_cache.rs:56` | presentation::views::preview_view::image_cache | 预览图片纹理缓存的条目上限（兜底）；主约束是字节预算，见 image_texture_budget。 | 硬编码 const |
| `image_texture_budget` | 134217728 字节 | `src/presentation/views/preview_view/image_cache.rs:63` | presentation::views::preview_view::image_cache | 纹理缓存字节预算（128 MiB，按解码后 W×H×4 结算）；超限从最旧开始淘汰，避免大图把显存吃穿。 | 硬编码 const |
| `image_decode_max_edge` | 1600 像素 | `src/presentation/views/preview_view/image_cache.rs:70` | presentation::views::preview_view::image_cache | 解码最长边上限；超出先等比缩到 1600px 再建纹理，一张 6000×4000 相机图从 96 MiB 降到约 6 MiB。 | 硬编码 const |
| `embedded_math_mermaid_cache` | 32 条 | `src/presentation/views/preview_view/embedded_cache.rs:36` | presentation::views::preview_view::embedded_cache | 公式与 Mermaid 排版结果记忆化容量；超出 LRU 淘汰后需重排。 | 硬编码 const |

### 5. 资源加载、截图与导出 — 9 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `asset_max_inline_bytes` | 512000 字节 | `crates/rustmark-core/src/infrastructure/persistence/asset_loader/policy.rs:29` | core::infrastructure::persistence::asset_loader::policy | data URI 内联体积硬上限：写进 Markdown 文本的数据过大既撑爆文档也拖慢解析；超限返回 TooLarge。 | 硬编码字面量（AssetLimits::default） |
| `asset_max_local_bytes` | 8388608 字节 | `crates/rustmark-core/src/infrastructure/persistence/asset_loader/policy.rs:30` | core::infrastructure::persistence::asset_loader::policy | 本地文件与远程下载的体积上限（8 MiB）；本地在读盘前按元数据预检，远程边读边判。超限返回 TooLarge 并显示占位。 | 硬编码字面量（AssetLimits::default） |
| `asset_download_timeout` | 10 秒 | `crates/rustmark-core/src/infrastructure/persistence/asset_loader/policy.rs:31` | core::infrastructure::persistence::asset_loader::policy | 远程图片下载总超时（10s，全局 deadline 含 TLS 与重定向）；超时即报 DownloadFailed，预览显示占位。 | 硬编码字面量（AssetLimits::default） |
| `asset_remote_stream_cap` | 8388608 字节 | `crates/rustmark-core/src/infrastructure/persistence/asset_loader.rs:317` | core::infrastructure::persistence::asset_loader | 远程下载边读边判的瞬时上限（= 本地上限 8 MiB），超过即中止，不再先读到 2× 再拒绝。 | 硬编码字面量（复用 max_local_bytes） |
| `image_dedupe_attempts` | 1000 次 | `crates/rustmark-core/src/infrastructure/persistence/image_store.rs:31` | core::infrastructure::persistence::image_store | 同名图片落盘的序号去重上限；1000 次仍冲突则返回可读错误，避免无限循环。 | 硬编码 const |
| `capture_max_dimension` | 8192 像素 | `crates/rustmark-core/src/infrastructure/export/viewport_capture.rs:110` | core::infrastructure::export::viewport_capture | 截图单轴边长上限（约 A4@300dpi）；超限拒绝导出，避免单张图先吃数百 MB 再失败。 | 硬编码 const |
| `capture_max_long_height` | 16384 像素 | `crates/rustmark-core/src/infrastructure/export/viewport_capture.rs:117` | core::infrastructure::export::viewport_capture | 长图拼接成品高度上限（约 131MB 内存峰值）；超限建议改用 HTML/PDF 导出。 | 硬编码 const |
| `long_image_max_segments` | 256 段 | `src/app/frame/capture.rs:53` | app::frame::capture | 整篇长图捕获的最大滚动段数；超限中止并提示改用文档导出。 | 硬编码 const |
| `crash_reports_retained` | 10 份 | `src/app/crash_log.rs:164` | app::crash_log | 崩溃报告保留份数；每次启动按文件名降序清理更旧的，防崩溃风暴吃满磁盘。 | 硬编码 const |

### 6. 拼写与 Markdown 规则 — 9 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `spell_min_check_len` | 3 字符 | `crates/rustmark-core/src/infrastructure/spell/simple_checker.rs:32` | core::infrastructure::spell::simple_checker | 短于该长度的词不判错；阈值过低会把缩写与噪声全部标红。 | 硬编码 const |
| `spell_max_edit_dist` | 1 级编辑距离 | `crates/rustmark-core/src/infrastructure/spell/simple_checker.rs:35` | core::infrastructure::spell::simple_checker | 判错时的最大编辑距离；放宽会把整篇技术文档标红（误报优先收紧）。 | 硬编码 const |
| `spell_max_suggest_dist` | 2 级编辑距离 | `crates/rustmark-core/src/infrastructure/spell/simple_checker.rs:42` | core::infrastructure::spell::simple_checker | 建议候选的最大编辑距离，比判错阈值宽一档；放宽只增加候选，不会误标红。 | 硬编码 const |
| `spell_suggest_limit` | 5 条 | `src/presentation/views/editor_view/spell.rs:16` | presentation::views::editor_view::spell | 右键拼写建议菜单最大条数；超过 5 条菜单过高且用户只看前几条。 | 硬编码 const |
| `heading_max_level` | 6 级 | `crates/rustmark-core/src/domain/document/services/headings.rs:26` | core::domain::document::services::headings | ATX 标题层级上限；7 个及以上 # 按 6 级处理，保证大纲缩进不越界。 | 硬编码字面量（CommonMark 规则） |
| `table_insert_default` | 3×3 行×列 | `crates/rustmark-core/src/application/document/service/editing.rs:22` | core::application::document::service::editing | 「插入表格」生成的默认骨架尺寸；固定 3×3，后续由用户增删行列。 | 硬编码 const |
| `table_max_rows` | 100 行 | `crates/rustmark-core/src/domain/document/services/compose.rs:112` | core::domain::document::services::compose | 程序化插入表格的行数上限；超出被夹紧到 100，防止一次生成超长表格。 | 硬编码字面量（clamp 上界） |
| `table_max_cols` | 32 列 | `crates/rustmark-core/src/domain/document/services/compose.rs:113` | core::domain::document::services::compose | 程序化插入表格的列数上限；超出夹紧到 32，避免超宽表格撑破预览。 | 硬编码字面量（clamp 上界） |
| `reading_wpm` | 300 词/分钟 | `crates/rustmark-core/src/application/document/service/stats.rs:93` | core::application::document::service::stats | 阅读时长的每分钟字数基准；用于把字数换算为分钟数，值越大估计越短。 | 硬编码字面量 |

### 7. UI 边界与交互窗口 — 19 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `window_min_width` | 960 点 | `src/app/frame.rs:682` | app::frame | 主窗口最小宽度；小于该值布局无法保证编辑区可用（侧栏动态上界的下限由此兜底）。 | 硬编码字面量 |
| `window_min_height` | 600 点 | `src/app/frame.rs:682` | app::frame | 主窗口最小高度；低于该值顶栏 + 状态栏 + 正文无法同时容纳。 | 硬编码字面量 |
| `split_min` | 0.2 比例 | `crates/rustmark-ui/src/presentation/state/app_ui_state.rs:57` | ui::presentation::state::app_ui_state | 编辑区/预览区分割比例下界；拖拽夹紧，防止一侧被拖到完全消失。 | 硬编码 const |
| `split_max` | 0.8 比例 | `crates/rustmark-ui/src/presentation/state/app_ui_state.rs:59` | ui::presentation::state::app_ui_state | 分割比例上界；与下界对称，保证另一侧仍够看全文。 | 硬编码 const |
| `sidebar_min` | 180 点 | `crates/rustmark-ui/src/presentation/state/app_ui_state.rs:64` | ui::presentation::state::app_ui_state | 侧栏宽度静态下界；再窄活动栏（48pt）+ 内容面板放不下文件树一行。 | 硬编码 const |
| `sidebar_max` | 560 点 | `crates/rustmark-ui/src/presentation/state/app_ui_state.rs:69` | ui::presentation::state::app_ui_state | 侧栏宽度静态上界；再宽会把编辑区压到角落。 | 硬编码 const |
| `sidebar_body_share_max` | 0.6 比例 | `crates/rustmark-ui/src/presentation/state/app_ui_state.rs:76` | ui::presentation::state::app_ui_state | 侧栏占主体区宽度的动态上界（窄窗口兜底）；展开为 sw ≤ 0.375×(total-divider)。 | 硬编码 const |
| `sidebar_default_width` | 308 点 | `crates/rustmark-ui/src/presentation/theme/builtin/metrics.rs:85` | ui::presentation::theme::builtin::metrics | 侧栏默认宽度（未在 settings.toml 指定时）；低于 180 会被夹紧。 | 配置文件 settings.toml（editor.sidebar_width）+ 硬编码默认值 |
| `preview_image_max_w` | 620 点 | `src/presentation/views/preview_view.rs:120` | presentation::views::preview_view | 预览图片最大显示宽度；超过等比缩放，避免小图放大或大图撑破面板。 | 硬编码 const |
| `message_ttl_ms` | 4000 毫秒 | `crates/rustmark-ui/src/presentation/state/status_bar_ui_state.rs:28` | ui::presentation::state::status_bar_ui_state | 状态栏消息默认存活时长；过短看不清，过长会让人以为程序卡住。 | 硬编码 const |
| `scroll_suppress_ms` | 100 毫秒 | `crates/rustmark-ui/src/presentation/state/scroll_sync_state.rs:36` | ui::presentation::state::scroll_sync_state | 滚动同步回声抑制窗口；短于帧间隔覆盖不住回声，长于 150ms 会让用户觉得前 0.1 秒没反应。 | 硬编码 const |
| `scroll_move_eps` | 0.5 点 | `crates/rustmark-ui/src/presentation/state/scroll_sync_state.rs:42` | ui::presentation::state::scroll_sync_state | 判定「偏移真的变了」的容差；取 0 会把静止帧的像素取整抖动误判为用户滚动。 | 硬编码 const |
| `find_scroll_eps` | 0.5 点 | `src/app/frame/find.rs:114` | app::frame::find | 查找跳转时「需要改滚动位置」的容差；与 MOVE_EPS 同值但独立定义。 | 硬编码字面量 |
| `ime_guard_ms` | 800 毫秒 | `src/app/frame.rs:412` | app::frame | IME 组合期冻结窗口；每次 Preedit 续期。过短会中途解冻打掉预编辑选区，过长则回车被吞。 | 硬编码 const |
| `ime_arm_ms` | 50 毫秒 | `src/app/frame.rs:431` | app::frame | egui ime_enabled 复位窗口；宁短勿长（过长等于冻结回车）。 | 硬编码 const |
| `scheduler_min_delay_ms` | 1 毫秒 | `src/app/frame/scheduler.rs:130` | app::frame::scheduler | 帧调度最小重绘延迟；已过期任务也至少等 1ms，避免 0ms 造成静止应用忙循环。 | 硬编码字面量 |
| `quick_open_candidates` | 12 条 | `crates/rustmark-ui/src/presentation/state/quick_open_ui_state.rs:26` | ui::presentation::state::quick_open_ui_state | 快速打开候选上限；再多需滚动，违背「不打字就选」的定位。 | 硬编码 const |
| `diff_max_cells` | 2000000 单元格（行×列） | `crates/rustmark-ui/src/presentation/state/diff_ui_state/model.rs:7` | ui::presentation::state::diff_ui_state::model | 差异比较规模上限（行数乘积，约 1400×1400）；超过降级，避免 Myers 算法等待过久。 | 硬编码 const |
| `find_max_matches` | 100000 处 | `crates/rustmark-ui/src/presentation/state/find_replace_ui_state/engine.rs:35` | ui::presentation::state::find_replace_ui_state::engine | 单次查找命中上限；超过即停止收集，防止退化模式把内存吃满。 | 硬编码 const（函数内） |

### 8. 偏好默认值（settings.toml） — 9 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `font_size_default` | 14 像素 | `crates/rustmark-core/src/domain/preferences/value_objects/theme.rs:198` | core::domain::preferences::value_objects::theme | 默认正文字号；影响行高与排版缓存键。 | 配置文件 settings.toml（theme.font_size）+ 硬编码默认值 |
| `font_size_range` | 8.0-48.0 像素 | `crates/rustmark-core/src/domain/preferences/value_objects/theme.rs:188` | core::domain::preferences::value_objects::theme | 字号合法区间（含端点）；越界配置被 is_valid 拒绝并整体回退默认。 | 硬编码 const |
| `line_height_default` | 1.6 比例 | `crates/rustmark-core/src/domain/preferences/value_objects/theme.rs:199` | core::domain::preferences::value_objects::theme | 默认行高倍数；过小中文拥挤，过大一屏信息量骤降。 | 配置文件 settings.toml（theme.line_height）+ 硬编码默认值 |
| `line_height_range` | 1.0-3.0 比例 | `crates/rustmark-core/src/domain/preferences/value_objects/theme.rs:190` | core::domain::preferences::value_objects::theme | 行高合法区间；1.0 为紧贴（字形可能裁切），3.0 为极端稀疏。 | 硬编码 const |
| `settings_font_presets` | 8,10,12,13,14,15,16,18,20,24,32,48 像素 | `src/presentation/views/settings_view.rs:67` | presentation::views::settings_view | 设置页可选字号档位；现已覆盖 8-48 合法区间的端点与常用档，配置任意合法值都有选中态。 | 硬编码字面量（表现层） |
| `settings_line_height_presets` | 1.0,1.2,1.4,1.5,1.6,1.7,1.8,2.0,2.4,3.0 比例 | `src/presentation/views/settings_view.rs:71` | presentation::views::settings_view | 设置页可选行高档位；现已覆盖 1.0-3.0 的端点与常用档。 | 硬编码字面量（表现层） |
| `tab_size_default` | 4 空格 | `crates/rustmark-core/src/domain/preferences/value_objects/editor.rs:256` | core::domain::preferences::value_objects::editor | 默认缩进宽度；枚举只允许 2/4/8，其它值无法表示。 | 配置文件 settings.toml（editor.tab_size）+ 硬编码默认值 |
| `split_ratio_default` | 0.5 比例 | `crates/rustmark-core/src/domain/preferences/value_objects/editor.rs:262` | core::domain::preferences::value_objects::editor | 默认分栏比例（编辑区:预览区 = 1:1）；渲染时再夹紧到 SPLIT_MIN-SPLIT_MAX。 | 配置文件 settings.toml（editor.split_ratio）+ 硬编码默认值 |
| `writing_goal_max` | 1000000 字符 | `crates/rustmark-core/src/domain/preferences/value_objects/editor.rs:63` | core::domain::preferences::value_objects::editor | 写作目标字数上限；超出饱和到 100 万而非报错，避免设置整体保存失败。 | 硬编码 const |

### 9. 运行时环境变量 — 3 项

| 键名 | 当前值（单位） | 位置（文件:行） | 所属模块 | 用途 / 击穿后果 | 可配置性 |
|---|---|---|---|---|---|
| `env_skip_arch_check` | 任意值 开关 | `build.rs:47` | 构建配置 | 设置后跳过构建期架构边界校验（逃生口）；CI 不得设置，否则分层违规不再拦截构建。 | 环境变量 SKIP_ARCH_CHECK |
| `env_startup_timing` | 1 / true 开关 | `src/app/startup_timing.rs:34` | app::startup_timing | 开启启动阶段耗时埋点；仅接受字符串 1 或 true，其它值视作关闭。 | 环境变量 RUSTMARK_STARTUP_TIMING |
| `env_log_default` | warn 日志级别 | `src/main.rs:23` | 入口 | RUST_LOG 未设时的默认日志级别；warn 保证告警与错误可见，debug 需显式开启。 | 环境变量 RUST_LOG（默认值 warn） |

## 二、分布概览

- 合计 **121 项**，按 9 个模块分组；来源构成：硬编码 101 项、
  配置文件 19 项、环境变量 7 项、命令行默认值 2 项（同一项可能兼有两类来源）。
- 阈值密度最高的是「构建工具链与 CI 门禁」与「UI 边界与交互窗口」：前者几乎全是
  发布纪律（体积 / 覆盖 / 超时），后者是用户可感知的边界（分栏、侧栏、滚动、IME）。
- 同一语义被多处复制是本次扫描最突出的模式：体积红线 7 处、150ms 防抖 3 处、
  500KB 大文件阈值 3 处、0.5pt 滚动容差 2 处、打开文档上限 2 处，均列入风险项。


## 三、风险项

以下问题均来自上表的重复定义或口径不一致，按影响面排序。**本次未修改任何代码**。

| 编号 | 风险 | 位置 | 问题 | 潜在影响 |
|---|---|---|---|---|
| R-1 | 体积门禁七处定义、两套口径 | .github/workflows/ci.yml:59-60；scripts/size_gate.sh:25-26；scripts/package-{linux,macos}.sh / package-windows.ps1 | NFR-PERF-01 与 size_gate.sh 已改为 11 MiB 红线 / 10 MiB 预警，但 CI env 仍写 10485760 / 8388608，三个打包脚本仍硬编码 10485760。CI 的 SIZE_LIMIT_BYTES 优先于脚本默认值，故实际执行的是旧的 10 MiB 红线。 | 按 docs/metrics.md 与 TD-01 记载的 Windows 发布基线 10,487,296 B 推算，已超过 10,485,760 B 达 1,536 B：CI 的 size gate 将 exit 1、check job 红；打包脚本亦会拦截（macOS / Linux 产物未真机复测，结论待验证）。预警线 8 MiB 低于当前产物，告警常亮、失去区分度。 |
| R-2 | 「大文件」阈值三处定义、两种取值 | crates/rustmark-core/src/application/markdown/service.rs:58（500*1024）；crates/rustmark-ui/src/presentation/theme/mapper/galley_cache.rs:81（500*1024）；crates/rustmark-core/src/infrastructure/highlighting/incremental.rs:58（512*1024） | 同一语义（超过即降级/停止着色）被复制三份；incremental.rs 注释写明「与现有 >500KB 降级对齐」，实际值为 512 KiB = 524,288 B，与 500 KiB = 512,000 B 相差 12,288 B。 | 512,000-524,288 B 之间的文档会出现「预览已降级但增量高亮仍启用」的口径分裂；后续调整阈值需改三处，漏改即行为不一致。 |
| R-3 | 预览防抖 150ms 两份常量 + 统计防抖第三份 | src/app/bootstrap.rs:63；crates/rustmark-ui/src/presentation/state/preview_ui_state.rs:21；src/app/frame.rs:148 | 同名 PREVIEW_DEBOUNCE_MS 在组合根与表现层各定义一次（值相同），统计防抖另立 STATS_DEBOUNCE_MS 同值。三处无编译期关联，注释均声称「与另一处一致」。 | 任一处的调整不会传导到其余两处，将出现「预览已刷新、计数未刷新」或反之的时序错位；FR-01 的 150ms 事实上是三个独立事实。 |
| R-4 | 字号/行高的合法区间与设置页档位不一致 | crates/rustmark-core/src/domain/preferences/value_objects/theme.rs:188,190；src/presentation/views/settings_view.rs:66,69；src/presentation/views/settings_view/appearance.rs:50,61 | 领域允许 8-48 px / 1.0-3.0，设置页只提供 12-20 px（7 档）/ 1.4-2.0（6 档），且选中判定用 f32::EPSILON 严格相等。 | 手改 settings.toml 为 8、48 或 1.0 时配置合法但设置页无任何档位高亮，用户无法判断当前值、也无法用 UI 回到该值；只能继续手改文件。 |
| R-5 | 打开文档上限在领域与表现层各写一遍 | crates/rustmark-core/src/domain/preferences/value_objects/editor.rs:157（30）；src/presentation/views/settings_view.rs:81（1..=30） | check-arch 禁止表现层依赖 domain，故量程端点被复制到视图层，仅靠注释声明「与领域同源」。 | 两侧任一改动都会让滑块量程与校验上限脱节：滑到 30 但领域只接受 20（或反之），保存时被 is_valid 拒绝并整份回退默认。 |
| R-6 | 本地图片先全量读入再判 512KB 上限 | crates/rustmark-core/src/infrastructure/persistence/asset_loader.rs:209（fs::read）与 :131（体积校验） | 远程路径是边读边判（:291），data URI 解码后判；唯独本地文件用 std::fs::read 一次性读入，再在 load 出口拒绝超限。 | 引用一张数百 MB 的本地图片时，进程会先把整份内容读进内存才报 TooLarge，可能触发内存尖峰甚至 OOM（panic=abort 下直接终止），而错误信息只显示「超过上限」。 |
| R-7 | 搜索命中硬截断且无分页 | crates/rustmark-core/src/application/workspace/service.rs:39,41；service/search.rs:130 | 单文件 50 / 总量 300 命中即停止扫描并置 truncated，没有「加载更多」或分页参数。 | 在大仓库或高频词检索时结果静默不全；若 UI 未显著呈现 truncated 标志，用户会误判「只有 300 处」。命中上限同时是唯一的分页替代物，无法按需放宽。 |
| R-8 | 滚动变化容差 0.5pt 两处独立定义 | crates/rustmark-ui/src/presentation/state/scroll_sync_state.rs:42（MOVE_EPS）；src/app/frame/find.rs:114（字面量 0.5） | 同一物理含义（偏移变化是否值得处理）在两处各写一次，后者未引用前者。 | 调整 MOVE_EPS 时查找跳转仍用 0.5，可能出现「滚动同步认为没动、查找认为动了」的不一致；亦无法统一按 DPI/缩放调整。 |

## 四、基线文件说明

- `docs/thresholds.baseline.yaml` 按 9 个模块分组，字段：`value` / `unit` / `source` /
  `purpose` / `recommended` / `configurable`。`source` 与上表逐字一致，可直接用于
  「读源码行 → 比对 value」的校验脚本。
- `recommended` 是**建议区间**（工程判断），不是代码事实；收紧或放宽须走评审并同步
  `docs/architecture-constraints.md` 的预算章节。
- 建议把 R-1（体积红线）与 R-2（大文件阈值）作为第一批收敛项：前者已使 CI 处于必红状态，
  后者是同一语义的三份真源。
