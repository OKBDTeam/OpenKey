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

# 链接 core 动态库：NDK 链接器对 SHARED 库强制 -Wl,--no-undefined，
# ok_* 符号必须在此解析（core so 由 CI 放入 jniLibs/<abi>/，随 APK 一起分发，
# 运行期 System.loadLibrary("openkey_ffi") 先加载，shim 再依赖其符号）。
# 本地若尚未放入 core so，则跳过链接（shim 仍可编译，便于先看编译错误）。
set(OPENKEY_CORE_SO_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../jniLibs/${ANDROID_ABI}")
find_library(OPENKEY_CORE_SO NAMES openkey_ffi
             PATHS "${OPENKEY_CORE_SO_DIR}" NO_DEFAULT_PATH)
if (OPENKEY_CORE_SO)
    message(STATUS "链接 core: ${OPENKEY_CORE_SO}")
    target_link_libraries(openkey_jni_shim PRIVATE "${OPENKEY_CORE_SO}")
else()
    message(WARNING
        "未找到 ${OPENKEY_CORE_SO_DIR}/libopenkey_ffi.so：shim 将在运行期解析 ok_* 符号"
        "（System.loadLibrary 先加载 core so）。")
endif()
# 无论是否在链接期找到 core，都允许 shim 保留未定义符号：
# NDK 的 lld 默认对 SHARED 库强制 --no-undefined，而 ok_* 的实际提供者
# 是随 APK 分发的 libopenkey_ffi.so（运行期先加载），链接期强制解析既不
# 必要也会让「先编 shim 后放 core so」的构建顺序无法工作。
target_link_options(openkey_jni_shim PRIVATE
    "LINKER:--allow-shlib-undefined"
    "LINKER:--unresolved-symbols=ignore-in-shared-libs")
