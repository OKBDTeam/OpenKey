import AppKit
import InputMethodKit
import OpenKeyCore

/// macOS 输入法是"后台应用"形态的 bundle（`LSBackgroundOnly`）：系统把 `.inputmethod`
/// 当后台应用拉起，入口在这里创建 `IMKServer` 并按其连接名注册 NSConnection；
/// 之后每个输入客户端由 IMKServer 依据 Info.plist 的 `InputMethodServerControllerClass`
/// 实例化 `InputController`（见 IMKServer Class Reference：initWithName:bundleIdentifier:
/// 会读取 Info.plist 中的连接名与控制器类名）。
///
/// 工程形态说明（与 docs/02 2.5 的偏差，README 有完整说明）：
/// 本 target 的 Xcode 产品类型是 application + `WRAPPER_EXTENSION=inputmethod`
/// （真实输入法工程如 Squirrel / azooKey / BSMInputMethod 均为此形态），
/// 因为输入法必须有自己的可执行入口来创建 IMKServer；app-extension 类型没有入口点。

/// 输入法进程的自定义 NSApplication：在 init 阶段就挂好 AppDelegate，
/// 保证 applicationDidFinishLaunching 之前 delegate 已就位（IMKitSample 同款做法）。
final class NSManualApplication: NSApplication {
    private let appDelegate = AppDelegate()

    override init() {
        super.init()
        delegate = appDelegate
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }
}

/// 输入法进程入口：创建并持有 IMKServer。
@main
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var server: IMKServer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        // 连接名规范（docs/02 4.2.1 / macOS 10.7+ NSConnection 命名要求）：
        // Info.plist 的 InputMethodConnectionName 必须是 $(PRODUCT_BUNDLE_IDENTIFIER)_Connection。
        let connectionName = Bundle.main.infoDictionary?["InputMethodConnectionName"] as? String
        guard let name = connectionName, let bundleId = Bundle.main.bundleIdentifier else {
            NSLog("[OpenKey] Info.plist 缺少 InputMethodConnectionName 或 bundleIdentifier，输入法无法注册")
            return
        }
        server = IMKServer(name: name, bundleIdentifier: bundleId)
        if server == nil {
            // IMKServer 初始化失败通常是连接名与 Info.plist 不一致或连接已存在。
            NSLog("[OpenKey] IMKServer 创建失败（连接名：\(name)）")
        }
    }
}

/// IMKServer 子类：进程内唯一的输入法服务对象。
///
/// 当前不覆写任何行为（Apple 文档：You should never need to override this class），
/// 保留子类作为后续定制钩子（如模式字典 IMKModeDictionary、候选面板定位策略）。
final class InputMethodServer: IMKServer {
}

/// 进程内共享引擎（输入法是单进程多客户端模型）。
///
/// librime 初始化/部署可能耗时数秒，因此按需懒加载、全进程复用；
/// 多客户端（多个输入框）各自持有 `OpenKeySession`，共享同一引擎。
enum EngineProvider {
    private static let lock = NSLock()
    private static var cachedEngine: OpenKeyEngine?
    private static var lastCreationError = ""

    /// 取得（或创建）共享引擎；创建失败时可用 `lastCreationError` 取详情。
    static func shared() -> OpenKeyEngine? {
        lock.lock()
        defer { lock.unlock() }
        if let engine = cachedEngine {
            return engine
        }
        let engine = OpenKeyEngine(
            sharedDataDir: OpenKeyPaths.sharedDataDir,
            userDataDir: OpenKeyPaths.userDataDir,
            dataDbPath: OpenKeyPaths.dataDbPath
        )
        if let engine = engine {
            cachedEngine = engine
        } else {
            lastCreationError = OpenKeyEngine.lastGlobalError()
        }
        return engine
    }

    /// 最近一次引擎创建失败详情（无则为空串）。
    static var lastError: String {
        lock.lock()
        defer { lock.unlock() }
        return lastCreationError
    }
}
