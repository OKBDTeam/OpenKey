import COpenKey
import Foundation

// MARK: - 对外数据结构（C 结构体的安全 Swift 表达）

/// 候选字词（对应 C 的 OkCandidate）。
public struct OpenKeyCandidate: Equatable {
    public let text: String
    public let comment: String?

    public init(text: String, comment: String? = nil) {
        self.text = text
        self.comment = comment
    }
}

/// 会话上下文（对应 C 的 OkContext）。
/// 注意：C 侧候选数组指针"归会话所有，下次调用前有效"，因此这里在读取时立即拷贝为 Swift 值。
public struct OpenKeyContext: Equatable {
    /// 预编辑文本；nil 表示未组词。
    public let preedit: String?
    public let cursorPos: Int
    public let selectionStart: Int
    public let selectionEnd: Int
    public let candidates: [OpenKeyCandidate]
    public let pageNo: Int
    public let pageSize: Int
    public let isLastPage: Bool
    public let highlightedCandidateIndex: Int

    public var isComposing: Bool {
        guard let preedit else { return false }
        return !preedit.isEmpty
    }
}

/// 会话状态快照（对应 C 的 OkStatus）。
public struct OpenKeyStatus: Equatable {
    public let schemaId: String
    public let schemaName: String
    public let isComposing: Bool
    public let isAsciiMode: Bool
    public let isFullShape: Bool
    public let isAsciiPunct: Bool
}

/// 已部署方案（schema_id + 展示名）。
public struct OpenKeySchemaInfo: Equatable, Identifiable {
    public let id: String
    public let name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

// MARK: - 引擎

/// openkey core 引擎句柄的 Swift 封装（对应 C 的 OkEngine）。
///
/// 职责边界（docs/02 2.1 铁律）：客户端只经本封装与 core 交互，不直接调用 librime。
/// 生命周期：`init?` 成功即持有 `ok_engine_create` 返回的句柄，`deinit` 时销毁。
public final class OpenKeyEngine {
    /// 用户数据目录（librime userdb/build、openkey.db、设置项均在此树下）。
    public let userDataDir: String

    private let handle: OpaquePointer

    /// 创建引擎：初始化 librime（含部署）、打开数据层与组件管理器。
    /// - Parameters:
    ///   - sharedDataDir: librime 共享数据目录（方案与词库）；nil 时由 core 取默认值。
    ///   - userDataDir: librime 用户数据目录（必填）。
    ///   - dataDbPath: openkey.db 路径；nil 则不启用数据层（设置持久化依赖数据层）。
    ///   - componentsDir: 组件根目录；nil 时取 `{user_data_dir}/components`。
    ///   - minLogLevel: glog 最小日志级别（0=INFO .. 3=FATAL）。
    /// - Returns: 创建失败返回 nil，详情见 `OpenKeyEngine.lastGlobalError()`。
    public init?(
        sharedDataDir: String? = nil,
        userDataDir: String,
        dataDbPath: String? = nil,
        componentsDir: String? = nil,
        minLogLevel: Int32 = 1
    ) {
        // C ABI 要求字符串指针在 ok_engine_create 调用期间有效：
        // 用 strdup 拷贝、调用后统一 free，避免 withCString 多层嵌套。
        let cShared = sharedDataDir.map { strdup($0) }
        let cUser = strdup(userDataDir)
        let cDb = dataDbPath.map { strdup($0) }
        let cComponents = componentsDir.map { strdup($0) }
        defer { [cShared, cUser, cDb, cComponents].compactMap { $0 }.forEach { free($0) } }

        var config = OkEngineConfig()
        config.shared_data_dir = cShared.map { UnsafePointer($0) }
        config.user_data_dir = UnsafePointer(cUser)
        config.data_db_path = cDb.map { UnsafePointer($0) }
        config.components_dir = cComponents.map { UnsafePointer($0) }
        config.log_dir = nil
        config.min_log_level = minLogLevel

        guard let engine = ok_engine_create(&config) else { return nil }
        self.handle = engine
        self.userDataDir = userDataDir
    }

    deinit {
        ok_engine_destroy(handle)
    }

    /// 创建失败时的全局错误详情（对应 `ok_last_error(NULL)`，引擎句柄尚未产生时使用）。
    public static func lastGlobalError() -> String {
        String(cString: ok_last_error(nil))
    }

    /// 最近一次错误详情（对当前引擎的最近一次调用）。
    public func lastError() -> String {
        String(cString: ok_last_error(handle))
    }

    /// OpenKey 版本字符串。
    public static var version: String {
        String(cString: ok_version_string())
    }

    /// 触发重新部署（方案/词库变更后调用）。
    @discardableResult
    public func deploy() -> Bool {
        ok_engine_deploy(handle)
    }

    /// 已部署方案数量。
    public var schemaCount: Int {
        Int(ok_engine_schema_count(handle))
    }

    /// 按下标取方案（写入调用方缓冲区；缓冲区不足时 core 返回 false，这里返回 nil）。
    public func schema(at index: Int) -> OpenKeySchemaInfo? {
        var idBuffer = [CChar](repeating: 0, count: 256)
        var nameBuffer = [CChar](repeating: 0, count: 256)
        let ok = ok_engine_schema_at(
            handle,
            UInt(index),
            &idBuffer,
            UInt(idBuffer.count),
            &nameBuffer,
            UInt(nameBuffer.count)
        )
        guard ok else { return nil }
        return OpenKeySchemaInfo(id: String(cString: idBuffer), name: String(cString: nameBuffer))
    }

    /// 全部已部署方案。
    public var schemas: [OpenKeySchemaInfo] {
        (0..<schemaCount).compactMap { schema(at: $0) }
    }

    /// 触发 userdb 同步快照任务（docs/02 3.3.4）。
    @discardableResult
    public func syncUserData() -> Bool {
        ok_engine_sync_user_data(handle)
    }

    /// 创建输入会话（失败返回 nil）。
    public func makeSession() -> OpenKeySession? {
        OpenKeySession(engine: self)
    }

    // MARK: 设置项（JSON 值文本，经 core 数据层持久化）

    /// 写入设置项（value 为 JSON 文本，如 `"luna_pinyin"` / `true`）。
    @discardableResult
    public func setSetting(_ key: String, jsonValue: String) -> Bool {
        key.withCString { keyPointer in
            jsonValue.withCString { valuePointer in
                ok_setting_set(handle, keyPointer, valuePointer)
            }
        }
    }

    /// 读取设置项（返回 JSON 文本；无此设置或缓冲区不足返回 nil）。
    public func setting(_ key: String) -> String? {
        key.withCString { keyPointer in
            var buffer = [CChar](repeating: 0, count: 1024)
            guard ok_setting_get(handle, keyPointer, &buffer, UInt(buffer.count)) else {
                return nil
            }
            return String(cString: buffer)
        }
    }

    /// 写入字符串设置项（自动编码为 JSON 字符串）。
    @discardableResult
    public func setSetting(_ key: String, string value: String) -> Bool {
        setSetting(key, jsonValue: Self.jsonStringLiteral(value))
    }

    /// 写入布尔设置项。
    @discardableResult
    public func setSetting(_ key: String, bool value: Bool) -> Bool {
        setSetting(key, jsonValue: value ? "true" : "false")
    }

    /// 读取字符串设置项（设置不存在或类型不符时返回 nil）。
    public func settingString(_ key: String) -> String? {
        guard let json = setting(key),
              let data = json.data(using: .utf8),
              let decoded = try? JSONSerialization.jsonObject(with: data),
              let string = decoded as? String
        else {
            return nil
        }
        return string
    }

    /// 读取布尔设置项（设置不存在或类型不符时返回 nil）。
    public func settingBool(_ key: String) -> Bool? {
        guard let json = setting(key),
              let data = json.data(using: .utf8),
              let decoded = try? JSONSerialization.jsonObject(with: data)
        else {
            return nil
        }
        return (decoded as? NSNumber)?.boolValue
    }

    /// 把 Swift 字符串编码为 JSON 字符串文本（含引号与必要转义）。
    static func jsonStringLiteral(_ value: String) -> String {
        var escaped = ""
        escaped.reserveCapacity(value.count + 2)
        for scalar in value.unicodeScalars {
            switch scalar {
            case "\"": escaped += "\\\""
            case "\\": escaped += "\\\\"
            case "\n": escaped += "\\n"
            case "\r": escaped += "\\r"
            case "\t": escaped += "\\t"
            default:
                if scalar.value < 0x20 {
                    escaped += String(format: "\\u%04x", scalar.value)
                } else {
                    escaped.unicodeScalars.append(scalar)
                }
            }
        }
        return "\"\(escaped)\""
    }
}

// MARK: - 会话

/// openkey core 输入会话的 Swift 封装（对应 C 的 OkSession）。
///
/// 生命周期与 C 约定：候选数组/状态指针"归会话所有，下次调用前有效"，
/// 因此 `context()` / `status()` 都在读取时立即拷贝为 Swift 值。
/// 会话强引用引擎，保证句柄生命周期有序（先毁会话、后毁引擎）。
public final class OpenKeySession {
    public let engine: OpenKeyEngine

    private let handle: OpaquePointer

    init?(engine: OpenKeyEngine) {
        guard let session = ok_session_create(engine.handle) else { return nil }
        self.engine = engine
        self.handle = session
    }

    deinit {
        ok_session_destroy(handle)
    }

    /// 处理按键。`keycode` 为 X11 keysym，`mask` 为 X11 修饰键（见 `X11Keysym.Mask`）。
    /// 返回是否被 librime 消费；未消费时客户端应按普通按键处理。
    @discardableResult
    public func processKey(keycode: UInt32, mask: UInt32) -> Bool {
        ok_session_process_key(handle, keycode, mask)
    }

    /// 上屏当前预编辑。
    @discardableResult
    public func commitComposition() -> Bool {
        ok_session_commit_composition(handle)
    }

    /// 清除组词。
    public func clearComposition() {
        ok_session_clear_composition(handle)
    }

    /// 取当前上下文（预编辑 + 当前页候选）。
    public func context() -> OpenKeyContext? {
        var raw = OkContext()
        guard ok_session_get_context(handle, &raw) else { return nil }

        // 候选数组指针归会话所有：立即拷贝为 Swift 值后再做任何其它调用。
        let count = Int(raw.num_candidates)
        var candidates: [OpenKeyCandidate] = []
        if count > 0, let pointer = raw.candidates {
            candidates.reserveCapacity(count)
            for index in 0..<count {
                let candidate = pointer[index]
                candidates.append(
                    OpenKeyCandidate(
                        text: String(cString: candidate.text),
                        comment: candidate.comment.map { String(cString: $0) }
                    )
                )
            }
        }

        return OpenKeyContext(
            preedit: raw.preedit.map { String(cString: $0) },
            cursorPos: Int(raw.cursor_pos),
            selectionStart: Int(raw.sel_start),
            selectionEnd: Int(raw.sel_end),
            candidates: candidates,
            pageNo: Int(raw.page_no),
            pageSize: Int(raw.page_size),
            isLastPage: raw.is_last_page,
            highlightedCandidateIndex: Int(raw.highlighted_candidate_index)
        )
    }

    /// 取待上屏文本；无待上屏文本（或缓冲区不足）返回 nil。
    public func commitText() -> String? {
        var buffer = [CChar](repeating: 0, count: 4096)
        guard ok_session_get_commit(handle, &buffer, UInt(buffer.count)) else { return nil }
        return String(cString: buffer)
    }

    /// 取状态快照。
    public func status() -> OpenKeyStatus? {
        var raw = OkStatus()
        guard ok_session_get_status(handle, &raw) else { return nil }
        return OpenKeyStatus(
            schemaId: raw.schema_id.map { String(cString: $0) } ?? "",
            schemaName: raw.schema_name.map { String(cString: $0) } ?? "",
            isComposing: raw.is_composing,
            isAsciiMode: raw.is_ascii_mode,
            isFullShape: raw.is_full_shape,
            isAsciiPunct: raw.is_ascii_punct
        )
    }

    /// 选择指定下标候选。
    @discardableResult
    public func selectCandidate(index: Int) -> Bool {
        ok_session_select_candidate(handle, UInt(index))
    }

    /// 翻页。
    @discardableResult
    public func changePage(backward: Bool) -> Bool {
        ok_session_change_page(handle, backward)
    }

    /// 设置会话选项（如 `ascii_mode`）。
    @discardableResult
    public func setOption(_ option: String, _ value: Bool) -> Bool {
        option.withCString { ok_session_set_option(handle, $0, value) }
    }

    /// 读取会话选项。
    /// 注意：返回 false 既可能是选项关闭，也可能是查询失败（C ABI 用同一返回值表达）。
    public func option(_ name: String) -> Bool {
        name.withCString { ok_session_get_option(handle, $0) }
    }

    /// 切换方案。
    @discardableResult
    public func selectSchema(_ schemaId: String) -> Bool {
        schemaId.withCString { ok_session_select_schema(handle, $0) }
    }

    /// 当前方案 ID（无方案时为 nil）。
    public var currentSchemaId: String? {
        ok_session_current_schema(handle).map { String(cString: $0) }
    }
}
