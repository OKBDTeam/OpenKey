//! openkey-sync 错误。

use crate::state::{SyncEvent, SyncState};

/// 同步引擎错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 错误。
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    /// HLC 解析或运算错误。
    #[error("HLC 错误: {0}")]
    Hlc(String),
    /// 数据损坏（如未知条目类型）。
    #[error("数据损坏: {0}")]
    Corrupt(String),
    /// 同步状态机非法迁移。
    #[error("非法状态迁移: {state:?} --{event:?}-->")]
    InvalidTransition { state: SyncState, event: SyncEvent },
    /// 快照格式错误。
    #[error("快照错误: {0}")]
    Snapshot(String),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;
