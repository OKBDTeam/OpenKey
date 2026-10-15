//! OpenKey 数据层：SQLite 打开与迁移、六类可同步条目 CRUD、组件注册表。
//!
//! 对应 docs/02 3.2。词频数据（librime userdb）不进 `sync_entries`，
//! 同步引擎只搬运其快照文件（kind=`rime_userdb`）。
//!
//! 加密算法本身不在此 crate：敏感字段由调用方（openkey-vault / 客户端平台层）
//! 以 `"enc$" 前缀密文写入 payload，本层只负责落盘。

pub mod entry;
pub mod error;
pub mod migrate;
use std::path::Path;

use rusqlite::Connection;

pub use crate::entry::{Entry, EntryKind, HlcClock, NewEntry};
use crate::error::{Error, Result};

/// `openkey.db` 数据库句柄。
pub struct Database {
    conn: Connection,
}

impl Database {
    /// 打开（或创建）数据库并执行待应用迁移。
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        Self::configure(conn)
    }

    /// 打开内存数据库（测试用）。
    pub fn open_in_memory() -> Result<Self> {
        Self::configure(Connection::open_in_memory()?)
    }

    fn configure(conn: Connection) -> Result<Self> {
        // docs/02 4.2.1：WAL 模式 + busy_timeout 保证多进程并发安全。
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000i64)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// 应用待执行迁移。
    pub fn migrate(&self) -> Result<()> {
        migrate::migrate(&self.conn)
    }

    /// 内部连接引用（高级用法）。
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 插入或更新条目（自动生成 `kind` 前缀 + ULID 主键）。
    pub fn upsert(&self, entry: &NewEntry) -> Result<Entry> {
        let key = entry.generate_key()?;
        self.upsert_with_key(&key, entry)?;
        self.get(&key)?.ok_or(Error::NotFound(key))
    }

    /// 以指定主键无条件写入（本地新建/本地更新；同步回放请用
    /// [`Database::apply_remote_entry`] 的 LWW 条件）。
    pub fn upsert_with_key(&self, key: &str, entry: &NewEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_entries (key, kind, payload, hlc, device_id, deleted, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(key) DO UPDATE SET
               kind = excluded.kind,
               payload = excluded.payload,
               hlc = excluded.hlc,
               device_id = excluded.device_id,
               deleted = excluded.deleted,
               updated_at = excluded.updated_at",
            rusqlite::params![
                key,
                entry.kind.as_str(),
                entry.payload,
                entry.hlc,
                entry.device_id,
                entry.deleted as i64,
                entry.updated_at,
            ],
        )?;
        Ok(())
    }

    /// 同步回放写入（docs/02 3.3.3 条目级 LWW）：仅当传入 HLC 大于已存储
    /// HLC 时才应用；返回是否实际写入。
    pub fn apply_remote_entry(&self, key: &str, entry: &NewEntry) -> Result<bool> {
        let affected = self.conn.execute(
            "INSERT INTO sync_entries (key, kind, payload, hlc, device_id, deleted, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(key) DO UPDATE SET
               kind = excluded.kind,
               payload = excluded.payload,
               hlc = excluded.hlc,
               device_id = excluded.device_id,
               deleted = excluded.deleted,
               updated_at = excluded.updated_at
             WHERE excluded.hlc > sync_entries.hlc",
            rusqlite::params![
                key,
                entry.kind.as_str(),
                entry.payload,
                entry.hlc,
                entry.device_id,
                entry.deleted as i64,
                entry.updated_at,
            ],
        )?;
        Ok(affected > 0)
    }

    /// 读取单个条目。
    pub fn get(&self, key: &str) -> Result<Option<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, kind, payload, hlc, device_id, deleted, updated_at
             FROM sync_entries WHERE key = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Entry::from_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// 列出条目。`kind` 为空表示全部；`include_deleted` 控制是否包含墓碑。
    pub fn list(&self, kind: Option<EntryKind>, include_deleted: bool) -> Result<Vec<Entry>> {
        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match (kind, include_deleted) {
            (Some(kind), true) => (
                "SELECT key, kind, payload, hlc, device_id, deleted, updated_at
                 FROM sync_entries WHERE kind = ?1 ORDER BY hlc"
                    .to_string(),
                vec![Box::new(kind.as_str().to_string())],
            ),
            (Some(kind), false) => (
                "SELECT key, kind, payload, hlc, device_id, deleted, updated_at
                 FROM sync_entries WHERE kind = ?1 AND deleted = 0 ORDER BY hlc"
                    .to_string(),
                vec![Box::new(kind.as_str().to_string())],
            ),
            (None, true) => (
                "SELECT key, kind, payload, hlc, device_id, deleted, updated_at
                 FROM sync_entries ORDER BY hlc"
                    .to_string(),
                vec![],
            ),
            (None, false) => (
                "SELECT key, kind, payload, hlc, device_id, deleted, updated_at
                 FROM sync_entries WHERE deleted = 0 ORDER BY hlc"
                    .to_string(),
                vec![],
            ),
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), Entry::from_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// 列出 HLC 大于给定值的条目（oplog 增量推送用）。
    pub fn list_since_hlc(&self, hlc: &str, limit: usize) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, kind, payload, hlc, device_id, deleted, updated_at
             FROM sync_entries WHERE hlc > ?1 ORDER BY hlc LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![hlc, limit as i64], Entry::from_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// 打墓碑（软删除）。仅当新 HLC 更大时生效；返回是否实际删除。
    pub fn tombstone(&self, key: &str, hlc: &str, device_id: &str) -> Result<bool> {
        let affected = self.conn.execute(
            "UPDATE sync_entries SET deleted = 1, hlc = ?2, device_id = ?3, updated_at = ?4
             WHERE key = ?1 AND (deleted = 0 OR ?2 > hlc)",
            rusqlite::params![key, hlc, device_id, now_ms()],
        )?;
        Ok(affected > 0)
    }

    /// 物理清除墓碑（GC，docs/02 3.3.3：保留 90 天后）。
    pub fn purge_tombstones_before(&self, hlc: &str) -> Result<usize> {
        let affected = self.conn.execute(
            "DELETE FROM sync_entries WHERE deleted = 1 AND hlc < ?1",
            rusqlite::params![hlc],
        )?;
        Ok(affected)
    }

    // ---- 设置条目便捷方法（kind = setting）----

    /// 写入设置项（JSON 值序列化后入 payload）。
    pub fn set_setting(
        &self,
        key: &str,
        value: &serde_json::Value,
        hlc: &str,
        device_id: &str,
    ) -> Result<Entry> {
        let payload = serde_json::to_vec(value)?;
        // 设置条目主键稳定为 setting-<key>，保证同键覆盖。
        let entry_key = format!("setting-{key}");
        self.upsert_with_key(
            &entry_key,
            &NewEntry {
                kind: EntryKind::Setting,
                payload,
                hlc: hlc.to_string(),
                device_id: device_id.to_string(),
                deleted: false,
                updated_at: now_ms(),
            },
        )?;
        self.get(&entry_key)?.ok_or(Error::NotFound(entry_key))
    }

    /// 读取设置项。
    pub fn get_setting(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let entry_key = format!("setting-{key}");
        let Some(entry) = self.get(&entry_key)? else {
            return Ok(None);
        };
        if entry.deleted {
            return Ok(None);
        }
        Ok(Some(serde_json::from_slice(&entry.payload)?))
    }

    // ---- 设备身份 ----

    /// 读取本机设备身份。
    pub fn device_identity(&self) -> Result<Option<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM device_identity LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        } else {
            Ok(None)
        }
    }

    /// 写入本机设备身份。
    pub fn set_device_identity(&self, id: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO device_identity (id, name) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            rusqlite::params![id, name],
        )?;
        Ok(())
    }

    // ---- 组件注册表（docs/02 3.2 ④）----

    /// 登记组件版本。
    pub fn component_register(
        &self,
        name: &str,
        version: &str,
        status: &str,
        installed_at: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO components_registry (name, version, status, installed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name, version) DO UPDATE SET
               status = excluded.status, installed_at = excluded.installed_at",
            rusqlite::params![name, version, status, installed_at],
        )?;
        Ok(())
    }

    /// 设置组件当前版本指针（原子切换/回滚的关键）。
    pub fn component_set_current(&self, name: &str, version: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO components_current (name, version) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET version = excluded.version",
            rusqlite::params![name, version],
        )?;
        Ok(())
    }

    /// 查询组件当前版本。
    pub fn component_current(&self, name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT version FROM components_current WHERE name = ?1")?;
        let mut rows = stmt.query(rusqlite::params![name])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get::<_, String>(0)?))
        } else {
            Ok(None)
        }
    }
}

/// 当前墙钟毫秒（仅供 UI 展示与安装时间记录）。
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
