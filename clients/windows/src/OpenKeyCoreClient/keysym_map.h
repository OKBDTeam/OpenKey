/*
 * 文件用途：Windows 虚拟键码（VK）到 X11 keysym 的映射，以及 Win32
 * 修饰键状态到 X11 mask 的换算。core 的 ok_session_process_key 只认
 * X11 keysym + mask（与 librime 全端一致，见 openkey.h 注释）。
 *
 * 布局说明：静态表按美式键盘（US-QWERTY）给出带/不带 Shift 的两个
 * 取值。非美式布局下 OEM 键位的实际字符不同，一期用静态表 +
 * TODO（见 keysym_map.cpp 头部注释），后续以 ToUnicodeEx 精确化。
 */

#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include <cstdint>

namespace openkey {

/* X11 修饰键 mask（与 openkey.h 中 ok_session_process_key 的约定一致）。 */
inline constexpr uint32_t kMaskShift = 1;
inline constexpr uint32_t kMaskCaps = 2;
inline constexpr uint32_t kMaskControl = 4;
inline constexpr uint32_t kMaskAlt = 8;  // Mod1

/* X11 keysym 特殊键（0xFF00 段）。 */
inline constexpr uint32_t kKsBackSpace = 0xFF08;
inline constexpr uint32_t kKsTab = 0xFF09;
inline constexpr uint32_t kKsReturn = 0xFF0D;
inline constexpr uint32_t kKsEscape = 0xFF1B;
inline constexpr uint32_t kKsDelete = 0xFFFF;
inline constexpr uint32_t kKsHome = 0xFF50;
inline constexpr uint32_t kKsLeft = 0xFF51;
inline constexpr uint32_t kKsUp = 0xFF52;
inline constexpr uint32_t kKsRight = 0xFF53;
inline constexpr uint32_t kKsDown = 0xFF54;
inline constexpr uint32_t kKsPageUp = 0xFF55;
inline constexpr uint32_t kKsPageDown = 0xFF56;
inline constexpr uint32_t kKsEnd = 0xFF57;
inline constexpr uint32_t kKsInsert = 0xFF63;
inline constexpr uint32_t kKsF1 = 0xFFBE;  // F1..F24 依次递增

/* VK -> keysym（不按 Shift 的基准值）。
 * 无法映射（纯修饰键、未知键）返回 0。 */
uint32_t VkToKeysym(UINT vk);

/* VK -> keysym（按下 Shift 时的值，仅对可打印键有效；
 * 非打印键与 VkToKeysym 相同）。返回 0 表示无法映射。 */
uint32_t VkToKeysymShifted(UINT vk);

/* 综合入口：TSF OnKeyDown 拿到 (vk, shift_down) 后调用。
 * 返回 0 表示该键应放行给系统（如纯修饰键）。 */
uint32_t VkToKeysymForEvent(UINT vk, bool shift_down);

/* 由当前线程键盘状态换算 X11 mask。
 * 约定：Shift=1, Caps=2, Control=4, Alt=8（与 openkey.h 一致）。 */
uint32_t CurrentKeyMask();

}  // namespace openkey
