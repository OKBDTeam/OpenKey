//! 快照分片序列化（docs/02 3.3.2 / 3.3.3）。
//!
//! 布局（本地导出目录与远端 `snapshots/{hlc}/` 一致）：
//! ```text
//! manifest.json                 # {hlc, shards:[{kind,file,count,sha256}]}
//! entries-{kind}.json           # 按条目类型分片；超 4MB 续分 entries-{kind}.1.json
//! ```
//! payload 以十六进制编码（与数据层 BLOB 对应，避免 base64 依赖）。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::hlc::Hlc;
use crate::oplog::SHARD_MAX_BYTES;

/// manifest 中的分片描述。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotShard {
    /// 条目类型（`phrase` / `setting` / …）。
    pub kind: String,
    /// 分片文件名（相对快照目录）。
    pub file: String,
    /// 条目数。
    pub count: usize,
    /// 分片文件 SHA-256（十六进制小写）。
    pub sha256: String,
}

/// 快照 manifest。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// 快照对应的 HLC（滚动切换的排序键）。
    pub hlc: Hlc,
    /// 分片列表（按 kind 名字序）。
    pub shards: Vec<SnapshotShard>,
}

/// 条目的可序列化视图（与 openkey-data 的 `Entry` 字段一一对应）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryView {
    /// 主键。
    pub key: String,
    /// 条目类型。
    pub kind: String,
    /// payload（十六进制）。
    pub payload: String,
    /// HLC 字符串。
    pub hlc: String,
    /// 最后写入设备。
    pub device_id: String,
    /// 墓碑标记。
    pub deleted: bool,
    /// 本地墙钟毫秒。
    pub updated_at: i64,
}

impl EntryView {
    /// 从数据层条目转换。
    pub fn from_entry(entry: &openkey_data::Entry) -> Self {
        Self {
            key: entry.key.clone(),
            kind: entry.kind.as_str().to_string(),
            payload: hex::encode(&entry.payload),
            hlc: entry.hlc.clone(),
            device_id: entry.device_id.clone(),
            deleted: entry.deleted,
            updated_at: entry.updated_at,
        }
    }

    /// 转回数据层条目。
    pub fn to_entry(&self) -> Result<openkey_data::Entry> {
        let kind = openkey_data::EntryKind::parse(&self.kind)
            .ok_or_else(|| Error::Corrupt(format!("未知条目类型: {}", self.kind)))?;
        let payload = hex::decode(&self.payload)
            .map_err(|e| Error::Corrupt(format!("payload 十六进制解码失败: {e}")))?;
        Ok(openkey_data::Entry {
            key: self.key.clone(),
            kind,
            payload,
            hlc: self.hlc.clone(),
            device_id: self.device_id.clone(),
            deleted: self.deleted,
            updated_at: self.updated_at,
        })
    }
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// 把一个分片组落盘并登记到 `shards`。
fn flush_shard(
    dir: &Path,
    kind: &str,
    chunk: &[&openkey_data::Entry],
    index: usize,
    shards: &mut Vec<SnapshotShard>,
) -> Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }
    let views: Vec<EntryView> = chunk.iter().map(|e| EntryView::from_entry(e)).collect();
    let data = serde_json::to_vec(&views).expect("EntryView 序列化不应失败");
    let file = if index == 0 {
        crate::backend::snapshot_shard_file(kind)
    } else {
        format!("entries-{kind}.{index}.json")
    };
    let path = dir.join(&file);
    fs::write(&path, &data)?;
    shards.push(SnapshotShard {
        kind: kind.to_string(),
        file,
        count: chunk.len(),
        sha256: sha256_hex(&data),
    });
    Ok(())
}

/// 写快照：按 kind 分片（单片超 [`SHARD_MAX_BYTES` 时续分），附 manifest。
pub fn write_snapshot(
    dir: &Path,
    hlc: &Hlc,
    entries: &[openkey_data::Entry],
) -> Result<SnapshotManifest> {
    fs::create_dir_all(dir)?;

    // 按 kind 分组（保持 kind 名字序，保证产物确定性）。
    let mut by_kind: BTreeMap<String, Vec<&openkey_data::Entry>> = BTreeMap::new();
    for entry in entries {
        by_kind
            .entry(entry.kind.as_str().to_string())
            .or_default()
            .push(entry);
    }

    let mut shards = Vec::new();
    for (kind, group) in &by_kind {
        let mut index = 0usize;
        let mut chunk: Vec<&openkey_data::Entry> = Vec::new();
        let mut chunk_bytes: usize = 2; // "[" "]"

        for entry in group {
            let view = EntryView::from_entry(entry);
            let size = serde_json::to_vec(&view)
                .expect("EntryView 序列化不应失败")
                .len()
                + 1;
            if !chunk.is_empty() && chunk_bytes + size > SHARD_MAX_BYTES as usize {
                flush_shard(dir, kind, &chunk, index, &mut shards)?;
                index += 1;
                chunk.clear();
                chunk_bytes = 2;
            }
            chunk_bytes += size;
            chunk.push(entry);
        }
        flush_shard(dir, kind, &chunk, index, &mut shards)?;
    }

    let manifest = SnapshotManifest {
        hlc: hlc.clone(),
        shards,
    };
    let manifest_path = dir.join(crate::backend::local_manifest_file());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("manifest 序列化不应失败"),
    )?;
    Ok(manifest)
}

/// 读快照：校验 manifest 与各分片 SHA-256，返回全部条目。
pub fn read_snapshot(dir: &Path) -> Result<(SnapshotManifest, Vec<openkey_data::Entry>)> {
    let manifest_path = dir.join(crate::backend::local_manifest_file());
    let manifest: SnapshotManifest =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(|e| {
            Error::Snapshot(format!("读取 {} 失败: {e}", manifest_path.display()))
        })?)?;

    let mut entries = Vec::new();
    for shard in &manifest.shards {
        let path = dir.join(&shard.file);
        let data = fs::read(&path)
            .map_err(|e| Error::Snapshot(format!("读取 {} 失败: {e}", path.display())))?;
        let actual = sha256_hex(&data);
        if actual != shard.sha256 {
            return Err(Error::Snapshot(format!(
                "分片 {} 校验失败: 期望 {}, 实际 {}",
                shard.file, shard.sha256, actual
            )));
        }
        let views: Vec<EntryView> = serde_json::from_slice(&data)?;
        if views.len() != shard.count {
            return Err(Error::Snapshot(format!(
                "分片 {} 条目数不符: manifest {}, 实际 {}",
                shard.file,
                shard.count,
                views.len()
            )));
        }
        for view in &views {
            entries.push(view.to_entry()?);
        }
    }
    Ok((manifest, entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openkey_data::{Entry, EntryKind};

    fn entry(key: &str, kind: EntryKind, payload: &str, hlc: &str) -> Entry {
        Entry {
            key: key.to_string(),
            kind,
            payload: payload.as_bytes().to_vec(),
            hlc: hlc.to_string(),
            device_id: "dev-a".to_string(),
            deleted: false,
            updated_at: 1_700_000_000_000,
        }
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openkey-sync-snap-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn snapshot_roundtrip_by_kind() {
        let dir = temp_dir("roundtrip");
        let hlc = Hlc::new(1_700_000_000_000, 0, "dev-a");
        let entries = vec![
            entry(
                "phrase-1",
                EntryKind::Phrase,
                "{\"text\":\"你好\"}",
                "100.0.dev-a",
            ),
            entry("setting-1", EntryKind::Setting, "\"on\"", "101.0.dev-a"),
            entry(
                "phrase-2",
                EntryKind::Phrase,
                "{\"text\":\"世界\"}",
                "102.0.dev-a",
            ),
        ];
        let manifest = write_snapshot(&dir, &hlc, &entries).unwrap();
        assert_eq!(manifest.hlc, hlc);
        assert_eq!(manifest.shards.len(), 2, "应按 kind 分片");
        let total: usize = manifest.shards.iter().map(|s| s.count).sum();
        assert_eq!(total, 3);

        let (read_back, read_entries) = read_snapshot(&dir).unwrap();
        assert_eq!(read_back, manifest);
        assert_eq!(read_entries.len(), 3);
        assert_eq!(read_entries[0].payload_json().unwrap()["text"], "你好");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_detects_tampering() {
        let dir = temp_dir("tamper");
        let hlc = Hlc::new(1, 0, "dev-a");
        let entries = vec![entry("phrase-1", EntryKind::Phrase, "x", "1.0.dev-a")];
        write_snapshot(&dir, &hlc, &entries).unwrap();

        let shard = dir.join("entries-phrase.json");
        let mut data: Vec<u8> = fs::read(&shard).unwrap();
        data.push(b' ');
        fs::write(&shard, &data).unwrap();
        assert!(read_snapshot(&dir).is_err(), "篡改分片必须被检测");
        let _ = fs::remove_dir_all(&dir);
    }
}
