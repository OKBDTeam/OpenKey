# OpenKey Linux 客户端（fcitx5 addon，docs/02 4.3）

进程内直跑 core（无独立常驻进程，4.3.1），预编辑与候选经 fcitx5 classic UI
呈现（不自绘候选框）。

## 组件

| 文件 | 说明 |
|---|---|
| `src/openkey.cpp` | `OpenKeyEngine : fcitx::InputMethodEngineV2` + `AddonFactory`；每 `InputContext` 一个 `OkSession`；组词态下翻页/数字选择/空格选词/回车上屏/Esc 清空由 addon 解析，其余按键经 `ok_session_process_key` 交 librime |
| `CMakeLists.txt` | 依赖 `Fcitx5::Core`（≥ 5.0.14）；core 静态库与 librime 路径见下 |
| `openkey.conf.in` | addon 声明（`Library=libfcitx5-openkey.so`，CMake 以 `OPENKEY_VERSION` 填充） |
| `openkeypinyin.conf` | 输入法注册（Name=OpenKey，Label=开门） |
| `packaging/` | deb（`DEBIAN/control`）、rpm spec、AUR `PKGBUILD`、Flatpak manifest |

## 构建

前置：CMake ≥ 3.16、g++ ≥ 11、pkg-config、`libfcitx5core-dev`（≥ 5.0.14）、
`librime-dev`，以及 core 静态库。

```sh
# 1) 构建 core（链接 librime）
cd core && cargo build --release -p openkey-ffi

# 2) 配置与构建 addon
cmake -S clients/linux -B build -DCMAKE_BUILD_TYPE=RelWithDebInfo \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DOPENKEY_CORE_LIB=$PWD/core/target/release/libopenkey_ffi.a \
      -DOPENKEY_CORE_INCLUDE_DIR=$PWD/core/ffi-headers \
      -DOPENKEY_RIME_LIB=$PWD/librime-install/lib/librime.so \
      -DOPENKEY_SHARED_DATA_DIR=/usr/share/openkey
cmake --build build
sudo cmake --install build
```

CMake 变量缺失时报 `FATAL_ERROR`；`OPENKEY_RIME_LIB` 可由环境变量
`LIBRIME_DIR` 推导（`$LIBRIME_DIR/lib/librime.so`）。

## 使用

1. 注销后重新登录（刷新 fcitx5 addon 目录）；
2. fcitx5 配置工具 → 输入法 → 添加「开门」；
3. 切换输入法（Ctrl+Space）后即可使用。

## 目录约定

- 共享数据（方案/词库）：`/usr/share/openkey`（构建时 `OPENKEY_SHARED_DATA_DIR`）；
- 用户数据：`${XDG_DATA_HOME:-~/.local/share}/openkey`，内含 rime 用户目录、
  `openkey.db`、组件目录与日志——全部留在本机（隐私优先）。

## 已知限制 / TODO

- 设置界面：当前经 fcitx5 配置项与 rime 自身设置（`~/.local/share/openkey/rime`）；
  OpenKey 设置窗口（docs/02 4.3 保留待建）尚未在本端落地；
- 候选导航按键沿用 core 约定（Page_Up/Page_Down、`-`/`=`、`,`/`.`、数字 1–9、
  空格选词）；`-`/`=` 换页与 fcitx5 全局配置可能冲突，用户可在
  fcitx5 设置中改绑；
- 竖排、日韩输入、皮肤未实现；
- Flatpak 沙箱内 librime 的插件（opencc/glog 日志路径）需额外配置，manifest
  当前按最小可用配置给出。
