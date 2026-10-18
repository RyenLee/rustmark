#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""rustmark Logo 投产构建器——几何唯一真源，同时驱动 SVG 与 PNG 产出。

设计来源：定稿概念「收敛A」（问题记录 056 之前的设计轮次存档于
`rustmark-logo.miora` / `rustmark-logo_assets/`）。本脚本把 AI 概念稿的几何
实测提炼为可投产的矢量规格：

  容器    2:1 横圆角矩形（概念稿实测 350×175 @1536px），墨黑 #1A1A1A
  圆角    r ≈ 9.14% 容器高（实测 16/175）→ 投产取 10%（小尺寸可辨性）
  分割线  Rust 橙 #CE422B，通高，宽 ≈ 3% 容器宽
  hash    纸白 #F7F5F2：两条通高斜杠 + 两条 1/3、2/3 位斜横杠，整体右倾 11.3°
  三角    纸白，右向圆角三角（上/下顶点 r≈7、尖点 r≈5，按 48 单位宽归一）
  标准字  "rustmark"，几何无衬线 Bahnschrift SemiBold/Condensed（wght 600 /
          wdth 87.5：概念稿词标宽高比 5.01 命中 87.5 档），升序高 = 58.3% 容器高

归一化决策（AI 概念稿几何自相矛盾处的收口，均向原始概念与暗底定稿对齐）：
  1. 分割线位置：原始概念与暗底定稿实测均在 50%，亮底定稿的 38.6% 是 AI
     编辑漂移（同稿右缘还有渲染伪影）→ 取 50%。
  2. hash / 三角各自居中于所属半区（原始概念即此布局）。
  3. 词标与容器间距：AI 稿 2% 过挤 → 投产取 5%（10 单位）。
  4. 词标字身：概念稿的几何无衬线无系统对应字体，取可得的最近似
     Bahnschrift SemiBold / Condensed（wght 600 / wdth 87.5：单层 a、
     直字身、几何骨架，宽高比 4.96 命中概念稿实测 5.01）。

另产出一套**紧凑模式**（小尺寸变体，见文件中「几何：紧凑模式」常量块）：
2:1 容器改为 1:1 方形、hash / 三角按 0.66 重排、分割线加粗到 5% 宽——
16px 下 2:1 标态的 hash 墨迹仅 ~4×4px 会糊成团块，方形容器把元素放到
~6×6px 且分割线通高，可辨度显著提升（对比记录 reference/square-vs-wide.png）。
用途分工：窗口 / 任务栏图标（16px 为主场景）用紧凑模式；关于框、启动页、
商店配图等大尺寸场景用 2:1 标态。两套共用同一 hash / 三角几何，只是容器
比例与排布不同，不引入新造型。

用法：
    python build_logo.py            # 全量构建（SVG + PNG + ICO + ICNS）
"""

from __future__ import annotations

import math
import struct
from pathlib import Path

from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from PIL import Image, ImageDraw, ImageFont

# ---------------------------------------------------------------- 设计常量

INK = "#1A1A1A"     # 墨黑（容器 / 浅底词标）
PAPER = "#F7F5F2"   # 纸白（hash / 三角 / 深底词标）
RUST = "#CE422B"    # Rust 橙（分割线）

# 容器栅格：200×100（概念稿容器 350×175 的归一化）
CONT_W, CONT_H = 200.0, 100.0
CONT_RADIUS = 10.0                       # 圆角（% 容器高）
DIVIDER_X0, DIVIDER_X1 = 97.0, 103.0     # 分割线（50% 居中，宽 6）
DIVIDER_W = DIVIDER_X1 - DIVIDER_X0

# hash：56×61，居中于左半区（0..97）→ 中心 x=48.5
HASH_W, HASH_H = 56.0, 61.0
HASH_CX, HASH_CY = 48.5, 50.0
HASH_SHEAR = 0.2                         # 右倾：顶部位移 = 0.2 × 高 ≈ 11.3°
# hash 局部构件（未剪切的 56×61 局部坐标，y 向下；比例取自概念稿实测）
_HASH_STROKE_W = 11.65                   # 斜杠宽（实测 20.4% × 56）
_HASH_STROKE_CENTERS = (18.3, 38.0)      # 两斜杠中心（实测 33% / 68%）
_HASH_BAR_H = 8.7                        # 横杠高（实测 14.3% × 61）
_HASH_BAR_Y = ((14.5, 23.2), (37.8, 46.5))  # 两横杠的 y 范围（实测 1/3、2/3 位）
_HASH_BAR_X = (5.2, 55.1)                # 横杠横向范围（实测 89% 宽）

# 三角：圆角后墨迹盒 48×55，居中于右半区（103..200）→ 中心 x=151.5
TRI_INK_W, TRI_INK_H = 48.0, 55.0        # 圆角后的墨迹盒（对齐概念稿实测）
TRI_CX, TRI_CY = 151.5, 50.0
TRI_R_CORNER = 7.0                       # 上/下顶点圆角
TRI_R_APEX = 5.0                         # 右尖点圆角

# 词标
WORDMARK_TEXT = "rustmark"
WORDMARK_FONT = "C:/Windows/Fonts/bahnschrift.ttf"
WORDMARK_WGHT = 600.0                    # SemiBold（500 偏轻，与重标态失衡）
WORDMARK_WDTH = 87.5                     # Condensed：概念稿词标宽高比 5.01，
                                         # 常规宽（5.76）过宽，87.5 → 4.96 命中
WORDMARK_INK_H = 58.0                    # 升序墨高 = 58.3% 容器高（实测）
WORDMARK_BASELINE_Y = 79.0               # 墨区垂直居中（y 21..79）
WORDMARK_INK_X = 210.0                   # 与容器右缘间距 10（5%）

OUT_DIR = Path(__file__).resolve().parent
SS = 4                                    # 超采样倍率（PIL 无抗锯齿）

INK_RGBA = (26, 26, 26, 255)
PAPER_RGBA = (247, 245, 242, 255)
RUST_RGBA = (206, 66, 43, 255)

# 紧凑模式（小尺寸变体）：2:1 容器 → 1:1，hash / 三角按比例重排布。
# 动机：16px 下 2:1 标态的 hash 墨迹仅 ~4×4px，双斜杠与横杠糊成团块、
# 3% 宽分割线不足半像素；方形容器把元素放到 ~6×6px 并给分割线通高视线
# （同尺寸对比记录见 reference/square-vs-wide.png）。
# 派生规则（不引入新造型，只改容器比例与重排布，全部由上列常量解出）：
#   - 容器 100×100（边长 = 2:1 容器的高），圆角 r=12
#   - 分割线居中通高、宽 5（16px 下 ≥0.8px 可辨）
#   - 元素缩放 0.66 = 「hash 墨迹右缘距分割线左缘 ≥5」的解：
#       (HASH_CX + 半墨宽) → 47.5 需留 5，且两半区各自居中
SQ_CONT = 100.0
SQ_RADIUS = 12.0
SQ_DIVIDER_X0, SQ_DIVIDER_X1 = 47.5, 52.5
SQ_SCALE = 0.66
SQ_HASH_CX = SQ_DIVIDER_X0 / 2                    # 23.75（左半区中心）
SQ_TRI_CX = (SQ_DIVIDER_X1 + SQ_CONT) / 2         # 76.25（右半区中心）
SQ_CY = CONT_H / 2                                 # 50（垂直中心不变）


# ---------------------------------------------------------------- 几何：hash

def _shear(pt: tuple[float, float]) -> tuple[float, float]:
    """hash 局部坐标 → 剪切后坐标（y 向下；顶部右移）。

    局部 y=0 在顶、y=HASH_H 在底；顶部位移 = HASH_SHEAR × HASH_H。
    """
    x, y = pt
    return (x + HASH_SHEAR * (HASH_H - y), y)


def _hash_local_polys() -> list[list[tuple[float, float]]]:
    """hash 的四个平行四边形构件（未剪切的局部坐标，y 向下）。"""
    polys: list[list[tuple[float, float]]] = []
    for cx in _HASH_STROKE_CENTERS:
        x0, x1 = cx - _HASH_STROKE_W / 2, cx + _HASH_STROKE_W / 2
        polys.append([
            (x0, 0.0), (x1, 0.0), (x1, HASH_H), (x0, HASH_H),
        ])
    for y0, y1 in _HASH_BAR_Y:
        polys.append([
            (_HASH_BAR_X[0], y0), (_HASH_BAR_X[1], y0),
            (_HASH_BAR_X[1], y1), (_HASH_BAR_X[0], y1),
        ])
    return polys


def _hash_offset() -> tuple[float, float]:
    """按**剪切后**的墨迹包围盒把 hash 居中到 (HASH_CX, HASH_CY)。

    剪切会把横杠的顶端推向右侧，墨迹盒比局部盒更宽且不贴局部原点——
    必须拿剪切后的真实墨迹盒归位，否则 hash 会整体右偏约 12 单位。
    """
    pts = [p for poly in _hash_local_polys() for p in poly]
    sp = [_shear(p) for p in pts]
    x0, x1 = min(p[0] for p in sp), max(p[0] for p in sp)
    y0, y1 = min(p[1] for p in sp), max(p[1] for p in sp)
    return (HASH_CX - (x0 + x1) / 2, HASH_CY - (y0 + y1) / 2)


_HASH_OFFSET = _hash_offset()


def _local_to_container(pt: tuple[float, float]) -> tuple[float, float]:
    """hash 局部坐标 → 容器坐标（先剪切，再按墨迹盒居中归位）。"""
    sx, sy = _shear(pt)
    return (sx + _HASH_OFFSET[0], sy + _HASH_OFFSET[1])


def hash_polys() -> list[list[tuple[float, float]]]:
    """hash 的四个平行四边形构件（容器坐标）。"""
    return [
        [_local_to_container(p) for p in poly] for poly in _hash_local_polys()
    ]


# ---------------------------------------------------------------- 几何：圆角三角

def _unit(a: tuple[float, float], b: tuple[float, float]) -> tuple[float, float]:
    dx, dy = b[0] - a[0], b[1] - a[1]
    ln = math.hypot(dx, dy)
    return (dx / ln, dy / ln)


def _add(a, b, k=1.0):
    return (a[0] + k * b[0], a[1] + k * b[1])


def rounded_triangle_raw(w: float, h: float) -> dict:
    """右向圆角三角（尖角尺寸 w×h；左竖边 x=0，上顶点 (0,0)，下顶点 (0,h)，尖点 (w,h/2)）。

    返回 SVG path 的 ``d`` 与 PIL 多边形采样点（同一组切点/圆心）。
    """
    top, bot, apex = (0.0, 0.0), (0.0, h), (w, h / 2)

    def corner(vertex, r, nb_a, nb_b):
        """顶点处圆角：返回 (切点a, 切点b, 圆心, 弧上中点=最深切点)。"""
        va, vb = _unit(vertex, nb_a), _unit(vertex, nb_b)
        cos_t = max(-1.0, min(1.0, va[0] * vb[0] + va[1] * vb[1]))
        theta = math.acos(cos_t)
        d = r / math.tan(theta / 2)           # 顶点到切点的距离
        ta, tb = _add(vertex, va, d), _add(vertex, vb, d)
        bis = _unit((0, 0), (va[0] + vb[0], va[1] + vb[1]))
        # 圆心在角平分线上、距顶点 r/sin(θ/2)（不是 r）
        center = _add(vertex, bis, r / math.sin(theta / 2))
        # 弧的中点（最深切点）在圆心指向顶点的一侧
        mid = _add(center, bis, -r)
        return ta, tb, center, mid

    t_top = corner(top, TRI_R_CORNER, bot, apex)      # 沿 top→bot / top→apex
    t_bot = corner(bot, TRI_R_CORNER, top, apex)      # 沿 bot→top / bot→apex
    t_apex = corner(apex, TRI_R_APEX, top, bot)       # 沿 apex→top / apex→bot

    # 遍历方向（屏幕逆时针）：左竖边下行 → 绕下顶点 → 上行至尖点 →
    # 绕尖点 → 下行至上顶点 → 绕上顶点 → 回左竖边。
    p_tb = t_top[0]    # 左竖边上、靠近上顶点
    p_tp = t_top[1]    # 上边上、靠近上顶点
    p_bt = t_bot[0]    # 左竖边上、靠近下顶点
    p_bp = t_bot[1]    # 下边上、靠近下顶点
    p_at = t_apex[0]   # 上边上、靠近尖点
    p_ab = t_apex[1]   # 下边上、靠近尖点

    def fmt(p):
        return f"{p[0]:.3f} {p[1]:.3f}"

    # 逆时针（y 向下坐标系中角度递减）→ SVG sweep-flag = 0
    d = (
        f"M {fmt(p_tb)} "
        f"L {fmt(p_bt)} "
        f"A {TRI_R_CORNER:.3f} {TRI_R_CORNER:.3f} 0 0 0 {fmt(p_bp)} "
        f"L {fmt(p_ab)} "
        f"A {TRI_R_APEX:.3f} {TRI_R_APEX:.3f} 0 0 0 {fmt(p_at)} "
        f"L {fmt(p_tp)} "
        f"A {TRI_R_CORNER:.3f} {TRI_R_CORNER:.3f} 0 0 0 {fmt(p_tb)} Z"
    )

    def arc_pts(center, r, p_start, p_end, p_mid, n=16):
        a1 = math.atan2(p_start[1] - center[1], p_start[0] - center[0])
        a2 = math.atan2(p_end[1] - center[1], p_end[0] - center[0])
        am = math.atan2(p_mid[1] - center[1], p_mid[0] - center[0])
        # 默认顺时针（角度递增）；弧中点不在该路径上则改逆时针
        delta = (a2 - a1) % (2 * math.pi)
        if not (0 < ((am - a1) % (2 * math.pi)) < delta):
            delta -= 2 * math.pi
        return [
            (center[0] + r * math.cos(a1 + delta * i / n),
             center[1] + r * math.sin(a1 + delta * i / n))
            for i in range(n + 1)
        ]

    poly = [p_tb, p_bt]
    poly += arc_pts(t_bot[2], TRI_R_CORNER, p_bt, p_bp, t_bot[3])
    poly += [p_ab]
    poly += arc_pts(t_apex[2], TRI_R_APEX, p_ab, p_at, t_apex[3])
    poly += [p_tp]
    poly += arc_pts(t_top[2], TRI_R_CORNER, p_tp, p_tb, t_top[3])
    return {"d": d, "poly": poly}


def _ink_bbox(poly: list[tuple[float, float]]) -> tuple[float, float, float, float]:
    xs = [p[0] for p in poly]
    ys = [p[1] for p in poly]
    return min(xs), min(ys), max(xs), max(ys)


def _fit_sharp_size(iters: int = 10) -> tuple[float, float]:
    """固定点迭代反解尖角尺寸，使**圆角后墨迹盒** = TRI_INK_W×TRI_INK_H。

    圆角切去量取决于内角与弧的朝向（并非恰好等于 r），手算不可靠；
    而切去量对尺寸的依赖很弱，固定点迭代数次即收敛。
    """
    w, h = TRI_INK_W + TRI_R_APEX, TRI_INK_H + 2 * TRI_R_CORNER
    for _ in range(iters):
        x0, y0, x1, y1 = _ink_bbox(rounded_triangle_raw(w, h)["poly"])
        w += TRI_INK_W - (x1 - x0)
        h += TRI_INK_H - (y1 - y0)
    return w, h


_TRI_SHARP = _fit_sharp_size()


def rounded_triangle() -> dict:
    """投产用圆角三角（尖角尺寸已拟合，墨迹盒 = TRI_INK_W×TRI_INK_H）。"""
    return rounded_triangle_raw(*_TRI_SHARP)


def tri_to_container(geo: dict) -> dict:
    """把局部三角几何按**圆角墨迹盒**平移到容器坐标。"""
    x0, y0, x1, y1 = _ink_bbox(geo["poly"])
    ox = TRI_CX - (x0 + x1) / 2
    oy = TRI_CY - (y0 + y1) / 2
    return {
        "d": geo["d"],
        "transform": f"translate({ox:.3f} {oy:.3f})",
        "poly": [(x + ox, y + oy) for x, y in geo["poly"]],
    }


# ---------------------------------------------------------------- 几何：紧凑模式

def _recenter(
    poly: list[tuple[float, float]],
    wide_c: tuple[float, float],
    sq_c: tuple[float, float],
) -> list[tuple[float, float]]:
    """容器坐标 → 紧凑坐标：以两模式各自中心为基准等比缩放重排。"""
    return [
        ((x - wide_c[0]) * SQ_SCALE + sq_c[0], (y - wide_c[1]) * SQ_SCALE + sq_c[1])
        for x, y in poly
    ]


def compact_hash_polys() -> list[list[tuple[float, float]]]:
    """hash 四构件（紧凑坐标）。"""
    return [
        _recenter(poly, (HASH_CX, HASH_CY), (SQ_HASH_CX, SQ_CY))
        for poly in hash_polys()
    ]


def compact_triangle() -> dict:
    """圆角三角（紧凑坐标；d / transform / poly 三者同源）。"""
    geo = rounded_triangle()
    x0, y0, x1, y1 = _ink_bbox(geo["poly"])
    lcx, lcy = (x0 + x1) / 2, (y0 + y1) / 2
    tx = SQ_TRI_CX - lcx * SQ_SCALE
    ty = SQ_CY - lcy * SQ_SCALE
    return {
        "d": geo["d"],
        "transform": f"translate({tx:.3f} {ty:.3f})",
        "poly": [(x * SQ_SCALE + tx, y * SQ_SCALE + ty) for x, y in geo["poly"]],
    }


# ---------------------------------------------------------------- 几何：词标

class Wordmark:
    """Bahnschrift 词标：fonttools 路径（SVG）+ PIL 尺寸（PNG）同源。"""

    def __init__(self) -> None:
        font = TTFont(WORDMARK_FONT)
        instantiateVariableFont(font, {"wght": WORDMARK_WGHT, "wdth": WORDMARK_WDTH}, inplace=True)
        self.upem = font["head"].unitsPerEm
        self.cmap = font.getBestCmap()
        self.glyph_set = font.getGlyphSet()
        self.hmtx = font["hmtx"]
        self._measure()

    def _measure(self) -> None:
        pen_x = 0.0
        x0 = y0 = None
        x1 = y1 = None
        self.glyphs: list[tuple[str, float]] = []
        for ch in WORDMARK_TEXT:
            gname = self.cmap[ord(ch)]
            bp = BoundsPen(self.glyph_set)
            self.glyph_set[gname].draw(bp)
            if bp.bounds:
                gx0, gy0, gx1, gy1 = bp.bounds
                x0 = gx0 + pen_x if x0 is None else min(x0, gx0 + pen_x)
                x1 = gx1 + pen_x if x1 is None else max(x1, gx1 + pen_x)
                y0 = gy0 if y0 is None else min(y0, gy0)
                y1 = gy1 if y1 is None else max(y1, gy1)
            self.glyphs.append((gname, pen_x))
            pen_x += self.hmtx[gname][0]
        self.ink_w_units = x1 - x0
        self.ink_h_units = y1 - y0
        self.scale = WORDMARK_INK_H / self.ink_h_units     # 容器单位 / 字体单位
        self.width = self.ink_w_units * self.scale          # 词标墨宽（容器单位）
        # SVG 组变换：X = scale*x + tx, Y = -scale*y + ty（墨左上角 → (INK_X, 21)）
        self.tx = WORDMARK_INK_X - self.scale * x0
        self.ty = 21.0 + self.scale * y1
        # PIL：em 尺寸（px，容器单位 1:1 时）
        self.pil_em = self.scale * self.upem

    def svg_paths(self) -> str:
        """词标 SVG（一组 <path>，外层 <g transform> 由调用方加）。"""
        out = []
        for gname, pen_x in self.glyphs:
            pen = SVGPathPen(self.glyph_set)
            self.glyph_set[gname].draw(pen)
            out.append(f'<path transform="translate({pen_x:.3f} 0)" d="{pen.getCommands()}"/>')
        return "\n    ".join(out)

    def svg_transform(self) -> str:
        return f"matrix({self.scale:.6f} 0 0 {-self.scale:.6f} {self.tx:.4f} {self.ty:.4f})"


# ---------------------------------------------------------------- SVG 产出

def _svg_doc(viewbox: str, body: str) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{viewbox}" '
        f'fill="none" role="img" aria-label="rustmark">\n  {body}\n</svg>\n'
    )


def _mark_body() -> str:
    parts = [
        f'<rect x="0" y="0" width="{CONT_W:g}" height="{CONT_H:g}" rx="{CONT_RADIUS:g}" fill="{INK}"/>',
        '<g fill="{}">'.format(PAPER),
    ]
    for poly in hash_polys():
        pts = " ".join(f"{x:.3f},{y:.3f}" for x, y in poly)
        parts.append(f'<polygon points="{pts}"/>')
    tri = tri_to_container(rounded_triangle())
    parts.append(f'<g transform="{tri["transform"]}"><path d="{tri["d"]}"/></g>')
    parts.append("</g>")
    parts.append(
        f'<rect x="{DIVIDER_X0:g}" y="0" width="{DIVIDER_W:g}" height="{CONT_H:g}" fill="{RUST}"/>'
    )
    return "\n  ".join(parts)


def _compact_mark_body() -> str:
    parts = [
        f'<rect x="0" y="0" width="{SQ_CONT:g}" height="{SQ_CONT:g}" rx="{SQ_RADIUS:g}" fill="{INK}"/>',
        '<g fill="{}">'.format(PAPER),
    ]
    for poly in compact_hash_polys():
        pts = " ".join(f"{x:.3f},{y:.3f}" for x, y in poly)
        parts.append(f'<polygon points="{pts}"/>')
    tri = compact_triangle()
    parts.append(f'<g transform="{tri["transform"]}"><path d="{tri["d"]}"/></g>')
    parts.append("</g>")
    parts.append(
        f'<rect x="{SQ_DIVIDER_X0:g}" y="0" width="{SQ_DIVIDER_X1 - SQ_DIVIDER_X0:g}" '
        f'height="{SQ_CONT:g}" fill="{RUST}"/>'
    )
    return "\n  ".join(parts)


def build_svgs(wm: Wordmark) -> dict[str, str]:
    mark = _svg_doc(f"0 0 {CONT_W:g} {CONT_H:g}", _mark_body())
    compact_mark = _svg_doc(f"0 0 {SQ_CONT:g} {SQ_CONT:g}", _compact_mark_body())
    lockup_w = WORDMARK_INK_X + wm.width
    lockup_vb = f"0 0 {lockup_w:.3f} {CONT_H:g}"

    def lockup(color: str) -> str:
        body = _mark_body() + "\n  " + (
            f'<g transform="{wm.svg_transform()}" fill="{color}">\n    '
            + wm.svg_paths()
            + "\n  </g>"
        )
        return _svg_doc(lockup_vb, body)

    return {
        "rustmark-mark.svg": mark,
        "rustmark-mark-compact.svg": compact_mark,
        "rustmark-logo.svg": lockup(INK),
        "rustmark-logo-on-dark.svg": lockup(PAPER),
    }


# ---------------------------------------------------------------- PNG 渲染

class Placement:
    """容器坐标 → 画布坐标的等比布置。"""

    def __init__(self, canvas_w: int, canvas_h: int, scale: float, ox: float, oy: float):
        self.canvas_w, self.canvas_h = canvas_w, canvas_h
        self.scale, self.ox, self.oy = scale, ox, oy

    def at(self, x: float, y: float) -> tuple[float, float]:
        return (self.ox + x * self.scale, self.oy + y * self.scale)


def render_png(pl: Placement, wordmark: Wordmark | None, wm_color: tuple[int, ...] | None) -> Image.Image:
    W, H = pl.canvas_w * SS, pl.canvas_h * SS
    im = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    s, ox, oy = pl.scale * SS, pl.ox * SS, pl.oy * SS

    def T(x, y):
        return (ox + x * s, oy + y * s)

    d.rounded_rectangle(
        [T(0, 0), T(CONT_W, CONT_H)], radius=CONT_RADIUS * s, fill=INK_RGBA
    )
    for poly in hash_polys():
        d.polygon([T(x, y) for x, y in poly], fill=PAPER_RGBA)
    tri = tri_to_container(rounded_triangle())
    d.polygon([T(x, y) for x, y in tri["poly"]], fill=PAPER_RGBA)
    d.rectangle(
        [T(DIVIDER_X0, 0), T(DIVIDER_X1, CONT_H)], fill=RUST_RGBA
    )
    if wordmark is not None and wm_color is not None:
        f = ImageFont.truetype(WORDMARK_FONT, max(8, round(wordmark.pil_em * s)))
        f.set_variation_by_axes([WORDMARK_WGHT, WORDMARK_WDTH])
        bb = d.textbbox((0, 0), WORDMARK_TEXT, font=f)
        d.text(
            (T(WORDMARK_INK_X, 21.0)[0] - bb[0], T(WORDMARK_INK_X, 21.0)[1] - bb[1]),
            WORDMARK_TEXT, font=f, fill=wm_color,
        )
    return im.resize((pl.canvas_w, pl.canvas_h), Image.LANCZOS)


def tight_mark(w: int) -> Placement:
    """紧贴包围盒的标态（宽 w，高 w/2）。"""
    return Placement(w, round(w / 2), w / CONT_W, 0.0, 0.0)


def square_icon(size: int, fill: float = 0.88) -> Placement:
    """方形画布图标：标态宽占画布 fill，垂直居中。"""
    scale = fill * size / CONT_W
    mark_h = CONT_H * scale
    return Placement(size, size, scale, (size - CONT_W * scale) / 2, (size - mark_h) / 2)


def lockup(w: int, wm: Wordmark) -> Placement:
    total = WORDMARK_INK_X + wm.width
    return Placement(w, round(w * CONT_H / total), w / total, 0.0, 0.0)


def render_compact(size: int) -> Image.Image:
    """紧凑模式方形图标（小尺寸优化变体；圆角由 rounded_rectangle 自带镂空）。"""
    W = size * SS
    im = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    s = size / SQ_CONT * SS
    box = [0, 0, SQ_CONT * s, SQ_CONT * s]
    d.rounded_rectangle(box, radius=SQ_RADIUS * s, fill=INK_RGBA)
    for poly in compact_hash_polys():
        d.polygon([(x * s, y * s) for x, y in poly], fill=PAPER_RGBA)
    d.polygon([(x * s, y * s) for x, y in compact_triangle()["poly"]], fill=PAPER_RGBA)
    d.rectangle(
        [SQ_DIVIDER_X0 * s, 0, SQ_DIVIDER_X1 * s, SQ_CONT * s], fill=RUST_RGBA
    )
    return im.resize((size, size), Image.LANCZOS)


# ---------------------------------------------------------------- ICO / ICNS

def build_ico(images: dict[int, Image.Image], path: Path) -> None:
    """PNG 数据条目的多尺寸 ICO（Vista+ 通用；image/egui 解码器均支持）。"""
    entries = sorted(images.items())
    header = struct.pack("<HHH", 0, 1, len(entries))
    offset = 6 + 16 * len(entries)
    dir_bytes, blobs = b"", []
    for size, im in entries:
        buf = _png_bytes(im)
        w = 0 if size >= 256 else size
        dir_bytes += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(buf), offset)
        blobs.append(buf)
        offset += len(buf)
    path.write_bytes(header + dir_bytes + b"".join(blobs))


_ICNS_TYPES = [
    ("icp4", 16), ("icp5", 32), ("icp6", 64), ("ic07", 128),
    ("ic08", 256), ("ic09", 512), ("ic10", 1024),
    ("ic11", 32), ("ic12", 64), ("ic13", 256), ("ic14", 512),
]


def build_icns(images: dict[int, Image.Image], path: Path) -> None:
    """标准 icns 条目集（PNG 数据；iconutil 同款编码）。"""
    body = b""
    for code, size in _ICNS_TYPES:
        im = images.get(size)
        if im is None:
            continue
        blob = _png_bytes(im)
        body += code.encode("ascii") + struct.pack(">I", len(blob) + 8) + blob
    path.write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)


def _png_bytes(im: Image.Image) -> bytes:
    import io

    buf = io.BytesIO()
    im.save(buf, format="PNG")
    return buf.getvalue()


# ---------------------------------------------------------------- main

def main() -> None:
    wm = Wordmark()
    print(f"词标：Bahnschrift {WORDMARK_WGHT:g}，墨高 {wm.ink_h_units:.1f} 字体单位 "
          f"→ {WORDMARK_INK_H:g} 容器单位，墨宽 {wm.width:.2f}，"
          f"横版总宽 {WORDMARK_INK_X + wm.width:.2f}")

    # 自检：几何墨迹包围盒
    hb = [p for poly in hash_polys() for p in poly]
    hx0, hy0 = min(p[0] for p in hb), min(p[1] for p in hb)
    hx1, hy1 = max(p[0] for p in hb), max(p[1] for p in hb)
    print(f"hash 墨迹盒：x {hx0:.2f}..{hx1:.2f}（中心 {(hx0+hx1)/2:.2f}），"
          f"y {hy0:.2f}..{hy1:.2f}（中心 {(hy0+hy1)/2:.2f}）")
    tri = tri_to_container(rounded_triangle())
    tx = [p[0] for p in tri["poly"]]
    ty = [p[1] for p in tri["poly"]]
    print(f"三角墨迹盒：x {min(tx):.2f}..{max(tx):.2f}（中心 {(min(tx)+max(tx))/2:.2f}），"
          f"y {min(ty):.2f}..{max(ty):.2f}（中心 {(min(ty)+max(ty))/2:.2f}）")
    _self_check_compact()

    # SVG
    png_dir = OUT_DIR / "png"
    png_dir.mkdir(parents=True, exist_ok=True)
    for name, doc in build_svgs(wm).items():
        (OUT_DIR / name).write_text(doc, encoding="utf-8")
        print(f"写出 {name}（{len(doc)} 字节）")

    # 方形图标 PNG（.ico/.icns 同源）
    icon_sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
    icon_imgs = {
        s: render_png(square_icon(s), None, None) for s in icon_sizes
    }
    for s, im in icon_imgs.items():
        im.save(png_dir / f"rustmark-icon-{s}.png")
    print(f"写出 png/rustmark-icon-*.png（{len(icon_imgs)} 个尺寸）")

    # 紧贴标态
    for w in (512, 1024):
        render_png(tight_mark(w), None, None).save(png_dir / f"rustmark-mark-{w}.png")
    print("写出 png/rustmark-mark-512/1024.png")

    # 横版锁定（两个变体）
    for w in (1024,):
        render_png(lockup(w, wm), wm, INK_RGBA).save(png_dir / f"rustmark-logo-{w}.png")
        render_png(lockup(w, wm), wm, PAPER_RGBA).save(
            png_dir / f"rustmark-logo-on-dark-{w}.png"
        )
    print("写出 png/rustmark-logo-1024.png、rustmark-logo-on-dark-1024.png")

    # ICO / ICNS（用 1024 母版各级重渲，保证每级独立超采样）
    ico_imgs = {s: icon_imgs[s] for s in (16, 24, 32, 48, 64, 128, 256)}
    build_ico(ico_imgs, OUT_DIR / "rustmark.ico")
    print(f"写出 rustmark.ico（{len(ico_imgs)} 级）")
    build_icns(icon_imgs, OUT_DIR / "rustmark.icns")
    print("写出 rustmark.icns")

    # 紧凑模式（小尺寸变体）：方形图标 PNG + ICO
    compact_imgs = {s: render_compact(s) for s in icon_sizes}
    for s, im in compact_imgs.items():
        im.save(png_dir / f"rustmark-icon-compact-{s}.png")
    print(f"写出 png/rustmark-icon-compact-*.png（{len(compact_imgs)} 个尺寸）")
    compact_ico = {s: compact_imgs[s] for s in (16, 24, 32, 48, 64, 128, 256)}
    build_ico(compact_ico, OUT_DIR / "rustmark-compact.ico")
    print(f"写出 rustmark-compact.ico（{len(compact_ico)} 级）")


def _self_check_compact() -> None:
    """紧凑模式几何自检：墨迹居中、分割线净空 ≥5。"""
    hb = [p for poly in compact_hash_polys() for p in poly]
    hx0, hx1 = min(p[0] for p in hb), max(p[0] for p in hb)
    hy0, hy1 = min(p[1] for p in hb), max(p[1] for p in hb)
    tri = compact_triangle()
    tx = [p[0] for p in tri["poly"]]
    ty = [p[1] for p in tri["poly"]]
    print(f"[紧凑] hash 墨迹盒：x {hx0:.2f}..{hx1:.2f}（中心 {(hx0+hx1)/2:.2f}），"
          f"y {hy0:.2f}..{hy1:.2f}（中心 {(hy0+hy1)/2:.2f}）")
    print(f"[紧凑] 三角墨迹盒：x {min(tx):.2f}..{max(tx):.2f}（中心 {(min(tx)+max(tx))/2:.2f}），"
          f"y {min(ty):.2f}..{max(ty):.2f}（中心 {(min(ty)+max(ty))/2:.2f}）")
    gap_l = SQ_DIVIDER_X0 - hx1
    gap_r = min(tx) - SQ_DIVIDER_X1
    print(f"[紧凑] 分割线净空：左 {gap_l:.2f}、右 {gap_r:.2f}（设计值 ≥5）")
    assert gap_l >= 5.0 - 1e-6 and gap_r >= 5.0 - 1e-6, "紧凑模式净空不足"


if __name__ == "__main__":
    main()
