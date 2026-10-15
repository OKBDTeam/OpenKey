# OpenKey Windows 客户端（docs/02 4.1）

TSF 输入法 + Server 常驻进程 + 设置程序，经命名管道（`\\.\pipe\openkey`）通信，
全部输入逻辑在 core（`core/ffi-headers/openkey.h`，C ABI）。

## 组件

| 目标 | 产物 | 职责 |
|---|---|---|
| `OpenKeyCoreClient` | 静态库 | `openkey.h` 的 C++ RAII 封装（`openkey_client.h`）、命名管道客户端与协议（`pipe_protocol.h` 紧凑文本行协议，无第三方 JSON 依赖）、Windows VK → X11 keysym 映射 |
| `OpenKeyTsf` | `OpenKeyTsf.dll` | TSF 文本服务：`ITfTextInputProcessorEx` + 键事件接收 → 管道转发 Server → `ITfRange` 更新预编辑、GDI 候选窗（跟随 caret、DPI aware）、CTF TIP 注册（`DllRegisterServer`） |
| `OpenKeyServer` | `OpenKeyServer.exe` | 常驻进程：单实例互斥体 `Global\OpenKeyServer`，持有 core 引擎与会话表，命名管道服务循环（每连接一线程，引擎互斥串行化），GitHub Releases 更新检查（WinHTTP，一期提示手动下载） |
| `OpenKeySetup` | `OpenKeySetup.exe` | 设置窗口：输入方案选择（主方案/模糊音/四套双拼，经方案切换落地）、英文模式开关 |

## 构建

前置：Visual Studio 2022（VC 工具链）、CMake ≥ 3.20、core 静态库与 librime 导入库。

```bat
rem 1) 构建 core（链接 librime；Windows 的 librime 由 librime 官方 build.bat 构建）
cd core
cargo build --release -p openkey-ffi

rem 2) 配置与构建客户端（按实际路径调整）
cmake -S clients\windows -B build -G "Visual Studio 17 2022" -A x64 ^
      -DOPENKEY_CORE_LIB=%CD%\target\release\openkey_ffi.lib ^
      -DOPENKEY_RIME_LIB=<librime>\dist\lib\rime.lib ^
      -DOPENKEY_CORE_INCLUDE_DIR=%CD%\ffi-headers
cmake --build build --config Release
```

CMake 变量缺失时报 `FATAL_ERROR`；`OPENKEY_RIME_LIB` 可从环境变量 `LIBRIME_DIR`
推导（`$LIBRIME_DIR/dist/lib/rime.lib`，librime 官方 Windows 构建布局）。

## 安装与注册

1. 将 `OpenKeyTsf.dll`、`OpenKeyServer.exe`、`OpenKeySetup.exe`、`rime.dll`
   与 `shared\`（词库方案目录）置于同一安装目录；
2. `regsvr32 OpenKeyTsf.dll`（写入 CTF TIP 注册表）；
3. 系统设置 → 时间与语言 → 语言与区域 → 中文 → 选项 → 添加键盘"开门输入法"；
4. 首次输入时 Server 自动启动（由 TSF 拉起）；设置经 `OpenKeySetup.exe`。

目录约定（docs/02 4.1.4）：共享数据 `<安装目录>\shared`，用户数据
`%APPDATA%\OpenKey`（rime user / `openkey.db` / `components` / `log`）；
Server 可用环境变量 `OPENKEY_SHARED_DIR` 覆盖共享数据目录。

## 已知限制 / TODO

- 代码签名（Authenticode）未配置：CI 检测到 `secrets.WINDOWS_CERT` 才签名，
  否则产物为未签名版本（DR-002，证书待配置）；
- 候选窗为 GDI 实现，D2D 皮肤与竖排布局待后续里程碑（4.1.2）；
- 设置程序为最小 Win32 窗口，完整设置项清单（4.1 设置 UI）待 WPF/WinUI 化；
- `KeysymMap` 方向键与功能键映射待补齐（当前覆盖字母/数字/常用标点/退格/回车）；
- 自更新一期为"检查 + 提示手动下载"（4.1.5）；
- TSf 各接口（`ITfCandidateListUIElement` 等）为最小可用实现，细节以微软
  TSF 文档为准逐步完善。
