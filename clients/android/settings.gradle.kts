pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "OpenKey"

// 模块划分见 docs/02-技术设计与实施方案.md 4.4.1：
//   :app          主 App（设置/词库中心/vault/组件下载，Phase 1 先做设置骨架）
//   :keyboard     IME 输入法 UI（InputMethodService + Compose）
//   :core-binding openkey-ffi 的 JNI 绑定（C shim 拍平 JSON + Kotlin 封装）
//   :voicepack    语音组件动态加载器（DR-004，Phase 6）
include(":app", ":keyboard", ":core-binding", ":voicepack")
