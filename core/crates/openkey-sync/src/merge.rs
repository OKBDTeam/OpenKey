//! 条目级合并规则（docs/02 3.3.3）。
//!
//! 合并规则表：
//!
//! | 数据类型 | 规则 |
//! |---|---|
//! | phrase / emoji_meta / setting / clipboard / user_word | 条目级 LWW（HLC 大者胜）；删除 = 墓碑 |
//! | rime_userdb（词频快照） | 频次累加合并（[`merge_userdb_freq`]） |
//! | 同 key 并发删除 vs 修改 | 墓碑 HLC 大者胜出；删除胜出时记录被删内容摘要（[`ConflictLog`]，30 天内可从快照找回） |
//!
//! 墓碑 GC（[`should_gc_tombstone`]）：保留 90 天（[`TOMBSTONE_TTL_MS`]），
//! 且仅当所有已知设备的 `last_seen_hlc` 都超过墓碑时间时才执行。

use log::warn;
use openkey_data::NewEntry;

use crate::hlc::Hlc;

/// 删除胜出冲突记录的找回窗口（30 天，docs/02 3.3.3 / 5.3）。
pub const CONFLICT_RECOVERY_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// 墓碑保留期（90 天，docs/02 3.3.3）。
pub const TOMBSTONE_TTL_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// 参与合并的条目：`openkey_data::NewEntry` 加主键。
#[derive(Debug, Clone)]
pub struct KeyedEntry {
    /// 条目主键。
    pub key: String,
    /// 条目内容（含 HLC / 墓碑标记）。
    pub entry: NewEntry,
}

/// 逐字段比较（`NewEntry` 未派生 `PartialEq`，这里显式实现以供合并结果断言）。
impl PartialEq for KeyedEntry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.entry.kind == other.entry.kind
            && self.entry.payload == other.entry.payload
            && self.entry.hlc == other.entry.hlc
            && self.entry.device_id == other.entry.device_id
            && self.entry.deleted == other.entry.deleted
            && self.entry.updated_at == other.entry.updated_at
    }
}

impl KeyedEntry {
    /// 构造。
    pub fn new(key: impl Into<String>, entry: NewEntry) -> Self {
        Self {
            key: key.into(),
            entry,
        }
    }

    /// 由 openkey-data 已存储条目构造（pull 后合并用）。
    pub fn from_entry(entry: &openkey_data::Entry) -> Self {
        Self {
            key: entry.key.clone(),
            entry: NewEntry {
                kind: entry.kind,
                payload: entry.payload.clone(),
                hlc: entry.hlc.clone(),
                device_id: entry.device_id.clone(),
                deleted: entry.deleted,
                updated_at: entry.updated_at,
            },
        }
    }

    /// 是否为墓碑。
    pub fn is_deleted(&self) -> bool {
        self.entry.deleted
    }

    /// 解析 HLC；损坏时返回 `None`（合并判定因此保守保留本地）。
    fn parsed_hlc(&self) -> Option<Hlc> {
        Hlc::parse(&self.entry.hlc).ok()
    }
}

/// 条目级合并结果。
#[derive(Debug, Clone, PartialEq)]
pub enum MergeOutcome {
    /// 保留本地条目：远端 HLC 全序不大于本地（旧值不覆盖新值）。
    KeepLocal,
    /// 应用远端条目（LWW：远端 HLC 全序更大）。载荷装箱以避免变体大小悬殊
    /// （`clippy::large_enum_variant`）。
    ApplyRemote(Box<ApplyRemotePayload>),
}

/// [`MergeOutcome::ApplyRemote`] 的载荷。
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyRemotePayload {
    /// 应写入本地的条目。
    pub entry: KeyedEntry,
    /// 远端墓碑覆盖本地存活条目时的冲突记录；普通更新为 `None`。
    pub conflict: Option<ConflictRecord>,
}

/// 条目级合并（LWW + 删除胜出）。
///
/// - 本地不存在：直接采用远端（含远端为墓碑的情形，由调用方落墓碑）；
/// - 远端 HLC 全序不大于本地：保留本地；
/// - 远端 HLC 更大：采用远端；若远端为墓碑且本地为存活条目
///   （同 key 并发删除 vs 修改），删除胜出并附带冲突记录。
///
/// HLC 相同的两条目视为同一事件（同设备同计数器）或确定性 tie-break
/// （跨设备按 `device_id` 字典序，见 [`Hlc`] 文档），均不产生新写入。
/// 任一条目 HLC 损坏时保守保留本地并记录警告。
pub fn merge_entries(local: Option<&KeyedEntry>, remote: &KeyedEntry) -> MergeOutcome {
    let Some(local) = local else {
        return MergeOutcome::ApplyRemote(Box::new(ApplyRemotePayload {
            entry: remote.clone(),
            conflict: None,
        }));
    };
    let (Some(local_hlc), Some(remote_hlc)) = (local.parsed_hlc(), remote.parsed_hlc()) else {
        warn!(
            "合并判定保守保留本地：条目 {} 的 HLC 损坏（local={:?}, remote={:?}）",
            remote.key, local.entry.hlc, remote.entry.hlc
        );
        return MergeOutcome::KeepLocal;
    };
    if remote_hlc <= local_hlc {
        return MergeOutcome::KeepLocal;
    }
    let conflict = if remote.is_deleted() && !local.is_deleted() {
        Some(ConflictRecord::from_keyed(local, &local_hlc))
    } else {
        None
    };
    MergeOutcome::ApplyRemote(Box::new(ApplyRemotePayload {
        entry: remote.clone(),
        conflict,
    }))
}

/// 冲突记录：删除胜出时被删内容的摘要（docs/02 3.3.3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRecord {
    /// 被删条目主键。
    pub key: String,
    /// 被删条目类型。
    pub kind: String,
    /// 被删条目的 HLC。
    pub hlc: Hlc,
    /// 被删内容摘要：payload 前 64 字符（按字符截断，不破坏 UTF-8）。
    pub summary: String,
}

impl ConflictRecord {
    /// 由被删条目生成冲突记录。
    pub fn from_keyed(entry: &KeyedEntry, hlc: &Hlc) -> Self {
        Self {
            key: entry.key.clone(),
            kind: entry.entry.kind.as_str().to_string(),
            hlc: hlc.clone(),
            summary: summarize_payload(&entry.entry.payload),
        }
    }
}

/// 内容摘要：payload 前 64 字符（按字符截断，保证不破坏 UTF-8 序列）。
pub fn summarize_payload(payload: &[u8]) -> String {
    match std::str::from_utf8(payload) {
        Ok(text) => text.chars().take(64).collect(),
        Err(_) => format!("<非 UTF-8 payload，{} 字节>", payload.len()),
    }
}

/// 冲突日志：记录删除胜出事件，支持 30 天内找回（docs/02 3.3.3 / 5.3）。
///
/// "找回"的语义：被删内容的**摘要**记录于此；完整内容在删除前的全量快照中
/// （快照按 24h / oplog > 4MB 惰性产出，默认保留覆盖 30 天窗口），
/// 由调用方据 `key` 与 HLC 从快照恢复。本类型只负责记录的留存与窗口过滤。
#[derive(Debug, Default)]
pub struct ConflictLog {
    records: Vec<ConflictRecord>,
}

impl ConflictLog {
    /// 空日志。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条冲突记录。
    pub fn record(&mut self, record: ConflictRecord) {
        self.records.push(record);
    }

    /// 全部记录（含已超出找回窗口的）。
    pub fn records(&self) -> &[ConflictRecord] {
        &self.records
    }

    /// 仍处于 30 天找回窗口内的记录（按记录顺序，即时间正序）。
    pub fn recoverable(&self, now: &Hlc) -> Vec<&ConflictRecord> {
        self.records
            .iter()
            .filter(|record| within_window(&record.hlc, now))
            .collect()
    }

    /// 按 key 查找仍在窗口内的冲突记录（供"同步日志"页 30 天找回）；
    /// 同一 key 多次被删时返回最近一条。
    pub fn find(&self, key: &str, now: &Hlc) -> Option<&ConflictRecord> {
        self.records
            .iter()
            .rev()
            .find(|record| record.key == key && within_window(&record.hlc, now))
    }

    /// 清理超出找回窗口的记录，返回清理条数。
    pub fn prune(&mut self, now: &Hlc) -> usize {
        let before = self.records.len();
        self.records
            .retain(|record| within_window(&record.hlc, now));
        before - self.records.len()
    }
}

/// 记录时间是否仍处于 30 天找回窗口内。
fn within_window(record_hlc: &Hlc, now: &Hlc) -> bool {
    now.physical_ms.saturating_sub(record_hlc.physical_ms) <= CONFLICT_RECOVERY_WINDOW_MS
}

/// rime userdb 频次累加合并（docs/02 3.3.3，Rime 快照合并语义）：
/// `freq(merged) = freq(A) + freq(B) - freq(共同祖先)`。
///
/// 共同祖先取双方快照中同 installation 的上一版。使用饱和运算：
/// 祖先频次异常大于单侧频次时结果饱和到 0（而非下溢），两侧频次和溢出时
/// 饱和到 `u64::MAX`。
pub fn merge_userdb_freq(a: u64, b: u64, common_ancestor: u64) -> u64 {
    // u128 中间量：避免 a+b 在 u64 内饱和后丢失被减数信息
    // （如 MAX + MAX - MAX 应得 MAX 而非 0）。
    let sum = a as u128 + b as u128;
    let result = sum.saturating_sub(common_ancestor as u128);
    result.min(u64::MAX as u128) as u64
}

/// 墓碑 GC 判定（docs/02 3.3.3）。
///
/// 两个条件同时满足才可 GC：
/// 1. 墓碑已超过 [`TOMBSTONE_TTL_MS`]（90 天）；
/// 2. `all_devices_seen` 中所有已知设备的 `last_seen_hlc` 都超过墓碑 HLC
///    （保证没有设备仍持有"该条目已删除"的旧视图而需要墓碑来传达删除）。
///
/// 已知设备列表为空时条件 2 自然成立（没有设备需要墓碑传达删除）。
pub fn should_gc_tombstone(hlc: &Hlc, now: &Hlc, all_devices_seen: &[(String, Hlc)]) -> bool {
    let age = now.physical_ms.saturating_sub(hlc.physical_ms);
    if age < TOMBSTONE_TTL_MS {
        return false;
    }
    all_devices_seen
        .iter()
        .all(|(_, last_seen)| last_seen > hlc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openkey_data::EntryKind;

    fn new_entry(kind: EntryKind, hlc: &str, deleted: bool) -> NewEntry {
        NewEntry {
            kind,
            payload: b"{\"text\":\"payload\"}".to_vec(),
            hlc: hlc.to_string(),
            device_id: "dev-a".to_string(),
            deleted,
            updated_at: 0,
        }
    }

    fn keyed(key: &str, kind: EntryKind, hlc: &str, deleted: bool) -> KeyedEntry {
        KeyedEntry::new(key, new_entry(kind, hlc, deleted))
    }

    #[test]
    fn local_missing_takes_remote() {
        let remote = keyed("phrase-1", EntryKind::Phrase, "100.0.dev-b", false);
        let outcome = merge_entries(None, &remote);
        assert_eq!(
            outcome,
            MergeOutcome::ApplyRemote(Box::new(ApplyRemotePayload {
                entry: remote,
                conflict: None,
            }))
        );
    }

    #[test]
    fn older_remote_does_not_overwrite() {
        let local = keyed("phrase-1", EntryKind::Phrase, "200.0.dev-a", false);
        let remote = keyed("phrase-1", EntryKind::Phrase, "100.0.dev-b", false);
        assert_eq!(
            merge_entries(Some(&local), &remote),
            MergeOutcome::KeepLocal
        );
    }

    #[test]
    fn equal_hlc_tiebreak_and_same_device() {
        // 跨设备 HLC 相同：按 Ord 的 device_id tie-break，dev-b 全序更大，采用远端
        let local = keyed("phrase-1", EntryKind::Phrase, "100.5.dev-a", false);
        let remote = keyed("phrase-1", EntryKind::Phrase, "100.5.dev-b", false);
        match merge_entries(Some(&local), &remote) {
            MergeOutcome::ApplyRemote(payload) => {
                assert_eq!(payload.entry, remote);
                assert!(payload.conflict.is_none());
            }
            MergeOutcome::KeepLocal => panic!("跨设备 tie-break 应采用远端"),
        }
        // 同设备同 HLC（同一事件）：保留本地，不产生新写入
        let local2 = keyed("phrase-1", EntryKind::Phrase, "100.5.dev-b", false);
        assert_eq!(
            merge_entries(Some(&local2), &remote),
            MergeOutcome::KeepLocal
        );
    }

    #[test]
    fn newer_remote_overwrites() {
        let local = keyed("phrase-1", EntryKind::Phrase, "100.0.dev-a", false);
        let remote = keyed("phrase-1", EntryKind::Phrase, "200.0.dev-b", false);
        let outcome = merge_entries(Some(&local), &remote);
        match outcome {
            MergeOutcome::ApplyRemote(payload) => {
                assert_eq!(payload.entry, remote);
                assert!(payload.conflict.is_none());
            }
            MergeOutcome::KeepLocal => panic!("远端更新应被采用"),
        }
    }

    #[test]
    fn concurrent_delete_vs_modify_delete_wins_with_conflict() {
        let local = keyed("phrase-1", EntryKind::Phrase, "100.0.dev-a", false);
        let remote = keyed("phrase-1", EntryKind::Phrase, "200.0.dev-b", true);
        let outcome = merge_entries(Some(&local), &remote);
        match outcome {
            MergeOutcome::ApplyRemote(payload) => {
                assert!(payload.entry.is_deleted());
                let conflict = payload.conflict.expect("删除胜出应记录冲突");
                assert_eq!(conflict.key, "phrase-1");
                assert_eq!(conflict.kind, "phrase");
                assert_eq!(conflict.hlc, Hlc::new(100, 0, "dev-a"));
                assert_eq!(conflict.summary, "{\"text\":\"payload\"}");
            }
            MergeOutcome::KeepLocal => panic!("删除应胜出"),
        }
    }

    #[test]
    fn delete_with_older_hlc_loses_to_modification() {
        // 墓碑 HLC 不大于本地修改：修改胜出（LWW）
        let local = keyed("phrase-1", EntryKind::Phrase, "300.0.dev-a", false);
        let remote = keyed("phrase-1", EntryKind::Phrase, "200.0.dev-b", true);
        assert_eq!(
            merge_entries(Some(&local), &remote),
            MergeOutcome::KeepLocal
        );
    }

    #[test]
    fn newer_modify_resurrects_tombstone() {
        let local = keyed("phrase-1", EntryKind::Phrase, "100.0.dev-a", true);
        let remote = keyed("phrase-1", EntryKind::Phrase, "200.0.dev-b", false);
        let outcome = merge_entries(Some(&local), &remote);
        match outcome {
            MergeOutcome::ApplyRemote(payload) => {
                assert!(!payload.entry.is_deleted());
                assert!(payload.conflict.is_none());
            }
            MergeOutcome::KeepLocal => panic!("新修改应复活条目"),
        }
    }

    #[test]
    fn tombstone_over_tombstone_no_conflict() {
        let local = keyed("phrase-1", EntryKind::Phrase, "100.0.dev-a", true);
        let remote = keyed("phrase-1", EntryKind::Phrase, "200.0.dev-b", true);
        let outcome = merge_entries(Some(&local), &remote);
        match outcome {
            MergeOutcome::ApplyRemote(payload) => assert!(payload.conflict.is_none()),
            MergeOutcome::KeepLocal => panic!("新墓碑应被采用"),
        }
    }

    #[test]
    fn corrupt_hlc_keeps_local_conservatively() {
        let local = keyed("phrase-1", EntryKind::Phrase, "not-an-hlc", false);
        let remote = keyed("phrase-1", EntryKind::Phrase, "200.0.dev-b", false);
        assert_eq!(
            merge_entries(Some(&local), &remote),
            MergeOutcome::KeepLocal
        );
    }

    #[test]
    fn summary_truncates_to_64_chars() {
        let long = "汉".repeat(100);
        let entry = KeyedEntry::new(
            "phrase-1",
            NewEntry {
                payload: long.clone().into_bytes(),
                ..new_entry(EntryKind::Phrase, "100.0.dev-a", false)
            },
        );
        let summary = summarize_payload(&entry.entry.payload);
        assert_eq!(summary.chars().count(), 64);
        assert!(long.starts_with(&summary));
    }

    #[test]
    fn merge_userdb_freq_additive_with_ancestor() {
        // 频次累加：A + B - 共同祖先
        assert_eq!(merge_userdb_freq(10, 8, 5), 13);
        assert_eq!(merge_userdb_freq(0, 0, 0), 0);
        assert_eq!(merge_userdb_freq(5, 3, 0), 8);
        // 饱和减：祖先异常大于单侧频次时不发生下溢
        assert_eq!(merge_userdb_freq(2, 1, 5), 0);
        // 饱和加：和溢出时不发生回绕
        assert_eq!(merge_userdb_freq(u64::MAX, 1, 0), u64::MAX);
        assert_eq!(merge_userdb_freq(u64::MAX, u64::MAX, u64::MAX), u64::MAX);
    }

    #[test]
    fn tombstone_gc_requires_ttl_and_all_devices_seen() {
        let day_ms = 24 * 60 * 60 * 1000i64;
        let tomb = Hlc::new(50 * day_ms, 0, "dev-a");
        let now_200d = Hlc::new(200 * day_ms, 0, "dev-a");
        let now_60d = Hlc::new(60 * day_ms, 0, "dev-a");
        // last_seen 在墓碑之后（100d > 50d）与之前（10d < 50d）各一
        let seen_after = (String::from("dev-b"), Hlc::new(100 * day_ms, 0, "dev-b"));
        let seen_before = (String::from("dev-b"), Hlc::new(10 * day_ms, 0, "dev-b"));

        // 未满 90 天：不 GC
        assert!(!should_gc_tombstone(
            &tomb,
            &now_60d,
            std::slice::from_ref(&seen_after)
        ));
        // 满 90 天但仍有设备未越过墓碑：不 GC
        assert!(!should_gc_tombstone(&tomb, &now_200d, &[seen_before]));
        // 满 90 天且所有设备已越过：GC（含一个恰好在墓碑后 90 天才被看到的设备）
        assert!(should_gc_tombstone(
            &tomb,
            &now_200d,
            std::slice::from_ref(&seen_after)
        ));
        let seen_exactly_ttl = (String::from("dev-c"), Hlc::new(140 * day_ms, 0, "dev-c"));
        assert!(should_gc_tombstone(
            &tomb,
            &now_200d,
            &[seen_after.clone(), seen_exactly_ttl]
        ));
        // 无已知设备：条件自然成立
        assert!(should_gc_tombstone(&tomb, &now_200d, &[]));
        // 时钟异常（now 早于墓碑）：不 GC
        assert!(!should_gc_tombstone(&now_60d, &tomb, &[seen_after]));
    }

    #[test]
    fn conflict_log_window_and_find() {
        let day_ms = 24 * 60 * 60 * 1000i64;
        let mut log = ConflictLog::new();
        let old = Hlc::new(0, 0, "dev-a");
        let recent = Hlc::new(20 * day_ms, 0, "dev-a");
        log.record(ConflictRecord {
            key: "phrase-1".to_string(),
            kind: "phrase".to_string(),
            hlc: old.clone(),
            summary: "旧删除".to_string(),
        });
        log.record(ConflictRecord {
            key: "phrase-2".to_string(),
            kind: "phrase".to_string(),
            hlc: recent.clone(),
            summary: "新删除".to_string(),
        });
        let now_20d = Hlc::new(20 * day_ms, 0, "dev-a");
        let now_31d = Hlc::new(31 * day_ms, 0, "dev-a");

        // 20 天时两条都在窗口内
        assert_eq!(log.recoverable(&now_20d).len(), 2);
        // 31 天时旧记录超出窗口
        let recoverable = log.recoverable(&now_31d);
        assert_eq!(recoverable.len(), 1);
        assert_eq!(recoverable[0].key, "phrase-2");

        // find：窗口内可按 key 找回
        assert!(log.find("phrase-1", &now_20d).is_some());
        assert!(log.find("phrase-1", &now_31d).is_none());
        assert!(log.find("phrase-2", &now_31d).is_some());

        // prune 清理超窗记录
        assert_eq!(log.prune(&now_31d), 1);
        assert_eq!(log.records().len(), 1);
    }

    #[test]
    fn keyed_entry_from_data_entry() {
        let entry = openkey_data::Entry {
            key: "phrase-01H".to_string(),
            kind: EntryKind::Phrase,
            payload: b"{}".to_vec(),
            hlc: "100.0.dev-a".to_string(),
            device_id: "dev-a".to_string(),
            deleted: true,
            updated_at: 42,
        };
        let keyed = KeyedEntry::from_entry(&entry);
        assert_eq!(keyed.key, "phrase-01H");
        assert!(keyed.is_deleted());
        assert_eq!(keyed.entry.kind, EntryKind::Phrase);
        assert_eq!(keyed.entry.updated_at, 42);
    }
}
