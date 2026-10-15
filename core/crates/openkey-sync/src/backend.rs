//! 同步后端抽象与远端目录布局（docs/02 3.3.2 / 3.3.5）。
//!
//! `SyncBackend` 是同步引擎与存储之间的唯一接口：v0.1.0 只定义 trait 与
//! 目录布局约定，WebDAV 实现（reqwest）随 Phase 3 落地。用户自持服务器
//! 的场景（坚果云 / 自建 WebDAV）由客户端注入实现。

use std::path::Path;

use crate::error::Result;
use crate::hlc::Hlc;

/// 同步存储后端（WebDAV 或等价协议）。
pub trait SyncBackend: Send + Sync {
    /// 上传文件（覆盖写）。
    fn put(&self, path: &str, data: &[u8]) -> Result<()>;
    /// 下载文件；不存在返回 `Ok(None)`。
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>>;
    /// 创建集合（目录，WebDAV MKCOL）。
    fn mkcol(&self, path: &str) -> Result<()>;
    /// 移动/重命名（WebDAV MOVE，用于 snapshot 滚动切换）。
    fn move_path(&self, from: &str, to: &str) -> Result<()>;
    /// 删除（WebDAV DELETE）。
    fn delete(&self, path: &str) -> Result<()>;
    /// 列目录（返回相对路径名，不含目录自身）。
    fn list(&self, path: &str) -> Result<Vec<String>>;
}

/// 清单文件路径（仓库根）。
pub fn manifest_path() -> &'static str {
    "manifest.json"
}

/// 设备信息文件路径。
pub fn device_info_path(device_id: &str) -> String {
    format!("devices/{device_id}/info.json")
}

/// 设备 oplog 分片路径（`{shard}` 形如 "00000.jsonl"）。
pub fn device_ops_shard(device_id: &str, shard: &str) -> String {
    format!("devices/{device_id}/ops/{shard}")
}

/// 设备 oplog 目录路径。
pub fn device_ops_dir(device_id: &str) -> String {
    format!("devices/{device_id}/ops")
}

/// 快照目录路径（以 HLC 命名，滚动时经 MOVE 原子切换）。
pub fn snapshot_dir(hlc: &Hlc) -> String {
    format!("snapshots/{hlc}")
}

/// 快照内条目分片文件名。
pub fn snapshot_shard_file(kind: &str) -> String {
    format!("entries-{kind}.json")
}

/// 校验远端路径片段（拒绝 `..`、绝对路径前缀与空段，防目录穿越）。
pub fn validate_remote_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(crate::Error::Snapshot("远端路径为空".into()));
    }
    if path.starts_with('/') || path.contains('\\') {
        return Err(crate::Error::Snapshot(format!(
            "远端路径非法（含绝对路径或反斜杠）: {path}"
        )));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(crate::Error::Snapshot(format!(
                "远端路径含非法片段: {path}"
            )));
        }
    }
    Ok(())
}

/// 本地快照目录的 manifest 文件名。
pub fn local_manifest_file() -> &'static str {
    "manifest.json"
}

/// 确保本地目录存在。
pub fn ensure_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_layout_paths() {
        assert_eq!(manifest_path(), "manifest.json");
        assert_eq!(device_info_path("dev-1"), "devices/dev-1/info.json");
        assert_eq!(
            device_ops_shard("dev-1", "00000.jsonl"),
            "devices/dev-1/ops/00000.jsonl"
        );
        assert_eq!(snapshot_shard_file("phrase"), "entries-phrase.json");
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(validate_remote_path("manifest.json").is_ok());
        assert!(validate_remote_path("devices/dev-1/ops/00000.jsonl").is_ok());
        assert!(validate_remote_path("../escape").is_err());
        assert!(validate_remote_path("/absolute").is_err());
        assert!(validate_remote_path("a//b").is_err());
        assert!(validate_remote_path("").is_err());
        assert!(validate_remote_path("a\\b").is_err());
    }
}
