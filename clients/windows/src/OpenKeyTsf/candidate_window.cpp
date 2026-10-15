/*
 * 文件用途：candidate_window.h 的实现：GDI 横向候选条 + 左键选择。
 *
 * 保守取舍：
 *   - 字体取 SystemParametersInfo(SPI_GETNONCLIENTMETRICS) 的消息字体，
 *     不依赖视觉样式 API（uxtheme），任何 Windows 版本可用；
 *   - DPI 缩放用屏幕 DC 的 LOGPIXELSX（全版本可用），不用
 *     GetDpiForWindow（Win10 1607+），避免 SDK 版本门槛；
 *   - 不调用 SetForegroundWindow（TSF 光标窗口抢焦点会打断宿主输入）。
 */

#include "candidate_window.h"

#include <cstdio>

namespace {
constexpr wchar_t kClassName[] = L"OpenKeyCandidates";
bool g_class_registered = false;

/* 当前屏幕 DPI（96 为基准）。 */
int ScreenDpi() {
  HDC dc = GetDC(nullptr);
  int dpi = dc ? GetDeviceCaps(dc, LOGPIXELSX) : 96;
  if (dc) ReleaseDC(nullptr, dc);
  return dpi > 0 ? dpi : 96;
}

int Scale(int px) { return px * ScreenDpi() / 96; }
}  // namespace

std::wstring Utf8ToWide(const std::string& s) {
  if (s.empty()) return {};
  int len = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s.data(),
                                static_cast<int>(s.size()), nullptr, 0);
  UINT cp = CP_UTF8;
  if (len <= 0) {
    // 非法 UTF-8：按本地代码页兜底，保证不崩
    cp = CP_ACP;
    len = MultiByteToWideChar(CP_ACP, 0, s.data(), static_cast<int>(s.size()),
                              nullptr, 0);
    if (len <= 0) return {};
  }
  std::wstring out(static_cast<size_t>(len), L'\0');
  MultiByteToWideChar(cp, 0, s.data(), static_cast<int>(s.size()), out.data(),
                      len);
  return out;
}

bool CandidateWindow::EnsureClassRegistered() {
  if (g_class_registered) return true;
  WNDCLASSEXW wc{};
  wc.cbSize = sizeof(wc);
  wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
  wc.lpfnWndProc = &CandidateWindow::WndProc;
  wc.hInstance = GetModuleHandleW(nullptr);
  wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
  wc.hbrBackground = reinterpret_cast<HBRUSH>(GetStockObject(WHITE_BRUSH));
  wc.lpszClassName = kClassName;
  if (!RegisterClassExW(&wc)) {
    if (GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return false;
  }
  g_class_registered = true;
  return true;
}

CandidateWindow::~CandidateWindow() { Destroy(); }

void CandidateWindow::Destroy() {
  if (hwnd_) {
    SetWindowLongPtrW(hwnd_, GWLP_USERDATA, 0);
    DestroyWindow(hwnd_);
    hwnd_ = nullptr;
  }
  if (font_) {
    DeleteObject(font_);
    font_ = nullptr;
  }
  items_.clear();
}

bool CandidateWindow::Create() {
  if (hwnd_) return true;
  if (!EnsureClassRegistered()) return false;
  hwnd_ = CreateWindowExW(
      WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, kClassName, L"",
      WS_POPUP, 0, 0, 10, 10, nullptr, nullptr,
      GetModuleHandleW(nullptr), nullptr);
  if (!hwnd_) return false;
  SetWindowLongPtrW(hwnd_, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(this));
  return true;
}

LRESULT CALLBACK CandidateWindow::WndProc(HWND hwnd, UINT msg, WPARAM wp,
                                         LPARAM lp) {
  auto* self = reinterpret_cast<CandidateWindow*>(
      GetWindowLongPtrW(hwnd, GWLP_USERDATA));
  if (msg == WM_NCCREATE) {
    auto* cs = reinterpret_cast<CREATESTRUCTW*>(lp);
    SetWindowLongPtrW(hwnd, GWLP_USERDATA,
                      reinterpret_cast<LONG_PTR>(cs->lpCreateParams));
    return DefWindowProcW(hwnd, msg, wp, lp);
  }
  if (self) return self->OnMessage(msg, wp, lp);
  return DefWindowProcW(hwnd, msg, wp, lp);
}

LRESULT CandidateWindow::OnMessage(UINT msg, WPARAM wp, LPARAM lp) {
  switch (msg) {
    case WM_PAINT:
      OnPaint();
      return 0;
    case WM_LBUTTONDOWN: {
      int idx = HitTest(static_cast<int>(static_cast<short>(LOWORD(lp))));
      if (idx >= 0 && on_select_) on_select_(static_cast<size_t>(idx));
      return 0;
    }
    case WM_ERASEBKGND:
      return 1;  // 双缓冲自绘，避免闪烁（简单起见直接禁擦除）
    default:
      return DefWindowProcW(hwnd_, msg, wp, lp);
  }
}

void CandidateWindow::RebuildLayout(HDC dc) {
  item_left_.clear();
  item_right_.clear();
  if (!font_) {
    // 消息字体（SPI_GETNONCLIENTMETRICS）
    NONCLIENTMETRICSW ncm{};
    ncm.cbSize = sizeof(ncm);
    if (SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, sizeof(ncm), &ncm, 0)) {
      font_ = CreateFontIndirectW(&ncm.lfMessageFont);
    }
  }
  HGDIOBJ old = dc ? SelectObject(dc, font_ ? font_ : GetStockObject(
                                                    DEFAULT_GUI_FONT))
                   : nullptr;
  int x = Scale(kItemPadding);
  for (size_t i = 0; i < items_.size(); ++i) {
    std::wstring label = std::to_wstring(i + 1) + L". " + items_[i].text;
    if (!items_[i].comment.empty()) {
      label += L"(" + items_[i].comment + L")";
    }
    SIZE sz{};
    GetTextExtentPoint32W(dc, label.c_str(), static_cast<int>(label.size()),
                          &sz);
    item_left_.push_back(x);
    item_right_.push_back(x + sz.cx + Scale(kItemPadding));
    x = item_right_.back() + Scale(kItemPadding);
  }
  // 页码标签 "n/m"
  std::wstring page = L"[" + std::to_wstring(page_no_ + 1) + L"/" +
                      std::to_wstring(is_last_page_ ? page_no_ + 1 : 0) + L"]";
  SIZE psz{};
  GetTextExtentPoint32W(dc, page.c_str(), static_cast<int>(page.size()), &psz);
  page_label_left_ = x;
  width_ = x + psz.cx + Scale(kItemPadding);
  TEXTMETRICW tm{};
  GetTextMetricsW(dc, &tm);
  height_ = tm.tmHeight + 2 * Scale(kItemPadding);
  if (old) SelectObject(dc, old);
}

void CandidateWindow::Show(const std::vector<CandidateItem>& items,
                           int page_no, bool is_last_page, int highlighted,
                           POINT caret) {
  if (!Create()) return;
  items_ = items;
  page_no_ = page_no;
  is_last_page_ = is_last_page;
  highlighted_ = highlighted;
  HDC dc = GetDC(hwnd_);
  RebuildLayout(dc);
  ReleaseDC(hwnd_, dc);
  if (width_ <= 0 || height_ <= 0) return;
  // 光标下方跟随；贴近屏幕底部时翻到上方
  int y = caret.y + Scale(20);
  int screen_h = GetSystemMetrics(SM_CYSCREEN);
  if (y + height_ > screen_h) y = caret.y - height_ - Scale(4);
  SetWindowPos(hwnd_, HWND_TOPMOST, caret.x, y, width_, height_,
               SWP_NOACTIVATE | SWP_SHOWWINDOW);
  InvalidateRect(hwnd_, nullptr, TRUE);
}

void CandidateWindow::Hide() {
  if (hwnd_) ShowWindow(hwnd_, SW_HIDE);
  items_.clear();
}

int CandidateWindow::HitTest(int x) const {
  for (size_t i = 0; i < item_left_.size(); ++i) {
    if (x >= item_left_[i] && x < item_right_[i]) return static_cast<int>(i);
  }
  return -1;
}

void CandidateWindow::OnPaint() {
  PAINTSTRUCT ps{};
  HDC dc = BeginPaint(hwnd_, &ps);
  HGDIOBJ old = SelectObject(dc, font_ ? font_ : GetStockObject(
                                                    DEFAULT_GUI_FONT));
  RECT rc{};
  GetClientRect(hwnd_, &rc);
  FillRect(dc, &rc, reinterpret_cast<HBRUSH>(GetStockObject(WHITE_BRUSH)));
  SetBkMode(dc, TRANSPARENT);
  for (size_t i = 0; i < items_.size(); ++i) {
    std::wstring label = std::to_wstring(i + 1) + L". " + items_[i].text;
    if (!items_[i].comment.empty()) {
      label += L"(" + items_[i].comment + L")";
    }
    RECT item_rc{item_left_[i], Scale(kItemPadding), item_right_[i],
                 height_ - Scale(kItemPadding)};
    if (static_cast<int>(i) == highlighted_) {
      FillRect(dc, &item_rc,
               reinterpret_cast<HBRUSH>(GetStockObject(LTGRAY_BRUSH)));
    }
    DrawTextW(dc, label.c_str(), static_cast<int>(label.size()), &item_rc,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
  }
  // 页码
  std::wstring page = L"[" + std::to_wstring(page_no_ + 1) + L"/" +
                      std::to_wstring(is_last_page_ ? page_no_ + 1 : 0) + L"]";
  RECT page_rc{page_label_left_, Scale(kItemPadding), width_,
               height_ - Scale(kItemPadding)};
  DrawTextW(dc, page.c_str(), static_cast<int>(page.size()), &page_rc,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
  SelectObject(dc, old);
  EndPaint(hwnd_, &ps);
}
