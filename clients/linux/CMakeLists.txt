# OpenKey fcitx5 addon（docs/02 4.3）
#
# 目标：fcitx5-openkey.so（MODULE），进程内直跑 core（4.3.1）。
# core 静态库由 OPENKEY_CORE_LIB 指定；librime 由 OPENKEY_RIME_LIB 指定；
# 缺失时从环境变量 LIBRIME_DIR 推导，均缺失报 FATAL_ERROR。

cmake_minimum_required(VERSION 3.16)
project(OpenKeyLinux LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

# ---- 依赖解析 ---------------------------------------------------------------
find_package(Fcitx5Core 5.0.14 REQUIRED)
find_package(PkgConfig REQUIRED)

if(NOT DEFINED OPENKEY_CORE_LIB)
  message(FATAL_ERROR
    "未提供 OPENKEY_CORE_LIB（core 静态库 libopenkey_ffi.a）。\n"
    "CI 由 core 构建步骤提供；本地先构建：cd core && cargo build --release -p openkey-ffi")
endif()
if(NOT DEFINED OPENKEY_CORE_INCLUDE_DIR AND DEFINED ENV{LIBRIME_DIR})
  set(OPENKEY_CORE_INCLUDE_DIR "$ENV{LIBRIME_DIR}/include")
endif()
if(NOT DEFINED OPENKEY_CORE_INCLUDE_DIR)
  message(FATAL_ERROR "未提供 OPENKEY_CORE_INCLUDE_DIR（core/ffi-headers 目录，含 openkey.h）")
endif()
if(NOT DEFINED OPENKEY_RIME_LIB AND DEFINED ENV{LIBRIME_DIR})
  set(OPENKEY_RIME_LIB "$ENV{LIBRIME_DIR}/lib/librime.so")
endif()
if(NOT DEFINED OPENKEY_RIME_LIB)
  message(FATAL_ERROR "未提供 OPENKEY_RIME_LIB（librime 动态库 librime.so 路径）")
endif()

# 安装的数据目录（方案/词库随包分发，docs/02 4.3.4）
if(NOT DEFINED OPENKEY_SHARED_DATA_DIR)
  set(OPENKEY_SHARED_DATA_DIR "${CMAKE_INSTALL_PREFIX}/share/openkey")
endif()

message(STATUS "OPENKEY_CORE_LIB     = ${OPENKEY_CORE_LIB}")
message(STATUS "OPENKEY_RIME_LIB     = ${OPENKEY_RIME_LIB}")
message(STATUS "OPENKEY_CORE_INCLUDE_DIR = ${OPENKEY_CORE_INCLUDE_DIR}")
message(STATUS "OPENKEY_SHARED_DATA_DIR   = ${OPENKEY_SHARED_DATA_DIR}")

# ---- addon 库 ---------------------------------------------------------------
find_package(Threads REQUIRED)
add_library(fcitx5-openkey MODULE src/openkey.cpp)
set_target_properties(fcitx5-openkey PROPERTIES PREFIX "" OUTPUT_NAME "fcitx5-openkey")

target_compile_definitions(fcitx5-openkey PRIVATE
  OPENKEY_SHARED_DATA_DIR="${OPENKEY_SHARED_DATA_DIR}")
target_include_directories(fcitx5-openkey PRIVATE "${OPENKEY_CORE_INCLUDE_DIR}")
target_link_libraries(fcitx5-openkey PRIVATE
  Fcitx5::Core
  "${OPENKEY_CORE_LIB}"
  "${OPENKEY_RIME_LIB}"
)
# 静态 core 归档需要的系统依赖（rusqlite bundled SQLite / 线程 / 动态加载）。
target_link_libraries(fcitx5-openkey PRIVATE Threads::Threads dl)

# addon 版本（与 core 版本一致，供 fcitx5 关于页展示）
set(OPENKEY_VERSION "0.1.0" CACHE STRING "OpenKey addon 版本")
configure_file(openkey.conf.in "${CMAKE_CURRENT_BINARY_DIR}/openkey.conf" @ONLY)

# ---- 安装规则（docs/02 2.2 目录约定）---------------------------------------
# 安装目标用相对路径：`cmake --install --prefix <dir>` 阶段才决定前缀，
# 便于 CI 打包到暂存目录（绝对 DESTINATION 会锁死配置期 CMAKE_INSTALL_PREFIX）。
install(TARGETS fcitx5-openkey DESTINATION lib/fcitx5)
install(FILES openkeypinyin.conf DESTINATION share/fcitx5/inputmethod)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/openkey.conf" DESTINATION share/fcitx5/addon)
