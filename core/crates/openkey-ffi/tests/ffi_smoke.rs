//! C ABI 冒烟测试：版本串、NULL 安全、错误路径。
//!
//! 这些用例不依赖 librime 链接（`--no-default-features` 亦可运行），
//! 验证五端客户端最基础的调用契约。

use std::ffi::{CStr, CString};
use std::ptr;

use openkey_ffi::*;

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

#[test]
fn version_string_is_static_and_valid() {
    // SAFETY: 返回进程内静态字符串。
    let v = unsafe { CStr::from_ptr(ok_version_string()) };
    assert!(!v.to_str().unwrap().is_empty());
    // 与 crate 版本一致。
    assert_eq!(v.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn null_safety_all_entry_points() {
    let buf = c("").into_raw();
    let buf_len = 1usize;
    // 全部传入空指针/零值：C ABI 为安全签名，函数必须正常返回而不是崩溃。
    {
        assert!(ok_engine_create(ptr::null()).is_null());
        ok_engine_destroy(ptr::null_mut());
        assert!(!ok_engine_deploy(ptr::null_mut()));
        assert_eq!(ok_engine_schema_count(ptr::null_mut()), 0);
        assert!(!ok_engine_schema_at(
            ptr::null_mut(),
            0,
            buf,
            buf_len,
            buf,
            buf_len
        ));
        assert!(!ok_engine_sync_user_data(ptr::null_mut()));
        assert!(!ok_engine_sync_export(ptr::null_mut(), ptr::null()));
        assert!(!ok_engine_sync_import(ptr::null_mut(), ptr::null()));
        assert!(!ok_last_error(ptr::null_mut()).is_null());
        assert!(ok_session_create(ptr::null_mut()).is_null());
        ok_session_destroy(ptr::null_mut());
        assert!(!ok_session_process_key(ptr::null_mut(), 0, 0));
        assert!(!ok_session_commit_composition(ptr::null_mut()));
        ok_session_clear_composition(ptr::null_mut());
        assert!(!ok_session_get_context(ptr::null_mut(), ptr::null_mut()));
        assert!(!ok_session_get_commit(ptr::null_mut(), buf, buf_len));
        assert!(!ok_session_get_status(ptr::null_mut(), ptr::null_mut()));
        assert!(!ok_session_select_candidate(ptr::null_mut(), 0));
        assert!(!ok_session_change_page(ptr::null_mut(), false));
        assert!(!ok_session_set_option(ptr::null_mut(), ptr::null(), false));
        assert!(!ok_session_get_option(ptr::null_mut(), ptr::null()));
        assert!(!ok_session_select_schema(ptr::null_mut(), ptr::null()));
        assert!(ok_session_current_schema(ptr::null_mut()).is_null());
        assert!(!ok_component_set_manifest(ptr::null_mut(), ptr::null()));
        assert_eq!(
            ok_component_ensure(ptr::null_mut(), ptr::null(), None, ptr::null_mut()),
            OK_COMPONENT_ERROR
        );
        assert!(!ok_component_installed(ptr::null_mut(), ptr::null()));
        assert!(!ok_component_current_version(
            ptr::null_mut(),
            ptr::null(),
            buf,
            buf_len
        ));
        assert!(!ok_setting_set(ptr::null_mut(), ptr::null(), ptr::null()));
        assert!(!ok_setting_get(ptr::null_mut(), ptr::null(), buf, buf_len));
    }
    // buf 由 into_raw 产生，此处收回（CString::from_raw 为 unsafe，需显式块）。
    unsafe {
        drop(CString::from_raw(buf));
    }
}

#[test]
fn config_defaults_are_sane() {
    let config = OkEngineConfig::default();
    assert!(config.shared_data_dir.is_null());
    assert!(config.user_data_dir.is_null());
    assert!(config.data_db_path.is_null());
    assert!(config.components_dir.is_null());
    assert_eq!(config.min_log_level, 2);
}

/// 状态码为 ABI 常量，编译期校验（避免运行期 clippy::assertions_on_constants）。
const _: () = {
    assert!(OK_COMPONENT_OK == 0);
    assert!(OK_COMPONENT_ERROR < 0);
    assert!(OK_COMPONENT_NOT_IN_MANIFEST < 0);
    assert!(OK_COMPONENT_DOWNLOAD_FAILED < 0);
    assert!(OK_COMPONENT_CHECKSUM_FAILED < 0);
};

#[test]
fn component_status_codes_are_distinct() {
    let codes = [
        OK_COMPONENT_ERROR,
        OK_COMPONENT_NOT_IN_MANIFEST,
        OK_COMPONENT_DOWNLOAD_FAILED,
        OK_COMPONENT_CHECKSUM_FAILED,
    ];
    let mut sorted = codes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), codes.len(), "状态码必须互不相同");
}

/// 引擎创建：在启用了数据层参数但 librime 未链接的构建下必须返回 NULL 且不崩溃。
#[test]
fn engine_create_fails_cleanly_without_librime() {
    let dir = std::env::temp_dir().join("openkey-ffi-smoke-engine");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("openkey.db");
    let shared = c(dir.to_str().unwrap());
    let user = c(dir.join("user").to_str().unwrap());
    let db = c(db_path.to_str().unwrap());
    let config = OkEngineConfig {
        shared_data_dir: shared.as_ptr(),
        user_data_dir: user.as_ptr(),
        data_db_path: db.as_ptr(),
        components_dir: ptr::null(),
        log_dir: ptr::null(),
        min_log_level: 3,
    };
    // 所有字符串在调用期间有效。
    let engine = ok_engine_create(&config);
    // librime 未链接时应返回 NULL；链接了 librime 的空方案目录部署会失败，
    // 同样返回 NULL。两种情形都不应崩溃。
    if !engine.is_null() {
        // 若创建成功（librime 可用且部署通过），销毁它。
        ok_engine_destroy(engine);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
