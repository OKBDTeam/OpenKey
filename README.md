# OpenKey（开门输入法）

开源五端中文拼音输入法。核心价值主张：**你的输入法，你的服务器**。

- 基础拼音输入开箱即用（librime 引擎 + 自建 CC0 词库）
- 用户数据经用户自持的 WebDAV 服务器多端同步
- Bitwarden 兼容密码管理
- 自定义图片表情（图床链接化 / 贴图）
- 开源语音识别（sherpa-onnx）与可选 AI 纠正润色

客户端整体采用 GPL-3.0；随包内置词库与素材为 CC0 / 自制（见
`THIRD_PARTY_NOTICES.md` 与 docs/02-技术设计与实施方案.md 的 DR-001）。
本仓库与 Bitwarden 无任何关联，对 Bitwarden 协议的兼容属描述性使用。

## 平台

| 平台 | 形态 | 目录 |
|---|---|---|
| Windows | TSF DLL + Server 进程 + 设置程序 | `clients/windows` |
| macOS | IMK 输入法 + 主 App | `clients/macos` |
| Linux | fcitx5 addon | `clients/linux` |
| Android | IME App（+ AutofillService） | `clients/android` |
| iOS | 主 App + KeyboardExt + CredentialProviderExt | `clients/ios` |

## 仓库结构

```
core/        Rust workspace（九 crate）+ librime submodule + C ABI 头文件
clients/     五端原生薄壳（只做平台 UI 与 IME 框架对接）
shared/      词库方案、CC0 内容、组件清单模板、文档
tools/       词库构建、基准语料、组件包构建脚本
.github/      CI / 发布流水线
```

职责边界（docs/02 2.1 铁律）：平台 UI 与 IME 框架代码只存在于 `clients/`；
纯逻辑（协议、算法、数据）只存在于 `core/`；`clients/` 只经由
`core/ffi-headers/openkey.h`（C ABI）与 core 交互。

## 构建

### Core（Rust）

```sh
cd core
cargo test                 # 需要 librime 时设置 LIBRIME_DIR=<安装前缀>
cargo test --no-default-features   # 不链接 librime 的纯逻辑测试
```

Linux/macOS 上 librime 由 `core/vendor/librime`（submodule，锁 1.17.0）构建；
CI 见 `.github/workflows/ci-core.yml`。

### 各端

见 `clients/<平台>/README.md`。

## 文档

- `docs/01-深度调研与技术方案.md` —— 市场与平台调研、选型论证
- `docs/02-技术设计与实施方案.md` —— 架构、协议、五端设计、WBS、风险登记（开发执行依据）

## 版本与发布

版本号语义化；发版由 tag `v*` 触发 `.github/workflows/release.yml`
（构建 → 测试 → 打包 → 生成 `versions.json` → GitHub Release）。
