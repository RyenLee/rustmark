# ADR-005 · 与 Web 原型 `markdown-editor-prototype` 的关系

- **状态**：已接受（2026-10-06）
- **关联需求**：§3（功能范围）、§4.4（质量）
- **关联任务**：S1-06、S7-02、S7-03

## 背景

工作区已存在 `markdown-editor-prototype/`：Vue 3 + Vite + TS + Pinia 的 Web 高保真原型，25 个组件、`marked` + `highlight.js`、152 项 smoke 自检，并产出 4 篇设计文档与一份 `REMAINING-WORK.md`（含 8 项功能失效、1 项安全绕过、8 项逻辑缺陷、6 项工程化缺口、7 处 README 错误）。

需要明确：原型对 Rust 工程是什么，不是什么。

## 决策

**原型是「行为契约来源」与「缺陷疫苗库」，不是代码来源，也不是目录归属。**

### 是

1. **交互与视觉契约**：布局、配色 token、组件职责、交互流程以原型为准；
2. **数值口径契约**：字数/字符/行数/阅读时长/任务完成度、防抖 150ms、搜索命中上限 50/300、最近文件上限 20；
3. **测试用例来源**：原型 `smoke/ssr-entry.ts` 的断言（格式化、查找、搜索、统计、diff、脚注、分页、命令注册表、焦点环绕、网格导航、sanitize、撤销隔离、快捷键匹配与冲突）转为 Rust 侧 golden 用例；
4. **缺陷疫苗库**：原型已修的缺陷逐条转为 Rust 侧的**设计层约束**与验收标准（见 `docs/archive/stage-reports/prototype-contract.md`）。

### 不是

1. **不是代码来源**：Vue/TS 实现不可移植；`lib/` 下的纯函数仅提取行为断言，不照搬实现；
2. **不是架构来源**：原型的「store 内部替换 localStorage 为 IPC 调用」对接设计基于 Tauri 假设，ADR-001 已否决；
3. **不是目录归属**：原型目录保持只读，Rust 工程与计划文档位于独立目录 `rustmark/`；
4. **不是绝对标准**：`marked` 与 pulldown-cmark 的渲染差异以「符合 CommonMark/GFM 规范」为最终判据，差异需记录而非强制对齐。

## 影响范围

- S1-06：产出 `docs/archive/stage-reports/prototype-contract.md`（A 行为契约 + B 缺陷免疫清单）；
- S7-02：行为对齐矩阵（≥40 行，含差异处置）；
- S7-03：17 项缺陷回归验证，≥6 项转为自动化断言。
