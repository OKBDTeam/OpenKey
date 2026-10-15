//! 条目模型（docs/02 3.2）。

use rusqlite::Row;

use crate::error::Result;

/// HLC 时钟接口。由 openkey-sync 提供具体实现；data 层只消费字符串形式的
/// `"物理毫秒.计数器.deviceId"`。
pub trait HlcClock: Send + Sync {
    /// 取一个新的单调递增 HLC 字符串。
    fn tick(&self) -> String;
}

/// 可同步条目类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// 自定义短语。
    Phrase,
    /// 自造词。
    UserWord,
    /// 表情元数据。
    EmojiMeta,
    /// 剪贴板。
    Clipboard,
    /// 设置项。
    Setting,
    /// rime userdb 快照。
    RimeUserDb,
}

impl EntryKind {
    /// 数据库存储值。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Phrase => "phrase",
            Self::UserWord => "user_word",
            Self::EmojiMeta => "emoji_meta",
            Self::Clipboard => "clipboard",
            Self::Setting => "setting",
            Self::RimeUserDb => "rime_userdb",
        }
    }

    /// 主键前缀（`type 前缀 + ULID`）。
    pub fn prefix(&self) -> &'static str {
        match self {
            Self::Phrase => "phrase",
            Self::UserWord => "word",
            Self::EmojiMeta => "emoji",
            Self::Clipboard => "clip",
            Self::Setting => "setting",
            Self::RimeUserDb => "userdb",
        }
    }

    /// 从数据库值解析。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "phrase" => Some(Self::Phrase),
            "user_word" => Some(Self::UserWord),
            "emoji_meta" => Some(Self::EmojiMeta),
            "clipboard" => Some(Self::Clipboard),
            "setting" => Some(Self::Setting),
            "rime_userdb" => Some(Self::RimeUserDb),
            _ => None,
        }
    }
}

impl std::fmt::Display for EntryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 待写入条目（主键由 `upsert` 生成）。
#[derive(Debug, Clone)]
pub struct NewEntry {
    /// 条目类型。
    pub kind: EntryKind,
    /// UTF-8 JSON payload。
    pub payload: Vec<u8>,
    /// HLC 字符串。
    pub hlc: String,
    /// 设备 ID。
    pub device_id: String,
    /// 是否为墓碑。
    pub deleted: bool,
    /// 本地墙钟毫秒。
    pub updated_at: i64,
}

impl NewEntry {
    /// 生成主键：`kind 前缀 + ULID`。
    pub fn generate_key(&self) -> Result<String> {
        Ok(format!("{}-{}", self.kind.prefix(), new_ulid()?))
    }
}

/// 进程级单调 ULID 生成器（ulid 3.x 起由 `Generator` 保证单调）。
fn new_ulid() -> Result<ulid::Ulid> {
    use std::sync::Mutex;
    static GENERATOR: Mutex<ulid::Generator> = Mutex::new(ulid::Generator::new());
    let mut generator = GENERATOR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    generator
        .generate()
        .map_err(|_| crate::error::Error::Corrupt("ULID 随机位耗尽".into()))
}

/// 已存储条目。
#[derive(Debug, Clone)]
pub struct Entry {
    /// 主键。
    pub key: String,
    /// 类型。
    pub kind: EntryKind,
    /// payload（UTF-8 JSON）。
    pub payload: Vec<u8>,
    /// HLC 字符串。
    pub hlc: String,
    /// 最后写入设备。
    pub device_id: String,
    /// 墓碑标记。
    pub deleted: bool,
    /// 本地墙钟毫秒。
    pub updated_at: i64,
}

impl Entry {
    /// 从查询行解析（返回 rusqlite 结果以便 `query_map` 使用）。
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let kind_raw: String = row.get(1)?;
        let kind = EntryKind::parse(&kind_raw).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(crate::error::Error::Corrupt(format!(
                    "未知条目类型: {kind_raw}"
                ))),
            )
        })?;
        Ok(Self {
            key: row.get(0)?,
            kind,
            payload: row.get(2)?,
            hlc: row.get(3)?,
            device_id: row.get(4)?,
            deleted: row.get::<_, i64>(5)? != 0,
            updated_at: row.get(6)?,
        })
    }

    /// payload 的 JSON 视图。
    pub fn payload_json(&self) -> Result<serde_json::Value> {
        Ok(serde_json::from_slice(&self.payload)?)
    }
}
