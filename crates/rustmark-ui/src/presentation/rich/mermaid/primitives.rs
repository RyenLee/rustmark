//! 图元生成、坐标归一化收尾与命令翻译；各图种共用。
//!
//! 已拆分为两个私有子模块：
//! - `emit`：节点外形、边线、箭头、标签的图元生成（`Graph` → `Vec<Prim>`）
//! - `paint`：`Prim` → `RenderCommand` 的翻译、finalize 收尾、热点解析与几何工具
//!
//! 两者都不 `use egui`：绘制由唯一 egui 后端 `render::apply_egui` 完成
//! （问题 F / O4）。facade 负责子模块装配与全量 re-export，外部路径零修改。

mod emit;
mod paint;

pub(crate) use emit::emit;
pub use paint::{
    MermaidStyle, collect_clicks, commands, finalize, first_md_link, fitted_height, prim_count,
    push_check, rect_boundary, rect_corners,
};
