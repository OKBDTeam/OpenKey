cmake_minimum_required(VERSION 3.22.1)
project(openkey_jni_shim LANGUAGES C)

# openkey.h 优先使用本目录副本（Gradle 任务 syncOpenKeyHeader 从仓库 core/ffi-headers 拷入）；
# 若不存在则回退仓库相对路径，方便脱离 Gradle 直接用 cmake/ndk-build 做编译验证。
set(OPENKEY_HEADER_DIR "${CMAKE_CURRENT_SOURCE_DIR}")
if (NOT EXISTS "${OPENKEY_HEADER_DIR}/openkey.h")
    set(OPENKEY_HEADER_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../../../../../core/ffi-headers")
endif()
if (NOT EXISTS "${OPENKEY_HEADER_DIR}/openkey.h")
    message(FATAL_ERROR "未找到 openkey.h：请确认仓库 core/ffi-headers 存在，或先执行 Gradle 任务 syncOpenKeyHeader")
endif()

add_library(openkey_jni_shim SHARED openkey_jni_shim.c)

target_include_directories(openkey_jni_shim PRIVATE "${OPENKEY_HEADER_DIR}")
target_compile_options(openkey_jni_shim PRIVATE -Wall -Wextra)

# 纯 C 工程，不链接 Android 系统库（ok_* 符号运行时由 libopenkey_ffi.so 提供）。
