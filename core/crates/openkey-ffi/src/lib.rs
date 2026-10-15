//! OpenKey 统一 C ABI（docs/02 2.1「统一导出层」）。
//!
//! 五端客户端唯一入口：头文件由 cbindgen 生成至 `core/ffi-headers/openkey.h`。
//! 本 crate 聚合 rime-ffi（输入管线）、data（条目/设置）、components（组件分发）
//! 三类能力；sync/vault/emoji/voice/imagehost 的 C ABI 随各自 Phase 落地时
//! 以新增函数扩展（ABI 只增不改）。
//!
//! 线程模型：每个入口点内部加锁；librime 会话可跨线程调用但全局部署等操作
//! 由引擎互斥保护。v0.1.0 采用「每引擎一个 Mutex」的粗粒度策略。

#![warn(missing_docs)]
// C ABI 约定：所有导出函数为安全签名（调用方为 C / Swift / Kotlin / JNI，
// 无法表达 unsafe 语义），全部前置条件（NULL 判空、句柄来源与唯一释放）
// 由实现内部保证，并在各函数的 SAFETY 注释中逐条说明。
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::sync::Mutex;

use openkey_components::{ComponentDownloader, ComponentManager};
use openkey_data::Database;
use openkey_rime_ffi::{EngineConfig, RimeEngine, Session};

/// C 布尔类型。
pub type OkBool = bool;

/// 空 C 字符串（NUL 结尾），用于"无内容"占位返回。
static EMPTY_C_STRING: &[u8] = b"\0";

/// 返回空 C 字符串指针。
fn empty_c_string() -> *const c_char {
    EMPTY_C_STRING.as_ptr() as *const c_char
}

/// 引擎初始化配置（调用方填充；字符串字段可为 NULL）。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OkEngineConfig {
    /// librime 共享数据目录（方案与词库）。
    pub shared_data_dir: *const c_char,
    /// librime 用户数据目录（userdb / build）。
    pub user_data_dir: *const c_char,
    /// `openkey.db` 路径（数据层；NULL 则不启用数据层）。
    pub data_db_path: *const c_char,
    /// 组件根目录（NULL 时取 `{user_data_dir}/components`）。
    pub components_dir: *const c_char,
    /// librime 日志目录（NULL 为默认）。
    pub log_dir: *const c_char,
    /// glog 最小日志级别（0=INFO .. 3=FATAL）。
    pub min_log_level: c_int,
}

impl Default for OkEngineConfig {
    fn default() -> Self {
        Self {
            shared_data_dir: std::ptr::null(),
            user_data_dir: std::ptr::null(),
            data_db_path: std::ptr::null(),
            components_dir: std::ptr::null(),
            log_dir: std::ptr::null(),
            min_log_level: 2,
        }
    }
}

/// 候选字词（指针归会话所有，见文件头字符串约定）。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OkCandidate {
    /// 候选文本。
    pub text: *const c_char,
    /// 候选备注（无则为 NULL）。
    pub comment: *const c_char,
}

/// 会话上下文（候选数组指针归会话所有）。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OkContext {
    /// 预编辑文本；NULL 表示未组词。
    pub preedit: *const c_char,
    /// 光标位置（字符数）。
    pub cursor_pos: c_int,
    /// 选择区起点（字符数）。
    pub sel_start: c_int,
    /// 选择区终点（字符数）。
    pub sel_end: c_int,
    /// 当前页候选数组。
    pub candidates: *const OkCandidate,
    /// 候选数量。
    pub num_candidates: usize,
    /// 页码（从 0 开始）。
    pub page_no: c_int,
    /// 每页候选数。
    pub page_size: c_int,
    /// 是否最后一页。
    pub is_last_page: OkBool,
    /// 高亮候选下标。
    pub highlighted_candidate_index: c_int,
}

/// 会话状态快照。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OkStatus {
    /// 当前方案 ID。
    pub schema_id: *const c_char,
    /// 当前方案名。
    pub schema_name: *const c_char,
    /// 是否组词中。
    pub is_composing: OkBool,
    /// 是否英文模式。
    pub is_ascii_mode: OkBool,
    /// 是否全角。
    pub is_full_shape: OkBool,
    /// 是否西文标点。
    pub is_ascii_punct: OkBool,
}

/// 组件下载进度回调：`(用户上下文, 已下载字节, 总字节或0)`。
pub type OkComponentProgressCallback =
    Option<unsafe extern "C" fn(context: *mut c_void, downloaded: u64, total: u64)>;

/// 组件确保就绪的状态码（`ok_component_ensure` 返回值）。
pub const OK_COMPONENT_OK: c_int = 0;
/// 失败（详情见 `ok_last_error`）。
pub const OK_COMPONENT_ERROR: c_int = -1;
/// 清单中无此组件。
pub const OK_COMPONENT_NOT_IN_MANIFEST: c_int = -2;
/// 下载失败。
pub const OK_COMPONENT_DOWNLOAD_FAILED: c_int = -3;
/// SHA-256 校验失败。
pub const OK_COMPONENT_CHECKSUM_FAILED: c_int = -4;

// ---------------------------------------------------------------------------
// 内部实现
// ---------------------------------------------------------------------------

struct EngineInner {
    db: Option<Database>,
    components: Option<ComponentManager>,
    last_error: Option<CString>,
}

/// 引擎句柄（不透明）。
pub struct OkEngine {
    inner: Mutex<EngineInner>,
}

struct ContextSnapshot {
    preedit: Option<CString>,
    /// 仅用于保活 `c_candidates` 指针所指向的 CString 堆数据。
    #[allow(dead_code)]
    candidates: Vec<(CString, Option<CString>)>,
    c_candidates: Vec<OkCandidate>,
    page_no: i32,
    page_size: i32,
    is_last_page: bool,
    highlighted: i32,
    cursor_pos: i32,
    sel_start: i32,
    sel_end: i32,
}

// SAFETY: 快照内的裸指针指向同一快照中 CString / Vec 的堆数据，
// 快照整体替换前有效；跨线程转移不改变其所有权语义（docs/02 2.1：指针
// 「直至下次调用前有效」的约定由调用方遵守）。
unsafe impl Send for ContextSnapshot {}
unsafe impl Sync for ContextSnapshot {}

struct StatusSnapshot {
    schema_id: CString,
    schema_name: CString,
    is_composing: bool,
    is_ascii_mode: bool,
    is_full_shape: bool,
    is_ascii_punct: bool,
}

struct SessionSlot {
    session: Session,
    context: Option<ContextSnapshot>,
    status: Option<StatusSnapshot>,
    last_schema: Option<CString>,
}

/// 会话句柄（不透明）。
pub struct OkSession {
    inner: Mutex<SessionSlot>,
}

// OkSession / OkEngine 需可跨线程使用：编译期断言，防止依赖 crate 引入
// 非 Send/Sync 类型而不被发现。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<OkEngine>();
    assert_send_sync::<OkSession>();
};

fn cstr_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: 调用方约定为 NUL 结尾 C 字符串。
    Some(
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn copy_to_buf(s: &str, buf: *mut c_char, len: usize) -> bool {
    if buf.is_null() || len == 0 {
        return false;
    }
    let bytes = s.as_bytes();
    if bytes.len() + 1 > len {
        return false;
    }
    // SAFETY: 容量已校验。
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, bytes.len());
        *buf.add(bytes.len()) = 0;
    }
    true
}

fn set_error(inner: &mut EngineInner, msg: impl std::fmt::Display) {
    log::error!("openkey-ffi: {msg}");
    inner.last_error = CString::new(msg.to_string()).ok();
}

/// 本地 HLC 生成器（v0.1.0 占位）：`物理毫秒.计数器.设备`。
/// openkey-sync 落地后由真正的 HLC 实现替换（docs/02 3.3.1）。
struct LocalHlc {
    device_id: String,
    counter: std::sync::atomic::AtomicU64,
    last_ms: std::sync::atomic::AtomicI64,
}

impl LocalHlc {
    fn new(device_id: &str) -> Self {
        Self {
            device_id: device_id.to_string(),
            counter: std::sync::atomic::AtomicU64::new(0),
            last_ms: std::sync::atomic::AtomicI64::new(0),
        }
    }

    fn tick(&self) -> String {
        use std::sync::atomic::Ordering;
        let now = openkey_data::now_ms();
        let last = self.last_ms.load(Ordering::Relaxed);
        let counter = if now <= last {
            self.counter.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            self.last_ms.store(now, Ordering::Relaxed);
            self.counter.store(0, Ordering::Relaxed);
            0
        };
        format!("{now}.{counter}.{}", self.device_id)
    }
}

fn engine_device_id(db: &Option<Database>) -> String {
    if let Some(db) = db {
        if let Ok(Some((id, _))) = db.device_identity() {
            return id;
        }
    }
    "local".to_string()
}

// ---------------------------------------------------------------------------
// 版本
// ---------------------------------------------------------------------------

/// 返回 OpenKey 版本字符串（静态，进程生命周期有效）。
#[no_mangle]
pub extern "C" fn ok_version_string() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// 引擎生命周期
// ---------------------------------------------------------------------------

/// 创建引擎：初始化 librime（含部署）、打开数据层与组件管理器。
/// 失败返回 NULL，详情见 `ok_last_error(NULL)`。
#[no_mangle]
pub extern "C" fn ok_engine_create(config: *const OkEngineConfig) -> *mut OkEngine {
    if config.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: 调用方保证指针有效。
    let config = unsafe { *config };

    let shared = cstr_opt(config.shared_data_dir).map(|s| Path::new(&s).to_path_buf());
    let user = cstr_opt(config.user_data_dir).map(|s| Path::new(&s).to_path_buf());
    let db_path = cstr_opt(config.data_db_path);
    let components_dir = cstr_opt(config.components_dir);
    let log_dir = cstr_opt(config.log_dir).map(|s| Path::new(&s).to_path_buf());

    let rime_config = EngineConfig {
        shared_data_dir: shared.clone(),
        user_data_dir: user.clone(),
        log_dir,
        app_name: "rime.openkey".to_string(),
        min_log_level: config.min_log_level,
    };

    if let Err(e) = RimeEngine::init(&rime_config) {
        log::error!("ok_engine_create: librime 初始化失败: {e}");
        return std::ptr::null_mut();
    }

    let db = match db_path
        .as_deref()
        .map(|p| Database::open(Path::new(p)))
        .transpose()
    {
        Ok(db) => db,
        Err(e) => {
            log::error!("ok_engine_create: 数据层打开失败: {e}");
            return std::ptr::null_mut();
        }
    };

    let components_root = components_dir
        .map(|s| Path::new(&s).to_path_buf())
        .or_else(|| user.as_ref().map(|u| u.join("components")));
    let components = components_root.map(ComponentManager::new);

    let inner = EngineInner {
        db,
        components,
        last_error: None,
    };
    Box::into_raw(Box::new(OkEngine {
        inner: Mutex::new(inner),
    }))
}

/// 销毁引擎。
#[no_mangle]
pub extern "C" fn ok_engine_destroy(engine: *mut OkEngine) {
    if engine.is_null() {
        return;
    }
    // SAFETY: 指针由 ok_engine_create 创建，仅释放一次。
    unsafe {
        drop(Box::from_raw(engine));
    }
}

/// 触发重新部署（方案/词库变更后调用）。
// C ABI 约定：传入 NULL 直接返回 false。
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn ok_engine_deploy(engine: *mut OkEngine) -> OkBool {
    let engine = unsafe { engine.as_ref() };
    let Some(engine) = engine else {
        return false;
    };
    match RimeEngine::global().and_then(|e| e.deploy()) {
        Ok(()) => true,
        Err(e) => {
            if let Ok(mut inner) = engine.inner.lock() {
                set_error(&mut inner, e);
            }
            false
        }
    }
}

/// 已部署方案数量。
#[no_mangle]
pub extern "C" fn ok_engine_schema_count(engine: *mut OkEngine) -> usize {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return 0;
    };
    RimeEngine::global()
        .map(|e| e.schema_list().len())
        .unwrap_or_else(|e| {
            if let Ok(mut inner) = engine.inner.lock() {
                set_error(&mut inner, e);
            }
            0
        })
}

/// 按下标取方案 `(schema_id, name)`，写入调用方缓冲区。
#[no_mangle]
pub extern "C" fn ok_engine_schema_at(
    engine: *mut OkEngine,
    index: usize,
    id_buf: *mut c_char,
    id_len: usize,
    name_buf: *mut c_char,
    name_len: usize,
) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    let list = match RimeEngine::global().map(|e| e.schema_list()) {
        Ok(list) => list,
        Err(e) => {
            if let Ok(mut inner) = engine.inner.lock() {
                set_error(&mut inner, e);
            }
            return false;
        }
    };
    let Some((id, name)) = list.get(index) else {
        return false;
    };
    copy_to_buf(id, id_buf, id_len) && copy_to_buf(name, name_buf, name_len)
}

/// 触发 userdb 同步快照任务（docs/02 3.3.4）。
#[no_mangle]
pub extern "C" fn ok_engine_sync_user_data(engine: *mut OkEngine) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    match RimeEngine::global().and_then(|e| e.sync_user_data()) {
        Ok(()) => true,
        Err(e) => {
            if let Ok(mut inner) = engine.inner.lock() {
                set_error(&mut inner, e);
            }
            false
        }
    }
}

/// 导出 userdb 快照到目录（docs/02 3.1 `ok_rime_sync_export`）。
#[no_mangle]
pub extern "C" fn ok_engine_sync_export(engine: *mut OkEngine, dest_dir: *const c_char) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    let Some(dest) = cstr_opt(dest_dir) else {
        return false;
    };
    match RimeEngine::global().and_then(|e| e.export_sync_snapshot(Path::new(&dest))) {
        Ok(()) => true,
        Err(e) => {
            if let Ok(mut inner) = engine.inner.lock() {
                set_error(&mut inner, e);
            }
            false
        }
    }
}

/// 从目录导入 userdb 快照（docs/02 3.1 `ok_rime_sync_import`）。
#[no_mangle]
pub extern "C" fn ok_engine_sync_import(engine: *mut OkEngine, src_dir: *const c_char) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    let Some(src) = cstr_opt(src_dir) else {
        return false;
    };
    match RimeEngine::global().and_then(|e| e.import_sync_snapshot(Path::new(&src))) {
        Ok(()) => true,
        Err(e) => {
            if let Ok(mut inner) = engine.inner.lock() {
                set_error(&mut inner, e);
            }
            false
        }
    }
}

/// 取最近一次错误详情（`engine` 为 NULL 时返回全局最近错误占位空串）。
#[no_mangle]
pub extern "C" fn ok_last_error(engine: *mut OkEngine) -> *const c_char {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return empty_c_string();
    };
    match engine.inner.lock() {
        Ok(inner) => inner
            .last_error
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(empty_c_string()),
        Err(_) => empty_c_string(),
    }
}

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

/// 创建输入会话。
#[no_mangle]
pub extern "C" fn ok_session_create(engine: *mut OkEngine) -> *mut OkSession {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return std::ptr::null_mut();
    };
    match Session::create() {
        Ok(session) => Box::into_raw(Box::new(OkSession {
            inner: Mutex::new(SessionSlot {
                session,
                context: None,
                status: None,
                last_schema: None,
            }),
        })),
        Err(e) => {
            if let Ok(mut inner) = engine.inner.lock() {
                set_error(&mut inner, e);
            }
            std::ptr::null_mut()
        }
    }
}

/// 销毁会话。
#[no_mangle]
pub extern "C" fn ok_session_destroy(session: *mut OkSession) {
    if session.is_null() {
        return;
    }
    // SAFETY: 指针由 ok_session_create 创建，仅释放一次。
    unsafe {
        drop(Box::from_raw(session));
    }
}

/// 处理按键。`keycode` 为 X11 keysym，`mask` 为 X11 修饰键
/// （Shift=1, Caps=2, Control=4, Alt/Mod1=8）。返回是否被 librime 消费。
#[no_mangle]
pub extern "C" fn ok_session_process_key(
    session: *mut OkSession,
    keycode: u32,
    mask: u32,
) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Ok(slot) = session.inner.lock() else {
        return false;
    };
    slot.session
        .process_key(keycode as i32, mask as i32)
        .unwrap_or(false)
}

/// 上屏当前预编辑。
#[no_mangle]
pub extern "C" fn ok_session_commit_composition(session: *mut OkSession) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Ok(slot) = session.inner.lock() else {
        return false;
    };
    slot.session.commit_composition().unwrap_or(false)
}

/// 清除组词。
#[no_mangle]
pub extern "C" fn ok_session_clear_composition(session: *mut OkSession) {
    if let Some(session) = unsafe { session.as_ref() } {
        if let Ok(slot) = session.inner.lock() {
            slot.session.clear_composition();
        }
    }
}

/// 取当前上下文（预编辑 + 候选页）。输出指针直至下次对本会话调用前有效。
#[no_mangle]
pub extern "C" fn ok_session_get_context(session: *mut OkSession, out: *mut OkContext) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let Ok(mut slot) = session.inner.lock() else {
        return false;
    };
    let ctx = match slot.session.context() {
        Ok(ctx) => ctx,
        Err(_) => return false,
    };

    let mut candidates: Vec<(CString, Option<CString>)> = Vec::new();
    for c in &ctx.candidates {
        let text = CString::new(c.text.clone()).unwrap_or_default();
        let comment = c
            .comment
            .as_ref()
            .map(|s| CString::new(s.clone()).unwrap_or_default());
        candidates.push((text, comment));
    }
    let c_candidates: Vec<OkCandidate> = candidates
        .iter()
        .map(|(text, comment)| OkCandidate {
            text: text.as_ptr(),
            comment: comment
                .as_ref()
                .map(|c| c.as_ptr())
                .unwrap_or(std::ptr::null()),
        })
        .collect();
    let preedit = ctx
        .preedit
        .as_ref()
        .map(|s| CString::new(s.clone()).unwrap_or_default());

    let snapshot = ContextSnapshot {
        preedit,
        candidates,
        c_candidates,
        page_no: ctx.page_no,
        page_size: ctx.page_size,
        is_last_page: ctx.is_last_page,
        highlighted: ctx.highlighted,
        cursor_pos: ctx.cursor_pos,
        sel_start: ctx.sel_start,
        sel_end: ctx.sel_end,
    };

    // SAFETY: 快照内指针均为堆分配，快照整体替换前保持有效；
    // 替换后旧指针失效——头文件已约定「直至下次调用前有效」。
    let view = OkContext {
        preedit: snapshot
            .preedit
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null()),
        cursor_pos: snapshot.cursor_pos,
        sel_start: snapshot.sel_start,
        sel_end: snapshot.sel_end,
        candidates: snapshot.c_candidates.as_ptr(),
        num_candidates: snapshot.c_candidates.len(),
        page_no: snapshot.page_no,
        page_size: snapshot.page_size,
        is_last_page: snapshot.is_last_page,
        highlighted_candidate_index: snapshot.highlighted,
    };
    slot.context = Some(snapshot);
    unsafe {
        *out = view;
    }
    true
}

/// 取待上屏文本，写入调用方缓冲区；无待上屏文本返回 false。
#[no_mangle]
pub extern "C" fn ok_session_get_commit(
    session: *mut OkSession,
    buf: *mut c_char,
    len: usize,
) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Ok(slot) = session.inner.lock() else {
        return false;
    };
    match slot.session.commit() {
        Ok(Some(text)) => copy_to_buf(&text, buf, len),
        _ => false,
    }
}

/// 取状态快照。输出指针直至下次对本会话调用前有效。
#[no_mangle]
pub extern "C" fn ok_session_get_status(session: *mut OkSession, out: *mut OkStatus) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let Ok(mut slot) = session.inner.lock() else {
        return false;
    };
    let st = match slot.session.status() {
        Ok(st) => st,
        Err(_) => return false,
    };
    let snapshot = StatusSnapshot {
        schema_id: CString::new(st.schema_id).unwrap_or_default(),
        schema_name: CString::new(st.schema_name).unwrap_or_default(),
        is_composing: st.is_composing,
        is_ascii_mode: st.is_ascii_mode,
        is_full_shape: st.is_full_shape,
        is_ascii_punct: st.is_ascii_punct,
    };
    // SAFETY: 同上；指针在快照替换前有效。
    let view = OkStatus {
        schema_id: snapshot.schema_id.as_ptr(),
        schema_name: snapshot.schema_name.as_ptr(),
        is_composing: snapshot.is_composing,
        is_ascii_mode: snapshot.is_ascii_mode,
        is_full_shape: snapshot.is_full_shape,
        is_ascii_punct: snapshot.is_ascii_punct,
    };
    slot.status = Some(snapshot);
    unsafe {
        *out = view;
    }
    true
}

/// 选择候选（`index` 为**当前页内**下标，与 `ok_session_get_context` 返回的
/// 候选顺序一致；越界或失败返回 false）。
#[no_mangle]
pub extern "C" fn ok_session_select_candidate(session: *mut OkSession, index: usize) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Ok(slot) = session.inner.lock() else {
        return false;
    };
    slot.session.select_candidate(index).unwrap_or(false)
}

/// 翻页。
#[no_mangle]
pub extern "C" fn ok_session_change_page(session: *mut OkSession, backward: OkBool) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Ok(slot) = session.inner.lock() else {
        return false;
    };
    slot.session.change_page(backward).unwrap_or(false)
}

/// 设置会话选项（如 `ascii_mode`）。
#[no_mangle]
pub extern "C" fn ok_session_set_option(
    session: *mut OkSession,
    option: *const c_char,
    value: OkBool,
) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Some(option) = cstr_opt(option) else {
        return false;
    };
    let Ok(slot) = session.inner.lock() else {
        return false;
    };
    slot.session.set_option(&option, value).is_ok()
}

/// 读取会话选项。
#[no_mangle]
pub extern "C" fn ok_session_get_option(session: *mut OkSession, option: *const c_char) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Some(option) = cstr_opt(option) else {
        return false;
    };
    let Ok(slot) = session.inner.lock() else {
        return false;
    };
    slot.session.get_option(&option).unwrap_or(false)
}

/// 切换方案。
#[no_mangle]
pub extern "C" fn ok_session_select_schema(
    session: *mut OkSession,
    schema_id: *const c_char,
) -> OkBool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let Some(schema_id) = cstr_opt(schema_id) else {
        return false;
    };
    let Ok(mut slot) = session.inner.lock() else {
        return false;
    };
    match slot.session.select_schema(&schema_id) {
        Ok(true) => {
            slot.last_schema = CString::new(schema_id).ok();
            true
        }
        _ => false,
    }
}

/// 当前方案 ID（归会话所有，下次调用前有效）。
#[no_mangle]
pub extern "C" fn ok_session_current_schema(session: *mut OkSession) -> *const c_char {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return std::ptr::null();
    };
    let Ok(mut slot) = session.inner.lock() else {
        return std::ptr::null();
    };
    match slot.session.current_schema() {
        Ok(id) if !id.is_empty() => {
            slot.last_schema = CString::new(id).ok();
            slot.last_schema
                .as_ref()
                .map(|s| s.as_ptr())
                .unwrap_or(std::ptr::null())
        }
        _ => std::ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// 组件分发（docs/02 3.7）
// ---------------------------------------------------------------------------

/// 加载 versions.json 清单文本。
#[no_mangle]
pub extern "C" fn ok_component_set_manifest(
    engine: *mut OkEngine,
    manifest_json: *const c_char,
) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    let Some(json) = cstr_opt(manifest_json) else {
        return false;
    };
    let Ok(mut inner) = engine.inner.lock() else {
        return false;
    };
    let Some(components) = inner.components.as_ref() else {
        set_error(&mut inner, "组件管理器未启用（未配置组件目录）");
        return false;
    };
    match components.set_manifest_json(&json) {
        Ok(()) => true,
        Err(e) => {
            set_error(&mut inner, e);
            false
        }
    }
}

/// 确保组件就绪（下载 → 校验 → 原子解压 → 登记）。
/// 返回 [`OK_COMPONENT_OK`] 或负状态码；进度经回调上报。
#[no_mangle]
pub extern "C" fn ok_component_ensure(
    engine: *mut OkEngine,
    name: *const c_char,
    progress: OkComponentProgressCallback,
    context: *mut c_void,
) -> c_int {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return OK_COMPONENT_ERROR;
    };
    let Some(name) = cstr_opt(name) else {
        return OK_COMPONENT_ERROR;
    };
    let Ok(mut inner) = engine.inner.lock() else {
        return OK_COMPONENT_ERROR;
    };
    let has_manager = inner.components.is_some();
    if !has_manager {
        set_error(&mut inner, "组件管理器未启用（未配置组件目录）");
        return OK_COMPONENT_ERROR;
    }

    struct CallbackDownloader {
        cb: OkComponentProgressCallback,
        ctx: *mut c_void,
    }
    // SAFETY: 回调与上下文由调用方在同步调用期间保持有效。
    unsafe impl Send for CallbackDownloader {}
    unsafe impl Sync for CallbackDownloader {}

    impl ComponentDownloader for CallbackDownloader {
        fn download(
            &self,
            url: &str,
            dest: &Path,
            _on_progress: &dyn Fn(u64, Option<u64>),
        ) -> openkey_components::Result<()> {
            #[cfg(feature = "net")]
            {
                use openkey_components::Error as CE;
                use std::io::Read as _;

                let client = reqwest::blocking::Client::builder()
                    .user_agent(concat!("OpenKey/", env!("CARGO_PKG_VERSION")))
                    .build()
                    .map_err(|e| CE::Download(e.to_string()))?;
                let mut response = client
                    .get(url)
                    .send()
                    .map_err(|e| CE::Download(e.to_string()))?;
                if !response.status().is_success() {
                    return Err(CE::Download(format!("HTTP {}", response.status())));
                }
                let total = response.content_length();
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).map_err(CE::Io)?;
                }
                let mut file = std::fs::File::create(dest).map_err(CE::Io)?;
                let mut buf = [0u8; 64 * 1024];
                let mut downloaded = 0u64;
                loop {
                    let n = response
                        .read(&mut buf)
                        .map_err(|e| CE::Download(e.to_string()))?;
                    if n == 0 {
                        break;
                    }
                    use std::io::Write as _;
                    file.write_all(&buf[..n]).map_err(CE::Io)?;
                    downloaded += n as u64;
                    if let Some(cb) = self.cb {
                        // SAFETY: 调用方约定同步期间上下文有效。
                        unsafe { cb(self.ctx, downloaded, total.unwrap_or(0)) };
                    }
                }
                Ok(())
            }
            #[cfg(not(feature = "net"))]
            {
                let _ = (url, dest, _on_progress, self.cb, self.ctx);
                Err(openkey_components::Error::Download(
                    "net feature 未启用".to_string(),
                ))
            }
        }
    }

    let downloader = CallbackDownloader {
        cb: progress,
        ctx: context,
    };
    // 组件管理器借用随 inner 锁持有；下载在锁内完成（v0.1.0 策略：
    // 组件操作与输入处理互斥，客户端应在设置流程中触发下载）。
    let Some(components) = inner.components.as_ref() else {
        return OK_COMPONENT_ERROR;
    };
    match components.ensure(&name, &downloader) {
        Ok(_version) => OK_COMPONENT_OK,
        Err(openkey_components::Error::NotInManifest(_)) => {
            set_error(&mut inner, format!("清单中无组件 {name}"));
            OK_COMPONENT_NOT_IN_MANIFEST
        }
        Err(openkey_components::Error::Download(_)) => {
            set_error(&mut inner, format!("组件 {name} 下载失败"));
            OK_COMPONENT_DOWNLOAD_FAILED
        }
        Err(openkey_components::Error::ChecksumMismatch { .. }) => {
            set_error(&mut inner, format!("组件 {name} 校验失败"));
            OK_COMPONENT_CHECKSUM_FAILED
        }
        Err(e) => {
            set_error(&mut inner, e);
            OK_COMPONENT_ERROR
        }
    }
}

/// 组件是否已安装。
#[no_mangle]
pub extern "C" fn ok_component_installed(engine: *mut OkEngine, name: *const c_char) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    let Some(name) = cstr_opt(name) else {
        return false;
    };
    let Ok(inner) = engine.inner.lock() else {
        return false;
    };
    inner
        .components
        .as_ref()
        .map(|c| c.is_installed(&name))
        .unwrap_or(false)
}

/// 取组件当前版本，写入调用方缓冲区。
#[no_mangle]
pub extern "C" fn ok_component_current_version(
    engine: *mut OkEngine,
    name: *const c_char,
    buf: *mut c_char,
    len: usize,
) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    let Some(name) = cstr_opt(name) else {
        return false;
    };
    let Ok(inner) = engine.inner.lock() else {
        return false;
    };
    match inner
        .components
        .as_ref()
        .and_then(|c| c.installed_version(&name))
    {
        Some(version) => copy_to_buf(&version, buf, len),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 数据层（设置条目）
// ---------------------------------------------------------------------------

/// 写入设置项（JSON 值文本）。
#[no_mangle]
pub extern "C" fn ok_setting_set(
    engine: *mut OkEngine,
    key: *const c_char,
    value_json: *const c_char,
) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    let (Some(key), Some(value_json)) = (cstr_opt(key), cstr_opt(value_json)) else {
        return false;
    };
    let Ok(mut inner) = engine.inner.lock() else {
        return false;
    };
    let value: serde_json::Value = match serde_json::from_str(&value_json) {
        Ok(v) => v,
        Err(e) => {
            set_error(&mut inner, format!("设置值非合法 JSON: {e}"));
            return false;
        }
    };
    let device_id = engine_device_id(&inner.db);
    let hlc = LocalHlc::new(&device_id).tick();
    let result = match inner.db.as_ref() {
        Some(db) => db.set_setting(&key, &value, &hlc, &device_id),
        None => {
            set_error(&mut inner, "数据层未启用");
            return false;
        }
    };
    match result {
        Ok(_) => true,
        Err(e) => {
            set_error(&mut inner, e);
            false
        }
    }
}

/// 读取设置项（JSON 文本）到调用方缓冲区。
#[no_mangle]
pub extern "C" fn ok_setting_get(
    engine: *mut OkEngine,
    key: *const c_char,
    buf: *mut c_char,
    len: usize,
) -> OkBool {
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return false;
    };
    let Some(key) = cstr_opt(key) else {
        return false;
    };
    let Ok(inner) = engine.inner.lock() else {
        return false;
    };
    let Some(db) = inner.db.as_ref() else {
        return false;
    };
    match db.get_setting(&key) {
        Ok(Some(value)) => copy_to_buf(&value.to_string(), buf, len),
        _ => false,
    }
}
