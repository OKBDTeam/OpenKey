plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "app.openkey.keyboard"
    compileSdk = 35

    defaultConfig {
        minSdk = 26
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
}

dependencies {
    implementation(project(":core-binding"))

    implementation(platform("androidx.compose:compose-bom:2024.09.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")

    // ComposeView 运行在 InputMethodService 窗口中，需要手动提供
    // ViewTreeLifecycleOwner / ViewTreeViewModelStoreOwner /
    // ViewTreeSavedStateRegistryOwner（见 OpenKeyIme）。
    // 注意：AndroidX 自 2024 年起 KMP 拆分——lifecycle 各库的 root 坐标
    // （如 lifecycle-runtime）只带 Gradle Module Metadata，通过 available-at
    // 重定向到 -android 变体；必须依赖 root 坐标，直接依赖 -android 变体
    // 会因 KMP 变体名（release*）与 AGP 的 debug variant 不匹配而拿不到类
    // （曾表现为 ViewTree* Unresolved reference）。savedstate 1.2.x 尚未
    // KMP 化，直接依赖即可。
    implementation("androidx.lifecycle:lifecycle-runtime:2.8.6")
    implementation("androidx.lifecycle:lifecycle-viewmodel:2.8.6")
    implementation("androidx.savedstate:savedstate:1.2.1")
}
