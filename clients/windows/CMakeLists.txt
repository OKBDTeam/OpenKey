# OpenKey Windows 客户端（docs/02 4.1）
#
# 目标：
#   OpenKeyCoreClient  静态库：openkey.h 的 C++ RAII 封装 + 命名管道协议 + keysym 映射
#   OpenKeyTsf         SHARED（OpenKeyTsf.dll）：TSF 文本服务（键事件 → 管道 → 预编辑/候选窗）
#   OpenKeyServer      可执行：常驻进程，持有 core 引擎，命名管道服务
#   OpenKeySetup       可执行：Win32 设置对话框（方案/双拼/模糊音/中英切换）
#
# 依赖 core 静态库（OPENKEY_CORE_LIB）与 librime（OPENKEY_RIME_LIB）；
# 变量未提供时从环境变量 LIBRIME_DIR 推导，均缺失时报 FATAL_ERROR。
#
# 构建（VS 2022 + CMake）：
#   cmake -S . -B build -G "Visual Studio 17 2022" -A x64 ^
#         -DOPENKEY_CORE_LIB=<core 静态库 openkey_ffi.lib> ^
#         -DOPENKEY_RIME_LIB=<librime 导入库 rime.lib> ^
#         -DOPENKEY_CORE_INCLUDE_DIR=<core/ffi-headers>
#   cmake --build build --config Release
#
# 安装：regsvr32 OpenKeyTsf.dll（DllRegisterServer 写 CTF TIP 注册表）后
#       在系统设置启用"开门输入法"。

cmake_minimum_required(VERSION 3.20)
project(OpenKeyWindows LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

# 源码为无 BOM 的 UTF-8（含中文注释与界面串）：显式指定源字符集与执行字符集，
# 否则 MSVC 按 ANSI 代码页解析，产生 C4819 警告并使窄字符串字面量乱码。
# UNICODE/_UNICODE：资源 ID 宏（IDC_ARROW 等）与 LoadCursorW 等 W 版 API
# 配套，未定义时 IDC_ARROW 为 LPSTR，无法传给 LoadCursorW。
if(MSVC)
  add_compile_options(/utf-8)
  add_compile_definitions(UNICODE _UNICODE)
endif()

# Rust 静态库（openkey_ffi.lib）在 MSVC 链接期需要的 Windows 系统库。
# Rust 的 .lib 不携带这些 defaultlib 指令，缺失会报 LNK2019（曾出现
# __imp_GetUserProfileDirectoryW / __imp_NtCreateNamedPipeFile）。
set(OPENKEY_RUST_SYSLIBS
  userenv ntdll ws2_32 bcrypt crypt32 secur32 oleaut32 ole32 uuid shell32 advapi32)

# ---- core / librime 路径解析 -------------------------------------------------
if(NOT DEFINED OPENKEY_CORE_INCLUDE_DIR AND DEFINED ENV{LIBRIME_DIR})
  set(OPENKEY_CORE_INCLUDE_DIR "$ENV{LIBRIME_DIR}/include")
endif()
if(NOT DEFINED OPENKEY_CORE_LIB AND DEFINED ENV{LIBRIME_DIR})
  # librime 官方 Windows 构建产物：dist/lib/rime.lib（docs/02 4.1.4）
  set(OPENKEY_RIME_LIB_DEFAULT "$ENV{LIBRIME_DIR}/dist/lib/rime.lib")
endif()
if(NOT DEFINED OPENKEY_CORE_LIB)
  message(FATAL_ERROR
    "未提供 OPENKEY_CORE_LIB（core 静态库 openkey_ffi.lib 路径）。\n"
    "CI 由 core 构建步骤提供；本地先构建 core（cd core && cargo build --release -p openkey-ffi）。")
endif()
if(NOT DEFINED OPENKEY_CORE_INCLUDE_DIR)
  message(FATAL_ERROR
    "未提供 OPENKEY_CORE_INCLUDE_DIR（core/ffi-headers 路径，含 openkey.h）。")
endif()
if(NOT DEFINED OPENKEY_RIME_LIB)
  if(DEFINED OPENKEY_RIME_LIB_DEFAULT AND EXISTS "${OPENKEY_RIME_LIB_DEFAULT}")
    set(OPENKEY_RIME_LIB "${OPENKEY_RIME_LIB_DEFAULT}")
  else()
    message(FATAL_ERROR
      "未提供 OPENKEY_RIME_LIB（librime 导入库 rime.lib 路径）。\n"
      "Windows 端 librime 由 librime 官方 build.bat 构建（deps + librime）。")
  endif()
endif()

message(STATUS "OPENKEY_CORE_LIB          = ${OPENKEY_CORE_LIB}")
message(STATUS "OPENKEY_RIME_LIB          = ${OPENKEY_RIME_LIB}")
message(STATUS "OPENKEY_CORE_INCLUDE_DIR  = ${OPENKEY_CORE_INCLUDE_DIR}")

# ---- OpenKeyCoreClient（静态库）---------------------------------------------
add_library(OpenKeyCoreClient STATIC
  src/OpenKeyCoreClient/openkey_client.cpp
  src/OpenKeyCoreClient/pipe_client.cpp
  src/OpenKeyCoreClient/pipe_protocol.cpp
  src/OpenKeyCoreClient/keysym_map.cpp
)
target_include_directories(OpenKeyCoreClient PUBLIC
  "${OPENKEY_CORE_INCLUDE_DIR}"
  ${CMAKE_CURRENT_SOURCE_DIR}/src/OpenKeyCoreClient
)
target_link_libraries(OpenKeyCoreClient PUBLIC
  "${OPENKEY_CORE_LIB}" "${OPENKEY_RIME_LIB}" ${OPENKEY_RUST_SYSLIBS})

# ---- OpenKeyTsf（TSF DLL）---------------------------------------------------
add_library(OpenKeyTsf SHARED
  src/OpenKeyTsf/dllmain.cpp
  src/OpenKeyTsf/text_service.cpp
  src/OpenKeyTsf/candidate_window.cpp
  src/OpenKeyTsf/register.cpp
  src/OpenKeyTsf/exports.def
)
target_link_libraries(OpenKeyTsf PRIVATE OpenKeyCoreClient ole32 uuid gdi32)

# ---- OpenKeyServer（常驻进程）-----------------------------------------------
# 不加 WIN32：保持控制台子系统。Server 为无窗口进程时用户无从判断它是否
# 在运行（这是 v0.1.0 被反馈「无法使用输入法」的直接原因之一）；
# 控制台窗口同时让 Ctrl+C / Ctrl+Break 优雅退出路径真正可用。
add_executable(OpenKeyServer
  src/OpenKeyServer/main.cpp
  src/OpenKeyServer/engine_host.cpp
  src/OpenKeyServer/update_check.cpp
)
target_link_libraries(OpenKeyServer PRIVATE OpenKeyCoreClient advapi32 winhttp)

# ---- OpenKeySetup（设置程序，最小 Win32 对话框）-----------------------------
add_executable(OpenKeySetup WIN32
  src/OpenKeySetup/main.cpp
)
target_link_libraries(OpenKeySetup PRIVATE OpenKeyCoreClient comctl32)

# 安装规则（供打包脚本取产物）
install(TARGETS OpenKeyTsf OpenKeyServer OpenKeySetup
  RUNTIME DESTINATION bin
  LIBRARY DESTINATION bin
  ARCHIVE DESTINATION bin)
