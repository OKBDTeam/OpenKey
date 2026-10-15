import Foundation

/// 客户端数据目录约定（双端共用）。
///
/// - macOS：`~/Library/Application Support/OpenKey/`（设置 App 与输入法进程共享，
///   SQLite 以 WAL + busy_timeout 保证并发安全，见 docs/02 4.2.1）。
/// - iOS：App Group `group.app.openkey.shared` 容器（主 App / 键盘扩展 / 凭据扩展共享，
///   见 docs/02 4.5.1）。未配置 App Group（如未签名构建）时退化到各自沙箱容器。
public enum OpenKeyPaths {
    /// iOS 三 target 共享容器标识（docs/02 4.5.1）。
    public static let appGroupIdentifier = "group.app.openkey.shared"

    /// librime 用户数据目录（userdb / build / 部署产物）与 openkey.db 所在目录。
    public static var userDataDir: String {
        let directory = baseDirectory.appendingPathComponent("OpenKey", isDirectory: true)
        // SQLite 与 librime 都不会自动创建多级目录，这里确保存在（失败时静默继续，由 core 报错）。
        try? FileManager.default.createDirectory(
            atPath: directory.path,
            withIntermediateDirectories: true
        )
        return directory.path
    }

    /// librime 共享数据目录（方案与词库；由 scripts/build-bundle.sh 部署）。
    public static var sharedDataDir: String {
        userDataDir + "/shared"
    }

    /// openkey.db 路径（数据层；设置项、词库中心等经此持久化）。
    public static var dataDbPath: String {
        userDataDir + "/openkey.db"
    }

    /// 平台相关的基础目录（App Group 容器 / 用户 Application Support）。
    private static var baseDirectory: URL {
        #if canImport(UIKit)
        if let container = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: appGroupIdentifier
        ) {
            return container
        }
        // 退化路径：未配置 App Group 时用各自沙箱容器（功能可用但三 target 不共享）。
        if let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first {
            return documents
        }
        #else
        if let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            return appSupport
        }
        #endif
        return URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
    }
}
