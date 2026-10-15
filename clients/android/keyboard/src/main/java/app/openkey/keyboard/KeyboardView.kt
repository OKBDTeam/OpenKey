package app.openkey.keyboard

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/** 候选条上的一个候选（来自 core 的 OkCandidate）。 */
data class Candidate(val text: String, val comment: String?)

/** 键盘 UI 状态快照（IME 每次刷新后整体替换，Compose 直接重组）。 */
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
 * Compose 键盘：候选条 + 字母/功能键。
 *
 * TODO（体验项）：符号/数字面板、候选条左右滑动翻页、窗口 insets 适配、
 * 按键触觉反馈；当前为 Phase 0/1 最小可用布局。
 *
 * @param onKey keysym + mask 直接透传给 core（see KeysymMap）。
 * @param onSelectCandidate 点击候选下标。
 * @param onPage backward=true 上一页。
 * @param onToggleAscii 中/英切换（core 选项 ascii_mode）。
 */
@Composable
fun KeyboardView(
    state: KeyboardUiState,
    onKey: (keysym: Int, mask: Int) -> Unit,
    onSelectCandidate: (index: Int) -> Unit,
    onPage: (backward: Boolean) -> Unit,
    onToggleAscii: () -> Unit,
) {
    Surface(color = MaterialTheme.colorScheme.surfaceVariant) {
        Column(modifier = Modifier.fillMaxWidth()) {
            CandidateBar(
                state = state,
                onSelectCandidate = onSelectCandidate,
                onPage = onPage,
            )
            // 字母键 keysym = 对应小写字母 ASCII（与 X11 一致）
            KeyRow("qwertyuiop".map { KeySpec("$it", it.code) }, onKey)
            KeyRow("asdfghjkl".map { KeySpec("$it", it.code) }, onKey)
            KeyRow(
                buildList {
                    add(KeySpec("Shift", KeysymMap.SHIFT_L, 1.5f, KeyKind.SHIFT))
                    "zxcvbnm".forEach { add(KeySpec("$it", it.code)) }
                    add(KeySpec("退格", KeysymMap.BACKSPACE, 1.5f, KeyKind.BACKSPACE))
                },
                onKey,
            )
            KeyRow(
                listOf(
                    KeySpec(if (state.asciiMode) "英" else "中", 0, 1.5f, KeyKind.ASCII),
                    KeySpec("，", ','.code),
                    KeySpec("空格", KeysymMap.SPACE, 4f, KeyKind.SPACE),
                    KeySpec("。", '.'.code),
                    KeySpec("回车", KeysymMap.RETURN, 1.5f, KeyKind.ENTER),
                ),
                onKey,
                onAsciiToggle = onToggleAscii,
            )
        }
    }
}

/* ------------------------------------------------------------------ */
/* 候选条                                                              */
/* ------------------------------------------------------------------ */

@Composable
private fun CandidateBar(
    state: KeyboardUiState,
    onSelectCandidate: (index: Int) -> Unit,
    onPage: (backward: Boolean) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(48.dp)
            .background(MaterialTheme.colorScheme.surface)
            .padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val notice = state.notice
        if (notice != null) {
            Text(
                text = notice,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            return@Row
        }
        if (state.preedit.isNotEmpty()) {
            Text(
                text = state.preedit,
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(horizontal = 8.dp),
            )
        }
        LazyRow(modifier = Modifier.weight(1f)) {
            itemsIndexed(state.candidates) { index, candidate ->
                CandidateItem(
                    candidate = candidate,
                    highlighted = index == state.highlighted,
                    onClick = { onSelectCandidate(index) },
                )
            }
        }
        // 翻页：‹ 上一页 / 页码 / › 下一页
        TextButton(onClick = { onPage(true) }, enabled = state.page > 0) {
            Text("‹")
        }
        Text(
            text = "${state.page + 1}",
            style = MaterialTheme.typography.bodySmall,
        )
        TextButton(onClick = { onPage(false) }, enabled = !state.isLastPage) {
            Text("›")
        }
    }
}

@Composable
private fun CandidateItem(
    candidate: Candidate,
    highlighted: Boolean,
    onClick: () -> Unit,
) {
    val bg =
        if (highlighted) MaterialTheme.colorScheme.primaryContainer
        else MaterialTheme.colorScheme.surface
    Box(
        modifier = Modifier
            .padding(horizontal = 3.dp, vertical = 6.dp)
            .clip(RoundedCornerShape(6.dp))
            .background(bg)
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 4.dp),
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(text = candidate.text, style = MaterialTheme.typography.bodyLarge)
            if (!candidate.comment.isNullOrEmpty()) {
                Text(
                    text = candidate.comment,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }
    }
}

/* ------------------------------------------------------------------ */
/* 按键区                                                              */
/* ------------------------------------------------------------------ */

private enum class KeyKind { CHAR, SHIFT, BACKSPACE, SPACE, ENTER, ASCII }

private data class KeySpec(
    val label: String,
    val keysym: Int,
    val weight: Float = 1f,
    val kind: KeyKind = KeyKind.CHAR,
)

@Composable
private fun KeyRow(
    keys: List<KeySpec>,
    onKey: (keysym: Int, mask: Int) -> Unit,
    onAsciiToggle: () -> Unit = {},
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 3.dp, vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        keys.forEach { spec ->
            KeyButton(spec = spec, onKey = onKey, onAsciiToggle = onAsciiToggle)
        }
    }
}

@Composable
private fun RowScope.KeyButton(
    spec: KeySpec,
    onKey: (keysym: Int, mask: Int) -> Unit,
    onAsciiToggle: () -> Unit,
) {
    Box(
        modifier = Modifier
            .padding(1.dp)
            .height(50.dp)
            .weight(spec.weight)
            .clip(RoundedCornerShape(6.dp))
            .background(
                when (spec.kind) {
                    KeyKind.CHAR -> MaterialTheme.colorScheme.surface
                    KeyKind.ENTER -> MaterialTheme.colorScheme.primaryContainer
                    else -> MaterialTheme.colorScheme.secondaryContainer
                }
            )
            .clickable {
                when (spec.kind) {
                    // Shift 键在 OpenKey 里是中/英切换入口（docs/02 5.1），
                    // 与第四行的「中/英」键等价，均走 core 选项 ascii_mode。
                    KeyKind.SHIFT, KeyKind.ASCII -> onAsciiToggle()
                    else -> onKey(spec.keysym, 0)
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = spec.label,
            style = MaterialTheme.typography.titleMedium,
            textAlign = TextAlign.Center,
        )
    }
}
