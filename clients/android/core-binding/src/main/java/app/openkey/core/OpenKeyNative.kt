package app.openkey.core

import android.util.Log

/**
 * core（openkey-ffi）C ABI 的 JNI 入口。
 *
 * 符号约定：`Java_<包名以下划线连接>_<类名>_<方法名>`，即
 * `Java_app_openkey_core_OpenKeyNative_nativeXxx`，
 * 与 cpp/openkey_jni_shim.c 中的 JNI 导出函数一一对应。
 *
 * 加载顺序：先加载 core so（libopenkey_ffi.so，由 CI/本地构建放入
 * core-binding/src/main/jniLibs/<abi>/），再加载本模块编译出的
 * libopenkey_jni_shim.so——shim 中未定义的 ok_* 符号在运行时由前者解析。
 * 任一缺失时 [isLoaded] 为 false，上层封装全部安全降级（不抛异常）。
 */
object OpenKeyNative {

    private const val TAG = "OpenKeyNative"

    private val loaded: Boolean = try {
        System.loadLibrary("openkey_ffi")
        System.loadLibrary("openkey_jni_shim")
        true
    } catch (e: Throwable) {
        Log.e(TAG, "core 动态库加载失败，键盘/设置不可用。构建与放置方法见 clients/android/README.md", e)
        false
    }

    /** core 是否可用（两个 so 均加载成功）。 */
    val isLoaded: Boolean
        get() = loaded

    // ---- 引擎 ----

    external fun nativeVersionString(): String

    /** 创建引擎；失败返回 0（详情用 [nativeEngineLastError]）。 */
    external fun nativeEngineCreate(
        sharedDataDir: String?,
        userDataDir: String,
        dataDbPath: String?,
        componentsDir: String?,
        logDir: String?,
        minLogLevel: Int,
    ): Long

    external fun nativeEngineDestroy(enginePtr: Long)

    external fun nativeEngineLastError(enginePtr: Long): String?

    external fun nativeEngineSchemaCount(enginePtr: Long): Int

    /** 返回 {"id":"..","name":".."}；失败返回 null。 */
    external fun nativeEngineSchemaAt(enginePtr: Long, index: Int): String?

    // ---- 会话 ----

    external fun nativeSessionCreate(enginePtr: Long): Long

    external fun nativeSessionDestroy(sessionPtr: Long)

    /** keycode 为 X11 keysym；mask：Shift=1, Caps=2, Control=4, Alt=8。返回是否被 librime 消费。 */
    external fun nativeProcessKey(sessionPtr: Long, keycode: Int, mask: Int): Boolean

    external fun nativeCommitComposition(sessionPtr: Long): Boolean

    external fun nativeClearComposition(sessionPtr: Long)

    /** 拍平后的 OkContext JSON；失败返回 null。 */
    external fun nativeGetContext(sessionPtr: Long): String?

    /** 待上屏文本；无则 null。 */
    external fun nativeGetCommit(sessionPtr: Long): String?

    /** 拍平后的 OkStatus JSON；失败返回 null。 */
    external fun nativeGetStatus(sessionPtr: Long): String?

    external fun nativeSelectCandidate(sessionPtr: Long, index: Int): Boolean

    external fun nativeChangePage(sessionPtr: Long, backward: Boolean): Boolean

    external fun nativeSetOption(sessionPtr: Long, option: String, value: Boolean): Boolean

    /** 读取选项值本身（无"失败"语义，选项未设置时 librime 返回 false）。 */
    external fun nativeGetOption(sessionPtr: Long, option: String): Boolean

    external fun nativeSelectSchema(sessionPtr: Long, schemaId: String): Boolean

    /** 当前方案 ID；无则 null。 */
    external fun nativeCurrentSchema(sessionPtr: Long): String?

    // TODO（后续阶段按需补充，均已在 openkey.h 中存在、shim 加一个转发即可）：
    //  - ok_engine_deploy / ok_engine_sync_user_data / sync_export / sync_import（Phase 3 同步）
    //  - ok_component_set_manifest / ok_component_ensure / ok_component_installed /
    //    ok_component_current_version（Phase 0 组件基建、Phase 6 语音）
    //  - ok_setting_set / ok_setting_get（设置条目同步）
}
