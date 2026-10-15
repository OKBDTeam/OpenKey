# shared/schemas —— OpenKey 内置方案与词库（DR-001：一律 CC0 / 自制）

本目录内容随客户端安装包分发，是 librime 的共享数据目录（shared data dir）。

## 文件清单

| 文件 | 说明 |
|---|---|
| `default.yaml` | 全局配置：方案选单、翻页、标点、按键绑定（CC0，参考 librime data/minimal 约定编写） |
| `openkey.schema.yaml` | 主方案「开门拼音」（全拼 + 纠错派生） |
| `openkey_fuzzy.schema.yaml` | 模糊音变体（平翘/鼻音/前后鼻音双向派生；v0.1.0 经方案切换启用，见下） |
| `openkey_double_flypy.schema.yaml` | 小鹤双拼 |
| `openkey_double_mspy.schema.yaml` | 微软双拼 |
| `openkey_double_abc.schema.yaml` | 智能 ABC 双拼 |
| `openkey_double_ziranma.schema.yaml` | 自然码双拼 |
| `lexicon/openkey-base.seed.tsv` | CC0 自制种子词库（词条 \t 无声调拼音 \t 词频） |
| `lexicon/openkey-base.dict.yaml` | **生成物**：由 tools/lexicon 从种子构建，rime 部署期编译为 table/prism |
| `openkey_base.manifest.json` | **生成物**：词库包清单（版本/条目数/SHA-256，5.6「标注版本与哈希」） |
| `openkey_base.dict.yaml` | **生成物**：构建时直接输出到本目录（rime 按 config id 在共享目录根解析词典） |

## 构建词库

```sh
cargo run --manifest-path tools/lexicon/Cargo.toml -- build \
    --seed shared/schemas/lexicon/openkey-base.seed.tsv \
    --out shared/schemas --version 0.1.0
cargo run --manifest-path tools/lexicon/Cargo.toml -- check \
    --seed shared/schemas/lexicon/openkey-base.seed.tsv
```

## 双拼与模糊音的启用方式（v0.1.0）

docs/02 5.1 将双拼/模糊音定义为「设置开关」。v0.1.0 的落地方式为**方案切换**：
设置 UI 的「双拼方案」下拉与「模糊音」复选框经 C ABI
`ok_session_select_schema()` 切换到对应方案（`openkey_double_flypy` 等）。
各双拼方案与主方案**共用同一份词库**，经 `translator.prism` 为每个方案构建
独立拼写索引（多方案共用词库时必须指定不同 prism 名）。

- 双拼键位为各方案公开发布的标准键位（事实映射）；algebra 表达由 OpenKey
  自行编写，与全局配置一并以 CC0-1.0 发布。
- 自然码零声母遵循其原始规则（单字母重复、双字母直输、三字母重复首字母）。
- 简繁切换（OpenCC）依赖 OpenCC 配置数据，随词库中心组件提供后启用，
  `openkey.schema.yaml` 中预留了注释开关。

## 词频数据的边界

用户词频（librime userdb）不随本目录分发，也不进 `sync_entries`；
同步引擎只搬运其快照文件（docs/02 3.3.4）。
