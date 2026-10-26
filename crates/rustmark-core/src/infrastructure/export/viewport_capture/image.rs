//! 纯像素操作：整帧裁剪、PNG 写盘、长图分段拼接。

use std::path::Path;

use image::RgbaImage;

use super::{CaptureError, MAX_DIMENSION, MAX_LONG_IMAGE_HEIGHT, PortFrame};

/// 从整帧截图里裁出指定区域。
///
/// `clip` 是**物理像素**坐标 `[left, top, right, bottom]`——
/// 调用方需自己乘 `pixels_per_point`，本函数不做单位换算：
/// 隐式换算会让「传进来的到底是逻辑点还是像素」变得不可察，
/// 而错了单位的表现是「裁出一块偏移的图」，不会报错。
///
/// # 为什么必须夹紧而不是 clamp 到边界
///
/// 窗口在发命令与回传之间可能被缩小，此时矩形右边界会超出画面。
/// 夹紧会**静默产出一张裁掉一半的图**，用户以为导出成功；
/// 这里选择明确报错——宁可让用户重试一次，也不要给一张残缺的成品。
pub fn crop(frame: &PortFrame, clip: [i64; 4]) -> Result<RgbaImage, CaptureError> {
    let (w, h) = (frame.size[0] as i64, frame.size[1] as i64);
    let [left, top, right, bottom] = clip;
    if right > w || bottom > h || left < 0 || top < 0 || right <= left || bottom <= top {
        return Err(if right <= left || bottom <= top {
            CaptureError::EmptyClip { requested: clip }
        } else {
            CaptureError::ClipOutOfBounds {
                requested: clip,
                actual: [frame.size[0], frame.size[1]],
            }
        });
    }
    let cw = (right - left) as u32;
    let ch = (bottom - top) as u32;
    if cw > MAX_DIMENSION || ch > MAX_DIMENSION {
        return Err(CaptureError::TooLarge {
            width: cw,
            height: ch,
            limit: MAX_DIMENSION,
        });
    }

    // 直接铺平为 RGBA8 字节缓冲再 `from_raw`。
    // 逐像素 `put_pixel` 会做四次边界检查，1920×1080 的裁剪要多花几百毫秒；
    // `PortFrame::pixels` 是行优先的 RGBA8（`[u8; 4]`），逐像素 `extend_from_slice`
    // 即铺平，无需逐分量读取。
    let mut buf = Vec::with_capacity(cw as usize * ch as usize * 4);
    for y in 0..ch as i64 {
        let src_start = ((top + y) * w + left) as usize;
        for px in &frame.pixels[src_start..src_start + cw as usize] {
            buf.extend_from_slice(px);
        }
    }
    RgbaImage::from_raw(cw, ch, buf).ok_or(CaptureError::EmptyClip { requested: clip })
}

/// 把裁好的图像编码成 PNG 并落盘，返回写入字节数。
///
/// 编码与写盘合一：调用方要的是「拿到一个可用的文件」，
/// 分成两步只会让上层的成功/失败分支多一个中间态。
pub fn write_png(img: &RgbaImage, path: &Path) -> Result<usize, CaptureError> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| CaptureError::Io {
            message: format!("PNG 编码失败：{e}"),
        })?;
    let bytes = buf.into_inner();
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| CaptureError::Io {
            message: format!("创建目录 {} 失败：{e}", parent.display()),
        })?;
    }
    std::fs::write(path, &bytes).map_err(|e| CaptureError::Io {
        message: format!("写入 {} 失败：{e}", path.display()),
    })?;
    Ok(bytes.len())
}

/// 把长图的各段纵向拼成一张完整图像。
///
/// `parts[i]` 是第 i 段的裁剪结果（内容区间 `[offset_i, offset_i + 视口高]`）；
/// `skip_top[i]` 是该段**顶部要丢弃的行数**（与上一段重叠的部分——
/// 首段为 0，末段因「滚动到底」与上一段重叠若干行）。
///
/// # 为什么拼接放在基础设施层
///
/// 纵向堆叠是纯像素操作，与 egui / GPU 无关；放这里才能用无头单测
/// 锁住「重叠行恰好被丢弃一次」这类边界，而不必驱动真实帧循环。
pub fn stitch(parts: &[RgbaImage], skip_top: &[u32]) -> Result<RgbaImage, CaptureError> {
    if parts.is_empty() {
        return Err(CaptureError::Io {
            message: "没有任何分段可拼接".to_string(),
        });
    }
    if parts.len() != skip_top.len() {
        return Err(CaptureError::Io {
            message: format!(
                "分段数（{}）与丢弃行数（{}）不一致",
                parts.len(),
                skip_top.len()
            ),
        });
    }
    let width = parts[0].width();
    let mut kept_rows = 0u64;
    for (i, part) in parts.iter().enumerate() {
        if part.width() != width {
            return Err(CaptureError::Io {
                message: format!(
                    "第 {i} 段宽度（{}）与首段（{width}）不一致，无法拼接",
                    part.width()
                ),
            });
        }
        // skip 必须给每段至少留 1 行，否则该段毫无贡献（调用方的偏移算错了）。
        if u64::from(skip_top[i]) >= u64::from(part.height()) {
            return Err(CaptureError::Io {
                message: format!(
                    "第 {i} 段丢弃行数（{}）不小于段高（{}）",
                    skip_top[i],
                    part.height()
                ),
            });
        }
        kept_rows += u64::from(part.height()) - u64::from(skip_top[i]);
    }
    if kept_rows > u64::from(MAX_LONG_IMAGE_HEIGHT) {
        return Err(CaptureError::TooLarge {
            width,
            height: u32::try_from(kept_rows).unwrap_or(u32::MAX),
            limit: MAX_LONG_IMAGE_HEIGHT,
        });
    }

    let total_height = kept_rows as u32;
    // `as_raw()` 返回的是**字节**缓冲，RGBA8 每像素 4 字节——
    // 行数与偏移都要按字节换算，否则切片长度对不上、`from_raw` 必然拒绝。
    let bytes_per_px = std::mem::size_of::<image::Rgba<u8>>();
    let mut buf = Vec::with_capacity(width as usize * total_height as usize * bytes_per_px);
    for (part, &skip) in parts.iter().zip(skip_top) {
        let start = skip as usize * width as usize * bytes_per_px;
        let rows = (part.height() - skip) as usize * width as usize * bytes_per_px;
        // 与 `crop` 同理：`RgbaImage` 底层缓冲连续，整段复制即可。
        buf.extend_from_slice(&part.as_raw()[start..start + rows]);
    }
    RgbaImage::from_raw(width, total_height, buf).ok_or(CaptureError::Io {
        message: "拼接缓冲尺寸不符".to_string(),
    })
}
