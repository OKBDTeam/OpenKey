//! openkey-sync：OpenKey 同步引擎（docs/02 3.3）。
//!
//! v0.1.0 交付范围：HLC（混合逻辑时钟）、oplog（JSONL 追加日志）、
//! 条目级合并规则（LWW / 删除胜出 / userdb 频次累加 / 墓碑 GC）、
//! 同步状态机与退避计划、快照分片序列化、`SyncBackend` 抽象与远端目录布局。
//!
//! 待 Phase 3 落地：reqwest WebDAV 适配器（`SyncBackend` 实现）、
//! E2E 加密（6.3）、并发控制（单飞锁）。
//!
//! 设计要点：
//! - HLC 字符串 `"物理毫秒.计数器.deviceId"` 为全局排序键，跨设备比较时
//!   device_id 仅作 tie-break（确定性全序，非因果序）；
//! - 词频（librime userdb）不进 `sync_entries`，同步引擎只搬运其快照文件，
//!   频次按 `a + b - common_ancestor` 累加合并（3.3.3）。

pub mod backend;
pub mod error;
pub mod hlc;
pub mod merge;
pub mod oplog;
pub mod snapshot;
pub mod state;

pub use backend::SyncBackend;
pub use error::{Error, Result};
pub use hlc::{Hlc, HlcClock as HlcClockImpl};
pub use merge::{
    merge_entries, merge_userdb_freq, should_gc_tombstone, ConflictLog, ConflictRecord, KeyedEntry,
    MergeOutcome, CONFLICT_RECOVERY_WINDOW_MS, TOMBSTONE_TTL_MS,
};
pub use oplog::{append_jsonl, read_jsonl, OpEntry, OpKind, SHARD_MAX_BYTES};
pub use snapshot::{read_snapshot, write_snapshot, SnapshotManifest, SnapshotShard};
pub use state::{next, BackoffSchedule, SyncEvent, SyncState};
