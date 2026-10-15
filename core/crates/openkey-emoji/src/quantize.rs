//! 中位切分（median-cut）颜色量化器与 Floyd–Steinberg 抖动。
//!
//! 设计依据 docs/02 3.5.1（DR-003）：256 色量化 + Floyd–Steinberg 抖动
//! （默认开启，防止渐变出现 banding）。本模块为纯 Rust 自实现，
//! 不依赖 `color_quant` 等第三方量化库，以保证行为确定、可单元测试。
//!
//! 透明度约定：alpha < [`TRANSPARENT_ALPHA_THRESHOLD`] 的像素视为透明；
//! 当图像包含透明像素时，调色板索引 0 固定保留为透明槽位（`[0, 0, 0, 0]`），
//! 不透明颜色自索引 1 起排布（GIF 仅支持 1 位透明掩码）。

use std::collections::HashMap;

use crate::error::{Error, Result};

/// alpha 低于该值的像素视为透明。
pub const TRANSPARENT_ALPHA_THRESHOLD: u8 = 128;

/// GIF 调色板容量上限。
const MAX_PALETTE: usize = 256;

/// 量化结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizeOutput {
    /// 调色板（RGBA）。含透明像素时索引 0 为 `[0, 0, 0, 0]`。
    pub palette: Vec<[u8; 4]>,
    /// 每像素的调色板索引，长度 = `width * height`。
    pub indices: Vec<u8>,
}

/// 将 RGBA 像素量化为至多 `max_colors` 色的调色板。
///
/// - `pixels`：RGBA8888 紧密排布，长度须为 `width * height * 4`；
/// - `max_colors`：调色板容量，取值 1..=256；
/// - `dither`：开启 Floyd–Steinberg 抖动（serpentine 扫描）。
///
/// # 错误
/// 像素长度与尺寸不符、`max_colors` 越界，或含透明像素而 `max_colors < 2`
/// （透明槽位至少需要额外 1 色）时返回 [`Error::Quantize`]。
pub fn quantize(
    pixels: &[u8],
    width: usize,
    height: usize,
    max_colors: usize,
    dither: bool,
) -> Result<QuantizeOutput> {
    let expected = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| Error::Quantize(format!("尺寸 {width}x{height} 溢出")))?;
    if pixels.len() != expected {
        return Err(Error::Quantize(format!(
            "像素数据长度 {} 与尺寸 {}x{} 不符",
            pixels.len(),
            width,
            height
        )));
    }
    if max_colors == 0 || max_colors > MAX_PALETTE {
        return Err(Error::Quantize(format!(
            "调色板大小 {max_colors} 超出 1..={MAX_PALETTE}"
        )));
    }
    if width == 0 || height == 0 {
        return Ok(QuantizeOutput {
            palette: Vec::new(),
            indices: Vec::new(),
        });
    }

    // 1. 直方图：仅统计不透明像素；同时统计透明像素数。
    let mut histogram: HashMap<u32, u32> = HashMap::new();
    let mut transparent_count: u64 = 0;
    for px in pixels.chunks_exact(4) {
        if px[3] < TRANSPARENT_ALPHA_THRESHOLD {
            transparent_count += 1;
            continue;
        }
        let key = pack_rgb(px[0], px[1], px[2]);
        *histogram.entry(key).or_insert(0) += 1;
    }
    let has_transparent = transparent_count > 0;
    if has_transparent && max_colors < 2 {
        return Err(Error::Quantize(
            "含透明像素时调色板至少需要 2 色（透明槽位 + 至少 1 个不透明色）".to_string(),
        ));
    }

    // 2. 中位切分：把直方图颜色划分为至多 target 个箱。
    let mut colors: Vec<(u32, u32)> = histogram.into_iter().collect();
    let color_slots = if has_transparent {
        max_colors - 1
    } else {
        max_colors
    };
    let target = color_slots.min(colors.len());
    let boxes = median_cut(&mut colors, target);

    // 3. 汇总调色板：每个箱取计数加权平均色；透明槽位置于索引 0。
    let mut palette: Vec<[u8; 4]> = Vec::with_capacity(boxes.len() + usize::from(has_transparent));
    let transparent_index = 0u8;
    if has_transparent {
        palette.push([0, 0, 0, 0]);
    }
    let opaque_offset = usize::from(has_transparent);
    for b in &boxes {
        let (mut sr, mut sg, mut sb, mut count) = (0u64, 0u64, 0u64, 0u64);
        for &(c, n) in &colors[b.start..b.end] {
            let (r, g, bch) = unpack_rgb(c);
            sr += u64::from(r) * u64::from(n);
            sg += u64::from(g) * u64::from(n);
            sb += u64::from(bch) * u64::from(n);
            count += u64::from(n);
        }
        if count == 0 {
            continue;
        }
        palette.push([
            (sr / count) as u8,
            (sg / count) as u8,
            (sb / count) as u8,
            255,
        ]);
    }
    if palette.len() > MAX_PALETTE {
        return Err(Error::Quantize(format!(
            "调色板大小 {} 超出 GIF 上限 {MAX_PALETTE}",
            palette.len()
        )));
    }

    // 4. 像素 → 索引映射。
    let mut color_to_index: HashMap<u32, u8> = HashMap::with_capacity(colors.len());
    for (i, b) in boxes.iter().enumerate() {
        let palette_index = (i + opaque_offset) as u8;
        for &(c, _) in &colors[b.start..b.end] {
            color_to_index.insert(c, palette_index);
        }
    }

    let mut indices = vec![0u8; width * height];
    if !dither {
        // 无抖动：按箱归属直接映射（O(1)）。
        for (i, px) in pixels.chunks_exact(4).enumerate() {
            if px[3] < TRANSPARENT_ALPHA_THRESHOLD {
                indices[i] = transparent_index;
                continue;
            }
            let key = pack_rgb(px[0], px[1], px[2]);
            indices[i] = color_to_index
                .get(&key)
                .copied()
                .unwrap_or_else(|| nearest_opaque(&palette, [px[0], px[1], px[2]]));
        }
    } else {
        dither_serpentine(
            pixels,
            width,
            height,
            &palette,
            transparent_index,
            &mut indices,
        );
    }

    Ok(QuantizeOutput { palette, indices })
}

/// 颜色箱（直方图切片 `[start, end)`）。
struct ColorBox {
    start: usize,
    end: usize,
}

/// 中位切分：反复拆分"RGB 体积"最大的箱，直至达到 `target` 个箱或无法再分。
fn median_cut(colors: &mut [(u32, u32)], target: usize) -> Vec<ColorBox> {
    let mut boxes: Vec<ColorBox> = Vec::new();
    if colors.is_empty() || target == 0 {
        return boxes;
    }
    boxes.push(ColorBox {
        start: 0,
        end: colors.len(),
    });
    while boxes.len() < target {
        // 选取可拆分（≥2 色）的最优箱；体积为 0 说明箱内仅一种颜色。
        let best = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.end - b.start >= 2)
            .max_by_key(|(_, b)| box_score(colors, b.start, b.end));
        let Some((best_idx, best_box)) = best else {
            break; // 所有箱均为单色
        };
        if box_score(colors, best_box.start, best_box.end) == 0 {
            break;
        }
        let (start, end) = (best_box.start, best_box.end);
        match split_box(colors, start, end) {
            Some((left, right)) => {
                boxes[best_idx] = left;
                boxes.push(right);
            }
            None => break, // 防御：不可再分则停止（正常路径不可达）
        }
    }
    boxes
}

/// 箱的拆分优先级：RGB 体积（各通道极差乘积）。
fn box_score(colors: &[(u32, u32)], start: usize, end: usize) -> u64 {
    let (mut min, mut max) = ([u8::MAX; 3], [0u8; 3]);
    for &(c, _) in &colors[start..end] {
        for ch in 0..3 {
            let v = channel_value(c, ch);
            min[ch] = min[ch].min(v);
            max[ch] = max[ch].max(v);
        }
    }
    let volume = (1 + u32::from(max[0]) - u32::from(min[0]))
        * (1 + u32::from(max[1]) - u32::from(min[1]))
        * (1 + u32::from(max[2]) - u32::from(min[2]));
    u64::from(volume)
}

/// 沿最长轴按计数中位切分箱；两侧均非空。
fn split_box(colors: &mut [(u32, u32)], start: usize, end: usize) -> Option<(ColorBox, ColorBox)> {
    if end - start < 2 {
        return None;
    }
    let (mut min, mut max) = ([u8::MAX; 3], [0u8; 3]);
    for &(c, _) in &colors[start..end] {
        for ch in 0..3 {
            let v = channel_value(c, ch);
            min[ch] = min[ch].min(v);
            max[ch] = max[ch].max(v);
        }
    }
    let axis = (0..3).max_by_key(|&ch| u16::from(max[ch]) - u16::from(min[ch]))?;
    if max[axis] == min[axis] {
        return None; // 箱内仅一种颜色
    }
    colors[start..end].sort_by_key(|&(c, _)| channel_value(c, axis));

    let total: u64 = colors[start..end].iter().map(|&(_, n)| u64::from(n)).sum();
    let half = total / 2;
    let mut acc: u64 = 0;
    let mut split = start + 1;
    for (i, &(_, n)) in colors[start..end].iter().enumerate() {
        acc += u64::from(n);
        if acc >= half {
            split = start + i + 1;
            break;
        }
    }
    if split <= start || split >= end {
        split = start + (end - start) / 2;
        if split <= start || split >= end {
            return None;
        }
    }
    Some((
        ColorBox { start, end: split },
        ColorBox { start: split, end },
    ))
}

/// Floyd–Steinberg 抖动（serpentine 扫描），误差仅向下一行扩散。
fn dither_serpentine(
    pixels: &[u8],
    width: usize,
    height: usize,
    palette: &[[u8; 4]],
    transparent_index: u8,
    indices: &mut [u8],
) {
    let mut nearest_cache: HashMap<u32, u8> = HashMap::new();
    let mut err_cur = vec![[0i16; 3]; width];
    let mut err_next = vec![[0i16; 3]; width];
    for y in 0..height {
        let forward = y % 2 == 0;
        let dir: isize = if forward { 1 } else { -1 };
        for i in 0..width {
            let x = if forward { i } else { width - 1 - i };
            let pos = y * width + x;
            let px = &pixels[pos * 4..pos * 4 + 4];
            if px[3] < TRANSPARENT_ALPHA_THRESHOLD {
                indices[pos] = transparent_index;
                continue; // 透明像素不参与误差扩散
            }
            let mut adjusted = [0u8; 3];
            for ch in 0..3 {
                let v = i32::from(px[ch]) + i32::from(err_cur[x][ch]);
                adjusted[ch] = v.clamp(0, 255) as u8;
            }
            let idx = nearest_opaque_cached(palette, adjusted, &mut nearest_cache);
            indices[pos] = idx;
            let mut err = [0i16; 3];
            for ch in 0..3 {
                err[ch] = i16::from(adjusted[ch]) - i16::from(palette[usize::from(idx)][ch]);
            }
            // 误差扩散：扫描方向下一像素 7/16；下一行 3/16、5/16、1/16（方向镜像）。
            let next = x as isize + dir;
            if (0..width as isize).contains(&next) {
                add_error(&mut err_cur[next as usize], err, 7);
                add_error(&mut err_next[next as usize], err, 1);
            }
            let back = x as isize - dir;
            if (0..width as isize).contains(&back) {
                add_error(&mut err_next[back as usize], err, 3);
            }
            add_error(&mut err_next[x], err, 5);
        }
        std::mem::swap(&mut err_cur, &mut err_next);
        for e in err_next.iter_mut() {
            *e = [0, 0, 0];
        }
    }
}

/// 累加误差（权重为 1/16 的倍数）。
fn add_error(slot: &mut [i16; 3], err: [i16; 3], weight: i16) {
    for ch in 0..3 {
        slot[ch] += err[ch] * weight / 16;
    }
}

/// 在调色板中查找与 `color` 最近的**不透明**颜色（跳过透明槽位）。
fn nearest_opaque(palette: &[[u8; 4]], color: [u8; 3]) -> u8 {
    let mut best = 0u8;
    let mut best_d = u32::MAX;
    for (i, p) in palette.iter().enumerate() {
        if p[3] == 0 {
            continue;
        }
        let d = squared_distance(color, [p[0], p[1], p[2]]);
        if d < best_d {
            best_d = d;
            best = i as u8;
        }
    }
    best
}

/// [`nearest_opaque`] 的缓存版本（抖动路径同一修正色会重复出现）。
fn nearest_opaque_cached(palette: &[[u8; 4]], color: [u8; 3], cache: &mut HashMap<u32, u8>) -> u8 {
    let key = pack_rgb(color[0], color[1], color[2]);
    if let Some(&idx) = cache.get(&key) {
        return idx;
    }
    let idx = nearest_opaque(palette, color);
    cache.insert(key, idx);
    idx
}

/// RGB 平方欧氏距离。
fn squared_distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    let dr = i32::from(a[0]) - i32::from(b[0]);
    let dg = i32::from(a[1]) - i32::from(b[1]);
    let db = i32::from(a[2]) - i32::from(b[2]);
    (dr * dr + dg * dg + db * db) as u32
}

/// 打包 RGB 为 `0x00RRGGBB`。
fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// 解包 `0x00RRGGBB`。
fn unpack_rgb(c: u32) -> (u8, u8, u8) {
    (
        ((c >> 16) & 0xFF) as u8,
        ((c >> 8) & 0xFF) as u8,
        (c & 0xFF) as u8,
    )
}

/// 取打包色的指定通道（0=R，1=G，2=B）。
fn channel_value(c: u32, ch: usize) -> u8 {
    ((c >> (16 - 8 * ch)) & 0xFF) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造水平黑 → 白渐变色带图，用于检验抖动效果。
    fn gradient_band(width: usize, height: usize) -> Vec<u8> {
        let mut px = Vec::with_capacity(width * height * 4);
        for _y in 0..height {
            for x in 0..width {
                let v = (x * 255 / (width - 1)) as u8;
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        px
    }

    /// 把量化结果渲染为 RGB 缓冲（长度 = width * height）。
    fn render_rgb(q: &QuantizeOutput, width: usize, height: usize) -> Vec<[u8; 3]> {
        (0..width * height)
            .map(|i| {
                let c = q.palette[usize::from(q.indices[i])];
                [c[0], c[1], c[2]]
            })
            .collect()
    }

    /// 量化图 3x3 邻域平均色与原图的平均绝对误差（每像素三通道均值）。
    fn local_mean_error(q: &QuantizeOutput, original: &[u8], width: usize, height: usize) -> f64 {
        let img = render_rgb(q, width, height);
        let mut total = 0f64;
        for y in 0..height {
            for x in 0..width {
                let (mut sr, mut sg, mut sb, mut n) = (0u32, 0u32, 0u32, 0u32);
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let (yy, xx) = (y as i32 + dy, x as i32 + dx);
                        if yy < 0 || xx < 0 || yy >= height as i32 || xx >= width as i32 {
                            continue;
                        }
                        let p = img[yy as usize * width + xx as usize];
                        sr += u32::from(p[0]);
                        sg += u32::from(p[1]);
                        sb += u32::from(p[2]);
                        n += 1;
                    }
                }
                let o = &original[(y * width + x) * 4..][..3];
                total += f64::from((sr / n).abs_diff(u32::from(o[0])));
                total += f64::from((sg / n).abs_diff(u32::from(o[1])));
                total += f64::from((sb / n).abs_diff(u32::from(o[2])));
            }
        }
        total / (width * height) as f64
    }

    #[test]
    fn banding_gradient_dither_reduces_local_error() {
        let px = gradient_band(256, 16);
        let flat = quantize(&px, 256, 16, 16, false).expect("量化失败");
        let dithered = quantize(&px, 256, 16, 16, true).expect("量化失败");

        assert!(flat.palette.len() <= 16, "无抖动态调色板应 ≤16 色");
        assert!(dithered.palette.len() <= 16, "抖动态调色板应 ≤16 色");

        // 指标一：3x3 邻域平均色与原图的误差。抖动以空间混合换取局部保真，
        // 局部误差应显著小于无抖动态（无抖动态存在 banding，邻域均值无能为力）。
        let flat_err = local_mean_error(&flat, &px, 256, 16);
        let dither_err = local_mean_error(&dithered, &px, 256, 16);
        assert!(
            dither_err < flat_err * 0.7,
            "抖动局部误差 {dither_err} 应显著小于无抖动 {flat_err}"
        );

        // 指标二：水平相邻像素换色次数。抖动在色带边界产生棋盘格混合，
        // 换色次数应多于无抖动态（仅带宽边界跳变）。
        let row_transitions = |q: &QuantizeOutput| {
            q.indices
                .chunks_exact(256)
                .map(|row| row.windows(2).filter(|w| w[0] != w[1]).count())
                .sum::<usize>()
        };
        assert!(
            row_transitions(&dithered) > row_transitions(&flat),
            "抖动应产生更多相邻换色（无抖动 {}，抖动 {}）",
            row_transitions(&flat),
            row_transitions(&dithered)
        );
    }

    #[test]
    fn pure_color_palette_size_one() {
        let px: Vec<u8> = (0..16 * 16).flat_map(|_| [200, 30, 90, 255]).collect();
        let q = quantize(&px, 16, 16, 256, true).expect("量化失败");
        assert_eq!(q.palette.len(), 1, "纯色图调色板应为 1 色");
        assert_eq!(q.palette[0], [200, 30, 90, 255]);
        assert!(q.indices.iter().all(|&i| i == 0));
    }

    #[test]
    fn transparent_pixels_keep_alpha_zero() {
        let mut px = Vec::new();
        for i in 0..(16 * 16) {
            if i % 2 == 0 {
                px.extend_from_slice(&[0, 0, 0, 0]); // 透明
            } else {
                px.extend_from_slice(&[10, 20, 30, 255]);
            }
        }
        let q = quantize(&px, 16, 16, 256, false).expect("量化失败");
        assert_eq!(q.palette[0], [0, 0, 0, 0], "索引 0 应为透明槽位");
        assert_eq!(q.palette.len(), 2, "应含透明槽位 + 1 个不透明色");
        for (i, &idx) in q.indices.iter().enumerate() {
            if i % 2 == 0 {
                assert_eq!(idx, 0, "透明像素应映射到索引 0");
            } else {
                assert_ne!(idx, 0, "不透明像素不应映射到透明槽位");
                assert_eq!(q.palette[usize::from(idx)][3], 255);
            }
        }
    }

    #[test]
    fn palette_never_exceeds_256() {
        // 伪随机噪声图：唯一色数远超 256。
        let mut px = Vec::with_capacity(64 * 64 * 4);
        let mut seed = 0x1234_5678u32;
        for _ in 0..64 * 64 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            px.extend_from_slice(&[(seed >> 16) as u8, (seed >> 8) as u8, seed as u8, 255]);
        }
        let q = quantize(&px, 64, 64, 256, true).expect("量化失败");
        assert!(
            q.palette.len() <= 256,
            "调色板 {} 超出 256",
            q.palette.len()
        );
        assert_eq!(q.indices.len(), 64 * 64);
        assert!(q.indices.iter().all(|&i| usize::from(i) < q.palette.len()));
    }

    #[test]
    fn rejects_bad_input() {
        let px = [0u8; 7];
        assert!(quantize(&px, 2, 2, 256, false).is_err(), "长度不符应报错");
        let px = [0u8; 16];
        assert!(
            quantize(&px, 2, 2, 0, false).is_err(),
            "max_colors=0 应报错"
        );
        assert!(
            quantize(&px, 2, 2, 257, false).is_err(),
            "max_colors>256 应报错"
        );
        // 全透明 + max_colors=1：透明槽位无处安放。
        let px: Vec<u8> = (0..4).flat_map(|_| [1, 2, 3, 0u8]).collect();
        assert!(
            quantize(&px, 2, 2, 1, false).is_err(),
            "透明图 max_colors=1 应报错"
        );
    }

    #[test]
    fn empty_image_ok() {
        let q = quantize(&[], 0, 0, 256, true).expect("空图应合法");
        assert!(q.palette.is_empty());
        assert!(q.indices.is_empty());
    }
}
