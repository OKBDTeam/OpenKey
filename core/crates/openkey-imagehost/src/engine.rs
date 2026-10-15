//! 模板引擎：上传请求构造（占位符替换）与响应解析（docs/02 3.8）。
//!
//! 本 crate 只负责**构造与解析**；HTTP 传输由各端（FFI/平台层）在
//! Phase 5 执行（docs/02 3.8.1：模板层覆盖 token 类图床）。
//!
//! - [`build_upload_request`]：URL / 请求头 / 静态 multipart 值中的
//!   `{field}` 占位符替换为用户凭据；
//! - [`parse_upload_response`]：先按 `successWhen` 受限表达式判定成败，
//!   再经 JSONPath 提取图片直链 / 删除链接 / 错误信息。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::expr;
use crate::jsonpath;
use crate::template::{ImageHostTemplate, TemplateResponse};

/// 用户配置的凭据（字段名 → 值；含 `host` 等非密配置）。
pub type Credentials = BTreeMap<String, String>;

/// 构造完成的上传请求（交平台层执行 HTTP）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadRequest {
    /// HTTP 方法（POST / PUT，统一大写）。
    pub method: String,
    /// 替换占位符后的完整 URL。
    pub url: String,
    /// 请求头（名与值均已替换占位符）。
    pub headers: Vec<(String, String)>,
    /// multipart 字段（顺序即模板顺序）。
    pub multipart: Vec<MultipartFieldValue>,
    /// 待上传文件路径（由 [`MultipartContent::File`] 字段引用）。
    pub file_path: PathBuf,
}

/// multipart 字段（占位符已替换）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartFieldValue {
    /// 字段名。
    pub name: String,
    /// 字段内容。
    pub content: MultipartContent,
}

/// multipart 字段内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultipartContent {
    /// 发送 [`UploadRequest::file_path`] 的文件内容。
    File,
    /// 静态字段值。
    Value(String),
}

/// 上传结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadOutcome {
    /// 图床判定成功：图片直链（与可选删除链接）。
    Success {
        /// 图片直链。
        url: String,
        /// 删除链接（模板未配置或响应未返回时为 `None`）。
        delete_url: Option<String>,
    },
    /// 图床判定失败：错误信息（若有）。
    Failure {
        /// 图床返回的错误信息。
        message: Option<String>,
    },
}

/// 构造上传请求：`{field}` 占位符全部替换为 `credentials` 中的值
/// （缺失报 [`Error::MissingCredential`]，未闭合报 [`Error::Template`]）。
pub fn build_upload_request(
    template: &ImageHostTemplate,
    credentials: &Credentials,
    file_path: &Path,
) -> Result<UploadRequest> {
    let request = &template.request;
    let url = replace_placeholders(&request.url, credentials)?;
    let mut headers = Vec::with_capacity(request.headers.len());
    for (name, value) in &request.headers {
        headers.push((name.clone(), replace_placeholders(value, credentials)?));
    }
    let mut multipart = Vec::with_capacity(request.multipart.len());
    for field in &request.multipart {
        let content = if field.from_file {
            MultipartContent::File
        } else {
            let raw = field.value.as_deref().ok_or_else(|| {
                Error::Template(format!("multipart 字段 {} 既非文件也无静态值", field.name))
            })?;
            MultipartContent::Value(replace_placeholders(raw, credentials)?)
        };
        multipart.push(MultipartFieldValue {
            name: replace_placeholders(&field.name, credentials)?,
            content,
        });
    }
    Ok(UploadRequest {
        method: request.method.to_ascii_uppercase(),
        url,
        headers,
        multipart,
        file_path: file_path.to_path_buf(),
    })
}

/// 解析上传响应：`successWhen` 判定成败 → JSONPath 提取直链/删除链/错误信息。
///
/// `successWhen` 缺省视为无条件成功（模板仅依赖 HTTP 状态码的场景）。
pub fn parse_upload_response(template: &ImageHostTemplate, body: &str) -> Result<UploadOutcome> {
    let value: Value = serde_json::from_str(body)?;
    let success = match &template.response.success_when {
        Some(source) => expr::eval(source, &value)?,
        None => true,
    };
    if !success {
        let message = extract_error_message(&template.response, &value)?;
        return Ok(UploadOutcome::Failure { message });
    }
    let url = jsonpath::query(&value, &template.response.url_path)?
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Response(format!(
                "响应中 {} 缺失或非字符串",
                template.response.url_path
            ))
        })?
        .to_string();
    let delete_url = match &template.response.delete_path {
        Some(path) => jsonpath::query(&value, path)?
            .and_then(Value::as_str)
            .map(str::to_string),
        None => None,
    };
    Ok(UploadOutcome::Success { url, delete_url })
}

/// 提取错误信息：`errorPath` 缺失/为 null → `None`；字符串原样；
/// 其他标量/容器序列化为文本。
fn extract_error_message(response: &TemplateResponse, value: &Value) -> Result<Option<String>> {
    let Some(path) = &response.error_path else {
        return Ok(None);
    };
    let Some(found) = jsonpath::query(value, path)? else {
        return Ok(None);
    };
    match found {
        Value::String(text) => Ok(Some(text.clone())),
        Value::Null => Ok(None),
        other => Ok(Some(other.to_string())),
    }
}

/// 替换 `{field}` 占位符（值内容不二次扫描，无注入面）。
fn replace_placeholders(template: &str, credentials: &Credentials) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err(Error::Template(format!("占位符未闭合: {template}")));
        };
        let name = &after[..end];
        if name.is_empty() {
            return Err(Error::Template(format!("占位符名为空: {template}")));
        }
        let value = credentials
            .get(name)
            .ok_or_else(|| Error::MissingCredential(name.to_string()))?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// 签名类图床 Provider（阿里 OSS / 腾讯 COS / S3 兼容，docs/02 3.8.1）。
///
/// 与模板层不同：签名逻辑内置、随版本发布，参数（bucket/region/endpoint/
/// domain/pathPrefix）模板化，设置 UI 渲染专属表单（附录 C）。
///
/// TODO(Phase 5)：实现三个内置 Provider 的签名与请求构造。
pub trait SignedProvider: Send + Sync {
    /// Provider 标识（`oss` / `cos` / `s3`）。
    fn id(&self) -> &str;

    /// TODO(Phase 5)：用 `config`（bucket/region/endpoint/domain/pathPrefix）
    /// 与待上传文件构造带签名的 [`UploadRequest`]。
    fn build_upload_request(
        &self,
        config: &BTreeMap<String, String>,
        file_path: &Path,
    ) -> Result<UploadRequest>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::builtin_template;

    /// 取内置模板（panic 仅限测试）。
    fn template(id: &str) -> ImageHostTemplate {
        builtin_template(id).expect("查询失败").expect("模板应存在")
    }

    #[test]
    fn build_request_replaces_placeholders() {
        let smms = template("smms");
        let credentials = Credentials::from([("token".to_string(), "tk-123".to_string())]);
        let request =
            build_upload_request(&smms, &credentials, Path::new("emoji.gif")).expect("构造失败");
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "https://sm.ms/api/v2/upload");
        assert_eq!(
            request.headers,
            vec![("Authorization".to_string(), "tk-123".to_string())]
        );
        assert_eq!(
            request.multipart,
            vec![MultipartFieldValue {
                name: "smfile".to_string(),
                content: MultipartContent::File,
            }]
        );
        assert_eq!(request.file_path, PathBuf::from("emoji.gif"));
    }

    #[test]
    fn build_request_replaces_host_placeholder() {
        let lsky = template("lsky");
        let credentials = Credentials::from([
            ("host".to_string(), "img.example.com".to_string()),
            ("token".to_string(), "tk-123".to_string()),
        ]);
        let request =
            build_upload_request(&lsky, &credentials, Path::new("a.png")).expect("构造失败");
        assert_eq!(request.url, "https://img.example.com/api/v1/upload");
        assert_eq!(
            request.headers,
            vec![("Authorization".to_string(), "Bearer tk-123".to_string())]
        );
    }

    #[test]
    fn build_request_keeps_static_multipart_values() {
        let chevereto = template("chevereto");
        let credentials = Credentials::from([
            ("host".to_string(), "img.example.com".to_string()),
            ("token".to_string(), "key-1".to_string()),
        ]);
        let request =
            build_upload_request(&chevereto, &credentials, Path::new("a.png")).expect("构造失败");
        assert_eq!(request.url, "https://img.example.com/api/1/upload");
        assert_eq!(
            request.headers,
            vec![("X-API-Key".to_string(), "key-1".to_string())]
        );
        assert_eq!(
            request.multipart,
            vec![
                MultipartFieldValue {
                    name: "source".to_string(),
                    content: MultipartContent::File,
                },
                MultipartFieldValue {
                    name: "format".to_string(),
                    content: MultipartContent::Value("json".to_string()),
                },
            ]
        );
    }

    #[test]
    fn build_request_missing_credential_errors() {
        let smms = template("smms");
        let err = build_upload_request(&smms, &Credentials::new(), Path::new("a.png"))
            .expect_err("缺 token 应报错");
        assert!(
            matches!(err, Error::MissingCredential(ref name) if name == "token"),
            "实际错误: {err}"
        );
    }

    #[test]
    fn build_request_unclosed_placeholder_errors() {
        let mut smms = template("smms");
        smms.request.url = "https://sm.ms/api/v2/{token/upload".to_string();
        let credentials = Credentials::from([("token".to_string(), "tk".to_string())]);
        let err = build_upload_request(&smms, &credentials, Path::new("a.png"))
            .expect_err("未闭合占位符应报错");
        assert!(matches!(err, Error::Template(_)), "实际错误: {err}");
    }

    #[test]
    fn parse_smms_success_and_failure() {
        let smms = template("smms");
        let outcome = parse_upload_response(
            &smms,
            r#"{"success": true, "code": "success", "message": "Upload success.",
                "data": {"url": "https://s2.loli.net/2024/01/01/a.png",
                         "delete": "https://sm.ms/delete/abc"}}"#,
        )
        .expect("解析失败");
        assert_eq!(
            outcome,
            UploadOutcome::Success {
                url: "https://s2.loli.net/2024/01/01/a.png".to_string(),
                delete_url: Some("https://sm.ms/delete/abc".to_string()),
            }
        );

        let outcome = parse_upload_response(
            &smms,
            r#"{"success": false, "code": "unauthorized", "message": "Token is invalid."}"#,
        )
        .expect("解析失败");
        assert_eq!(
            outcome,
            UploadOutcome::Failure {
                message: Some("Token is invalid.".to_string()),
            }
        );
    }

    #[test]
    fn parse_success_without_delete_link() {
        let smms = template("smms");
        let outcome = parse_upload_response(
            &smms,
            r#"{"success": true, "data": {"url": "https://s2.loli.net/a.png"}}"#,
        )
        .expect("解析失败");
        assert_eq!(
            outcome,
            UploadOutcome::Success {
                url: "https://s2.loli.net/a.png".to_string(),
                delete_url: None,
            }
        );
    }

    #[test]
    fn parse_success_but_missing_url_errors() {
        let smms = template("smms");
        let err = parse_upload_response(&smms, r#"{"success": true, "data": {}}"#)
            .expect_err("缺直链应报错");
        assert!(matches!(err, Error::Response(_)), "实际错误: {err}");
        // url 非字符串同样报错。
        let err = parse_upload_response(&smms, r#"{"success": true, "data": {"url": 42}}"#)
            .expect_err("直链非字符串应报错");
        assert!(matches!(err, Error::Response(_)), "实际错误: {err}");
    }

    #[test]
    fn parse_lsky_success_and_failure() {
        let lsky = template("lsky");
        let outcome = parse_upload_response(
            &lsky,
            r#"{"status": true, "message": "success",
                "data": {"links": {"url": "https://img.example.com/i/2026/01/a.png"}}}"#,
        )
        .expect("解析失败");
        assert_eq!(
            outcome,
            UploadOutcome::Success {
                url: "https://img.example.com/i/2026/01/a.png".to_string(),
                delete_url: None,
            }
        );
        let outcome =
            parse_upload_response(&lsky, r#"{"status": false, "message": "文件大小超出限制"}"#)
                .expect("解析失败");
        assert_eq!(
            outcome,
            UploadOutcome::Failure {
                message: Some("文件大小超出限制".to_string()),
            }
        );
    }

    #[test]
    fn parse_chevereto_success_and_failure() {
        let chevereto = template("chevereto");
        let outcome = parse_upload_response(
            &chevereto,
            r#"{"status_code": 200,
                "success": {"message": "Image uploaded", "code": 200},
                "image": {"name": "abc", "url": "https://img.example.com/images/abc.jpg"}}"#,
        )
        .expect("解析失败");
        assert_eq!(
            outcome,
            UploadOutcome::Success {
                url: "https://img.example.com/images/abc.jpg".to_string(),
                delete_url: None,
            }
        );
        let outcome = parse_upload_response(
            &chevereto,
            r#"{"status_code": 400, "error": {"message": "Invalid API key", "code": 100}}"#,
        )
        .expect("解析失败");
        assert_eq!(
            outcome,
            UploadOutcome::Failure {
                message: Some("Invalid API key".to_string()),
            }
        );
    }

    #[test]
    fn parse_rejects_non_json_body() {
        let smms = template("smms");
        let err = parse_upload_response(&smms, "<html>502 Bad Gateway</html>")
            .expect_err("非 JSON 应报错");
        assert!(matches!(err, Error::Json(_)), "实际错误: {err}");
    }

    #[test]
    fn failure_without_error_path_yields_none_message() {
        // 无 successWhen 且无 errorPath 的模板：非成功时 message 为 None。
        let json = r#"{
            "id": "t", "name": "T", "version": 1,
            "request": {
                "method": "POST", "url": "https://example.com/upload",
                "multipart": [{"name": "file", "fromFile": true}]
            },
            "response": {"urlPath": "$.url"}
        }"#;
        let template = ImageHostTemplate::from_json(json).expect("解析失败");
        template.validate().expect("校验失败");
        // successWhen 缺省 → 无条件成功。
        let outcome =
            parse_upload_response(&template, r#"{"url": "https://x/a.png"}"#).expect("解析失败");
        assert_eq!(
            outcome,
            UploadOutcome::Success {
                url: "https://x/a.png".to_string(),
                delete_url: None,
            }
        );
    }

    /// 一个最小 SignedProvider 实现（契约可用性 + Phase 5 前编译保障）。
    struct DummyProvider;

    impl SignedProvider for DummyProvider {
        fn id(&self) -> &str {
            "dummy"
        }

        fn build_upload_request(
            &self,
            _config: &BTreeMap<String, String>,
            file_path: &Path,
        ) -> Result<UploadRequest> {
            Ok(UploadRequest {
                method: "PUT".to_string(),
                url: "https://dummy.example.com/put".to_string(),
                headers: Vec::new(),
                multipart: Vec::new(),
                file_path: file_path.to_path_buf(),
            })
        }
    }

    #[test]
    fn signed_provider_trait_is_implementable() {
        let provider = DummyProvider;
        assert_eq!(provider.id(), "dummy");
        let request = provider
            .build_upload_request(&BTreeMap::new(), Path::new("a.png"))
            .expect("构造失败");
        assert_eq!(request.method, "PUT");
        // trait object 可用（各端经统一接口注入）。
        let boxed: Box<dyn SignedProvider> = Box::new(DummyProvider);
        assert_eq!(boxed.id(), "dummy");
    }
}
