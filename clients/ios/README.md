# OpenKey iOS 客户端（docs/02 4.5）

三 target 结构：主 App（设置/引导/关于）+ 自定义键盘扩展 + 自动填充扩展，
core 静态库直链键盘扩展（进程内，4.5.2）。

## 组件

| target | 产物 | 职责 |
|---|---|---|
| `OpenKey` | `OpenKey.app` | 设置页（输入方案 / 双拼方案 / 模糊音 / 英文模式，经 `OpenKeySettingsStore` 写 `openkey.db` 的 setting 条目）、启用引导、About；App Group `group.app.openkey.shared` |
| `OpenKeyKeyboard` | `OpenKeyKeyboard.appex` | 自定义键盘：候选条（UICollectionView）+ 程序化布局的字母面板；按键 → X11 keysym → `OpenKeySession.processKey`；直链 core |
| `OpenKeyCredentialProvider` | `OpenKeyCredentialProvider.appex` | 自动填充骨架（Phase 4 接入 vault 后启用） |

`Packages/OpenKeyCore` 复用 `../macos/Packages/OpenKeyCore`（同一份 SwiftPM 包，
避免双份维护）：C 目标 `COpenKey`（`openkey.h` + modulemap）+ Swift 封装
`OpenKeyEngine` / `OpenKeySession` / `OpenKeySettingsStore` / `X11Keysym`。

## 构建

前置：Xcode 15+、XcodeGen、core 静态库（模拟器架构）。

```sh
# 1) 生成 core 静态库（iOS 模拟器；iOS 端 librime 链接方案待定，关闭默认特性）
cd core
rustup target add aarch64-apple-ios-simulator x86_64-apple-ios-simulator
cargo build --release --target aarch64-apple-ios-simulator -p openkey-ffi --no-default-features
cargo build --release --target x86_64-apple-ios-simulator   -p openkey-ffi --no-default-features

# 2) 放置到 clients/ios/core-binding/（CI 的 ios-client job 自动完成）
mkdir -p ../clients/ios/core-binding/aarch64-apple-ios-simulator
mkdir -p ../clients/ios/core-binding/x86_64-apple-ios-simulator
cp target/aarch64-apple-ios-simulator/release/libopenkey_ffi.a ../clients/ios/core-binding/aarch64-apple-ios-simulator/
cp target/x86_64-apple-ios-simulator/release/libopenkey_ffi.a   ../clients/ios/core-binding/x86_64-apple-ios-simulator/
cp ffi-headers/openkey.h ../clients/ios/core-binding/

# 3) 生成工程并构建（模拟器）
cd ../clients/ios
xcodegen generate
xcodebuild -project OpenKey.xcodeproj -scheme OpenKey -configuration Debug \
  -destination 'generic/platform=iOS Simulator' CODE_SIGNING_ALLOWED=NO build
```

CI（`.github/workflows/ci-clients.yml` 的 `ios-client` job）即上述流程，
产物按 `build/Build/Products/*` 收集。

## 词库与方案的打包位置

键盘扩展在**未开启 Full Access** 时读不到主 App 容器，因此方案与词库需随
扩展可读：打包时把 `shared/schemas/`（方案 + `openkey_base.dict.yaml`）放入
App bundle 的 `SharedData/`，扩展以 `Bundle.main/SharedData` 解析。
CI 暂只构建模拟器产物；TestFlight 分发前需补该拷贝步骤（见下方 TODO）。

## 内存预算（4.5.2）

键盘扩展常驻内存目标 ≤40MB。本版本 core 直链扩展、词库由 librime 按需映射；
若真机实测超限，按设计改为「主 App 内 core + App Group 只读共享」。

## 已知限制 / TODO

- iOS 端 librime 链接方案未落地：CI 以 `--no-default-features` 构建，
  模拟器产物可编译但**运行时 `ok_engine_create` 返回 NULL**（输入不可用）；
  真机可用版本需先完成 librime 的 iOS 交叉编译（toolchain file + 依赖交叉构建）；
- Full Access 未开启（`RequestsOpenAccess=false`）：无网络与共享容器访问，
  设置同步（App Group）与词库在线更新留待后续里程碑；
- 自动填充为骨架（Phase 4 接入 `openkey-vault`）；
- App Group 需在开发者账号注册后方可真机/TestFlight 分发（DR-002：iOS 走
  TestFlight，交付排最后）；
- 键盘面板为简化 QWERTY 布局，符号面板、滑动翻页、主题与高度自适应待补。
