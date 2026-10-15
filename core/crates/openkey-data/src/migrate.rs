//! 迁移执行器：`migrations/` 下按文件名排序应用，
//! 以 `_openkey_migrations` 表记录已应用版本（docs/02 附录 A：
//! `migrations/0001_init.sql` 为 schema 单一事实源）。

use rusqlite::Connection;

use crate::error::{Error, Result};

/// 应用所有待执行迁移。
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _openkey_migrations (
           name TEXT PRIMARY KEY,
           applied_at INTEGER NOT NULL
         )",
    )?;

    let mut applied: Vec<String> = conn
        .prepare("SELECT name FROM _openkey_migrations")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut entries: Vec<(String, String)> = Vec::new();
    for name in MIGRATIONS {
        if applied.iter().any(|a| a == name) {
            continue;
        }
        let sql =
            migration_sql(name).ok_or_else(|| Error::Migration(format!("迁移文件缺失: {name}")))?;
        entries.push((name.to_string(), sql));
    }

    for (name, sql) in entries {
        log::info!("应用迁移 {name}");
        conn.execute_batch(&sql)?;
        conn.execute(
            "INSERT INTO _openkey_migrations (name, applied_at) VALUES (?1, ?2)",
            rusqlite::params![name, crate::now_ms()],
        )?;
        applied.push(name);
    }
    Ok(())
}

/// 已排序的迁移文件清单。
pub const MIGRATIONS: &[&str] = &["0001_init.sql"];

/// 取迁移 SQL（编译期内嵌）。
pub fn migration_sql(name: &str) -> Option<String> {
    match name {
        "0001_init.sql" => Some(include_str!("../migrations/0001_init.sql").to_string()),
        _ => None,
    }
}
