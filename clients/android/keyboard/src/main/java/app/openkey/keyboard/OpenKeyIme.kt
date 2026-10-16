package app.openkey.keyboard

import android.inputmethodservice.InputMethodService
import android.text.InputType
import android.util.Log
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import app.openkey.core.OpenKeyEngine
import app.openkey.core.OpenKeyNative
import app.openkey.core.OpenKeySession

/**
 * OpenKey 输入法服务（docs/02 4.4.2）。
 *
 * 数据流：按键 → keysym/mask → [OpenKeySession.processKey] → 取 commit 上屏、
 * 取 context 刷新候选条。键盘面板为传统 View 实现（[KeyboardPanelView]），
 * 形态偏差与 Compose 化的后续计划见该类文档与 README。
 */
class OpenKeyIme : InputMethodService() {

    private var keyboardView: KeyboardPanelView? = null

    private var engine: OpenKeyEngine? = null
    private var session: OpenKeySession? = null

    /** 当前输入框是否为密码类（docs/02 4.4.2：密码模式 Phase 4 接入 vault，先做检测）。 */
    private var passwordField = false

    override fun onCreate() {
        super.onCreate()
        initEngine()
    }

    override fun onCreateInputView(): View {
        val panel = KeyboardPanelView(this)
        panel.bind(
            onKey = ::onKey,
            onSelectCandidate = ::onSelectCandidate,
            onPage = ::onPage,
            onToggleAscii = ::onToggleAscii,
        )
        keyboardView = panel
        refresh()
        return panel
    }

    override fun onStartInputView(editorInfo: EditorInfo, restarting: Boolean) {
        super.onStartInputView(editorInfo, restarting)
        passwordField = isPasswordField(editorInfo)
        // TODO（Phase 4 密码管理）：passwordField 时进入键盘内密码模式
        // （解锁 vault → 按包名/URI 匹配 → 逐字 commitText，docs/02 4.4.2/5.2）。
        if (passwordField) {
            Log.i(TAG, "检测到密码输入框（Phase 4 接入 vault）")
        }
        ensureSession()
        refresh()
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        // 保留会话（下次输入直接复用），仅清除组词状态。
        session?.clearComposition()
        refresh()
        super.onFinishInputView(finishingInput)
    }

    override fun onDestroy() {
        session?.destroy()
        session = null
        engine?.destroy()
        engine = null
        keyboardView = null
        super.onDestroy()
    }

    /* -------------------------------------------------------------- */
    /* 硬件键盘                                                         */
    /* -------------------------------------------------------------- */

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        val s = session
        if (s != null) {
            val keysym = KeysymMap.fromAndroidKeyCode(keyCode)
            if (keysym != null) {
                onKey(keysym, KeysymMap.maskFromEvent(event))
                return true
            }
        }
        return super.onKeyDown(keyCode, event)
    }

    // librime 只处理按键按下（process_key 无释放语义），释放事件不转发 core。
    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        val s = session
        if (s != null && KeysymMap.fromAndroidKeyCode(keyCode) != null) {
            return true
        }
        return super.onKeyUp(keyCode, event)
    }

    /* -------------------------------------------------------------- */
    /* 输入管线（面板键盘与硬件键盘共用）                                 */
    /* -------------------------------------------------------------- */

    /** 处理一次按键；未被 librime 消费时对退格/回车做编辑器级兜底。 */
    private fun onKey(keysym: Int, mask: Int) {
        val s = session
        if (s == null) {
            // core 未就绪：直接向编辑器发送可打印字符，保证键盘不是完全砖块。
            fallbackRawKey(keysym)
            return
        }
        val consumed = s.processKey(keysym, mask)
        flushCommit()
        refresh()
        if (!consumed) {
            fallbackRawKey(keysym)
        }
    }

    /** librime 未消费时的兜底：退格删一字、回车换行、可打印字符直接上屏。 */
    private fun fallbackRawKey(keysym: Int) {
        val ic = currentInputConnection ?: return
        when (keysym) {
            KeysymMap.BACKSPACE -> ic.deleteSurroundingText(1, 0)
            KeysymMap.RETURN -> ic.commitText("\n", 1)
            in 0x20..0x7E -> ic.commitText(keysym.toChar().toString(), 1)
            else -> Unit
        }
    }

    /** 把 core 的待上屏文本提交给宿主编辑器。 */
    private fun flushCommit() {
        val text = session?.takeCommit() ?: return
        currentInputConnection?.commitText(text, 1)
    }

    private fun onSelectCandidate(index: Int) {
        val s = session ?: return
        if (s.selectCandidate(index)) {
            flushCommit()
        }
        refresh()
    }

    private fun onPage(backward: Boolean) {
        val s = session ?: return
        if (s.changePage(backward)) {
            refresh()
        }
    }

    /** 中/英切换：core 会话选项 ascii_mode。 */
    private fun onToggleAscii() {
        val s = session ?: return
        val next = !s.getOption("ascii_mode")
        s.setOption("ascii_mode", next)
        refresh()
    }

    /* -------------------------------------------------------------- */
    /* 状态刷新                                                          */
    /* -------------------------------------------------------------- */

    private fun refresh() {
        val s = session
        if (s == null) {
            keyboardView?.updateState(
                KeyboardUiState(
                    notice = if (OpenKeyNative.isLoaded) {
                        "引擎初始化失败，详见日志与 README"
                    } else {
                        "core 未加载，详见 clients/android/README.md"
                    }
                )
            )
            return
        }
        val ctx = s.context()
        val st = s.status()
        keyboardView?.updateState(
            KeyboardUiState(
                preedit = ctx?.preedit ?: "",
                candidates = ctx?.candidates?.map { Candidate(it.text, it.comment) } ?: emptyList(),
                page = ctx?.page ?: 0,
                pageSize = ctx?.pageSize ?: 5,
                isLastPage = ctx?.isLastPage ?: true,
                highlighted = ctx?.highlighted ?: 0,
                asciiMode = st?.isAsciiMode ?: false,
            )
        )
    }

    /* -------------------------------------------------------------- */
    /* 引擎/会话管理                                                    */
    /* -------------------------------------------------------------- */

    private fun initEngine() {
        if (!OpenKeyNative.isLoaded) {
            Log.w(TAG, "core so 未加载（core-binding/src/main/jniLibs 为空），键盘处于降级模式")
            return
        }
        // TODO（Phase 1）：shared 数据目录当前为空；后续从 assets 释放 openkey-base
        // 词库或经组件机制下载，再触发 ok_engine_deploy。
        engine = OpenKeyEngine.createForContext(this)
        if (engine == null) {
            Log.e(TAG, "引擎创建失败")
        }
        refresh()
    }

    private fun ensureSession() {
        if (session != null) return
        val e = engine ?: return
        session = e.openSession()
    }

    private fun isPasswordField(info: EditorInfo): Boolean {
        val variation = info.inputType and InputType.TYPE_MASK_VARIATION
        val klass = info.inputType and InputType.TYPE_MASK_CLASS
        return klass == InputType.TYPE_CLASS_TEXT &&
            (variation == InputType.TYPE_TEXT_VARIATION_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD)
    }

    companion object {
        private const val TAG = "OpenKeyIme"
    }
}
