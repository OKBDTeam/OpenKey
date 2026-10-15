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
    // 注意：AndroidX 自 2024 年起 KMP 拆分，lifecycle-runtime / savedstate 等
    // 根构件是空壳（无 class），实现类位于 -android 变体，必须显式依赖。
    implementation("androidx.lifecycle:lifecycle-runtime-android:2.8.6")
    implementation("androidx.lifecycle:lifecycle-viewmodel-android:2.8.6")
    implementation("androidx.savedstate:savedstate:1.2.1")
}
