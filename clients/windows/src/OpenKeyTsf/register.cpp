/*
 * 文件用途：OpenKeyTsf 的 CTF TIP 注册表登记（DllRegisterServer /
 * DllUnregisterServer），按微软 TIP 注册惯例（docs/02 4.1.2）。
 *
 * 注册内容（x64 视图；需管理员权限执行 regsvr32）：
 *   HKCR\CLSID\{clsid}\InprocServer32            = dll 路径, ThreadingModel=Both
 *   HKLM\SOFTWARE\Microsoft\CTF\TIP\{clsid}      Description / DisplayName /
 *                                                IconFile / IconIndex
 *   HKLM\SOFTWARE\Microsoft\CTF\TIP\{clsid}\Category
 *                                                GUID_TFCAT_TIP_KEYBOARD
 *   HKLM\SOFTWARE\Microsoft\CTF\TIP\{clsid}\LanguageProfile
 *        \0x00000004\{profile guid}              Description / DisplayName /
 *                                                IconFile / IconIndex
 *
 * 已知 TODO（README 同步）：
 *   - 图标资源：IconFile 暂指向 DLL 自身、IconIndex=0，待补图标资源；
 *   - 能力类目（GUID_TFCAT_TIPCAP_SECUREMODE 等）按需再加；
 *   - ARM64 注册与 x64 同构，CI 出包时分别执行。
 */

#include <windows.h>

#include <string>

#include "text_service.h"

namespace {

const wchar_t kDescription[] = L"OpenKey Pinyin Input Method";
const wchar_t kDisplayName[] = L"OpenKey";
const wchar_t kProfileDescription[] = L"OpenKey Pinyin";

/* 键盘类目 GUID（GUID_TFCAT_TIP_KEYBOARD）。 */
const wchar_t kCategoryKeyboard[] =
    L"{34745C63-B2F0-4784-8B67-5E12C8701A31}";

/* 中文（简体）语言 ID 的注册键名。 */
const wchar_t kLangIdZhCn[] = L"0x00000004";

/* 拼音方案 profile GUID（自定义固定值，全局唯一即可）。 */
const wchar_t kProfileGuid[] = L"{6B3E1C4D-9A2F-4E5B-8C7D-1F2A3B4C5D6E}";

std::wstring GuidToString(REFCLSID clsid) {
  wchar_t buf[64] = {};
  StringFromGUID2(clsid, buf, 64);
  return buf;
}

/* 取本 DLL 路径（用函数地址反查模块句柄，避免依赖 DllMain 传递）。 */
std::wstring GetDllPath() {
  HMODULE module = nullptr;
  GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                         GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                     reinterpret_cast<LPCWSTR>(&GetDllPath), &module);
  wchar_t path[MAX_PATH] = {};
  if (module) GetModuleFileNameW(module, path, MAX_PATH);
  return path;
}

LONG SetStringValue(HKEY key, const wchar_t* name, const wchar_t* value) {
  return RegSetValueExW(key, name, 0, REG_SZ,
                        reinterpret_cast<const BYTE*>(value),
                        static_cast<DWORD>((wcslen(value) + 1) *
                                           sizeof(wchar_t)));
}

LONG SetDwordValue(HKEY key, const wchar_t* name, DWORD value) {
  return RegSetValueExW(key, name, 0, REG_DWORD,
                        reinterpret_cast<const BYTE*>(&value), sizeof(value));
}

/* 递归删除子键（RegDeleteTreeW 需 Vista+，本项目目标 Win10/11）。 */
LONG DeleteKeyRecursive(HKEY parent, const wchar_t* name) {
  return RegDeleteTreeW(parent, name);
}

HRESULT RegisterComClass() {
  std::wstring clsid = GuidToString(CLSID_OpenKeyTextService);
  std::wstring path = L"CLSID\\" + clsid + L"\\InprocServer32";
  HKEY key = nullptr;
  LONG err = RegCreateKeyExW(HKEY_CLASSES_ROOT, path.c_str(), 0, nullptr, 0,
                             KEY_ALL_ACCESS, nullptr, &key, nullptr);
  if (err != ERROR_SUCCESS) return HRESULT_FROM_WIN32(err);
  SetStringValue(key, nullptr, GetDllPath().c_str());
  SetStringValue(key, L"ThreadingModel", L"Both");
  RegCloseKey(key);

  // CLSID 键默认值（描述）
  path = L"CLSID\\" + clsid;
  err = RegCreateKeyExW(HKEY_CLASSES_ROOT, path.c_str(), 0, nullptr, 0,
                        KEY_ALL_ACCESS, nullptr, &key, nullptr);
  if (err != ERROR_SUCCESS) return HRESULT_FROM_WIN32(err);
  SetStringValue(key, nullptr, kDescription);
  RegCloseKey(key);
  return S_OK;
}

HRESULT RegisterTip() {
  std::wstring clsid = GuidToString(CLSID_OpenKeyTextService);
  std::wstring base = L"SOFTWARE\\Microsoft\\CTF\\TIP\\" + clsid;

  HKEY key = nullptr;
  LONG err = RegCreateKeyExW(HKEY_LOCAL_MACHINE, base.c_str(), 0, nullptr, 0,
                             KEY_ALL_ACCESS, nullptr, &key, nullptr);
  if (err != ERROR_SUCCESS) return HRESULT_FROM_WIN32(err);
  SetStringValue(key, L"Description", kDescription);
  SetStringValue(key, L"DisplayName", kDisplayName);
  SetStringValue(key, L"IconFile", GetDllPath().c_str());
  SetDwordValue(key, L"IconIndex", 0);
  RegCloseKey(key);

  // 类目：值名为类目 GUID、数据为空串（微软注册惯例）
  err = RegCreateKeyExW(HKEY_LOCAL_MACHINE, (base + L"\\Category").c_str(), 0,
                        nullptr, 0, KEY_ALL_ACCESS, nullptr, &key, nullptr);
  if (err != ERROR_SUCCESS) return HRESULT_FROM_WIN32(err);
  SetStringValue(key, kCategoryKeyboard, L"");
  RegCloseKey(key);

  // 语言 profile：0x00000004（zh-CN）-> profile guid
  std::wstring profile = base + L"\\LanguageProfile\\" + kLangIdZhCn +
                         L"\\" + kProfileGuid;
  err = RegCreateKeyExW(HKEY_LOCAL_MACHINE, profile.c_str(), 0, nullptr, 0,
                        KEY_ALL_ACCESS, nullptr, &key, nullptr);
  if (err != ERROR_SUCCESS) return HRESULT_FROM_WIN32(err);
  SetStringValue(key, L"Description", kProfileDescription);
  SetStringValue(key, L"DisplayName", kDisplayName);
  SetStringValue(key, L"IconFile", GetDllPath().c_str());
  SetDwordValue(key, L"IconIndex", 0);
  RegCloseKey(key);
  return S_OK;
}

}  // namespace

STDAPI DllRegisterServer() {
  HRESULT hr = RegisterComClass();
  if (FAILED(hr)) return hr;
  return RegisterTip();
}

STDAPI DllUnregisterServer() {
  std::wstring clsid = GuidToString(CLSID_OpenKeyTextService);
  // 先删 TIP 树，再删 COM 类树；个别失败不阻断另一方
  DeleteKeyRecursive(HKEY_LOCAL_MACHINE,
                     (L"SOFTWARE\\Microsoft\\CTF\\TIP\\" + clsid).c_str());
  DeleteKeyRecursive(HKEY_CLASSES_ROOT, (L"CLSID\\" + clsid).c_str());
  return S_OK;
}
