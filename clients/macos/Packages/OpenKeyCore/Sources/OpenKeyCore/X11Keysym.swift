import Foundation

/// macOS/iOS 键盘事件 → X11 keysym / 修饰键映射。
///
/// core 的 `ok_session_process_key(session, keycode, mask)` 约定 X11 keysym 与
/// X11 修饰键掩码（Shift=1, Caps=2, Control=4, Alt/Mod1=8），见
/// core/ffi-headers/openkey.h 与 docs/02 3.1。客户端只负责把平台按键事件翻译成该约定。
public enum X11Keysym {
    /// X11 修饰键掩码（与 openkey.h 注释一致）。
    public struct Mask: OptionSet {
        public let rawValue: UInt32

        public init(rawValue: UInt32) {
            self.rawValue = rawValue
        }

        public static let shift = Mask(rawValue: 1)
        public static let caps = Mask(rawValue: 2)
        public static let control = Mask(rawValue: 4)
        public static let alt = Mask(rawValue: 8)
    }

    /// 无对应字符的特殊键（回车/退格/方向键等）。
    public enum SpecialKey {
        case returnKey
        case keypadEnter
        case tab
        case backspace
        case delete
        case escape
        case home
        case end
        case pageUp
        case pageDown
        case left
        case up
        case right
        case down

        /// 对应的 X11 keysym。
        public var keysym: UInt32 {
            switch self {
            case .returnKey: return 0xff0d
            case .keypadEnter: return 0xff8d
            case .tab: return 0xff09
            case .backspace: return 0xff08
            case .delete: return 0xffff
            case .escape: return 0xff1b
            case .home: return 0xff50
            case .end: return 0xff57
            case .pageUp: return 0xff55
            case .pageDown: return 0xff56
            case .left: return 0xff51
            case .up: return 0xff52
            case .right: return 0xff53
            case .down: return 0xff54
            }
        }
    }

    /// 字符 → X11 keysym（a-z / 0-9 / 空格 / 常用标点）。
    /// 只收录"未加修饰键"的字符；Shift 等状态由调用方以 Mask 表达。
    public static func keysym(for character: Character) -> UInt32? {
        guard let scalar = character.unicodeScalars.first else { return nil }
        return characterMap[scalar.value]
    }

    /// 字符表（Latin 区字符与 X11 keysym 的对应；一次性构建）。
    private static let characterMap: [UInt32: UInt32] = {
        var map: [UInt32: UInt32] = [:]
        for (index, scalar) in "abcdefghijklmnopqrstuvwxyz".unicodeScalars.enumerated() {
            map[scalar.value] = UInt32(0x061 + index) // XK_a .. XK_z
        }
        for (index, scalar) in "0123456789".unicodeScalars.enumerated() {
            map[scalar.value] = UInt32(0x030 + index) // XK_0 .. XK_9
        }
        let punctuation: [(String, UInt32)] = [
            (" ", 0x020), // space
            ("-", 0x02d), ("=", 0x03d), ("[", 0x05b), ("]", 0x05d),
            ("\\", 0x05c), (";", 0x03b), ("'", 0x027), ("`", 0x060),
            (",", 0x02c), (".", 0x02e), ("/", 0x02f),
        ]
        for (text, keysym) in punctuation {
            if let scalar = text.unicodeScalars.first {
                map[scalar.value] = keysym
            }
        }
        return map
    }()
}
