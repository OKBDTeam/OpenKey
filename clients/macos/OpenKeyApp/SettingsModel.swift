import Foundation
import OpenKeyCore

/// 设置页视图模型。
///
/// 与输入法进程共享同一份用户数据目录（docs/02 4.2.1）：
/// - 引擎以只读用途创建（枚举已部署方案、读写设置项）；
/// - 设置写入 core 数据层（openkey.db），输入法进程在会话创建时读取生效。
@MainActor
final class SettingsModel: ObservableObject {
    @Published private(set) var engineReady = false
    @Published private(set) var engineError: String?
    @Published private(set) var schemas: [OpenKeySchemaInfo] = []

    @Published var schemaId = "" {
        didSet { store?.update(schemaId: schemaId) }
    }
    @Published var doublePinyin: DoublePinyinScheme = .off {
        didSet { store?.update(doublePinyin: doublePinyin) }
    }
    @Published var fuzzyPinyin = false {
        didSet { store?.update(fuzzyPinyin: fuzzyPinyin) }
    }
    @Published var asciiMode = false {
        didSet { store?.update(asciiMode: asciiMode) }
    }

    private var store: OpenKeySettingsStore?

    /// 懒加载引擎：librime 初始化/部署可能耗时数秒，放后台线程完成后回主线程刷新。
    func load() async {
        if engineReady || engineError != nil {
            return
        }
        let userDataDir = OpenKeyPaths.userDataDir
        let dataDbPath = OpenKeyPaths.dataDbPath
        let sharedDataDir = OpenKeyPaths.sharedDataDir
        // Task.detached 闭包不捕获 self（@MainActor 隔离类型），避免并发告警。
        let outcome: (OpenKeyEngine?, String) = await Task.detached {
            if let engine = OpenKeyEngine(
                sharedDataDir: sharedDataDir,
                userDataDir: userDataDir,
                dataDbPath: dataDbPath
            ) {
                return (engine, "")
            }
            return (nil, OpenKeyEngine.lastGlobalError())
        }.value

        if let engine = outcome.0 {
            let settings = OpenKeySettingsStore(engine: engine)
            self.store = settings
            self.schemas = engine.schemas
            self.schemaId = settings.schemaId ?? (engine.schemas.first?.id ?? OpenKeySchemaId.pinyin)
            self.doublePinyin = settings.doublePinyin
            self.fuzzyPinyin = settings.fuzzyPinyin
            self.asciiMode = settings.asciiMode
            self.engineReady = true
        } else {
            let message = outcome.1
            self.engineError = message.isEmpty ? "引擎初始化失败（无错误详情）" : message
        }
    }
}
