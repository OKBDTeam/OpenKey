//! openkey-vault 错误类型。

use crate::lock::{LockEvent, LockState};

/// vault 协议栈错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    /// Base64 解码错误。
    #[error("Base64 解码错误: {0}")]
    Base64(#[from] base64::DecodeError),
    /// JSON 错误。
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    /// 解密结果不是合法 UTF-8。
    #[error("解密结果不是合法 UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    /// EncString 线格式错误。
    #[error("EncString 格式错误: {0}")]
    EncString(String),
    /// 不支持的 EncString 加密类型（一期仅支持 type2 `AesCbc256_HmacSha256`）。
    #[error("不支持的 EncString 加密类型: {0:?}（一期仅支持 type2 AesCbc256_HmacSha256）")]
    UnsupportedType(String),
    /// MAC 校验失败：密文被篡改、密钥不正确或主密码错误。
    #[error("MAC 校验失败：密文被篡改或密钥不正确")]
    MacMismatch,
    /// AES-CBC 解密失败（长度非分组倍数或 PKCS#7 填充非法）。
    #[error("解密失败: {0}")]
    Decrypt(String),
    /// KDF 参数或派生错误。
    #[error("KDF 错误: {0}")]
    Kdf(String),
    /// Argon2 错误。
    #[error("Argon2 错误: {0}")]
    Argon2(#[from] argon2::Error),
    /// vault 处于锁定/软锁定状态，需先解锁。
    #[error("vault 已锁定（{state:?}），需先解锁")]
    Locked { state: LockState },
    /// 锁定状态机非法迁移。
    #[error("非法锁定状态迁移: {from:?} --{event:?}-->")]
    InvalidLockTransition { from: LockState, event: LockEvent },
    /// 条目不存在。
    #[error("条目不存在: {0}")]
    NotFound(String),
    /// REST 层错误（HTTP 适配器待 Phase 3 落地，此处先固定协议语义错误）。
    #[error("REST 错误: {0}")]
    Rest(String),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;
