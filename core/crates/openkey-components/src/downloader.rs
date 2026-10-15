//! reqwest 网络下载实现（`net` feature，docs/02 3.7）。
//!
//! 流式下载：64 KiB 分块读写、边下边报进度；半成品文件由调用方
//! （[`crate::ComponentManager::ensure`]）在失败路径统一清理。

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::error::{Error, Result};
use crate::ComponentDownloader;

/// 基于 reqwest blocking + native-tls 的组件下载器。
#[derive(Debug, Clone)]
pub struct ReqwestDownloader {
    client: reqwest::blocking::Client,
}

impl ReqwestDownloader {
    /// 构造下载器（UA 标识 OpenKey 版本，便于服务端识别与统计）。
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("OpenKey/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::Download(e.to_string()))?;
        Ok(Self { client })
    }
}

impl ComponentDownloader for ReqwestDownloader {
    fn download(&self, url: &str, dest: &Path, progress: &dyn Fn(u64, Option<u64>)) -> Result<()> {
        let mut response = self
            .client
            .get(url)
            .send()
            .map_err(|e| Error::Download(format!("{url}: {e}")))?;
        if !response.status().is_success() {
            return Err(Error::Download(format!(
                "{url}: HTTP {}",
                response.status()
            )));
        }
        let total = response.content_length();
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(dest)?;
        let mut buf = [0u8; super::CHUNK];
        let mut downloaded = 0u64;
        loop {
            let n = response
                .read(&mut buf)
                .map_err(|e| Error::Download(format!("{url}: {e}")))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            downloaded += n as u64;
            progress(downloaded, total);
        }
        // 落盘后再返回，确保 ensure 的 rename/登记不会读到未刷新数据。
        file.sync_all()?;
        Ok(())
    }
}
