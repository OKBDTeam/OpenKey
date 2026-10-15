//! KDF：主密钥派生、stretch、服务器认证哈希（docs/02 3.4.2 / 6.2）。
//!
//! 加密流程（与官方客户端语义一致）：
//!
//! ```text
//! master password + email(lowercased) ──KDF──▶ master key(32B)
//! master key ──HKDF-SHA256(expand)──▶ stretched master key(64B = enc||mac)
//! stretched master key ──解密──▶ user key(64B = 32B enc + 32B mac)
//! 认证密码 = PBKDF2(pw, master_key, 1)，仅发服务器
//! ```
//!
//! KDF 参数由 prelogin 从服务器获取（一期也支持本地 vault 自设）；
//! PBKDF2-SHA256 默认 600k 迭代，Argon2id 默认 m=64MiB / t=3 / p=4，
//! salt 均为 email 转小写。

use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::keys::StretchedMasterKey;

/// PBKDF2-SHA256 默认迭代数（Bitwarden 现行默认）。
pub const PBKDF2_DEFAULT_ITERATIONS: u32 = 600_000;

/// Argon2id 默认内存成本（KiB，64MiB）。
pub const ARGON2_DEFAULT_MEMORY_KIB: u32 = 64 * 1024;
/// Argon2id 默认时间成本（迭代数）。
pub const ARGON2_DEFAULT_ITERATIONS: u32 = 3;
/// Argon2id 默认并行度。
pub const ARGON2_DEFAULT_PARALLELISM: u32 = 4;

/// HKDF-Expand 的 info 标签：加密子密钥。
pub const HKDF_INFO_ENC: &[u8] = b"enc";
/// HKDF-Expand 的 info 标签：MAC 子密钥。
pub const HKDF_INFO_MAC: &[u8] = b"mac";

type HmacSha256 = Hmac<Sha256>;

/// KDF 配置（prelogin 从服务器获取，或本地 vault 自设）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdfConfig {
    /// PBKDF2-HMAC-SHA256；salt = email 小写。
    Pbkdf2 {
        /// 迭代数。
        iterations: u32,
    },
    /// Argon2id；salt = email 小写。
    Argon2id {
        /// 内存成本（KiB）。
        memory_kib: u32,
        /// 时间成本（迭代数）。
        iterations: u32,
        /// 并行度。
        parallelism: u32,
    },
}

impl Default for KdfConfig {
    fn default() -> Self {
        Self::pbkdf2_default()
    }
}

impl KdfConfig {
    /// Bitwarden 默认档：PBKDF2-SHA256 600k。
    pub fn pbkdf2_default() -> Self {
        Self::Pbkdf2 {
            iterations: PBKDF2_DEFAULT_ITERATIONS,
        }
    }

    /// Bitwarden 默认档：Argon2id m=64MiB / t=3 / p=4。
    pub fn argon2_default() -> Self {
        Self::Argon2id {
            memory_kib: ARGON2_DEFAULT_MEMORY_KIB,
            iterations: ARGON2_DEFAULT_ITERATIONS,
            parallelism: ARGON2_DEFAULT_PARALLELISM,
        }
    }

    /// Bitwarden 线路上的 KDF 类型值：0 = PBKDF2-SHA256，1 = Argon2id。
    pub fn wire_id(&self) -> u32 {
        match self {
            KdfConfig::Pbkdf2 { .. } => 0,
            KdfConfig::Argon2id { .. } => 1,
        }
    }

    /// 由 prelogin 响应字段构造（Bitwarden/Vaultwarden `accounts/prelogin`）。
    ///
    /// - `kdf`：0 = PBKDF2-SHA256，1 = Argon2id；
    /// - `iterations`：`kdfIterations`（PBKDF2 迭代数 / Argon2id 时间成本）；
    /// - `memory_mib`：`kdfMemory`（MB，仅 Argon2id；缺省按 64MB）；
    /// - `parallelism`：`kdfParallelism`（仅 Argon2id；缺省按 4）。
    pub fn from_server(
        kdf: u32,
        iterations: u32,
        memory_mib: Option<u32>,
        parallelism: Option<u32>,
    ) -> Result<Self> {
        if iterations == 0 {
            return Err(Error::Kdf("KDF 迭代数为 0".to_string()));
        }
        match kdf {
            0 => Ok(KdfConfig::Pbkdf2 { iterations }),
            1 => {
                let memory_kib = memory_mib
                    .unwrap_or(64)
                    .checked_mul(1024)
                    .ok_or_else(|| Error::Kdf(format!("kdfMemory 溢出: {memory_mib:?} MB")))?;
                Ok(KdfConfig::Argon2id {
                    memory_kib,
                    iterations,
                    parallelism: parallelism.unwrap_or(ARGON2_DEFAULT_PARALLELISM),
                })
            }
            other => Err(Error::Kdf(format!("未知 KDF 类型: {other}"))),
        }
    }
}

/// KDF salt：email 转小写（Bitwarden 语义；大小写不敏感登录）。
pub fn kdf_salt(email: &str) -> String {
    email.to_lowercase()
}

/// 主密钥派生：`master password + email(小写) ──KDF──▶ master key(32B)`。
pub fn derive_master_key(
    password: &str,
    email: &str,
    config: &KdfConfig,
) -> Result<Zeroizing<[u8; 32]>> {
    let salt = kdf_salt(email);
    match *config {
        KdfConfig::Pbkdf2 { iterations } => {
            let key = pbkdf2::pbkdf2_hmac_array::<Sha256, 32>(
                password.as_bytes(),
                salt.as_bytes(),
                iterations,
            );
            Ok(Zeroizing::new(key))
        }
        KdfConfig::Argon2id {
            memory_kib,
            iterations,
            parallelism,
        } => {
            let params = argon2::Params::new(memory_kib, iterations, parallelism, Some(32))?;
            let argon2 =
                argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
            let mut key = Zeroizing::new([0u8; 32]);
            argon2.hash_password_into(password.as_bytes(), salt.as_bytes(), &mut *key)?;
            Ok(key)
        }
    }
}

/// HKDF-Expand（RFC 5869，SHA-256；Bitwarden 仅用 Expand 阶段，PRK 即 master key）。
///
/// `prk` 为伪随机密钥，`info` 为上下文标签，`len` 为输出长度（≤ 255×32）。
pub fn hkdf_expand(prk: &[u8], info: &[u8], len: usize) -> Result<Vec<u8>> {
    const HASH_LEN: usize = 32;
    const MAX_LEN: usize = 255 * HASH_LEN;
    if len > MAX_LEN {
        return Err(Error::Kdf(format!("HKDF 输出长度超限: {len} > {MAX_LEN}")));
    }
    let mut okm = Vec::with_capacity(len);
    let mut block: Vec<u8> = Vec::with_capacity(HASH_LEN);
    let mut counter: u8 = 0;
    while okm.len() < len {
        counter = counter
            .checked_add(1)
            .ok_or_else(|| Error::Kdf("HKDF 计数器溢出".to_string()))?;
        let mut mac = HmacSha256::new_from_slice(prk)
            .map_err(|e| Error::Kdf(format!("HKDF HMAC 初始化失败: {e}")))?;
        mac.update(&block);
        mac.update(info);
        mac.update(&[counter]);
        block.clear();
        block.extend_from_slice(&mac.finalize().into_bytes());
        let take = (len - okm.len()).min(block.len());
        okm.extend_from_slice(&block[..take]);
    }
    Ok(okm)
}

/// stretch master key：`master key ──HKDF-SHA256(expand)──▶ enc||mac(64B)`。
pub fn stretch_master_key(master_key: &[u8; 32]) -> Result<StretchedMasterKey> {
    let enc = hkdf_expand(master_key, HKDF_INFO_ENC, 32)?;
    let mac = hkdf_expand(master_key, HKDF_INFO_MAC, 32)?;
    StretchedMasterKey::from_keys(&enc, &mac)
}

/// 服务器认证哈希：`PBKDF2-SHA256(password, master_key, 1)`（仅发服务器，
/// 客户端不用于本地派生；Bitwarden `HashPurpose::ServerAuthorization`）。
pub fn master_password_hash(password: &str, master_key: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    Zeroizing::new(pbkdf2::pbkdf2_hmac_array::<Sha256, 32>(
        password.as_bytes(),
        master_key,
        1,
    ))
}

/// 服务器认证哈希的 base64（Standard 字母表，带填充）。
pub fn master_password_hash_b64(password: &str, master_key: &[u8; 32]) -> Zeroizing<String> {
    use base64::Engine as _;
    Zeroizing::new(
        base64::engine::general_purpose::STANDARD
            .encode(*master_password_hash(password, master_key)),
    )
}

/// 便捷构造：派生主密钥后立即 stretch（本地 vault 与登录路径常用）。
pub fn derive_and_stretch(
    password: &str,
    email: &str,
    config: &KdfConfig,
) -> Result<StretchedMasterKey> {
    let master_key = derive_master_key(password, email, config)?;
    stretch_master_key(&master_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::CryptoKey as _;

    // ---- 测试向量 ----
    //
    // 来源：Miguel Grinberg《How Bitwarden Encrypts and Decrypts Secrets》
    // （https://blog.miguelgrinberg.com/post/how-bitwarden-encrypts-and-decrypts-secrets），
    // 与 Bitwarden 官方客户端语义一致；本仓库已用独立 Python 实现
    // （hashlib/hmac/pycryptodome/argon2-cffi）交叉验证全部数值。
    // Bitwarden 官方仓库未发布可直接引用的向量文件（bitwarden/sdk 目录结构
    // 多次变动，2026-10 检索 404），故采用该公开向量 + 下方 RFC 5869 标准向量。

    /// 向量 2 的 master password。
    const VEC_PASSWORD: &str = "the moon landing was fake";
    /// 向量 2 的 email（KDF salt，小写）。
    const VEC_EMAIL: &str = "somebody@example.com";
    /// 向量 2 的 master key（PBKDF2-SHA256 600k 输出，32B；注意它不是 64B user key 的前半）。
    const VEC_MASTER_KEY: [u8; 32] = [
        0xbd, 0x15, 0x66, 0x58, 0x44, 0x76, 0x9e, 0x6e, 0x88, 0xb4, 0x40, 0x09, 0x98, 0x7b, 0xeb,
        0xeb, 0xa7, 0xaa, 0xe7, 0x54, 0x76, 0x3a, 0x91, 0x78, 0xdd, 0x0b, 0xe6, 0xc9, 0x4f, 0x42,
        0x1e, 0x0b,
    ];
    /// 向量 2 的 wrapped user key（stretched master key 加密的 64B user key）。
    const VEC_WRAPPED_USER_KEY: &str = "2.i5dH92a79wJ8L8tsqQEdLw==|a5swb8CeW5cTM2N+XQZCF+mX263BMaag+ghxiu+ci4W+fqqLZ82g+i7ReIcdiPLafoCAmeWZE48PETGJOsoOb6DcrK3sRdvHCx8xbRt1Xas=|MhY1/he1RntesSYJyZqFh8s8dQTXqmPRQM2hVcsjIWk=";
    /// 向量 2 解包出的 user key（64B）。
    const VEC_USER_KEY: [u8; 64] = [
        0xa3, 0x3f, 0xbc, 0x86, 0x18, 0x7f, 0x9c, 0x7c, 0xe2, 0xf1, 0x10, 0xd4, 0xee, 0x20, 0x42,
        0xde, 0x93, 0x12, 0x67, 0x03, 0x5c, 0x83, 0x9a, 0xc5, 0x53, 0x3c, 0x21, 0x18, 0xc0, 0x0e,
        0x52, 0x70, 0xb5, 0xbc, 0x60, 0xfc, 0xce, 0x75, 0x29, 0x93, 0x51, 0x84, 0x72, 0xaa, 0xd6,
        0xde, 0x1f, 0xc6, 0x59, 0x92, 0x85, 0x3f, 0xf8, 0x6a, 0x95, 0xe9, 0x38, 0x8e, 0xe5, 0xe0,
        0x98, 0xd8, 0x85, 0x9c,
    ];
    /// stretch(VEC_MASTER_KEY) 的 enc 子密钥（Python 交叉验证）。
    const VEC_STRETCH_ENC: [u8; 32] = [
        0x8a, 0x02, 0x1d, 0x6f, 0x9a, 0x99, 0xa9, 0x69, 0x3e, 0x60, 0xdc, 0xf5, 0x1f, 0x03, 0xe0,
        0xfc, 0x4f, 0xc0, 0xff, 0xfe, 0x54, 0x40, 0x57, 0xb9, 0xd8, 0xd3, 0x70, 0xfe, 0x3c, 0x40,
        0x91, 0xc1,
    ];
    /// stretch(VEC_MASTER_KEY) 的 mac 子密钥（Python 交叉验证）。
    const VEC_STRETCH_MAC: [u8; 32] = [
        0x1a, 0x9c, 0x3f, 0x09, 0x30, 0xbb, 0xda, 0x59, 0x13, 0x50, 0x5b, 0x95, 0xc2, 0x3c, 0x07,
        0xb0, 0x64, 0x6f, 0x2c, 0xa8, 0x85, 0xc7, 0xa7, 0x46, 0x3b, 0x38, 0xb6, 0xa9, 0x25, 0x73,
        0x03, 0xe7,
    ];
    /// master_password_hash(VEC_PASSWORD, VEC_MASTER_KEY)（Python 交叉验证）。
    const VEC_MASTER_PW_HASH: [u8; 32] = [
        0xfa, 0x13, 0x4a, 0xd3, 0x93, 0x89, 0x29, 0x3a, 0x9c, 0x5f, 0x17, 0x96, 0x2b, 0x77, 0x29,
        0x42, 0x7c, 0xa8, 0xe1, 0xa9, 0x7f, 0x75, 0x3a, 0x8f, 0xff, 0xff, 0x52, 0xb6, 0x86, 0x61,
        0x34, 0x59,
    ];
    /// Argon2id 向量（argon2-cffi 生成，Bitwarden 默认参数 m=64MiB/t=3/p=4）。
    const VEC_ARGON2_EMAIL: &str = "argon2.user@example.com";
    const VEC_ARGON2_PASSWORD: &str = "correct horse battery staple";
    const VEC_ARGON2_KEY: [u8; 32] = [
        0xb2, 0xda, 0x8f, 0xc8, 0xbe, 0x9f, 0xd5, 0xac, 0xcc, 0x17, 0xb6, 0xbb, 0xf9, 0x03, 0xcb,
        0xce, 0xed, 0x11, 0x5f, 0x07, 0x53, 0xa9, 0x5d, 0xf3, 0x1c, 0x3d, 0x45, 0xed, 0xd3, 0xea,
        0x7d, 0x16,
    ];

    #[test]
    fn full_kdf_chain_unwraps_user_key() {
        // PBKDF2 600k 略慢（数百毫秒），属可接受范围
        let master_key = derive_master_key(VEC_PASSWORD, VEC_EMAIL, &KdfConfig::pbkdf2_default())
            .ok()
            .unwrap();
        assert_eq!(*master_key, VEC_MASTER_KEY);
        let stretched = stretch_master_key(&master_key).ok().unwrap();
        let wrapped = crate::enc::EncString::parse(VEC_WRAPPED_USER_KEY)
            .ok()
            .unwrap();
        let user_key = wrapped.decrypt(&stretched).ok().unwrap();
        assert_eq!(user_key.as_slice(), &VEC_USER_KEY);
    }

    #[test]
    fn stretch_master_key_matches_reference() {
        let stretched = stretch_master_key(&VEC_MASTER_KEY).ok().unwrap();
        assert_eq!(stretched.enc_key(), &VEC_STRETCH_ENC);
        assert_eq!(stretched.mac_key(), &VEC_STRETCH_MAC);
    }

    #[test]
    fn master_password_hash_matches_reference() {
        let hash = master_password_hash(VEC_PASSWORD, &VEC_MASTER_KEY);
        assert_eq!(*hash, VEC_MASTER_PW_HASH);
        use base64::Engine as _;
        let b64 = master_password_hash_b64(VEC_PASSWORD, &VEC_MASTER_KEY);
        assert_eq!(
            *b64,
            base64::engine::general_purpose::STANDARD.encode(VEC_MASTER_PW_HASH)
        );
    }

    #[test]
    fn argon2id_default_params_match_reference() {
        let key = derive_master_key(
            VEC_ARGON2_PASSWORD,
            VEC_ARGON2_EMAIL,
            &KdfConfig::argon2_default(),
        )
        .ok()
        .unwrap();
        assert_eq!(*key, VEC_ARGON2_KEY);
    }

    #[test]
    fn hkdf_expand_rfc5869_sha256_case1() {
        // RFC 5869 测试用例 1（SHA-256，L=42）
        let prk = [
            0x07, 0x77, 0x09, 0x36, 0x2c, 0x2e, 0x32, 0xdf, 0x0d, 0xdc, 0x3f, 0x0d, 0xc4, 0x7b,
            0xba, 0x63, 0x90, 0xb6, 0xc7, 0x3b, 0xb5, 0x0f, 0x9c, 0x31, 0x22, 0xec, 0x84, 0x4a,
            0xd7, 0xc2, 0xb3, 0xe5,
        ];
        let info = [0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9];
        let okm = hkdf_expand(&prk, &info, 42).ok().unwrap();
        let expected = [
            0x3c, 0xb2, 0x5f, 0x25, 0xfa, 0xac, 0xd5, 0x7a, 0x90, 0x43, 0x4f, 0x64, 0xd0, 0x36,
            0x2f, 0x2a, 0x2d, 0x2d, 0x0a, 0x90, 0xcf, 0x1a, 0x5a, 0x4c, 0x5d, 0xb0, 0x2d, 0x56,
            0xec, 0xc4, 0xc5, 0xbf, 0x34, 0x00, 0x72, 0x08, 0xd5, 0xb8, 0x87, 0x18, 0x58, 0x65,
        ];
        assert_eq!(okm, expected);
    }

    #[test]
    fn hkdf_expand_multi_block_and_limits() {
        let prk = [7u8; 32];
        // 65B 输出横跨三个 HMAC 块：T(1) 32B || T(2) 32B || T(3) 1B
        let okm65 = hkdf_expand(&prk, b"enc", 65).ok().unwrap();
        assert_eq!(okm65.len(), 65);
        let okm32 = hkdf_expand(&prk, b"enc", 32).ok().unwrap();
        // 前缀一致性：短输出是长输出的前缀
        assert_eq!(&okm65[..32], okm32.as_slice());
        // 第二块由 counter=2 产生，与第一块不同
        assert_ne!(&okm65[..32], &okm65[32..64]);
        // 与 33B 输出的尾部（T(2) 首字节）一致
        let okm33 = hkdf_expand(&prk, b"enc", 33).ok().unwrap();
        assert_eq!(&okm65[32..33], &okm33[32..33]);
        // info 域分离：enc/mac 输出不同
        let okm_mac = hkdf_expand(&prk, b"mac", 32).ok().unwrap();
        assert_ne!(okm32, okm_mac);
        // 上限：255*32 可，超过报错
        assert!(hkdf_expand(&prk, b"enc", 255 * 32).is_ok());
        assert!(hkdf_expand(&prk, b"enc", 255 * 32 + 1).is_err());
    }

    #[test]
    fn kdf_salt_lowercases_email() {
        assert_eq!(kdf_salt("SomeBody@Example.COM"), "somebody@example.com");
        assert_eq!(kdf_salt("a@B.c"), "a@b.c");
    }

    #[test]
    fn pbkdf2_salt_is_case_insensitive() {
        let upper = derive_master_key(
            "pw",
            "User@Example.com",
            &KdfConfig::Pbkdf2 { iterations: 100 },
        )
        .ok()
        .unwrap();
        let lower = derive_master_key(
            "pw",
            "user@example.com",
            &KdfConfig::Pbkdf2 { iterations: 100 },
        )
        .ok()
        .unwrap();
        assert_eq!(*upper, *lower);
    }

    #[test]
    fn derive_is_deterministic_and_password_sensitive() {
        let config = KdfConfig::Pbkdf2 { iterations: 100 };
        let a = derive_master_key("right", "u@e.com", &config).ok().unwrap();
        let b = derive_master_key("right", "u@e.com", &config).ok().unwrap();
        let c = derive_master_key("wrong", "u@e.com", &config).ok().unwrap();
        assert_eq!(*a, *b);
        assert_ne!(*a, *c);
    }

    #[test]
    fn wire_id_and_from_server_roundtrip() {
        let pbkdf2 = KdfConfig::Pbkdf2 {
            iterations: 600_000,
        };
        assert_eq!(pbkdf2.wire_id(), 0);
        assert_eq!(
            KdfConfig::from_server(0, 600_000, None, None).ok(),
            Some(pbkdf2)
        );
        let argon = KdfConfig::Argon2id {
            memory_kib: 64 * 1024,
            iterations: 3,
            parallelism: 4,
        };
        assert_eq!(argon.wire_id(), 1);
        assert_eq!(
            KdfConfig::from_server(1, 3, Some(64), Some(4)).ok(),
            Some(argon)
        );
        // 缺省内存/并行度按 64MB / 4 补齐
        assert_eq!(KdfConfig::from_server(1, 3, None, None).ok(), Some(argon));
        // 非法输入
        assert!(KdfConfig::from_server(0, 0, None, None).is_err());
        assert!(KdfConfig::from_server(7, 100, None, None).is_err());
    }

    #[test]
    fn derive_and_stretch_consistency() {
        let config = KdfConfig::Pbkdf2 { iterations: 50 };
        let mk = derive_master_key("pw", "u@e.com", &config).ok().unwrap();
        let a = stretch_master_key(&mk).ok().unwrap();
        let b = derive_and_stretch("pw", "u@e.com", &config).ok().unwrap();
        assert_eq!(a.enc_key(), b.enc_key());
        assert_eq!(a.mac_key(), b.mac_key());
    }

    #[test]
    fn argon2_params_validation_surfaces_as_error() {
        // 内存成本低于 Argon2 下限（2 * p 块）应报错而非 panic
        let config = KdfConfig::Argon2id {
            memory_kib: 1,
            iterations: 3,
            parallelism: 4,
        };
        assert!(derive_master_key("pw", "u@e.com", &config).is_err());
    }
}
