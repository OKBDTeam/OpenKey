//! openkey-imagehost 错误类型。

/// 图床模板引擎错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 模板非法（结构缺字段 / 取值为空 / 路径或表达式无法解析）。
    #[error("模板非法: {0}")]
    Template(String),
    /// JSONPath 解析失败。
    #[error("JSONPath 错误: {0}")]
    JsonPath(String),
    /// 受限表达式解析/求值失败。
    #[error("表达式错误: {0}")]
    Expr(String),
    /// 响应体 JSON 解析失败。
    #[error("响应 JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    /// 占位符所需凭据缺失。
    #[error("缺少凭据字段: {0}")]
    MissingCredential(String),
    /// 响应解析失败（判定成功但缺图片直链等）。
    #[error("响应解析失败: {0}")]
    Response(String),
    /// 读取待上传文件失败。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;
