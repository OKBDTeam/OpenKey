/*
 * 文件用途：OpenKeyServer 的引擎宿主：持有 openkey core 的 Engine 实例与
 * 每 TSF 线程一个的会话表，并实现 pipe_protocol.h 声明的全部服务端方法
 * （kMethodTable 定义在本文件对应的 .cpp）。
 *
 * 进程模型（docs/02 4.1.1）：本进程加载 librime + openkey core 全量能力，
 * TSF DLL 只做按键拦截与 UI，全部计算经命名管道委派到这里。
 */

#pragma once

#include <map>
#include <mutex>
#include <string>

#include "openkey_client.h"      // openkey core RAII 封装
#include "pipe_protocol.h"       // 协议与 kMethodTable 声明

namespace openkey {

class EngineHost {
 public:
  static EngineHost& Instance();

  /* 初始化引擎（默认目录约定见 docs/02 4.1.4 与 .cpp 注释）。
   * 失败返回 false，详情见 last_error()。 */
  bool Init();

  /* 反初始化（进程退出前调用）。 */
  void Shutdown();

  bool IsReady() const { return engine_.IsValid(); }

  std::string last_error() const { return engine_.LastError(); }

  /* 初始化/会话创建失败的详情（对应 ok_last_error 不可用时）。 */
  const std::string& init_error() const { return init_error_; }

  /* 会话表访问（方法处理器内部使用；持锁）。 */
  std::mutex& mutex() { return mutex_; }
  openkey::Session* FindSession(long long key);
  bool CreateSession(long long key);
  bool DestroySession(long long key);

  /* 引擎访问（方法处理器内部使用；Init 后一直有效）。 */
  openkey::Engine& engine() { return engine_; }

 private:
  EngineHost() = default;
  ~EngineHost() = default;

  std::mutex mutex_;
  openkey::Engine engine_;
  std::string init_error_;
  std::map<long long, openkey::Session> sessions_;  // key: TSF 线程 ID
};

}  // namespace openkey
