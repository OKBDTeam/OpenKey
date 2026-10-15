//! openkey-emoji 错误类型。

/// 图像管线错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 图像解码失败（格式不支持或数据损坏）。
    #[error("图像解码失败: {0}")]
    Decode(String),
    /// GIF 编码失败。
    #[error("GIF 编码失败: {0}")]
    Encode(String),
    /// 量化失败（像素数据或参数非法）。
    #[error("量化失败: {0}")]
    Quantize(String),
    /// 参数非法。
    #[error("参数非法: {0}")]
    InvalidParams(String),
    /// IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;
