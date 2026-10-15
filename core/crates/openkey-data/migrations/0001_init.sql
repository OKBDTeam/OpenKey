-- OpenKey 初始 schema（docs/02 3.2 / 附录 A 的单一事实源）
-- 正式实施时本文档与设计文档不一致，以本迁移脚本为准。

-- ① 统一条目表：所有可同步数据
CREATE TABLE sync_entries (
  key         TEXT PRIMARY KEY,      -- 条目 UUID（type 前缀 + ULID）
  kind        TEXT NOT NULL,         -- phrase|user_word|emoji_meta|clipboard|setting|rime_userdb
  payload     BLOB NOT NULL,         -- UTF-8 JSON；敏感字段在 JSON 内以 "enc$" 前缀密文呈现
  hlc         TEXT NOT NULL,         -- "物理毫秒.计数器.deviceId"，排序键
  device_id   TEXT NOT NULL,
  deleted     INTEGER NOT NULL DEFAULT 0,   -- 墓碑
  updated_at  INTEGER NOT NULL       -- 本地墙钟，仅供 UI 展示
);
CREATE INDEX idx_entries_kind ON sync_entries(kind, deleted);
CREATE INDEX idx_entries_hlc  ON sync_entries(hlc);

-- ② vault（Bitwarden 模型映射；不直接参与条目同步，走 Bitwarden 服务器协议）
CREATE TABLE vault_items (
  id          TEXT PRIMARY KEY,      -- Bitwarden cipher UUID（本地模式自生成）
  type        INTEGER NOT NULL,      -- 1=login
  name_enc    BLOB NOT NULL,         -- EncString 序列化
  data_enc    BLOB NOT NULL,         -- 加密的登录字段 JSON
  folder_id   TEXT,
  favorite    INTEGER NOT NULL DEFAULT 0,
  revision    TEXT,                  -- Bitwarden revisionDate
  deleted     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE vault_meta (            -- KDF 参数、服务器地址、账户、加密的用户密钥等
  k TEXT PRIMARY KEY, v BLOB NOT NULL
);

-- ④ 组件注册表
CREATE TABLE components_registry (
  name        TEXT NOT NULL,         -- voice-engine / voice-model-zh / lexicon-rime-ice / imagehost-templates
  version     TEXT NOT NULL,
  status      TEXT NOT NULL,         -- installed|downloading|broken
  installed_at INTEGER NOT NULL,
  PRIMARY KEY(name, version)
);
CREATE TABLE components_current (name TEXT PRIMARY KEY, version TEXT NOT NULL);

-- ⑤ 设备注册（本机身份）
CREATE TABLE device_identity (
  id TEXT PRIMARY KEY,               -- installation UUID
  name TEXT NOT NULL                 -- 用户可读设备名
);
