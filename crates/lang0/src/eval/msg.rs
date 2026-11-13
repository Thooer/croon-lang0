//! 异常的消息：取自 Subset.md 的 lang0.messages，按 id 选类与模板，填上占位。
//! 参考：docs/Subset.md（异常的消息）。

use super::heap::Heap;
use std::sync::OnceLock;

use serde_json::Value as Json;

use super::value::{Exc, Value, exc_class};

fn messages() -> &'static Json {
    static M: OnceLock<Json> = OnceLock::new();
    M.get_or_init(|| crate::blocks::json("lang0.messages"))
}

/// 模板里的 `{名字}` 换成占位，`{{`、`}}` 是字面的花括号。
pub fn render(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let c = &rest[i..];
        if c.starts_with("{{") || c.starts_with("}}") {
            out.push_str(&c[..1]);
            rest = &c[2..];
        } else if c.starts_with('{') {
            let end = c.find('}').expect("模板的占位要闭合");
            let name = &c[1..end];
            let v = vars
                .iter()
                .find(|(k, _)| *k == name)
                .unwrap_or_else(|| panic!("消息模板 {template} 缺占位 {name}"));
            out.push_str(&v.1);
            rest = &c[end + 1..];
        } else {
            panic!("消息模板 {template} 里有单个的 }}");
        }
    }
    out.push_str(rest);
    out
}

fn entry(id: &str) -> &'static Json {
    messages()
        .get(id)
        .unwrap_or_else(|| panic!("lang0.messages 里没有 {id}"))
}

/// 一条消息的文本：`sel` 选按键分的那一个。
pub fn text(id: &str, sel: Option<&str>, vars: &[(&str, String)]) -> String {
    let m = &entry(id)["message"];
    let template = match (m, sel) {
        (Json::String(s), _) => s.as_str(),
        (Json::Object(o), Some(k)) => o[k].as_str().unwrap_or_else(|| panic!("{id} 没有 {k}")),
        _ => panic!("{id} 的消息要选一个键"),
    };
    render(template, vars)
}

/// 按 id 造出 Lang0 自己抛出的异常。
pub fn err(heap: &Heap, id: &str, sel: Option<&str>, vars: &[(&str, String)]) -> Value {
    let e = entry(id);
    let class = e["class"].as_str().unwrap();
    err_as(
        heap,
        class.strip_prefix("croon.").unwrap_or(class),
        id,
        sel,
        vars,
    )
}

/// 同上，类另给，如重复次数超出时的 OverflowError。
pub fn err_as(
    heap: &Heap,
    class: &str,
    id: &str,
    sel: Option<&str>,
    vars: &[(&str, String)],
) -> Value {
    let message = text(id, sel, vars);
    let kind = entry(id)
        .get("kind")
        .and_then(Json::as_str)
        .map(str::to_owned);
    exc(heap, class, vec![Value::str(heap, message)], kind)
}

pub fn exc(heap: &Heap, class: &str, args: Vec<Value>, kind: Option<String>) -> Value {
    let class = exc_class(class).unwrap_or_else(|| panic!("没有异常类 {class}"));
    Value::Exc(heap.alloc(Exc {
        class,
        args,
        kind,
        data: if class == "Failure" {
            serde_json::json!({"error":null,"dependencies":[]})
        } else {
            Json::Null
        },
    }))
}

/// KeyError：args 是那个键本身。
pub fn key_error(heap: &Heap, key: Value) -> Value {
    exc(heap, "KeyError", vec![key], None)
}

/// 超出资源的上限：`steps`、`depth` 或 `live`。
pub fn limit(heap: &Heap, which: &str) -> Value {
    let message = text("limit", Some(which), &[]);
    exc(heap, "Limit", vec![Value::str(heap, message)], None)
}

/// not_value 的原因与消息使用同一份文档模板。
pub fn value_reason(reason: &str, vars: &[(&str, String)]) -> String {
    render(
        entry("not_value")["reason"][reason]
            .as_str()
            .expect("not_value 的原因已声明"),
        vars,
    )
}
