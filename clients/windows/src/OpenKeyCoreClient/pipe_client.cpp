/*
 * 文件用途：命名管道客户端实现（CreateFile/WriteFile/ReadFile，字节流模式）。
 */

#include "pipe_client.h"

#include <cstring>

namespace openkey {

PipeClient::~PipeClient() { Disconnect(); }

PipeClient::PipeClient(PipeClient&& other) noexcept
    : handle_(other.handle_), read_buffer_(std::move(other.read_buffer_)) {
  other.handle_ = INVALID_HANDLE_VALUE;
}

PipeClient& PipeClient::operator=(PipeClient&& other) noexcept {
  if (this != &other) {
    Disconnect();
    handle_ = other.handle_;
    read_buffer_ = std::move(other.read_buffer_);
    other.handle_ = INVALID_HANDLE_VALUE;
  }
  return *this;
}

void PipeClient::Connect(DWORD wait_ms) {
  Disconnect();
  // 等待管道实例就绪（服务端未启动时快速失败，不挂起宿主线程）。
  if (!WaitNamedPipeW(pipe::kPipeName, wait_ms)) {
    // 服务端不在： ERROR_SEM_TIMEOUT / ERROR_FILE_NOT_FOUND 均按此上报
    throw PipeError("WaitNamedPipe failed: server not available",
                    GetLastError());
  }
  HANDLE h = CreateFileW(pipe::kPipeName, GENERIC_READ | GENERIC_WRITE,
                         0,            // 不共享
                         nullptr,      // 默认安全属性
                         OPEN_EXISTING,
                         0,            // 字节流模式，同步 IO
                         nullptr);
  if (h == INVALID_HANDLE_VALUE) {
    throw PipeError("CreateFile(pipe) failed", GetLastError());
  }
  // 客户端侧设为字节读模式（服务端创建时已指定，双保险）。
  DWORD mode = PIPE_READMODE_BYTE | PIPE_WAIT;
  SetNamedPipeHandleState(h, &mode, nullptr, nullptr);
  handle_ = h;
  read_buffer_.clear();
}

void PipeClient::Disconnect() {
  if (handle_ != INVALID_HANDLE_VALUE) {
    FlushFileBuffers(handle_);
    CloseHandle(handle_);
    handle_ = INVALID_HANDLE_VALUE;
  }
  read_buffer_.clear();
}

std::string PipeClient::ReadLine(DWORD timeout_ms) {
  std::string line;
  for (;;) {
    // 先在缓冲区内找换行
    size_t nl = read_buffer_.find('\n');
    if (nl != std::string::npos) {
      line = read_buffer_.substr(0, nl);
      read_buffer_.erase(0, nl + 1);
      if (!line.empty() && line.back() == '\r') line.pop_back();
      return line;
    }
    if (read_buffer_.size() > pipe::kMaxLineBytes) {
      throw PipeError("response line too long", 0);
    }
    char chunk[4096];
    DWORD got = 0;
    // 同步 ReadFile 无超时参数；用 PeekNamedPipe + 少量轮询实现上界，
    // 保证管道异常挂起时不会永久阻塞调用线程（TSF 侧尤其重要）。
    DWORD avail = 0;
    ULONGLONG deadline = GetTickCount64() + timeout_ms;
    for (;;) {
      if (!PeekNamedPipe(handle_, nullptr, 0, nullptr, &avail, nullptr)) {
        throw PipeError("PeekNamedPipe failed (broken pipe)", GetLastError());
      }
      if (avail > 0) break;
      if (GetTickCount64() >= deadline) {
        throw PipeError("read timeout", 0);
      }
      Sleep(2);
    }
    if (!ReadFile(handle_, chunk, sizeof(chunk), &got, nullptr) || got == 0) {
      throw PipeError("ReadFile failed", GetLastError());
    }
    read_buffer_.append(chunk, got);
  }
}

void PipeClient::Transact(const std::string& request_line, DWORD timeout_ms,
                          bool* ok, pipe::FieldMap* resp) {
  if (!IsConnected()) {
    throw PipeError("pipe not connected", 0);
  }
  std::string wire = request_line;
  wire.push_back('\n');
  DWORD written = 0;
  if (!WriteFile(handle_, wire.data(), static_cast<DWORD>(wire.size()),
                 &written, nullptr) ||
      written != wire.size()) {
    DWORD err = GetLastError();
    Disconnect();
    throw PipeError("WriteFile failed", err);
  }
  std::string line = ReadLine(timeout_ms);
  std::string token;
  if (!pipe::ParseLine(line, &token, resp)) {
    throw PipeError("malformed response line", 0);
  }
  if (token == "OK") {
    *ok = true;
  } else if (token == "ERR") {
    *ok = false;
  } else {
    throw PipeError("unexpected response token: " + token, 0);
  }
}

namespace {

/* 进程内互斥体：保护共享管道连接（TSF 侧多个编辑线程可能并发请求）。 */
class Mutex {
 public:
  Mutex() { InitializeCriticalSection(&cs_); }
  ~Mutex() { DeleteCriticalSection(&cs_); }
  void Lock() { EnterCriticalSection(&cs_); }
  void Unlock() { LeaveCriticalSection(&cs_); }

 private:
  CRITICAL_SECTION cs_;
};

Mutex& GlobalMutex() {
  static Mutex m;
  return m;
}

struct MutexGuard {
  ~MutexGuard() { GlobalMutex().Unlock(); }
};

}  // namespace

pipe::FieldMap Request(const char* method, const pipe::FieldMap& req,
                       DWORD timeout_ms) {
  static PipeClient client;  // 进程内单连接；并发经 GlobalMutex 保护
  GlobalMutex().Lock();
  MutexGuard guard;

  if (!client.IsConnected()) {
    // 服务端未运行或已崩溃：抛 PipeError，调用方（TSF）可降级为直连 core
    client.Connect();
  }
  std::string line = pipe::BuildRequestLine(method, req);
  bool ok = false;
  pipe::FieldMap resp;
  client.Transact(line, timeout_ms, &ok, &resp);
  if (!ok) {
    // 服务端返回 ERR：附标记字段，调用方读取 err_code / err_msg
    resp.emplace_back("__error__", "1");
  }
  return resp;
}

}  // namespace openkey
