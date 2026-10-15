//! openkey-emoji：表情上传图像管线（docs/02 3.5，DR-003）。
//!
//! 上传图床前统一执行：等比缩放（长边 > `max_edge` 时缩至 `max_edge`，默认 400px）
//! → 256 色量化 + Floyd–Steinberg 抖动 → GIF89a 编码；产物经 openkey-imagehost
//! 上传，原图留存本地表情库。
//!
//! 产物缓存布局（docs/02 3.5.1）：
//! - `emoji_cache/{key}/upload.gif`：转换产物，`key` 见 [`cache_key_for`]；
//! - `emoji_files/{sha256}.{ext}`：原图，上传失败重试不重复转换。
//!
//! 本 crate 只被各端主 App / 设置程序链接（转换执行位置见 docs/02 3.5.1），
//! 键盘进程不执行图像管线。

pub mod cache;
pub mod error;
pub mod pipeline;
pub mod quantize;

pub use error::{Error, Result};
pub use pipeline::{cache_key_for, convert, ConversionOutput, PipelineParams};
pub use quantize::{quantize, QuantizeOutput, TRANSPARENT_ALPHA_THRESHOLD};
