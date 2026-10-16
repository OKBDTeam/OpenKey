// OpenKeyServer 入口：单实例常驻进程 + 命名管道服务循环（docs/02 4.1.4）。
//
// 线程模型：
//   - 主线程：创建命名管道并循环 Accept；每来一个连接起一个工作线程；
//   - 工作线程：读一行请求 → 经 pipe::kMethodTable 分派 → 写一行响应；
//   - 引擎状态由 EngineHost 内部互斥保护（键处理串行化，v0.1.0 策略）。
// 生命周期：单实例互斥体 Global\OpenKeyServer；控制台 Ctrl 事件优雅退出。

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include <cstdio>
#include <string>
#include <vector>

#include "engine_host.h"
#include "../OpenKeyCoreClient/pipe_protocol.h"

namespace {

constexpr DWORD kPipeBufferBytes = 64 * 1024;
constexpr int kMaxPipeInstances = 8;

HANDLE g_stop_event = nullptr;

BOOL WINAPI OnConsoleCtrl(DWORD ctrl_type) {
  switch (ctrl_type) {
    case CTRL_C_EVENT:
    case CTRL_BREAK_EVENT:
    case CTRL_CLOSE_EVENT:
    case CTRL_SHUTDOWN_EVENT:
      if (g_stop_event) {
        SetEvent(g_stop_event);
      }
      // 给管道循环一点时间清理后由进程退出；此处返回 TRUE 表示已处理。
      return TRUE;
    default:
      return FALSE;
  }
}

// 从管道读取一行（以 '\n' 结尾，消息模式下一次 Read 即一行）。
bool ReadLine(HANDLE pipe, std::string* line) {
  line->clear();
  char buf[4096];
  while (true) {
    DWORD read = 0;
    if (!ReadFile(pipe, buf, sizeof(buf), &read, nullptr)) {
      DWORD err = GetLastError();
      if (err == ERROR_BROKEN_PIPE || err == ERROR_PIPE_NOT_CONNECTED) {
        return false;  // 客户端断开
      }
      if (err == ERROR_MORE_DATA) {
        line->append(buf, read);
        continue;
      }
      return false;
    }
    if (read == 0) {
      return false;
    }
    line->append(buf, read);
    if (line->find('\n') != std::string::npos) {
      // 截到第一个换行（消息模式下通常一次即完整一行）。
      line->resize(line->find('\n'));
      return true;
    }
    if (line->size() > openkey::pipe::kMaxLineBytes) {
      return false;  // 超长行视为协议错误，断开
    }
  }
}

bool WriteAll(HANDLE pipe, const std::string& data) {
  size_t sent = 0;
  while (sent < data.size()) {
    DWORD written = 0;
    if (!WriteFile(pipe, data.data() + sent, static_cast<DWORD>(data.size() - sent),
                   &written, nullptr)) {
      return false;
    }
    sent += written;
  }
  return true;
}

// 分发一行请求；返回响应行（不含换行）。
std::string Dispatch(openkey::EngineHost* host, const std::string& line) {
  std::string first;
  openkey::pipe::FieldMap req;
  if (!openkey::pipe::ParseLine(line, &first, &req)) {
    return openkey::pipe::BuildResponseLine(false, {}, 100, "bad request line");
  }
  for (size_t i = 0; i < openkey::pipe::kMethodTableSize; ++i) {
    if (first == openkey::pipe::kMethodTable[i].name) {
      openkey::pipe::FieldMap resp;
      if (openkey::pipe::kMethodTable[i].handler(host, req, &resp)) {
        return openkey::pipe::BuildResponseLine(true, resp, 0, "");
      }
      std::string err;
      openkey::pipe::GetField(resp, "error", &err);
      return openkey::pipe::BuildResponseLine(false, resp, 200, err);
    }
  }
  return openkey::pipe::BuildResponseLine(false, {}, 404, "unknown method");
}

DWORD WINAPI ConnectionThread(LPVOID param) {
  HANDLE pipe = static_cast<HANDLE>(param);
  openkey::EngineHost* host = &openkey::EngineHost::Instance();
  std::string line;
  while (ReadLine(pipe, &line)) {
    if (line.empty()) {
      continue;
    }
    std::string response = Dispatch(host, line);
    response.push_back('\n');
    if (!WriteAll(pipe, response)) {
      break;
    }
  }
  FlushFileBuffers(pipe);
  DisconnectNamedPipe(pipe);
  CloseHandle(pipe);
  return 0;
}

}  // namespace

/* 入口用 wWinMain：CMake 以 WIN32 子系统生成（无控制台窗口），此时链接器
 * 需要 WinMain/wWinMain 而非 main（曾报 LNK2019 unresolved WinMain）。 */
int WINAPI wWinMain(HINSTANCE instance, HINSTANCE prev_instance,
                    LPWSTR cmd_line, int cmd_show) {
  (void)instance;
  (void)prev_instance;
  (void)cmd_line;
  (void)cmd_show;
  // 单实例：已运行则静默退出（TSF 客户端会依赖唯一 Server）。
  HANDLE instance_mutex =
      CreateMutexW(nullptr, TRUE, L"Global\\OpenKeyServer");
  if (!instance_mutex) {
    std::fprintf(stderr, "CreateMutex 失败: %lu\n", GetLastError());
    return 1;
  }
  if (GetLastError() == ERROR_ALREADY_EXISTS) {
    std::fprintf(stderr, "OpenKeyServer 已在运行。\n");
    CloseHandle(instance_mutex);
    return 0;
  }

  openkey::EngineHost& host = openkey::EngineHost::Instance();
  if (!host.Init()) {
    std::fprintf(stderr, "引擎初始化失败: %s\n", host.init_error().c_str());
    // 引擎不可用时仍然常驻：TSF 需要管道存在以查询状态/触发修复；
    // 输入方法调用会以错误响应返回（见 EngineHost::last_error）。
  }

  g_stop_event = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  SetConsoleCtrlHandler(OnConsoleCtrl, TRUE);

  // 创建首个管道实例。
  HANDLE pipe = CreateNamedPipeW(
      openkey::pipe::kPipeName,
      PIPE_ACCESS_DUPLEX,
      PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
      kMaxPipeInstances,
      kPipeBufferBytes,
      kPipeBufferBytes,
      openkey::pipe::kDefaultTimeoutMs,
      nullptr);
  if (pipe == INVALID_HANDLE_VALUE) {
    std::fprintf(stderr, "CreateNamedPipe 失败: %lu\n", GetLastError());
    host.Shutdown();
    return 1;
  }

  std::printf("OpenKeyServer 就绪（%ls）。\n", openkey::pipe::kPipeName);

  while (WaitForSingleObject(g_stop_event, 0) == WAIT_TIMEOUT) {
    if (!ConnectNamedPipe(pipe, nullptr)) {
      DWORD err = GetLastError();
      if (err == ERROR_PIPE_CONNECTED) {
        // 客户端在 ConnectNamedPipe 返回前已连接，正常路径。
      } else if (err == ERROR_NO_DATA) {
        // 客户端连接后立即断开，继续等待下一个。
        DisconnectNamedPipe(pipe);
        continue;
      } else {
        std::fprintf(stderr, "ConnectNamedPipe 失败: %lu\n", err);
        break;
      }
    }
    HANDLE thread = CreateThread(nullptr, 0, ConnectionThread, pipe, 0, nullptr);
    if (!thread) {
      std::fprintf(stderr, "CreateThread 失败: %lu\n", GetLastError());
      DisconnectNamedPipe(pipe);
      CloseHandle(pipe);
      break;
    }
    CloseHandle(thread);  // 线程自行管理生命周期；句柄仅用于等待结束。
    // 为下一个连接创建新实例。
    pipe = CreateNamedPipeW(
        openkey::pipe::kPipeName,
        PIPE_ACCESS_DUPLEX,
        PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
        kMaxPipeInstances,
        kPipeBufferBytes,
        kPipeBufferBytes,
        openkey::pipe::kDefaultTimeoutMs,
        nullptr);
    if (pipe == INVALID_HANDLE_VALUE) {
      std::fprintf(stderr, "CreateNamedPipe 失败: %lu\n", GetLastError());
      break;
    }
  }

  // 退出清理（工作线程为分离状态；消息模式管道断开即结束）。
  host.Shutdown();
  if (instance_mutex) {
    ReleaseMutex(instance_mutex);
    CloseHandle(instance_mutex);
  }
  if (g_stop_event) {
    CloseHandle(g_stop_event);
  }
  std::printf("OpenKeyServer 已退出。\n");
  return 0;
}
