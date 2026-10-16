plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
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
}

dependencies {
    implementation(project(":core-binding"))

    // Phase 1 键盘面板为传统 View 实现（见 KeyboardPanelView 文档与 README
    // 「与设计文档的偏差」）：Compose 化需要 lifecycle / savedstate 的
    // ViewTree* owner 支撑，该组依赖在 CI 组合下解析异常，待定位后恢复。
}
