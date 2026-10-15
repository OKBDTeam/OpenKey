//! EncString type2：AES-256-CBC + HMAC-SHA256（docs/02 3.4.1 enc 模块）。
//!
//! 线格式（与 Bitwarden 官方客户端一致）：
//!
//! ```text
//! 2.{base64(iv,16B)}|{base64(ciphertext)}|{base64(mac,32B)}
//! ```
//!
//! - 加密：AES-256-CBC + PKCS#7，IV 每次随机 16B；
//! - MAC：HMAC-SHA256(mac_key, iv || ciphertext)，Encrypt-then-MAC，
//!   解密前以恒定时间比较校验（篡改即拒）；
//! - 仅支持 type2（`AesCbc256_HmacSha256`）；type7（COSE）等预留，
//!   解析时以 [`Error::UnsupportedType`] 明确拒绝。

use core::fmt;
use core::str::FromStr;

use aes::cipher::{block_padding::Pkcs7, BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use base64::Engine as _;
use cbc::{Decryptor, Encryptor};
use hmac::{Hmac, KeyInit as _, Mac as _};
use rand::Rng as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::keys::CryptoKey;

const IV_LEN: usize = 16;
const MAC_LEN: usize = 32;
const BLOCK_LEN: usize = 16;

/// Bitwarden EncString（一期仅 type2）。
///
/// 密文与 MAC 属公开数据（无密钥不可读），可直接 `Clone` / `Debug`；
/// 敏感的是明文，由调用方在取得后自行处理（见
/// [`EncString::decrypt_to_string`] 的 `Zeroizing` 返回）。
#[derive(Clone, PartialEq, Eq)]
pub struct EncString {
    iv: [u8; IV_LEN],
    ciphertext: Vec<u8>,
    mac: [u8; MAC_LEN],
}

impl EncString {
    /// 以随机 IV 加密（生产路径）。
    pub fn encrypt(plaintext: &[u8], key: &impl CryptoKey) -> Result<Self> {
        let mut iv = [0u8; IV_LEN];
        rand::rng().fill_bytes(&mut iv);
        Self::encrypt_with_iv(plaintext, key, iv)
    }

    /// 以指定 IV 加密（确定性；供测试与互操作对拍，生产路径请用
    /// [`EncString::encrypt`]）。
    pub fn encrypt_with_iv(
        plaintext: &[u8],
        key: &impl CryptoKey,
        iv: [u8; IV_LEN],
    ) -> Result<Self> {
        let (enc_key, mac_key) = key_arrays(key)?;
        let ciphertext = Encryptor::<aes::Aes256>::new(&enc_key.into(), &iv.into())
            .encrypt_padded_vec::<Pkcs7>(plaintext);
        let mac = compute_mac(&mac_key, &iv, &ciphertext)?;
        Ok(Self {
            iv,
            ciphertext,
            mac,
        })
    }

    /// 加密字符串（UTF-8 字节）。
    pub fn encrypt_string(plaintext: &str, key: &impl CryptoKey) -> Result<Self> {
        Self::encrypt(plaintext.as_bytes(), key)
    }

    /// 校验 MAC 并解密（恒定时间比较；失败返回 [`Error::MacMismatch`]）。
    pub fn decrypt(&self, key: &impl CryptoKey) -> Result<Vec<u8>> {
        self.verify_mac(key)?;
        let (enc_key, _mac_key) = key_arrays(key)?;
        let plaintext = Decryptor::<aes::Aes256>::new(&enc_key.into(), &self.iv.into())
            .decrypt_padded_vec::<Pkcs7>(&self.ciphertext)
            .map_err(|_| Error::Decrypt("密文长度非分组倍数或 PKCS#7 填充非法".to_string()))?;
        Ok(plaintext)
    }

    /// 校验 MAC 并解密为字符串（结果经 `Zeroizing` 持有）。
    pub fn decrypt_to_string(&self, key: &impl CryptoKey) -> Result<Zeroizing<String>> {
        let plaintext = Zeroizing::new(self.decrypt(key)?);
        let text = String::from_utf8(plaintext.to_vec()).map_err(Error::Utf8)?;
        Ok(Zeroizing::new(text))
    }

    /// 仅校验 MAC（不解密）；用于快速甄别密钥/主密码是否正确。
    pub fn verify_mac(&self, key: &impl CryptoKey) -> Result<()> {
        let (_enc_key, mac_key) = key_arrays(key)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&mac_key)
            .map_err(|e| Error::Kdf(format!("HMAC 初始化失败: {e}")))?;
        mac.update(&self.iv);
        mac.update(&self.ciphertext);
        mac.verify_slice(&self.mac)
            .map_err(|_| Error::MacMismatch)?;
        Ok(())
    }

    /// 从线格式解析：`2.{iv}|{ct}|{mac}`。
    pub fn parse(s: &str) -> Result<Self> {
        let (version, payload) = s
            .split_once('.')
            .ok_or_else(|| Error::EncString(format!("缺少类型前缀: {s:?}")))?;
        if version != "2" {
            return Err(Error::UnsupportedType(version.to_string()));
        }
        let mut fields = payload.split('|');
        let iv_b64 = fields
            .next()
            .ok_or_else(|| Error::EncString("缺少 iv 段".to_string()))?;
        let ct_b64 = fields
            .next()
            .ok_or_else(|| Error::EncString("缺少密文段".to_string()))?;
        let mac_b64 = fields
            .next()
            .ok_or_else(|| Error::EncString("缺少 mac 段".to_string()))?;
        if fields.next().is_some() {
            return Err(Error::EncString(
                "段数过多（期望 iv|ct|mac 三段）".to_string(),
            ));
        }
        let iv = decode_exact::<IV_LEN>(iv_b64, "iv")?;
        let mac = decode_exact::<MAC_LEN>(mac_b64, "mac")?;
        let ciphertext = base64::engine::general_purpose::STANDARD.decode(ct_b64)?;
        if ciphertext.is_empty() || ciphertext.len() % BLOCK_LEN != 0 {
            return Err(Error::EncString(format!(
                "密文长度非法: {}B（须为非空且为 {BLOCK_LEN} 的倍数）",
                ciphertext.len()
            )));
        }
        Ok(Self {
            iv,
            ciphertext,
            mac,
        })
    }

    /// IV（16B）。
    pub fn iv(&self) -> &[u8; IV_LEN] {
        &self.iv
    }

    /// 密文（含 PKCS#7 填充，长度为 16 的倍数）。
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    /// MAC（32B，HMAC-SHA256 over iv||ciphertext）。
    pub fn mac(&self) -> &[u8; MAC_LEN] {
        &self.mac
    }
}

impl fmt::Debug for EncString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncString")
            .field("iv", &format_args!("{}B", self.iv.len()))
            .field("ciphertext", &format_args!("{}B", self.ciphertext.len()))
            .field("mac", &format_args!("{}B", self.mac.len()))
            .finish()
    }
}

impl fmt::Display for EncString {
    /// 线格式：`2.{iv}|{ct}|{mac}`（base64 Standard 字母表，带填充）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let engine = base64::engine::general_purpose::STANDARD;
        write!(
            f,
            "2.{}|{}|{}",
            engine.encode(self.iv),
            engine.encode(&self.ciphertext),
            engine.encode(self.mac)
        )
    }
}

impl FromStr for EncString {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

impl Serialize for EncString {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EncString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        EncString::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// 从 [`CryptoKey`] 取出定长 enc/mac 密钥数组。
fn key_arrays(key: &impl CryptoKey) -> Result<([u8; 32], [u8; 32])> {
    let enc: [u8; 32] = key
        .enc_key()
        .try_into()
        .map_err(|_| Error::Kdf("enc 密钥长度非法（期望 32B）".to_string()))?;
    let mac: [u8; 32] = key
        .mac_key()
        .try_into()
        .map_err(|_| Error::Kdf("mac 密钥长度非法（期望 32B）".to_string()))?;
    Ok((enc, mac))
}

/// HMAC-SHA256(mac_key, iv || ciphertext)。
fn compute_mac(mac_key: &[u8; 32], iv: &[u8; IV_LEN], ciphertext: &[u8]) -> Result<[u8; MAC_LEN]> {
    let mut mac = Hmac::<Sha256>::new_from_slice(mac_key)
        .map_err(|e| Error::Kdf(format!("HMAC 初始化失败: {e}")))?;
    mac.update(iv);
    mac.update(ciphertext);
    let out = mac.finalize().into_bytes();
    let mut tag = [0u8; MAC_LEN];
    tag.copy_from_slice(&out);
    Ok(tag)
}

/// base64 解码并要求恰好 N 字节。
fn decode_exact<const N: usize>(s: &str, what: &str) -> Result<[u8; N]> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(s)?;
    bytes.try_into().map_err(|v: Vec<u8>| {
        Error::EncString(format!("{what} 长度非法：期望 {N}B，得到 {}B", v.len()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::UserKey;

    // ---- 测试向量 ----
    //
    // 向量 1（解密）：Miguel Grinberg《How Bitwarden Encrypts and Decrypts
    // Secrets》公开向量，与 Bitwarden 官方客户端语义一致；本仓库已用独立
    // Python 实现（hmac + pycryptodome）交叉验证。Bitwarden 官方仓库未发布
    // 可直接引用的向量文件（2026-10 检索 bitwarden/sdk 404），故采用该公开
    // 向量；另附本仓库生成的确定性加密向量（Python 对拍）覆盖加密路径。

    /// 64B 对称密钥（前 32B enc，后 32B mac）。
    const VEC_KEY: [u8; 64] = [
        0xa3, 0x3f, 0xbc, 0x86, 0x18, 0x7f, 0x9c, 0x7c, 0xe2, 0xf1, 0x10, 0xd4, 0xee, 0x20, 0x42,
        0xde, 0x93, 0x12, 0x67, 0x03, 0x5c, 0x83, 0x9a, 0xc5, 0x53, 0x3c, 0x21, 0x18, 0xc0, 0x0e,
        0x52, 0x70, 0xb5, 0xbc, 0x60, 0xfc, 0xce, 0x75, 0x29, 0x93, 0x51, 0x84, 0x72, 0xaa, 0xd6,
        0xde, 0x1f, 0xc6, 0x59, 0x92, 0x85, 0x3f, 0xf8, 0x6a, 0x95, 0xe9, 0x38, 0x8e, 0xe5, 0xe0,
        0x98, 0xd8, 0x85, 0x9c,
    ];
    /// 向量 1 的 EncString（type2）。
    const VEC_ENC_STRING: &str = "2.IkWFb104bXv7Zwl7eFbsnQ==|SB42jIOvjhV32hSusW/J7WfAnQV8DKIV/CJQB7IDaiz4lQv4lIcXzWp9+IT0ncVQ|S8Tcp2klhcOOzZvoA0C9WRURaWUq+U1F9jbuBskDIz0=";
    /// 向量 1 的明文。
    const VEC_PLAINTEXT: &str = "The quick brown fox jumps over the lazy dog";
    /// 确定性加密向量：固定 IV = 0x00..0x0f，明文 "hello bitwarden"，
    /// 由 Python（pycryptodome + hmac）对拍生成。
    const VEC_DETERMINISTIC: &str =
        "2.AAECAwQFBgcICQoLDA0ODw==|+bwSGjqkqNVmwo+UYmgUcg==|UvE5Vq4nIEtERrsF65D+PvB8If/vzf7eDPnQznmYNYQ=";

    fn vec_key() -> UserKey {
        UserKey::from_bytes(&VEC_KEY)
    }

    #[test]
    fn decrypt_official_vector() {
        let key = vec_key();
        let enc = EncString::parse(VEC_ENC_STRING).ok().unwrap();
        let plaintext = enc.decrypt_to_string(&key).ok().unwrap();
        assert_eq!(*plaintext, VEC_PLAINTEXT);
    }

    #[test]
    fn deterministic_encrypt_matches_reference() {
        let key = vec_key();
        let iv: [u8; 16] = core::array::from_fn(|i| i as u8);
        let enc = EncString::encrypt_with_iv(b"hello bitwarden", &key, iv)
            .ok()
            .unwrap();
        assert_eq!(enc.to_string(), VEC_DETERMINISTIC);
        // 且可解回原文
        let pt = enc.decrypt(&key).ok().unwrap();
        assert_eq!(pt, b"hello bitwarden");
    }

    #[test]
    fn roundtrip_various_lengths() {
        let key = vec_key();
        for len in [0usize, 1, 15, 16, 17, 31, 32, 43, 64, 100] {
            let plaintext: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let enc = EncString::encrypt(&plaintext, &key).ok().unwrap();
            assert_eq!(enc.ciphertext().len() % BLOCK_LEN, 0);
            assert!(enc.ciphertext().len() >= plaintext.len());
            let back = enc.decrypt(&key).ok().unwrap();
            assert_eq!(back, plaintext, "len={len}");
        }
    }

    #[test]
    fn roundtrip_string() {
        let key = vec_key();
        let enc = EncString::encrypt_string("密码 #① 🔐", &key).ok().unwrap();
        let back = enc.decrypt_to_string(&key).ok().unwrap();
        assert_eq!(*back, "密码 #① 🔐");
    }

    #[test]
    fn random_iv_makes_ciphertexts_differ() {
        let key = vec_key();
        let a = EncString::encrypt_string("same", &key).ok().unwrap();
        let b = EncString::encrypt_string("same", &key).ok().unwrap();
        assert_ne!(a.iv(), b.iv());
        assert_ne!(a.ciphertext(), b.ciphertext());
        assert_ne!(a.to_string(), b.to_string());
    }

    #[test]
    fn mac_tamper_detected() {
        let key = vec_key();
        let enc = EncString::encrypt_string("tamper me", &key).ok().unwrap();
        let mut tampered = enc.clone();
        tampered.ciphertext[0] ^= 0x01;
        assert!(matches!(
            tampered.decrypt(&key).err(),
            Some(Error::MacMismatch)
        ));
        let mut bad_mac = enc.clone();
        bad_mac.mac[0] ^= 0x01;
        assert!(matches!(
            bad_mac.decrypt(&key).err(),
            Some(Error::MacMismatch)
        ));
        let mut bad_iv = enc.clone();
        bad_iv.iv[0] ^= 0x01;
        assert!(matches!(
            bad_iv.decrypt(&key).err(),
            Some(Error::MacMismatch)
        ));
    }

    #[test]
    fn wrong_key_rejected() {
        let key = vec_key();
        let other = UserKey::random();
        let enc = EncString::encrypt_string("secret", &key).ok().unwrap();
        assert!(enc.verify_mac(&other).is_err());
        assert!(matches!(
            enc.decrypt(&other).err(),
            Some(Error::MacMismatch)
        ));
    }

    #[test]
    fn decrypt_to_string_rejects_non_utf8() {
        let key = vec_key();
        let raw = [0xffu8, 0xfe, 0x00, 0x80];
        let enc = EncString::encrypt(&raw, &key).ok().unwrap();
        assert!(matches!(
            enc.decrypt_to_string(&key).err(),
            Some(Error::Utf8(_))
        ));
        // 字节路径不受影响
        assert_eq!(enc.decrypt(&key).ok().unwrap(), raw);
    }

    #[test]
    fn parse_roundtrip_and_accessors() {
        let enc = EncString::parse(VEC_ENC_STRING).ok().unwrap();
        assert_eq!(enc.iv().len(), 16);
        assert_eq!(enc.mac().len(), 32);
        assert_eq!(enc.ciphertext().len() % 16, 0);
        assert_eq!(enc.to_string(), VEC_ENC_STRING);
        let reparsed = EncString::parse(&enc.to_string()).ok().unwrap();
        assert_eq!(reparsed, enc);
    }

    #[test]
    fn parse_rejects_malformed() {
        let key = vec_key();
        let good = EncString::encrypt_string("x", &key).ok().unwrap();
        let wire = good.to_string();
        // 缺类型前缀
        assert!(EncString::parse(&wire[2..]).is_err());
        // 不支持的类型
        assert!(matches!(
            EncString::parse("0.AAAA|BBBB|CCCC").err(),
            Some(Error::UnsupportedType(t)) if t == "0"
        ));
        assert!(matches!(
            EncString::parse("7.AAAA|BBBB|CCCC").err(),
            Some(Error::UnsupportedType(t)) if t == "7"
        ));
        // 段数不足 / 过多
        assert!(EncString::parse("2.onlyone").is_err());
        assert!(EncString::parse("2.a|b").is_err());
        assert!(EncString::parse("2.a|b|c|d").is_err());
        // 非法 base64
        assert!(matches!(
            EncString::parse("2.!!!|@@@|###").err(),
            Some(Error::Base64(_))
        ));
        // 长度非法：iv 非 16B / mac 非 32B / 密文非 16 倍数
        assert!(matches!(
            EncString::parse("2.AAAA|QUJD|QUJD").err(),
            Some(Error::EncString(_))
        ));
        let mut short_mac = String::from("2.");
        short_mac.push_str(&base64::engine::general_purpose::STANDARD.encode([0u8; 16]));
        short_mac.push('|');
        short_mac.push_str(&base64::engine::general_purpose::STANDARD.encode([0u8; 16]));
        short_mac.push('|');
        short_mac.push_str(&base64::engine::general_purpose::STANDARD.encode([0u8; 31]));
        assert!(EncString::parse(&short_mac).is_err());
        let mut bad_ct = String::from("2.");
        bad_ct.push_str(&base64::engine::general_purpose::STANDARD.encode([0u8; 16]));
        bad_ct.push('|');
        bad_ct.push_str(&base64::engine::general_purpose::STANDARD.encode([0u8; 15]));
        bad_ct.push('|');
        bad_ct.push_str(&base64::engine::general_purpose::STANDARD.encode([0u8; 32]));
        assert!(EncString::parse(&bad_ct).is_err());
        // 密文为空（base64 空串）
        let mut empty_ct = String::from("2.");
        empty_ct.push_str(&base64::engine::general_purpose::STANDARD.encode([0u8; 16]));
        empty_ct.push('|');
        empty_ct.push('|');
        empty_ct.push_str(&base64::engine::general_purpose::STANDARD.encode([0u8; 32]));
        assert!(EncString::parse(&empty_ct).is_err());
    }

    #[test]
    fn serde_json_roundtrip() {
        let key = vec_key();
        let enc = EncString::encrypt_string("serde", &key).ok().unwrap();
        let json = serde_json::to_string(&enc).ok().unwrap();
        assert_eq!(json, format!("\"{}\"", enc));
        let back: EncString = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(back, enc);
        // 反序列化非法字符串报错
        let bad = serde_json::from_str::<EncString>("\"2.short\"");
        assert!(bad.is_err());
    }

    #[test]
    fn from_str_matches_parse() {
        use std::str::FromStr as _;
        let a = EncString::from_str(VEC_ENC_STRING).ok().unwrap();
        let b = EncString::parse(VEC_ENC_STRING).ok().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn debug_redacts_content() {
        let key = vec_key();
        let enc = EncString::encrypt_string("debug test", &key).ok().unwrap();
        let rendered = format!("{enc:?}");
        assert!(rendered.starts_with("EncString {"), "{rendered}");
        assert!(rendered.contains("16B"), "{rendered}");
        assert!(rendered.contains("32B"), "{rendered}");
        // 不含 base64 密文正文
        let engine = base64::engine::general_purpose::STANDARD;
        assert!(!rendered.contains(&engine.encode(enc.ciphertext())));
    }
}
