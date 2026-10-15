/*
 * OpenKey C ABI（由 core/crates/openkey-ffi 的 cbindgen 自动生成，请勿手改）。
* 生成命令：cd core && cargo build -p openkey-ffi
* 五端客户端只应包含本头文件与 core 交互（docs/02 2.1 职责边界铁律）。
*
* 字符串约定：
*  - *_buf/len 形式的输出：调用方提供缓冲区，函数写入 NUL 结尾 UTF-8；
*    缓冲区不足返回 false。
*  - 直接返回 const char* 的：指针归对象所有，下次对同一对象调用前有效。
*  - 错误详情：ok_last_error() 返回，直至下次出错前有效。
*/

#ifndef OPENKEY_H
#define OPENKEY_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * 组件确保就绪的状态码（`ok_component_ensure` 返回值）。
 */
#define OK_COMPONENT_OK 0

/**
 * 失败（详情见 `ok_last_error`）。
 */
#define OK_COMPONENT_ERROR -1

/**
 * 清单中无此组件。
 */
#define OK_COMPONENT_NOT_IN_MANIFEST -2

/**
 * 下载失败。
 */
#define OK_COMPONENT_DOWNLOAD_FAILED -3

/**
 * SHA-256 校验失败。
 */
#define OK_COMPONENT_CHECKSUM_FAILED -4

/**
 * 引擎句柄（不透明）。
 */
typedef struct OkEngine OkEngine;

/**
 * 会话句柄（不透明）。
 */
typedef struct OkSession OkSession;

/**
 * 引擎初始化配置（调用方填充；字符串字段可为 NULL）。
 */
typedef struct OkEngineConfig {
  /**
   * librime 共享数据目录（方案与词库）。
   */
  const char *shared_data_dir;
  /**
   * librime 用户数据目录（userdb / build）。
   */
  const char *user_data_dir;
  /**
   * `openkey.db` 路径（数据层；NULL 则不启用数据层）。
   */
  const char *data_db_path;
  /**
   * 组件根目录（NULL 时取 `{user_data_dir}/components`）。
   */
  const char *components_dir;
  /**
   * librime 日志目录（NULL 为默认）。
   */
  const char *log_dir;
  /**
   * glog 最小日志级别（0=INFO .. 3=FATAL）。
   */
  int min_log_level;
} OkEngineConfig;

/**
 * C 布尔类型。
 */
typedef bool OkBool;

/**
 * 候选字词（指针归会话所有，见文件头字符串约定）。
 */
typedef struct OkCandidate {
  /**
   * 候选文本。
   */
  const char *text;
  /**
   * 候选备注（无则为 NULL）。
   */
  const char *comment;
} OkCandidate;

/**
 * 会话上下文（候选数组指针归会话所有）。
 */
typedef struct OkContext {
  /**
   * 预编辑文本；NULL 表示未组词。
   */
  const char *preedit;
  /**
   * 光标位置（字符数）。
   */
  int cursor_pos;
  /**
   * 选择区起点（字符数）。
   */
  int sel_start;
  /**
   * 选择区终点（字符数）。
   */
  int sel_end;
  /**
   * 当前页候选数组。
   */
  const struct OkCandidate *candidates;
  /**
   * 候选数量。
   */
  uintptr_t num_candidates;
  /**
   * 页码（从 0 开始）。
   */
  int page_no;
  /**
   * 每页候选数。
   */
  int page_size;
  /**
   * 是否最后一页。
   */
  OkBool is_last_page;
  /**
   * 高亮候选下标。
   */
  int highlighted_candidate_index;
} OkContext;

/**
 * 会话状态快照。
 */
typedef struct OkStatus {
  /**
   * 当前方案 ID。
   */
  const char *schema_id;
  /**
   * 当前方案名。
   */
  const char *schema_name;
  /**
   * 是否组词中。
   */
  OkBool is_composing;
  /**
   * 是否英文模式。
   */
  OkBool is_ascii_mode;
  /**
   * 是否全角。
   */
  OkBool is_full_shape;
  /**
   * 是否西文标点。
   */
  OkBool is_ascii_punct;
} OkStatus;

/**
 * 组件下载进度回调：`(用户上下文, 已下载字节, 总字节或0)`。
 */
typedef void (*OkComponentProgressCallback)(void *context, uint64_t downloaded, uint64_t total);

/**
 * 返回 OpenKey 版本字符串（静态，进程生命周期有效）。
 */
const char *ok_version_string(void);

/**
 * 创建引擎：初始化 librime（含部署）、打开数据层与组件管理器。
 * 失败返回 NULL，详情见 `ok_last_error(NULL)`。
 */
struct OkEngine *ok_engine_create(const struct OkEngineConfig *config);

/**
 * 销毁引擎。
 */
void ok_engine_destroy(struct OkEngine *engine);

/**
 * 触发重新部署（方案/词库变更后调用）。
 */
OkBool ok_engine_deploy(struct OkEngine *engine);

/**
 * 已部署方案数量。
 */
uintptr_t ok_engine_schema_count(struct OkEngine *engine);

/**
 * 按下标取方案 `(schema_id, name)`，写入调用方缓冲区。
 */
OkBool ok_engine_schema_at(struct OkEngine *engine,
                           uintptr_t index,
                           char *id_buf,
                           uintptr_t id_len,
                           char *name_buf,
                           uintptr_t name_len);

/**
 * 触发 userdb 同步快照任务（docs/02 3.3.4）。
 */
OkBool ok_engine_sync_user_data(struct OkEngine *engine);

/**
 * 导出 userdb 快照到目录（docs/02 3.1 `ok_rime_sync_export`）。
 */
OkBool ok_engine_sync_export(struct OkEngine *engine, const char *dest_dir);

/**
 * 从目录导入 userdb 快照（docs/02 3.1 `ok_rime_sync_import`）。
 */
OkBool ok_engine_sync_import(struct OkEngine *engine, const char *src_dir);

/**
 * 取最近一次错误详情（`engine` 为 NULL 时返回全局最近错误占位空串）。
 */
const char *ok_last_error(struct OkEngine *engine);

/**
 * 创建输入会话。
 */
struct OkSession *ok_session_create(struct OkEngine *engine);

/**
 * 销毁会话。
 */
void ok_session_destroy(struct OkSession *session);

/**
 * 处理按键。`keycode` 为 X11 keysym，`mask` 为 X11 修饰键
 * （Shift=1, Caps=2, Control=4, Alt/Mod1=8）。返回是否被 librime 消费。
 */
OkBool ok_session_process_key(struct OkSession *session, uint32_t keycode, uint32_t mask);

/**
 * 上屏当前预编辑。
 */
OkBool ok_session_commit_composition(struct OkSession *session);

/**
 * 清除组词。
 */
void ok_session_clear_composition(struct OkSession *session);

/**
 * 取当前上下文（预编辑 + 候选页）。输出指针直至下次对本会话调用前有效。
 */
OkBool ok_session_get_context(struct OkSession *session,
                              struct OkContext *out);

/**
 * 取待上屏文本，写入调用方缓冲区；无待上屏文本返回 false。
 */
OkBool ok_session_get_commit(struct OkSession *session, char *buf, uintptr_t len);

/**
 * 取状态快照。输出指针直至下次对本会话调用前有效。
 */
OkBool ok_session_get_status(struct OkSession *session, struct OkStatus *out);

/**
 * 选择候选（`index` 为**当前页内**下标，与 `ok_session_get_context` 返回的
 * 候选顺序一致；越界或失败返回 false）。
 */
OkBool ok_session_select_candidate(struct OkSession *session, uintptr_t index);

/**
 * 翻页。
 */
OkBool ok_session_change_page(struct OkSession *session, OkBool backward);

/**
 * 设置会话选项（如 `ascii_mode`）。
 */
OkBool ok_session_set_option(struct OkSession *session, const char *option, OkBool value);

/**
 * 读取会话选项。
 */
OkBool ok_session_get_option(struct OkSession *session, const char *option);

/**
 * 切换方案。
 */
OkBool ok_session_select_schema(struct OkSession *session, const char *schema_id);

/**
 * 当前方案 ID（归会话所有，下次调用前有效）。
 */
const char *ok_session_current_schema(struct OkSession *session);

/**
 * 加载 versions.json 清单文本。
 */
OkBool ok_component_set_manifest(struct OkEngine *engine, const char *manifest_json);

/**
 * 确保组件就绪（下载 → 校验 → 原子解压 → 登记）。
 * 返回 [`OK_COMPONENT_OK`] 或负状态码；进度经回调上报。
 */
int ok_component_ensure(struct OkEngine *engine,
                        const char *name,
                        OkComponentProgressCallback progress,
                        void *context);

/**
 * 组件是否已安装。
 */
OkBool ok_component_installed(struct OkEngine *engine, const char *name);

/**
 * 取组件当前版本，写入调用方缓冲区。
 */
OkBool ok_component_current_version(struct OkEngine *engine,
                                    const char *name,
                                    char *buf,
                                    uintptr_t len);

/**
 * 写入设置项（JSON 值文本）。
 */
OkBool ok_setting_set(struct OkEngine *engine, const char *key, const char *value_json);

/**
 * 读取设置项（JSON 文本）到调用方缓冲区。
 */
OkBool ok_setting_get(struct OkEngine *engine, const char *key, char *buf, uintptr_t len);

#endif  /* OPENKEY_H */
