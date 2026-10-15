//! oplog 读写与分片（docs/02 3.3.2）。
//!
//! oplog 以 JSONL 形式落盘（一行一条），远端目录布局为
//! `devices/{device_id}/ops/{shard}`，分片按 [`SHARD_MAX_BYTES`] 滚动。
//! 行格式字段名与设计一致：`key/kind/op/hlc/enc`，其中 `key/kind/op/hlc`
//! 为明文元数据（最小化），`enc` 为 E2E 加密后的条目密文（docs/02 6.3，
//! Phase 3 实现；未启用 E2E 时该字段缺省）。

use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use log::warn;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::hlc::Hlc;

/// oplog 分片滚动阈值（4MB，docs/02 3.3.2）。
pub const SHARD_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// oplog 操作类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpKind {
    /// 写入/更新（含墓碑之外的存活条目）。
    Upsert,
    /// 删除（墓碑）。
    Delete,
}

/// oplog 行（JSONL，一行一条）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpEntry {
    /// 条目主键。
    pub key: String,
    /// 条目类型（`openkey_data::EntryKind` 存储值）。
    pub kind: String,
    /// 操作类型。
    pub op: OpKind,
    /// 条目 HLC（线格式字符串）。
    pub hlc: Hlc,
    /// 条目 payload 的传输表示：启用 E2E 时为密文；未启用时缺省
    /// （此时 payload 不经本字段传输，仅受 TLS 保护，见 docs/02 5.3 强提示）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enc: Option<String>,
}

impl OpEntry {
    /// 由 openkey-data 已存储条目构造 oplog 记录。
    ///
    /// `payload_repr` 为 payload 的传输表示（见 [`OpEntry::enc`] 文档）。
    pub fn from_entry(entry: &openkey_data::Entry, payload_repr: Option<String>) -> Result<Self> {
        let op = if entry.deleted {
            OpKind::Delete
        } else {
            OpKind::Upsert
        };
        Ok(Self {
            key: entry.key.clone(),
            kind: entry.kind.as_str().to_string(),
            op,
            hlc: Hlc::parse(&entry.hlc)?,
            enc: payload_repr,
        })
    }

    /// 还原为 openkey-data 待写入条目（同步回放 / apply 阶段使用）。
    ///
    /// `payload` 为 `enc` 解密（E2E 开启）或直接取得的 payload 字节；
    /// `updated_at` 为回放时的本地墙钟毫秒。
    pub fn to_new_entry(
        &self,
        payload: Vec<u8>,
        updated_at: i64,
    ) -> Result<openkey_data::NewEntry> {
        let kind = openkey_data::EntryKind::parse(&self.kind)
            .ok_or_else(|| Error::Corrupt(format!("未知条目类型: {}", self.kind)))?;
        Ok(openkey_data::NewEntry {
            kind,
            payload,
            hlc: self.hlc.to_string(),
            device_id: self.hlc.device_id.clone(),
            deleted: self.op == OpKind::Delete,
            updated_at,
        })
    }
}

/// 生成 oplog 分片文件名（`"00000.jsonl"` 风格；超过 5 位自然增长）。
pub fn shard_name(index: u32) -> String {
    format!("{index:05}.jsonl")
}

/// 追加写入 oplog 分片（JSONL；文件不存在则创建，父目录不存在则创建）。
pub fn append_jsonl(path: &Path, entries: &[OpEntry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for entry in entries {
        let line = serde_json::to_string(entry)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    Ok(())
}

/// 读取 oplog 分片。
///
/// 损坏行（JSON 解析失败或非 UTF-8）跳过并记录警告；文件不存在时返回空列表
/// （远端尚未上传该分片属正常情况）。
pub fn read_jsonl(path: &Path) -> Result<Vec<OpEntry>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut buf = Vec::new();
    let mut line_no = 0usize;
    loop {
        buf.clear();
        let read = reader.read_until(b'\n', &mut buf)?;
        if read == 0 {
            break;
        }
        line_no += 1;
        let line = match std::str::from_utf8(&buf) {
            Ok(line) => line.trim(),
            Err(_) => {
                warn!("oplog {} 第 {line_no} 行非 UTF-8，已跳过", path.display());
                continue;
            }
        };
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<OpEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                warn!("oplog {} 第 {line_no} 行损坏，已跳过: {e}", path.display());
            }
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<OpEntry> {
        vec![
            OpEntry {
                key: "phrase-01H...".to_string(),
                kind: "phrase".to_string(),
                op: OpKind::Upsert,
                hlc: Hlc::new(1_760_000_000_123, 4, "win-abc"),
                enc: Some("gAAAA...".to_string()),
            },
            OpEntry {
                key: "phrase-01H...".to_string(),
                kind: "phrase".to_string(),
                op: OpKind::Delete,
                hlc: Hlc::new(1_760_000_000_999, 0, "mac-def"),
                enc: None,
            },
        ]
    }

    #[test]
    fn json_field_names_and_order_match_design() {
        let entries = sample_entries();
        let line = serde_json::to_string(&entries[0]).ok().unwrap_or_default();
        assert_eq!(
            line,
            "{\"key\":\"phrase-01H...\",\"kind\":\"phrase\",\"op\":\"upsert\",\
             \"hlc\":\"1760000000123.4.win-abc\",\"enc\":\"gAAAA...\"}"
        );
        // enc 缺省时不出现（E2E 未启用）
        let line = serde_json::to_string(&entries[1]).ok().unwrap_or_default();
        assert!(!line.contains("\"enc\""));
        assert!(line.contains("\"op\":\"delete\""));
    }

    #[test]
    fn append_and_read_roundtrip() {
        let dir = tempfile_dir("roundtrip");
        let path = dir.join("00000.jsonl");
        append_jsonl(&path, &sample_entries())
            .ok()
            .unwrap_or_default();
        // 再追加一轮（模拟滚动写入）
        append_jsonl(&path, &sample_entries())
            .ok()
            .unwrap_or_default();
        let read = read_jsonl(&path).ok().unwrap_or_default();
        assert_eq!(read, [sample_entries(), sample_entries()].concat());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_missing_file_returns_empty() {
        let dir = tempfile_dir("missing");
        let read = read_jsonl(&dir.join("00001.jsonl"))
            .ok()
            .unwrap_or_default();
        assert!(read.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_skips_corrupt_lines() {
        let dir = tempfile_dir("corrupt");
        let path = dir.join("00000.jsonl");
        let mut content = String::new();
        content
            .push_str("{\"key\":\"a\",\"kind\":\"phrase\",\"op\":\"upsert\",\"hlc\":\"1.0.d\"}\n");
        content.push_str("这一行不是 JSON\n");
        content.push('\n'); // 空行跳过
        content
            .push_str("{\"key\":\"b\",\"kind\":\"phrase\",\"op\":\"delete\",\"hlc\":\"2.0.d\"}\n");
        content.push_str("{\"key\":\"c\",\"kind\":\"phrase\",\"op\":\"upsert\"}\n"); // 缺 hlc
        std::fs::write(&path, content).ok();
        let read = read_jsonl(&path).ok().unwrap_or_default();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].key, "a");
        assert_eq!(read[1].key, "b");
        assert_eq!(read[1].op, OpKind::Delete);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn shard_name_format() {
        assert_eq!(shard_name(0), "00000.jsonl");
        assert_eq!(shard_name(1), "00001.jsonl");
        assert_eq!(shard_name(99_999), "99999.jsonl");
        assert_eq!(shard_name(100_000), "100000.jsonl");
    }

    #[test]
    fn entry_adapters_roundtrip() {
        let entries = sample_entries();
        let op = &entries[0];
        let new_entry = op
            .to_new_entry("{\"text\":\"你好\"}".as_bytes().to_vec(), 1_760_000_000_000)
            .expect("转换应成功");
        assert_eq!(new_entry.kind, openkey_data::EntryKind::Phrase);
        assert!(!new_entry.deleted);
        assert_eq!(new_entry.hlc, "1760000000123.4.win-abc");
        // 设备 ID 取 HLC 的写入设备
        assert_eq!(new_entry.device_id, "win-abc");

        let op = &entries[1];
        let new_entry = op
            .to_new_entry(Vec::new(), 1_760_000_000_000)
            .expect("转换应成功");
        assert!(new_entry.deleted);
        assert_eq!(new_entry.device_id, "mac-def");

        // 未知类型报错
        let bad = OpEntry {
            key: "x".to_string(),
            kind: "unknown_kind".to_string(),
            op: OpKind::Upsert,
            hlc: Hlc::new(1, 0, "d"),
            enc: None,
        };
        assert!(bad.to_new_entry(Vec::new(), 0).is_err());
    }

    /// 测试用临时目录（目标目录下唯一子目录，避免并行测试互相干扰）。
    fn tempfile_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("openkey-sync-oplog-{tag}"));
        std::fs::create_dir_all(&dir).ok();
        dir
    }
}
