//! openkey-imagehost：图床模板引擎（docs/02 3.8）。
//!
//! 分层（docs/02 3.8.1）：
//! - **Template（本 crate 主体，声明式、可热更）**：JSON 描述上传端点、
//!   认证头（`{token}` 占位）、multipart 字段、响应 JSONPath 与受限成功
//!   表达式；内置首发 SM.MS / 兰空 Lsky Pro / Chevereto（自制模板数据，
//!   `templates/*.json`）；模板随 components 机制更新（数据非代码）；
//! - **Provider（签名类，契约见 [`SignedProvider`]）**：阿里 OSS / 腾讯
//!   COS / S3 兼容，签名逻辑内置随版本发布，Phase 5 实现。
//!
//! 本 crate 只做**请求构造与响应解析**（[`build_upload_request`] /
//! [`parse_upload_response`]）；HTTP 传输由各端执行（Phase 5）。
//! 表情管线上传入口见 openkey-emoji（docs/02 3.5）。

pub mod engine;
pub mod error;
pub mod expr;
pub mod jsonpath;
pub mod template;

pub use engine::{
    build_upload_request, parse_upload_response, Credentials, MultipartContent,
    MultipartFieldValue, SignedProvider, UploadOutcome, UploadRequest,
};
pub use error::{Error, Result};
pub use template::{
    builtin_template, builtin_template_ids, builtin_templates, CredentialField, CredentialKind,
    ImageHostTemplate, MultipartField, TemplateCredential, TemplateRequest, TemplateResponse,
};
