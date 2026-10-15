//! openkey-components：组件包分发基建（docs/02 3.7）。
//!
//! 本 crate 的类型与签名即接口约定（契约），本文件为**实现**：
//! - versions.json 解析（schema 必须为 1）；
//! - SHA-256 校验（每组件完整性，docs/02 3.7）；
//! - 原子解压：先解到 `{dest}.tmp/`，再 rename 到 `dest`，失败清理 tmp
//!   （zip crate 自身防御 `..` 路径逃逸）；
//! - 注册表持久化到 `{root}/registry.json`（原子写：`.tmp` → rename），
//!   [`ComponentManager::new`] 时加载；版本回滚 = 改注册表指针；
//! - `ensure` 流程：已安装直接返回 → 查清单 → 下载到
//!   `{root}/{name}/{version}.zip.tmp` → 校验 → 解压到 `.tmp` → rename
//!   → 写注册表；任一步失败即清理临时文件并返回（保留旧版本）。
//!
//! 下载经 [`ComponentDownloader`] 抽象注入：测试用本地假实现，
//! 生产用 `net` feature 下的 [`ReqwestDownloader`]。

pub mod error;

#[cfg(feature = "net")]
pub mod downloader;

pub use error::{Error, Result};

#[cfg(feature = "net")]
pub use downloader::ReqwestDownloader;

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// 支持的 versions.json schema 版本（docs/02 附录 B）。
const MANIFEST_SCHEMA: u32 = 1;

/// 注册表文件名（`{root}/registry.json`）。
const REGISTRY_FILE: &str = "registry.json";

/// 读写缓冲（字节）：SHA-256 流式计算与下载分块共用。
const CHUNK: usize = 64 * 1024;

/// versions.json 中的单个组件条目（docs/02 附录 B）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ComponentEntry {
    /// 版本号。
    pub version: String,
    /// 下载 URL。
    pub url: String,
    /// SHA-256（十六进制小写）。
    pub sha256: String,
    /// 体积（字节，可选）。
    pub size: Option<u64>,
    /// 要求的最低 App 版本（可选）。
    pub min_app: Option<String>,
    /// 许可证（可选，如 GPL-3.0）。
    pub license: Option<String>,
    /// 来源（可选）。
    pub source: Option<String>,
    /// 渠道（可选，stable/beta）。
    pub channel: Option<String>,
}

/// versions.json 清单。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ComponentManifest {
    /// 清单 schema 版本。
    pub schema: u32,
    /// 发布时间（RFC 3339）。
    pub published_at: String,
    /// GPG 签名（可选，PEM armor）。
    pub signature: Option<String>,
    /// 组件表。
    pub components: BTreeMap<String, ComponentEntry>,
}

/// 解析 versions.json。
pub fn parse_manifest(json: &str) -> Result<ComponentManifest> {
    let manifest: ComponentManifest = serde_json::from_str(json)?;
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(Error::UnsupportedSchema {
            expected: MANIFEST_SCHEMA,
            found: manifest.schema,
        });
    }
    Ok(manifest)
}

/// 计算文件 SHA-256（十六进制小写）。
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 校验文件 SHA-256，不符时返回 [`Error::ChecksumMismatch`]。
///
/// `expected` 容忍大小写与首尾空白（清单约定小写十六进制）。
pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let actual = sha256_file(path)?;
    let expected = expected.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(Error::ChecksumMismatch { expected, actual });
    }
    Ok(())
}

/// 原子解压 zip：先解到 `{dest}.tmp/`，再 rename 到 `dest`；
/// 失败时清理 tmp。`dest` 已存在则报错（调用方先切换或清理）。
pub fn extract_zip_atomic(zip_path: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        return Err(Error::VersionExists(dest.display().to_string()));
    }
    let tmp = tmp_path(dest);
    // 清理上次失败安装的残留（保证 tmp 起点干净）。
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let result =
        extract_into(zip_path, &tmp).and_then(|()| fs::rename(&tmp, dest).map_err(Error::from));
    if let Err(e) = result {
        // 失败即清理 tmp，不留下半成品（旧版本不受影响，可回滚）。
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    Ok(())
}

/// 打开 zip 并整体解压到 `tmp`（zip crate 已防御 `..` 路径逃逸与 CRC 校验）。
fn extract_into(zip_path: &Path, tmp: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::Zip(e.to_string()))?;
    archive.extract(tmp).map_err(|e| Error::Zip(e.to_string()))
}

/// `{path}.tmp`：解压中转目录 / 注册表临时文件共用命名。
fn tmp_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".tmp");
    PathBuf::from(os)
}

/// 组件下载器抽象（网络实现由 `net` feature / 各端注入）。
pub trait ComponentDownloader: Send + Sync {
    /// 下载 `url` 到 `dest`；`progress(downloaded, total)` 可被高频调用。
    fn download(
        &self,
        _url: &str,
        _dest: &Path,
        _progress: &dyn Fn(u64, Option<u64>),
    ) -> Result<()>;
}

/// 管理器内部状态（清单 + 注册表指针）。
#[derive(Debug, Default)]
struct ManagerState {
    /// 已加载的 versions.json（`set_manifest_json` 注入）。
    manifest: Option<ComponentManifest>,
    /// 注册表：组件名 → 当前版本（`{root}/registry.json` 持久化）。
    registry: BTreeMap<String, String>,
}

/// 组件管理器：注册表 + 版本指针 + 安装目录布局
/// `{root}/{name}/{version}/`。
#[derive(Debug)]
pub struct ComponentManager {
    root: PathBuf,
    state: Mutex<ManagerState>,
}

impl ComponentManager {
    /// 以组件根目录创建（加载磁盘上已有的注册表）。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let registry = load_registry(&root);
        Self {
            root,
            state: Mutex::new(ManagerState {
                manifest: None,
                registry,
            }),
        }
    }

    /// 组件根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 加载 versions.json 文本。
    pub fn set_manifest_json(&self, json: &str) -> Result<()> {
        let manifest = parse_manifest(json)?;
        log::info!(
            "组件清单已加载：{} 个组件（发布于 {}）",
            manifest.components.len(),
            manifest.published_at
        );
        self.lock().manifest = Some(manifest);
        Ok(())
    }

    /// 查询清单中的组件条目。
    pub fn manifest_entry(&self, name: &str) -> Option<ComponentEntry> {
        self.lock()
            .manifest
            .as_ref()
            .and_then(|m| m.components.get(name))
            .cloned()
    }

    /// 确保组件就绪：已安装则直接返回版本；否则经 `downloader`
    /// 下载 → 校验 → 原子解压 → 登记 → 返回版本。
    pub fn ensure(&self, name: &str, downloader: &dyn ComponentDownloader) -> Result<String> {
        let mut state = self.lock();
        // 已安装（注册表指针 + 目录均在）→ 直接返回，不重复下载
        // （docs/02 3.7：用户开启功能 → 检查注册表 → 无则下载）。
        if let Some(version) = state.registry.get(name) {
            if self.component_dir(name, version).is_dir() {
                return Ok(version.clone());
            }
        }
        let entry = state
            .manifest
            .as_ref()
            .and_then(|m| m.components.get(name))
            .cloned()
            .ok_or_else(|| Error::NotInManifest(name.to_string()))?;
        let version = entry.version.clone();
        let dest = self.component_dir(name, &version);
        let zip_tmp = self.root.join(name).join(format!("{version}.zip.tmp"));
        fs::create_dir_all(self.root.join(name))?;
        // 上次安装崩溃的残留目录（未登记注册表）：删除后以本次 zip 重装。
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        let installed = self
            .download_verify_extract(&entry, &zip_tmp, &dest, downloader)
            .and_then(|()| {
                state.registry.insert(name.to_string(), version.clone());
                persist_registry(&self.root, &state.registry)
            });
        // zip 临时文件不再需要：成功已解压、失败不留残file。
        let _ = fs::remove_file(&zip_tmp);
        installed?;
        log::info!("组件 {name} v{version} 安装完成");
        Ok(version)
    }

    /// 当前版本指针（无则 `None`）。
    pub fn installed_version(&self, name: &str) -> Option<String> {
        self.lock().registry.get(name).cloned()
    }

    /// 是否已安装（当前版本目录存在）。
    pub fn is_installed(&self, name: &str) -> bool {
        let state = self.lock();
        match state.registry.get(name) {
            Some(version) => self.component_dir(name, version).is_dir(),
            None => false,
        }
    }

    /// 组件安装目录（`{root}/{name}/{version}`）。
    pub fn component_dir(&self, name: &str, version: &str) -> PathBuf {
        self.root.join(name).join(version)
    }

    /// 清理旧版本（保留 `keep` 中的版本与当前版本）。
    ///
    /// 返回被删除的版本标识（`"{name}/{version}"`，供日志/UI 展示）。
    pub fn gc(&self, keep: &[String]) -> Result<Vec<String>> {
        let state = self.lock();
        let mut removed = Vec::new();
        let Ok(components) = fs::read_dir(&self.root) else {
            return Ok(removed); // 根目录不存在：无版本可清理
        };
        for component in components.flatten() {
            let component_path = component.path();
            if !component_path.is_dir() {
                continue; // registry.json 等非组件条目
            }
            let Some(name) = component_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let current = state.registry.get(name).cloned();
            let Ok(versions) = fs::read_dir(&component_path) else {
                continue;
            };
            for version in versions.flatten() {
                let version_path = version.path();
                if !version_path.is_dir() {
                    continue; // `{version}.zip.tmp` 等残留文件
                }
                let Some(version) = version_path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if current.as_deref() == Some(version) {
                    continue; // 当前版本
                }
                if keep.iter().any(|k| k == version) {
                    continue; // 显式保留（回滚目标）
                }
                if fs::remove_dir_all(&version_path).is_ok() {
                    removed.push(format!("{name}/{version}"));
                } else {
                    log::warn!("清理旧版本失败：{}", version_path.display());
                }
            }
        }
        Ok(removed)
    }

    /// 下载 → 校验 → 原子解压（注册表登记由调用方完成）。
    fn download_verify_extract(
        &self,
        entry: &ComponentEntry,
        zip_tmp: &Path,
        dest: &Path,
        downloader: &dyn ComponentDownloader,
    ) -> Result<()> {
        log::info!("下载组件包：{}", entry.url);
        downloader.download(&entry.url, zip_tmp, &|downloaded, total| match total {
            Some(total) if total > 0 => {
                log::debug!("下载进度：{downloaded}/{total}");
            }
            _ => {
                log::debug!("下载进度：{downloaded}");
            }
        })?;
        verify_sha256(zip_tmp, &entry.sha256)?;
        extract_zip_atomic(zip_tmp, dest)
    }

    /// 取状态锁（中毒时恢复内容：状态不变量在 panic 前后保持一致）。
    fn lock(&self) -> std::sync::MutexGuard<'_, ManagerState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// 注册表路径：`{root}/registry.json`。
fn registry_path(root: &Path) -> PathBuf {
    root.join(REGISTRY_FILE)
}

/// 从磁盘加载注册表（不存在/损坏按空处理，不阻断启动）。
fn load_registry(root: &Path) -> BTreeMap<String, String> {
    let path = registry_path(root);
    let Ok(json) = fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    match serde_json::from_str(&json) {
        Ok(registry) => registry,
        Err(e) => {
            log::warn!("注册表 {} 解析失败，按空处理：{e}", path.display());
            BTreeMap::new()
        }
    }
}

/// 原子持久化注册表：写 `registry.json.tmp` → rename（先 sync_all 落盘）。
fn persist_registry(root: &Path, registry: &BTreeMap<String, String>) -> Result<()> {
    fs::create_dir_all(root)?;
    let path = registry_path(root);
    let tmp = tmp_path(&path);
    let json = serde_json::to_vec_pretty(registry)?;
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 测试临时目录（Drop 时自动清理，panic 也不留残file）。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "openkey-components-test-{}-{tag}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).expect("创建临时目录失败");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 假下载器：从本地目录按 url（文件名）复制，并计数。
    struct FakeDownloader {
        source_dir: PathBuf,
        downloads: AtomicUsize,
    }

    impl FakeDownloader {
        fn new(source_dir: &Path) -> Self {
            Self {
                source_dir: source_dir.to_path_buf(),
                downloads: AtomicUsize::new(0),
            }
        }

        fn count(&self) -> usize {
            self.downloads.load(Ordering::SeqCst)
        }
    }

    impl ComponentDownloader for FakeDownloader {
        fn download(
            &self,
            url: &str,
            dest: &Path,
            progress: &dyn Fn(u64, Option<u64>),
        ) -> Result<()> {
            let src = self.source_dir.join(url);
            let bytes = fs::read(&src).map_err(|e| Error::Download(format!("{url}: {e}")))?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(dest, &bytes)?;
            self.downloads.fetch_add(1, Ordering::SeqCst);
            progress(bytes.len() as u64, Some(bytes.len() as u64));
            Ok(())
        }
    }

    /// 构造含指定条目的 zip（Stored，免 deflate 依赖差异）。
    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("创建 zip 失败");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, content) in entries {
            writer.start_file(*name, options).expect("start_file 失败");
            writer.write_all(content).expect("写 zip 失败");
        }
        writer.finish().expect("finish zip 失败");
    }

    /// 搭建测试环境：临时根 + 本地“远端”目录 + comp-1.0.0.zip。
    /// 返回（临时根, 源目录, zip 的 sha256）。
    fn setup(tag: &str) -> (TempDir, PathBuf, String) {
        let tmp = TempDir::new(tag);
        let source = tmp.path().join("source");
        fs::create_dir_all(&source).expect("创建源目录失败");
        let zip_path = source.join("comp-1.0.0.zip");
        make_zip(
            &zip_path,
            &[("ok.txt", b"hello"), ("sub/dir.txt", b"world")],
        );
        let sha = sha256_file(&zip_path).expect("计算 sha 失败");
        (tmp, source, sha)
    }

    /// 指向本地 zip 的 manifest JSON。
    fn manifest_json(sha: &str) -> String {
        format!(
            r#"{{"schema":1,"published_at":"2026-10-13T00:00:00Z",
                 "components":{{"comp":{{"version":"1.0.0","url":"comp-1.0.0.zip",
                 "sha256":"{sha}","size":10}}}}}}"#
        )
    }

    #[test]
    fn parse_manifest_ok() {
        let json = r#"{
            "schema": 1,
            "published_at": "2026-10-13T00:00:00Z",
            "signature": "-----BEGIN PGP SIGNATURE-----...",
            "components": {
                "lexicon-rime-ice": {
                    "version": "2026.10",
                    "url": "https://example.com/ice.zip",
                    "sha256": "abc123",
                    "size": 22000000,
                    "min_app": "1.5.0",
                    "license": "GPL-3.0",
                    "source": "github.com/iDvel/rime-ice",
                    "channel": "stable"
                },
                "imagehost-templates": {
                    "version": "1",
                    "url": "https://example.com/templates-1.zip",
                    "sha256": "def456"
                }
            }
        }"#;
        let manifest = parse_manifest(json).expect("解析失败");
        assert_eq!(manifest.schema, 1);
        assert_eq!(manifest.published_at, "2026-10-13T00:00:00Z");
        assert!(manifest.signature.is_some());
        assert_eq!(manifest.components.len(), 2);
        let ice = &manifest.components["lexicon-rime-ice"];
        assert_eq!(ice.version, "2026.10");
        assert_eq!(ice.license.as_deref(), Some("GPL-3.0"));
        assert_eq!(ice.channel.as_deref(), Some("stable"));
        let templates = &manifest.components["imagehost-templates"];
        assert_eq!(templates.version, "1");
        assert!(templates.size.is_none());
        assert!(templates.min_app.is_none());
    }

    #[test]
    fn parse_manifest_rejects_other_schema() {
        let json = r#"{"schema":2,"published_at":"2026-10-13T00:00:00Z","components":{}}"#;
        let err = parse_manifest(json).expect_err("schema 2 应被拒绝");
        assert!(
            matches!(
                err,
                Error::UnsupportedSchema {
                    expected: 1,
                    found: 2
                }
            ),
            "实际错误: {err}"
        );
    }

    #[test]
    fn parse_manifest_rejects_broken_json() {
        assert!(matches!(parse_manifest("not json"), Err(Error::Json(_))));
        // 缺必填字段（components / published_at）同样拒绝。
        assert!(parse_manifest(r#"{"schema":1}"#).is_err());
    }

    #[test]
    fn sha256_file_matches_known_digest() {
        let tmp = TempDir::new("sha");
        let path = tmp.path().join("hello.bin");
        fs::write(&path, b"hello").expect("写文件失败");
        assert_eq!(
            sha256_file(&path).expect("计算失败"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(matches!(
            sha256_file(&tmp.path().join("missing.bin")),
            Err(Error::Io(_))
        ));
    }

    #[test]
    fn verify_sha256_accepts_and_rejects() {
        let tmp = TempDir::new("verify");
        let path = tmp.path().join("data.bin");
        fs::write(&path, b"hello").expect("写文件失败");
        let digest = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        verify_sha256(&path, digest).expect("匹配应通过");
        // 期望值容忍大写与空白。
        verify_sha256(&path, &format!("  {digest}  ").to_ascii_uppercase())
            .expect("规范化后应通过");
        let err = verify_sha256(&path, &"0".repeat(64)).expect_err("不匹配应报错");
        assert!(
            matches!(err, Error::ChecksumMismatch { .. }),
            "实际错误: {err}"
        );
    }

    #[test]
    fn extract_zip_atomic_roundtrip() {
        let tmp = TempDir::new("extract");
        let zip_path = tmp.path().join("pkg.zip");
        make_zip(&zip_path, &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        let dest = tmp.path().join("dest");
        extract_zip_atomic(&zip_path, &dest).expect("解压失败");
        assert_eq!(
            fs::read_to_string(dest.join("a.txt")).expect("读 a.txt 失败"),
            "alpha"
        );
        assert_eq!(
            fs::read_to_string(dest.join("sub/b.txt")).expect("读 b.txt 失败"),
            "beta"
        );
        // tmp 中转目录应已 rename 消失。
        assert!(!tmp_path(&dest).exists());
    }

    #[test]
    fn extract_zip_atomic_rejects_existing_dest() {
        let tmp = TempDir::new("extract-exists");
        let zip_path = tmp.path().join("pkg.zip");
        make_zip(&zip_path, &[("a.txt", b"alpha")]);
        let dest = tmp.path().join("dest");
        fs::create_dir_all(&dest).expect("创建 dest 失败");
        let err = extract_zip_atomic(&zip_path, &dest).expect_err("dest 存在应报错");
        assert!(matches!(err, Error::VersionExists(_)), "实际错误: {err}");
    }

    #[test]
    fn extract_zip_atomic_bad_zip_cleans_tmp() {
        let tmp = TempDir::new("extract-bad");
        let zip_path = tmp.path().join("bad.zip");
        fs::write(&zip_path, b"this is not a zip").expect("写坏 zip 失败");
        let dest = tmp.path().join("dest");
        let err = extract_zip_atomic(&zip_path, &dest).expect_err("坏 zip 应报错");
        assert!(matches!(err, Error::Zip(_)), "实际错误: {err}");
        assert!(!dest.exists(), "失败不应留下 dest");
        assert!(!tmp_path(&dest).exists(), "失败应清理 tmp");
    }

    #[test]
    fn ensure_installs_and_persists_registry() {
        let (tmp, _source, sha) = setup("ensure");
        let root = tmp.path().join("components");
        let manager = ComponentManager::new(&root);
        manager
            .set_manifest_json(&manifest_json(&sha))
            .expect("加载清单失败");
        let downloader = FakeDownloader::new(&tmp.path().join("source"));

        let version = manager.ensure("comp", &downloader).expect("安装失败");
        assert_eq!(version, "1.0.0");
        assert!(manager.is_installed("comp"));
        assert_eq!(manager.installed_version("comp").as_deref(), Some("1.0.0"));
        assert_eq!(
            fs::read_to_string(manager.component_dir("comp", "1.0.0").join("ok.txt"))
                .expect("读安装产物失败"),
            "hello"
        );
        // 注册表已原子落盘。
        let registry: BTreeMap<String, String> = serde_json::from_str(
            &fs::read_to_string(root.join("registry.json")).expect("读注册表失败"),
        )
        .expect("解析注册表失败");
        assert_eq!(registry.get("comp").map(String::as_str), Some("1.0.0"));
        // zip 临时文件应已清理。
        assert!(!root.join("comp/1.0.0.zip.tmp").exists());
        assert_eq!(downloader.count(), 1);
    }

    #[test]
    fn ensure_skips_download_when_installed() {
        let (tmp, _source, sha) = setup("ensure-skip");
        let root = tmp.path().join("components");
        let manager = ComponentManager::new(&root);
        manager
            .set_manifest_json(&manifest_json(&sha))
            .expect("加载清单失败");
        let downloader = FakeDownloader::new(&tmp.path().join("source"));

        assert_eq!(
            manager.ensure("comp", &downloader).expect("首次安装失败"),
            "1.0.0"
        );
        assert_eq!(
            manager
                .ensure("comp", &downloader)
                .expect("二次 ensure 失败"),
            "1.0.0"
        );
        assert_eq!(downloader.count(), 1, "已安装不应重复下载");
    }

    #[test]
    fn ensure_registry_survives_manager_reload() {
        let (tmp, _source, sha) = setup("ensure-reload");
        let root = tmp.path().join("components");
        {
            let manager = ComponentManager::new(&root);
            manager
                .set_manifest_json(&manifest_json(&sha))
                .expect("加载清单失败");
            let downloader = FakeDownloader::new(&tmp.path().join("source"));
            manager.ensure("comp", &downloader).expect("安装失败");
        }
        // 新管理器（无清单）从磁盘注册表恢复“已安装”状态。
        let manager = ComponentManager::new(&root);
        assert_eq!(manager.installed_version("comp").as_deref(), Some("1.0.0"));
        assert!(manager.is_installed("comp"));
        assert!(manager.manifest_entry("comp").is_none());
    }

    #[test]
    fn ensure_unknown_component_rejected() {
        let (tmp, _source, sha) = setup("ensure-unknown");
        let manager = ComponentManager::new(tmp.path().join("components"));
        manager
            .set_manifest_json(&manifest_json(&sha))
            .expect("加载清单失败");
        let downloader = FakeDownloader::new(&tmp.path().join("source"));
        let err = manager
            .ensure("missing-comp", &downloader)
            .expect_err("未知组件应报错");
        assert!(matches!(err, Error::NotInManifest(_)), "实际错误: {err}");
    }

    #[test]
    fn ensure_checksum_failure_cleans_up() {
        let (tmp, _source, _sha) = setup("ensure-checksum");
        let root = tmp.path().join("components");
        let manager = ComponentManager::new(&root);
        // 清单里放一个错误的 sha256。
        manager
            .set_manifest_json(&manifest_json(&"0".repeat(64)))
            .expect("加载清单失败");
        let downloader = FakeDownloader::new(&tmp.path().join("source"));

        let err = manager
            .ensure("comp", &downloader)
            .expect_err("校验失败应报错");
        assert!(
            matches!(err, Error::ChecksumMismatch { .. }),
            "实际错误: {err}"
        );
        assert!(!manager.is_installed("comp"));
        assert!(
            !root.join("comp/1.0.0.zip.tmp").exists(),
            "失败应清理 zip 临时文件"
        );
        assert!(!root.join("comp/1.0.0").exists(), "失败不应留下安装目录");
    }

    #[test]
    fn ensure_reinstalls_when_version_dir_deleted() {
        let (tmp, _source, sha) = setup("ensure-reinstall");
        let root = tmp.path().join("components");
        let manager = ComponentManager::new(&root);
        manager
            .set_manifest_json(&manifest_json(&sha))
            .expect("加载清单失败");
        let downloader = FakeDownloader::new(&tmp.path().join("source"));
        manager.ensure("comp", &downloader).expect("首次安装失败");

        fs::remove_dir_all(manager.component_dir("comp", "1.0.0")).expect("删除版本目录失败");
        assert!(!manager.is_installed("comp"), "目录删除后应为未安装");

        let version = manager.ensure("comp", &downloader).expect("重装失败");
        assert_eq!(version, "1.0.0");
        assert_eq!(downloader.count(), 2, "目录缺失应重新下载");
        assert!(manager.is_installed("comp"));
    }

    #[test]
    fn set_manifest_json_and_entry_lookup() {
        let (tmp, _source, sha) = setup("manifest-entry");
        let manager = ComponentManager::new(tmp.path().join("components"));
        manager
            .set_manifest_json(&manifest_json(&sha))
            .expect("加载清单失败");
        let entry = manager.manifest_entry("comp").expect("应能查到条目");
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.url, "comp-1.0.0.zip");
        assert_eq!(entry.sha256, sha);
        assert_eq!(entry.size, Some(10));
        assert!(manager.manifest_entry("other").is_none());
        // schema 非 1 的清单应整体拒绝。
        let bad = r#"{"schema":9,"published_at":"2026-10-13T00:00:00Z","components":{}}"#;
        assert!(manager.set_manifest_json(bad).is_err());
    }

    #[test]
    fn gc_keeps_current_and_keep_list() {
        let (tmp, _source, sha) = setup("gc");
        let root = tmp.path().join("components");
        let manager = ComponentManager::new(&root);
        manager
            .set_manifest_json(&manifest_json(&sha))
            .expect("加载清单失败");
        let downloader = FakeDownloader::new(&tmp.path().join("source"));
        manager.ensure("comp", &downloader).expect("安装失败");

        // 手动制造两个旧版本目录。
        fs::create_dir_all(root.join("comp/0.9.0")).expect("创建 0.9.0 失败");
        fs::create_dir_all(root.join("comp/0.8.0")).expect("创建 0.8.0 失败");

        let removed = manager.gc(&["0.9.0".to_string()]).expect("gc 失败");
        assert_eq!(removed, vec!["comp/0.8.0".to_string()]);
        assert!(root.join("comp/1.0.0").is_dir(), "当前版本应保留");
        assert!(root.join("comp/0.9.0").is_dir(), "keep 列表中的版本应保留");
        assert!(!root.join("comp/0.8.0").exists(), "旧版本应被清理");

        // 无 keep：0.9.0 也被清理，当前版本仍在。
        let removed = manager.gc(&[]).expect("gc 失败");
        assert_eq!(removed, vec!["comp/0.9.0".to_string()]);
        assert!(root.join("comp/1.0.0").is_dir());
    }

    #[test]
    fn gc_on_empty_root_is_noop() {
        let tmp = TempDir::new("gc-empty");
        let manager = ComponentManager::new(tmp.path().join("nonexistent"));
        let removed = manager.gc(&[]).expect("gc 失败");
        assert!(removed.is_empty());
        assert!(manager.installed_version("comp").is_none());
        assert!(!manager.is_installed("comp"));
    }
}
