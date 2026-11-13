//! 生成模块的渲染：把 JSON 写成 Lang0 里的 `Port(...)`、`Schema(...)` 字面量。
//! 参考：docs/README.md（模块、Port 与交出）；croon-app:docs/Library/README.md「地图」。
//!
//! 这里不认识任何具体的 Port：Kernel 有哪几个 Port、模型服务的形状是什么，由提供那些形状的
//! 一方给出。所以它把「谁有什么」与「Lang0 怎么写」分开，一份形状只有一个出处。

use serde_json::Value as Json;

/// 一行声明：`名字 = Port(spec)` 或 `名字 = Schema(spec)`。
pub struct Decl<'a> {
    pub name: &'a str,
    pub spec: &'a Json,
    pub schema: bool,
}

/// 一整个模块：`preamble` 是说明与 import，逐字给出；之后每行一份声明。
pub fn module(preamble: &str, decls: &[Decl]) -> String {
    let mut out = String::from(preamble);
    for decl in decls {
        let kind = if decl.schema { "Schema" } else { "Port" };
        out.push_str(&format!("{} = {kind}({})\n", decl.name, literal(decl.spec)));
    }
    out
}

/// JSON → Python 的字面量：键按 JSON 的顺序，true、false、null 写成 True、False、None。
fn literal(v: &Json) -> String {
    match v {
        Json::Null => "None".into(),
        Json::Bool(b) => if *b { "True" } else { "False" }.into(),
        Json::Number(n) => n.to_string(),
        Json::String(s) => serde_json::to_string(s).unwrap(),
        Json::Array(a) => format!("[{}]", a.iter().map(literal).collect::<Vec<_>>().join(", ")),
        Json::Object(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, x)| format!("{}: {}", serde_json::to_string(k).unwrap(), literal(x)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_declaration_renders_its_kind_and_spec() {
        let spec = json!({"description": "d", "in": {"type": "null"}, "attach": {"additionalProperties": false}});
        let text = module(
            "from croon import Port\n\n",
            &[Decl {
                name: "Revoke",
                spec: &spec,
                schema: false,
            }],
        );
        // serde_json 的 Map 按字典序，所以键的顺序与文档里写的不一定相同；重放时逐字相同即可。
        assert_eq!(
            text,
            "from croon import Port\n\nRevoke = Port({\"attach\": {\"additionalProperties\": False}, \"description\": \"d\", \"in\": {\"type\": \"null\"}})\n"
        );
    }

    #[test]
    fn a_schema_declaration_uses_schema() {
        let spec = json!({"type": "null"});
        let text = module(
            "",
            &[Decl {
                name: "Outcome",
                spec: &spec,
                schema: true,
            }],
        );
        assert_eq!(text, "Outcome = Schema({\"type\": \"null\"})\n");
    }

    #[test]
    fn booleans_and_nulls_use_python_spelling() {
        assert_eq!(
            literal(&json!({"a": true, "b": null})),
            "{\"a\": True, \"b\": None}"
        );
    }
}
