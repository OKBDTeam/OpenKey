use std::fmt;

/// librime 桥错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 本构建未链接 librime（`librime` feature 未启用）。
    #[error("librime 未链接到本构建（feature `librime` 未启用）")]
    NotLinked,
    /// 引擎未初始化。
    #[error("librime 引擎尚未初始化")]
    NotInitialized,
    /// librime API 调用失败。
    #[error("librime API 调用失败: {0}")]
    Api(&'static str),
    /// 非法输入（如路径含 NUL 字节）。
    #[error("非法输入: {0}")]
    InvalidInput(String),
    /// IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;

/// 把 librime 返回的 C 字符串安全转换为 `String`（NULL → 空串）。
///
/// # Safety
/// `ptr` 必须是有效的 NUL 结尾 C 字符串或 NULL。
pub(crate) unsafe fn cstr_to_string(ptr: *const std::os::raw::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// 把 Rust 字符串转换为 CString，拒绝内含 NUL 的输入。
#[cfg(feature = "librime")]
pub(crate) fn to_cstring(s: &str) -> Result<std::ffi::CString> {
    std::ffi::CString::new(s).map_err(|_| Error::InvalidInput("字符串内含 NUL 字节".into()))
}

/// 把路径按 UTF-8 转换为 CString。
#[cfg(feature = "librime")]
pub(crate) fn path_to_cstring(path: &std::path::Path) -> Result<std::ffi::CString> {
    let s = path
        .to_str()
        .ok_or_else(|| Error::InvalidInput(format!("路径非 UTF-8: {}", path.display())))?;
    to_cstring(s)
}

impl fmt::Debug for crate::engine::RimeEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RimeEngine").finish_non_exhaustive()
    }
}
