import AppKit
import InputMethodKit
import OpenKeyCore

/// 每个输入客户端一个实例（IMKServer 按 Info.plist 的 `InputMethodServerControllerClass` 创建）。
///
/// 按键管线（docs/02 2.4 输入管线）：
/// ```
/// NSEvent → X11Keysym 翻译 → OpenKeySession.processKey
///   → 有待上屏文本：IMKTextInput.insertText 提交宿主
///   → 组词中：自绘候选面板（NSPanel）展示预编辑 + 候选
///   → 未消费：事件交还系统（return false）
/// ```
///
/// 已知简化（README TODO 清单）：
/// - 预编辑只在自绘面板中展示，未走 `setMarkedText`（宿主内联预编辑留待 Phase 2）；
/// - 候选面板定位暂用鼠标位置兜底，未接 `firstRectForCharacterRange`  caret 定位；
/// - CapsLock 中英切换未接 flagsChanged（依赖 rime ascii_mode 与 Shift）。
final class InputController: IMKInputController {
    /// 当前输入客户端（`IMKTextInput` 提供方），每次 handle 时刷新。
    private weak var currentClient: AnyObject?
    private var session: OpenKeySession?
    private var panel: CandidatePanel?

    override init!(server: IMKServer!, delegate: Any!, client: Any!) {
        super.init(server: server, delegate: delegate, client: client)
        currentClient = client as? AnyObject
    }

    // MARK: - 服务器生命周期

    override func activateServer(_ sender: Any!) {
        super.activateServer(sender)
        // 激活时懒创建会话并恢复设置 App 写入的偏好（方案/中英状态）。
        if let session = ensureSession() {
            applyStoredPreferences(to: session)
            refreshPanel(with: session)
        }
    }

    override func deactivateServer(_ sender: Any!) {
        // 失活（切换输入法/切走焦点）：先了结组词，避免预编辑残留在面板里。
        if let session {
            if let text = session.commitText(), !text.isEmpty {
                insertText(text)
            } else {
                session.clearComposition()
            }
        }
        panel?.hide()
        super.deactivateServer(sender)
    }

    /// 宿主主动请求提交组词（如窗口关闭）。
    override func commitComposition(_ sender: Any!) {
        commitOrClear()
    }

    // MARK: - 按键处理

    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event else { return false }
        currentClient = sender as? AnyObject

        switch event.type {
        case .keyDown:
            return handleKeyDown(event)
        case .flagsChanged:
            // TODO(Phase 2)：修饰键单独按下。部分宿主收不到 IMK 的 flagsChanged
            // （docs/02 4.2.4 验证清单），当前 CapsLock 中英切换未实现。
            return false
        default:
            return false
        }
    }

    private func handleKeyDown(_ event: NSEvent) -> Bool {
        guard let (keycode, mask) = Self.keysymAndMask(for: event) else { return false }
        guard let session = ensureSession() else { return false }

        guard session.processKey(keycode: keycode, mask: mask) else {
            // 未被 rime 消费：了结本地组词状态，事件交还系统处理。
            commitOrClear()
            panel?.hide()
            return false
        }

        // 1) 有待上屏文本 → 插入宿主并收尾。
        if let text = session.commitText(), !text.isEmpty {
            insertText(text)
            session.clearComposition()
            panel?.hide()
            return true
        }

        // 2) 组词中 / 有候选 → 刷新候选面板。
        if let context = session.context(), context.isComposing || !context.candidates.isEmpty {
            showPanel(with: context)
            return true
        }

        panel?.hide()
        return true
    }

    // MARK: - 面板交互

    private func showPanel(with context: OpenKeyContext) {
        let panel: CandidatePanel
        if let existing = self.panel {
            panel = existing
        } else {
            panel = CandidatePanel()
            panel.onSelect = { [weak self] index in
                self?.selectCandidate(at: index)
            }
            panel.onPage = { [weak self] backward in
                self?.changePage(backward: backward)
            }
            self.panel = panel
        }
        panel.update(context: context)
        // TODO(Phase 2)：改为基于宿主 caret 的定位（IMKTextInput firstRectForCharacterRange）；
        // 当前用鼠标位置兜底，保证 Phase 1 可用。
        panel.show(near: NSEvent.mouseLocation)
    }

    private func refreshPanel(with session: OpenKeySession) {
        if let context = session.context(), context.isComposing || !context.candidates.isEmpty {
            showPanel(with: context)
        } else {
            panel?.hide()
        }
    }

    private func selectCandidate(at index: Int) {
        guard let session else { return }
        guard session.selectCandidate(index: index) else { return }
        if let text = session.commitText(), !text.isEmpty {
            insertText(text)
            session.clearComposition()
            panel?.hide()
        } else {
            refreshPanel(with: session)
        }
    }

    private func changePage(backward: Bool) {
        guard let session else { return }
        _ = session.changePage(backward: backward)
        refreshPanel(with: session)
    }

    // MARK: - 上屏与状态

    private func insertText(_ text: String) {
        guard let client = currentClient as? IMKTextInput else {
            NSLog("[OpenKey] 当前 client 不遵循 IMKTextInput，无法上屏")
            return
        }
        client.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
    }

    private func commitOrClear() {
        guard let session else { return }
        if let text = session.commitText(), !text.isEmpty {
            insertText(text)
        }
        session.clearComposition()
        panel?.hide()
    }

    // MARK: - 会话与偏好

    private func ensureSession() -> OpenKeySession? {
        if let session {
            return session
        }
        guard let engine = EngineProvider.shared() else {
            NSLog("[OpenKey] 引擎初始化失败：\(EngineProvider.lastError)")
            return nil
        }
        guard let newSession = OpenKeySession(engine: engine) else {
            NSLog("[OpenKey] 会话创建失败：\(engine.lastError())")
            return nil
        }
        session = newSession
        return newSession
    }

    /// 从设置 App 写入的偏好恢复会话状态。
    /// TODO(Phase 2)：设置变更热更新——当前需重新激活输入法（重开会话）才生效，
    /// 可后续用 NSDistributedNotificationCenter 广播 + 会话内 apply。
    private func applyStoredPreferences(to session: OpenKeySession) {
        let store = OpenKeySettingsStore(engine: session.engine)
        let schemaId = store.effectiveSchemaId()
        if session.currentSchemaId != schemaId {
            _ = session.selectSchema(schemaId)
        }
        _ = session.setOption("ascii_mode", store.asciiMode)
    }

    // MARK: - NSEvent → X11 keysym

    /// 把 macOS 按键事件翻译为 core 约定的 (keysym, 修饰键掩码)。
    ///
    /// 优先用 `charactersIgnoringModifiers` 查字符表（与键盘布局无关），
    /// 特殊键（无字符）回退到 macOS 虚拟键码表。
    static func keysymAndMask(for event: NSEvent) -> (keycode: UInt32, mask: UInt32)? {
        guard let keysym = keysym(for: event) else { return nil }
        return (keysym, modifierMask(for: event))
    }

    static func modifierMask(for event: NSEvent) -> UInt32 {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var mask = X11Keysym.Mask()
        if flags.contains(.shift) { mask.insert(.shift) }
        if flags.contains(.capsLock) { mask.insert(.caps) }
        if flags.contains(.control) { mask.insert(.control) }
        if flags.contains(.option) { mask.insert(.alt) }
        return mask.rawValue
    }

    static func keysym(for event: NSEvent) -> UInt32? {
        if let characters = event.charactersIgnoringModifiers,
           let character = characters.first,
           let keysym = X11Keysym.keysym(for: character)
        {
            return keysym
        }
        return specialKeysym(forKeyCode: event.keyCode)
    }

    /// 特殊键：macOS 虚拟键码 → X11 keysym（charactersIgnoringModifiers 为空的键）。
    private static func specialKeysym(forKeyCode keyCode: UInt16) -> UInt32? {
        let specialKey: X11Keysym.SpecialKey?
        switch keyCode {
        case 36: specialKey = .returnKey
        case 76: specialKey = .keypadEnter
        case 48: specialKey = .tab
        case 51: specialKey = .backspace
        case 117: specialKey = .delete
        case 53: specialKey = .escape
        case 115: specialKey = .home
        case 119: specialKey = .end
        case 116: specialKey = .pageUp
        case 121: specialKey = .pageDown
        case 123: specialKey = .left
        case 124: specialKey = .up
        case 125: specialKey = .right
        case 126: specialKey = .down
        default:
            // TODO(Phase 2)：F1-F12 等尚未映射。
            specialKey = nil
        }
        return specialKey?.keysym
    }
}
