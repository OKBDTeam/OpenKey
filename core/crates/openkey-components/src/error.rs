//! openkey-components 错误。

/// 组件分发错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 错误。
    #[error("清单 JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    /// zip 错误。
    #[error("zip 错误: {0}")]
    Zip(String),
    /// 清单中无此组件。
    #[error("清单中无组件: {0}")]
    NotInManifest(String),
    /// 清单 schema 版本不支持（当前仅支持 1）。
    #[error("不支持的清单 schema: 期望 {expected}, 实际 {found}")]
    UnsupportedSchema { expected: u32, found: u32 },
    /// 组件未安装。
    #[error("组件未安装: {0}")]
    NotInstalled(String),
    /// SHA-256 不匹配。
    #[error("SHA-256 不匹配: 期望 {expected}, 实际 {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    /// 版本已存在（解压目标非空）。
    #[error("版本已存在: {0}")]
    VersionExists(String),
    /// 清单签名缺失或无效（供应链信任，docs/02 6.4）。
    #[error("清单签名无效: {0}")]
    BadSignature(String),
    /// 下载失败。
    #[error("下载失败: {0}")]
    Download(String),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;
