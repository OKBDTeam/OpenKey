import Foundation

/// 设置项键名约定（写入 core 数据层，JSON 值文本；两端的设置 App 与输入法/键盘进程共用）。
public enum OpenKeySettingKey {
    /// 主方案 schema_id（JSON 字符串）。
    public static let schemaId = "schema_id"
    /// 双拼方案：off / ziranma / flypy / mspy / abc（JSON 字符串）。
    public static let doublePinyin = "double_pinyin"
    /// 模糊音开关（JSON bool）。
    public static let fuzzyPinyin = "fuzzy_pinyin"
    /// 启动时英文模式（JSON bool；输入中可用 Shift 切换）。
    public static let asciiMode = "ascii_mode"
}

/// 双拼方案预设（对应 shared/schemas 下的 openkey_double_*.schema.yaml）。
public enum DoublePinyinScheme: String, CaseIterable {
    case off
    case ziranma
    case flypy
    case mspy
    case abc

    /// 界面文案（集中一处，macOS/iOS 双端复用）。
    public var title: String {
        switch self {
        case .off: return "关闭"
        case .ziranma: return "自然码"
        case .flypy: return "小鹤"
        case .mspy: return "微软"
        case .abc: return "智能 ABC"
        }
    }

    /// 对应的 rime schema_id（off 时返回主方案）。
    public var schemaId: String {
        switch self {
        case .off: return OpenKeySchemaId.pinyin
        case .ziranma: return "openkey_double_ziranma"
        case .flypy: return "openkey_double_flypy"
        case .mspy: return "openkey_double_mspy"
        case .abc: return "openkey_double_abc"
        }
    }
}

/// 内置方案的 schema_id（来自 shared/schemas/*.schema.yaml）。
public enum OpenKeySchemaId {
    public static let pinyin = "openkey"
    public static let fuzzy = "openkey_fuzzy"
}

/// 设置项读写 + 方案解析（双端共用）。
///
/// 设置 App 与输入法/键盘进程各自持有引擎实例，经同一份 openkey.db 读写
/// （WAL + busy_timeout 保证并发安全，docs/02 4.2.1）。
public struct OpenKeySettingsStore {
    private let engine: OpenKeyEngine

    public init(engine: OpenKeyEngine) {
        self.engine = engine
    }

    // MARK: 读取

    public var schemaId: String? {
        engine.settingString(OpenKeySettingKey.schemaId)
    }

    public var doublePinyin: DoublePinyinScheme {
        let raw = engine.settingString(OpenKeySettingKey.doublePinyin) ?? ""
        return DoublePinyinScheme(rawValue: raw) ?? .off
    }

    public var fuzzyPinyin: Bool {
        engine.settingBool(OpenKeySettingKey.fuzzyPinyin) ?? false
    }

    public var asciiMode: Bool {
        engine.settingBool(OpenKeySettingKey.asciiMode) ?? false
    }

    // MARK: 写入

    public func update(schemaId: String) {
        engine.setSetting(OpenKeySettingKey.schemaId, string: schemaId)
    }

    public func update(doublePinyin: DoublePinyinScheme) {
        engine.setSetting(OpenKeySettingKey.doublePinyin, string: doublePinyin.rawValue)
    }

    public func update(fuzzyPinyin: Bool) {
        engine.setSetting(OpenKeySettingKey.fuzzyPinyin, bool: fuzzyPinyin)
    }

    public func update(asciiMode: Bool) {
        engine.setSetting(OpenKeySettingKey.asciiMode, bool: asciiMode)
    }

    // MARK: 方案解析

    /// 解析当前生效方案：双拼 > 模糊音 > 主方案。
    ///
    /// Phase 1 近似：rime 会话同时只能挂一个 schema，这里用"切换 schema"近似表达
    /// 双拼/模糊音开关；后续在 core 侧落地 schema patch / option 覆盖（见 README TODO）。
    public func effectiveSchemaId() -> String {
        if doublePinyin != .off {
            return doublePinyin.schemaId
        }
        if fuzzyPinyin {
            return OpenKeySchemaId.fuzzy
        }
        return schemaId ?? OpenKeySchemaId.pinyin
    }
}
