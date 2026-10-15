//! openkey-data 单元测试（docs/02 3.2 / 7.1）。

use openkey_data::{Database, EntryKind, NewEntry};

fn new_entry(kind: EntryKind, payload: &str, hlc: &str, device: &str) -> NewEntry {
    NewEntry {
        kind,
        payload: payload.as_bytes().to_vec(),
        hlc: hlc.to_string(),
        device_id: device.to_string(),
        deleted: false,
        updated_at: 1_700_000_000_000,
    }
}

#[test]
fn crud_roundtrip() {
    let db = Database::open_in_memory().unwrap();

    let entry = db
        .upsert(&new_entry(
            EntryKind::Phrase,
            r#"{"text":"你好","abbr":"nh"}"#,
            "100.0.dev-a",
            "dev-a",
        ))
        .unwrap();
    assert!(entry.key.starts_with("phrase-"));

    let fetched = db.get(&entry.key).unwrap().expect("应能读回");
    assert_eq!(fetched.kind, EntryKind::Phrase);
    assert_eq!(fetched.payload_json().unwrap()["text"], "你好");
    assert_eq!(fetched.hlc, "100.0.dev-a");

    // 按类型列出（不含墓碑）。
    let phrases = db.list(Some(EntryKind::Phrase), false).unwrap();
    assert_eq!(phrases.len(), 1);

    // 增量查询（oplog 推送语义）。
    let since = db.list_since_hlc("100.0.dev-a", 100).unwrap();
    assert!(since.is_empty(), "HLC 相等不应再出现");
    let since = db.list_since_hlc("099.0.dev-a", 100).unwrap();
    assert_eq!(since.len(), 1);
}

#[test]
fn lww_remote_apply() {
    let db = Database::open_in_memory().unwrap();

    // 本地写入 hlc=200。
    let local = db
        .upsert(&new_entry(
            EntryKind::Setting,
            "\"a\"",
            "200.0.dev-a",
            "dev-a",
        ))
        .unwrap();

    // 远端旧版本（hlc=100）不得覆盖。
    let applied = db
        .apply_remote_entry(
            &local.key,
            &new_entry(EntryKind::Setting, "\"old\"", "100.0.dev-b", "dev-b"),
        )
        .unwrap();
    assert!(!applied, "旧 HLC 不应覆盖");
    assert_eq!(
        db.get(&local.key).unwrap().unwrap().payload_json().unwrap(),
        serde_json::json!("a")
    );

    // 远端新版本（hlc=300）应覆盖。
    let applied = db
        .apply_remote_entry(
            &local.key,
            &new_entry(EntryKind::Setting, "\"new\"", "300.0.dev-b", "dev-b"),
        )
        .unwrap();
    assert!(applied, "新 HLC 应覆盖");
    assert_eq!(
        db.get(&local.key).unwrap().unwrap().payload_json().unwrap(),
        serde_json::json!("new")
    );
}

#[test]
fn tombstone_and_gc() {
    let db = Database::open_in_memory().unwrap();
    let entry = db
        .upsert(&new_entry(
            EntryKind::Clipboard,
            "\"x\"",
            "100.0.dev-a",
            "dev-a",
        ))
        .unwrap();

    assert!(db.tombstone(&entry.key, "200.0.dev-a", "dev-a").unwrap());
    assert!(db.get(&entry.key).unwrap().unwrap().deleted);
    assert!(db.list(None, false).unwrap().is_empty(), "墓碑默认不可见");
    assert_eq!(db.list(None, true).unwrap().len(), 1);

    // 旧 HLC 的重复墓碑不生效。
    assert!(!db.tombstone(&entry.key, "150.0.dev-b", "dev-b").unwrap());

    // GC：仅清除 HLC 早于阈值的墓碑。
    assert_eq!(db.purge_tombstones_before("250.0.zzz").unwrap(), 1);
    assert!(db.list(None, true).unwrap().is_empty());
}

#[test]
fn settings_and_device_identity() {
    let db = Database::open_in_memory().unwrap();

    db.set_setting("emoji.max_edge", &serde_json::json!(400), "10.0.d", "d")
        .unwrap();
    assert_eq!(
        db.get_setting("emoji.max_edge").unwrap(),
        Some(serde_json::json!(400))
    );
    assert_eq!(db.get_setting("absent").unwrap(), None);

    db.set_device_identity("01JINSTALL", "我的 Windows")
        .unwrap();
    let (id, name) = db.device_identity().unwrap().unwrap();
    assert_eq!(id, "01JINSTALL");
    assert_eq!(name, "我的 Windows");
}

#[test]
fn component_registry() {
    let db = Database::open_in_memory().unwrap();
    db.component_register("voice-model-zh-small-int8", "2024.06", "installed", 1)
        .unwrap();
    db.component_set_current("voice-model-zh-small-int8", "2024.06")
        .unwrap();
    assert_eq!(
        db.component_current("voice-model-zh-small-int8").unwrap(),
        Some("2024.06".to_string())
    );
    // 切换指针（回滚 = 指回旧版本）。
    db.component_set_current("voice-model-zh-small-int8", "2023.01")
        .unwrap();
    assert_eq!(
        db.component_current("voice-model-zh-small-int8").unwrap(),
        Some("2023.01".to_string())
    );
    assert_eq!(db.component_current("absent").unwrap(), None);
}

#[test]
fn migration_is_idempotent() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db.migrate().unwrap();
    // 迁移记录表存在且仅一条。
    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM _openkey_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}
