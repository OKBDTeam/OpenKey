/*
 * 文件用途：每日检查 GitHub Releases 的 latest.json（docs/02 4.1.5）。
 *
 * 一期策略：不自更新。发现新版本仅提示用户手动打开下载页。
 * latest.json 预期字段（由 release 流水线生成）：
 *   {"version": "1.2.3", "url": "https://.../releases/latest"}
 * 解析用最简子串查找，不引入 JSON 库（格式由本方发布流水线控制）。
 */

#pragma once

#include <string>

namespace openkey {

/* 检查更新。有新版本返回 true 并输出版本串与下载页 URL。 */
bool CheckForUpdate(std::string* latest_version, std::string* download_url);

/* 检查更新并在有新版本时弹窗提示（Server 隐藏窗口的定时器调用）。 */
void PromptUpdateIfAvailable(HWND owner);

}  // namespace openkey
