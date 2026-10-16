// OpenKeySetup：Win32 设置窗口（docs/02 4.1「设置 UI 骨架」）。
//
// v0.1.0 最小实现：输入方案选择（主方案/模糊音/四套双拼——docs/02 5.1 的
// 双拼与模糊音开关在本里程碑经 ok_session_select_schema 以方案切换落地）、
// 英文模式复选框（ok_session_set_option("ascii_mode")）。
// 设置窗口直连 core（不经 Server；目录约定见下）。
//
// 目录约定（docs/02 4.1.4）：
//   共享数据：环境变量 OPENKEY_SHARED_DIR，缺省 <exe 目录>\shared；
//   用户数据：%APPDATA%\OpenKey（rime user / openkey.db / components / log）。
// 后续里程碑替换为 WPF/WinUI 富 UI（docs/02 4.1 设置项清单）。

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>

#include <cstdlib>  // getenv
#include <string>
#include <vector>

#include "openkey_client.h"

namespace {

constexpr wchar_t kWindowClass[] = L"OpenKeySetupWindow";
constexpr int IDC_SCHEMA_COMBO = 1001;
constexpr int IDC_ASCII_CHECK = 1002;

openkey::Engine* g_engine = nullptr;
openkey::Session* g_session = nullptr;
std::vector<openkey::SchemaInfo> g_schemas;

std::wstring Utf8ToWide(const std::string& s) {
  if (s.empty()) return {};
  int len = MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0);
  std::wstring out(len, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), out.data(), len);
  return out;
}

std::string ExeDirectory() {
  wchar_t path[MAX_PATH] = {0};
  GetModuleFileNameW(nullptr, path, MAX_PATH);
  std::wstring dir(path);
  size_t slash = dir.find_last_of(L"\\/");
  if (slash != std::wstring::npos) {
    dir.resize(slash);
  }
  int len = WideCharToMultiByte(CP_UTF8, 0, dir.data(), (int)dir.size(), nullptr, 0,
                                nullptr, nullptr);
  std::string out(len, '\0');
  WideCharToMultiByte(CP_UTF8, 0, dir.data(), (int)dir.size(), out.data(), len,
                      nullptr, nullptr);
  return out;
}

std::string AppDataDir() {
  // %APPDATA%\OpenKey（与 OpenKeyServer 的目录约定一致，docs/02 4.1.4）。
  // 用环境变量而非 SHGetKnownFolderPath：避免引入 knownfolders GUID 的
  // 额外链接依赖（uuid.lib 之外的符号），且行为与 Server 侧完全一致。
  wchar_t buf[MAX_PATH] = {0};
  DWORD n = GetEnvironmentVariableW(L"APPDATA", buf, MAX_PATH);
  std::wstring dir = (n > 0 && n < MAX_PATH) ? std::wstring(buf) : L".";
  dir += L"\\OpenKey";
  int len = WideCharToMultiByte(CP_UTF8, 0, dir.data(), (int)dir.size(), nullptr, 0,
                                nullptr, nullptr);
  std::string out(len, '\0');
  WideCharToMultiByte(CP_UTF8, 0, dir.data(), (int)dir.size(), out.data(), len,
                      nullptr, nullptr);
  return out;
}

bool InitEngineAndSession() {
  openkey::EngineConfig config;
  if (const char* shared_env = getenv("OPENKEY_SHARED_DIR")) {
    config.shared_data_dir = shared_env;
  } else {
    config.shared_data_dir = ExeDirectory() + "\\shared";
  }
  config.user_data_dir = AppDataDir();
  config.data_db_path = config.user_data_dir + "\\openkey.db";
  config.components_dir = config.user_data_dir + "\\components";
  config.log_dir = config.user_data_dir + "\\log";

  CreateDirectoryA(config.user_data_dir.c_str(), nullptr);
  CreateDirectoryA(config.components_dir.c_str(), nullptr);

  static openkey::Engine engine = openkey::Engine::Create(config);
  g_engine = &engine;
  static openkey::Session session = openkey::Session::Create(engine);
  g_session = &session;
  g_schemas = engine.SchemaList();
  return true;
}

void SaveSettings(HWND dlg) {
  HWND combo = GetDlgItem(dlg, IDC_SCHEMA_COMBO);
  LRESULT index = SendMessageW(combo, CB_GETCURSEL, 0, 0);
  if (index >= 0 && index < (LRESULT)g_schemas.size()) {
    if (!g_session->SelectSchema(g_schemas[index].id) && g_engine) {
      MessageBoxW(dlg, Utf8ToWide("方案切换失败: " + g_engine->LastError()).c_str(),
                  L"开门输入法", MB_ICONERROR);
    }
  }
  if (g_session) {
    bool ascii = IsDlgButtonChecked(dlg, IDC_ASCII_CHECK) == BST_CHECKED;
    g_session->SetOption("ascii_mode", ascii);
  }
}

LRESULT CALLBACK SetupWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
  switch (msg) {
    case WM_CREATE: {
      CreateWindowW(L"STATIC", L"输入方案：", WS_CHILD | WS_VISIBLE, 16, 16, 80, 20,
                    hwnd, nullptr, nullptr, nullptr);
      CreateWindowW(L"COMBOBOX", nullptr,
                    WS_CHILD | WS_VISIBLE | CBS_DROPDOWNLIST | WS_VSCROLL,
                    100, 12, 280, 200, hwnd, (HMENU)(INT_PTR)IDC_SCHEMA_COMBO, nullptr, nullptr);
      CreateWindowW(L"BUTTON", L"英文模式", WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX,
                    16, 48, 100, 20, hwnd, (HMENU)(INT_PTR)IDC_ASCII_CHECK, nullptr, nullptr);
      CreateWindowW(L"BUTTON", L"确定", WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
                    220, 150, 80, 26, hwnd, (HMENU)(INT_PTR)IDOK, nullptr, nullptr);
      CreateWindowW(L"BUTTON", L"取消", WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
                    310, 150, 80, 26, hwnd, (HMENU)(INT_PTR)IDCANCEL, nullptr, nullptr);

      if (!InitEngineAndSession()) {
        MessageBoxW(hwnd, L"引擎初始化失败（详见 %APPDATA%\\OpenKey\\log）",
                    L"开门输入法", MB_ICONERROR);
        return -1;
      }
      HWND combo = GetDlgItem(hwnd, IDC_SCHEMA_COMBO);
      std::string current = g_session->CurrentSchema();
      int select = 0;
      for (size_t i = 0; i < g_schemas.size(); ++i) {
        SendMessageW(combo, CB_ADDSTRING, 0, (LPARAM)Utf8ToWide(g_schemas[i].name).c_str());
        if (g_schemas[i].id == current) {
          select = (int)i;
        }
      }
      SendMessageW(combo, CB_SETCURSEL, (WPARAM)select, 0);
      CheckDlgButton(hwnd, IDC_ASCII_CHECK,
                     g_session->GetOption("ascii_mode") ? BST_CHECKED : BST_UNCHECKED);
      return 0;
    }
    case WM_COMMAND:
      if (LOWORD(wp) == IDOK) {
        SaveSettings(hwnd);
        DestroyWindow(hwnd);
        return 0;
      }
      if (LOWORD(wp) == IDCANCEL) {
        DestroyWindow(hwnd);
        return 0;
      }
      return 0;
    case WM_CLOSE:
      DestroyWindow(hwnd);
      return 0;
    case WM_DESTROY:
      PostQuitMessage(0);
      return 0;
    default:
      return DefWindowProcW(hwnd, msg, wp, lp);
  }
}

}  // namespace

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE, LPWSTR, int cmd_show) {
  WNDCLASSEXW wc = {0};
  wc.cbSize = sizeof(wc);
  wc.lpfnWndProc = SetupWndProc;
  wc.hInstance = instance;
  wc.lpszClassName = kWindowClass;
  wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
  wc.hbrBackground = (HBRUSH)(COLOR_BTNFACE + 1);
  if (!RegisterClassExW(&wc)) {
    return 1;
  }

  HWND hwnd = CreateWindowExW(WS_EX_DLGMODALFRAME, kWindowClass, L"开门输入法设置",
                              WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                              CW_USEDEFAULT, CW_USEDEFAULT, 420, 230,
                              nullptr, nullptr, instance, nullptr);
  if (!hwnd) {
    return 1;
  }
  ShowWindow(hwnd, cmd_show);
  UpdateWindow(hwnd);

  MSG msg;
  while (GetMessageW(&msg, nullptr, 0, 0)) {
    TranslateMessage(&msg);
    DispatchMessageW(&msg);
  }
  return (int)msg.wParam;
}
