/*
 * openkey_shim —— Swift 侧调用 core C ABI 的薄封装（macOS / iOS 共用）。
 *
 * 存在的理由：ClangImporter 对 C 结构体中 `const char *` 字段的 Swift 类型
 * 推断随工具链版本变化（可能是 UnsafePointer 或 UnsafeMutablePointer），
 * 直接在 Swift 里逐字段赋值会出现类型不匹配。此处用 C 函数集中完成
 * 「字符串指针 → OkEngineConfig」的转换，Swift 只需传原始指针。
 */
#ifndef OPENKEY_SHIM_H
#define OPENKEY_SHIM_H

#include "openkey.h"

#ifdef __cplusplus
extern "C" {
#endif

/* 用给定路径填充 OkEngineConfig（任一路径可为 NULL）。 */
void openkey_shim_fill_config(OkEngineConfig *config,
                              const char *shared_data_dir,
                              const char *user_data_dir,
                              const char *data_db_path,
                              const char *components_dir,
                              const char *log_dir,
                              int min_log_level);

/* 复制并释放：Swift 侧 strdup 的所有权交回 C，便于统一释放。 */
void openkey_shim_free(void *ptr);

#ifdef __cplusplus
}
#endif

#endif /* OPENKEY_SHIM_H */
