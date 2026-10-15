//! librime 引擎全局初始化与部署。
//!
//! librime 为进程级单例（API 结构内为全局状态）。本封装以 `OnceLock`
//! 保证全进程只初始化一次；重复调用 [`RimeEngine::init`] 返回同一实例。

use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[cfg(feature = "librime")]
use std::os::raw::{c_int, c_void};

#[cfg(feature = "librime")]
use log::info;

use crate::error::{cstr_to_string, Error, Result};
#[cfg(feature = "librime")]
use crate::error::{path_to_cstring, to_cstring};
use crate::session::Session;
use crate::sys::*;

static ENGINE: OnceLock<RimeEngine> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// 引擎初始化配置（对应 docs/02 3.1 的 `ok_rime_setup` 参数）。
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// 共享数据目录（方案与词库，各端安装目录下的 shared 数据）。
    pub shared_data_dir: Option<PathBuf>,
    /// 用户数据目录（userdb、build 产物，各端用户目录）。
    pub user_data_dir: Option<PathBuf>,
    /// 日志目录；`None` 表示 librime 临时目录。
    pub log_dir: Option<PathBuf>,
    /// 应用名，librime 约定 `"rime.<应用名>"` 形式。
    pub app_name: String,
    /// glog 最小日志级别（0=INFO .. 3=FATAL）。
    pub min_log_level: i32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            shared_data_dir: None,
            user_data_dir: None,
            log_dir: None,
            app_name: "rime.openkey".to_string(),
            min_log_level: 2,
        }
    }
}

/// librime 全局引擎。进程内单例。
pub struct RimeEngine {
    api: &'static RimeApi,
}

impl RimeEngine {
    /// 初始化（或获取已初始化的）全局引擎，并触发部署。
    ///
    /// 对应官方用法：`setup → set_notification_handler → initialize →
    /// start_maintenance(full_check) → join_maintenance_thread`。
    pub fn init(config: &EngineConfig) -> Result<&'static RimeEngine> {
        let _guard = INIT_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(engine) = ENGINE.get() {
            return Ok(engine);
        }
        let engine = Self::init_librime(config)?;
        // set 只可能因并发失败，而并发已被 INIT_LOCK 排除。
        let _ = ENGINE.set(engine);
        ENGINE.get().ok_or(Error::NotInitialized)
    }

    #[cfg(not(feature = "librime"))]
    fn init_librime(_config: &EngineConfig) -> Result<RimeEngine> {
        Err(Error::NotLinked)
    }

    #[cfg(feature = "librime")]
    fn init_librime(config: &EngineConfig) -> Result<RimeEngine> {
        // 字符串泄漏到进程生命周期：traits 指针需在 setup/initialize 期间及之后
        // 保持有效；引擎为进程级单例，泄漏量可忽略。
        let shared = config
            .shared_data_dir
            .as_ref()
            .map(|p| path_to_cstring(p))
            .transpose()?
            .map(|s| Box::leak(s.into_boxed_c_str()).as_ptr())
            .unwrap_or(std::ptr::null());
        let user = config
            .user_data_dir
            .as_ref()
            .map(|p| path_to_cstring(p))
            .transpose()?
            .map(|s| Box::leak(s.into_boxed_c_str()).as_ptr())
            .unwrap_or(std::ptr::null());
        let log_dir = config
            .log_dir
            .as_ref()
            .map(|p| path_to_cstring(p))
            .transpose()?
            .map(|s| Box::leak(s.into_boxed_c_str()).as_ptr())
            .unwrap_or(std::ptr::null());
        let app_name = Box::leak(to_cstring(&config.app_name)?.into_boxed_c_str()).as_ptr();

        let mut traits = crate::rime_struct_init!(RimeTraits, t);
        traits.shared_data_dir = shared;
        traits.user_data_dir = user;
        traits.log_dir = log_dir;
        traits.app_name = app_name;
        traits.min_log_level = config.min_log_level as c_int;

        // SAFETY: api 为 librime 进程内静态；traits 字段均已有效初始化。
        let engine = unsafe {
            let api = &*rime_get_api();
            if api.data_size <= 0 {
                return Err(Error::Api("rime_get_api 返回空结构"));
            }
            let setup = api.setup.ok_or(Error::Api("缺少 setup"))?;
            setup(&mut traits);
            if let Some(handler) = api.set_notification_handler {
                handler(Some(on_notification), std::ptr::null_mut());
            }
            let initialize = api.initialize.ok_or(Error::Api("缺少 initialize"))?;
            initialize(&mut traits);
            let start = api
                .start_maintenance
                .ok_or(Error::Api("缺少 start_maintenance"))?;
            if start(TRUE) != 0 {
                if let Some(join) = api.join_maintenance_thread {
                    join();
                }
            }
            RimeEngine { api }
        };
        info!("librime {} 初始化完成", Self::version());
        Ok(engine)
    }

    /// librime API 结构引用（进程内静态）。
    pub fn api(&self) -> &'static RimeApi {
        self.api
    }

    /// 获取全局引擎（未初始化时返回 [`Error::NotInitialized`]）。
    pub fn global() -> Result<&'static RimeEngine> {
        ENGINE.get().ok_or(Error::NotInitialized)
    }

    /// librime 版本字符串。
    pub fn version() -> String {
        #[cfg(feature = "librime")]
        {
            // SAFETY: 返回进程内静态字符串。
            unsafe {
                cstr_to_string(
                    (&*rime_get_api())
                        .get_version
                        .map(|f| f())
                        .unwrap_or(std::ptr::null()),
                )
            }
        }
        #[cfg(not(feature = "librime"))]
        {
            String::from("not-linked")
        }
    }

    /// 已部署方案列表 `(schema_id, name)`。
    pub fn schema_list(&self) -> Vec<(String, String)> {
        // RimeSchemaList 非自版本化结构，直接零初始化。
        let mut list: RimeSchemaList = unsafe { std::mem::zeroed() };
        let mut out = Vec::new();
        // SAFETY: list 已按 RIME_STRUCT_INIT 初始化。
        unsafe {
            if let Some(f) = self.api.get_schema_list {
                if f(&mut list) != 0 && !list.list.is_null() {
                    for i in 0..list.size {
                        let item = &*list.list.add(i);
                        out.push((cstr_to_string(item.schema_id), cstr_to_string(item.name)));
                    }
                    if let Some(free) = self.api.free_schema_list {
                        free(&mut list);
                    }
                }
            }
        }
        out
    }

    /// librime 的 userdb 同步快照目录。
    pub fn user_data_sync_dir(&self) -> PathBuf {
        let mut buf = vec![0u8; 4096];
        // SAFETY: buf 容量随调用传入。
        unsafe {
            if let Some(f) = self.api.get_user_data_sync_dir {
                f(buf.as_mut_ptr() as *mut c_char, buf.len());
            }
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned().into()
    }

    /// 触发 userdb 同步快照任务并等待完成（docs/02 3.3.4 的导出前置）。
    pub fn sync_user_data(&self) -> Result<()> {
        // SAFETY: api 有效。
        unsafe {
            let f = self
                .api
                .sync_user_data
                .ok_or(Error::Api("缺少 sync_user_data"))?;
            if f() == 0 {
                return Err(Error::Api("sync_user_data 返回失败"));
            }
            if let Some(join) = self.api.join_maintenance_thread {
                join();
            }
        }
        Ok(())
    }

    /// 导出 userdb 快照：触发同步任务后，把快照目录整体复制到 `dest`。
    pub fn export_sync_snapshot(&self, dest: &Path) -> Result<()> {
        self.sync_user_data()?;
        let sync_dir = self.user_data_sync_dir();
        if sync_dir.as_os_str().is_empty() || !sync_dir.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(dest)?;
        copy_dir_all(&sync_dir, dest)
    }

    /// 导入 userdb 快照：把 `src` 内容合并进快照目录，再触发同步任务
    /// 由 librime 完成合并与重部署。
    pub fn import_sync_snapshot(&self, src: &Path) -> Result<()> {
        let sync_dir = self.user_data_sync_dir();
        if sync_dir.as_os_str().is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&sync_dir)?;
        copy_dir_all(src, &sync_dir)?;
        self.sync_user_data()
    }

    /// 触发一次完整部署并等待完成（schemas 变更后调用）。
    pub fn deploy(&self) -> Result<()> {
        // SAFETY: api 有效。
        unsafe {
            let f = self
                .api
                .start_maintenance
                .ok_or(Error::Api("缺少 start_maintenance"))?;
            if f(TRUE) != 0 {
                if let Some(join) = self.api.join_maintenance_thread {
                    join();
                }
            }
        }
        Ok(())
    }

    /// 创建一个输入会话。
    pub fn create_session(&self) -> Result<Session> {
        Session::create()
    }
}

/// librime 通知回调：转 `log` 记录（部署进度、方案切换、选项变更）。
#[cfg(feature = "librime")]
unsafe extern "C" fn on_notification(
    _context_object: *mut c_void,
    session_id: RimeSessionId,
    message_type: *const c_char,
    message_value: *const c_char,
) {
    // SAFETY: 回调参数为有效 C 字符串。
    let ty = unsafe { cstr_to_string(message_type) };
    let val = unsafe { cstr_to_string(message_value) };
    info!("rime 通知 [session={session_id}] {ty}={val}");
}

fn copy_dir_all(src: &Path, dest: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if file_type.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir_all(&entry.path(), &target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
