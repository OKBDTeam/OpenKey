//! Bitwarden REST 抽象（docs/02 3.4.1 api 模块）。
//!
//! 一期只定义协议语义：identity token（password grant + refresh）、
//! prelogin（KDF 参数）、sync 全量拉取、cipher CRUD（login 类型读写）。
//! HTTP 传输层（reqwest 适配器）待 Phase 3 落地，实现方需自行处理
//! TLS、UA（`OpenKey/{ver}`）、重试与限频。
//!
//! identity 与 api 基址分离配置（docs/02 3.4.3：用户自指 URL 允许任意
//! base URL；Vaultwarden 自建通常同域 `/identity` + `/api` 两个路径）。

use core::fmt;

use zeroize::Zeroizing;

use crate::error::Result;
use crate::kdf::KdfConfig;

/// identity / api 基址对。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitwardenEndpoints {
    identity_base: String,
    api_base: String,
}

impl BitwardenEndpoints {
    /// 自定义 identity / api 基址（各自为根 URL，不带尾斜杠亦可）。
    pub fn new(identity_base: impl Into<String>, api_base: impl Into<String>) -> Self {
        Self {
            identity_base: trim_slash(identity_base.into()),
            api_base: trim_slash(api_base.into()),
        }
    }

    /// bitwarden.com 官方云。
    pub fn cloud() -> Self {
        Self::new(
            "https://identity.bitwarden.com",
            "https://api.bitwarden.com",
        )
    }

    /// 自建服务器（Vaultwarden 等）：同一 base URL 下 `/identity` + `/api`。
    pub fn selfhosted(base_url: &str) -> Self {
        let base = trim_slash(base_url.to_string());
        Self::new(format!("{base}/identity"), format!("{base}/api"))
    }

    /// identity 基址。
    pub fn identity_base(&self) -> &str {
        &self.identity_base
    }

    /// api 基址。
    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// `POST {identity}/accounts/prelogin`：获取服务器 KDF 参数。
    pub fn prelogin_url(&self) -> String {
        format!("{}/accounts/prelogin", self.identity_base)
    }

    /// `POST {identity}/connect/token`：password grant / refresh grant。
    pub fn token_url(&self) -> String {
        format!("{}/connect/token", self.identity_base)
    }

    /// `GET {api}/sync`：全量拉取（可带 `?excludeDomains=true`）。
    pub fn sync_url(&self) -> String {
        format!("{}/sync", self.api_base)
    }

    /// `POST {api}/ciphers`：新建条目。
    pub fn ciphers_url(&self) -> String {
        format!("{}/ciphers", self.api_base)
    }

    /// `PUT/DELETE {api}/ciphers/{id}`：更新/删除条目。
    pub fn cipher_url(&self, id: &str) -> String {
        format!("{}/ciphers/{}", self.api_base, id)
    }
}

fn trim_slash(mut s: String) -> String {
    while s.ends_with('/') {
        s.pop();
    }
    s
}

/// 认证令牌（敏感字段 `Zeroizing` 持有；`Debug` 脱敏）。
#[derive(Clone)]
pub struct AuthTokens {
    /// access token（Bearer）。
    pub access_token: Zeroizing<String>,
    /// refresh token（password grant 带 `offline_access` 时下发）。
    pub refresh_token: Option<Zeroizing<String>>,
    /// token 类型（通常 `Bearer`）。
    pub token_type: String,
    /// 有效期（秒）。
    pub expires_in: u64,
}

impl fmt::Debug for AuthTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthTokens")
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// Bitwarden REST 一期能力面。
///
/// 实现方持有自己的 [`BitwardenEndpoints`] 与 HTTP 客户端；
/// 本 trait 固定调用语义与错误类型，便于 Phase 3 接入 reqwest 适配器，
/// 也便于测试以内存 mock 实现对拍协议层。
pub trait BitwardenRest {
    /// 实现方使用的端点配置。
    fn endpoints(&self) -> &BitwardenEndpoints;

    /// prelogin：email → 服务器 KDF 参数。
    fn prelogin(&self, email: &str) -> Result<KdfConfig>;

    /// password grant 登录。
    ///
    /// `password_hash_b64` 为 [`crate::kdf::master_password_hash_b64`]
    /// 的输出（PBKDF2(pw, master_key, 1) 的 base64），仅发服务器。
    fn login(&self, email: &str, password_hash_b64: &str) -> Result<AuthTokens>;

    /// refresh grant 续期。
    fn refresh(&self, refresh_token: &str) -> Result<AuthTokens>;

    /// sync 全量拉取，返回原始 JSON 字节（解密由调用方负责）。
    fn sync(&self, access_token: &str) -> Result<Vec<u8>>;

    /// 新建 cipher，请求体为 cipher JSON 字节，返回服务端响应字节。
    fn create_cipher(&self, access_token: &str, cipher_json: &[u8]) -> Result<Vec<u8>>;

    /// 更新 cipher（`PUT {api}/ciphers/{id}`）。
    fn update_cipher(
        &self,
        access_token: &str,
        cipher_id: &str,
        cipher_json: &[u8],
    ) -> Result<Vec<u8>>;

    /// 删除 cipher（`DELETE {api}/ciphers/{id}`）。
    fn delete_cipher(&self, access_token: &str, cipher_id: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_endpoints_build_urls() {
        let ep = BitwardenEndpoints::cloud();
        assert_eq!(ep.identity_base(), "https://identity.bitwarden.com");
        assert_eq!(ep.api_base(), "https://api.bitwarden.com");
        assert_eq!(
            ep.prelogin_url(),
            "https://identity.bitwarden.com/accounts/prelogin"
        );
        assert_eq!(
            ep.token_url(),
            "https://identity.bitwarden.com/connect/token"
        );
        assert_eq!(ep.sync_url(), "https://api.bitwarden.com/sync");
        assert_eq!(ep.ciphers_url(), "https://api.bitwarden.com/ciphers");
        assert_eq!(
            ep.cipher_url("abc-123"),
            "https://api.bitwarden.com/ciphers/abc-123"
        );
    }

    #[test]
    fn selfhosted_endpoints_split_identity_and_api() {
        let ep = BitwardenEndpoints::selfhosted("https://vault.example.com/");
        assert_eq!(ep.identity_base(), "https://vault.example.com/identity");
        assert_eq!(ep.api_base(), "https://vault.example.com/api");
        assert_eq!(
            ep.prelogin_url(),
            "https://vault.example.com/identity/accounts/prelogin"
        );
        assert_eq!(
            ep.token_url(),
            "https://vault.example.com/identity/connect/token"
        );
        assert_eq!(ep.sync_url(), "https://vault.example.com/api/sync");
        assert_eq!(
            ep.cipher_url("id1"),
            "https://vault.example.com/api/ciphers/id1"
        );
    }

    #[test]
    fn custom_endpoints_and_trailing_slash() {
        let ep = BitwardenEndpoints::new("https://id.example.com///", "https://api.example.com/");
        assert_eq!(ep.identity_base(), "https://id.example.com");
        assert_eq!(ep.api_base(), "https://api.example.com");
        assert_eq!(ep.sync_url(), "https://api.example.com/sync");
    }

    #[test]
    fn auth_tokens_debug_redacts() {
        let tokens = AuthTokens {
            access_token: Zeroizing::new("super-secret-token".to_string()),
            refresh_token: Some(Zeroizing::new("refresh-secret".to_string())),
            token_type: "Bearer".to_string(),
            expires_in: 3600,
        };
        let rendered = format!("{tokens:?}");
        assert!(!rendered.contains("super-secret-token"), "{rendered}");
        assert!(!rendered.contains("refresh-secret"), "{rendered}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
        assert!(rendered.contains("Bearer"), "{rendered}");
    }

    /// 内存 mock：证明 trait 可实现、协议面无 IO 依赖。
    struct MockRest {
        endpoints: BitwardenEndpoints,
    }

    impl MockRest {
        fn new() -> Self {
            Self {
                endpoints: BitwardenEndpoints::cloud(),
            }
        }
    }

    impl BitwardenRest for MockRest {
        fn endpoints(&self) -> &BitwardenEndpoints {
            &self.endpoints
        }

        fn prelogin(&self, _email: &str) -> Result<KdfConfig> {
            KdfConfig::from_server(0, 600_000, None, None)
        }

        fn login(&self, _email: &str, _password_hash_b64: &str) -> Result<AuthTokens> {
            Ok(AuthTokens {
                access_token: Zeroizing::new("at".to_string()),
                refresh_token: Some(Zeroizing::new("rt".to_string())),
                token_type: "Bearer".to_string(),
                expires_in: 3600,
            })
        }

        fn refresh(&self, _refresh_token: &str) -> Result<AuthTokens> {
            self.login("", "")
        }

        fn sync(&self, _access_token: &str) -> Result<Vec<u8>> {
            Ok(b"{}".to_vec())
        }

        fn create_cipher(&self, _access_token: &str, _cipher_json: &[u8]) -> Result<Vec<u8>> {
            Ok(b"{}".to_vec())
        }

        fn update_cipher(
            &self,
            _access_token: &str,
            _cipher_id: &str,
            _cipher_json: &[u8],
        ) -> Result<Vec<u8>> {
            Ok(b"{}".to_vec())
        }

        fn delete_cipher(&self, _access_token: &str, _cipher_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn trait_is_implementable_with_in_memory_mock() {
        let rest = MockRest::new();
        assert_eq!(rest.endpoints(), &BitwardenEndpoints::cloud());
        let kdf = rest.prelogin("user@example.com").ok().unwrap();
        assert_eq!(kdf, KdfConfig::pbkdf2_default());
        let tokens = rest.login("user@example.com", "hash").ok().unwrap();
        assert_eq!(tokens.token_type, "Bearer");
        let refreshed = rest.refresh("rt").ok().unwrap();
        assert_eq!(refreshed.expires_in, 3600);
        assert_eq!(rest.sync("at").ok().unwrap(), b"{}");
        assert!(rest.create_cipher("at", b"{}").is_ok());
        assert!(rest.update_cipher("at", "id", b"{}").is_ok());
        assert!(rest.delete_cipher("at", "id").is_ok());
    }
}
