/*
 * OpenKey JNI shim（clients/android/core-binding/src/main/cpp）
 *
 * 这是 Android 侧唯一直接接触 core C ABI（裸指针 / OkContext 等结构体）的代码。
 * 职责：调用 core/ffi-headers/openkey.h 声明的 ok_* 函数，把结果拍平成
 * JSON 字符串或标量，通过 JNI 交给 Kotlin 侧（org.json 解析）。
 *
 * 约束（自查要点）：
 *  - 只调用 openkey.h 中真实存在的函数，不新增/不改写 core 语义；
 *  - JNI 符号名与 Kotlin external fun 严格对应：
 *      Java_app_openkey_core_OpenKeyNative_<方法名>
 *  - 不链接 libopenkey_ffi.so：ok_* 为未定义符号，运行时由
 *    System.loadLibrary("openkey_ffi") 加载的 so 在同一命名空间解析；
 *  - 字符串均为 UTF-8；JSON 序列化为手写极简实现（含转义）。
 */
#include <jni.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "openkey.h"

/* ---------------------------------------------------------------------- */
/* 极简 JSON 字符串构建器（仅本文件使用）                                    */
/* ---------------------------------------------------------------------- */

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} JStr;

#define JSTR_INIT_CAP 256

static int jstr_reserve(JStr *s, size_t extra) {
    if (s->data != NULL && s->len + extra + 1 <= s->cap) {
        return 1;
    }
    size_t ncap = (s->cap != 0) ? s->cap : JSTR_INIT_CAP;
    while (ncap < s->len + extra + 1) {
        ncap *= 2;
    }
    char *nb = (char *)realloc(s->data, ncap);
    if (nb == NULL) {
        return 0;
    }
    s->data = nb;
    s->cap = ncap;
    return 1;
}

static void jstr_putn(JStr *s, const char *str, size_t n) {
    if (!jstr_reserve(s, n)) {
        return;
    }
    memcpy(s->data + s->len, str, n);
    s->len += n;
    s->data[s->len] = '\0';
}

static void jstr_puts(JStr *s, const char *str) {
    jstr_putn(s, str, strlen(str));
}

static void jstr_putc(JStr *s, char c) {
    jstr_putn(s, &c, 1);
}

static void jstr_put_int(JStr *s, long v) {
    char tmp[32];
    snprintf(tmp, sizeof(tmp), "%ld", v);
    jstr_puts(s, tmp);
}

static void jstr_put_bool(JStr *s, int v) {
    jstr_puts(s, v ? "true" : "false");
}

/* 追加 JSON 字符串字面量（带引号与转义）；NULL 输出 ""（避免 org.json
 * optString 对 JSON null 的处理歧义）。UTF-8 字节原样透传，
 * 仅转义引号/反斜杠/控制字符。 */
static void jstr_put_json_string(JStr *s, const char *str) {
    jstr_putc(s, '"');
    if (str != NULL) {
        const unsigned char *p;
        for (p = (const unsigned char *)str; *p != '\0'; p++) {
            unsigned char c = *p;
            switch (c) {
            case '"':
                jstr_puts(s, "\\\"");
                break;
            case '\\':
                jstr_puts(s, "\\\\");
                break;
            case '\b':
                jstr_puts(s, "\\b");
                break;
            case '\f':
                jstr_puts(s, "\\f");
                break;
            case '\n':
                jstr_puts(s, "\\n");
                break;
            case '\r':
                jstr_puts(s, "\\r");
                break;
            case '\t':
                jstr_puts(s, "\\t");
                break;
            default:
                if (c < 0x20) {
                    char tmp[8];
                    snprintf(tmp, sizeof(tmp), "\\u%04x", (unsigned)c);
                    jstr_puts(s, tmp);
                } else {
                    jstr_putc(s, (char)c);
                }
            }
        }
    }
    jstr_putc(s, '"');
}

static void jstr_free(JStr *s) {
    free(s->data);
    s->data = NULL;
    s->len = 0;
    s->cap = 0;
}

/* 构建器 -> jstring；内存不足时返回 NULL（Kotlin 侧按失败处理）。 */
static jstring to_jstring(JNIEnv *env, JStr *s) {
    if (s->data == NULL) {
        return NULL;
    }
    jstring r = (*env)->NewStringUTF(env, s->data);
    jstr_free(s);
    return r;
}

/* ---------------------------------------------------------------------- */
/* 辅助                                                                     */
/* ---------------------------------------------------------------------- */

static struct OkEngine *engine_of(jlong ptr) {
    return (struct OkEngine *)(intptr_t)ptr;
}

static struct OkSession *session_of(jlong ptr) {
    return (struct OkSession *)(intptr_t)ptr;
}

/* ---------------------------------------------------------------------- */
/* 引擎生命周期                                                             */
/* ---------------------------------------------------------------------- */

JNIEXPORT jstring JNICALL
Java_app_openkey_core_OpenKeyNative_nativeVersionString(JNIEnv *env, jobject thiz) {
    (void)thiz;
    return (*env)->NewStringUTF(env, ok_version_string());
}

JNIEXPORT jlong JNICALL
Java_app_openkey_core_OpenKeyNative_nativeEngineCreate(JNIEnv *env, jobject thiz,
                                                       jstring shared_dir, jstring user_dir,
                                                       jstring db_path, jstring components_dir,
                                                       jstring log_dir, jint min_log_level) {
    (void)thiz;
    struct OkEngineConfig cfg;
    memset(&cfg, 0, sizeof(cfg));

    /* GetStringUTFChars 返回修改版 UTF-8；目录路径均为 ASCII，安全。
     * 释放必须在 ok_engine_create 返回之后（引擎内部已拷贝）。 */
    if (shared_dir != NULL) {
        cfg.shared_data_dir = (*env)->GetStringUTFChars(env, shared_dir, NULL);
    }
    if (user_dir != NULL) {
        cfg.user_data_dir = (*env)->GetStringUTFChars(env, user_dir, NULL);
    }
    if (db_path != NULL) {
        cfg.data_db_path = (*env)->GetStringUTFChars(env, db_path, NULL);
    }
    if (components_dir != NULL) {
        cfg.components_dir = (*env)->GetStringUTFChars(env, components_dir, NULL);
    }
    if (log_dir != NULL) {
        cfg.log_dir = (*env)->GetStringUTFChars(env, log_dir, NULL);
    }
    cfg.min_log_level = (int)min_log_level;

    struct OkEngine *engine = ok_engine_create(&cfg);

    if (shared_dir != NULL && cfg.shared_data_dir != NULL) {
        (*env)->ReleaseStringUTFChars(env, shared_dir, cfg.shared_data_dir);
    }
    if (user_dir != NULL && cfg.user_data_dir != NULL) {
        (*env)->ReleaseStringUTFChars(env, user_dir, cfg.user_data_dir);
    }
    if (db_path != NULL && cfg.data_db_path != NULL) {
        (*env)->ReleaseStringUTFChars(env, db_path, cfg.data_db_path);
    }
    if (components_dir != NULL && cfg.components_dir != NULL) {
        (*env)->ReleaseStringUTFChars(env, components_dir, cfg.components_dir);
    }
    if (log_dir != NULL && cfg.log_dir != NULL) {
        (*env)->ReleaseStringUTFChars(env, log_dir, cfg.log_dir);
    }
    return (jlong)(intptr_t)engine;
}

JNIEXPORT void JNICALL
Java_app_openkey_core_OpenKeyNative_nativeEngineDestroy(JNIEnv *env, jobject thiz,
                                                        jlong engine_ptr) {
    (void)env;
    (void)thiz;
    ok_engine_destroy(engine_of(engine_ptr));
}

JNIEXPORT jstring JNICALL
Java_app_openkey_core_OpenKeyNative_nativeEngineLastError(JNIEnv *env, jobject thiz,
                                                          jlong engine_ptr) {
    (void)thiz;
    const char *err = ok_last_error(engine_of(engine_ptr));
    if (err == NULL) {
        return NULL;
    }
    return (*env)->NewStringUTF(env, err);
}

JNIEXPORT jint JNICALL
Java_app_openkey_core_OpenKeyNative_nativeEngineSchemaCount(JNIEnv *env, jobject thiz,
                                                            jlong engine_ptr) {
    (void)env;
    (void)thiz;
    struct OkEngine *engine = engine_of(engine_ptr);
    if (engine == NULL) {
        return 0;
    }
    /* uintptr_t -> jint：方案数量不可能超过 int 范围 */
    return (jint)ok_engine_schema_count(engine);
}

/* 返回 {"id":"..","name":".."}；失败返回 NULL。 */
JNIEXPORT jstring JNICALL
Java_app_openkey_core_OpenKeyNative_nativeEngineSchemaAt(JNIEnv *env, jobject thiz,
                                                         jlong engine_ptr, jint index) {
    (void)thiz;
    struct OkEngine *engine = engine_of(engine_ptr);
    if (engine == NULL) {
        return NULL;
    }
    char id_buf[256];
    char name_buf[256];
    memset(id_buf, 0, sizeof(id_buf));
    memset(name_buf, 0, sizeof(name_buf));
    if (!ok_engine_schema_at(engine, (uintptr_t)index, id_buf, sizeof(id_buf), name_buf,
                             sizeof(name_buf))) {
        return NULL;
    }
    JStr s = {0};
    jstr_puts(&s, "{\"id\":");
    jstr_put_json_string(&s, id_buf);
    jstr_puts(&s, ",\"name\":");
    jstr_put_json_string(&s, name_buf);
    jstr_putc(&s, '}');
    return to_jstring(env, &s);
}

/* ---------------------------------------------------------------------- */
/* 会话生命周期                                                             */
/* ---------------------------------------------------------------------- */

JNIEXPORT jlong JNICALL
Java_app_openkey_core_OpenKeyNative_nativeSessionCreate(JNIEnv *env, jobject thiz,
                                                        jlong engine_ptr) {
    (void)env;
    (void)thiz;
    return (jlong)(intptr_t)ok_session_create(engine_of(engine_ptr));
}

JNIEXPORT void JNICALL
Java_app_openkey_core_OpenKeyNative_nativeSessionDestroy(JNIEnv *env, jobject thiz,
                                                         jlong session_ptr) {
    (void)env;
    (void)thiz;
    ok_session_destroy(session_of(session_ptr));
}

JNIEXPORT jboolean JNICALL
Java_app_openkey_core_OpenKeyNative_nativeProcessKey(JNIEnv *env, jobject thiz,
                                                     jlong session_ptr, jint keycode,
                                                     jint mask) {
    (void)env;
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL) {
        return JNI_FALSE;
    }
    return (jboolean)(ok_session_process_key(session, (uint32_t)keycode, (uint32_t)mask)
                          ? JNI_TRUE
                          : JNI_FALSE);
}

JNIEXPORT jboolean JNICALL
Java_app_openkey_core_OpenKeyNative_nativeCommitComposition(JNIEnv *env, jobject thiz,
                                                            jlong session_ptr) {
    (void)env;
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL) {
        return JNI_FALSE;
    }
    return (jboolean)(ok_session_commit_composition(session) ? JNI_TRUE : JNI_FALSE);
}

JNIEXPORT void JNICALL
Java_app_openkey_core_OpenKeyNative_nativeClearComposition(JNIEnv *env, jobject thiz,
                                                           jlong session_ptr) {
    (void)env;
    (void)thiz;
    ok_session_clear_composition(session_of(session_ptr));
}

/* 拍平 OkContext：
 * {"preedit":"..","cursorPos":n,"selStart":n,"selEnd":n,
 *  "candidates":[{"text":"..","comment":".."}],
 *  "page":n,"pageSize":n,"isLastPage":bool,"highlighted":n}
 * 候选数组指针归会话所有，必须在本次调用内完成序列化（下次调用前有效）。 */
JNIEXPORT jstring JNICALL
Java_app_openkey_core_OpenKeyNative_nativeGetContext(JNIEnv *env, jobject thiz,
                                                     jlong session_ptr) {
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL) {
        return NULL;
    }
    struct OkContext ctx;
    memset(&ctx, 0, sizeof(ctx));
    if (!ok_session_get_context(session, &ctx)) {
        return NULL;
    }
    JStr s = {0};
    jstr_putc(&s, '{');
    jstr_puts(&s, "\"preedit\":");
    jstr_put_json_string(&s, ctx.preedit);
    jstr_puts(&s, ",\"cursorPos\":");
    jstr_put_int(&s, (long)ctx.cursor_pos);
    jstr_puts(&s, ",\"selStart\":");
    jstr_put_int(&s, (long)ctx.sel_start);
    jstr_puts(&s, ",\"selEnd\":");
    jstr_put_int(&s, (long)ctx.sel_end);
    jstr_puts(&s, ",\"candidates\":[");
    if (ctx.candidates != NULL) {
        uintptr_t i;
        for (i = 0; i < ctx.num_candidates; i++) {
            if (i > 0) {
                jstr_putc(&s, ',');
            }
            jstr_puts(&s, "{\"text\":");
            jstr_put_json_string(&s, ctx.candidates[i].text);
            jstr_puts(&s, ",\"comment\":");
            if (ctx.candidates[i].comment != NULL) {
                jstr_put_json_string(&s, ctx.candidates[i].comment);
            } else {
                jstr_puts(&s, "null");
            }
            jstr_putc(&s, '}');
        }
    }
    jstr_puts(&s, "],\"page\":");
    jstr_put_int(&s, (long)ctx.page_no);
    jstr_puts(&s, ",\"pageSize\":");
    jstr_put_int(&s, (long)ctx.page_size);
    jstr_puts(&s, ",\"isLastPage\":");
    jstr_put_bool(&s, ctx.is_last_page ? 1 : 0);
    jstr_puts(&s, ",\"highlighted\":");
    jstr_put_int(&s, (long)ctx.highlighted_candidate_index);
    jstr_putc(&s, '}');
    return to_jstring(env, &s);
}

/* 取待上屏文本；无待上屏文本返回 NULL。
 * TODO: 缓冲区 8192 字节，超长文本（罕见）会返回 false，按无提交处理。 */
JNIEXPORT jstring JNICALL
Java_app_openkey_core_OpenKeyNative_nativeGetCommit(JNIEnv *env, jobject thiz,
                                                    jlong session_ptr) {
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL) {
        return NULL;
    }
    char buf[8192];
    memset(buf, 0, sizeof(buf));
    if (!ok_session_get_commit(session, buf, sizeof(buf))) {
        return NULL;
    }
    return (*env)->NewStringUTF(env, buf);
}

/* 拍平 OkStatus：
 * {"schemaId":"..","schemaName":"..","isComposing":bool,"isAsciiMode":bool,
 *  "isFullShape":bool,"isAsciiPunct":bool} */
JNIEXPORT jstring JNICALL
Java_app_openkey_core_OpenKeyNative_nativeGetStatus(JNIEnv *env, jobject thiz,
                                                    jlong session_ptr) {
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL) {
        return NULL;
    }
    struct OkStatus st;
    memset(&st, 0, sizeof(st));
    if (!ok_session_get_status(session, &st)) {
        return NULL;
    }
    JStr s = {0};
    jstr_putc(&s, '{');
    jstr_puts(&s, "\"schemaId\":");
    jstr_put_json_string(&s, st.schema_id);
    jstr_puts(&s, ",\"schemaName\":");
    jstr_put_json_string(&s, st.schema_name);
    jstr_puts(&s, ",\"isComposing\":");
    jstr_put_bool(&s, st.is_composing ? 1 : 0);
    jstr_puts(&s, ",\"isAsciiMode\":");
    jstr_put_bool(&s, st.is_ascii_mode ? 1 : 0);
    jstr_puts(&s, ",\"isFullShape\":");
    jstr_put_bool(&s, st.is_full_shape ? 1 : 0);
    jstr_puts(&s, ",\"isAsciiPunct\":");
    jstr_put_bool(&s, st.is_ascii_punct ? 1 : 0);
    jstr_putc(&s, '}');
    return to_jstring(env, &s);
}

JNIEXPORT jboolean JNICALL
Java_app_openkey_core_OpenKeyNative_nativeSelectCandidate(JNIEnv *env, jobject thiz,
                                                          jlong session_ptr, jint index) {
    (void)env;
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL) {
        return JNI_FALSE;
    }
    return (jboolean)(ok_session_select_candidate(session, (uintptr_t)index) ? JNI_TRUE
                                                                            : JNI_FALSE);
}

JNIEXPORT jboolean JNICALL
Java_app_openkey_core_OpenKeyNative_nativeChangePage(JNIEnv *env, jobject thiz,
                                                     jlong session_ptr, jboolean backward) {
    (void)env;
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL) {
        return JNI_FALSE;
    }
    return (jboolean)(ok_session_change_page(session, backward ? 1 : 0) ? JNI_TRUE : JNI_FALSE);
}

JNIEXPORT jboolean JNICALL
Java_app_openkey_core_OpenKeyNative_nativeSetOption(JNIEnv *env, jobject thiz,
                                                    jlong session_ptr, jstring option,
                                                    jboolean value) {
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL || option == NULL) {
        return JNI_FALSE;
    }
    const char *opt = (*env)->GetStringUTFChars(env, option, NULL);
    if (opt == NULL) {
        return JNI_FALSE;
    }
    jboolean r = (jboolean)(ok_session_set_option(session, opt, value ? 1 : 0) ? JNI_TRUE
                                                                              : JNI_FALSE);
    (*env)->ReleaseStringUTFChars(env, option, opt);
    return r;
}

JNIEXPORT jboolean JNICALL
Java_app_openkey_core_OpenKeyNative_nativeGetOption(JNIEnv *env, jobject thiz,
                                                    jlong session_ptr, jstring option) {
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL || option == NULL) {
        return JNI_FALSE;
    }
    const char *opt = (*env)->GetStringUTFChars(env, option, NULL);
    if (opt == NULL) {
        return JNI_FALSE;
    }
    /* ok_session_get_option 返回选项值本身（OkBool），不是成功标志 */
    jboolean r = (jboolean)(ok_session_get_option(session, opt) ? JNI_TRUE : JNI_FALSE);
    (*env)->ReleaseStringUTFChars(env, option, opt);
    return r;
}

JNIEXPORT jboolean JNICALL
Java_app_openkey_core_OpenKeyNative_nativeSelectSchema(JNIEnv *env, jobject thiz,
                                                       jlong session_ptr, jstring schema_id) {
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL || schema_id == NULL) {
        return JNI_FALSE;
    }
    const char *id = (*env)->GetStringUTFChars(env, schema_id, NULL);
    if (id == NULL) {
        return JNI_FALSE;
    }
    jboolean r = (jboolean)(ok_session_select_schema(session, id) ? JNI_TRUE : JNI_FALSE);
    (*env)->ReleaseStringUTFChars(env, schema_id, id);
    return r;
}

JNIEXPORT jstring JNICALL
Java_app_openkey_core_OpenKeyNative_nativeCurrentSchema(JNIEnv *env, jobject thiz,
                                                        jlong session_ptr) {
    (void)thiz;
    struct OkSession *session = session_of(session_ptr);
    if (session == NULL) {
        return NULL;
    }
    const char *id = ok_session_current_schema(session);
    if (id == NULL) {
        return NULL;
    }
    /* 指针归会话所有：立即拷贝为 jstring */
    return (*env)->NewStringUTF(env, id);
}
