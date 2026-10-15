# tools/component-pack/ —— 组件包构建（Phase 6 语音组件）

把语音引擎（sherpa-onnx 动态库）与 ASR 模型打包为运行时下载的"语音组件包"。
设计依据：`docs/02-技术设计与实施方案.md` 3.6（openkey-voice）、3.7（组件分发基建）、
DR-004（引擎与模型均不随安装包分发）。

## 产物形态

每个组件包一个 zip + 一个 `manifest.json` + 一个 `.sha256` 文件：

```
voice-engine-android-arm64-1.10.0.zip
voice-engine-android-arm64-1.10.0.manifest.json
voice-engine-android-arm64-1.10.0.zip.sha256
```

`manifest.json` 结构：

```json
{
  "name": "voice-engine-android-arm64",
  "version": "1.10.0",
  "platform": "android",
  "abi": "arm64-v8a",
  "engine": "sherpa-onnx",
  "sherpa_version": "1.10.0",
  "files": [
    { "path": "lib/libsherpa-onnx-c-api.so", "sha256": "..." },
    { "path": "lib/libonnxruntime.so", "sha256": "..." }
  ]
}
```

| 字段 | 说明 |
|---|---|
| `name` / `version` | 组件标识与版本（对应 versions.json 条目名） |
| `platform` / `abi` | 目标平台与 ABI（`android/arm64-v8a`、`linux/x86_64`、`macos/aarch64`、`windows/x86_64`；**iOS 除外**——引擎静态链接随包，DR-004） |
| `engine` / `sherpa_version` | 引擎名与 sherpa-onnx 版本 |
| `files[]` | 包内文件相对路径与逐文件 SHA-256（客户端解压后逐项校验，3.7） |

`.sha256` 文件为整个 zip 的 SHA-256（一行 `<hex>  <文件名>`），供 versions.json 引用。

## 使用方式

构建脚本 `pack.py`（**Phase 6 实现，当前为 TODO**）计划 CLI：

```sh
python3 tools/component-pack/pack.py \
    --name voice-engine-android-arm64 \
    --version 1.10.0 \
    --platform android --abi arm64-v8a \
    --engine sherpa-onnx --sherpa-version 1.10.0 \
    --root <引擎文件目录> --out dist/
```

输出 `dist/voice-engine-android-arm64-1.10.0.{zip,manifest.json}` 与 `....zip.sha256`。

在 `pack.py` 就绪前，可用一行 python3（仅标准库）生成 `files[]` 清单预览哈希：

```sh
python3 -c "import json,hashlib,pathlib; root=pathlib.Path('.'); print(json.dumps({'files':[{'path':str(p.relative_to(root)),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(root.rglob('*')) if p.is_file()]}, indent=2))"
```

打 zip 与整体哈希同理：

```sh
python3 -c "import zipfile,pathlib; zipfile.ZipFile('out.zip','w',zipfile.ZIP_DEFLATED).write  # 略"  # 见 scripts/release/make_versions.py 的打包写法
```

## 与发布流水线的衔接

1. 组件包随 `release.yml` 的产物一起上传（或作为独立 asset）。
2. `package-versions` job 用 `scripts/release/make_versions.py` 生成 `versions.json`，
   引用各组件包的 URL / SHA-256 / size（附录 B 结构，见 `shared/components/README.md`）。
3. 维护者在离线机对 `versions.json` 做 GPG 签名（6.4，签名私钥离线保管；
   CI 只产出未签名 manifest）。
4. 客户端 `ok_component_ensure(name, callback)` 拉取 → SHA-256 校验 → 验签 →
   原子解压到 `components/{name}/{version}/`（3.7）。

## 待办（TODO）

- [ ] `pack.py` 实现（Phase 6）。
- [ ] sherpa-onnx 各平台引擎/模型的构建与版本矩阵（`core/vendor/sherpa-onnx` submodule
      仅用于组件包构建验证，不进主构建，2.2）。
- [ ] iOS 例外落地：引擎静态链接随包，仅模型走组件下载（DR-004 / Apple 2.5.2）。
- [ ] 组件包的体积与许可元数据（首启流程的"组件说明页"展示，5.5）。
