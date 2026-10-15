# OpenKey macOS 客户端（输入法 + 设置 App）

`clients/macos` 是 OpenKey 的 macOS 端实现：系统输入法（`.inputmethod` bundle）+ 设置 App。
输入逻辑全部委托 core（Rust C ABI），本目录只做 IMK 平台适配与 UI。
职责边界铁律见 `docs/02-技术设计与实施方案.md` 2.1：客户端只包含 `openkey.h` 与 core 交互。

## 目录结构

```
clients/macos/
├── project.yml                      # XcodeGen 工程规格（工程名/scheme 名均为 OpenKey）
├── Packages/OpenKeyCore/            # 本地 SwiftPM 包（iOS 工程复用同一路径）
│   ├── Package.swift                #   C 目标 COpenKey + Swift 封装 OpenKeyCore
│   └── Sources/
│       ├── COpenKey/
│       │   ├── include/openkey.h    #     core/ffi-headers/openkey.h 的拷贝（cbindgen 产物，勿手改）
│       │   ├── include/module.modulemap
│       │   └── shim.c               #     占位编译单元
│       └── OpenKeyCore/
│           ├── OpenKeyEngine.swift  #     ok_* C ABI 的类型安全封装（引擎/会话/设置）
│           ├── OpenKeyPaths.swift   #     数据目录约定（~/Library/Application Support/OpenKey）
│           ├── OpenKeySettings.swift#     设置项读写 + 双拼/模糊音方案解析（双端共用）
│           └── X11Keysym.swift      #     按键 → X11 keysym/修饰键映射（core 输入约定）
├── OpenKey.inputmethod/             # 输入法 bundle（安装至 /Library/Input Methods）
│   ├── InputMethodServer.swift      #   NSManualApplication + @main AppDelegate（创建 IMKServer）
│   │                                #   + IMKServer 子类 + 进程共享引擎 EngineProvider
│   ├── InputController.swift        #   IMKInputController 子类：按键管线/上屏/会话生命周期
│   └── CandidatePanel.swift         #   自绘候选条（NSPanel，不用系统 IMKCandidates）
├── OpenKey.app/                     # 设置 App（SwiftUI）
│   ├── OpenKeyApp.swift             #   @main 入口
│   ├── SettingsModel.swift          #   视图模型（懒加载引擎、读写设置）
│   └── SettingsView.swift           #   方案选择/双拼/模糊音/中英切换
├── scripts/
│   └── build-bundle.sh              # 构建 + dylib 嵌入 + 数据部署 + 安装（说明性脚本）
└── README.md
```

## 构建

前置：macOS 13+、Xcode 15+、XcodeGen（`brew install xcodegen`）、Rust stable。

```sh
cd clients/macos
# 方式一：脚本（推荐，含头文件同步 / dylib 嵌入 / rime 数据部署）
scripts/build-bundle.sh            # 追加 --install 安装到 /Library/Input Methods

# 方式二：手动
xcodegen generate
xcodebuild -project OpenKey.xcodeproj -scheme OpenKey \
  -configuration Release -derivedDataPath build CODE_SIGNING_ALLOWED=NO build
```

### core 库准备（core-binding/）

链接目标位于 `core-binding/`（**CI 自动放置**，见下节）：

| 文件 | 用途 |
|---|---|
| `libopenkey_ffi.a` | core 静态库（链接进两个 target） |
| `librime.dylib` | librime 动态库（链接 + 运行期嵌入） |
| `openkey.h` | C ABI 头文件（仅参考；编译期声明用包内拷贝） |

本地无 CI 产物时链接会报 `file not found`（预期行为）。可用 core 构建产物手动放置：

```sh
mkdir -p core-binding
cp core/target/aarch64-apple-darwin/release/libopenkey_ffi.a core-binding/
cp <librime-install>/lib/librime.dylib core-binding/
cp core/ffi-headers/openkey.h core-binding/
```

## 与 CI 的衔接（.github/workflows/ci-clients.yml → macos-client）

CI 在 `macos-14` runner 上执行：`brew install xcodegen` → 构建 librime(arm64) →
构建 core（`--target aarch64-apple-darwin`）→ 放置产物到 `clients/macos/core-binding/` →
`xcodegen generate` → `xcodebuild -scheme OpenKey -configuration Release build`。
因此本工程固定遵守以下约定，改动时需同步 CI：

- `clients/macos/project.yml` 是 CI 的入口守卫文件；
- 工程名 `OpenKey`、scheme 名 `OpenKey`（xcodegen 默认在 project.yml 所在目录生成 `OpenKey.xcodeproj`）；
- target 名 `OpenKey`（设置 App）/ `OpenKeyInputMethod`（输入法），CI 按 `*.app` / `*.inputmethod` 收集产物；
- core 链接经 `OTHER_LDFLAGS` 引用 `core-binding/` 下的文件。

CI 当前**只验证可构建性**（不运行、不签名）；运行期验证见下方 TODO。

## 安装与运行

```sh
scripts/build-bundle.sh --install   # sudo 拷贝到 /Library/Input Methods
```

然后 系统设置 > 键盘 > 输入法 > 编辑 > 添加「OpenKey 输入法」（首次安装按 docs/02 4.2.3 需注销重登）。
调试：输入法是独立后台进程，`print()` 不输出，请用 `NSLog()` 并在「控制台」按进程名过滤。

## 数据目录

两端进程共享 `~/Library/Application Support/OpenKey/`（docs/02 4.2.1）：

```
~/Library/Application Support/OpenKey/
├── shared/      # rime 共享数据（build-bundle.sh 从 shared/schemas 部署）
├── build/       # librime 部署产物
├── userdb/      # 用户词频
└── openkey.db   # 数据层（设置项等；WAL + busy_timeout 保证并发安全）
```

## 已知偏差与设计说明

1. **输入法 target 产品类型是 application 而非 app-extension**（与初步约定的偏差）。
   真实可用的 macOS 输入法（Squirrel / azooKey / BSMInputMethod）均为
   `application + WRAPPER_EXTENSION=inputmethod`：输入法以 `LSBackgroundOnly` 后台应用形态
   被系统拉起，需要可执行入口（`@main AppDelegate`）创建 `IMKServer` 并注册 NSConnection；
   `app-extension` 产品类型的 `NSExtensionPrincipalClass` 机制与 IMK 不兼容（无入口点）。
   产物仍是 `OpenKeyInputMethod.inputmethod`，CI 收集/嵌入逻辑不受影响。
2. **输入法控制器用系统 IMKit（IMKServer + IMKInputController）而非 IMKSwift**
   （docs/02 2.5 选型偏差）。IMKit 是 Apple 官方框架、零第三方依赖；IMKSwift 对
   Swift 6 并发更友好，计划在 Phase 2 评估切换（切换只影响 `OpenKey.inputmethod/` 内文件，
   core 交互层 `OpenKeyCore` 不变）。
3. **预编辑不走 `setMarkedText`**：预编辑只在自绘候选面板展示，宿主内联预编辑留待 Phase 2。
4. **候选面板定位用鼠标位置兜底**：TODO 接 `IMKTextInput` 的 caret 矩形。

## TODO / 不确定项

- [ ] DR-002：Developer ID Application 签名（`codesign --options runtime`）+ notarytool 公证；
      证书经 CI secrets 注入（`APPLE_CERT_BASE64`，ci-clients.yml 已有占位步骤）。
- [ ] entitlements：App Sandbox + `com.appley.security.temporary-exception.mach-register.global-name`
      （值为连接名 `app.openkey.client.inputmethod.OpenKey_Connection`）目前未启用；
      未沙盒化时 IMKServer 的 NSConnection 注册正常（沙盒是注册失败的常见根因）。
- [ ] 输入法实例风暴防护（docs/02 4.2.4：IMKInputController 实例数/会话复用策略、真机回归）。
- [ ] CapsLock / Shift 中英切换（flagsChanged 在部分宿主不送达）。
- [ ] 设置变更热更新（当前需重新激活输入法）；候选备注（comment）展示；翻页快捷键（-/=、左右键）。
- [ ] 密码模式 / 贴图（docs/02 4.2.2）依赖 openkey-vault，Phase 3。
- [ ] 词库在线更新 / userdb 同步调度（docs/02 4.2.1 的驻留位置）。
