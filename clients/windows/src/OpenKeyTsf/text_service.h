/*
 * 文件用途：OpenKey TSF 文本服务（Text Input Processor）。
 *
 * 职责（docs/02 4.1.2）：
 *   - 实现 ITfTextInputProcessorEx / ITfKeyEventSink / ITfThreadMgrEventSink /
 *     ITfCompositionSink / ITfEditSession 的最小可用版本；
 *   - 按键管线：OnKeyDown -> keysym+mask -> 命名管道 ProcessKey -> 被消费则
 *     经同步编辑会话更新预编辑（ITfRange::SetText）并刷新候选窗；未被消费
 *     且该键此前被 OnTestKeyDown 吃掉时，插入原字符，保证英文可打；
 *   - 零网络、零重逻辑（宿主容器约束）：所有计算在 OpenKeyServer 进程。
 *
 * 一期明确不做（留 TODO，见 README）：
 *   - ITfDisplayAttributeProvider（预编辑下划线样式）：预编辑以纯文本呈现；
 *   - ITfCandidateListUIElement：候选窗为自绘 GDI 弹出窗（candidate_window）。
 *     注：tsf-sample-ime 的类厂只需标准 IClassFactory（DllGetClassObject），
 *     ITfClassFactory 用于按配置激活的 TIP 实例化路径，一期不走该路径。
 */

#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include <msctf.h>
#include <unknwnbase.h>

#include <cstdint>
#include <functional>

#include "candidate_window.h"
#include "keysym_map.h"
#include "pipe_client.h"

// OpenKey TIP 的 CLSID（{7E2A9C31-5B4D-4A6F-9E58-2C1D0F3A8B60}），
// 与 register.cpp 中注册的 CLSID 必须一致。
static const CLSID CLSID_OpenKeyTextService = {
    0x7e2a9c31,
    0x5b4d,
    0x4a6f,
    {0x9e, 0x58, 0x2c, 0x1d, 0x0f, 0x3a, 0x8b, 0x60}};

/* 服务端会话键：以 TSF 线程 ID 标识（Server 侧 map<DWORD, OkSession*>）。 */
inline DWORD OpenKeySessionKey() { return GetCurrentThreadId(); }

/* DLL 级引用计数（dllmain.cpp 实现；供 DllCanUnloadNow 判定卸载时机）。 */
void OpenKeyDllAddRef();
void OpenKeyDllRelease();
void OpenKeyDllLock();
void OpenKeyDllUnlock();

/* 编辑会话：把一段「持 ec 的逻辑」托管给 TSF 在上下文线程执行。 */
class OpenKeyEditSession : public ITfEditSession {
 public:
  OpenKeyEditSession(std::function<void(TfEditCookie)> fn);
  virtual ~OpenKeyEditSession();

  // IUnknown
  STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;

  // ITfEditSession
  STDMETHODIMP DoEditSession(TfEditCookie ec) override;

 private:
  ULONG ref_ = 1;
  std::function<void(TfEditCookie)> fn_;
};

/* OpenKey 文本服务对象。线程模型 Both（register.cpp 注册）。 */
class OpenKeyTextService : public ITfTextInputProcessorEx,
                           public ITfKeyEventSink,
                           public ITfThreadMgrEventSink,
                           public ITfCompositionSink {
 public:
  OpenKeyTextService();
  virtual ~OpenKeyTextService();

  // IUnknown
  STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;

  // ITfTextInputProcessor
  STDMETHODIMP Activate(ITfThreadMgr* ptim, TfClientId tid) override;
  STDMETHODIMP Deactivate() override;

  // ITfTextInputProcessorEx
  STDMETHODIMP ActivateEx(ITfThreadMgr* ptim, TfClientId tid, DWORD flags) override;

  // ITfKeyEventSink
  STDMETHODIMP OnKeyDown(ITfKeyEvent* pKeyEvent) override;
  STDMETHODIMP OnKeyUp(ITfKeyEvent* pKeyEvent) override;
  STDMETHODIMP OnTestKeyDown(ITfKeyEvent* pKeyEvent) override;
  STDMETHODIMP OnTestKeyUp(ITfKeyEvent* pKeyEvent) override;

  // ITfThreadMgrEventSink
  STDMETHODIMP OnInitDocumentMgr(ITfDocumentMgr* pdim) override;
  STDMETHODIMP OnUninitDocumentMgr(ITfDocumentMgr* pdim) override;
  STDMETHODIMP OnSetFocus(ITfDocumentMgr* pdimFocus, ITfDocumentMgr* pdimPrev) override;
  STDMETHODIMP OnPushContext(ITfContext* pic) override;
  STDMETHODIMP OnPopContext(ITfContext* pic) override;

  // ITfCompositionSink
  STDMETHODIMP OnCompositionTerminated(TfEditCookie ecWrite,
                                       ITfComposition* pComposition) override;

 private:
  // 与 Server 的会话（惰性创建；失败时 IME 进入透传模式）
  bool EnsureSession();
  bool DestroySession();

  // 发起同步编辑会话；失败返回 false（ptim 无焦点上下文等）
  bool RequestEditSession(std::function<void(TfEditCookie)> fn);

  // 在 ec 上更新预编辑 / 上屏 / 候选窗（DoEditSession 内调用）
  void UpdateUi(TfEditCookie ec);

  // 结束当前组词（ec 内调用；discard=true 时不写入文本）
  void EndComposition(TfEditCookie ec, bool discard);

  // 在光标处插入原字符（按键未被引擎消费时的兜底）
  void InsertLiteral(TfEditCookie ec, uint32_t keysym);

  // 候选窗刷新：取 Server 上下文并展示
  void RefreshCandidates();

  // 刷新 ascii_mode_ 缓存
  void RefreshStatus();

  // 候选窗点击回调（UI 线程）
  void OnCandidateSelected(size_t index);

  // 事件接收器注册/注销
  bool AdviseSinks(ITfContext* context_for_composition);
  void UnadviseSinks();
  void TrackContext(ITfDocumentMgr* pdim);

  ULONG ref_ = 1;

  ITfThreadMgr* ptim_ = nullptr;
  TfClientId tid_ = 0;
  DWORD key_event_cookie_ = TF_INVALID_COOKIE;
  DWORD thread_mgr_cookie_ = TF_INVALID_COOKIE;
  DWORD composition_cookie_ = TF_INVALID_COOKIE;
  ITfContext* context_ = nullptr;  // 焦点上下文（随 OnSetFocus 更新）

  long long session_key_ = 0;  // 服务端会话键（本线程 ID）
  bool session_ready_ = false;
  bool composing_ = false;
  bool ascii_mode_ = false;

  ITfComposition* composition_ = nullptr;  // 当前组词
  ITfRange* composition_range_ = nullptr;  // 组词文本范围
  CandidateWindow candidates_;
};

/* 类厂（标准 IClassFactory；由 DllGetClassObject 返回）。 */
class OpenKeyClassFactory : public IClassFactory {
 public:
  OpenKeyClassFactory();
  virtual ~OpenKeyClassFactory();

  // IUnknown
  STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;

  // IClassFactory
  STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid,
                              void** ppv) override;
  STDMETHODIMP LockServer(BOOL lock) override;

 private:
  ULONG ref_ = 1;
};
