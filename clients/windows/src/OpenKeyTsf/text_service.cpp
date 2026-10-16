/*
 * 文件用途：OpenKeyTextService 与 OpenKeyClassFactory 的实现。
 *
 * 关键流程：
 *   Activate -> AdviseSinks（KeyEvent/ThreadMgr/Composition）
 *   OnTestKeyDown: 决定是否吃掉按键（组词中全吃；非组词时中文模式吃可打印键）
 *   OnKeyDown: keysym+mask -> 管道 ProcessKey
 *     消费 -> 同步编辑会话：SetText 预编辑 / 上屏 commit / 刷新候选窗
 *     未消费 -> 同步编辑会话：插入原字符（此前已被测试阶段吃掉，必须兜底）
 *   Deactivate -> 反注册、销毁会话、清理组词
 */

#include "text_service.h"

#include <string>

#include "openkey_client.h"  // openkey::Context / openkey::Candidate
#include "pipe_protocol.h"

/* 协议栈与 core 封装都定义在 openkey / openkey::pipe 命名空间内；本文件是
 * 全局命名空间下的 COM 实现，用 using 简化调用，避免逐处加限定。 */
using namespace openkey;
using namespace openkey::pipe;

/* 注意：Server 协议只收 UTF-8；本文件的预编辑/上屏文本来自 Server 响应，
 * 已由 pipe_protocol 解码为 UTF-8 std::string，转宽字符用 Utf8ToWide。 */

/* ------------------------------------------------------------------ */
/* OpenKeyEditSession                                                   */
/* ------------------------------------------------------------------ */

OpenKeyEditSession::OpenKeyEditSession(std::function<void(TfEditCookie)> fn)
    : fn_(std::move(fn)) {}

OpenKeyEditSession::~OpenKeyEditSession() = default;

STDMETHODIMP OpenKeyEditSession::QueryInterface(REFIID riid, void** ppv) {
  if (!ppv) return E_INVALIDARG;
  if (riid == IID_IUnknown || riid == IID_ITfEditSession) {
    *ppv = static_cast<ITfEditSession*>(this);
  } else {
    *ppv = nullptr;
    return E_NOINTERFACE;
  }
  AddRef();
  return S_OK;
}

STDMETHODIMP_(ULONG) OpenKeyEditSession::AddRef() { return ++ref_; }

STDMETHODIMP_(ULONG) OpenKeyEditSession::Release() {
  ULONG n = --ref_;
  if (n == 0) delete this;
  return n;
}

STDMETHODIMP OpenKeyEditSession::DoEditSession(TfEditCookie ec) {
  if (fn_) fn_(ec);
  return S_OK;
}

/* ------------------------------------------------------------------ */
/* OpenKeyTextService                                                   */
/* ------------------------------------------------------------------ */

OpenKeyTextService::OpenKeyTextService() { OpenKeyDllAddRef(); }

OpenKeyTextService::~OpenKeyTextService() {
  Deactivate();
  candidates_.Destroy();
  OpenKeyDllRelease();
}

STDMETHODIMP OpenKeyTextService::QueryInterface(REFIID riid, void** ppv) {
  if (!ppv) return E_INVALIDARG;
  if (riid == IID_IUnknown || riid == IID_ITfTextInputProcessor) {
    *ppv = static_cast<ITfTextInputProcessor*>(this);
  } else if (riid == IID_ITfTextInputProcessorEx) {
    *ppv = static_cast<ITfTextInputProcessorEx*>(this);
  } else if (riid == IID_ITfKeyEventSink) {
    *ppv = static_cast<ITfKeyEventSink*>(this);
  } else if (riid == IID_ITfThreadMgrEventSink) {
    *ppv = static_cast<ITfThreadMgrEventSink*>(this);
  } else if (riid == IID_ITfCompositionSink) {
    *ppv = static_cast<ITfCompositionSink*>(this);
  } else {
    *ppv = nullptr;
    return E_NOINTERFACE;
  }
  AddRef();
  return S_OK;
}

STDMETHODIMP_(ULONG) OpenKeyTextService::AddRef() { return ++ref_; }

STDMETHODIMP_(ULONG) OpenKeyTextService::Release() {
  ULONG n = --ref_;
  if (n == 0) delete this;
  return n;
}

STDMETHODIMP OpenKeyTextService::Activate(ITfThreadMgr* ptim,
                                          TfClientId tid) {
  if (!ptim) return E_INVALIDARG;
  ptim_ = ptim;
  ptim_->AddRef();
  tid_ = tid;
  candidates_.SetSelectCallback(
      [this](size_t index) { this->OnCandidateSelected(index); });
  // 当前焦点上下文（可能尚无焦点，允许为空）
  ITfContext* focus = nullptr;
  if (SUCCEEDED(ptim_->GetFocus(&focus)) && focus) {
    AdviseSinks(focus);
  } else {
    AdviseSinks(nullptr);
  }
  if (focus) focus->Release();
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::ActivateEx(ITfThreadMgr* ptim, TfClientId tid,
                                            DWORD flags) {
  // dwFlags 指示激活场景（如安全模式/管理员窗口）；一期不区分，同 Activate。
  (void)flags;
  return Activate(ptim, tid);
}

STDMETHODIMP OpenKeyTextService::Deactivate() {
  UnadviseSinks();
  // 组词随上下文失效，不做 ec 收尾（Deactivate 时通常已无 ec）
  if (composition_range_) {
    composition_range_->Release();
    composition_range_ = nullptr;
  }
  if (composition_) {
    composition_->Release();
    composition_ = nullptr;
  }
  composing_ = false;
  candidates_.Hide();
  DestroySession();
  if (context_) {
    context_->Release();
    context_ = nullptr;
  }
  if (keystroke_mgr_) {
    keystroke_mgr_->Release();
    keystroke_mgr_ = nullptr;
  }
  if (ptim_) {
    ptim_->Release();
    ptim_ = nullptr;
  }
  tid_ = 0;
  return S_OK;
}

bool OpenKeyTextService::AdviseSinks(ITfContext* context_for_composition) {
  if (!ptim_) return false;

  // 按键接收器：优先 ITfKeystrokeMgr::AdviseKeyEventSink（微软 SampleIME 的
  // 注册路径）；该路径不可用时退回 ITfSource::AdviseSink（部分宿主只暴露
  // 后者）。两条路径互斥，cookie / 句柄分别记录以便对称注销。
  ITfKeystrokeMgr* keystroke = nullptr;
  if (SUCCEEDED(ptim_->QueryInterface(IID_ITfKeystrokeMgr,
                                      reinterpret_cast<void**>(&keystroke))) &&
      keystroke) {
    if (SUCCEEDED(keystroke->AdviseKeyEventSink(
            tid_, static_cast<ITfKeyEventSink*>(this), TRUE))) {
      keystroke_mgr_ = keystroke;  // 引用移交成员，Deactivate 时释放
    } else {
      keystroke->Release();
    }
  }

  ITfSource* source = nullptr;
  if (SUCCEEDED(ptim_->QueryInterface(IID_ITfSource,
                                      reinterpret_cast<void**>(&source))) &&
      source) {
    if (!keystroke_mgr_) {
      source->AdviseSink(IID_ITfKeyEventSink,
                         static_cast<ITfKeyEventSink*>(this),
                         &key_event_cookie_);
    }
    source->AdviseSink(IID_ITfThreadMgrEventSink,
                       static_cast<ITfThreadMgrEventSink*>(this),
                       &thread_mgr_cookie_);
    source->Release();
  }

  // 组词事件接收器：TSF 惯例是挂到上下文对象的 ITfSource。
  // 若该 QI 失败（个别宿主封装差异），退化为挂到线程管理器；两者都失败
  // 时仅失去 OnCompositionTerminated 回调，不影响主流程（组词状态本地跟踪）。
  if (context_for_composition) {
    ITfSource* ctx_source = nullptr;
    if (SUCCEEDED(context_for_composition->QueryInterface(
            IID_ITfSource, reinterpret_cast<void**>(&ctx_source))) &&
        ctx_source) {
      ctx_source->AdviseSink(IID_ITfCompositionSink,
                             static_cast<ITfCompositionSink*>(this),
                             &composition_cookie_);
      ctx_source->Release();
    }
  }
  return true;
}

void OpenKeyTextService::UnadviseSinks() {
  if (keystroke_mgr_) {
    keystroke_mgr_->UnadviseKeyEventSink(tid_);
    keystroke_mgr_->Release();
    keystroke_mgr_ = nullptr;
  }
  if (ptim_) {
    ITfSource* source = nullptr;
    if (SUCCEEDED(ptim_->QueryInterface(IID_ITfSource,
                                        reinterpret_cast<void**>(&source))) &&
        source) {
      if (key_event_cookie_ != TF_INVALID_COOKIE) {
        source->UnadviseSink(key_event_cookie_);
        key_event_cookie_ = TF_INVALID_COOKIE;
      }
      if (thread_mgr_cookie_ != TF_INVALID_COOKIE) {
        source->UnadviseSink(thread_mgr_cookie_);
        thread_mgr_cookie_ = TF_INVALID_COOKIE;
      }
      source->Release();
    }
  }
  if (context_) {
    ITfSource* ctx_source = nullptr;
    if (SUCCEEDED(context_->QueryInterface(
            IID_ITfSource, reinterpret_cast<void**>(&ctx_source))) &&
        ctx_source) {
      if (composition_cookie_ != TF_INVALID_COOKIE) {
        ctx_source->UnadviseSink(composition_cookie_);
        composition_cookie_ = TF_INVALID_COOKIE;
      }
      ctx_source->Release();
    }
  }
}

void OpenKeyTextService::TrackContext(ITfDocumentMgr* pdim) {
  // 切换焦点文档：释放旧上下文，取新上下文并挂组词事件接收器。
  if (context_) {
    context_->Release();
    context_ = nullptr;
  }
  if (!pdim) return;
  if (FAILED(pdim->GetTop(&context_)) || !context_) return;
  // 焦点切换后旧组词失效（ec 已不可用），仅清本地状态。
  if (composition_range_) {
    composition_range_->Release();
    composition_range_ = nullptr;
  }
  if (composition_) {
    composition_->Release();
    composition_ = nullptr;
  }
  composing_ = false;
  candidates_.Hide();
  ITfSource* ctx_source = nullptr;
  if (SUCCEEDED(context_->QueryInterface(
          IID_ITfSource, reinterpret_cast<void**>(&ctx_source))) &&
      ctx_source) {
    ctx_source->AdviseSink(IID_ITfCompositionSink,
                           static_cast<ITfCompositionSink*>(this),
                           &composition_cookie_);
    ctx_source->Release();
  }
}

bool OpenKeyTextService::EnsureSession() {
  if (session_ready_ && session_key_ != 0) return true;
  try {
    pipe::FieldMap req;
    pipe::SetFieldInt(&req, "key",
                      static_cast<long long>(OpenKeySessionKey()));
    auto resp = pipe::Request(pipe::kMethodCreateSession, req);
    std::string err;
    if (pipe::GetField(resp, "__error__", &err)) return false;
    long long key = 0;
    if (!pipe::GetFieldInt(resp, "session", &key) || key == 0) return false;
    session_key_ = key;
    session_ready_ = true;
    RefreshStatus();
    return true;
  } catch (const PipeError&) {
    session_ready_ = false;
    return false;
  }
}

bool OpenKeyTextService::DestroySession() {
  if (!session_ready_ || session_key_ == 0) return true;
  try {
    pipe::FieldMap req;
    pipe::SetFieldInt(&req, "session", session_key_);
    pipe::Request(pipe::kMethodDestroySession, req);
  } catch (const PipeError&) {
    // 服务端已退出：无需处理
  }
  session_ready_ = false;
  session_key_ = 0;
  return true;
}

void OpenKeyTextService::RefreshStatus() {
  if (!session_ready_) return;
  try {
    pipe::FieldMap req;
    pipe::SetFieldInt(&req, "session", session_key_);
    auto resp = pipe::Request(pipe::kMethodGetStatus, req);
    std::string err;
    if (pipe::GetField(resp, "__error__", &err)) return;
    long long ascii = 0;
    if (pipe::GetFieldInt(resp, "ascii", &ascii)) ascii_mode_ = ascii != 0;
  } catch (const PipeError&) {
  }
}

bool OpenKeyTextService::RequestEditSession(
    std::function<void(TfEditCookie)> fn) {
  if (!ptim_) return false;
  ITfContext* ctx = context_;
  if (!ctx) {
    // 焦点文档 -> 栈顶上下文（GetFocus 返回 ITfDocumentMgr）
    ITfDocumentMgr* dim = nullptr;
    if (FAILED(ptim_->GetFocus(&dim)) || !dim) {
      if (dim) dim->Release();
      return false;
    }
    if (FAILED(dim->GetTop(&ctx)) || !ctx) {
      dim->Release();
      return false;
    }
    dim->Release();  // ctx 的引用由 GetTop 转交调用方
  } else {
    ctx->AddRef();
  }
  auto* es = new OpenKeyEditSession(std::move(fn));
  HRESULT hr_session = S_OK;
  // 同步编辑会话（TF_ES_READWRITE）：键处理延迟确定性高。
  // TODO: 若遇模态宿主死锁，改 TF_ES_ASYNC + 状态机。
  HRESULT hr = ctx->RequestEditSession(tid_, es, TF_ES_READWRITE,
                                       &hr_session);
  es->Release();
  ctx->Release();
  return SUCCEEDED(hr) && SUCCEEDED(hr_session);
}

STDMETHODIMP OpenKeyTextService::OnSetFocus(BOOL fForeground) {
  // 按键接收器侧的前后台焦点通知；Server 会话按线程 ID 标识，无需处理。
  (void)fForeground;
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnTestKeyDown(ITfContext* pic, WPARAM wParam,
                                               LPARAM lParam, BOOL* pfEaten) {
  (void)pic;
  (void)lParam;
  if (!pfEaten) return E_INVALIDARG;
  *pfEaten = FALSE;
  UINT vk = static_cast<UINT>(wParam);
  if (vk == VK_PROCESSKEY) return S_OK;
  uint32_t mask = CurrentKeyMask();
  // 组合键（Ctrl/Alt）一律放行；AltGr（部分布局 Ctrl+Alt）列入 TODO。
  if (mask & (kMaskControl | kMaskAlt)) return S_OK;
  bool shift = (mask & kMaskShift) != 0;
  uint32_t keysym = VkToKeysymForEvent(vk, shift);
  if (!keysym) return S_OK;
  if (composing_) {  // 组词中：所有可映射键都吃
    *pfEaten = TRUE;
    return S_OK;
  }
  if (ascii_mode_) return S_OK;  // 英文模式：全放行
  // 非组词中文模式：可打印 ASCII 可能开启组词，吃掉由引擎裁决
  if (keysym >= 0x20 && keysym <= 0x7E) *pfEaten = TRUE;
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnTestKeyUp(ITfContext* pic, WPARAM wParam,
                                             LPARAM lParam, BOOL* pfEaten) {
  (void)pic;
  (void)wParam;
  (void)lParam;
  if (!pfEaten) return E_INVALIDARG;
  *pfEaten = FALSE;
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnKeyUp(ITfContext* pic, WPARAM wParam,
                                         LPARAM lParam, BOOL* pfEaten) {
  (void)pic;
  (void)wParam;
  (void)lParam;
  if (!pfEaten) return E_INVALIDARG;
  *pfEaten = FALSE;
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnPreservedKey(ITfContext* pic, REFGUID rguid,
                                                BOOL* pfEaten) {
  (void)pic;
  (void)rguid;
  if (!pfEaten) return E_INVALIDARG;
  *pfEaten = FALSE;  // 一期不注册保留键
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnKeyDown(ITfContext* pic, WPARAM wParam,
                                           LPARAM lParam, BOOL* pfEaten) {
  (void)lParam;
  if (!pfEaten) return E_INVALIDARG;
  *pfEaten = FALSE;
  // 按键回调自带上下文：焦点通知未到时用它兜底（与 OnSetFocus 同一对象）。
  if (pic && pic != context_) {
    if (context_) context_->Release();
    context_ = pic;
    context_->AddRef();
  }
  UINT vk = static_cast<UINT>(wParam);
  if (vk == VK_PROCESSKEY) return S_OK;
  uint32_t mask = CurrentKeyMask();
  if (mask & (kMaskControl | kMaskAlt)) return S_OK;
  bool shift = (mask & kMaskShift) != 0;
  uint32_t keysym = VkToKeysymForEvent(vk, shift);
  if (!keysym) return S_OK;

  if (!session_ready_ && !EnsureSession()) {
    // Server 未启动：透传模式。此前若已吃掉可打印键，插入原字符兜底。
    if (keysym >= 0x20 && keysym <= 0x7E) {
      RequestEditSession([this, keysym](TfEditCookie ec) {
        EndComposition(ec, true);
        InsertLiteral(ec, keysym);
      });
      *pfEaten = TRUE;
      return S_OK;
    }
    return S_OK;  // 非可打印键：交由宿主处理
  }

  pipe::FieldMap req;
  pipe::SetFieldInt(&req, "session", session_key_);
  pipe::SetFieldInt(&req, "keycode", static_cast<long long>(keysym));
  pipe::SetFieldInt(&req, "mask", static_cast<long long>(mask));

  bool consumed = false;
  try {
    auto resp = pipe::Request(pipe::kMethodProcessKey, req);
    std::string err;
    if (pipe::GetField(resp, "__error__", &err)) {
      // 服务端报错或掉线：重建会话重试一次，再失败进入透传
      session_ready_ = false;
      if (EnsureSession()) {
        pipe::SetFieldInt(&req, "session", session_key_);
        resp = pipe::Request(pipe::kMethodProcessKey, req);
      } else {
        if (keysym >= 0x20 && keysym <= 0x7E) {
          RequestEditSession([this, keysym](TfEditCookie ec) {
            EndComposition(ec, true);
            InsertLiteral(ec, keysym);
          });
          *pfEaten = TRUE;
          return S_OK;
        }
        return S_OK;
      }
    }
    long long v = 0;
    pipe::GetFieldInt(resp, "consumed", &v);
    consumed = v != 0;
    long long ascii = 0;
    if (pipe::GetFieldInt(resp, "ascii", &ascii)) ascii_mode_ = ascii != 0;
  } catch (const PipeError&) {
    session_ready_ = false;
    if (keysym >= 0x20 && keysym <= 0x7E) {
      RequestEditSession([this, keysym](TfEditCookie ec) {
        EndComposition(ec, true);
        InsertLiteral(ec, keysym);
      });
      *pfEaten = TRUE;
      return S_OK;
    }
    return S_OK;
  }

  if (!consumed) {
    // 引擎未消费：该键此前被 OnTestKeyDown 吃掉，必须插入原字符。
    RequestEditSession([this, keysym](TfEditCookie ec) {
      EndComposition(ec, true);
      InsertLiteral(ec, keysym);
      candidates_.Hide();
    });
    *pfEaten = TRUE;
    return S_OK;
  }

  RequestEditSession([this](TfEditCookie ec) { this->UpdateUi(ec); });
  *pfEaten = TRUE;
  return S_OK;
}

void OpenKeyTextService::UpdateUi(TfEditCookie ec) {
  if (!context_ && ptim_) {
    // ITfThreadMgr::GetFocus 返回的是文档管理器，需再取栈顶上下文。
    ITfDocumentMgr* dim = nullptr;
    if (SUCCEEDED(ptim_->GetFocus(&dim)) && dim) {
      ITfContext* ctx = nullptr;
      if (SUCCEEDED(dim->GetTop(&ctx)) && ctx) context_ = ctx;
      dim->Release();
    }
  }
  if (!context_) return;

  // 1) 有待上屏文本：写入并结束组词
  std::string commit;
  bool has_commit = false;
  if (session_ready_) {
    try {
      pipe::FieldMap req;
      pipe::SetFieldInt(&req, "session", session_key_);
      auto resp = pipe::Request(pipe::kMethodGetCommit, req);
      std::string err;
      if (!pipe::GetField(resp, "__error__", &err)) {
        has_commit = pipe::GetField(resp, "text", &commit);
      }
    } catch (const PipeError&) {
      session_ready_ = false;
    }
  }
  if (has_commit && !commit.empty()) {
    std::wstring text = Utf8ToWide(commit);
    if (composition_range_) {
      composition_range_->SetText(ec, 0, text.c_str(),
                                  static_cast<LONG>(text.size()));
    } else {
      // 无组词时在光标插入 commit
      WriteTextAtSelection(ec, text);
    }
    EndComposition(ec, false);
    candidates_.Hide();
    RefreshStatus();
    return;
  }

  // 2) 取上下文（预编辑 + 候选）
  openkey::Context ctx{};
  bool ok = false;
  if (session_ready_) {
    try {
      pipe::FieldMap req;
      pipe::SetFieldInt(&req, "session", session_key_);
      auto resp = pipe::Request(pipe::kMethodGetContext, req);
      std::string err;
      if (!pipe::GetField(resp, "__error__", &err)) {
        // 解析协议字段
        pipe::GetField(resp, "preedit", &ctx.preedit);
        ctx.composing = !ctx.preedit.empty();
        long long v = 0;
        if (pipe::GetFieldInt(resp, "cursor", &v)) ctx.cursor_pos = (int)v;
        if (pipe::GetFieldInt(resp, "page", &v)) ctx.page_no = (int)v;
        if (pipe::GetFieldInt(resp, "page_size", &v)) ctx.page_size = (int)v;
        if (pipe::GetFieldInt(resp, "last_page", &v)) ctx.is_last_page = v != 0;
        if (pipe::GetFieldInt(resp, "highlighted", &v))
          ctx.highlighted_candidate_index = (int)v;
        long long n = 0;
        pipe::GetFieldInt(resp, "cands", &n);
        for (long long i = 0; i < n && i < 64; ++i) {
          std::string key = "cand." + std::to_string(i);
          openkey::Candidate c;
          if (!pipe::GetField(resp, key.c_str(), &c.text)) continue;
          pipe::GetField(resp, (key + ".comment").c_str(), &c.comment);
          ctx.candidates.push_back(std::move(c));
        }
        ok = true;
      }
    } catch (const PipeError&) {
      session_ready_ = false;
    }
  }

  if (!ok || !ctx.composing) {
    EndComposition(ec, true);
    candidates_.Hide();
    return;
  }

  // 3) 确保组词对象
  if (!composition_) {
    ITfContextComposition* comp = nullptr;
    if (FAILED(context_->QueryInterface(IID_ITfContextComposition,
                                        reinterpret_cast<void**>(&comp))) ||
        !comp) {
      return;
    }
    // 取当前选区作为组词起始范围（TF_SELECTION 形式，见 msctf.h）。
    TF_SELECTION sel{};
    ULONG fetched = 0;
    if (FAILED(context_->GetSelection(ec, TF_DEFAULT_SELECTION, 1, &sel,
                                      &fetched)) ||
        fetched != 1 || !sel.range) {
      if (sel.range) sel.range->Release();
      comp->Release();
      return;
    }
    HRESULT hr = comp->StartComposition(
        ec, sel.range, static_cast<ITfCompositionSink*>(this), &composition_);
    sel.range->Release();
    comp->Release();
    if (FAILED(hr) || !composition_) return;
    if (FAILED(composition_->GetRange(&composition_range_)) ||
        !composition_range_) {
      composition_->Release();
      composition_ = nullptr;
      return;
    }
  }

  // 4) 写预编辑
  std::wstring preedit = Utf8ToWide(ctx.preedit);
  composition_range_->SetText(ec, 0, preedit.c_str(),
                              static_cast<LONG>(preedit.size()));
  composing_ = true;

  // 5) 刷新候选窗（光标位置）
  std::vector<CandidateItem> items;
  items.reserve(ctx.candidates.size());
  for (const auto& c : ctx.candidates) {
    CandidateItem item;
    item.text = Utf8ToWide(c.text);
    item.comment = Utf8ToWide(c.comment);
    items.push_back(std::move(item));
  }
  GUITHREADINFO gti{};
  gti.cbSize = sizeof(gti);
  POINT caret{};
  if (GetGUIThreadInfo(GetCurrentThreadId(), &gti)) {
    caret.x = gti.rcCaret.left;
    caret.y = gti.rcCaret.bottom;
    if (caret.x == 0 && caret.y == 0) GetCursorPos(&caret);
  } else {
    GetCursorPos(&caret);
  }
  if (items.empty()) {
    candidates_.Hide();
  } else {
    candidates_.Show(items, ctx.page_no, ctx.is_last_page,
                     ctx.highlighted_candidate_index, caret);
  }
}

void OpenKeyTextService::EndComposition(TfEditCookie ec, bool discard) {
  if (composition_) {
    if (discard && composition_range_) {
      composition_range_->SetText(ec, 0, L"", 0);
    }
    composition_->EndComposition(ec);
    composition_->Release();
    composition_ = nullptr;
  }
  if (composition_range_) {
    composition_range_->Release();
    composition_range_ = nullptr;
  }
  composing_ = false;
}

/* 在光标处写入文本：取当前选区（TF_SELECTION）-> SetText -> 回写选区，
 * 使光标停在插入文本之后（TSF 惯例：只 SetText 不 SetSelection 会导致
 * 光标停留在原处，连续输入时字符顺序错乱）。 */
bool OpenKeyTextService::WriteTextAtSelection(TfEditCookie ec,
                                              const std::wstring& text) {
  if (!context_) return false;
  TF_SELECTION sel{};
  ULONG fetched = 0;
  if (FAILED(context_->GetSelection(ec, TF_DEFAULT_SELECTION, 1, &sel,
                                    &fetched)) ||
      fetched != 1 || !sel.range) {
    if (sel.range) sel.range->Release();
    return false;
  }
  sel.range->SetText(ec, 0, text.c_str(), static_cast<LONG>(text.size()));
  context_->SetSelection(ec, 1, &sel);
  sel.range->Release();
  return true;
}

void OpenKeyTextService::InsertLiteral(TfEditCookie ec, uint32_t keysym) {
  if (!context_) return;
  if (keysym == 0 || keysym > 0xFFFF) return;
  wchar_t ch = static_cast<wchar_t>(keysym);
  if (ch < 0x20) return;
  WriteTextAtSelection(ec, std::wstring(1, ch));
}

void OpenKeyTextService::RefreshCandidates() {
  if (!session_ready_) {
    candidates_.Hide();
    return;
  }
  openkey::Context ctx{};
  try {
    pipe::FieldMap req;
    pipe::SetFieldInt(&req, "session", session_key_);
    auto resp = pipe::Request(pipe::kMethodGetContext, req);
    std::string err;
    if (pipe::GetField(resp, "__error__", &err)) return;
    pipe::GetField(resp, "preedit", &ctx.preedit);
    ctx.composing = !ctx.preedit.empty();
    long long v = 0;
    if (pipe::GetFieldInt(resp, "page", &v)) ctx.page_no = (int)v;
    if (pipe::GetFieldInt(resp, "last_page", &v)) ctx.is_last_page = v != 0;
    if (pipe::GetFieldInt(resp, "highlighted", &v))
      ctx.highlighted_candidate_index = (int)v;
    long long n = 0;
    pipe::GetFieldInt(resp, "cands", &n);
    for (long long i = 0; i < n && i < 64; ++i) {
      std::string key = "cand." + std::to_string(i);
      openkey::Candidate c;
      if (!pipe::GetField(resp, key.c_str(), &c.text)) continue;
      pipe::GetField(resp, (key + ".comment").c_str(), &c.comment);
      ctx.candidates.push_back(std::move(c));
    }
  } catch (const PipeError&) {
    candidates_.Hide();
    return;
  }
  if (!ctx.composing || ctx.candidates.empty()) {
    candidates_.Hide();
    return;
  }
  std::vector<CandidateItem> items;
  items.reserve(ctx.candidates.size());
  for (const auto& c : ctx.candidates) {
    CandidateItem item;
    item.text = Utf8ToWide(c.text);
    item.comment = Utf8ToWide(c.comment);
    items.push_back(std::move(item));
  }
  GUITHREADINFO gti{};
  gti.cbSize = sizeof(gti);
  POINT caret{};
  if (GetGUIThreadInfo(GetCurrentThreadId(), &gti)) {
    caret.x = gti.rcCaret.left;
    caret.y = gti.rcCaret.bottom;
  } else {
    GetCursorPos(&caret);
  }
  candidates_.Show(items, ctx.page_no, ctx.is_last_page,
                   ctx.highlighted_candidate_index, caret);
}

void OpenKeyTextService::OnCandidateSelected(size_t index) {
  if (!session_ready_) return;
  try {
    pipe::FieldMap req;
    pipe::SetFieldInt(&req, "session", session_key_);
    pipe::SetFieldInt(&req, "index", static_cast<long long>(index));
    auto resp = pipe::Request(pipe::kMethodSelectCandidate, req);
    std::string err;
    if (pipe::GetField(resp, "__error__", &err)) return;
  } catch (const PipeError&) {
    session_ready_ = false;
    return;
  }
  // 选择后通常直接上屏或刷新预编辑；统一刷新。
  RefreshCandidates();
  if (!composing_) candidates_.Hide();
}

/* ---------------- ITfThreadMgrEventSink ---------------- */

STDMETHODIMP OpenKeyTextService::OnInitDocumentMgr(ITfDocumentMgr* pdim) {
  (void)pdim;
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnUninitDocumentMgr(ITfDocumentMgr* pdim) {
  (void)pdim;
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnSetFocus(ITfDocumentMgr* pdimFocus,
                                            ITfDocumentMgr* pdimPrev) {
  (void)pdimPrev;
  TrackContext(pdimFocus);
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnPushContext(ITfContext* pic) {
  (void)pic;
  return S_OK;
}

STDMETHODIMP OpenKeyTextService::OnPopContext(ITfContext* pic) {
  (void)pic;
  return S_OK;
}

/* ---------------- ITfCompositionSink ---------------- */

STDMETHODIMP OpenKeyTextService::OnCompositionTerminated(TfEditCookie ecWrite,
                                                         ITfComposition*
                                                             pComposition) {
  (void)ecWrite;
  if (pComposition && pComposition == composition_) {
    if (composition_range_) {
      composition_range_->Release();
      composition_range_ = nullptr;
    }
    composition_->Release();
    composition_ = nullptr;
    composing_ = false;
    candidates_.Hide();
  }
  return S_OK;
}

/* ------------------------------------------------------------------ */
/* OpenKeyClassFactory                                                  */
/* ------------------------------------------------------------------ */

OpenKeyClassFactory::OpenKeyClassFactory() { OpenKeyDllAddRef(); }

OpenKeyClassFactory::~OpenKeyClassFactory() { OpenKeyDllRelease(); }

STDMETHODIMP OpenKeyClassFactory::QueryInterface(REFIID riid, void** ppv) {
  if (!ppv) return E_INVALIDARG;
  if (riid == IID_IUnknown || riid == IID_IClassFactory) {
    *ppv = static_cast<IClassFactory*>(this);
    AddRef();
    return S_OK;
  }
  *ppv = nullptr;
  return E_NOINTERFACE;
}

STDMETHODIMP_(ULONG) OpenKeyClassFactory::AddRef() { return ++ref_; }

STDMETHODIMP_(ULONG) OpenKeyClassFactory::Release() {
  ULONG n = --ref_;
  if (n == 0) delete this;
  return n;
}

STDMETHODIMP OpenKeyClassFactory::CreateInstance(IUnknown* pUnkOuter,
                                                 REFIID riid, void** ppv) {
  if (pUnkOuter) return CLASS_E_NOAGGREGATION;
  if (!ppv) return E_INVALIDARG;
  auto* svc = new OpenKeyTextService();
  HRESULT hr = svc->QueryInterface(riid, ppv);
  svc->Release();
  return hr;
}

STDMETHODIMP OpenKeyClassFactory::LockServer(BOOL lock) {
  if (lock) {
    OpenKeyDllLock();
  } else {
    OpenKeyDllUnlock();
  }
  return S_OK;
}
