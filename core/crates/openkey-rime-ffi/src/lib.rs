//! OpenKey librime 会话桥。
//!
//! 本 crate 封装 librime（`core/vendor/librime`，BSD-3，submodule 锁 1.17.0）
//! 的初始化、会话与输入管线，对应 docs/02 3.1 节：
//! 初始化（`ok_rime_setup` 语义）、会话（按键/候选/上屏）、方案管理、
//! userdb 同步快照导出导入。不包含任何 UI 与网络代码。
//!
//! 未启用 `librime` 特性时，所有调用返回 [`Error::NotLinked`]，
//! 供尚未接入 librime 共享库的平台先行编译。

pub mod engine;
pub mod session;
pub mod sys;

mod error;

pub use engine::{EngineConfig, RimeEngine};
pub use error::{Error, Result};
pub use session::{Candidate, Context, Session, Status};
