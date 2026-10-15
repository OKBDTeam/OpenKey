package app.openkey.voicepack

import android.content.Context
import android.util.Log
import java.io.File

/**
 * 语音组件动态加载器（docs/02 4.4.5 / DR-004）。
 *
 * 流程（Phase 6 落地，当前仅状态机与接口骨架）：
 * ```
 * 用户开启语音 → 检查 components_current(voice-engine-<abi>)
 *  → 无 → 展示组件说明（体积/用途/来源）→ 用户确认 → 下载 zip
 *  → SHA-256 校验 → 解压 filesDir/voicepacks/<version>/ → System.load(绝对路径)
 *  → VoiceEngine 就绪；失败（ABI/损坏）→ 清理并回滚旧版本
 * ```
 *
 * 目录约定：下载固定在 :app 进程的 `filesDir/voicepacks/<name>/<version>/`；
 * 键盘进程与主 App 同 uid，对该目录可读，可自行 [VoiceEngine] 的 so 加载。
 */
class VoicePackLoader(private val context: Context) {

    /** 组件状态机。 */
    sealed interface State {
        /** 未安装。 */
        data object Absent : State

        /** 下载/校验/解压中。 */
        data class Downloading(val downloadedBytes: Long, val totalBytes: Long) : State

        /** 已安装就绪（解压目录）。 */
        data class Installed(val dir: File, val version: String) : State

        /** 失败（已清理半成品，旧版本保留可回滚）。 */
        data class Failed(val reason: String) : State
    }

    /**
     * 语音引擎运行时接口（sherpa-onnx C API 之上的薄封装，Phase 6 实现）。
     * 音频采集参数：16kHz / 单声道 / 16bit（与 core 的 openkey-voice 约定一致）。
     */
    interface VoiceEngine {
        /** 加载模型后创建流式识别会话。 */
        fun startStream()

        /** 喂入 PCM 采样（16bit LE）。 */
        fun feed(pcm: ByteArray, length: Int)

        /** 结束采集，等待最终结果。 */
        fun stop()

        fun destroy()
    }

    var state: State = State.Absent
        private set

    private val packsRoot: File
        get() = File(context.filesDir, "voicepacks").apply { mkdirs() }

    /** 本机 ABI（如 arm64-v8a）；组件包按 ABI 分发。 */
    private val abi: String
        get() = android.os.Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown"

    /**
     * 确保组件就绪：下载 → 校验 → 原子解压 → 登记。
     *
     * TODO（Phase 6）：接 core 的 `ok_component_ensure(name, progress, ctx)`
     * （openkey.h 已声明，需在 core-binding 的 shim 中补 JNI 转发，含进度回调），
     * 或在 Kotlin 侧用 versions.json + reqwest 之外的 HTTP 客户端实现后由 core 登记。
     * 当前仅做状态机占位，不发起任何网络请求（遵循"无静默网络行为"原则）。
     */
    fun ensureInstalled(
        componentName: String = "voice-engine-android",
        onProgress: (downloaded: Long, total: Long) -> Unit = { _, _ -> },
    ): State {
        Log.i(TAG, "ensureInstalled($componentName, abi=$abi)：Phase 6 TODO，当前不下载")
        return state
    }

    /**
     * 从已安装目录动态加载引擎 so（System.load 绝对路径，DR-004）。
     *
     * TODO（Phase 6）：加载顺序 libc++_shared.so → libonnxruntime.so →
     * libsherpa-onnx-c-api.so（组件包自带依赖，manifest 声明最低 NDK ABI）；
     * 失败时清理并回滚旧版本（R-02 降级预案：保留"精简版 APK 内置引擎"开关）。
     */
    fun loadEngine(): VoiceEngine {
        throw NotImplementedError("VoicePackLoader.loadEngine：Phase 6 实现")
    }

    /** 已安装版本目录（无则 null）。 */
    fun installedDir(componentName: String = "voice-engine-android"): File? {
        val dir = File(packsRoot, componentName)
        return dir.takeIf { it.isDirectory && (it.listFiles()?.isNotEmpty() == true) }
    }

    /** 卸载/清理指定组件（保留接口；Phase 6 与 core 注册表联动）。 */
    fun uninstall(componentName: String = "voice-engine-android") {
        File(packsRoot, componentName).deleteRecursively()
        state = State.Absent
    }

    private companion object {
        const val TAG = "VoicePackLoader"
    }
}
