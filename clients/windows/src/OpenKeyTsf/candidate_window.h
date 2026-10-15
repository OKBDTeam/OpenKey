/*
 * 文件用途：TSF 候选窗（GDI 自绘弹出窗口，docs/02 4.1.2）。
 *
 * 形态：横向候选条 + 页码，跟随光标（调用方用 GetGUIThreadInfo 取 caret
 * 位置后 MoveTo），WS_EX_TOPMOST|WS_EX_TOOLWINDOW 不抢焦点；左键点击选择。
 * DPI：进程级 SetProcessDPIAware（见 dllmain.cpp），尺寸按 LOGPIXELSX 缩放。
 *
 * 为什么不用 ITfCandidateListUIElement：一期候选条为独立顶层窗，实现与
 * 调试成本最低；UITF 候选列表涉及 ITfCandidateList 与宿主集成，列入 TODO。
 */

#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include <functional>
#include <string>
#include <vector>

/* 候选条目（宽字符；UTF-8 -> UTF-16 转换见 .cpp）。 */
struct CandidateItem {
  std::wstring text;
  std::wstring comment;  // 可空
};

/* UTF-8 字符串转 UTF-16（无效字节按替换字符处理）。 */
std::wstring Utf8ToWide(const std::string& s);

/* 候选窗：单例式使用（每个 TSF 线程一个实例，随文本服务创建）。 */
class CandidateWindow {
 public:
  using SelectCallback = std::function<void(size_t index)>;

  CandidateWindow() = default;
  ~CandidateWindow();

  CandidateWindow(const CandidateWindow&) = delete;
  CandidateWindow& operator=(const CandidateWindow&) = delete;

  /* 注册窗口类（进程一次，幂等）。 */
  static bool EnsureClassRegistered();

  /* 创建底层窗口（惰性；失败返回 false，后续调用均安全）。 */
  bool Create();

  void Destroy();

  /* 展示/刷新候选。caret 为光标屏幕坐标；highlighted 为高亮下标。 */
  void Show(const std::vector<CandidateItem>& items, int page_no,
            bool is_last_page, int highlighted, POINT caret);

  /* 隐藏候选窗。 */
  void Hide();

  bool IsVisible() const { return hwnd_ != nullptr && IsWindowVisible(hwnd_); }

  void SetSelectCallback(SelectCallback cb) { on_select_ = std::move(cb); }

 private:
  static LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp);
  LRESULT OnMessage(UINT msg, WPARAM wp, LPARAM lp);
  void OnPaint();
  int HitTest(int x) const;  // 客户区 x -> 候选下标，未命中返回 -1
  void RebuildLayout(HDC dc);

  HWND hwnd_ = nullptr;
  std::vector<CandidateItem> items_;
  std::vector<int> item_left_;   // 每项左边界（客户区坐标）
  std::vector<int> item_right_;  // 每项右边界
  int page_label_left_ = 0;
  int highlighted_ = 0;
  int page_no_ = 0;
  bool is_last_page_ = true;
  int width_ = 0;
  int height_ = 0;
  HFONT font_ = nullptr;
  SelectCallback on_select_;

  /* 窗口实例数据槽（GWLP_USERDATA 指回本对象）。 */
  static constexpr int kItemPadding = 6;
};
