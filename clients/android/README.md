# OpenKey Android 客户端

Gradle（KTS）工程：`:app`（主 App）/ `:keyboard`（IME）/ `:core-binding`（openkey-ffi JNI）/ `:voicepack`（语音组件加载器）。
设计依据：`docs/02-技术设计与实施方案.md` 4.4（Android）、DR-004、2.2。

## 模块

| 模块 | 职责 |
|---|---|
| `:app` | 主 App：设置骨架（方案选择 / 中英切换）、输入法启用引导、关于页 |
| `:keyboard` | `OpenKeyIme`（InputMethodService + ComposeView 键盘）：按键 → keysym → core，候选条自绘 |
| `:core-binding` | `openkey.h` 的 JNI 绑定：`cpp/openkey_jni_shim.c`（裸指针 → JSON 拍平）+ Kotlin 封装 |
| `:voicepack` | 语音组件动态加载器骨架（状态机 + 接口，Phase 6 实现下载/校验/`System.load`） |

## 构建

前置：JDK 17、Android SDK（compileSdk 35）、Android NDK（r26/r27，与 AGP 8.7 默认版本兼容；
纯 C shim 不依赖 STL）。CMake 3.22.1（随 AGP 8.7 分发，`externalNativeBuild` 已指定）。
仓库未提交 Gradle wrapper 二进制 jar，首次使用请先生成：

```sh
cd clients/android
gradle wrapper --gradle-version 8.9   # 生成 gradlew / gradlew.bat / gradle-wrapper.jar
./gradlew assembleDebug
```

### core so（必需，否则键盘降级）

CI 或本地需先把 core 交叉编译产物放入 `core-binding/src/main/jniLibs/<abi>/libopenkey_ffi.so`：

```sh
cd core
cargo ndk -t arm64-v8a -t x86_64 \
  -o ../clients/android/core-binding/src/main/jniLibs \
  build -p openkey-ffi --release
```

随后 `:core-binding` 的 Gradle 任务 `syncOpenKeyHeader` 会把仓库根的
`core/ffi-headers/openkey.h`（cbindgen 产物，唯一事实源）拷到
`src/main/cpp/` 供 CMake 编译 `libopenkey_jni_shim.so`。
运行时加载顺序：`System.loadLibrary("openkey_ffi")` → `System.loadLibrary("openkey_jni_shim")`，
shim 中未定义的 `ok_*` 符号由前者在同一命名空间解析（shim 不链接 core so）。

未放置 so 时 `OpenKeyNative.isLoaded == false`，键盘可弹出但输入降级为
"可打印字符直发 + 退格/回车兜底"，设置页给出提示（不崩溃）。

### 目录布局（引擎，IME 与主 App 同 uid 共享）

```
filesDir/openkey/
├── rime/shared/     # librime 共享数据（方案/词库；TODO Phase 1 由 assets/组件填充）
├── rime/user/       # userdb / build
├── components/      # 组件根目录（DR-004）
├── log/
└── openkey.db       # 数据层（openkey-data）
```

## 版本选择（2024-09 前后稳定组合）

| 组件 | 版本 | 理由 |
|---|---|---|
| AGP | 8.7.2 | 稳定线；支持 compileSdk 35 与 Gradle 8.9 |
| Gradle | 8.9 | AGP 8.7 要求的最低版本 |
| Kotlin | 2.0.21 | K2 编译器稳定后的稳妥版本；compose compiler 随 Kotlin 发布 |
| Compose BOM | 2024.09.00 | Compose 1.7.x，配合 Kotlin 2.0.21 的 compose 插件 |
| compileSdk / targetSdk | 35 | 方案基线（docs/02 2.5） |
| minSdk | 26 | AutofillService 需要（docs/02 4.4.3） |
| Java | 17 | AGP 8.x 默认工具链要求 |
| lifecycle-runtime / savedstate | 2.8.6 / 1.2.1 | 为 IME 窗口中的 ComposeView 提供 LifecycleOwner/SavedStateRegistryOwner |

## TODO 清单（按阶段）

- **Phase 1**：`rime/shared` 填充（assets 释放 openkey-base CC0 词库 / 词库中心组件下载 + `ok_engine_deploy`）；双拼/模糊音设置项；候选条符号面板与翻页滑动手势；正式图标；主 App 与 IME 双进程各自创建引擎的用户目录并发协调（建议 Phase 1 收敛为仅 IME 创建引擎、设置页经短时会话读取）。
- **Phase 2**：用户词库/自定义短语同步入口；设置条目 `ok_setting_set/get` 绑定。
- **Phase 3**：云同步（WebDAV）调度；rime userdb 快照导出导入（`ok_engine_sync_*`，需补 shim 转发）。
- **Phase 4**：`OpenKeyAutofillService`（API 26+，inline suggestions）；键盘密码模式（vault 逐字键入，docs/02 4.4.2/5.2）；`passwordField` 检测已就绪。
- **Phase 5**：表情候选注入与 `commitContent` 贴图 / 剪贴板引导路径。
- **Phase 6**：`VoicePackLoader` 下载/校验/解压/`System.load`；`ok_component_*` 的 shim 转发（含进度回调）；键盘麦克风键。
- **持续**：KeysymMap 方向键等按键补齐；主流 ROM（MIUI/ColorOS/HyperOS）兼容验证。
