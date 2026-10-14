# ADR-001 · UI 框架选型：egui + eframe（否决 Tauri）

- **状态**：已接受（2026-10-06）
- **决策者**：项目架构负责人
- **关联需求**：§5.1、§5.2、§1.2（`NFR-PERF-01/02/03`、`CONSTR-01`）
- **关联任务**：S1-02、S2-01、S6-01

## 背景

存在两处互相冲突的既有声明：

1. 《需求文档 v2.0 整合版》§5.1 选择 `egui + eframe 0.33.3`（wgpu 后端），§5.2 明确**拒绝 Tauri**（理由：依赖 WebView、体积 3–10 MB 不稳定、启动 317ms+）。
2. 既有 Web 原型 `../markdown-editor-prototype/docs/DESIGN-OVERVIEW.md` §1 声明「目标态为 Tauri v2 桌面端（Rust core + SQLite + WebView）」，并配套产出了 `DESIGN-BACKEND-CONTRACT.md`（数据库表设计、HTTP/IPC 接口契约、前后端映射表）。

若不显式裁决，阶段 05（持久化）与阶段 06（表现层）会按两套互斥假设实施。

## 决策

**采用 egui + eframe 0.33.3 + wgpu 后端，构建纯原生 GUI 应用。**

同时明确：

- 原型 `DESIGN-OVERVIEW.md` §1 的「Tauri v2 目标态」**在 v1.0 不生效**，降级为历史假设；
- `DESIGN-BACKEND-CONTRACT.md`（SQLite `files` / `drafts` / `settings` 表、`file_*` 命令、HTTP 接口）**v1.0 不实施**，降级为 v1.1+ 的可选参考；
- v1.0 持久化方案为**本地文件 + TOML 配置**（ADR-003），不存在数据库与后端进程。

## 备选方案与代价

| 方案 | 体积 | 冷启动 | 内存 | 代价 |
|---|---|---|---|---|
| **egui + eframe（选定）** | 3–4 MB | 最快 | 最低 | 需自绘文本编辑，IME 支持是最大技术风险（需求 §10 列为中风险） |
| Tauri v2 | 3–10 MB（依赖系统 WebView 与否不确定） | 317ms+ | 依赖 WebView 常驻 | 体积不可控、与「原生轻量」定位冲突；需重写为 Web 前端 |
| Electron | 80–135 MB | 2–5s | 200–500 MB | 全面击穿 §1.2 三项指标 |
| iced | 与 egui 接近 | 接近 | 接近 | 生态弱于 egui，无优势（§5.2） |

## 影响范围

- 阶段 03 领域层：路径用领域值对象 `FilePath`，**不引入 `std::path` 的 IO 语义**（领域层零 IO）。
- 阶段 05 持久化：文件仓储 + TOML 配置，**无 SQLite、无后端进程**（原型的后端契约不迁移）。
- 阶段 06 表现层：egui 立即模式 UI；`TextEdit` 的 IME 能力须在 S6-03 优先验证，不达标则启用 `cosmic-text` 自绘方案（预留 2 周打磨）。
- 阶段 07 对齐：原型的「store 内部替换 localStorage 为 IPC 调用」对接设计不再适用，改为「原型行为契约」对齐（见 `docs/archive/stage-reports/prototype-contract.md`）。

## 复盘触发条件

若出现下列任一情况，需重开本 ADR：

1. S6-03 的 IME 方案在预留 2 周内无法达到可用标准；
2. `cargo bloat` 实测 egui + eframe > 5 MB，导致总预算击穿 10 MB。
