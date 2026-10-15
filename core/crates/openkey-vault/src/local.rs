//! 本地 vault：增删查 + 锁定状态机（docs/02 3.4.1 local/lock 模块）。
//!
//! - 条目字段以 user key 逐字段 EncString 加密存储（与 Bitwarden 一致）；
//! - user key 本身以 stretched master key 包裹（`wrapped_user_key`），
//!   仅该包裹件与密文条目可持久化，密钥材料不出内存；
//! - 锁定状态机见 [`crate::lock`]：Unlocked / SoftLocked / Locked；
//! - 持久化路径：`wrapped_user_key()` + `encrypted_item()/insert_encrypted()`
//!   由应用层（openkey-data）落库，本 crate 不直接做 IO。

use std::collections::BTreeMap;
use std::fmt;

use zeroize::Zeroizing;

use crate::enc::EncString;
use crate::error::{Error, Result};
use crate::kdf::{derive_and_stretch, KdfConfig};
use crate::keys::UserKey;
use crate::lock::{lock_next, LockEvent, LockState};

/// 明文条目（一期：login 类型字段集）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultItem {
    /// 条目名称（明文存储前必填）。
    pub name: String,
    /// 用户名。
    pub username: Option<String>,
    /// 密码。
    pub password: Option<String>,
    /// 登录 URI。
    pub uri: Option<String>,
    /// 备注。
    pub notes: Option<String>,
}

/// 密文存储形态（各字段独立 EncString；可安全持久化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedItem {
    /// 加密的名称。
    pub name: EncString,
    /// 加密的用户名。
    pub username: Option<EncString>,
    /// 加密的密码。
    pub password: Option<EncString>,
    /// 加密的 URI。
    pub uri: Option<EncString>,
    /// 加密的备注。
    pub notes: Option<EncString>,
}

impl EncryptedItem {
    /// 以 user key 加密明文条目。
    fn encrypt(item: &VaultItem, key: &UserKey) -> Result<Self> {
        Ok(Self {
            name: EncString::encrypt_string(&item.name, key)?,
            username: item
                .username
                .as_ref()
                .map(|v| EncString::encrypt_string(v, key))
                .transpose()?,
            password: item
                .password
                .as_ref()
                .map(|v| EncString::encrypt_string(v, key))
                .transpose()?,
            uri: item
                .uri
                .as_ref()
                .map(|v| EncString::encrypt_string(v, key))
                .transpose()?,
            notes: item
                .notes
                .as_ref()
                .map(|v| EncString::encrypt_string(v, key))
                .transpose()?,
        })
    }

    /// 以 user key 解密为明文条目。
    fn decrypt(&self, key: &UserKey) -> Result<VaultItem> {
        Ok(VaultItem {
            name: self.name.decrypt_to_string(key)?.to_string(),
            username: self
                .username
                .as_ref()
                .map(|e| e.decrypt_to_string(key))
                .transpose()?
                .map(|s| s.to_string()),
            password: self
                .password
                .as_ref()
                .map(|e| e.decrypt_to_string(key))
                .transpose()?
                .map(|s| s.to_string()),
            uri: self
                .uri
                .as_ref()
                .map(|e| e.decrypt_to_string(key))
                .transpose()?
                .map(|s| s.to_string()),
            notes: self
                .notes
                .as_ref()
                .map(|e| e.decrypt_to_string(key))
                .transpose()?
                .map(|s| s.to_string()),
        })
    }
}

/// 本地 vault。
///
/// 锁定语义：
/// - `Unlocked`：增删查可用；
/// - `SoftLocked`：读写全部拒绝（密钥仍在内存，等待快速认证 Resume）；
/// - `Locked`：密钥已清零，必须 [`LocalVault::unlock`] 重解。
pub struct LocalVault {
    state: LockState,
    key: Option<UserKey>,
    wrapped_user_key: EncString,
    kdf: KdfConfig,
    items: BTreeMap<String, EncryptedItem>,
}

impl LocalVault {
    /// 新建 vault：生成随机 user key，以 stretched master key 包裹后存入。
    pub fn create(password: &str, email: &str, kdf: &KdfConfig) -> Result<Self> {
        let user_key = UserKey::random();
        let stretched = derive_and_stretch(password, email, kdf)?;
        let wrapped = EncString::encrypt(user_key.as_bytes(), &stretched)?;
        Ok(Self {
            state: LockState::Unlocked,
            key: Some(user_key),
            wrapped_user_key: wrapped,
            kdf: *kdf,
            items: BTreeMap::new(),
        })
    }

    /// 以持久化的包裹件构造处于 Locked 状态的 vault（应用层恢复路径）。
    pub fn from_locked(wrapped_user_key: EncString, kdf: KdfConfig) -> Self {
        Self {
            state: LockState::Locked,
            key: None,
            wrapped_user_key,
            kdf,
            items: BTreeMap::new(),
        }
    }

    /// 当前锁定状态。
    pub fn state(&self) -> LockState {
        self.state
    }

    /// user key 包裹件（可持久化；本身是 EncString，无密钥不可读）。
    pub fn wrapped_user_key(&self) -> &EncString {
        &self.wrapped_user_key
    }

    /// KDF 配置（create 时设定；换密码/改 KDF 待后续版本）。
    pub fn kdf(&self) -> &KdfConfig {
        &self.kdf
    }

    /// 解锁：重新派生 master key 并解包 user key。
    ///
    /// 幂等：已 `Unlocked` 时直接返回。主密码错误或数据损坏表现为
    /// [`Error::MacMismatch`]（MAC 校验失败），状态保持 Locked。
    pub fn unlock(&mut self, password: &str, email: &str) -> Result<()> {
        if self.state.is_unlocked() {
            return Ok(());
        }
        let stretched = derive_and_stretch(password, email, &self.kdf)?;
        let wrapped_bytes = Zeroizing::new(self.wrapped_user_key.decrypt(&stretched)?);
        let user_key = UserKey::from_slice(&wrapped_bytes)?;
        self.key = Some(user_key);
        self.state = LockState::Unlocked;
        Ok(())
    }

    /// 硬锁定：清除内存中的密钥材料（[`UserKey`] 析构即零化）。
    pub fn lock(&mut self) -> Result<()> {
        self.state = lock_next(self.state, LockEvent::HardLock)?;
        self.key = None;
        Ok(())
    }

    /// 软锁定：密钥保留在内存，但拒绝一切读写。
    pub fn soft_lock(&mut self) -> Result<()> {
        self.state = lock_next(self.state, LockEvent::SoftLock)?;
        Ok(())
    }

    /// 从软锁定恢复（快速认证通过；无需主密码）。
    pub fn resume(&mut self) -> Result<()> {
        self.state = lock_next(self.state, LockEvent::Resume)?;
        Ok(())
    }

    /// 新增或覆盖条目（幂等 upsert；需 Unlocked）。
    pub fn add_item(&mut self, id: &str, item: &VaultItem) -> Result<()> {
        let stored = {
            let key = self.require_unlocked()?;
            EncryptedItem::encrypt(item, key)?
        };
        self.items.insert(id.to_string(), stored);
        Ok(())
    }

    /// 查询并解密条目（需 Unlocked）；不存在返回 `Ok(None)`。
    pub fn get_item(&self, id: &str) -> Result<Option<VaultItem>> {
        let key = self.require_unlocked()?;
        match self.items.get(id) {
            Some(stored) => Ok(Some(stored.decrypt(key)?)),
            None => Ok(None),
        }
    }

    /// 删除条目（需 Unlocked）；不存在返回 [`Error::NotFound`]。
    pub fn remove_item(&mut self, id: &str) -> Result<()> {
        self.require_unlocked()?;
        self.items
            .remove(id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        Ok(())
    }

    /// 条目 ID 列表（需 Unlocked；ID 本身非密文，统一门禁避免锁定态外泄结构）。
    pub fn ids(&self) -> Result<Vec<&str>> {
        self.require_unlocked()?;
        Ok(self.items.keys().map(String::as_str).collect())
    }

    /// 条目数量（需 Unlocked）。
    pub fn len(&self) -> Result<usize> {
        self.require_unlocked()?;
        Ok(self.items.len())
    }

    /// 是否为空（需 Unlocked）。
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// 密文条目访问（持久化/备份路径；不触碰密钥，任何状态可用）。
    pub fn encrypted_item(&self, id: &str) -> Option<&EncryptedItem> {
        self.items.get(id)
    }

    /// 写入密文条目（从持久化恢复；不触碰密钥，任何状态可用）。
    pub fn insert_encrypted(&mut self, id: String, item: EncryptedItem) {
        self.items.insert(id, item);
    }

    /// 移除密文条目并返回（持久化管理路径；任何状态可用）。
    pub fn remove_encrypted(&mut self, id: &str) -> Option<EncryptedItem> {
        self.items.remove(id)
    }

    /// 取内存中的 user key；非 Unlocked 状态一律拒绝（SoftLocked 同样拒绝）。
    fn require_unlocked(&self) -> Result<&UserKey> {
        match (self.state, &self.key) {
            (LockState::Unlocked, Some(key)) => Ok(key),
            (state, _) => Err(Error::Locked { state }),
        }
    }
}

impl fmt::Debug for LocalVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalVault")
            .field("state", &self.state)
            .field("items", &self.items.len())
            .field("key_in_memory", &self.key.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdf::PBKDF2_DEFAULT_ITERATIONS;

    /// 测试用低迭代 KDF（单测速度优先；语义与生产一致）。
    fn test_kdf() -> KdfConfig {
        KdfConfig::Pbkdf2 { iterations: 100 }
    }

    const EMAIL: &str = "user@example.com";
    const PASSWORD: &str = "correct horse battery staple";

    fn sample_item() -> VaultItem {
        VaultItem {
            name: "GitHub".to_string(),
            username: Some("octocat".to_string()),
            password: Some("hunter2-but-longer".to_string()),
            uri: Some("https://github.com".to_string()),
            notes: None,
        }
    }

    #[test]
    fn create_add_get_remove_roundtrip() {
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        assert_eq!(vault.state(), LockState::Unlocked);
        assert_eq!(vault.len().ok(), Some(0));
        assert!(vault.is_empty().ok() == Some(true));

        vault.add_item("item-1", &sample_item()).ok().unwrap();
        assert_eq!(vault.len().ok(), Some(1));
        assert_eq!(vault.ids().ok().unwrap(), vec!["item-1"]);

        let got = vault.get_item("item-1").ok().unwrap();
        assert_eq!(got.as_ref(), Some(&sample_item()));

        vault.remove_item("item-1").ok().unwrap();
        assert_eq!(vault.len().ok(), Some(0));
        assert_eq!(vault.get_item("item-1").ok(), Some(None));
    }

    #[test]
    fn add_item_upserts_same_id() {
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        vault.add_item("i", &sample_item()).ok().unwrap();
        let updated = VaultItem {
            name: "GitLab".to_string(),
            ..sample_item()
        };
        vault.add_item("i", &updated).ok().unwrap();
        assert_eq!(vault.len().ok(), Some(1));
        let got = vault.get_item("i").ok().unwrap().unwrap_or_default();
        assert_eq!(got.name, "GitLab");
    }

    #[test]
    fn remove_missing_item_is_not_found() {
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        assert!(matches!(
            vault.remove_item("nope").err(),
            Some(Error::NotFound(id)) if id == "nope"
        ));
    }

    #[test]
    fn lock_blocks_ops_and_unlock_restores() {
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        vault.add_item("i", &sample_item()).ok().unwrap();

        vault.lock().ok().unwrap();
        assert_eq!(vault.state(), LockState::Locked);
        assert!(matches!(
            vault.get_item("i").err(),
            Some(Error::Locked {
                state: LockState::Locked
            })
        ));
        assert!(vault.add_item("j", &sample_item()).is_err());
        assert!(vault.remove_item("i").is_err());
        assert!(vault.ids().is_err());

        // 正确主密码解锁恢复
        vault.unlock(PASSWORD, EMAIL).ok().unwrap();
        assert_eq!(vault.state(), LockState::Unlocked);
        let got = vault.get_item("i").ok().unwrap();
        assert_eq!(got.as_ref(), Some(&sample_item()));

        // 再次锁定 + 错误主密码：MacMismatch，状态保持 Locked
        vault.lock().ok().unwrap();
        assert!(matches!(
            vault.unlock("wrong password", EMAIL).err(),
            Some(Error::MacMismatch)
        ));
        assert_eq!(vault.state(), LockState::Locked);
    }

    #[test]
    fn unlock_is_idempotent_when_unlocked() {
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        vault.unlock(PASSWORD, EMAIL).ok().unwrap();
        assert_eq!(vault.state(), LockState::Unlocked);
        // 已解锁时即便密码不同也直接 Ok（幂等，不覆盖内存密钥）
        vault.unlock("anything", EMAIL).ok().unwrap();
        assert_eq!(vault.state(), LockState::Unlocked);
    }

    #[test]
    fn soft_lock_keeps_keys_resume_restores() {
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        vault.add_item("i", &sample_item()).ok().unwrap();

        vault.soft_lock().ok().unwrap();
        assert_eq!(vault.state(), LockState::SoftLocked);
        assert!(matches!(
            vault.get_item("i").err(),
            Some(Error::Locked {
                state: LockState::SoftLocked
            })
        ));
        // 软锁定下硬锁定仍允许（密钥清除）
        vault.lock().ok().unwrap();
        assert_eq!(vault.state(), LockState::Locked);

        vault.unlock(PASSWORD, EMAIL).ok().unwrap();
        vault.soft_lock().ok().unwrap();
        vault.resume().ok().unwrap();
        assert_eq!(vault.state(), LockState::Unlocked);
        let got = vault.get_item("i").ok().unwrap();
        assert_eq!(got.as_ref(), Some(&sample_item()));
    }

    #[test]
    fn persistence_path_survives_reconstruction() {
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        vault.add_item("i", &sample_item()).ok().unwrap();
        let wrapped = vault.wrapped_user_key().clone();
        let encrypted = vault.encrypted_item("i").cloned();

        // 模拟应用层：仅凭包裹件 + 密文条目重建
        let mut restored = LocalVault::from_locked(wrapped, test_kdf());
        assert_eq!(restored.state(), LockState::Locked);
        restored.insert_encrypted("i".to_string(), encrypted.unwrap());
        assert!(matches!(
            restored.get_item("i").err(),
            Some(Error::Locked {
                state: LockState::Locked
            })
        ));

        restored.unlock(PASSWORD, EMAIL).ok().unwrap();
        let got = restored.get_item("i").ok().unwrap();
        assert_eq!(got.as_ref(), Some(&sample_item()));
    }

    #[test]
    fn wrong_password_cannot_read_items() {
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        vault.add_item("i", &sample_item()).ok().unwrap();
        vault.lock().ok().unwrap();
        // 错误密码无法解锁，密文条目也不可读
        assert!(vault.unlock("nope", EMAIL).is_err());
        assert!(vault.get_item("i").is_err());
        // 但密文访问（持久化路径）仍可用
        assert!(vault.encrypted_item("i").is_some());
    }

    #[test]
    fn create_with_argon2_defaults() {
        // Argon2id 默认参数（m=64MiB）下完整链路
        let mut vault = LocalVault::create(PASSWORD, EMAIL, &KdfConfig::argon2_default())
            .ok()
            .unwrap();
        vault.add_item("i", &sample_item()).ok().unwrap();
        vault.lock().ok().unwrap();
        vault.unlock(PASSWORD, EMAIL).ok().unwrap();
        let got = vault.get_item("i").ok().unwrap();
        assert_eq!(got.as_ref(), Some(&sample_item()));
    }

    #[test]
    fn debug_does_not_leak_key_material() {
        let vault = LocalVault::create(PASSWORD, EMAIL, &test_kdf())
            .ok()
            .unwrap();
        let rendered = format!("{vault:?}");
        assert!(rendered.contains("Unlocked"), "{rendered}");
        assert!(rendered.contains("key_in_memory: true"), "{rendered}");
    }

    #[test]
    fn production_default_iterations_is_600k() {
        assert_eq!(PBKDF2_DEFAULT_ITERATIONS, 600_000);
        assert_eq!(KdfConfig::default(), KdfConfig::pbkdf2_default());
    }
}
