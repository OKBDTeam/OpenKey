package app.openkey.core

import android.content.Context
import java.io.File

/**
 * core 引擎句柄的 Kotlin 封装（对应 C ABI 的 `OkEngine`）。
 *
 * 引擎是进程级单例资源：IME 进程与主 App 进程各自持有一个实例，
 * 通过同一套用户数据目录（同 uid）共享词库与设置。
 */
class OpenKeyEngine private constructor(private val ptr: Long) {

    private var destroyed = false

    private val usable: Boolean
        get() = OpenKeyNative.isLoaded && ptr != 0L && !destroyed

    /** 最近一次错误详情（无则 null）。 */
    fun lastError(): String? =
        if (usable) OpenKeyNative.nativeEngineLastError(ptr) else null

    fun schemaCount(): Int =
        if (usable) OpenKeyNative.nativeEngineSchemaCount(ptr) else 0

    fun schemaAt(index: Int): SchemaInfo? {
        if (!usable) return null
        val json = OpenKeyNative.nativeEngineSchemaAt(ptr, index) ?: return null
        return try {
            val o = org.json.JSONObject(json)
            SchemaInfo(o.optString("id"), o.optString("name"))
        } catch (e: org.json.JSONException) {
            null
        }
    }

    fun schemas(): List<SchemaInfo> = (0 until schemaCount()).mapNotNull { schemaAt(it) }

    /** 创建输入会话；失败返回 null。 */
    fun openSession(): OpenKeySession? {
        if (!usable) return null
        val s = OpenKeyNative.nativeSessionCreate(ptr)
        return if (s != 0L) OpenKeySession(s) else null
    }

    /** 销毁引擎；幂等，重复调用安全。 */
    fun destroy() {
        if (!destroyed) {
            destroyed = true
            if (OpenKeyNative.isLoaded && ptr != 0L) {
                OpenKeyNative.nativeEngineDestroy(ptr)
            }
        }
    }

    data class SchemaInfo(val id: String, val name: String)

    companion object {

        /**
         * 创建引擎。失败（core so 未加载 / librime 初始化失败 / 数据层打开失败）
         * 返回 null，错误详情见 [lastError] 或日志。
         *
         * @param sharedDataDir librime 共享数据目录（方案与词库）。
         *   TODO（Phase 1）：当前为空目录时 librime 部署无方案，引擎可能创建失败；
         *   后续由 assets 释放 openkey-base 词库 / 组件下载补齐。
         * @param userDataDir librime 用户数据目录（userdb / build）。
         * @param dataDbPath openkey.db 路径（数据层）。
         * @param componentsDir 组件根目录（NULL 时取 {user_data_dir}/components）。
         * @param logDir librime 日志目录。
         * @param minLogLevel glog 级别：0=INFO .. 3=FATAL。
         */
        fun create(
            sharedDataDir: String?,
            userDataDir: String,
            dataDbPath: String?,
            componentsDir: String?,
            logDir: String?,
            minLogLevel: Int = 2,
        ): OpenKeyEngine? {
            if (!OpenKeyNative.isLoaded) return null
            val ptr = OpenKeyNative.nativeEngineCreate(
                sharedDataDir,
                userDataDir,
                dataDbPath,
                componentsDir,
                logDir,
                minLogLevel,
            )
            return if (ptr != 0L) OpenKeyEngine(ptr) else null
        }

        /**
         * 按 Android 常规目录布局创建引擎（IME 与主 App 共用，见 README「目录布局」）：
         * filesDir/openkey/{rime/shared, rime/user, components, log, openkey.db}。
         */
        fun createForContext(context: Context, minLogLevel: Int = 2): OpenKeyEngine? {
            val root = File(context.filesDir, "openkey").apply { mkdirs() }
            fun sub(name: String) = File(root, name).apply { mkdirs() }.absolutePath
            return create(
                sharedDataDir = sub("rime/shared"),
                userDataDir = sub("rime/user"),
                dataDbPath = File(root, "openkey.db").absolutePath,
                componentsDir = sub("components"),
                logDir = sub("log"),
                minLogLevel = minLogLevel,
            )
        }
    }
}
