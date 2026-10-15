package app.openkey.core

/**
 * 输入会话的 Kotlin 封装（对应 C ABI 的 `OkSession`）。
 *
 * 每个输入上下文一个会话：IME 在 [app.openkey.keyboard.OpenKeyIme.onStartInputView]
 * 创建、输入结束时保留（仅清除组词），进程销毁时释放。
 * core 未加载或句柄为 0 时所有方法安全降级（返回 false / null）。
 */
class OpenKeySession(val ptr: Long) {

    private var destroyed = false

    private val usable: Boolean
        get() = OpenKeyNative.isLoaded && ptr != 0L && !destroyed

    data class Candidate(val text: String, val comment: String?)

    data class Context(
        val preedit: String,
        val cursorPos: Int,
        val candidates: List<Candidate>,
        val page: Int,
        val pageSize: Int,
        val isLastPage: Boolean,
        val highlighted: Int,
    )

    data class Status(
        val schemaId: String,
        val schemaName: String,
        val isComposing: Boolean,
        val isAsciiMode: Boolean,
        val isFullShape: Boolean,
        val isAsciiPunct: Boolean,
    )

    /** 处理按键；返回是否被 librime 消费。 */
    fun processKey(keycode: Int, mask: Int = 0): Boolean =
        usable && OpenKeyNative.nativeProcessKey(ptr, keycode, mask)

    fun commitComposition(): Boolean = usable && OpenKeyNative.nativeCommitComposition(ptr)

    fun clearComposition() {
        if (usable) OpenKeyNative.nativeClearComposition(ptr)
    }

    /** 当前上下文（预编辑 + 候选页）；core 不可用时返回 null。 */
    fun context(): Context? {
        if (!usable) return null
        val json = OpenKeyNative.nativeGetContext(ptr) ?: return null
        return try {
            val o = org.json.JSONObject(json)
            val arr = o.optJSONArray("candidates")
            val candidates = ArrayList<Candidate>(arr?.length() ?: 0)
            if (arr != null) {
                for (i in 0 until arr.length()) {
                    val c = arr.optJSONObject(i) ?: continue
                    candidates += Candidate(
                        c.optString("text"),
                        if (c.isNull("comment")) null else c.optString("comment"),
                    )
                }
            }
            Context(
                preedit = o.optString("preedit"),
                cursorPos = o.optInt("cursorPos"),
                candidates = candidates,
                page = o.optInt("page"),
                pageSize = o.optInt("pageSize", 5),
                isLastPage = o.optBoolean("isLastPage", true),
                highlighted = o.optInt("highlighted"),
            )
        } catch (e: org.json.JSONException) {
            null
        }
    }

    /** 取待上屏文本（无则 null）。每次调用消费一次，勿重复取。 */
    fun takeCommit(): String? = if (usable) OpenKeyNative.nativeGetCommit(ptr) else null

    /** 状态快照；core 不可用时返回 null。 */
    fun status(): Status? {
        if (!usable) return null
        val json = OpenKeyNative.nativeGetStatus(ptr) ?: return null
        return try {
            val o = org.json.JSONObject(json)
            Status(
                schemaId = o.optString("schemaId"),
                schemaName = o.optString("schemaName"),
                isComposing = o.optBoolean("isComposing"),
                isAsciiMode = o.optBoolean("isAsciiMode"),
                isFullShape = o.optBoolean("isFullShape"),
                isAsciiPunct = o.optBoolean("isAsciiPunct"),
            )
        } catch (e: org.json.JSONException) {
            null
        }
    }

    fun selectCandidate(index: Int): Boolean =
        usable && OpenKeyNative.nativeSelectCandidate(ptr, index)

    /** 翻页；backward=true 上一页。 */
    fun changePage(backward: Boolean): Boolean =
        usable && OpenKeyNative.nativeChangePage(ptr, backward)

    fun setOption(option: String, value: Boolean): Boolean =
        usable && OpenKeyNative.nativeSetOption(ptr, option, value)

    fun getOption(option: String): Boolean =
        usable && OpenKeyNative.nativeGetOption(ptr, option)

    fun selectSchema(schemaId: String): Boolean =
        usable && OpenKeyNative.nativeSelectSchema(ptr, schemaId)

    fun currentSchema(): String? = if (usable) OpenKeyNative.nativeCurrentSchema(ptr) else null

    /** 销毁会话；幂等。 */
    fun destroy() {
        if (!destroyed) {
            destroyed = true
            if (OpenKeyNative.isLoaded && ptr != 0L) {
                OpenKeyNative.nativeSessionDestroy(ptr)
            }
        }
    }
}
