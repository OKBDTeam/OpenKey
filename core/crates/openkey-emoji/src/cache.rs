//! 表情产物缓存布局（docs/02 3.5.1）。
//!
//! - `emoji_cache/{key}/upload.gif`：转换产物；`key` 由 [`crate::cache_key_for`]
//!   生成（源字节 SHA-256 前 16 位十六进制 + 长边上限），上传失败重试不重复转换；
//! - `emoji_files/{sha256}.{ext}`：原图留存，供重新转换或导出。

use std::path::{Path, PathBuf};

/// 转换产物目录：`{cache_root}/{key}/`。
pub fn cache_dir(cache_root: &Path, key: &str) -> PathBuf {
    cache_root.join(key)
}

/// 转换产物路径：`{cache_root}/{key}/upload.gif`。
pub fn upload_gif_path(cache_root: &Path, key: &str) -> PathBuf {
    cache_dir(cache_root, key).join("upload.gif")
}

/// 原图路径：`{files_root}/{sha256}.{ext}`（`ext` 不含点，如 `png`）。
pub fn source_file_path(files_root: &Path, sha256: &str, ext: &str) -> PathBuf {
    files_root.join(format!("{sha256}.{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache_key_for;

    #[test]
    fn cache_layout_paths() {
        let root = Path::new("/data/emoji_cache");
        let key = cache_key_for(b"bytes", 400);
        assert_eq!(
            upload_gif_path(root, &key),
            root.join(&key).join("upload.gif")
        );
        let files = Path::new("/data/emoji_files");
        assert_eq!(
            source_file_path(files, "abcdef", "png"),
            files.join("abcdef.png")
        );
    }
}
