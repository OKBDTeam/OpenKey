// OpenKey fcitx5 addon（docs/02 4.3）：进程内直跑 core，经 fcitx5 classic UI
// 呈现预编辑与候选（不自绘候选框，4.3.1）。
//
// 对接点：core/ffi-headers/openkey.h（C ABI）。每个 InputContext 一个
// OkSession；引擎为进程级单例（librime 全局状态由 fcitx5 单进程承载）。
//
// 按键分工：字母/标点经 ok_session_process_key 交 librime；组词状态下
// 翻页（Page_Up/Page_Down/-/=/,/.）与数字选择（1-9）由本 addon 解析后调
// ok_session_change_page / ok_session_select_candidate；空格选择高亮候选；
// 回车上屏预编辑；Esc 清空。

#include <fcitx/instance.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputcontext.h>
#include <fcitx/candidatelist.h>
#include <fcitx/addonmanager.h>
#include <fcitx/addonfactory.h>
#include <fcitx-utils/i18n.h>
#include <fcitx-utils/log.h>
#include <fcitx-utils/standardpath.h>

#include <cstdlib>
#include <filesystem>
#include <memory>
#include <string>
#include <unordered_map>

#include "openkey.h"

#ifndef OPENKEY_SHARED_DATA_DIR
#define OPENKEY_SHARED_DATA_DIR "/usr/share/openkey"
#endif

namespace {

// X11 keysym（fcitx::Key::sym() 即 X11 keysym；此处自带常量避免额外依赖）。
constexpr uint32_t kXK_space = 0x0020;
constexpr uint32_t kXK_1 = 0x0031;
constexpr uint32_t kXK_9 = 0x0039;
constexpr uint32_t kXK_minus = 0x002d;
constexpr uint32_t kXK_equal = 0x003d;
constexpr uint32_t kXK_comma = 0x002c;
constexpr uint32_t kXK_period = 0x002e;
constexpr uint32_t kXK_BackSpace = 0xff08;
constexpr uint32_t kXK_Return = 0xff0d;
constexpr uint32_t kXK_Escape = 0xff1b;
constexpr uint32_t kXK_Page_Up = 0xff55;
constexpr uint32_t kXK_Page_Down = 0xff56;

OkEngine *g_engine = nullptr;

/// 用户数据目录：${XDG_DATA_HOME:-~/.local/share}/openkey。
std::string UserDataDir() {
  std::string base;
  if (const char *xdg = std::getenv("XDG_DATA_HOME"); xdg && *xdg) {
    base = xdg;
  } else if (const char *home = std::getenv("HOME"); home && *home) {
    base = std::string(home) + "/.local/share";
  } else {
    base = "/tmp";
  }
  return base + "/openkey";
}

}  // namespace

/// OpenKey 输入法引擎（fcitx5 InputMethodEngineV2）。
class OpenKeyEngine : public fcitx::InputMethodEngineV2 {
 public:
  explicit OpenKeyEngine(fcitx::Instance *instance);
  ~OpenKeyEngine() override;

  void keyEvent(const fcitx::InputMethodEntry &entry,
                fcitx::KeyEvent &keyEvent) override;
  void reset(const fcitx::InputMethodEntry &entry,
             fcitx::InputContext *ic) override;
  void deactivate(const fcitx::InputMethodEntry &entry,
                  fcitx::InputContext *ic) override;

 private:
  std::unordered_map<fcitx::InputContext *, OkSession *> sessions_;

  OkSession *SessionFor(fcitx::InputContext *ic);
  void DestroySession(fcitx::InputContext *ic);
  bool IsComposing(OkSession *session);
  void UpdateUI(fcitx::InputContext *ic, OkSession *session);
  void CommitIfAny(fcitx::InputContext *ic, OkSession *session);
};

OpenKeyEngine::OpenKeyEngine(fcitx::Instance *instance) {
  std::string shared = OPENKEY_SHARED_DATA_DIR;
  std::string user = UserDataDir();
  std::string db = user + "/openkey.db";
  std::string components = user + "/components";
  std::string log = user + "/log";
  std::error_code ec;
  std::filesystem::create_directories(user, ec);
  std::filesystem::create_directories(components, ec);
  std::filesystem::create_directories(log, ec);

  OkEngineConfig config = {};
  config.shared_data_dir = shared.c_str();
  config.user_data_dir = user.c_str();
  config.data_db_path = db.c_str();
  config.components_dir = components.c_str();
  config.log_dir = log.c_str();
  config.min_log_level = 2;
  g_engine = ok_engine_create(&config);
  if (!g_engine) {
    FCITX_ERROR() << "ok_engine_create 失败：" << ok_last_error(nullptr);
  } else {
    FCITX_INFO() << "OpenKey core 初始化完成（shared=" << shared
                 << ", user=" << user << "）";
  }
}

OpenKeyEngine::~OpenKeyEngine() {
  for (auto &pair : sessions_) {
    ok_session_destroy(pair.second);
  }
  sessions_.clear();
  if (g_engine) {
    ok_engine_destroy(g_engine);
    g_engine = nullptr;
  }
}

OkSession *OpenKeyEngine::SessionFor(fcitx::InputContext *ic) {
  auto it = sessions_.find(ic);
  if (it != sessions_.end()) {
    return it->second;
  }
  OkSession *session = ok_session_create(g_engine);
  if (session) {
    sessions_[ic] = session;
  }
  return session;
}

void OpenKeyEngine::DestroySession(fcitx::InputContext *ic) {
  auto it = sessions_.find(ic);
  if (it != sessions_.end()) {
    ok_session_destroy(it->second);
    sessions_.erase(it);
  }
}

bool OpenKeyEngine::IsComposing(OkSession *session) {
  OkStatus status = {};
  if (ok_session_get_status(session, &status) && status.is_composing) {
    return true;
  }
  OkContext ctx = {};
  return ok_session_get_context(session, &ctx) && ctx.preedit != nullptr;
}

void OpenKeyEngine::UpdateUI(fcitx::InputContext *ic, OkSession *session) {
  OkContext ctx = {};
  if (!ok_session_get_context(session, &ctx)) {
    return;
  }
  fcitx::InputPanel &panel = ic->inputPanel();
  if (ctx.preedit) {
    fcitx::Text preedit;
    preedit.append(std::string(ctx.preedit));
    preedit.setCursor(ctx.cursor_pos);
    panel.setPreedit(preedit);
  } else {
    panel.reset();
  }

  if (ctx.num_candidates > 0) {
    std::vector<std::unique_ptr<fcitx::Candidate>> candidates;
    candidates.reserve(ctx.num_candidates);
    for (size_t i = 0; i < ctx.num_candidates; ++i) {
      const OkCandidate &c = ctx.candidates[i];
      fcitx::Text text;
      text.append(std::string(c.text ? c.text : ""));
      if (c.comment && c.comment[0]) {
        text.append(std::string(" ") + c.comment);
      }
      candidates.push_back(std::make_unique<fcitx::TextCandidate>(text));
    }
    auto list = std::make_unique<fcitx::CandidateList>(std::move(candidates));
    list->setLayoutHint(fcitx::CandidateLayoutHint::Vertical);
    panel.setCandidateList(std::move(list));
  } else {
    panel.setCandidateList(nullptr);
  }
  ic->updatePreedit();
  ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

void OpenKeyEngine::CommitIfAny(fcitx::InputContext *ic, OkSession *session) {
  char buf[4096] = {0};
  if (ok_session_get_commit(session, buf, sizeof(buf)) && buf[0]) {
    ic->commitString(buf);
  }
}

void OpenKeyEngine::keyEvent(const fcitx::InputMethodEntry &entry,
                             fcitx::KeyEvent &keyEvent) {
  if (keyEvent.isRelease()) {
    return;
  }
  auto *ic = keyEvent.inputContext();
  OkSession *session = SessionFor(ic);
  if (!session) {
    return;
  }
  const fcitx::Key &key = keyEvent.key();
  const uint32_t sym = key.sym();
  const uint32_t mask = static_cast<uint32_t>(key.states());

  if (IsComposing(session)) {
    // 组词态：本 addon 解析翻页/选择/上屏/清空，其余交 librime。
    if (sym == kXK_Page_Up || sym == kXK_minus || sym == kXK_comma) {
      ok_session_change_page(session, true);
      keyEvent.filterAndAccept();
      UpdateUI(ic, session);
      return;
    }
    if (sym == kXK_Page_Down || sym == kXK_equal || sym == kXK_period) {
      ok_session_change_page(session, false);
      keyEvent.filterAndAccept();
      UpdateUI(ic, session);
      return;
    }
    if (sym >= kXK_1 && sym <= kXK_9) {
      ok_session_select_candidate(session, sym - kXK_1);
      keyEvent.filterAndAccept();
      CommitIfAny(ic, session);
      UpdateUI(ic, session);
      return;
    }
    if (sym == kXK_space) {
      ok_session_select_candidate(session, 0);
      keyEvent.filterAndAccept();
      CommitIfAny(ic, session);
      UpdateUI(ic, session);
      return;
    }
    if (sym == kXK_Return) {
      ok_session_commit_composition(session);
      keyEvent.filterAndAccept();
      CommitIfAny(ic, session);
      UpdateUI(ic, session);
      return;
    }
    if (sym == kXK_Escape) {
      ok_session_clear_composition(session);
      keyEvent.filterAndAccept();
      UpdateUI(ic, session);
      return;
    }
  }

  if (ok_session_process_key(session, sym, mask)) {
    keyEvent.filterAndAccept();
    CommitIfAny(ic, session);
    UpdateUI(ic, session);
  }
}

void OpenKeyEngine::reset(const fcitx::InputMethodEntry &entry,
                          fcitx::InputContext *ic) {
  OkSession *session = SessionFor(ic);
  if (session) {
    ok_session_clear_composition(session);
  }
  ic->inputPanel().reset();
  ic->updatePreedit();
  ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

void OpenKeyEngine::deactivate(const fcitx::InputMethodEntry &entry,
                               fcitx::InputContext *ic) {
  reset(entry, ic);
  DestroySession(ic);
}

class OpenKeyEngineFactory : public fcitx::AddonFactory {
 public:
  fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
    return new OpenKeyEngine(manager->instance());
  }
};

}  // namespace

FCITX_ADDON_FACTORY(OpenKeyEngineFactory);
