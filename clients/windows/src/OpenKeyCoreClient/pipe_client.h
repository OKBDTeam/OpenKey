/*
 * 文件用途：Windows 命名管道客户端（\\.\pipe\openkey）的 C++ 薄封装。
 *
 * 设计取舍：
 *   - 采用「每连接一线程」的同步服务端模型，客户端用最稳定的
 *     CreateFile + WriteFile/ReadFile 字节流管道（非消息模式），
 *     自己按 '\n' 切行；不引入重叠 IO / IOCP，降低首批代码风险。
 *   - 断线不自动重连：transact() 失败抛出异常，由调用方（TSF 侧）
 *     决定是否重建连接并重试一次。这样错误路径对调用方可见。
 */

#pragma once

#include <stdexcept>
#include <string>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include "pipe_protocol.h"

namespace openkey {

/* 管道通信错误（含 Win32 错误码）。 */
class PipeError : public std::runtime_error {
 public:
  PipeError(const std::string& what, DWORD win32_error)
      : std::runtime_error(what), win32_error_(win32_error) {}
  DWORD win32_error() const { return win32_error_; }

 private:
  DWORD win32_error_;
};

/* 命名管道客户端：非拷贝、可移动。 */
class PipeClient {
 public:
  PipeClient() = default;
  ~PipeClient();

  PipeClient(const PipeClient&) = delete;
  PipeClient& operator=(const PipeClient&) = delete;
  PipeClient(PipeClient&& other) noexcept;
  PipeClient& operator=(PipeClient&& other) noexcept;

  /* 连接服务端管道；失败抛 PipeError。wait_ms 为等待超时。 */
  void Connect(DWORD wait_ms = pipe::kDefaultTimeoutMs);

  /* 关闭连接（幂等）。 */
  void Disconnect();

  bool IsConnected() const { return handle_ != INVALID_HANDLE_VALUE; }

  /* 请求-响应事务：发送一行请求，读取一行响应。
   * ok 为 false 或通信失败时抛 PipeError；resp 为解码后的字段表。 */
  void Transact(const std::string& request_line, DWORD timeout_ms,
                bool* ok, pipe::FieldMap* resp);

 private:
  /* 读取一行（至 '\n' 为止，不含换行符）。 */
  std::string ReadLine(DWORD timeout_ms);

  HANDLE handle_ = INVALID_HANDLE_VALUE;
  std::string read_buffer_;  // 已收到但未消费的字节
};

/* 高层封装：连接 + 发送方法请求，解析响应。连接不存在时先尝试连接。
 * 返回响应字段表；ok=false（服务端 ERR）时不抛异常，由调用方判读。 */
pipe::FieldMap Request(const char* method, const pipe::FieldMap& req,
                       DWORD timeout_ms = pipe::kDefaultTimeoutMs);

}  // namespace openkey
