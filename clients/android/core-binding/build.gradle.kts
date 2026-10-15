plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "app.openkey.core"
    compileSdk = 35

    defaultConfig {
        minSdk = 26
        // 只出 arm64 与 x86_64 两个 ABI（桌面模拟器/真机主流；其余 ROM 占比低，后续按需加）
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
        externalNativeBuild {
            cmake {
                cFlags += "-Wall -Wextra"
            }
        }
    }

    // JNI shim（src/main/cpp/openkey_jni_shim.c）用 CMake 编译：
    // 它只 include 仓库根的 core/ffi-headers/openkey.h，不链接 core so——
    // ok_* 符号在运行时由 System.loadLibrary("openkey_ffi") 加载的
    // libopenkey_ffi.so 提供（CI 放入 src/main/jniLibs/<abi>/）。
    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    // org.json 为 Android 平台内置，无需额外依赖
}

// 把仓库根的 core/ffi-headers/openkey.h 拷到 cpp 目录，保证 externalNativeBuild 前可用
// （cbindgen 产物是唯一事实源，这里只做拷贝，不修改内容）。
val syncOpenKeyHeader by tasks.registering(Copy::class) {
    from(rootProject.file("../../core/ffi-headers/openkey.h"))
    into(layout.projectDirectory.dir("src/main/cpp"))
    onlyIf { rootProject.file("../../core/ffi-headers/openkey.h").exists() }
}
tasks.matching {
    it.name == "preBuild" || it.name.startsWith("externalNativeBuild")
}.configureEach {
    dependsOn(syncOpenKeyHeader)
}
