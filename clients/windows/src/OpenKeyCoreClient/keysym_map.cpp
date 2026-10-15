/*
 * 文件用途：keysym_map.h 的实现（静态查表）。
 *
 * 布局局限（一期已知 TODO）：
 *   - 静态表描述美式键盘；德/法/日等布局下 VK_OEM_* 的实际字符不同，
 *     TSF 侧若需精确支持，应在 OnKeyDown 中用 ToUnicodeEx(键盘布局)
 *     取实际字符再查表（本文件保留 VkToKeysym 作为无布局依赖的基准）。
 *   - 死键（dead key，如美式布局的 `^` 二次击键）一期不处理，
 *     librime 侧目前也不消费组合键序列。
 */

#include "keysym_map.h"

namespace openkey {
namespace {

/* 可打印键的美式布局基准表（不含 Shift）。 */
struct KeyEntry {
  UINT vk;
  uint32_t base;     // 未按 Shift
  uint32_t shifted;  // 按下 Shift
};

const KeyEntry kPrintable[] = {
    {'1', '1', '!'}, {'2', '2', '@'}, {'3', '3', '#'}, {'4', '4', '$'},
    {'5', '5', '%'}, {'6', '6', '^'}, {'7', '7', '&'}, {'8', '8', '*'},
    {'9', '9', '('}, {'0', '0', ')'},

    {'A', 'a', 'A'}, {'B', 'b', 'B'}, {'C', 'c', 'C'}, {'D', 'd', 'D'},
    {'E', 'e', 'E'}, {'F', 'f', 'F'}, {'G', 'g', 'G'}, {'H', 'h', 'H'},
    {'I', 'i', 'I'}, {'J', 'j', 'J'}, {'K', 'k', 'K'}, {'L', 'l', 'L'},
    {'M', 'm', 'M'}, {'N', 'n', 'N'}, {'O', 'o', 'O'}, {'P', 'p', 'P'},
    {'Q', 'q', 'Q'}, {'R', 'r', 'R'}, {'S', 's', 'S'}, {'T', 't', 'T'},
    {'U', 'u', 'U'}, {'V', 'v', 'V'}, {'W', 'w', 'W'}, {'X', 'x', 'X'},
    {'Y', 'y', 'Y'}, {'Z', 'z', 'Z'},

    {VK_OEM_1, ';', ':'},      // ;:
    {VK_OEM_2, '/', '?'},      // /?
    {VK_OEM_3, '`', '~'},      // `~
    {VK_OEM_4, '[', '{'},      // [{
    {VK_OEM_5, '\\', '|'},     // \|
    {VK_OEM_6, ']', '}'},      // ]}
    {VK_OEM_7, '\'', '"'},     // '"
    {VK_OEM_MINUS, '-', '_'},  // -_
    {VK_OEM_PLUS, '=', '+'},   // =+
    {VK_OEM_COMMA, ',', '<'},  // ,<
    {VK_OEM_PERIOD, '.', '>'}, // .>
    {VK_SPACE, ' ', ' '},      // 空格（keysym 0x20）
};

/* 小键盘数字键（XK_KP_0..KP_9 = 0xFFB0..0xFFB9）。 */
const KeyEntry kKeypad[] = {
    {VK_NUMPAD0, 0xFFB0, 0xFFB0}, {VK_NUMPAD1, 0xFFB1, 0xFFB1},
    {VK_NUMPAD2, 0xFFB2, 0xFFB2}, {VK_NUMPAD3, 0xFFB3, 0xFFB3},
    {VK_NUMPAD4, 0xFFB4, 0xFFB4}, {VK_NUMPAD5, 0xFFB5, 0xFFB5},
    {VK_NUMPAD6, 0xFFB6, 0xFFB6}, {VK_NUMPAD7, 0xFFB7, 0xFFB7},
    {VK_NUMPAD8, 0xFFB8, 0xFFB8}, {VK_NUMPAD9, 0xFFB9, 0xFFB9},
    {VK_DECIMAL, 0xFFAE, 0xFFAE},   // KP_Decimal
    {VK_DIVIDE, 0xFFAF, 0xFFAF},    // KP_Divide
    {VK_MULTIPLY, 0xFFAA, 0xFFAA},  // KP_Multiply
    {VK_SUBTRACT, 0xFFAD, 0xFFAD},  // KP_Subtract
    {VK_ADD, 0xFFAB, 0xFFAB},       // KP_Add
};

const KeyEntry* FindEntry(UINT vk, const KeyEntry* table, size_t count) {
  for (size_t i = 0; i < count; ++i) {
    if (table[i].vk == vk) return &table[i];
  }
  return nullptr;
}

/* 功能键/编辑键 VK -> keysym（Shift 无影响）。 */
bool FunctionalKeysym(UINT vk, uint32_t* out) {
  switch (vk) {
    case VK_BACK: *out = kKsBackSpace; return true;
    case VK_TAB: *out = kKsTab; return true;
    case VK_RETURN: *out = kKsReturn; return true;
    case VK_ESCAPE: *out = kKsEscape; return true;
    case VK_DELETE: *out = kKsDelete; return true;
    case VK_HOME: *out = kKsHome; return true;
    case VK_END: *out = kKsEnd; return true;
    case VK_INSERT: *out = kKsInsert; return true;
    case VK_PRIOR: *out = kKsPageUp; return true;
    case VK_NEXT: *out = kKsPageDown; return true;
    case VK_LEFT: *out = kKsLeft; return true;
    case VK_UP: *out = kKsUp; return true;
    case VK_RIGHT: *out = kKsRight; return true;
    case VK_DOWN: *out = kKsDown; return true;
    default: break;
  }
  if (vk >= VK_F1 && vk <= VK_F24) {
    *out = kKsF1 + (vk - VK_F1);  // XK_F1..XK_F24 连续
    return true;
  }
  return false;
}

}  // namespace

uint32_t VkToKeysym(UINT vk) {
  uint32_t ks = 0;
  if (FunctionalKeysym(vk, &ks)) return ks;
  if (const KeyEntry* e = FindEntry(vk, kPrintable,
                                    sizeof(kPrintable) / sizeof(kPrintable[0]))) {
    return e->base;
  }
  if (const KeyEntry* e =
          FindEntry(vk, kKeypad, sizeof(kKeypad) / sizeof(kKeypad[0]))) {
    return e->base;
  }
  return 0;
}

uint32_t VkToKeysymShifted(UINT vk) {
  if (const KeyEntry* e = FindEntry(vk, kPrintable,
                                    sizeof(kPrintable) / sizeof(kPrintable[0]))) {
    return e->shifted;
  }
  if (const KeyEntry* e =
          FindEntry(vk, kKeypad, sizeof(kKeypad) / sizeof(kKeypad[0]))) {
    return e->shifted;
  }
  return VkToKeysym(vk);  // 功能键 Shift 无影响
}

uint32_t VkToKeysymForEvent(UINT vk, bool shift_down) {
  if (vk == VK_SHIFT || vk == VK_CONTROL || vk == VK_MENU || vk == VK_CAPITAL ||
      vk == VK_LWIN || vk == VK_RWIN || vk == VK_NUMLOCK ||
      vk == VK_SCROLL) {
    return 0;  // 纯修饰键放行
  }
  return shift_down ? VkToKeysymShifted(vk) : VkToKeysym(vk);
}

uint32_t CurrentKeyMask() {
  uint32_t mask = 0;
  if (GetKeyState(VK_SHIFT) & 0x8000) mask |= kMaskShift;
  if (GetKeyState(VK_CAPITAL) & 0x0001) mask |= kMaskCaps;
  if (GetKeyState(VK_CONTROL) & 0x8000) mask |= kMaskControl;
  if (GetKeyState(VK_MENU) & 0x8000) mask |= kMaskAlt;
  return mask;
}

}  // namespace openkey
