/*
 * 文件用途：OpenKey TSF DLL 与 OpenKeyServer 之间的命名管道协议定义。
 *
 * 协议为紧凑文本行协议：
 *   请求： "METHOD key=value key=value ...\n"
 *   响应： "OK key=value ...\n" 或 "ERR <code> <message>\n"
 *
 * 为什么不引入第三方 JSON 库：
 *   - TSF DLL 是 in-proc COM 组件，宿主进程（浏览器/Office/游戏等）对加载的
 *     依赖库极度敏感；引入 JSON 库会增加体积与攻击面，且需要额外打包运行时。
 *   - 本协议的取值均为短字符串（预编辑、候选、方案名等），手工编解码足够，
 *     且可逐字段懒解析，无需整树解析。
 *
 * 取值编码规则（仅对 value 生效，key 为固定 ASCII 名无需编码）：
 *   '\\' -> "\\\\"   换行 -> "\\n"   空格 -> "\\s"
 * 解码为上述三条的逆过程。
 */

#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <utility>
#include <vector>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

namespace openkey {
namespace pipe {

/* 命名管道全名（WeaselServer 模式，单实例）。 */
inline constexpr const wchar_t* kPipeName = L"\\\\.\\pipe\\openkey";

/* 单次事务默认超时（毫秒）。 */
inline constexpr DWORD kDefaultTimeoutMs = 5000;

/* 单行最大长度（预编辑 + 候选总量上限，超限判协议错误）。 */
inline constexpr size_t kMaxLineBytes = 64 * 1024;

/* ------------------------------------------------------------------ */
/* 方法名（服务端分发表以此为准；新增方法必须同步 kMethodTable）          */
/* ------------------------------------------------------------------ */
inline constexpr const char* kMethodPing = "Ping";
inline constexpr const char* kMethodVersion = "Version";
inline constexpr const char* kMethodCreateSession = "CreateSession";
inline constexpr const char* kMethodDestroySession = "DestroySession";
inline constexpr const char* kMethodProcessKey = "ProcessKey";
inline constexpr const char* kMethodCommitComposition = "CommitComposition";
inline constexpr const char* kMethodClearComposition = "ClearComposition";
inline constexpr const char* kMethodGetContext = "GetContext";
inline constexpr const char* kMethodGetCommit = "GetCommit";
inline constexpr const char* kMethodGetStatus = "GetStatus";
inline constexpr const char* kMethodSelectCandidate = "SelectCandidate";
inline constexpr const char* kMethodChangePage = "ChangePage";
inline constexpr const char* kMethodSetOption = "SetOption";
inline constexpr const char* kMethodGetOption = "GetOption";
inline constexpr const char* kMethodSelectSchema = "SelectSchema";
inline constexpr const char* kMethodGetSchemaList = "GetSchemaList";
inline constexpr const char* kMethodSyncUserData = "SyncUserData";
inline constexpr const char* kMethodSyncExport = "SyncExport";
inline constexpr const char* kMethodSyncImport = "SyncImport";

/* ------------------------------------------------------------------ */
/* 字段编解码                                                           */
/* ------------------------------------------------------------------ */

/* 原始字符串 -> 协议取值（转义 \\、换行、空格）。 */
std::string EncodeValue(const std::string& raw);

/* 协议取值 -> 原始字符串。无法识别转义时按原样保留。 */
std::string DecodeValue(const std::string& enc);

/* 请求/响应字段表：key 为固定名字，value 已编码。 */
using FieldMap = std::vector<std::pair<std::string, std::string>>;

/* 取字段值（找不到返回 false）。 */
bool GetField(const FieldMap& fields, const char* key, std::string* out);

/* 取整数字段；找不到或格式错误返回 false。 */
bool GetFieldInt(const FieldMap& fields, const char* key, long long* out);

/* 设置/覆盖字段。 */
void SetField(FieldMap* fields, const char* key, const std::string& value);
void SetFieldInt(FieldMap* fields, const char* key, long long value);

/* 构造请求行（不含换行）。 */
std::string BuildRequestLine(const char* method, const FieldMap& fields);

/* 构造响应行（不含换行）。 */
std::string BuildResponseLine(bool ok, const FieldMap& fields,
                              int err_code, const std::string& err_msg);

/* 解析一行请求/响应：
 * - 请求：首 token 为方法名，其余为 key=value；
 * - 响应：首 token 为 "OK" 或 "ERR"；
 * 返回 false 表示行格式非法。 */
bool ParseLine(const std::string& line, std::string* first_token,
               FieldMap* fields);

/* ------------------------------------------------------------------ */
/* 服务端方法分发表                                                     */
/* ------------------------------------------------------------------ */

/* 处理器签名：ctx 为 EngineHost*；req 为入参；resp 为出参（返回 false 时
 * 由调用方填 ERR）。返回 true 表示处理成功。 */
using MethodHandler = bool (*)(void* ctx, const FieldMap& req, FieldMap* resp);

struct MethodEntry {
  const char* name;
  MethodHandler handler;
};

/* 由 OpenKeyServer 的 engine_host.cpp 定义。 */
extern const MethodEntry kMethodTable[];
extern const size_t kMethodTableSize;

}  // namespace pipe
}  // namespace openkey
