/*
 * COpenKey 目标的实现：openkey_shim（配置结构体填充 + 释放辅助）。
 * 顺带在编译期校验 openkey.h 自洽性。
 */
#include "include/shim.h"

#include <stdlib.h>

void openkey_shim_fill_config(OkEngineConfig *config,
                              const char *shared_data_dir,
                              const char *user_data_dir,
                              const char *data_db_path,
                              const char *components_dir,
                              const char *log_dir,
                              int min_log_level) {
  if (!config) {
    return;
  }
  config->shared_data_dir = shared_data_dir;
  config->user_data_dir = user_data_dir;
  config->data_db_path = data_db_path;
  config->components_dir = components_dir;
  config->log_dir = log_dir;
  config->min_log_level = min_log_level;
}

void openkey_shim_free(void *ptr) { free(ptr); }
