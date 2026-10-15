/*
 * 文件用途：OpenKeyTsf.dll 入口：DLL 引用计数、DPI 感知、COM 导出函数
 * （DllGetClassObject / DllCanUnloadNow / DllRegisterServer /
 * DllUnregisterServer，后两者实现在 register.cpp）。
 */

#include <windows.h>

#include <new>

#include "text_service.h"

static LONG g_lock_count = 0;
static LONG g_object_count = 0;

void OpenKeyDllAddRef() { InterlockedIncrement(&g_object_count); }
void OpenKeyDllRelease() { InterlockedDecrement(&g_object_count); }
void OpenKeyDllLock() { InterlockedIncrement(&g_lock_count); }
void OpenKeyDllUnlock() { InterlockedDecrement(&g_lock_count); }

STDAPI DllCanUnloadNow() {
  return (g_object_count == 0 && g_lock_count == 0) ? S_OK : S_FALSE;
}

STDAPI DllGetClassObject(REFCLSID clsid, REFIID riid, void** ppv) {
  if (clsid != CLSID_OpenKeyTextService) return CLASS_E_CLASSNOTAVAILABLE;
  if (!ppv) return E_INVALIDARG;
  auto* factory = new (std::nothrow) OpenKeyClassFactory();
  if (!factory) return E_OUTOFMEMORY;
  HRESULT hr = factory->QueryInterface(riid, ppv);
  factory->Release();
  return hr;
}

STDAPI DllRegisterServer();
STDAPI DllUnregisterServer();

BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID reserved) {
  (void)reserved;
  switch (reason) {
    case DLL_PROCESS_ATTACH:
      DisableThreadLibraryCalls(module);
      // 进程级 DPI 感知：候选窗坐标按物理像素计算（docs/02 4.1.6 多显示器 DPI）。
      // 一期用最稳定的 SetProcessDPIAware（每显示器 V2 感知列入 TODO）。
      SetProcessDPIAware();
      break;
    case DLL_PROCESS_DETACH:
      break;
    default:
      break;
  }
  return TRUE;
}
