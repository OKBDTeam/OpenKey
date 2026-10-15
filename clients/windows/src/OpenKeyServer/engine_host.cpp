/*
 * 文件用途：EngineHost 初始化与 pipe_protocol.h 全部服务端方法的实现。
 *
 * 并发模型：所有方法处理器持 EngineHost 互斥体串行执行。librime 会话彼此
 * 独立，但 core 的用户库写入与组件管理器存在全局状态，串行化是一期最稳
 * 的选择；后续如需并发键入吞吐，再按会话粒度细化（TODO）。
 */

#include "engine_host.h"

#include <mutex>
#include <stdexcept>

namespace openkey {
namespace {

std::string WideToUtf8(const std::wstring& s) {
  if (s.empty()) return {};
  int len = WideCharToMultiByte(CP_UTF8, 0, s.data(), static_cast<int>(s.size()),
                                nullptr, 0, nullptr, nullptr);
  if (len <= 0) return {};
  std::string out(static_cast<size_t>(len), '\0');
  WideCharToMultiByte(CP_UTF8, 0, s.data(), static_cast<int>(s.size()),
                      out.data(), len, nullptr, nullptr);
  return out;
}

std::wstring ExpandEnv(const wchar_t* name) {
  wchar_t buf[MAX_PATH] = {};
  DWORD n = GetEnvironmentVariableW(name, buf, MAX_PATH);
  if (n == 0 || n >= MAX_PATH) return L"";
  return buf;
}

std::wstring ExeDirectory() {
  wchar_t path[MAX_PATH] = {};
  GetModuleFileNameW(nullptr, path, MAX_PATH);
  std::wstring p(path);
  size_t slash = p.find_last_of(L"\\/");
  return slash == std::wstring::npos ? L"." : p.substr(0, slash);
}

EngineHost* AsHost(void* ctx) { return static_cast<EngineHost*>(ctx); }

/* ---------------- 方法处理器 ---------------- */

bool HandlePing(void* ctx, const pipe::FieldMap& req, pipe::FieldMap* resp) {
  (void)ctx;
  (void)req;
  (void)resp;
  return true;
}

bool HandleVersion(void* ctx, const pipe::FieldMap& req,
                   pipe::FieldMap* resp) {
  (void)ctx;
  (void)req;
  pipe::SetField(resp, "version", openkey::VersionString());
  return true;
}

bool HandleCreateSession(void* ctx, const pipe::FieldMap& req,
                         pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  if (!pipe::GetFieldInt(req, "key", &key) || key <= 0) return false;
  if (!host->CreateSession(key)) return false;
  pipe::SetFieldInt(resp, "session", key);
  return true;
}

bool HandleDestroySession(void* ctx, const pipe::FieldMap& req,
                          pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  if (!pipe::GetFieldInt(req, "session", &key)) return false;
  host->DestroySession(key);
  (void)resp;
  return true;
}

bool HandleProcessKey(void* ctx, const pipe::FieldMap& req,
                      pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0, keycode = 0, mask = 0;
  if (!pipe::GetFieldInt(req, "session", &key) ||
      !pipe::GetFieldInt(req, "keycode", &keycode) ||
      !pipe::GetFieldInt(req, "mask", &mask)) {
    return false;
  }
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  bool consumed = s->ProcessKey(static_cast<uint32_t>(keycode),
                                static_cast<uint32_t>(mask));
  pipe::SetFieldInt(resp, "consumed", consumed ? 1 : 0);
  // 附带 ascii 模式快照：TSF 侧 OnTestKeyDown 需要它决定是否吃键
  openkey::Status st;
  if (s->GetStatus(&st)) {
    pipe::SetFieldInt(resp, "ascii", st.is_ascii_mode ? 1 : 0);
  }
  return true;
}

bool HandleCommitComposition(void* ctx, const pipe::FieldMap& req,
                             pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  if (!pipe::GetFieldInt(req, "session", &key)) return false;
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  pipe::SetFieldInt(resp, "committed", s->CommitComposition() ? 1 : 0);
  return true;
}

bool HandleClearComposition(void* ctx, const pipe::FieldMap& req,
                            pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  if (!pipe::GetFieldInt(req, "session", &key)) return false;
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  s->ClearComposition();
  (void)resp;
  return true;
}

bool HandleGetContext(void* ctx, const pipe::FieldMap& req,
                      pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  if (!pipe::GetFieldInt(req, "session", &key)) return false;
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  openkey::Context c;
  if (!s->GetContext(&c)) return false;
  pipe::SetField(resp, "preedit", c.preedit);
  pipe::SetFieldInt(resp, "cursor", c.cursor_pos);
  pipe::SetFieldInt(resp, "sel_start", c.sel_start);
  pipe::SetFieldInt(resp, "sel_end", c.sel_end);
  pipe::SetFieldInt(resp, "page", c.page_no);
  pipe::SetFieldInt(resp, "page_size", c.page_size);
  pipe::SetFieldInt(resp, "last_page", c.is_last_page ? 1 : 0);
  pipe::SetFieldInt(resp, "highlighted", c.highlighted_candidate_index);
  pipe::SetFieldInt(resp, "cands", static_cast<long long>(c.candidates.size()));
  for (size_t i = 0; i < c.candidates.size(); ++i) {
    std::string key_name = "cand." + std::to_string(i);
    pipe::SetField(resp, key_name.c_str(), c.candidates[i].text);
    if (!c.candidates[i].comment.empty()) {
      pipe::SetField(resp, (key_name + ".comment").c_str(),
                     c.candidates[i].comment);
    }
  }
  return true;
}

bool HandleGetCommit(void* ctx, const pipe::FieldMap& req,
                     pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  if (!pipe::GetFieldInt(req, "session", &key)) return false;
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  std::string commit;
  if (s->GetCommit(&commit)) {
    // 有待上屏文本才带 text 字段；缺失即无（TSF 侧据此判断）
    pipe::SetField(resp, "text", commit);
  }
  return true;
}

bool HandleGetStatus(void* ctx, const pipe::FieldMap& req,
                     pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  if (!pipe::GetFieldInt(req, "session", &key)) return false;
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  openkey::Status st;
  if (!s->GetStatus(&st)) return false;
  pipe::SetField(resp, "schema_id", st.schema_id);
  pipe::SetField(resp, "schema_name", st.schema_name);
  pipe::SetFieldInt(resp, "composing", st.is_composing ? 1 : 0);
  pipe::SetFieldInt(resp, "ascii", st.is_ascii_mode ? 1 : 0);
  pipe::SetFieldInt(resp, "full_shape", st.is_full_shape ? 1 : 0);
  pipe::SetFieldInt(resp, "ascii_punct", st.is_ascii_punct ? 1 : 0);
  return true;
}

bool HandleSelectCandidate(void* ctx, const pipe::FieldMap& req,
                           pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0, index = 0;
  if (!pipe::GetFieldInt(req, "session", &key) ||
      !pipe::GetFieldInt(req, "index", &index)) {
    return false;
  }
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  bool consumed = s->SelectCandidate(static_cast<size_t>(index));
  pipe::SetFieldInt(resp, "consumed", consumed ? 1 : 0);
  return true;
}

bool HandleChangePage(void* ctx, const pipe::FieldMap& req,
                      pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0, backward = 0;
  if (!pipe::GetFieldInt(req, "session", &key) ||
      !pipe::GetFieldInt(req, "backward", &backward)) {
    return false;
  }
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  bool consumed = s->ChangePage(backward != 0);
  pipe::SetFieldInt(resp, "consumed", consumed ? 1 : 0);
  return true;
}

bool HandleSetOption(void* ctx, const pipe::FieldMap& req,
                     pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0, value = 0;
  std::string option;
  if (!pipe::GetFieldInt(req, "session", &key) ||
      !pipe::GetField(req, "option", &option) ||
      !pipe::GetFieldInt(req, "value", &value)) {
    return false;
  }
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  pipe::SetFieldInt(resp, "ok", s->SetOption(option, value != 0) ? 1 : 0);
  return true;
}

bool HandleGetOption(void* ctx, const pipe::FieldMap& req,
                     pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  std::string option;
  if (!pipe::GetFieldInt(req, "session", &key) ||
      !pipe::GetField(req, "option", &option)) {
    return false;
  }
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  pipe::SetFieldInt(resp, "value", s->GetOption(option) ? 1 : 0);
  return true;
}

bool HandleSelectSchema(void* ctx, const pipe::FieldMap& req,
                        pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  long long key = 0;
  std::string schema_id;
  if (!pipe::GetFieldInt(req, "session", &key) ||
      !pipe::GetField(req, "schema_id", &schema_id)) {
    return false;
  }
  std::lock_guard<std::mutex> lock(host->mutex());
  openkey::Session* s = host->FindSession(key);
  if (!s) return false;
  pipe::SetFieldInt(resp, "ok", s->SelectSchema(schema_id) ? 1 : 0);
  return true;
}

bool HandleGetSchemaList(void* ctx, const pipe::FieldMap& req,
                         pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  (void)req;
  std::lock_guard<std::mutex> lock(host->mutex());
  auto list = host->engine().SchemaList();
  pipe::SetFieldInt(resp, "count", static_cast<long long>(list.size()));
  for (size_t i = 0; i < list.size(); ++i) {
    pipe::SetField(resp, ("schema." + std::to_string(i) + ".id").c_str(),
                   list[i].id);
    pipe::SetField(resp, ("schema." + std::to_string(i) + ".name").c_str(),
                   list[i].name);
  }
  return true;
}

bool HandleSyncUserData(void* ctx, const pipe::FieldMap& req,
                        pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  (void)req;
  std::lock_guard<std::mutex> lock(host->mutex());
  pipe::SetFieldInt(resp, "ok", host->engine().SyncUserData() ? 1 : 0);
  return true;
}

bool HandleSyncExport(void* ctx, const pipe::FieldMap& req,
                      pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  std::string dir;
  if (!pipe::GetField(req, "dir", &dir)) return false;
  std::lock_guard<std::mutex> lock(host->mutex());
  pipe::SetFieldInt(resp, "ok", host->engine().SyncExport(dir) ? 1 : 0);
  return true;
}

bool HandleSyncImport(void* ctx, const pipe::FieldMap& req,
                      pipe::FieldMap* resp) {
  auto* host = AsHost(ctx);
  std::string dir;
  if (!pipe::GetField(req, "dir", &dir)) return false;
  std::lock_guard<std::mutex> lock(host->mutex());
  pipe::SetFieldInt(resp, "ok", host->engine().SyncImport(dir) ? 1 : 0);
  return true;
}

}  // namespace

/* ---------------- EngineHost ---------------- */

EngineHost& EngineHost::Instance() {
  static EngineHost host;
  return host;
}

bool EngineHost::Init() {
  // 目录约定（docs/02 4.1.4）：
  //   共享数据（方案/词库）：<exe 目录>\shared
  //   用户数据（userdb/build）：%APPDATA%\OpenKey
  //   组件：%LOCALAPPDATA%\OpenKey\components
  //   日志：%APPDATA%\OpenKey\log
  //   数据层：%APPDATA%\OpenKey\openkey.db
  std::wstring appdata = ExpandEnv(L"APPDATA");
  std::wstring localappdata = ExpandEnv(L"LOCALAPPDATA");
  std::wstring user_dir = appdata + L"\\OpenKey";
  if (appdata.empty()) user_dir = ExeDirectory() + L"\\user-data";

  openkey::EngineConfig cfg;
  cfg.shared_data_dir = WideToUtf8(ExeDirectory() + L"\\shared");
  cfg.user_data_dir = WideToUtf8(user_dir);
  cfg.data_db_path = WideToUtf8(user_dir + L"\\openkey.db");
  cfg.components_dir =
      WideToUtf8(localappdata.empty() ? user_dir + L"\\components"
                                      : localappdata + L"\\OpenKey\\components");
  cfg.log_dir = WideToUtf8(user_dir + L"\\log");
  cfg.min_log_level = 1;

  try {
    engine_ = openkey::Engine::Create(cfg);
  } catch (const std::exception& e) {
    init_error_ = e.what();
    return false;
  }
  return engine_.IsValid();
}

void EngineHost::Shutdown() {
  std::lock_guard<std::mutex> lock(mutex_);
  sessions_.clear();
  // engine_ 析构即 ok_engine_destroy
}

openkey::Session* EngineHost::FindSession(long long key) {
  auto it = sessions_.find(key);
  return it == sessions_.end() ? nullptr : &it->second;
}

bool EngineHost::CreateSession(long long key) {
  auto it = sessions_.find(key);
  if (it != sessions_.end()) return true;  // 已存在：幂等复用
  try {
    auto session = openkey::Session::Create(engine_);
    sessions_.emplace(key, std::move(session));
  } catch (const std::exception& e) {
    init_error_ = e.what();
    return false;
  }
  return true;
}

bool EngineHost::DestroySession(long long key) {
  auto it = sessions_.find(key);
  if (it == sessions_.end()) return false;
  sessions_.erase(it);
  return true;
}

namespace pipe {

/* 服务端方法分发表（pipe_protocol.h 声明的定义）。 */
const MethodEntry kMethodTable[] = {
    {kMethodPing, HandlePing},
    {kMethodVersion, HandleVersion},
    {kMethodCreateSession, HandleCreateSession},
    {kMethodDestroySession, HandleDestroySession},
    {kMethodProcessKey, HandleProcessKey},
    {kMethodCommitComposition, HandleCommitComposition},
    {kMethodClearComposition, HandleClearComposition},
    {kMethodGetContext, HandleGetContext},
    {kMethodGetCommit, HandleGetCommit},
    {kMethodGetStatus, HandleGetStatus},
    {kMethodSelectCandidate, HandleSelectCandidate},
    {kMethodChangePage, HandleChangePage},
    {kMethodSetOption, HandleSetOption},
    {kMethodGetOption, HandleGetOption},
    {kMethodSelectSchema, HandleSelectSchema},
    {kMethodGetSchemaList, HandleGetSchemaList},
    {kMethodSyncUserData, HandleSyncUserData},
    {kMethodSyncExport, HandleSyncExport},
    {kMethodSyncImport, HandleSyncImport},
};

const size_t kMethodTableSize =
    sizeof(kMethodTable) / sizeof(kMethodTable[0]);

}  // namespace pipe
}  // namespace openkey
