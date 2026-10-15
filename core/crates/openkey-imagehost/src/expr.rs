//! 受限成功判定表达式（docs/02 3.8 / 附录 C `successWhen`）。
//!
//! 文法（递归下降；优先级 `||` < `&&` < 比较）：
//! ```text
//! expr    := or
//! or      := and ("||" and)*
//! and     := cmp ("&&" cmp)*
//! cmp     := "(" expr ")" | operand (("=="|"!="|"<"|"<="|">"|">=") operand)?
//! operand := ident | int | string | true | false
//! ```
//! - 标识符在响应 JSON 上按 `$.{ident}` 求值（docs/02 3.8.2：`code == 200`
//!   中的 `code` 即响应顶层字段）；缺失/非标量按 null 处理；
//! - 字符串支持单/双引号；`true`/`false` 为布尔字面量（Lsky 的
//!   `status == true` 需要）；
//! - `==`/`!=` 跨类型恒不（相）等；`<`/`<=`/`>`/`>=` 仅同型（数值/字符串）
//!   比较，异型为 false；
//! - `&&`/`||` 操作数须为布尔值，否则求值报错（短路求值）；
//! - 整体表达式结果须为布尔值（裸非布尔操作数报错）。

use std::cmp::Ordering;

use serde_json::Value;

use crate::error::{Error, Result};

/// 校验表达式语法（模板校验期调用，不做求值）。
pub fn validate(expr: &str) -> Result<()> {
    parse(expr).map(|_| ())
}

/// 对响应 JSON 求值，返回布尔结果。
pub fn eval(expr: &str, context: &Value) -> Result<bool> {
    let ast = parse(expr)?;
    eval_ast(&ast, context)?.as_bool()
}

/// 求值中间值。
#[derive(Debug, Clone)]
enum EvalValue {
    Null,
    Num(f64),
    Str(String),
    Bool(bool),
}

impl EvalValue {
    /// 逻辑运算/整体结果要求布尔值。
    fn as_bool(&self) -> Result<bool> {
        match self {
            EvalValue::Bool(b) => Ok(*b),
            _ => Err(Error::Expr("表达式结果须为布尔值".to_string())),
        }
    }
}

/// 语法树。
#[derive(Debug)]
enum Ast {
    Or(Box<Ast>, Box<Ast>),
    And(Box<Ast>, Box<Ast>),
    Cmp(Atom, CmpOp, Atom),
    /// 裸操作数（如 `status`），求值后按布尔要求校验。
    Bare(Atom),
}

/// 比较运算符。
#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// 操作数。
#[derive(Debug)]
enum Atom {
    Ident(String),
    Int(i64),
    Str(String),
    Bool(bool),
}

/// 词法单元。
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Int(i64),
    Str(String),
    Bool(bool),
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    LParen,
    RParen,
}

/// 解析为语法树（语法错误即 [`Error::Expr`]）。
fn parse(input: &str) -> Result<Ast> {
    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err(Error::Expr(format!("表达式为空: {input}")));
    }
    let mut parser = Parser {
        tokens,
        pos: 0,
        input: input.to_string(),
    };
    let ast = parser.parse_or()?;
    if parser.pos != parser.tokens.len() {
        return Err(Error::Expr(format!(
            "表达式存在无法解析的内容: {}（剩余 {:?}）",
            input,
            &parser.tokens[parser.pos..]
        )));
    }
    Ok(ast)
}

/// 分词：标识符 / 整数 / 字符串（单双引号）/ 运算符 / 括号；跳过空白。
fn tokenize(input: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            '&' => {
                chars.next();
                if chars.peek().map(|&(_, c)| c) == Some('&') {
                    chars.next();
                    tokens.push(Token::AndAnd);
                } else {
                    return Err(Error::Expr(format!(
                        "位置 {i}: 期望 &&（不支持单 &）: {input}"
                    )));
                }
            }
            '|' => {
                chars.next();
                if chars.peek().map(|&(_, c)| c) == Some('|') {
                    chars.next();
                    tokens.push(Token::OrOr);
                } else {
                    return Err(Error::Expr(format!(
                        "位置 {i}: 期望 ||（不支持单 |）: {input}"
                    )));
                }
            }
            '=' => {
                chars.next();
                if chars.peek().map(|&(_, c)| c) == Some('=') {
                    chars.next();
                    tokens.push(Token::Eq);
                } else {
                    return Err(Error::Expr(format!(
                        "位置 {i}: 期望 ==（赋值不支持）: {input}"
                    )));
                }
            }
            '!' => {
                chars.next();
                if chars.peek().map(|&(_, c)| c) == Some('=') {
                    chars.next();
                    tokens.push(Token::Ne);
                } else {
                    return Err(Error::Expr(format!(
                        "位置 {i}: 期望 !=（不支持单 !）: {input}"
                    )));
                }
            }
            '<' => {
                chars.next();
                if chars.peek().map(|&(_, c)| c) == Some('=') {
                    chars.next();
                    tokens.push(Token::Le);
                } else {
                    tokens.push(Token::Lt);
                }
            }
            '>' => {
                chars.next();
                if chars.peek().map(|&(_, c)| c) == Some('=') {
                    chars.next();
                    tokens.push(Token::Ge);
                } else {
                    tokens.push(Token::Gt);
                }
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            '"' | '\'' => {
                let quote = c;
                chars.next(); // 起始引号
                let mut text = String::new();
                let mut closed = false;
                while let Some(&(_, cc)) = chars.peek() {
                    chars.next();
                    if cc == quote {
                        closed = true;
                        break;
                    }
                    text.push(cc);
                }
                if !closed {
                    return Err(Error::Expr(format!("位置 {i}: 字符串未闭合: {input}")));
                }
                tokens.push(Token::Str(text));
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while chars.peek().is_some_and(|&(_, cc)| cc.is_ascii_digit()) {
                    chars.next();
                }
                let end = chars.peek().map_or(input.len(), |&(j, _)| j);
                let number: i64 = input[start..end]
                    .parse()
                    .map_err(|_| Error::Expr(format!("位置 {start}: 整数超出范围: {input}")))?;
                tokens.push(Token::Int(number));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while chars
                    .peek()
                    .is_some_and(|&(_, cc)| cc.is_ascii_alphanumeric() || cc == '_')
                {
                    chars.next();
                }
                let end = chars.peek().map_or(input.len(), |&(j, _)| j);
                match &input[start..end] {
                    "true" => tokens.push(Token::Bool(true)),
                    "false" => tokens.push(Token::Bool(false)),
                    word => tokens.push(Token::Ident(word.to_string())),
                }
            }
            _ => {
                return Err(Error::Expr(format!("位置 {i}: 非法字符 {c:?}: {input}")));
            }
        }
    }
    Ok(tokens)
}

/// 递归下降parser。
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    input: String,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, expect: &Token) -> bool {
        if self.peek() == Some(expect) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn error(&self, msg: &str) -> Error {
        Error::Expr(format!("{msg}: {}", self.input))
    }

    fn parse_or(&mut self) -> Result<Ast> {
        let mut left = self.parse_and()?;
        while self.eat(&Token::OrOr) {
            let right = self.parse_and()?;
            left = Ast::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Ast> {
        let mut left = self.parse_cmp()?;
        while self.eat(&Token::AndAnd) {
            let right = self.parse_cmp()?;
            left = Ast::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cmp(&mut self) -> Result<Ast> {
        // 括号布尔子式（优先级最低，直接作为一层返回）。
        if self.eat(&Token::LParen) {
            let inner = self.parse_or()?;
            if !self.eat(&Token::RParen) {
                return Err(self.error("期望 ) 闭合括号"));
            }
            return Ok(inner);
        }
        let left = self.parse_atom()?;
        let op = match self.peek() {
            Some(Token::Eq) => Some(CmpOp::Eq),
            Some(Token::Ne) => Some(CmpOp::Ne),
            Some(Token::Lt) => Some(CmpOp::Lt),
            Some(Token::Le) => Some(CmpOp::Le),
            Some(Token::Gt) => Some(CmpOp::Gt),
            Some(Token::Ge) => Some(CmpOp::Ge),
            _ => None,
        };
        match op {
            Some(op) => {
                self.pos += 1;
                let right = self.parse_atom()?;
                Ok(Ast::Cmp(left, op, right))
            }
            None => Ok(Ast::Bare(left)),
        }
    }

    fn parse_atom(&mut self) -> Result<Atom> {
        match self.advance() {
            Some(Token::Ident(name)) => Ok(Atom::Ident(name)),
            Some(Token::Int(n)) => Ok(Atom::Int(n)),
            Some(Token::Str(s)) => Ok(Atom::Str(s)),
            Some(Token::Bool(b)) => Ok(Atom::Bool(b)),
            Some(token) => Err(self.error(&format!("期望操作数，得到 {token:?}"))),
            None => Err(self.error("表达式不完整，期望操作数")),
        }
    }
}

/// 求值（短路）。
fn eval_ast(ast: &Ast, context: &Value) -> Result<EvalValue> {
    match ast {
        Ast::Or(left, right) => {
            if eval_ast(left, context)?.as_bool()? {
                Ok(EvalValue::Bool(true))
            } else {
                Ok(EvalValue::Bool(eval_ast(right, context)?.as_bool()?))
            }
        }
        Ast::And(left, right) => {
            if !eval_ast(left, context)?.as_bool()? {
                Ok(EvalValue::Bool(false))
            } else {
                Ok(EvalValue::Bool(eval_ast(right, context)?.as_bool()?))
            }
        }
        Ast::Cmp(left, op, right) => {
            let lv = eval_atom(left, context);
            let rv = eval_atom(right, context);
            Ok(EvalValue::Bool(compare(*op, &lv, &rv)))
        }
        Ast::Bare(atom) => Ok(eval_atom(atom, context)),
    }
}

/// 操作数求值：标识符按 `$.{name}` 查响应 JSON（缺失/非标量 → null）。
fn eval_atom(atom: &Atom, context: &Value) -> EvalValue {
    match atom {
        Atom::Int(n) => EvalValue::Num(*n as f64),
        Atom::Str(s) => EvalValue::Str(s.clone()),
        Atom::Bool(b) => EvalValue::Bool(*b),
        Atom::Ident(name) => json_to_value(
            crate::jsonpath::query(context, &format!("$.{name}"))
                .ok()
                .flatten(),
        ),
    }
}

/// JSON 标量 → 求值（对象/数组/null 一律按 null）。
fn json_to_value(value: Option<&Value>) -> EvalValue {
    match value {
        None | Some(Value::Null) => EvalValue::Null,
        Some(Value::Bool(b)) => EvalValue::Bool(*b),
        Some(Value::Number(n)) => EvalValue::Num(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => EvalValue::Str(s.clone()),
        Some(_) => EvalValue::Null,
    }
}

/// 比较（`==`/`!=` 跨类型恒不（相）等；定序仅同型，异型 false）。
fn compare(op: CmpOp, left: &EvalValue, right: &EvalValue) -> bool {
    match op {
        CmpOp::Eq => values_equal(left, right),
        CmpOp::Ne => !values_equal(left, right),
        CmpOp::Lt => order(left, right).is_some_and(Ordering::is_lt),
        CmpOp::Le => order(left, right).is_some_and(Ordering::is_le),
        CmpOp::Gt => order(left, right).is_some_and(Ordering::is_gt),
        CmpOp::Ge => order(left, right).is_some_and(Ordering::is_ge),
    }
}

/// 同型定序（数值按数值、字符串按字典序）；异型 `None`。
fn order(left: &EvalValue, right: &EvalValue) -> Option<Ordering> {
    match (left, right) {
        (EvalValue::Num(a), EvalValue::Num(b)) => a.partial_cmp(b),
        (EvalValue::Str(a), EvalValue::Str(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// 相等（同型才可相等；null 仅等于 null）。
fn values_equal(left: &EvalValue, right: &EvalValue) -> bool {
    match (left, right) {
        (EvalValue::Num(a), EvalValue::Num(b)) => a == b,
        (EvalValue::Str(a), EvalValue::Str(b)) => a == b,
        (EvalValue::Bool(a), EvalValue::Bool(b)) => a == b,
        (EvalValue::Null, EvalValue::Null) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// SM.MS 风格响应。
    fn smms_context() -> Value {
        json!({
            "success": true,
            "code": "success",
            "message": "Upload success.",
            "data": {"url": "https://s2.loli.net/2024/01/01/a.png"}
        })
    }

    /// Lsky 风格响应。
    fn lsky_context() -> Value {
        json!({
            "status": true,
            "message": "success",
            "data": {"links": {"url": "https://img.example.com/i/a.png"}}
        })
    }

    #[test]
    fn validate_accepts_supported_forms() {
        for ok in [
            "code == 200",
            "code != 200",
            "code >= 200 && code < 300",
            "(code == 200 || code == 201) && message == \"ok\"",
            "message == 'success'",
            "status == true",
            "status",
            "code > 100",
        ] {
            assert!(validate(ok).is_ok(), "{ok} 应通过校验");
        }
    }

    #[test]
    fn validate_rejects_bad_forms() {
        for bad in [
            "",
            "   ",
            "code = 200",
            "code == ",
            "code === 200",
            "(code == 200",
            "code == 200)",
            "code & 1",
            "code | 1",
            "1 2",
            "code == 200 &&",
            "code == 200 extra",
            "code == \"unclosed",
            "code.<x> == 1",
            "@code == 1",
        ] {
            assert!(validate(bad).is_err(), "{bad:?} 应被拒绝");
        }
    }

    #[test]
    fn eval_integer_and_string_comparisons() {
        let ctx = json!({"code": 200, "message": "success"});
        assert!(eval("code == 200", &ctx).expect("求值失败"));
        assert!(!eval("code == 201", &ctx).expect("求值失败"));
        assert!(!eval("code != 200", &ctx).expect("求值失败"));
        assert!(eval("code > 100 && code < 300", &ctx).expect("求值失败"));
        assert!(eval("code >= 200", &ctx).expect("求值失败"));
        assert!(eval("code <= 200", &ctx).expect("求值失败"));
        assert!(!eval("code > 200", &ctx).expect("求值失败"));
        assert!(eval("message == \"success\"", &ctx).expect("求值失败"));
        assert!(eval("message == 'success'", &ctx).expect("求值失败"));
        assert!(!eval("message == \"other\"", &ctx).expect("求值失败"));
        // 字符串定序。
        assert!(eval("message < \"z\"", &ctx).expect("求值失败"));
        // 跨类型 == 恒假、!= 恒真。
        assert!(!eval("code == \"200\"", &ctx).expect("求值失败"));
        assert!(eval("code != \"200\"", &ctx).expect("求值失败"));
        // 跨类型定序为 false。
        assert!(!eval("code < \"z\"", &ctx).expect("求值失败"));
    }

    #[test]
    fn eval_logic_precedence_and_parens() {
        let ctx = json!({"code": 200, "message": "success"});
        // && 优先于 ||。
        assert!(eval("code == 500 && message == \"x\" || code == 200", &ctx).expect("求值失败"));
        assert!(!eval("code == 200 && message == \"x\" || code == 500", &ctx).expect("求值失败"));
        // 括号改变结合。
        assert!(!eval(
            "(code == 500 || code == 201) && message == \"success\"",
            &ctx
        )
        .expect("求值失败"));
        assert!(eval(
            "(code == 200 || code == 201) && message == \"success\"",
            &ctx
        )
        .expect("求值失败"));
        // 嵌套括号。
        assert!(eval("((code == 200))", &ctx).expect("求值失败"));
    }

    #[test]
    fn eval_bool_literals_and_bare_ident() {
        let ctx = lsky_context();
        assert!(eval("status == true", &ctx).expect("求值失败"));
        assert!(!eval("status == false", &ctx).expect("求值失败"));
        // 裸布尔标识符。
        assert!(eval("status", &ctx).expect("求值失败"));
        // 裸非布尔 → 求值错误。
        assert!(eval("message", &ctx).is_err());
        // 逻辑运算的操作数须布尔。
        assert!(eval("message && status", &ctx).is_err());
        assert!(eval("code && true", &ctx).is_err());
    }

    #[test]
    fn eval_missing_identifier_is_null() {
        let ctx = json!({"code": 200});
        assert!(!eval("missing == 1", &ctx).expect("求值失败"));
        assert!(eval("missing != 1", &ctx).expect("求值失败"));
        assert!(!eval("missing == true", &ctx).expect("求值失败"));
        // null 不参与定序。
        assert!(!eval("missing < 1", &ctx).expect("求值失败"));
    }

    #[test]
    fn eval_real_template_conditions() {
        assert!(eval("success == true", &smms_context()).expect("求值失败"));
        assert!(!eval(
            "success == true",
            &json!({"success": false, "message": "fail"})
        )
        .expect("求值失败"));
        assert!(eval("status == true", &lsky_context()).expect("求值失败"));
        // 短路：左侧已定胜负时不再求值右侧（右侧类型错误也不炸）。
        assert!(eval("success == true || message && 1", &smms_context()).expect("求值失败"));
        assert!(!eval("success == false && message && 1", &smms_context()).expect("求值失败"));
    }

    #[test]
    fn eval_rejects_syntax_error_at_runtime() {
        let ctx = json!({"code": 200});
        assert!(eval("code = 200", &ctx).is_err());
        assert!(eval("", &ctx).is_err());
    }
}
