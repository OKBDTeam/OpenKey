//! 图床模板：声明式 HTTP 上传描述（docs/02 3.8 / 附录 C）。
//!
//! 模板即数据（JSON）：随组件包（components）分发、可热更，覆盖 token 类
//! 图床；签名类图床（OSS/COS/S3）走内置 [`crate::engine::SignedProvider`]。
//! 内置首发 SM.MS / 兰空 Lsky Pro / Chevereto 三个自制模板
//! （`templates/*.json`，自制内容，DR-001）。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::expr;

/// 图床上传模板。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageHostTemplate {
    /// 模板标识（smms / lsky / chevereto）。
    pub id: String,
    /// 展示名（设置 UI 用）。
    pub name: String,
    /// 模板版本（递增，随组件包热更）。
    pub version: u32,
    /// 请求描述。
    pub request: TemplateRequest,
    /// 响应描述。
    pub response: TemplateResponse,
    /// 凭据表单定义（可选；无占位符的模板可省）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<TemplateCredential>,
}

/// 请求描述。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateRequest {
    /// HTTP 方法（POST / PUT）。
    pub method: String,
    /// 上传端点（支持 `{host}`、`{token}` 等占位符）。
    pub url: String,
    /// 请求头模板（值支持占位符）。
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// multipart 字段：文件字段（`fromFile`）或静态字段（`value`）。
    #[serde(default)]
    pub multipart: Vec<MultipartField>,
}

/// multipart 字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultipartField {
    /// 字段名。
    pub name: String,
    /// 是否发送待上传文件（`fromFile`，默认 false）。
    #[serde(default, rename = "fromFile")]
    pub from_file: bool,
    /// 静态字段值（支持占位符；`fromFile` 为真时忽略）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// 响应描述（JSON 键名为 camelCase，docs/02 附录 C）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateResponse {
    /// 图片直链的 JSONPath（如 `$.data.url`）。
    pub url_path: String,
    /// 删除链接的 JSONPath（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_path: Option<String>,
    /// 错误信息的 JSONPath（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_path: Option<String>,
    /// 成功判定表达式（可选；缺省视为无条件成功）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_when: Option<String>,
}

/// 凭据表单定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateCredential {
    /// 字段定义（按表单顺序）。
    #[serde(default)]
    pub fields: Vec<CredentialField>,
}

/// 凭据字段类型（docs/02 附录 C：token/password/text）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialKind {
    /// API Token（密码框）。
    Token,
    /// 密码（密码框）。
    Password,
    /// 普通文本（如服务器地址）。
    Text,
}

/// 单个凭据字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialField {
    /// 字段名（同时是占位符名，如 `token` / `host`）。
    pub name: String,
    /// 字段类型（决定设置 UI 的输入框形态）。
    pub kind: CredentialKind,
    /// 展示标签（可选，缺省用字段名）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 是否必填（默认必填）。
    #[serde(default = "default_true")]
    pub required: bool,
}

/// serde 默认值助手（`required` 缺省 true）。
fn default_true() -> bool {
    true
}

impl ImageHostTemplate {
    /// 从 JSON 文本解析（不做语义校验）。
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// 语义校验（docs/02 附录 C）：标识非空、方法合法、URL 为 http(s)、
    /// JSONPath 与 `successWhen` 表达式可解析、multipart 恰有一个文件字段、
    /// 占位符均已在 `credential.fields` 定义。
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(Error::Template("id 为空".to_string()));
        }
        if self.name.is_empty() {
            return Err(Error::Template(format!("模板 {} name 为空", self.id)));
        }
        if self.version == 0 {
            return Err(Error::Template(format!("模板 {} version 须 ≥ 1", self.id)));
        }
        let method = self.request.method.to_ascii_uppercase();
        if method != "POST" && method != "PUT" {
            return Err(Error::Template(format!(
                "模板 {} 仅支持 POST/PUT，实际 {}",
                self.id, self.request.method
            )));
        }
        if !is_http_url(&self.request.url) {
            return Err(Error::Template(format!(
                "模板 {} url 须为 http(s):// 开头: {}",
                self.id, self.request.url
            )));
        }
        if self.request.headers.keys().any(String::is_empty) {
            return Err(Error::Template(format!("模板 {} 含空请求头名", self.id)));
        }
        let mut file_fields = 0usize;
        for field in &self.request.multipart {
            if field.name.is_empty() {
                return Err(Error::Template(format!(
                    "模板 {} 含空 multipart 字段名",
                    self.id
                )));
            }
            if field.from_file {
                file_fields += 1;
            } else if field.value.is_none() {
                return Err(Error::Template(format!(
                    "模板 {} 的 multipart 字段 {} 既非文件也无静态值",
                    self.id, field.name
                )));
            }
        }
        if file_fields != 1 {
            return Err(Error::Template(format!(
                "模板 {} 须恰有一个文件字段（fromFile），实际 {file_fields}",
                self.id
            )));
        }
        crate::jsonpath::validate(&self.response.url_path)
            .map_err(|e| Error::Template(format!("模板 {} urlPath 非法: {e}", self.id)))?;
        for (kind, path) in [
            ("deletePath", self.response.delete_path.as_deref()),
            ("errorPath", self.response.error_path.as_deref()),
        ] {
            if let Some(path) = path {
                crate::jsonpath::validate(path).map_err(|e| {
                    Error::Template(format!("模板 {} 的 {kind} 非法: {e}", self.id))
                })?;
            }
        }
        if let Some(expr) = &self.response.success_when {
            expr::validate(expr)
                .map_err(|e| Error::Template(format!("模板 {} successWhen 非法: {e}", self.id)))?;
        }
        // 占位符 ↔ 凭据字段交叉校验（防模板笔误运行时才炸）。
        if let Some(credential) = &self.credential {
            for field in &credential.fields {
                if field.name.is_empty() {
                    return Err(Error::Template(format!("模板 {} 含空凭据字段名", self.id)));
                }
            }
        }
        let placeholders = self.placeholders();
        let defined: BTreeSet<&str> = self
            .credential
            .iter()
            .flat_map(|c| c.fields.iter())
            .map(|f| f.name.as_str())
            .collect();
        for name in &placeholders {
            if !defined.contains(name.as_str()) {
                return Err(Error::Template(format!(
                    "模板 {} 使用未定义的占位符 {{{name}}}",
                    self.id
                )));
            }
        }
        Ok(())
    }

    /// 模板中出现的全部占位符名（`{name}`，URL / 请求头 / 静态 multipart 值）。
    pub fn placeholders(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        scan_placeholders(&self.request.url, &mut out);
        for value in self.request.headers.values() {
            scan_placeholders(value, &mut out);
        }
        for field in &self.request.multipart {
            if let Some(value) = &field.value {
                scan_placeholders(value, &mut out);
            }
        }
        out
    }
}

/// 是否为 http(s) URL（占位符可出现在 `{host}` 等位置）。
fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// 扫描文本中的 `{name}` 占位符（值内容不会被二次扫描，无注入面）。
fn scan_placeholders(text: &str, out: &mut BTreeSet<String>) {
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break; // 未闭合：交给 build 阶段的占位符替换报错
        };
        let name = &after[..end];
        if !name.is_empty() {
            out.insert(name.to_string());
        }
        rest = &after[end + 1..];
    }
}

/// 内置模板（自制数据，随 crate 发布；亦可经组件包热更覆盖）。
const BUILTIN_TEMPLATES: &[(&str, &str)] = &[
    ("smms", include_str!("../templates/smms.json")),
    ("lsky", include_str!("../templates/lsky.json")),
    ("chevereto", include_str!("../templates/chevereto.json")),
];

/// 内置模板 ID 列表（顺序同 [`builtin_templates`]）。
pub fn builtin_template_ids() -> Vec<&'static str> {
    BUILTIN_TEMPLATES.iter().map(|(id, _)| *id).collect()
}

/// 解析并校验全部内置模板。
pub fn builtin_templates() -> Result<Vec<ImageHostTemplate>> {
    BUILTIN_TEMPLATES
        .iter()
        .map(|(key, json)| load_builtin(key, json))
        .collect()
}

/// 按 ID 解析并校验单个内置模板（无此 ID 返回 `None`）。
pub fn builtin_template(id: &str) -> Result<Option<ImageHostTemplate>> {
    match BUILTIN_TEMPLATES.iter().find(|(key, _)| *key == id) {
        Some((key, json)) => Ok(Some(load_builtin(key, json)?)),
        None => Ok(None),
    }
}

/// 加载内置模板并校验（ID 键与 JSON `id` 一致性防护笔误）。
fn load_builtin(key: &str, json: &str) -> Result<ImageHostTemplate> {
    let template = ImageHostTemplate::from_json(json)?;
    template.validate()?;
    if template.id != key {
        return Err(Error::Template(format!(
            "内置模板键 {key} 与 JSON id {} 不一致",
            template.id
        )));
    }
    Ok(template)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_parse_and_validate() {
        assert_eq!(builtin_template_ids(), vec!["smms", "lsky", "chevereto"]);
        let templates = builtin_templates().expect("内置模板应全部通过");
        assert_eq!(templates.len(), 3);
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["smms", "lsky", "chevereto"]);
        // 逐个抽查关键字段。
        let smms = builtin_template("smms")
            .expect("查询失败")
            .expect("应有 smms 模板");
        assert_eq!(smms.name, "SM.MS");
        assert_eq!(smms.request.method, "POST");
        assert_eq!(
            smms.request
                .headers
                .get("Authorization")
                .map(String::as_str),
            Some("{token}")
        );
        assert_eq!(smms.response.url_path, "$.data.url");
        assert_eq!(
            smms.response.success_when.as_deref(),
            Some("success == true")
        );
        assert_eq!(smms.placeholders(), BTreeSet::from(["token".to_string()]));
        assert!(builtin_template("nonexistent").expect("查询失败").is_none());
    }

    #[test]
    fn from_json_roundtrip_preserves_camel_case() {
        let smms = builtin_template("smms")
            .expect("查询失败")
            .expect("应有 smms 模板");
        let json = serde_json::to_string(&smms).expect("序列化失败");
        let back = ImageHostTemplate::from_json(&json).expect("反序列化失败");
        assert_eq!(smms, back);
        // camelCase 键名（附录 C）。
        assert!(json.contains("\"urlPath\""), "序列化应保持 urlPath 键名");
        assert!(json.contains("\"successWhen\""));
        assert!(json.contains("\"fromFile\""));
    }

    /// 最小合法模板 JSON（供变异测试）。
    fn base_template_json() -> String {
        r#"{
            "id": "t",
            "name": "测试图床",
            "version": 1,
            "request": {
                "method": "POST",
                "url": "https://example.com/upload",
                "headers": {"Authorization": "Bearer {token}"},
                "multipart": [{"name": "file", "fromFile": true}]
            },
            "response": {
                "urlPath": "$.data.url",
                "successWhen": "code == 200"
            },
            "credential": {
                "fields": [{"name": "token", "kind": "password"}]
            }
        }"#
        .to_string()
    }

    #[test]
    fn validate_accepts_minimal_template() {
        let template = ImageHostTemplate::from_json(&base_template_json()).expect("解析失败");
        template.validate().expect("校验失败");
    }

    #[test]
    fn validate_rejects_broken_templates() {
        let cases: Vec<(&str, String)> = vec![
            ("空 id", base_template_json().replace("\"id\": \"t\"", "\"id\": \"\"")),
            ("version 0", base_template_json().replace("\"version\": 1", "\"version\": 0")),
            ("方法非法", base_template_json().replace("\"POST\"", "\"GET\"")),
            ("URL 非 http", base_template_json().replace("https://example.com", "ftp://example.com")),
            ("urlPath 非法", base_template_json().replace("$.data.url", "data.url")),
            ("successWhen 非法", base_template_json().replace("code == 200", "code = 200")),
            ("无文件字段", base_template_json().replace("{\"name\": \"file\", \"fromFile\": true}", "{\"name\": \"file\", \"value\": \"x\"}")),
            ("两个文件字段", base_template_json().replace(
                "\"multipart\": [{\"name\": \"file\", \"fromFile\": true}]",
                "\"multipart\": [{\"name\": \"file\", \"fromFile\": true}, {\"name\": \"file2\", \"fromFile\": true}]",
            )),
            ("静态字段无值", base_template_json().replace(
                "\"multipart\": [{\"name\": \"file\", \"fromFile\": true}]",
                "\"multipart\": [{\"name\": \"file\", \"fromFile\": true}, {\"name\": \"extra\"}]",
            )),
            ("占位符未定义", base_template_json().replace("Bearer {token}", "Bearer {tken}")),
            ("空字段名", base_template_json().replace("\"name\": \"token\"", "\"name\": \"\"")),
        ];
        for (label, json) in cases {
            let template = ImageHostTemplate::from_json(&json)
                .unwrap_or_else(|e| panic!("{label}: JSON 解析应成功: {e}"));
            assert!(template.validate().is_err(), "{label}: 应被 validate 拒绝");
        }
    }

    #[test]
    fn placeholders_collected_from_all_slots() {
        let json = r#"{
            "id": "t",
            "name": "T",
            "version": 1,
            "request": {
                "method": "POST",
                "url": "https://{host}/upload",
                "headers": {"Authorization": "{token}", "X-Account": "{account}"},
                "multipart": [
                    {"name": "file", "fromFile": true},
                    {"name": "album", "value": "{album_id}"},
                    {"name": "format", "value": "json"}
                ]
            },
            "response": {"urlPath": "$.data.url"},
            "credential": {"fields": [
                {"name": "host", "kind": "text"},
                {"name": "token", "kind": "token"},
                {"name": "account", "kind": "text"},
                {"name": "album_id", "kind": "text"}
            ]}
        }"#;
        let template = ImageHostTemplate::from_json(json).expect("解析失败");
        template.validate().expect("校验失败");
        assert_eq!(
            template.placeholders(),
            BTreeSet::from([
                "host".to_string(),
                "token".to_string(),
                "account".to_string(),
                "album_id".to_string(),
            ])
        );
    }

    #[test]
    fn unclosed_placeholder_left_for_engine() {
        // 未闭合占位符：validate 不认（扫描不到），构造请求时报错。
        let json = r#"{
            "id": "t", "name": "T", "version": 1,
            "request": {
                "method": "POST",
                "url": "https://example.com/{token/upload",
                "multipart": [{"name": "file", "fromFile": true}]
            },
            "response": {"urlPath": "$.data.url"}
        }"#;
        let template = ImageHostTemplate::from_json(json).expect("解析失败");
        template.validate().expect("未闭合占位符不在校验范围");
        assert!(template.placeholders().is_empty());
    }
}
