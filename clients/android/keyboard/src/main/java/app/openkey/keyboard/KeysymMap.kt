package app.openkey.keyboard

import android.view.KeyEvent

/**
 * Android KeyEvent keycode → X11 keysym 映射（core `ok_session_process_key` 的输入约定）。
 *
 * keysym 取值与 X11 拉丁区一致：a-z = 0x61..0x7a，0-9 = 0x30..0x39，
 * 可打印标点等于对应 ASCII；功能键用 X11 官方 keysym 常量。
 */
object KeysymMap {

    // X11 keysym 常量（只列用到的）
    const val SPACE = 0x20
    const val BACKSPACE = 0xFF08
    const val RETURN = 0xFF0D
    const val SHIFT_L = 0xFFE1
    const val ESCAPE = 0xFF1B
    const val PAGE_UP = 0xFF55
    const val PAGE_DOWN = 0xFF56

    // X11 修饰键掩码（与 openkey.h 中 ok_session_process_key 的约定一致）
    const val MASK_SHIFT = 1
    const val MASK_CAPS = 2
    const val MASK_CONTROL = 4
    const val MASK_ALT = 8

    /**
     * 硬件键盘 keycode → keysym；未覆盖的键返回 null（IME 交给系统默认处理）。
     * TODO：后续补充方向键（Left/Right/Up/Down = 0xFF51..0xFF54）、Home/End、
     * 以及 F1-F12 等 librime 可能消费的按键。
     */
    fun fromAndroidKeyCode(keyCode: Int): Int? = when (keyCode) {
        in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z -> 'a'.code + (keyCode - KeyEvent.KEYCODE_A)
        in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 -> '0'.code + (keyCode - KeyEvent.KEYCODE_0)
        KeyEvent.KEYCODE_SPACE -> SPACE
        KeyEvent.KEYCODE_ENTER -> RETURN
        KeyEvent.KEYCODE_DEL -> BACKSPACE
        KeyEvent.KEYCODE_COMMA -> ','.code
        KeyEvent.KEYCODE_PERIOD -> '.'.code
        KeyEvent.KEYCODE_MINUS -> '-'.code
        KeyEvent.KEYCODE_EQUALS -> '='.code
        KeyEvent.KEYCODE_SEMICOLON -> ';'.code
        KeyEvent.KEYCODE_APOSTROPHE -> '\''.code
        KeyEvent.KEYCODE_SLASH -> '/'.code
        KeyEvent.KEYCODE_BACKSLASH -> '\\'.code
        KeyEvent.KEYCODE_LEFT_BRACKET -> '['.code
        KeyEvent.KEYCODE_RIGHT_BRACKET -> ']'.code
        KeyEvent.KEYCODE_GRAVE -> '`'.code
        KeyEvent.KEYCODE_ESCAPE -> ESCAPE
        else -> null
    }

    /** 修饰键状态 → X11 mask。 */
    fun maskFromEvent(event: KeyEvent): Int {
        var mask = 0
        if (event.isShiftPressed) mask = mask or MASK_SHIFT
        if (event.isCapsLockOn) mask = mask or MASK_CAPS
        if (event.isCtrlPressed) mask = mask or MASK_CONTROL
        if (event.isAltPressed) mask = mask or MASK_ALT
        return mask
    }
}
