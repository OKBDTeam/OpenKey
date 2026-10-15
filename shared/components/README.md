# shared/components/ —— 组件清单（versions.json）模板与说明

本目录约定 OpenKey 运行时组件分发的清单格式与签名流程。
设计依据：`docs/02-技术设计与实施方案.md` 3.7（组件分发基建）、6.4（组件供应链）、附录 B。

## 清单格式（versions.json，schema = 1）

```json
{
  "schema": 1,
  "published_at": "2026-10-13T00:00:00Z",
  "components": {
    "app-windows":  { "version": "0.1.0", "url": "https://.../OpenKey-0.1.0-windows-x86_64.zip",
                      "sha256": "...", "size": 38000000, "channel": "stable" },
    "core-rust":    { "version": "0.1.0", "url": "https://.../OpenKey-core-0.1.0.zip",
                      "sha256": "...", "size": 12000000, "channel": "stable" },
    "lexicon-openkey-base": { "version": "0.1.0", "url": "https://.../OpenKey-lexicon-0.1.0.zip",
                      "sha256": "...", "size": 22000000, "license": "CC0-1.0",
                      "source": "https://github.com/leic4u/OpenKey", "channel": "stable" }
  }
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `schema` | int | 清单结构版本，当前为 `1` |
| `published_at` | string | ISO 8601 UTC 发布时间 |
| `signature` | string | **PGP 签名**（离线步骤追加，见下） |
| `components.<name>.version` | string | 组件版本 |
| `components.<name>.url` | string | 下载地址（GitHub Releases，可配镜像） |
| `components.<name>.sha256` / `size` | string / int | 完整性与体积 |
| `components.<name>.min_app` | string? | 要求的最低 app 版本（可选） |
| `components.<name>.license` / `source` | string? | 许可与来源（第三方内容必填，如 rime-ice 为 GPL-3.0） |
| `components.<name>.channel` | string | 发布通道，当前固定 `stable` |

组件命名约定（v0.1.0 六类 + 可选词库包）：
`app-windows` / `app-macos` / `app-linux` / `app-android` / `app-ios` / `core-rust` /
`lexicon-openkey-base`；远期按附录 B 增加 `voice-engine-*`、`voice-model-*`、
`lexicon-rime-ice`、`imagehost-templates` 等。

## versions.json 由 CI 生成

`versions.json` **不手工维护**。发版时由 `.github/workflows/release.yml` 的
`package-versions` job 执行：

```sh
python3 scripts/release/make_versions.py \
    --artifacts-dir staging --release-dir dist \
    --tag "$TAG" --repo "${{ github.repository }}"
```

脚本按文件名规则识别组件（`*.deb` → app-linux、`*.apk` → app-android、
`*ios*.zip` → app-ios、`*macos*.zip` → app-macos、`*windows*.zip` / `*.exe` → app-windows、
合并后的 `OpenKey-core-<ver>.zip` → core-rust、`*lexicon*.zip` → lexicon-openkey-base），
计算 SHA-256 / size，并连同 `sha256sums.txt` 一起作为 `release-manifest` artifact 上传，
最终由 `release` job 发布到 GitHub Release。客户端更新检查拉取该 asset
（可配镜像，3.7：镜像与官方 manifest 验签不一致时明确警示降级）。

## GPG 签名流程（docs/02 6.4）—— TODO

**CI 只产出未签名 manifest**（上述脚本不写 `signature` 字段）。签名私钥离线保管，
由维护者在发布前于离线机完成：

```sh
# 1. 从 release-manifest artifact 取回 CI 产出的 versions.json
# 2. 离线机签名（发布私钥不出离线环境）
gpg --armor --detach-sign --local-user <发布密钥 ID> versions.json
# 3. 将签名写入 manifest 的 signature 字段后重新上传（或随 manifest 一同发布）
```

- 客户端内置公钥**指纹**；拉取 manifest 后验签，失败即拒绝并警示（3.7 / 6.4）。
- 发布私钥轮换 / 吊销流程待定（TODO），轮换时需随版本升级客户端内置指纹。

## 待办（TODO）

- [ ] `voice-engine-*` / `voice-model-*` 组件包构建（Phase 6 语音输入，见 `tools/component-pack/`）。
- [ ] `lexicon-rime-ice` 条目（GPL-3.0，来源 github.com/iDvel/rime-ice，运行时导入，DR-001 不默认内置）。
- [ ] `imagehost-templates` 模板包条目（3.8 图床模板引擎）。
- [ ] GPG 发布密钥生成、指纹内置与吊销流程（6.4）。
