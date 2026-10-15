# OpenKey 第三方组件声明

本文件登记 OpenKey 随发行版分发或构建时依赖的第三方组件及其许可证。
依据 DR-001（docs/02-技术设计与实施方案.md 0.2 节）：客户端整体采用 GPL-3.0；
随包内置内容一律 CC0/自制；GPL/AGPL 依赖须经合并审查后方可引入。

## 构建与链接的第三方组件

| 组件 | 版本 | 许可证 | 用途 | 分发方式 |
|---|---|---|---|---|
| librime | 1.17.0 | BSD-3-Clause | 拼音输入引擎（core/vendor/librime submodule） | 随端打包（动态库） |
| Boost | 1.74+ | Boost Software License 1.0 | librime 依赖 | 随 librime 链接 |
| yaml-cpp | 0.7+ | MIT | librime 依赖 | 随 librime 链接 |
| LevelDB | 1.23 | BSD-3-Clause | librime 用户词库存储 | 随 librime 链接 |
| marisa-trie | 0.2.6 | BSD-2-Clause / MIT | librime 依赖 | 随 librime 链接 |
| OpenCC | 1.1+ | Apache-2.0 | 简繁转换（librime 依赖） | 随 librime 链接 |
| glog | 0.6+ | BSD-3-Clause | librime 日志 | 随 librime 链接 |
| SQLite | 3.45（bundled） | Public Domain | openkey-data 存储 | 静态链接（rusqlite bundled） |
| sherpa-onnx | 组件包版本（运行时下载，不进主构建） | Apache-2.0 | 语音识别引擎（Phase 6） | 运行时组件包 |

## Rust 依赖（Cargo 解析，随构建链）

serde / serde_json（MIT OR Apache-2.0）、thiserror（MIT OR Apache-2.0）、
log（MIT OR Apache-2.0）、rusqlite（MIT）、ulid（MIT）、aes / cbc / hmac /
sha2 / pbkdf2（MIT OR Apache-2.0）、argon2（MIT OR Apache-2.0）、
base64（MIT OR Apache-2.0）、rand（MIT OR Apache-2.0）、zeroize（MIT OR
Apache-2.0）、image（MIT）、gif（MIT）、zip（MIT OR Apache-2.0）、reqwest
（MIT OR Apache-2.0）、hex（MIT OR Apache-2.0）、cbindgen（MPL-2.0，仅构建期）。

完整传递依赖清单由各端构建产物中的 lock 文件为准。

## 运行时下载内容（不进安装包）

- 语音引擎与模型（sherpa-onnx，DR-004）：Apache-2.0，用户显式开启后下载。
- rime-ice 等社区词库（DR-001）：GPL-3.0，经词库中心由用户显式导入，不明文内置。
- 图床模板包：OpenKey 自制（CC0）。

## 商标与名称

OpenKey 与 Bitwarden 无任何关联；对 Bitwarden 协议与文件格式的兼容属描述性使用。
Rime / 中州韵为 RIME Developers 的项目，本仓库 vendor 的 librime 以其 BSD-3
许可证分发。
