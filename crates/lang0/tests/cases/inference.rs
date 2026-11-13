//! Interface 的机器预期以 Delegate 为主；这里检查推导规则的独立边角。
//! 参考：docs/README.md（Interface、值与类型、Port 与交出）。

use super::support::*;
use croon_eval::host::Language;
use lang0::Lang0;
use serde_json::json;

#[test]
fn annotations_preserve_field_union_literal_and_parameter_order() {
    let source = r#"from typing import TypedDict as TD, NotRequired as NR, Literal as Lit, Annotated as Ann, Any
Out = TD("Out", {"z": str | None, "a": NR[list[int]], "m": dict[str, Any]})
def main(z: Ann[str, "type description"], a: Lit["second", "first"] = "first") -> Out:
    """First line.

    Second paragraph.
    Args:
        z: Parameter description
            with continuation.
    """
    return {}
"#;
    let mut d = definition(source);
    d["interface"]["description"] = json!("First line.\n\nSecond paragraph.");
    d["interface"]["signature"] = json!({
        "in": {"type": "object", "properties": {
            "z": {"type": "string", "description": "Parameter description with continuation."},
            "a": {"enum": ["second", "first"]}
        }, "required": ["z"], "additionalProperties": false},
        "out": {"type": "object", "properties": {
            "z": {"anyOf": [{"type": "string"}, {"type": "null"}]},
            "a": {"type": "array", "items": {"type": "integer"}},
            "m": {"type": "object", "additionalProperties": {}}
        }, "required": ["z", "m"], "additionalProperties": false}
    });
    Lang0.check(&d, &Contents::default()).unwrap();
    d["interface"]["signature"]["out"]["required"] = json!(["m", "z"]);
    assert!(
        Lang0
            .check(&d, &Contents::default())
            .unwrap_err()
            .to_string()
            .contains("规范文本")
    );
}

#[test]
fn port_attach_and_inbox_share_specs_and_keep_optional_names_out_of_required() {
    let source = r#"from typing import NotRequired
from croon import Port, Inbox
Answer = Port(str, doc="answer")
Chat = Port(str | None, attach={"z": Answer, "a": NotRequired[Answer]}, doc="chat")
def main(to: Chat, inbox: Inbox(Chat)) -> None:
    pass
"#;
    let answer = json!({"description": "answer", "in": {"type": "string"}, "attach": {"additionalProperties": false}});
    let chat = json!({"description": "chat", "in": {"anyOf": [{"type": "string"}, {"type": "null"}]}, "attach": {
        "properties": {"z": answer, "a": answer}, "required": ["z"], "additionalProperties": false
    }});
    let mut d = definition(source);
    d["interface"]["provides"] = json!({"inbox": chat});
    d["interface"]["requires"] = json!({"to": chat});
    Lang0.check(&d, &Contents::default()).unwrap();
}

#[test]
fn imported_aliases_and_qualified_type_names_resolve_without_evaluation() {
    let mut read = Contents::default();
    let code = read.code(&[(
        "types.py",
        "import typing as t\nT = t.TypedDict('T', {'value': t.Literal[1, 2]})\n",
    )]);
    let mut d = definition("import types as t\ndef main() -> t.T:\n    return {}\n");
    d["body"]["modules"] = json!(code);
    d["interface"]["signature"]["out"] = json!({"type": "object", "properties": {"value": {"enum": [1, 2]}}, "required": ["value"], "additionalProperties": false});
    Lang0.check(&d, &read).unwrap();
}

#[test]
fn interface_comparison_uses_canonical_numbers_not_json_representation() {
    let mut d = definition("from typing import Literal\ndef main() -> Literal[1]:\n    return 1\n");
    d["interface"]["signature"]["out"] = json!({"enum": [1.0]});
    Lang0.check(&d, &Contents::default()).unwrap();
}

#[test]
fn annotations_require_unique_top_level_bindings_and_cannot_hide_cycles() {
    for (source, expected) in [
        ("N = None\nN = None\ndef main() -> N:\n    pass\n", "唯一"),
        (
            "if True:\n    N = None\ndef main() -> N:\n    pass\n",
            "模块顶层",
        ),
        ("A = B\nB = A\ndef main() -> A:\n    pass\n", "类型别名"),
        ("def main():\n    pass\n", "返回值须标类型"),
        ("def main(x) -> None:\n    pass\n", "参数须标类型"),
        (
            "def main() -> dict:\n    pass\n",
            "用 `dict[str, Any]` 或 TypedDict",
        ),
        (
            "def main() -> list:\n    pass\n",
            "用 `dict[str, Any]` 或 TypedDict",
        ),
        (
            "from typing import Callable\ndef main(f: Callable) -> None:\n    pass\n",
            "捕获",
        ),
        (
            "from croon import Schema\nT = Schema({'typo': 'string'})\ndef main() -> T:\n    pass\n",
            "typo",
        ),
        ("def main() -> dict[int, str]:\n    pass\n", "dict[str, T]"),
    ] {
        let error = Lang0
            .check(&definition(source), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn top_level_port_invocation_has_documented_diagnostic() {
    let mut read = Contents::default();
    let code = read.code(&[("ports.py", "from croon import Port\nExec = Port(str)\n")]);
    let mut d =
        definition("from ports import Exec\nexec = Exec()\ndef main() -> None:\n    pass\n");
    d["body"]["modules"] = json!(code);
    let error = Lang0.check(&d, &read).unwrap_err().to_string();
    assert!(
        error.ends_with("引用值写在 `main` 的参数里，工具在外层函数里捕获它"),
        "{error}"
    );
}

#[test]
fn only_imports_and_direct_type_declarations_are_followed() {
    for (head, annotation) in [
        ("TD = TypedDict\n", "TD('T', {'x': str})"),
        ("Seq = list\n", "Seq[str]"),
        ("import croon\nC = croon\n", "C.Schema({'type': 'null'})"),
        ("Out = TypedDict('Out', {})\nOut2 = Out\n", "Out2"),
    ] {
        for function in ["main", "helper"] {
            let body = format!(
                "from typing import TypedDict\n{head}def {function}() -> {annotation}:\n    pass\n"
            );
            let body = if function == "main" {
                body
            } else {
                format!("{body}def main() -> None:\n    pass\n")
            };
            let error = Lang0
                .check(&definition(&body), &Contents::default())
                .unwrap_err()
                .to_string();
            assert!(error.contains("首版不收类型别名"), "{error}");
        }
    }
}

#[test]
fn explicit_schema_literals_accept_numbers_and_reject_value_overflow() {
    let mut d = definition(
        "from croon import Schema\nOut = Schema({'enum': [-1, 2.5, None, True]})\ndef main() -> Out:\n    return None\n",
    );
    d["interface"]["signature"]["out"] = json!({"enum": [-1, 2.5, null, true]});
    Lang0.check(&d, &Contents::default()).unwrap();
    let d = definition(
        "from typing import Literal\ndef main() -> Literal[9999999999999999999999999999999999999]:\n    return None\n",
    );
    assert!(Lang0.check(&d, &Contents::default()).is_err());
    // body 内的整数不在 Value 边界；构造不能把 Python int 收窄。
    Lang0
        .check(
            &definition("def main() -> None:\n    x = 9999999999999999999999999999999999999\n"),
            &Contents::default(),
        )
        .unwrap();
}

#[test]
fn specification_literals_reject_duplicate_keys_even_when_unused() {
    for (expression, message) in [
        (
            "TypedDict('T', {'x': str, 'x': int})",
            "TypedDict 的字段名重复",
        ),
        (
            "Schema({'type': 'null', 'type': 'null'})",
            "Schema 或 Port spec 的字面量键重复",
        ),
        (
            "Schema({'type': 'object', 'properties': {'x': {}, 'x': {}}})",
            "Schema 或 Port spec 的字面量键重复",
        ),
        (
            "Port({'description': '', 'in': {}, 'attach': {'additionalProperties': False}, 'description': ''})",
            "Schema 或 Port spec 的字面量键重复",
        ),
        (
            "Port(str, attach={'p': Port(str), 'p': Port(int)})",
            "attach 的名字重复",
        ),
    ] {
        let body = format!(
            "from typing import TypedDict\nfrom croon import Schema, Port\nT = {expression}\ndef main() -> None:\n    pass\n"
        );
        let error = Lang0
            .check(&definition(&body), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(error.ends_with(message), "{error}");
    }
    Lang0
        .check(
            &definition("def main() -> None:\n    x = {'a': 1, 'a': 2}\n"),
            &Contents::default(),
        )
        .unwrap();
}
