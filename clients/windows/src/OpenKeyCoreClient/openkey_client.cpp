/*
 * 文件用途：openkey_client.h 的实现。所有调用直通 openkey.h 的 C ABI，
 * 仅做 std::string <-> char* 缓冲区的转换与错误消息提取。
 */

#include "openkey_client.h"

#include <array>
#include <stdexcept>

namespace openkey {
namespace {

/* *_buf 形式输出的通用读取：先试 4KB，不足则翻倍，最多 1MB。 */
template <typename Fn>
std::string ReadBuf(Fn&& write_fn, const char* what) {
  size_t cap = 4096;
  while (cap <= 1024 * 1024) {
    std::vector<char> buf(cap, '\0');
    if (write_fn(buf.data(), buf.size())) {
      return std::string(buf.data());
    }
    cap *= 2;
  }
  throw std::runtime_error(std::string(what) + ": buffer overflow");
}

std::string ErrorMessage(OkEngine* engine, const char* what) {
  std::string msg = what;
  const char* detail = ok_last_error(engine);
  if (detail && *detail) {
    msg += ": ";
    msg += detail;
  }
  return msg;
}

}  // namespace

std::string VersionString() { return ok_version_string(); }

Engine Engine::Create(const EngineConfig& config) {
  OkEngineConfig c{};
  c.shared_data_dir = config.shared_data_dir.empty()
                          ? nullptr
                          : config.shared_data_dir.c_str();
  c.user_data_dir =
      config.user_data_dir.empty() ? nullptr : config.user_data_dir.c_str();
  c.data_db_path =
      config.data_db_path.empty() ? nullptr : config.data_db_path.c_str();
  c.components_dir = config.components_dir.empty()
                         ? nullptr
                         : config.components_dir.c_str();
  c.log_dir = config.log_dir.empty() ? nullptr : config.log_dir.c_str();
  c.min_log_level = config.min_log_level;

  OkEngine* engine = ok_engine_create(&c);
  if (!engine) {
    throw std::runtime_error(ErrorMessage(nullptr, "ok_engine_create failed"));
  }
  return Engine(engine);
}

Engine::~Engine() = default;

Engine::Engine(Engine&& other) noexcept = default;
Engine& Engine::operator=(Engine&& other) noexcept = default;

bool Engine::Deploy() {
  if (!engine_) return false;
  return ok_engine_deploy(engine_) != 0;
}

uintptr_t Engine::SchemaCount() const {
  if (!engine_) return 0;
  return ok_engine_schema_count(engine_);
}

std::vector<SchemaInfo> Engine::SchemaList() const {
  std::vector<SchemaInfo> out;
  if (!engine_) return out;
  uintptr_t n = ok_engine_schema_count(engine_);
  out.reserve(n);
  for (uintptr_t i = 0; i < n; ++i) {
    SchemaInfo info;
    std::string id;
    std::array<char, 256> name_buf{};  // 方案名上限 255 字符
    // ReadBuf 逐级扩容 id 缓冲区；每次成功都把 id 捕获到局部变量。
    if (!ReadBuf(
            [&](char* id_buf, size_t id_len) {
              if (!ok_engine_schema_at(engine_, i, id_buf, id_len,
                                       name_buf.data(), name_buf.size())) {
                return false;
              }
              id = id_buf;
              return true;
            },
            "ok_engine_schema_at")) {
      break;  // 越界或失败：截断
    }
    info.id = id;
    info.name = name_buf.data();
    out.push_back(std::move(info));
  }
  return out;
}

bool Engine::SyncUserData() {
  if (!engine_) return false;
  return ok_engine_sync_user_data(engine_) != 0;
}

bool Engine::SyncExport(const std::string& dest_dir) {
  if (!engine_) return false;
  return ok_engine_sync_export(engine_, dest_dir.c_str()) != 0;
}

bool Engine::SyncImport(const std::string& src_dir) {
  if (!engine_) return false;
  return ok_engine_sync_import(engine_, src_dir.c_str()) != 0;
}

std::string Engine::LastError() const {
  const char* s = ok_last_error(engine_);
  return s ? std::string(s) : std::string();
}

bool Engine::ComponentInstalled(const std::string& name) const {
  if (!engine_) return false;
  return ok_component_installed(engine_, name.c_str()) != 0;
}

std::string Engine::ComponentCurrentVersion(const std::string& name) const {
  if (!engine_) return {};
  return ReadBuf(
      [&](char* buf, size_t len) {
        return ok_component_current_version(engine_, name.c_str(), buf, len) != 0;
      },
      "ok_component_current_version");
}

bool Engine::SetSetting(const std::string& key, const std::string& value_json) {
  if (!engine_) return false;
  return ok_setting_set(engine_, key.c_str(), value_json.c_str()) != 0;
}

bool Engine::GetSetting(const std::string& key, std::string* value_json) {
  if (!engine_) return false;
  *value_json = ReadBuf(
      [&](char* buf, size_t len) {
        return ok_setting_get(engine_, key.c_str(), buf, len) != 0;
      },
      "ok_setting_get");
  return true;
}

Session Session::Create(Engine& engine) {
  if (!engine.IsValid()) {
    throw std::runtime_error("Session::Create: engine invalid");
  }
  OkSession* s = ok_session_create(engine.Get());
  if (!s) {
    throw std::runtime_error(ErrorMessage(engine.Get(), "ok_session_create failed"));
  }
  return Session(s);
}

Session::~Session() = default;

Session::Session(Session&& other) noexcept = default;
Session& Session::operator=(Session&& other) noexcept = default;

bool Session::ProcessKey(uint32_t keycode, uint32_t mask) {
  if (!session_) return false;
  return ok_session_process_key(session_, keycode, mask) != 0;
}

bool Session::CommitComposition() {
  if (!session_) return false;
  return ok_session_commit_composition(session_) != 0;
}

void Session::ClearComposition() {
  if (session_) ok_session_clear_composition(session_);
}

bool Session::GetContext(Context* out) {
  if (!session_) return false;
  OkContext ctx{};
  if (!ok_session_get_context(session_, &ctx)) return false;
  out->preedit = ctx.preedit ? ctx.preedit : "";
  out->composing = !out->preedit.empty();
  out->cursor_pos = ctx.cursor_pos;
  out->sel_start = ctx.sel_start;
  out->sel_end = ctx.sel_end;
  out->candidates.clear();
  if (ctx.candidates && ctx.num_candidates > 0) {
    out->candidates.reserve(ctx.num_candidates);
    for (uintptr_t i = 0; i < ctx.num_candidates; ++i) {
      Candidate c;
      c.text = ctx.candidates[i].text ? ctx.candidates[i].text : "";
      c.comment =
          ctx.candidates[i].comment ? ctx.candidates[i].comment : "";
      out->candidates.push_back(std::move(c));
    }
  }
  out->page_no = ctx.page_no;
  out->page_size = ctx.page_size;
  out->is_last_page = ctx.is_last_page != 0;
  out->highlighted_candidate_index = ctx.highlighted_candidate_index;
  return true;
}

bool Session::GetCommit(std::string* out) {
  if (!session_) return false;
  std::vector<char> buf(4096, '\0');
  if (!ok_session_get_commit(session_, buf.data(), buf.size())) {
    return false;  // 无待上屏文本或缓冲区过小（4KB 对单次提交足够）
  }
  *out = buf.data();
  return true;
}

bool Session::GetStatus(Status* out) {
  if (!session_) return false;
  OkStatus st{};
  if (!ok_session_get_status(session_, &st)) return false;
  out->schema_id = st.schema_id ? st.schema_id : "";
  out->schema_name = st.schema_name ? st.schema_name : "";
  out->is_composing = st.is_composing != 0;
  out->is_ascii_mode = st.is_ascii_mode != 0;
  out->is_full_shape = st.is_full_shape != 0;
  out->is_ascii_punct = st.is_ascii_punct != 0;
  return true;
}

bool Session::SelectCandidate(size_t index) {
  if (!session_) return false;
  return ok_session_select_candidate(session_, index) != 0;
}

bool Session::ChangePage(bool backward) {
  if (!session_) return false;
  return ok_session_change_page(session_, backward ? 1 : 0) != 0;
}

bool Session::SetOption(const std::string& option, bool value) {
  if (!session_) return false;
  return ok_session_set_option(session_, option.c_str(), value ? 1 : 0) != 0;
}

bool Session::GetOption(const std::string& option) {
  if (!session_) return false;
  return ok_session_get_option(session_, option.c_str()) != 0;
}

bool Session::SelectSchema(const std::string& schema_id) {
  if (!session_) return false;
  return ok_session_select_schema(session_, schema_id.c_str()) != 0;
}

std::string Session::CurrentSchema() const {
  if (!session_) return {};
  const char* s = ok_session_current_schema(session_);
  return s ? std::string(s) : std::string();
}

}  // namespace openkey
