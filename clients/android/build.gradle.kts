// 根构建脚本：仅声明插件版本，模块内 apply false 后按需引用。
// 版本选择（2024-09 前后稳定组合，见 README.md「版本选择」）：
//   AGP 8.7.2 + Gradle 8.9 + Kotlin 2.0.21 + compose compiler plugin 2.0.21
//   compileSdk 35 / minSdk 26 / Java 17 / Compose BOM 2024.09.00
plugins {
    id("com.android.application") version "8.7.2" apply false
    id("com.android.library") version "8.7.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
}
