//! 密钥层级：64B 对称密钥对（enc + mac）与 stretched master key（docs/02 3.4.2 / 6.2）。
//!
//! ```text
//! master key(32B) ──HKDF-SHA256(expand)──▶ stretched master key(64B = enc||mac)
//! user key(64B = enc||mac) ──包裹于 stretched master key──▶ 服务器存放
//! ```
//!
//! 两类密钥结构相同（均为 32B AES-256 加密密钥 || 32B HMAC-SHA256 MAC 密钥），
//! 但语义不同，故以不同类型区分；均实现 [`CryptoKey`] 供 [`EncString`](crate::enc::EncString)
//! 加解密使用，并在析构时零化清零。

use core::fmt;

use rand::Rng as _;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Error, Result};

/// 密钥材料长度：32B 加密密钥 + 32B MAC 密钥。
pub const KEY_LEN: usize = 64;
const ENC_LEN: usize = 32;
const MAC_LEN: usize = 32;

/// 可执行 AES-256-CBC + HMAC-SHA256 的对称密钥对。
pub trait CryptoKey {
    /// AES-256 加密密钥（32B）。
    fn enc_key(&self) -> &[u8];
    /// HMAC-SHA256 MAC 密钥（32B）。
    fn mac_key(&self) -> &[u8];
}

/// 拼接 enc/mac 两段为 64B 密钥材料（带长度校验）。
fn concat_keys(enc: &[u8], mac: &[u8]) -> Result<[u8; KEY_LEN]> {
    if enc.len() != ENC_LEN || mac.len() != MAC_LEN {
        return Err(Error::Kdf(format!(
            "密钥长度非法：期望 enc {ENC_LEN}B + mac {MAC_LEN}B，得到 enc {}B + mac {}B",
            enc.len(),
            mac.len()
        )));
    }
    let mut out = [0u8; KEY_LEN];
    out[..ENC_LEN].copy_from_slice(enc);
    out[ENC_LEN..].copy_from_slice(mac);
    Ok(out)
}

/// 用户的对称密钥（Bitwarden user key / symmetric key）。
///
/// 服务器上以 stretched master key 加密（wrapped）存放；内存中由
/// [`ZeroizeOnDrop`] 保证析构时清零。
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct UserKey {
    key: [u8; KEY_LEN],
}

impl UserKey {
    /// 生成随机 user key（CSPRNG）。
    pub fn random() -> Self {
        let mut key = [0u8; KEY_LEN];
        rand::rng().fill_bytes(&mut key);
        Self { key }
    }

    /// 从 64B 密钥材料构造。
    pub fn from_bytes(bytes: &[u8; KEY_LEN]) -> Self {
        Self { key: *bytes }
    }

    /// 从切片构造（长度必须为 64B）。
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; KEY_LEN] = bytes.try_into().map_err(|_| {
            Error::Kdf(format!(
                "user key 长度非法：期望 {KEY_LEN}B，得到 {}B",
                bytes.len()
            ))
        })?;
        Ok(Self { key: bytes })
    }

    /// 从 enc/mac 两段构造。
    pub fn from_keys(enc: &[u8], mac: &[u8]) -> Result<Self> {
        Ok(Self {
            key: concat_keys(enc, mac)?,
        })
    }

    /// 64B 密钥材料（前 32B 为 enc，后 32B 为 mac）。
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.key
    }
}

impl CryptoKey for UserKey {
    fn enc_key(&self) -> &[u8] {
        &self.key[..ENC_LEN]
    }

    fn mac_key(&self) -> &[u8] {
        &self.key[ENC_LEN..]
    }
}

impl fmt::Debug for UserKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UserKey { .. }")
    }
}

/// 恒定时间比较（密钥内容不应经由普通 `==` 泄露时序）。
impl PartialEq for UserKey {
    fn eq(&self, other: &Self) -> bool {
        let mut acc = 0u8;
        for (a, b) in self.key.iter().zip(other.key.iter()) {
            acc |= a ^ b;
        }
        acc == 0
    }
}

impl Eq for UserKey {}

/// stretched master key：master key 经 HKDF-SHA256(expand) 派生的 enc/mac 子密钥对，
/// 用于包裹/解包 user key（docs/02 3.4.2）。
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct StretchedMasterKey {
    key: [u8; KEY_LEN],
}

impl StretchedMasterKey {
    /// 从 enc/mac 两段构造。
    pub fn from_keys(enc: &[u8], mac: &[u8]) -> Result<Self> {
        Ok(Self {
            key: concat_keys(enc, mac)?,
        })
    }
}

impl CryptoKey for StretchedMasterKey {
    fn enc_key(&self) -> &[u8] {
        &self.key[..ENC_LEN]
    }

    fn mac_key(&self) -> &[u8] {
        &self.key[ENC_LEN..]
    }
}

impl fmt::Debug for StretchedMasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StretchedMasterKey { .. }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ([u8; 32], [u8; 32]) {
        let enc = [0xA3; 32];
        let mac = [0x5B; 32];
        (enc, mac)
    }

    #[test]
    fn user_key_accessors_split_enc_mac() {
        let (enc, mac) = sample();
        let key = UserKey::from_keys(&enc, &mac).ok().unwrap();
        assert_eq!(key.enc_key(), &enc);
        assert_eq!(key.mac_key(), &mac);
        let bytes = key.as_bytes();
        assert_eq!(&bytes[..32], &enc);
        assert_eq!(&bytes[32..], &mac);
    }

    #[test]
    fn user_key_from_slice_and_from_bytes_roundtrip() {
        let key = UserKey::random();
        let a = UserKey::from_slice(key.as_bytes()).ok().unwrap();
        let b = UserKey::from_bytes(key.as_bytes());
        assert_eq!(a, key);
        assert_eq!(b, key);
    }

    #[test]
    fn wrong_length_keys_rejected() {
        assert!(UserKey::from_keys(&[0u8; 31], &[0u8; 32]).is_err());
        assert!(UserKey::from_keys(&[0u8; 32], &[0u8; 33]).is_err());
        assert!(UserKey::from_slice(&[0u8; 63]).is_err());
        assert!(StretchedMasterKey::from_keys(&[0u8; 32], &[0u8; 31]).is_err());
    }

    #[test]
    fn random_keys_are_distinct() {
        let a = UserKey::random();
        let b = UserKey::random();
        assert_ne!(a, b);
    }

    #[test]
    fn equality_is_by_content() {
        let (enc, mac) = sample();
        let a = UserKey::from_keys(&enc, &mac).ok().unwrap();
        let b = UserKey::from_keys(&enc, &mac).ok().unwrap();
        assert_eq!(a, b);
        let mut enc2 = enc;
        enc2[0] ^= 1;
        let c = UserKey::from_keys(&enc2, &mac).ok().unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn debug_does_not_leak_key_material() {
        let key = UserKey::random();
        let rendered = format!("{key:?}");
        assert_eq!(rendered, "UserKey { .. }");
        let stretched = StretchedMasterKey::from_keys(&[1u8; 32], &[2u8; 32])
            .ok()
            .unwrap();
        assert_eq!(format!("{stretched:?}"), "StretchedMasterKey { .. }");
    }

    #[test]
    fn stretched_master_key_implements_crypto_key() {
        let stretched = StretchedMasterKey::from_keys(&[1u8; 32], &[2u8; 32])
            .ok()
            .unwrap();
        assert_eq!(stretched.enc_key(), &[1u8; 32]);
        assert_eq!(stretched.mac_key(), &[2u8; 32]);
    }
}
