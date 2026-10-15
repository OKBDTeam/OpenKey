/*
 * 文件用途：update_check.h 的实现（WinHTTP GET + 版本比较 + 提示弹窗）。
 */

#include "update_check.h"

#include <windows.h>

#include <winhttp.h>

#include <vector>

#include "openkey_client.h"  // ok_version_string

#pragma comment(lib, "winhttp.lib")

namespace openkey {
namespace {

/* TODO: 仓库归属待官方组织确定后集中到配置（与 release 流水线保持一致）。 */
constexpr wchar_t kLatestJsonUrl[] =
    L"https://github.com/openkey-im/openkey/releases/latest/download/"
    L"latest.json";

/* 同步 HTTPS GET；成功返回响应体。 */
bool HttpGetText(const wchar_t* url, std::string* out) {
  URL_COMPONENTS comp{};
  comp.dwStructSize = sizeof(comp);
  wchar_t host[256] = {};
  wchar_t path[2048] = {};
  comp.lpszHostName = host;
  comp.dwHostNameLength = _countof(host);
  comp.lpszUrlPath = path;
  comp.dwUrlPathLength = _countof(path);
  if (!WinHttpCrackUrl(url, 0, 0, &comp)) return false;

  HINTERNET session = WinHttpOpen(L"OpenKeyServer/1.0",
                                  WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                                  WINHTTP_NO_PROXY_NAME,
                                  WINHTTP_NO_PROXY_BYPASS, 0);
  if (!session) return false;
  HINTERNET connect = WinHttpConnect(session, host,
                                     comp.nPort ? comp.nPort
                                                : INTERNET_DEFAULT_HTTPS_PORT,
                                     0);
  if (!connect) {
    WinHttpCloseHandle(session);
    return false;
  }
  HINTERNET request = WinHttpOpenRequest(
      connect, L"GET", path, nullptr, WINHTTP_NO_REFERER,
      WINHTTP_DEFAULT_ACCEPT_TYPES,
      comp.nScheme == INTERNET_SCHEME_HTTPS ? WINHTTP_FLAG_SECURE : 0);
  if (!request) {
    WinHttpCloseHandle(connect);
    WinHttpCloseHandle(session);
    return false;
  }
  BOOL ok = WinHttpSendRequest(request, WINHTTP_NO_ADDITIONAL_HEADERS, 0,
                               WINHTTP_NO_REQUEST_DATA, 0, 0, 0) &&
            WinHttpReceiveResponse(request, nullptr);
  while (ok) {
    DWORD avail = 0;
    if (!WinHttpQueryDataAvailable(request, &avail)) break;
    if (avail == 0) break;
    std::vector<char> chunk(avail);
    DWORD got = 0;
    if (!WinHttpReadData(request, chunk.data(), avail, &got)) break;
    out->append(chunk.data(), got);
  }
  WinHttpCloseHandle(request);
  WinHttpCloseHandle(connect);
  WinHttpCloseHandle(session);
  return ok;
}

/* 从 JSON 文本中提取字符串字段值（最简实现：找 "key" : "value"）。 */
bool ExtractJsonString(const std::string& json, const char* key,
                       std::string* out) {
  std::string needle = std::string("\"") + key + "\"";
  size_t p = json.find(needle);
  if (p == std::string::npos) return false;
  p = json.find(':', p + needle.size());
  if (p == std::string::npos) return false;
  p = json.find('"', p + 1);
  if (p == std::string::npos) return false;
  size_t end = json.find('"', p + 1);
  if (end == std::string::npos) return false;
  *out = json.substr(p + 1, end - p - 1);
  return !out->empty();
}

std::vector<int> ParseVersion(const std::string& v) {
  std::vector<int> parts;
  size_t start = 0;
  while (start <= v.size()) {
    size_t dot = v.find('.', start);
    std::string token = v.substr(start, dot == std::string::npos
                                            ? std::string::npos
                                            : dot - start);
    // 去掉预发布后缀（如 "1.2.3-beta" 的 "-beta"）
    size_t dash = token.find('-');
    if (dash != std::string::npos) token = token.substr(0, dash);
    parts.push_back(atoi(token.c_str()));
    if (dot == std::string::npos) break;
    start = dot + 1;
  }
  while (parts.size() < 3) parts.push_back(0);
  return parts;
}

/* latest > current 返回 true（语义化版本三段比较；预发布忽略）。 */
bool IsNewer(const std::string& current, const std::string& latest) {
  auto a = ParseVersion(current);
  auto b = ParseVersion(latest);
  for (size_t i = 0; i < 3; ++i) {
    if (b[i] != a[i]) return b[i] > a[i];
  }
  return false;
}

}  // namespace

bool CheckForUpdate(std::string* latest_version, std::string* download_url) {
  std::string body;
  if (!HttpGetText(kLatestJsonUrl, &body)) return false;
  std::string version;
  if (!ExtractJsonString(body, "version", &version)) return false;
  std::string url;
  ExtractJsonString(body, "url", &url);  // 缺失时用默认下载页
  if (url.empty()) url = "https://github.com/openkey-im/openkey/releases";
  std::string current = VersionString();
  if (!IsNewer(current, version)) return false;
  if (latest_version) *latest_version = version;
  if (download_url) *download_url = url;
  return true;
}

void PromptUpdateIfAvailable(HWND owner) {
  std::string version;
  std::string url;
  if (!CheckForUpdate(&version, &url)) return;
  // 仅提示手动下载（一期不自更新，docs/02 4.1.5）
  std::wstring text = L"发现新版本 " + std::wstring(version.begin(), version.end()) +
                      L"\n请前往下载页手动更新：\n" +
                      std::wstring(url.begin(), url.end());
  MessageBoxW(owner, text.c_str(), L"OpenKey 更新提示", MB_OK | MB_ICONINFORMATION);
}

}  // namespace openkey
