//! 表情上传图像管线（docs/02 3.5.1，DR-003）。
//!
//! 管线：探测格式与动画 →（gif 动画逐帧保留帧延迟；webp/apng 动画取首帧）→
//! 长边 > [`PipelineParams::max_edge`] 时 Lanczos3 等比缩小（不放大）→
//! 256 色量化 + Floyd–Steinberg 抖动 → GIF89a 编码（循环标记/透明索引/帧延迟）→
//! 体积检查（超出 [`PipelineParams::max_bytes_warn`] 仅警告，由用户确认后继续）。

use std::io::Cursor;

use gif::{Encoder, Frame, Repeat};
use image::codecs::gif::GifDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, ImageFormat, RgbaImage};

use crate::error::{Error, Result};
use crate::quantize;

/// 管线参数（设置项，docs/02 3.5.1 参数表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineParams {
    /// 长边上限（px）。范围 200–800，默认 400（DR-003）。
    pub max_edge: u32,
    /// 是否启用 Floyd–Steinberg 抖动（默认开，防渐变 banding）。
    pub dither: bool,
    /// 体积警告阈值（字节）。默认 2 MiB；超出仅警告不阻断。
    pub max_bytes_warn: u64,
}

impl PipelineParams {
    /// 默认参数：长边 400px、开抖动、2 MiB 警告阈值。
    pub fn new() -> Self {
        Self {
            max_edge: 400,
            dither: true,
            max_bytes_warn: 2 * 1024 * 1024,
        }
    }

    /// 带校验的构造：`max_edge` 限制在 200–800（设置项取值范围）。
    pub fn validated(max_edge: u32, dither: bool, max_bytes_warn: u64) -> Result<Self> {
        if !(200..=800).contains(&max_edge) {
            return Err(Error::InvalidParams(format!(
                "max_edge {max_edge} 超出 200..=800"
            )));
        }
        Ok(Self {
            max_edge,
            dither,
            max_bytes_warn,
        })
    }
}

impl Default for PipelineParams {
    fn default() -> Self {
        Self::new()
    }
}

/// 转换产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionOutput {
    /// GIF89a 字节流。
    pub gif: Vec<u8>,
    /// 输出宽度（px）。
    pub width: u32,
    /// 输出高度（px）。
    pub height: u32,
    /// 输出帧数。
    pub frames: usize,
    /// 体积超过 [`PipelineParams::max_bytes_warn`]（仍允许继续）。
    pub size_warning: bool,
    /// 源为 webp/apng 动画，已按 docs/02 3.5.1 降级取首帧。
    pub animation_downgraded: bool,
}

/// 解码后的单帧。
struct DecodedFrame {
    image: RgbaImage,
    /// 帧延迟（毫秒）；静态图为 0。
    delay_ms: u32,
}

/// 解码后的整图（含动画信息）。
struct DecodedImage {
    frames: Vec<DecodedFrame>,
    animation_downgraded: bool,
}

/// 缩放后的单帧。
struct ScaledFrame {
    image: RgbaImage,
    delay_ms: u32,
}

/// 执行完整转换管线（docs/02 3.5.1）。
pub fn convert(source: &[u8], params: &PipelineParams) -> Result<ConversionOutput> {
    let decoded = decode_source(source)?;
    let frames = scale_frames(&decoded, params.max_edge);
    let gif = encode_gif(&frames, params.dither)?;
    let width = frames[0].image.width();
    let height = frames[0].image.height();
    Ok(ConversionOutput {
        size_warning: gif.len() as u64 > params.max_bytes_warn,
        gif,
        width,
        height,
        frames: frames.len(),
        animation_downgraded: decoded.animation_downgraded,
    })
}

/// 探测格式并解码：gif 逐帧（保留延迟）；webp/apng 动画取首帧并标记降级；
/// 其余格式按静态图解码单帧。
fn decode_source(source: &[u8]) -> Result<DecodedImage> {
    let format = image::guess_format(source).map_err(|e| Error::Decode(e.to_string()))?;
    match format {
        ImageFormat::Gif => decode_gif(source),
        ImageFormat::WebP => Ok(DecodedImage {
            frames: vec![decode_first_frame(source)?],
            animation_downgraded: webp_is_animated(source),
        }),
        ImageFormat::Png => Ok(DecodedImage {
            frames: vec![decode_first_frame(source)?],
            animation_downgraded: png_is_animated(source),
        }),
        _ => Ok(DecodedImage {
            frames: vec![decode_first_frame(source)?],
            animation_downgraded: false,
        }),
    }
}

/// 静态解码（取首帧）。
fn decode_first_frame(source: &[u8]) -> Result<DecodedFrame> {
    let image = image::load_from_memory(source)
        .map_err(|e| Error::Decode(e.to_string()))?
        .to_rgba8();
    if image.width() == 0 || image.height() == 0 {
        return Err(Error::Decode("图像尺寸为 0".to_string()));
    }
    Ok(DecodedFrame { image, delay_ms: 0 })
}

/// GIF 逐帧解码，帧延迟换算为毫秒。
fn decode_gif(source: &[u8]) -> Result<DecodedImage> {
    let decoder = GifDecoder::new(Cursor::new(source)).map_err(|e| Error::Decode(e.to_string()))?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|e| Error::Decode(e.to_string()))?;
    if frames.is_empty() {
        return Err(Error::Decode("GIF 不含任何帧".to_string()));
    }
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        let (numer, denom) = frame.delay().numer_denom_ms();
        let delay_ms = if denom == 0 {
            0
        } else {
            (u64::from(numer) / u64::from(denom)) as u32
        };
        let image = frame.into_buffer();
        if image.width() == 0 || image.height() == 0 {
            return Err(Error::Decode("GIF 帧尺寸为 0".to_string()));
        }
        out.push(DecodedFrame { image, delay_ms });
    }
    Ok(DecodedImage {
        frames: out,
        animation_downgraded: false,
    })
}

/// 检测 webp 是否为动画（RIFF 容器中 VP8X 块的 ANIM 标志）。
fn webp_is_animated(data: &[u8]) -> bool {
    if data.len() < 16 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return false;
    }
    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let fourcc = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        if fourcc == b"VP8X" {
            // VP8X 负载首字节为标志位：bit1（0x02）= 动画。
            return data.get(pos + 8).is_some_and(|f| f & 0x02 != 0);
        }
        pos += 8 + size + (size & 1); // 块按偶数长度对齐
    }
    false
}

/// 检测 PNG 是否为 APNG 动画（存在 `acTL` 块；一期取首帧，docs/02 3.5.1）。
fn png_is_animated(data: &[u8]) -> bool {
    data.windows(4).any(|w| w == b"acTL")
}

/// 等比缩放全部帧：长边 > `max_edge` 时缩至 `max_edge`（Lanczos3）；小于则不放大。
fn scale_frames(decoded: &DecodedImage, max_edge: u32) -> Vec<ScaledFrame> {
    decoded
        .frames
        .iter()
        .map(|f| ScaledFrame {
            image: scale_image(&f.image, max_edge),
            delay_ms: f.delay_ms,
        })
        .collect()
}

/// 单帧缩放。
fn scale_image(image: &RgbaImage, max_edge: u32) -> RgbaImage {
    let (width, height) = image.dimensions();
    let (nwidth, nheight) = fit_within(width, height, max_edge);
    if nwidth == width && nheight == height {
        return image.clone();
    }
    image::imageops::resize(image, nwidth, nheight, FilterType::Lanczos3)
}

/// 计算等比缩放目标尺寸（长边 ≤ `max_edge`，不放大；任一边至少 1px）。
fn fit_within(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let long_edge = width.max(height);
    if long_edge <= max_edge || long_edge == 0 {
        return (width, height);
    }
    if width >= height {
        let nheight = (u64::from(height) * u64::from(max_edge) / u64::from(width)).max(1);
        (max_edge, nheight as u32)
    } else {
        let nwidth = (u64::from(width) * u64::from(max_edge) / u64::from(height)).max(1);
        (nwidth as u32, max_edge)
    }
}

/// 编码 GIF89a：循环标记（无限循环）、每帧本地调色板、透明索引、帧延迟（10ms 单位）。
fn encode_gif(frames: &[ScaledFrame], dither: bool) -> Result<Vec<u8>> {
    let first = frames
        .first()
        .ok_or_else(|| Error::Encode("没有可编码的帧".to_string()))?;
    let width = first.image.width();
    let height = first.image.height();
    let gif_width =
        u16::try_from(width).map_err(|_| Error::Encode(format!("宽度 {width} 超出 GIF 上限")))?;
    let gif_height =
        u16::try_from(height).map_err(|_| Error::Encode(format!("高度 {height} 超出 GIF 上限")))?;
    let usize_width = usize::try_from(width)
        .map_err(|_| Error::Encode(format!("宽度 {width} 超出平台 usize 上限")))?;
    let usize_height = usize::try_from(height)
        .map_err(|_| Error::Encode(format!("高度 {height} 超出平台 usize 上限")))?;

    let mut buf: Vec<u8> = Vec::new();
    {
        let mut encoder = Encoder::new(&mut buf, gif_width, gif_height, &[])
            .map_err(|e| Error::Encode(e.to_string()))?;
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| Error::Encode(e.to_string()))?;
        for frame in frames {
            let (fw, fh) = frame.image.dimensions();
            if fw != width || fh != height {
                return Err(Error::Encode(format!(
                    "各帧尺寸不一致：{fw}x{fh} 与 {width}x{height}"
                )));
            }
            let quantized =
                quantize::quantize(frame.image.as_raw(), usize_width, usize_height, 256, dither)?;
            let mut palette_bytes = Vec::with_capacity(quantized.palette.len() * 3);
            for color in &quantized.palette {
                palette_bytes.extend_from_slice(&color[..3]);
            }
            // 透明槽位固定为索引 0（见 quantize 模块约定）。
            let transparent = quantized.palette.first().filter(|c| c[3] == 0).map(|_| 0u8);
            let mut gif_frame = Frame::from_palette_pixels(
                gif_width,
                gif_height,
                quantized.indices,
                palette_bytes,
                transparent,
            );
            // GIF 帧延迟单位为 10ms；保留毫秒精度（四舍五入）。
            gif_frame.delay =
                ((u64::from(frame.delay_ms) + 5) / 10).min(u64::from(u16::MAX)) as u16;
            encoder
                .write_frame(&gif_frame)
                .map_err(|e| Error::Encode(e.to_string()))?;
        }
    }
    Ok(buf)
}

/// 产物缓存键（docs/02 3.5.1）：源字节 SHA-256 前 16 位十六进制 + 长边上限。
///
/// 用于 `emoji_cache/{key}/upload.gif` 目录名；上传失败重试不重复转换。
pub fn cache_key_for(source: &[u8], max_edge: u32) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(source);
    let hex = hex::encode(digest);
    format!("{}{}", &hex[..16], max_edge)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 编码 PNG 测试图。
    fn encode_png(image: RgbaImage) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .expect("编码 PNG 失败");
        cursor.into_inner()
    }

    /// 生成 600x400 水平渐变 PNG（测试图一）。
    fn gradient_png(width: u32, height: u32) -> Vec<u8> {
        let mut img = RgbaImage::new(width, height);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let t = (x as f32) / ((width - 1) as f32);
            let s = (y as f32) / ((height - 1) as f32);
            *px = image::Rgba([
                (t * 255.0) as u8,
                (s * 255.0) as u8,
                ((1.0 - t) * 255.0) as u8,
                255,
            ]);
        }
        encode_png(img)
    }

    /// 生成指定尺寸纯色 PNG。
    fn solid_png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut img = RgbaImage::new(width, height);
        for px in img.pixels_mut() {
            *px = image::Rgba(color);
        }
        encode_png(img)
    }

    /// 生成 n 帧纯色 GIF（帧延迟单位为 10ms）。
    fn multi_frame_gif(size: u16, frames: &[([u8; 3], u16)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder =
                gif::Encoder::new(&mut buf, size, size, &[]).expect("创建 GIF 编码器失败");
            encoder
                .set_repeat(gif::Repeat::Infinite)
                .expect("设置循环失败");
            for (color, delay) in frames {
                let mut pixels = Vec::with_capacity(usize::from(size) * usize::from(size) * 4);
                for _ in 0..usize::from(size) * usize::from(size) {
                    pixels.extend_from_slice(&[color[0], color[1], color[2], 255]);
                }
                let mut frame = gif::Frame::from_rgba(size, size, &mut pixels);
                frame.delay = *delay;
                encoder.write_frame(&frame).expect("写帧失败");
            }
        }
        buf
    }

    /// 解码 GIF，返回（宽, 高, 帧数, 帧延迟(10ms), 是否含无限循环标记）。
    fn inspect_gif(data: &[u8]) -> (u32, u32, usize, Vec<u16>, bool) {
        let decoder = GifDecoder::new(Cursor::new(data)).expect("解码 GIF 失败");
        let frames = decoder.into_frames().collect_frames().expect("收集帧失败");
        let first = frames.first().expect("GIF 无帧");
        let (w, h) = first.buffer().dimensions();
        let delays: Vec<u16> = frames
            .iter()
            .map(|f| f.delay().numer_denom_ms().0 as u16 / 10)
            .collect();
        // NETSCAPE2.0 应用扩展 + 子块 03 01 00 00 = 无限循环。
        let loops = data.windows(11).any(|w| w == b"NETSCAPE2.0")
            && data.windows(4).any(|w| w == [0x03, 0x01, 0x00, 0x00]);
        (w, h, frames.len(), delays, loops)
    }

    #[test]
    fn gradient_png_scales_down_keeping_ratio() {
        let source = gradient_png(600, 400);
        let params = PipelineParams::new();
        let out = convert(&source, &params).expect("转换失败");
        assert!(out.width <= 400, "宽 {} 应 ≤400", out.width);
        assert!(out.height <= 400, "高 {} 应 ≤400", out.height);
        // 600:400 = 3:2，允许 1px 取整误差。
        assert!(
            (i64::from(out.width) * 2 - i64::from(out.height) * 3).abs() <= 2,
            "比例未保持：{}x{}",
            out.width,
            out.height
        );
        let (w, h, frames, ..) = inspect_gif(&out.gif);
        assert_eq!((w, h), (out.width, out.height));
        assert_eq!(frames, 1);
        assert!(!out.animation_downgraded);
    }

    #[test]
    fn animated_gif_keeps_frames_delays_and_loop() {
        let source = multi_frame_gif(
            100,
            &[([255, 0, 0], 10), ([0, 255, 0], 20), ([0, 0, 255], 5)],
        );
        let out = convert(&source, &PipelineParams::new()).expect("转换失败");
        let (w, h, frames, delays, loops) = inspect_gif(&out.gif);
        assert_eq!((w, h), (100, 100));
        assert_eq!(frames, 3, "应保留 3 帧");
        assert_eq!(out.frames, 3);
        assert_eq!(delays, vec![10, 20, 5], "帧延迟应保留（10ms 单位）");
        assert!(loops, "应含无限循环标记");
        assert!(!out.animation_downgraded);
    }

    #[test]
    fn small_image_not_upscaled() {
        let source = solid_png(100, 100, [12, 34, 56, 255]);
        let out = convert(&source, &PipelineParams::new()).expect("转换失败");
        assert_eq!((out.width, out.height), (100, 100), "小图不应放大");
        let (w, h, ..) = inspect_gif(&out.gif);
        assert_eq!((w, h), (100, 100));
    }

    #[test]
    fn size_warning_when_exceeding_threshold() {
        let source = gradient_png(400, 400);
        let mut params = PipelineParams::new();
        params.max_bytes_warn = 100; // 强制触发警告阈值
        let out = convert(&source, &params).expect("转换失败");
        assert!(out.size_warning, "超过阈值应置警告位");
        assert!(out.gif.len() > 100);

        let mut ok_params = PipelineParams::new();
        ok_params.max_bytes_warn = 10 * 1024 * 1024;
        let small = convert(&solid_png(64, 64, [1, 2, 3, 255]), &ok_params).expect("转换失败");
        assert!(!small.size_warning, "小图不应触发警告");
    }

    #[test]
    fn palette_within_256_colors() {
        let source = gradient_png(400, 300);
        let out = convert(&source, &PipelineParams::new()).expect("转换失败");
        // 经 gif crate 解码复核调色板大小。
        let mut decoder = gif::Decoder::new(Cursor::new(&out.gif)).expect("解码 GIF 失败");
        let mut frame_count = 0usize;
        while let Some(frame) = decoder.read_next_frame().expect("读帧失败") {
            frame_count += 1;
            if let Some(palette) = &frame.palette {
                assert!(
                    palette.len() / 3 <= 256,
                    "帧调色板 {} 超出 256",
                    palette.len() / 3
                );
            }
        }
        assert_eq!(frame_count, out.frames);
    }

    #[test]
    fn transparent_png_marks_transparent_index() {
        // 左半透明、右半不透明的 PNG。
        let mut img = RgbaImage::new(64, 64);
        for (x, _y, px) in img.enumerate_pixels_mut() {
            if x < 32 {
                *px = image::Rgba([0, 0, 0, 0]);
            } else {
                *px = image::Rgba([200, 100, 50, 255]);
            }
        }
        let buf = encode_png(img);
        let out = convert(&buf, &PipelineParams::new()).expect("转换失败");
        let mut decoder = gif::Decoder::new(Cursor::new(&out.gif)).expect("解码 GIF 失败");
        let frame = decoder
            .read_next_frame()
            .expect("读帧失败")
            .expect("GIF 无帧");
        assert_eq!(frame.transparent, Some(0), "索引 0 应为透明索引");
    }

    #[test]
    fn webp_animation_detection() {
        // 静态 webp（VP8 块）不应判定为动画。
        let mut static_webp = b"RIFF____WEBPVP8 ".to_vec();
        static_webp.extend_from_slice(&[0u8; 16]);
        assert!(!webp_is_animated(&static_webp));

        // VP8X 标志位 0x02 = 动画。
        let mut animated_webp = b"RIFF____WEBPVP8X".to_vec();
        animated_webp.extend_from_slice(&10u32.to_le_bytes());
        animated_webp.extend_from_slice(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(webp_is_animated(&animated_webp));

        assert!(!webp_is_animated(b"not a webp file at all"));
    }

    #[test]
    fn png_animation_detection() {
        let mut apng = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        apng.extend_from_slice(&[0, 0, 0, 13]);
        apng.extend_from_slice(b"IHDR");
        apng.extend_from_slice(&[0u8; 13]);
        apng.extend_from_slice(&[0, 0, 0, 8]);
        apng.extend_from_slice(b"acTL");
        apng.extend_from_slice(&[2u32.to_le_bytes(), 0u32.to_le_bytes()].concat());
        assert!(png_is_animated(&apng));
        assert!(!png_is_animated(&[
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0
        ]));
    }

    #[test]
    fn cache_key_format() {
        let key = cache_key_for(b"hello", 400);
        assert_eq!(key.len(), 16 + 3, "应为 16 位十六进制 + max_edge");
        assert!(key[..16].chars().all(|c| c.is_ascii_hexdigit()));
        assert!(key.ends_with("400"));
        // 同一输入 + 同一参数 → 稳定键；不同 max_edge → 不同键。
        assert_eq!(key, cache_key_for(b"hello", 400));
        assert_ne!(key, cache_key_for(b"hello", 600));
        assert_ne!(key, cache_key_for(b"world", 400));
    }

    #[test]
    fn invalid_source_rejected() {
        assert!(convert(b"not an image", &PipelineParams::new()).is_err());
        assert!(convert(&[], &PipelineParams::new()).is_err());
    }

    #[test]
    fn validated_params_range() {
        assert!(PipelineParams::validated(400, true, 1024).is_ok());
        assert!(PipelineParams::validated(199, true, 1024).is_err());
        assert!(PipelineParams::validated(801, true, 1024).is_err());
        assert_eq!(PipelineParams::default().max_edge, 400);
    }
}
