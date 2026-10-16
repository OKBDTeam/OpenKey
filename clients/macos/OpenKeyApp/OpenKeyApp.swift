import SwiftUI

/// OpenKey 设置 App 入口（macOS 13+）。
///
/// 职责（docs/02 4.2.1）：方案选择、双拼、模糊音、中英切换等偏好设置；
/// 与输入法进程共享 `~/Library/Application Support/OpenKey/`（经 core 数据层 openkey.db）。
/// 词库中心 / 表情管理 / vault / 组件下载等留待后续 Phase。
@main
struct OpenKeyApp: App {
    var body: some Scene {
        Window("OpenKey 设置", id: "main") {
            SettingsView()
        }
        .defaultSize(width: 560, height: 480)
    }
}
