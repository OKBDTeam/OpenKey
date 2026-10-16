/*
 * 文件用途：core C ABI（core/ffi-headers/openkey.h）的 C++ RAII 薄封装。
 *
 * 设计约束（docs/02 2.1 职责边界铁律）：
 *   - 本封装只做句柄生命周期管理与类型转换，不增加任何策略逻辑；
 *   - 客户端代码只允许包含本头文件与 openkey.h，不得直接调用 C ABI；
 *   - OkEngine/OkSession 句柄在 openkey.h 中即不透明结构体指针，
 *     这里用 unique_ptr 自定义 Deleter 实现异常安全。
 */

#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

extern "C" {
#include "openkey.h"  // core/ffi-headers/openkey.h（唯一入口）
}

namespace openkey {

/* 引擎初始化配置（对应 OkEngineConfig，字符串为 std::string）。 */
struct EngineConfig {
  std::string shared_data_dir;  // librime 共享数据目录
  std::string user_data_dir;    // librime 用户数据目录
  std::string data_db_path;     // openkey.db（留空则不启用数据层）
  std::string components_dir;   // 组件根目录
  std::string log_dir;          // librime 日志目录
  int min_log_level = 1;        // glog 最小级别 0=INFO..3=FATAL
};

/* 候选（对应 OkCandidate）。 */
struct Candidate {
  std::string text;
  std::string comment;  // 可空
};

/* 会话上下文（对应 OkContext）。 */
struct Context {
  bool composing = false;  // preedit 非空
  std::string preedit;
  int cursor_pos = 0;
  int sel_start = 0;
  int sel_end = 0;
  std::vector<Candidate> candidates;
  int page_no = 0;
  int page_size = 0;
  bool is_last_page = true;
  int highlighted_candidate_index = 0;
};

/* 会话状态快照（对应 OkStatus）。 */
struct Status {
  std::string schema_id;
  std::string schema_name;
  bool is_composing = false;
  bool is_ascii_mode = false;
  bool is_full_shape = false;
  bool is_ascii_punct = false;
};

/* 方案列表项。 */
struct SchemaInfo {
  std::string id;
  std::string name;
};

/* OkEngine 的 RAII 封装。不可拷贝，可移动。 */
class Engine {
 public:
  Engine() = default;

  /* 创建引擎（初始化 librime / 数据层 / 组件管理器）。
   * 失败抛 std::runtime_error，消息取自 ok_last_error(NULL)。 */
  static Engine Create(const EngineConfig& config);

  ~Engine();
  Engine(Engine&& other) noexcept;
  Engine& operator=(Engine&& other) noexcept;
  Engine(const Engine&) = delete;
  Engine& operator=(const Engine&) = delete;

  bool IsValid() const { return engine_ != nullptr; }
  OkEngine* Get() const { return engine_.get(); }

  /* 触发重新部署。 */
  bool Deploy();

  /* 方案列表。 */
  uintptr_t SchemaCount() const;
  std::vector<SchemaInfo> SchemaList() const;

  /* userdb 同步快照（docs/02 3.3.4）。 */
  bool SyncUserData();
  bool SyncExport(const std::string& dest_dir);
  bool SyncImport(const std::string& src_dir);

  /* 最近一次错误（engine 为空的全局错误）。 */
  std::string LastError() const;

  /* 组件（一期占位：Server/Setup 未接入组件下载 UI，保留透传）。 */
  bool ComponentInstalled(const std::string& name) const;
  std::string ComponentCurrentVersion(const std::string& name) const;

  /* 设置项（JSON 文本）。 */
  bool SetSetting(const std::string& key, const std::string& value_json);
  bool GetSetting(const std::string& key, std::string* value_json);

 private:
  explicit Engine(OkEngine* engine) : engine_(engine) {}
  struct EngineDeleter {
    void operator()(OkEngine* e) const {
      if (e) ok_engine_destroy(e);
    }
  };
  std::unique_ptr<OkEngine, EngineDeleter> engine_;
};

/* OkSession 的 RAII 封装：引用创建它的 Engine（生命周期由调用方保证）。 */
class Session {
 public:
  Session() = default;

  /* 在 engine 上创建会话；失败抛 std::runtime_error。 */
  static Session Create(Engine& engine);

  ~Session();
  Session(Session&& other) noexcept;
  Session& operator=(Session&& other) noexcept;
  Session(const Session&) = delete;
  Session& operator=(const Session&) = delete;

  bool IsValid() const { return session_ != nullptr; }
  OkSession* Get() const { return session_.get(); }

  /* 处理按键：keycode 为 X11 keysym，mask 见 keysym_map.h。
   * 返回是否被 librime 消费。 */
  bool ProcessKey(uint32_t keycode, uint32_t mask);

  /* 上屏当前预编辑。 */
  bool CommitComposition();

  /* 清除组词。 */
  void ClearComposition();

  /* 取上下文；组词中返回 true。 */
  bool GetContext(Context* out);

  /* 取待上屏文本；无则返回 false。 */
  bool GetCommit(std::string* out);

  /* 取状态快照。 */
  bool GetStatus(Status* out);

  /* 选择候选 / 翻页。 */
  bool SelectCandidate(size_t index);
  bool ChangePage(bool backward);

  /* 会话选项（如 ascii_mode）。 */
  bool SetOption(const std::string& option, bool value);
  bool GetOption(const std::string& option);

  /* 方案切换。 */
  bool SelectSchema(const std::string& schema_id);
  std::string CurrentSchema() const;

 private:
  explicit Session(OkSession* session) : session_(session) {}
  struct SessionDeleter {
    void operator()(OkSession* s) const {
      if (s) ok_session_destroy(s);
    }
  };
  std::unique_ptr<OkSession, SessionDeleter> session_;
};

/* core 版本字符串（静态）。 */
std::string VersionString();

}  // namespace openkey
