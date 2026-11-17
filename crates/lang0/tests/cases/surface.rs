//! 模块公开面：枚举与推导。
//! 参考：docs/README.md（模块、Interface）。

use super::support::*;
use serde_json::json;

fn root(files: &[(&str, &str)]) -> (Contents, String) {
    let mut read = Contents::default();
    let code = read.code(files);
    (read, code)
}

#[test]
fn modules_lists_dotted_names_and_surface_lists_public_items() {
    let (mut read, _) = root(&[]);
    let leaf = read.put_source("def deep() -> None:\n    pass\n");
    let init = read.put_source("");
    let pkg = read.put_json(json!({"leaf.py": leaf, "__init__.py": init}));
    let util = read.put_source(
        "\"\"\"工具。\n\n更多。\"\"\"\nfrom typing import TypedDict\nfrom croon import Port, Schema\n\nPair = TypedDict(\"Pair\", {\"a\": int, \"b\": str})\nAsk = Port(Pair)\nName = Schema({\"type\": \"string\"})\n_hidden = Schema({\"type\": \"null\"})\n\n\ndef add(x: int, y: int = 2) -> int:\n    \"\"\"加起来。\n\n    Args:\n        x: 第一个数。\n    \"\"\"\n    return x + y\n\n\ndef _private() -> None:\n    pass\n",
    );
    let top = read.put_json(json!({"util.py": util, "pkg": {"dir": pkg}}));
    assert_eq!(
        lang0::modules(&top, &read).unwrap(),
        vec!["pkg", "pkg.leaf", "util"]
    );
    let surface = lang0::surface(&top, "util", &read).unwrap();
    assert_eq!(surface["module"], "util");
    assert_eq!(surface["description"], "工具。\n\n更多。");
    let items = surface["items"].as_array().unwrap();
    let names: Vec<_> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Pair", "Ask", "Name", "add"]);
    assert_eq!(items[0]["kind"], "schema");
    assert_eq!(items[0]["spec"]["required"], json!(["a", "b"]));
    assert_eq!(items[1]["kind"], "port");
    assert_eq!(items[2]["spec"], json!({"type": "string"}));
    let add = &items[3];
    assert_eq!(add["kind"], "function");
    assert_eq!(add["description"], "加起来。");
    assert_eq!(add["returns"], "int");
    assert_eq!(
        add["params"],
        json!([
            {"name": "x", "type": "int", "description": "第一个数。"},
            {"name": "y", "type": "int", "default": "2"}
        ])
    );
    croon_eval::schema::validate(&lang0_block("lang0.surface"), &surface).unwrap();
}

fn lang0_block(name: &str) -> serde_json::Value {
    lang0::blocks_json(name)
}

#[test]
fn a_broken_module_is_reported_with_its_position() {
    let (mut read, _) = root(&[]);
    let bad = read.put_source("def f(:\n");
    let top = read.put_json(json!({"bad.py": bad}));
    let error = lang0::surface(&top, "bad", &read).unwrap_err().to_string();
    assert!(error.starts_with("/body/modules/bad.py:"), "{error}");
}

#[test]
fn one_undecidable_declaration_does_not_spoil_the_rest() {
    let (mut read, _) = root(&[]);
    let m = read.put_source("from typing import TypedDict\n\nBad = TypedDict(\"Bad\", {\"x\": list})\n\n\ndef ok() -> None:\n    pass\n");
    let top = read.put_json(json!({"m.py": m}));
    let surface = lang0::surface(&top, "m", &read).unwrap();
    let items = surface["items"].as_array().unwrap();
    assert!(items[0]["error"].is_string(), "{items:?}");
    assert_eq!(items[1]["name"], "ok");
}
