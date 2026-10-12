//! 图元生成、坐标归一化收尾与 egui 绘制；各图种共用。
//!
//! 已拆分为两个私有子模块：
//! - `emit`：节点外形、边线、箭头、标签的图元生成
//! - `paint`：egui 绘制、finalize 收尾、热点解析与几何工具
//!
//! facade 负责子模块装配与全量 re-export，外部路径零修改。

mod emit;
mod paint;

pub use emit::emit;
pub use paint::{
    MermaidStyle, collect_clicks, finalize, first_md_link, fitted_height, paint, prim_count,
    push_check, push_rect_outline, rect_boundary, rect_corners,
};
