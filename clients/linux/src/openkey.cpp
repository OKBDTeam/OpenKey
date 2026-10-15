// OpenKey fcitx5 addon（docs/02 4.3）：进程内直跑 core，经 fcitx5 classic UI
// 呈现预编辑与候选（不自绘候选框，4.3.1）。
//
// 对接点：core/ffi-headers/openkey.h（C ABI）。每个 InputContext 一个
// OkSession；引擎为进程级单例（librime 全局状态由 fcitx5 单进程承载）。
//
// 按键分工：组词态下的翻页（Page_Up/Page_Down/-/=/,/.）与数字选择（1-9）、
// 空格选词、回车上屏、Esc 清空由本 addon 解析后调用 core 对应接口；其余按键
// 经 ok_session_process_key 交 librime 判定是否消费。
//
// fcitx5 API 要点（5.0.x / 5.1.x）：
//   - InputMethodEngineV2 的 deactivate/reset 第二参数是 InputContextEvent
//     （不是 InputContext*），需 event.inputContext() 取上下文；
//   - 候选列表用 DisplayOnlyCandidateList（setContent 传 vector<Text>），
//     本 addon 不实现可交互候选动作（选中由本 addon 自行解析按键）。

#include <fcitx/addonfactory.h>
#include <fcitx/addonmanager.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx-utils/log.h>
#include <fcitx-utils/standardpath.h>

#include <cstdlib>
#include <filesystem>
#include <map>
#include <string>
#include <vector>

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

/// 会话表：InputContext 指针 → OkSession（deactivate 时清理）。
using SessionMap = std::map<fcitx::InputContext *, OkSession *>;

}  // namespace

/// OpenKey 输入法引擎（fcitx5 InputMethodEngineV2）。
class OpenKeyEngine : public fcitx::InputMethodEngineV2 {
 public:
  explicit OpenKeyEngine(fcitx::Instance *instance);
  ~OpenKeyEngine() override;

  void keyEvent(const fcitx::InputMethodEntry &entry,
                fcitx::KeyEvent &keyEvent) override;
  void deactivate(const fcitx::InputMethodEntry &entry,
                  fcitx::InputContextEvent &event) override;
  void reset(const fcitx::InputMethodEntry &entry,
             fcitx::InputContextEvent &event) override;

 private:
  SessionMap sessions_;

  OkSession *SessionFor(fcitx::InputContext *ic);
  void DestroySession(fcitx::InputContext *ic);
  void ClearInputPanel(fcitx::InputContext *ic);
  void UpdateUI(fcitx::InputContext *ic, OkSession *session);
  void CommitPending(fcitx::InputContext *ic, OkSession *session);
};

OpenKeyEngine::OpenKeyEngine(fcitx::Instance *instance) {
  const std::string shared = OPENKEY_SHARED_DATA_DIR;
  const std::string user = UserDataDir();
  const std::string db = user + "/openkey.db";
  const std::string components = user + "/components";
  const std::string log = user + "/log";
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
  FCITX_UNUSED(instance);
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

void OpenKeyEngine::ClearInputPanel(fcitx::InputContext *ic) {
  ic->inputPanel().reset();
  ic->updatePreedit();
  ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
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
    std::vector<fcitx::Text> contents;
    contents.reserve(ctx.num_candidates);
    for (size_t i = 0; i < ctx.num_candidates; ++i) {
      const OkCandidate &c = ctx.candidates[i];
      fcitx::Text text;
      text.append(std::string(c.text ? c.text : ""));
      if (c.comment && c.comment[0]) {
        text.append(std::string(" ") + c.comment);
      }
      contents.push_back(std::move(text));
    }
    auto list = std::make_unique<fcitx::DisplayOnlyCandidateList>();
    list->setContent(std::move(contents));
    list->setLayoutHint(fcitx::CandidateLayoutHint::Vertical);
    list->setCursorIndex(static_cast<int>(ctx.highlighted_candidate_index));
    panel.setCandidateList(std::move(list));
  } else {
    panel.setCandidateList(nullptr);
  }
  ic->updatePreedit();
  ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

void OpenKeyEngine::CommitPending(fcitx::InputContext *ic, OkSession *session) {
  char buf[4096] = {0};
  if (ok_session_get_commit(session, buf, sizeof(buf)) && buf[0]) {
    ic->commitString(std::string(buf));
  }
}

void OpenKeyEngine::keyEvent(const fcitx::InputMethodEntry &entry,
                             fcitx::KeyEvent &keyEvent) {
  if (keyEvent.isRelease()) {
    return;
  }
  FCITX_UNUSED(entry);
  fcitx::InputContext *ic = keyEvent.inputContext();
  OkSession *session = SessionFor(ic);
  if (!session) {
    return;
  }
  const fcitx::Key &key = keyEvent.key();
  const uint32_t sym = key.sym();
  // KeyState 位定义与 X11 修饰掩码一致（Shift=1<<0, Ctrl=1<<2, Alt=1<<3）。
  const uint32_t mask = static_cast<uint32_t>(key.states());

  OkContext ctx = {};
  const bool composing =
      ok_session_get_context(session, &ctx) && ctx.preedit != nullptr;

  if (composing) {
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
      CommitPending(ic, session);
      UpdateUI(ic, session);
      return;
    }
    if (sym == kXK_space) {
      ok_session_select_candidate(session, 0);
      keyEvent.filterAndAccept();
      CommitPending(ic, session);
      UpdateUI(ic, session);
      return;
    }
    if (sym == kXK_Return) {
      ok_session_commit_composition(session);
      keyEvent.filterAndAccept();
      CommitPending(ic, session);
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
    CommitPending(ic, session);
    UpdateUI(ic, session);
  }
}

void OpenKeyEngine::reset(const fcitx::InputMethodEntry &entry,
                          fcitx::InputContextEvent &event) {
  FCITX_UNUSED(entry);
  fcitx::InputContext *ic = event.inputContext();
  if (OkSession *session = SessionFor(ic)) {
    ok_session_clear_composition(session);
  }
  ClearInputPanel(ic);
}

void OpenKeyEngine::deactivate(const fcitx::InputMethodEntry &entry,
                               fcitx::InputContextEvent &event) {
  reset(entry, event);
  DestroySession(event.inputContext());
}

class OpenKeyEngineFactory : public fcitx::AddonFactory {
 public:
  fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
    return new OpenKeyEngine(manager->instance());
  }
};

}  // namespace

FCITX_ADDON_FACTORY(OpenKeyEngineFactory);
