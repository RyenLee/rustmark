//! 纯几何运算（egui-free）：`Pt` 的向量操作与 `Rect` 的包围盒运算。
//!
//! 取代 `egui` 的 `Pos2` / `Vec2` / `Rect` 几何语义，使排版引擎不再依赖 egui。

use super::Pt;

impl Pt {
    /// 按标量缩放（点坐标或自由向量通用）。
    pub fn scale(self, s: f32) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
    /// 向量长度。
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

/// 轴对齐矩形（取代 egui `Rect`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// 最小角（左上）。
    pub min: Pt,
    /// 最大角（右下）。
    pub max: Pt,
}

impl Rect {
    /// 由最小 / 最大角构造。
    pub fn from_min_max(min: Pt, max: Pt) -> Self {
        Rect { min, max }
    }
    /// 由最小角 + 尺寸构造。
    pub fn from_min_size(min: Pt, size: Pt) -> Self {
        Rect::from_min_max(min, min + size)
    }
    /// 由中心 + 尺寸构造。
    pub fn from_center_size(center: Pt, size: Pt) -> Self {
        let h = size.scale(0.5);
        Rect::from_min_max(center - h, center + h)
    }
    /// 由两点（任意对角）构造。
    pub fn from_two_pos(a: Pt, b: Pt) -> Self {
        Rect::from_min_max(
            Pt::new(a.x.min(b.x), a.y.min(b.y)),
            Pt::new(a.x.max(b.x), a.y.max(b.y)),
        )
    }
    /// 由一组点构造（取外接框；空集退化为原点）。
    pub fn from_points(pts: &[Pt]) -> Self {
        let mut it = pts.iter();
        let first = it.next().copied().unwrap_or_else(Pt::zero);
        let mut min = first;
        let mut max = first;
        for p in it {
            min = Pt::new(min.x.min(p.x), min.y.min(p.y));
            max = Pt::new(max.x.max(p.x), max.y.max(p.y));
        }
        Rect::from_min_max(min, max)
    }
    /// 左边界 x。
    pub fn left(&self) -> f32 {
        self.min.x
    }
    /// 右边界 x。
    pub fn right(&self) -> f32 {
        self.max.x
    }
    /// 上边界 y。
    pub fn top(&self) -> f32 {
        self.min.y
    }
    /// 下边界 y。
    pub fn bottom(&self) -> f32 {
        self.max.y
    }
    /// 宽度。
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }
    /// 高度。
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }
    /// 中心点。
    pub fn center(&self) -> Pt {
        Pt::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }
    /// 与另一矩形的并集（外包）。
    pub fn union(&self, o: Rect) -> Rect {
        Rect::from_min_max(
            Pt::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            Pt::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        )
    }
    /// 整体平移 `d`。
    pub fn translate(&self, d: Pt) -> Rect {
        Rect::from_min_max(self.min + d, self.max + d)
    }
    /// 四边各外扩 `v`（负值为内缩）。
    pub fn expand(&self, v: f32) -> Rect {
        Rect::from_min_max(self.min - Pt::splat(v), self.max + Pt::splat(v))
    }
    /// 点是否落在矩形内（含边界）。
    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}
