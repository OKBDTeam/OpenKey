//! openkey-vault：OpenKey 密码库（Bitwarden 协议栈 + 本地 vault，docs/02 3.4 / 6.2）。
//!
//! v0.1.0 交付范围：
//! - `enc`：[`EncString`] type2（AES-256-CBC + HMAC-SHA256，线格式
//!   `2.{iv}|{ct}|{mac}`），加解密、MAC 恒定时间校验、serde 线格式；
//! - `kdf`：PBKDF2-SHA256（默认 600k）/ Argon2id（m=64MiB,t=3,p=4），
//!   email 小写作 salt；HKDF-SHA256 stretch master key；服务器认证哈希；
//! - `keys`：[`UserKey`]（64B = enc||mac，`ZeroizeOnDrop`）与
//!   [`StretchedMasterKey`]；
//! - `local`：[`LocalVault`] 增删查 + 锁定状态机（`lock` 模块）；
//! - `api`：[`BitwardenRest`] trait 与 [`BitwardenEndpoints`]（identity/api
//!   分离），HTTP 传输层待 Phase 3 reqwest 适配器。
//!
//! 测试向量：`enc`/`kdf` 采用与 Bitwarden 官方客户端语义一致的公开向量
//! （Miguel Grinberg 博客；Bitwarden 官方仓库未发布可直接引用的向量文件，
//! 2026-10 检索 bitwarden/sdk 相关路径 404），全部数值已经独立 Python
//! 实现交叉验证；另附 RFC 5869 HKDF 标准向量与 Python 对拍的确定性加密向量。
//!
//! 待后续阶段：
//! - type7（COSE）EncString 与附件流式加解密；
//! - WebDAV/官方云 REST 适配器（`BitwardenRest` 实现）；
//! - Bitwarden password-protected 加密 JSON 导出（互通备份）；
//! - 生物识别包裹件（PIN / keystore）与改密码/KDF 轮换。

pub mod api;
pub mod enc;
pub mod error;
pub mod kdf;
pub mod keys;
pub mod local;
pub mod lock;

pub use api::{AuthTokens, BitwardenEndpoints, BitwardenRest};
pub use enc::EncString;
pub use error::{Error, Result};
pub use kdf::{
    derive_and_stretch, derive_master_key, hkdf_expand, kdf_salt, master_password_hash,
    master_password_hash_b64, stretch_master_key, KdfConfig, ARGON2_DEFAULT_ITERATIONS,
    ARGON2_DEFAULT_MEMORY_KIB, ARGON2_DEFAULT_PARALLELISM, HKDF_INFO_ENC, HKDF_INFO_MAC,
    PBKDF2_DEFAULT_ITERATIONS,
};
pub use keys::{CryptoKey, StretchedMasterKey, UserKey, KEY_LEN};
pub use local::{EncryptedItem, LocalVault, VaultItem};
pub use lock::{lock_next, LockEvent, LockState};
