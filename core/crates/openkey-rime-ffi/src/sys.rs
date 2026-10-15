//! librime C API 的裸绑定。
//!
//! 结构体布局与函数签名逐字段对齐 `core/vendor/librime/src/rime_api.h`
//! （submodule 锁定 1.17.0）。`RimeApi` 为自版本化结构（`data_size` 记录
//! 扩展字段长度），这里完整声明至 1.17.0 的全部字段；调用方通过
//! [`crate::engine`] 的安全封装访问。
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_void};

/// rime 的 `Bool` 为 `int`。
pub type Bool = c_int;
/// `uintptr_t`。
pub type RimeSessionId = usize;

pub const TRUE: Bool = 1;
pub const FALSE: Bool = 0;

/// 自版本化结构的初始化：`data_size = sizeof(T) - sizeof(data_size)`。
///
/// 对齐 rime_api.h 的 `RIME_STRUCT_INIT`。路径使用全限定写法，
/// 便于在任意模块展开。
#[macro_export]
macro_rules! rime_struct_init {
    ($ty:ty, $var:ident) => {{
        let mut $var: $ty = unsafe { std::mem::zeroed() };
        $var.data_size = (std::mem::size_of::<$ty>() - std::mem::size_of::<std::os::raw::c_int>())
            as std::os::raw::c_int;
        $var
    }};
}

pub type RimeNotificationHandler = Option<
    unsafe extern "C" fn(
        context_object: *mut c_void,
        session_id: RimeSessionId,
        message_type: *const c_char,
        message_value: *const c_char,
    ),
>;

#[repr(C)]
pub struct RimeTraits {
    pub data_size: c_int,
    pub shared_data_dir: *const c_char,
    pub user_data_dir: *const c_char,
    pub distribution_name: *const c_char,
    pub distribution_code_name: *const c_char,
    pub distribution_version: *const c_char,
    pub app_name: *const c_char,
    pub modules: *const *const c_char,
    pub min_log_level: c_int,
    pub log_dir: *const c_char,
    pub prebuilt_data_dir: *const c_char,
    pub staging_dir: *const c_char,
}

#[repr(C)]
pub struct RimeComposition {
    pub length: c_int,
    pub cursor_pos: c_int,
    pub sel_start: c_int,
    pub sel_end: c_int,
    pub preedit: *mut c_char,
}

#[repr(C)]
pub struct RimeCandidate {
    pub text: *mut c_char,
    pub comment: *mut c_char,
    pub reserved: *mut c_void,
}

#[repr(C)]
pub struct RimeMenu {
    pub page_size: c_int,
    pub page_no: c_int,
    pub is_last_page: Bool,
    pub highlighted_candidate_index: c_int,
    pub num_candidates: c_int,
    pub candidates: *mut RimeCandidate,
    pub select_keys: *mut c_char,
}

#[repr(C)]
pub struct RimeCommit {
    pub data_size: c_int,
    pub text: *mut c_char,
}

#[repr(C)]
pub struct RimeContext {
    pub data_size: c_int,
    pub composition: RimeComposition,
    pub menu: RimeMenu,
    pub commit_text_preview: *mut c_char,
    pub select_labels: *mut *mut c_char,
}

#[repr(C)]
pub struct RimeStatus {
    pub data_size: c_int,
    pub schema_id: *mut c_char,
    pub schema_name: *mut c_char,
    pub is_disabled: Bool,
    pub is_composing: Bool,
    pub is_ascii_mode: Bool,
    pub is_full_shape: Bool,
    pub is_simplified: Bool,
    pub is_traditional: Bool,
    pub is_ascii_punct: Bool,
}

#[repr(C)]
pub struct RimeCandidateListIterator {
    pub ptr: *mut c_void,
    pub index: c_int,
    pub candidate: RimeCandidate,
}

#[repr(C)]
pub struct RimeConfig {
    pub ptr: *mut c_void,
}

#[repr(C)]
pub struct RimeConfigIterator {
    pub list: *mut c_void,
    pub map: *mut c_void,
    pub index: c_int,
    pub key: *const c_char,
    pub path: *const c_char,
}

#[repr(C)]
pub struct RimeSchemaListItem {
    pub schema_id: *mut c_char,
    pub name: *mut c_char,
    pub reserved: *mut c_void,
}

#[repr(C)]
pub struct RimeSchemaList {
    pub size: usize,
    pub list: *mut RimeSchemaListItem,
}

#[repr(C)]
pub struct RimeStringSlice {
    pub str: *const c_char,
    pub length: usize,
}

#[repr(C)]
pub struct RimeCustomApi {
    pub data_size: c_int,
}

#[repr(C)]
pub struct RimeModule {
    pub data_size: c_int,
    pub module_name: *const c_char,
    pub initialize: Option<unsafe extern "C" fn()>,
    pub finalize: Option<unsafe extern "C" fn()>,
    pub get_api: Option<unsafe extern "C" fn() -> *mut RimeCustomApi>,
}

#[repr(C)]
pub struct RimeApi {
    pub data_size: c_int,
    pub setup: Option<unsafe extern "C" fn(*mut RimeTraits)>,
    pub set_notification_handler:
        Option<unsafe extern "C" fn(RimeNotificationHandler, *mut c_void)>,
    pub initialize: Option<unsafe extern "C" fn(*mut RimeTraits)>,
    pub finalize: Option<unsafe extern "C" fn()>,
    pub start_maintenance: Option<unsafe extern "C" fn(Bool) -> Bool>,
    pub is_maintenance_mode: Option<unsafe extern "C" fn() -> Bool>,
    pub join_maintenance_thread: Option<unsafe extern "C" fn()>,
    pub deployer_initialize: Option<unsafe extern "C" fn(*mut RimeTraits)>,
    pub prebuild: Option<unsafe extern "C" fn() -> Bool>,
    pub deploy: Option<unsafe extern "C" fn() -> Bool>,
    pub deploy_schema: Option<unsafe extern "C" fn(*const c_char) -> Bool>,
    pub deploy_config_file: Option<unsafe extern "C" fn(*const c_char, *const c_char) -> Bool>,
    pub sync_user_data: Option<unsafe extern "C" fn() -> Bool>,
    pub create_session: Option<unsafe extern "C" fn() -> RimeSessionId>,
    pub find_session: Option<unsafe extern "C" fn(RimeSessionId) -> Bool>,
    pub destroy_session: Option<unsafe extern "C" fn(RimeSessionId) -> Bool>,
    pub cleanup_stale_sessions: Option<unsafe extern "C" fn()>,
    pub cleanup_all_sessions: Option<unsafe extern "C" fn()>,
    pub process_key: Option<unsafe extern "C" fn(RimeSessionId, c_int, c_int) -> Bool>,
    pub commit_composition: Option<unsafe extern "C" fn(RimeSessionId) -> Bool>,
    pub clear_composition: Option<unsafe extern "C" fn(RimeSessionId)>,
    pub get_commit: Option<unsafe extern "C" fn(RimeSessionId, *mut RimeCommit) -> Bool>,
    pub free_commit: Option<unsafe extern "C" fn(*mut RimeCommit) -> Bool>,
    pub get_context: Option<unsafe extern "C" fn(RimeSessionId, *mut RimeContext) -> Bool>,
    pub free_context: Option<unsafe extern "C" fn(*mut RimeContext) -> Bool>,
    pub get_status: Option<unsafe extern "C" fn(RimeSessionId, *mut RimeStatus) -> Bool>,
    pub free_status: Option<unsafe extern "C" fn(*mut RimeStatus) -> Bool>,
    pub set_option: Option<unsafe extern "C" fn(RimeSessionId, *const c_char, Bool)>,
    pub get_option: Option<unsafe extern "C" fn(RimeSessionId, *const c_char) -> Bool>,
    pub set_property: Option<unsafe extern "C" fn(RimeSessionId, *const c_char, *const c_char)>,
    pub get_property:
        Option<unsafe extern "C" fn(RimeSessionId, *const c_char, *mut c_char, usize) -> Bool>,
    pub get_schema_list: Option<unsafe extern "C" fn(*mut RimeSchemaList) -> Bool>,
    pub free_schema_list: Option<unsafe extern "C" fn(*mut RimeSchemaList)>,
    pub get_current_schema: Option<unsafe extern "C" fn(RimeSessionId, *mut c_char, usize) -> Bool>,
    pub select_schema: Option<unsafe extern "C" fn(RimeSessionId, *const c_char) -> Bool>,
    pub schema_open: Option<unsafe extern "C" fn(*const c_char, *mut RimeConfig) -> Bool>,
    pub config_open: Option<unsafe extern "C" fn(*const c_char, *mut RimeConfig) -> Bool>,
    pub config_close: Option<unsafe extern "C" fn(*mut RimeConfig) -> Bool>,
    pub config_get_bool:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, *mut Bool) -> Bool>,
    pub config_get_int:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, *mut c_int) -> Bool>,
    pub config_get_double:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, *mut f64) -> Bool>,
    pub config_get_string:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, *mut c_char, usize) -> Bool>,
    pub config_get_cstring:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char) -> *const c_char>,
    pub config_update_signature:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char) -> Bool>,
    pub config_begin_map: Option<
        unsafe extern "C" fn(*mut RimeConfigIterator, *mut RimeConfig, *const c_char) -> Bool,
    >,
    pub config_next: Option<unsafe extern "C" fn(*mut RimeConfigIterator) -> Bool>,
    pub config_end: Option<unsafe extern "C" fn(*mut RimeConfigIterator)>,
    pub simulate_key_sequence: Option<unsafe extern "C" fn(RimeSessionId, *const c_char) -> Bool>,
    pub register_module: Option<unsafe extern "C" fn(*mut RimeModule) -> Bool>,
    pub find_module: Option<unsafe extern "C" fn(*const c_char) -> *mut RimeModule>,
    pub run_task: Option<unsafe extern "C" fn(*const c_char) -> Bool>,
    pub get_shared_data_dir: Option<unsafe extern "C" fn() -> *const c_char>,
    pub get_user_data_dir: Option<unsafe extern "C" fn() -> *const c_char>,
    pub get_sync_dir: Option<unsafe extern "C" fn() -> *const c_char>,
    pub get_user_id: Option<unsafe extern "C" fn() -> *const c_char>,
    pub get_user_data_sync_dir: Option<unsafe extern "C" fn(*mut c_char, usize)>,
    pub config_init: Option<unsafe extern "C" fn(*mut RimeConfig) -> Bool>,
    pub config_load_string: Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char) -> Bool>,
    pub config_set_bool: Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, Bool) -> Bool>,
    pub config_set_int: Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, c_int) -> Bool>,
    pub config_set_double:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, f64) -> Bool>,
    pub config_set_string:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, *const c_char) -> Bool>,
    pub config_get_item:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, *mut RimeConfig) -> Bool>,
    pub config_set_item:
        Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char, *mut RimeConfig) -> Bool>,
    pub config_clear: Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char) -> Bool>,
    pub config_create_list: Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char) -> Bool>,
    pub config_create_map: Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char) -> Bool>,
    pub config_list_size: Option<unsafe extern "C" fn(*mut RimeConfig, *const c_char) -> usize>,
    pub config_begin_list: Option<
        unsafe extern "C" fn(*mut RimeConfigIterator, *mut RimeConfig, *const c_char) -> Bool,
    >,
    pub get_input: Option<unsafe extern "C" fn(RimeSessionId) -> *const c_char>,
    pub get_caret_pos: Option<unsafe extern "C" fn(RimeSessionId) -> usize>,
    pub select_candidate: Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub get_version: Option<unsafe extern "C" fn() -> *const c_char>,
    pub set_caret_pos: Option<unsafe extern "C" fn(RimeSessionId, usize)>,
    pub select_candidate_on_current_page:
        Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub candidate_list_begin:
        Option<unsafe extern "C" fn(RimeSessionId, *mut RimeCandidateListIterator) -> Bool>,
    pub candidate_list_next: Option<unsafe extern "C" fn(*mut RimeCandidateListIterator) -> Bool>,
    pub candidate_list_end: Option<unsafe extern "C" fn(*mut RimeCandidateListIterator)>,
    pub user_config_open: Option<unsafe extern "C" fn(*const c_char, *mut RimeConfig) -> Bool>,
    pub candidate_list_from_index:
        Option<unsafe extern "C" fn(RimeSessionId, *mut RimeCandidateListIterator, c_int) -> Bool>,
    pub get_prebuilt_data_dir: Option<unsafe extern "C" fn() -> *const c_char>,
    pub get_staging_dir: Option<unsafe extern "C" fn() -> *const c_char>,
    pub commit_proto: Option<unsafe extern "C" fn(RimeSessionId, *mut c_void)>,
    pub context_proto: Option<unsafe extern "C" fn(RimeSessionId, *mut c_void)>,
    pub status_proto: Option<unsafe extern "C" fn(RimeSessionId, *mut c_void)>,
    pub get_state_label:
        Option<unsafe extern "C" fn(RimeSessionId, *const c_char, Bool) -> *const c_char>,
    pub delete_candidate: Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub delete_candidate_on_current_page:
        Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub get_state_label_abbreviated:
        Option<unsafe extern "C" fn(RimeSessionId, *const c_char, Bool, Bool) -> RimeStringSlice>,
    pub set_input: Option<unsafe extern "C" fn(RimeSessionId, *const c_char) -> Bool>,
    pub get_shared_data_dir_s: Option<unsafe extern "C" fn(*mut c_char, usize)>,
    pub get_user_data_dir_s: Option<unsafe extern "C" fn(*mut c_char, usize)>,
    pub get_prebuilt_data_dir_s: Option<unsafe extern "C" fn(*mut c_char, usize)>,
    pub get_staging_dir_s: Option<unsafe extern "C" fn(*mut c_char, usize)>,
    pub get_sync_dir_s: Option<unsafe extern "C" fn(*mut c_char, usize)>,
    pub highlight_candidate: Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub highlight_candidate_on_current_page:
        Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub change_page: Option<unsafe extern "C" fn(RimeSessionId, Bool) -> Bool>,
}

extern "C" {
    /// 获取 librime 的 API 结构（进程内静态，生命周期同进程）。
    pub fn rime_get_api() -> *mut RimeApi;
}
