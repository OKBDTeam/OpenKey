package app.openkey.keyboard

import android.content.Context
import android.graphics.Color
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView

/** 候选条上的一个候选（来自 core 的 OkCandidate）。 */
data class Candidate(val text: String, val comment: String?)

/** 键盘 UI 状态快照（IME 每次刷新后整体替换）。 */
data class KeyboardUiState(
    val preedit: String = "",
    val candidates: List<Candidate> = emptyList(),
    val page: Int = 0,
    val pageSize: Int = 5,
    val isLastPage: Boolean = true,
    val highlighted: Int = 0,
    val asciiMode: Boolean = false,
    /** core 未就绪等提示（正常为 null）。 */
    val notice: String? = null,
)

/**
 * OpenKey 键盘面板（传统 View 实现）：候选条 + 字母/功能键。
 *
 * 形态说明（README「与设计文档的偏差」）：docs/02 4.4.2 原计划 Compose 键盘。
 * CI 实测发现 ComposeView 在 InputMethodService 窗口中需要手动挂
 * ViewTreeLifecycleOwner / ViewTreeViewModelStoreOwner /
 * ViewTreeSavedStateRegistryOwner 三者，而这三个符号在 CI 的 AndroidX
 * 依赖组合下无法解析（lifecycle-runtime-android 已在编译类路径中但 Kotlin
 * 侧仍报 Unresolved reference，Compose 侧可正常编译）。为不让 v0.1.0 的
 * 交付阻塞在一个环境性问题上，Phase 1 改用传统 View 体系（功能等价：
 * 同样的按键集、候选条、翻页与中英切换）；Compose 化作为后续任务，
 * 待该 classpath 问题定位后恢复（届时把本类替换为 Compose 实现即可，
 * 对外回调签名不变）。
 *
 * TODO（体验项）：符号/数字面板、候选条左右滑动翻页、窗口 insets 适配、
 * 按键触觉反馈；当前为 Phase 1 最小可用布局。
 */
class KeyboardPanelView(context: Context) : LinearLayout(context) {

    private var onKey: ((keysym: Int, mask: Int) -> Unit)? = null
    private var onSelectCandidate: ((index: Int) -> Unit)? = null
    private var onPage: ((backward: Boolean) -> Unit)? = null
    private var onToggleAscii: (() -> Unit)? = null

    private val preeditLabel: TextView = TextView(context)
    private val candidateRow: LinearLayout = LinearLayout(context)
    private val asciiButton: Button

    init {
        orientation = VERTICAL
        setBackgroundColor(Color.parseColor("#E8EAED"))
        buildCandidateBar()
        asciiButton = buildKeyRows()
    }

    /** 绑定输入回调（IME 侧装配）。 */
    fun bind(
        onKey: (keysym: Int, mask: Int) -> Unit,
        onSelectCandidate: (index: Int) -> Unit,
        onPage: (backward: Boolean) -> Unit,
        onToggleAscii: () -> Unit,
    ) {
        this.onKey = onKey
        this.onSelectCandidate = onSelectCandidate
        this.onPage = onPage
        this.onToggleAscii = onToggleAscii
    }

    /* -------------------------------------------------------------- */
    /* 候选条                                                            */
    /* -------------------------------------------------------------- */

    private fun buildCandidateBar() {
        val bar = LinearLayout(context).apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            layoutParams = LayoutParams(LayoutParams.MATCH_PARENT, dp(40))
        }

        bar.addView(keyButton("‹", weight = 0.6f) { onPage?.invoke(true) })

        preeditLabel.apply {
            textSize = 14f
            setTextColor(Color.DKGRAY)
            gravity = Gravity.CENTER_VERTICAL
            layoutParams = LayoutParams(dp(56), LayoutParams.MATCH_PARENT)
        }
        bar.addView(preeditLabel)

        candidateRow.apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val scroll = HorizontalScrollView(context).apply {
            layoutParams = LayoutParams(0, LayoutParams.MATCH_PARENT, 1f)
            isHorizontalScrollBarEnabled = false
        }
        scroll.addView(candidateRow)
        bar.addView(scroll)

        bar.addView(keyButton("›", weight = 0.6f) { onPage?.invoke(false) })
        addView(bar)
    }

    /* -------------------------------------------------------------- */
    /* 按键面板                                                          */
    /* -------------------------------------------------------------- */

    /** 构建按键行，返回「中/英」按钮（状态刷新时改文案）。 */
    private fun buildKeyRows(): Button {
        val rows: List<List<KeySpec>> = listOf(
            "qwertyuiop".map { KeySpec.Character(it) },
            "asdfghjkl".map { KeySpec.Character(it) },
            listOf(
                KeySpec.Ascii,
                KeySpec.Character('z'), KeySpec.Character('x'), KeySpec.Character('c'),
                KeySpec.Character('v'), KeySpec.Character('b'), KeySpec.Character('n'),
                KeySpec.Character('m'),
                KeySpec.Backspace,
            ),
            listOf(
                KeySpec.Digit(1), KeySpec.Digit(2), KeySpec.Digit(3),
                KeySpec.Digit(4), KeySpec.Digit(5),
                KeySpec.Space, KeySpec.Return,
            ),
        )

        var ascii: Button? = null
        for (row in rows) {
            val rowView = LinearLayout(context).apply {
                orientation = HORIZONTAL
                layoutParams = LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f)
            }
            for (key in row) {
                val button = keyButton(label(key)) {
                    when (key) {
                        is KeySpec.Character ->
                            onKey?.invoke(key.c.code, 0)
                        is KeySpec.Digit ->
                            onKey?.invoke('0'.code + key.digit, 0)
                        KeySpec.Space -> onKey?.invoke(KeysymMap.SPACE, 0)
                        KeySpec.Return -> onKey?.invoke(KeysymMap.RETURN, 0)
                        KeySpec.Backspace -> onKey?.invoke(KeysymMap.BACKSPACE, 0)
                        KeySpec.Ascii -> onToggleAscii?.invoke()
                    }
                }
                if (key == KeySpec.Ascii) {
                    ascii = button
                }
                rowView.addView(button)
            }
            addView(rowView)
        }
        return requireNotNull(ascii) { "按键行缺少中/英键" }
    }

    private fun keyButton(
        text: String,
        weight: Float = 1f,
        onClick: () -> Unit,
    ): Button = Button(context).apply {
        this.text = text
        textSize = 16f
        isAllCaps = false
        layoutParams = LayoutParams(0, LayoutParams.MATCH_PARENT, weight).apply {
            setMargins(dp(2), dp(3), dp(2), dp(3))
        }
        setOnClickListener { onClick() }
    }

    private fun label(key: KeySpec): String = when (key) {
        is KeySpec.Character -> key.c.toString()
        is KeySpec.Digit -> key.digit.toString()
        KeySpec.Space -> "空格"
        KeySpec.Return -> "换行"
        KeySpec.Backspace -> "⌫"
        KeySpec.Ascii -> "中/英"
    }

    /* -------------------------------------------------------------- */
    /* 状态刷新                                                          */
    /* -------------------------------------------------------------- */

    /** 用最新状态刷新候选条与按键态（IME 主线程调用）。 */
    fun updateState(state: KeyboardUiState) {
        preeditLabel.text = state.preedit
        asciiButton.text = if (state.asciiMode) "英" else "中"

        candidateRow.removeAllViews()
        if (state.notice != null) {
            candidateRow.addView(TextView(context).apply {
                text = state.notice
                textSize = 13f
                setTextColor(Color.RED)
            })
            return
        }
        state.candidates.forEachIndexed { index, candidate ->
            candidateRow.addView(TextView(context).apply {
                text = candidate.comment?.let { "${candidate.text} $it" } ?: candidate.text
                textSize = 17f
                setTextColor(if (index == state.highlighted) Color.BLUE else Color.BLACK)
                gravity = Gravity.CENTER
                layoutParams = LayoutParams(dp(72), LayoutParams.MATCH_PARENT)
                setOnClickListener { onSelectCandidate?.invoke(index) }
            })
        }
        if (state.candidates.isEmpty()) {
            candidateRow.addView(TextView(context).apply {
                text = "OpenKey"
                textSize = 13f
                setTextColor(Color.LTGRAY)
            })
        }
    }

    private fun dp(value: Int): Int =
        TypedValue.applyDimension(
            TypedValue.COMPLEX_UNIT_DIP,
            value.toFloat(),
            resources.displayMetrics,
        ).toInt()

    private sealed class KeySpec {
        data class Character(val c: Char) : KeySpec()
        data class Digit(val digit: Int) : KeySpec()
        object Space : KeySpec()
        object Return : KeySpec()
        object Backspace : KeySpec()
        object Ascii : KeySpec()
    }
}
