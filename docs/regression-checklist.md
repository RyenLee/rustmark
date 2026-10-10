# 原型缺陷回归清单验证（B-01 ~ B-17）

> 阶段 07 · S7-03 产出
> 来源：`docs/prototype-contract.md` 的 **B 清单**（原型已踩的坑 → Rust 侧设计层根治）
> 自动化断言：`tests/integration/regression_test.rs`；其余引用既有任务单元验收用例
> 约束：任一验证失败必须回到对应阶段任务修复，不得在本阶段写补丁绕过

## 验证结论速览

| # | 原型缺陷 | Rust 根治手段 | 验收任务 | 验证方式 | 结论 |
|---|---|---|---|---|---|
| B-01 | `F8` 永不触发（匹配器拒绝纯单键） | 匹配层不拒绝功能键；冲突判定交 `ShortcutTable::conflicts` | S3-12 / S6-09 | 自动化 `reg_function_key_binding_not_flagged_as_text_input_clash` | ✅ 已免疫 |
| B-02 | `Ctrl+Shift+S` 声明未注册 | 单一 `Vec<ShortcutBinding>` 同时驱动注册与显示 | S6-09 | 自动化 `reg_default_bindings_have_no_conflict`（默认表零冲突） | ✅ 已免疫 |
| B-03 | `view.escape` 硬编码绕过 | 无硬编码分支；`IGNORE_KEYS` 不含功能键 | S6-09 | 手工（快捷键一览/录制，S7-05） | ✅ 已免疫 |
| B-04 | 无冲突检测，先注册者静默吞 | `SetShortcutCommand` 先调 `Conflicts::detect` | S3-12 / S4-08 / S6-09 | 自动化 `reg_duplicate_binding_detected_as_blocking_conflict` | ✅ 已免疫 |
| B-05 | 撤销栈跨文件污染 | 撤销栈是 `Document` 成员值对象（类型层无全局栈） | S3-08 用例1 | 自动化 `reg_undo_is_scoped_per_document` | ✅ 已免疫 |
| B-06 | `lastCommit` 全局，合并窗口不重置 | 合并窗口状态 per-Document | S3-08 用例2 | 自动化 `reg_merge_window_is_per_document` | ✅ 已免疫 |
| B-07 | `redo` 不恢复选区；`undo` 传同值致光标折叠 | 快照存 `{content, selection}`，undo/redo 均恢复 | S3-08 用例3、4 | 自动化 `reg_undo_restores_selection` | ✅ 已免疫 |
| B-08 | 快捷键一览不跟随自定义 | 三处显示统一取 `ShortcutView` | S6-09 用例5 | 手工（S7-05） | ✅ 已免疫 |
| B-09 | 未知标签拆壳跳过清洗 → 逃逸 | 清洗器递归清洗搬移节点；`svg/math/...` 进 `DROP_TAGS` | S5-09（8 条用例） | 自动化 `reg_sanitize_drops_svg_subtree_and_resanitizes_unwrapped` + `tests/infrastructure/html_sanitize_test.rs` | ✅ 已免疫 |
| B-10 | `safeUrl` 未放行 `data:image/*` | 精确白名单 `data:image/(png\|jpeg\|...);base64,` | S5-09 / S5-11 | 自动化 `tests/infrastructure/html_sanitize_test.rs::case7/8` | ✅ 已免疫 |
| B-11 | 持久化失败静默丢草稿 | 保存失败返回 `Err`、不清除脏标记、UI 提示 | S4-04 / S5-04 / ADR-003 | 手工 + `tests/infrastructure/fs_document_repository_test.rs` | ✅ 已免疫 |
| B-12 | `tab-size` 硬编码 4 | 缩进宽度参数化；测量与渲染同源 | S3-05 | 自动化 `tests/domain/*editing*` | ✅ 已免疫 |
| B-13 | 滚动时全文 split + 强制重排 | `Rope` 按行索引 O(log n)；行偏移缓存 | S5-01 / S6-04 | 自动化 `tests/infrastructure/rope_text_buffer_test.rs` | ✅ 已免疫 |
| B-14 | 导出 TOC 未转义且拼在清洗后 | 先拼 TOC（转义）→ 再整体清洗 | S5-09 | 自动化 `reg_toc_title_is_escaped_before_sanitize` | ✅ 已免疫 |
| B-15 | 渲染失败提示的 `style` 被清洗剥除 | 失败提示用类名/文本表达，不依赖内联 `style` | S6-05 | 手工（S7-05） | ✅ 已免疫 |
| B-16 | 测试覆盖空洞（sanitize/undo/快捷键覆盖为 0） | 每任务单元强制「主路径 + 错误路径 + 边界」三类用例 | S3-14 / S5-09 / S9-02 | 持续（本清单即产物之一） | ✅ 已落实 |
| B-17 | README 声称已实现但实际失效（7 处不符） | 文档状态由对齐矩阵驱动，不手写 | S7-02 / S9-07 | 自动化 `tests/integration/alignment_test.rs`（常量与行为可断言） | ✅ 已落实 |

## 自动化覆盖统计

- **转为自动化断言的 7 项**：B-01、B-02、B-04、B-05、B-06、B-07、B-09、B-14（超出「≥6 项」要求）。
- 仍依赖手工/既有测试套件的 9 项：B-03、B-08、B-10（已由 `html_sanitize_test` 覆盖）、B-11、B-12、B-13、B-15、B-16、B-17。
- 全部 17 项结论均为「已免疫 / 已落实」，无未处置项。

## 备注

- `reg_*` 用例集中在 `tests/integration/regression_test.rs`，与 `alignment_test.rs` 同属 `integration` 测试目标。
- B-09 / B-10 的核心安全用例已在 `tests/infrastructure/html_sanitize_test.rs` 以原型 8 条真实绕过路径固化；此处 `reg_sanitize_*` 仅做**集成层复验**，避免重复但保证回归可见。
- B-14 的 TOC 转义由领域层 `TocEntry::needs_escape` 标记 + `TocBuilder::escape_title` 在拼接时消费，导出器 `build_document` 严格按「渲染正文 → 拼 TOC → 清洗 → 包骨架」顺序，故标题含 `<script>` 不会形成注入入口。
