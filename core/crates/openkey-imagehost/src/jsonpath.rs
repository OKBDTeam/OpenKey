//! 极简 JSONPath 子集（docs/02 3.8 / 附录 C）。
//!
//! 仅支持 `$` 根与 `.key` 逐级对象取值：`$`（根）、`$.data.url`、
//! `$.data.links.url`。不支持数组下标、通配与函数——模板响应解析
//! 只需对象字段导航。键不存在返回 `Ok(None)`；路径语法非法返回 `Err`
//! （模板校验期即拦截，见 [`validate`]）。

use serde_json::Value;

use crate::error::{Error, Result};

/// 校验路径语法（模板校验期调用）。
pub fn validate(path: &str) -> Result<()> {
    parse_segments(path).map(|_| ())
}

/// 按路径从 `root` 取值；键不存在返回 `Ok(None)`，路径非法返回 `Err`。
pub fn query<'a>(root: &'a Value, path: &str) -> Result<Option<&'a Value>> {
    let segments = parse_segments(path)?;
    let mut current = root;
    for key in &segments {
        match current.get(key.as_str()) {
            Some(next) => current = next,
            None => return Ok(None),
        }
    }
    Ok(Some(current))
}

/// 解析 `$.a.b` 为 `["a", "b"]`；`$` 为空段表。
fn parse_segments(path: &str) -> Result<Vec<String>> {
    let rest = path
        .strip_prefix('$')
        .ok_or_else(|| Error::JsonPath(format!("路径须以 $ 开头: {path}")))?;
    if rest.is_empty() {
        return Ok(Vec::new()); // `$` = 根
    }
    let body = rest
        .strip_prefix('.')
        .ok_or_else(|| Error::JsonPath(format!("$ 之后须为 .key 形式: {path}")))?;
    let mut segments = Vec::new();
    for part in body.split('.') {
        if part.is_empty() {
            return Err(Error::JsonPath(format!("路径含空键名: {path}")));
        }
        segments.push(part.to_string());
    }
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn root_and_nested_paths() {
        let value = json!({"data": {"links": {"url": "https://img.example.com/a.png"}}});
        assert_eq!(query(&value, "$").expect("$ 查询失败"), Some(&value));
        assert_eq!(
            query(&value, "$.data.links.url").expect("查询失败"),
            Some(&json!("https://img.example.com/a.png"))
        );
        // 中间层同样可取到。
        assert_eq!(
            query(&value, "$.data.links").expect("查询失败"),
            Some(&json!({"url": "https://img.example.com/a.png"}))
        );
    }

    #[test]
    fn missing_key_is_none_not_error() {
        let value = json!({"code": 200});
        assert_eq!(query(&value, "$.data.url").expect("查询失败"), None);
        // 对非对象取值（标量上继续导航）同样为 None。
        assert_eq!(query(&value, "$.code.anything").expect("查询失败"), None);
    }

    #[test]
    fn malformed_paths_rejected() {
        for bad in ["data.url", "$data", "$.", "$.a..b", "$..a", "$.a."] {
            assert!(query(&json!({"a": 1}), bad).is_err(), "路径 {bad} 应被拒绝");
            assert!(validate(bad).is_err(), "validate({bad}) 应报错");
        }
        assert!(validate("$").is_ok());
        assert!(validate("$.data.links.url").is_ok());
    }
}
