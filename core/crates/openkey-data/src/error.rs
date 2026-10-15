//! data 层错误。

/// 数据层错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// SQLite 错误。
    #[error("SQLite 错误: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 错误。
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    /// 数据损坏（如未知条目类型）。
    #[error("数据损坏: {0}")]
    Corrupt(String),
    /// 记录不存在。
    #[error("记录不存在: {0}")]
    NotFound(String),
    /// 迁移失败。
    #[error("迁移失败: {0}")]
    Migration(String),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;
